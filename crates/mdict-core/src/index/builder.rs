use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::index::format::{FORMAT_VERSION, HEADER_SIZE, IndexHeader, RECORD_SIZE};
use crate::util::unix_millis;

const ZSTD_LEVEL: i32 = 3;
const COMPRESS_MIN_LEN: usize = 64;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub entries: usize,
    pub raw_bytes: u64,
    pub stored_bytes: u64,
    pub index_bytes: u64,
    pub file_bytes: u64,
}

struct Pending {
    sort_key: String,
    key: String,
    blob_off: u64,
    blob_len: u32,
    raw_len: u32,
    flags: u32,
}

/// Streams headwords and compressed definitions into temporary files, then
/// writes the final sorted index.
pub struct IndexBuilder {
    tmp_blob_path: PathBuf,
    writer: BufWriter<File>,
    blobs_len: u64,
    raw_bytes: u64,
    pending: Vec<Pending>,
    source_size: u64,
    source_mtime: i64,
}

impl IndexBuilder {
    pub fn new(tmp_dir: &Path, source_size: u64, source_mtime: i64) -> Result<Self> {
        fs::create_dir_all(tmp_dir)?;
        let tmp_blob_path = tmp_dir.join(format!(
            "blobs-{}-{}.tmp",
            std::process::id(),
            fastrand::u64(..)
        ));
        let file = File::create(&tmp_blob_path)?;
        Ok(Self {
            tmp_blob_path,
            writer: BufWriter::with_capacity(1024 * 1024, file),
            blobs_len: 0,
            raw_bytes: 0,
            pending: Vec::new(),
            source_size,
            source_mtime,
        })
    }

    pub fn add(&mut self, key: &str, definition: &[u8]) -> Result<()> {
        let key = key.trim().trim_start_matches('\u{feff}').trim();
        if key.is_empty() || key.as_bytes().contains(&0) {
            return Ok(());
        }
        if definition.len() > u32::MAX as usize {
            return Err(Error::index(format!("definition for {key:?} is too large")));
        }
        let raw_len = definition.len();
        let (stored, flags) = if raw_len >= COMPRESS_MIN_LEN {
            match zstd::bulk::compress(definition, ZSTD_LEVEL) {
                Ok(compressed) if compressed.len() + 16 < raw_len => (compressed, 1u32),
                _ => (definition.to_vec(), 0u32),
            }
        } else {
            (definition.to_vec(), 0u32)
        };
        if stored.len() > u32::MAX as usize {
            return Err(Error::index("compressed definition is too large"));
        }

        let blob_off = self.blobs_len;
        self.writer.write_all(&stored)?;
        self.blobs_len += stored.len() as u64;
        self.raw_bytes += raw_len as u64;

        self.pending.push(Pending {
            sort_key: key.to_lowercase(),
            key: key.to_string(),
            blob_off,
            blob_len: stored.len() as u32,
            raw_len: raw_len as u32,
            flags,
        });
        Ok(())
    }

    pub fn entry_count(&self) -> usize {
        self.pending.len()
    }

    pub fn finish(mut self, out_path: &Path) -> Result<IndexStats> {
        self.writer.flush()?;
        self.pending
            .sort_by(|a, b| a.sort_key.cmp(&b.sort_key).then_with(|| a.key.cmp(&b.key)));

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let entry_count = self.pending.len();
        let index_len = (entry_count as u64)
            .checked_mul(RECORD_SIZE as u64)
            .ok_or_else(|| Error::index("index length overflow"))?;
        let index_off = HEADER_SIZE as u64;
        let keys_off = index_off + index_len;

        let mut keys = Vec::new();
        let mut key_meta = Vec::with_capacity(entry_count);
        for pending in &self.pending {
            let offset = keys.len() as u64;
            keys.extend_from_slice(pending.key.as_bytes());
            key_meta.push((offset, pending.key.len() as u32));
        }
        let blobs_off = keys_off + keys.len() as u64;

        let tmp_out = out_path.with_extension("idx.tmp");
        let result = self.write_final(
            out_path, &tmp_out, &keys, &key_meta, index_off, index_len, keys_off, blobs_off,
        );
        if result.is_err() {
            let _ = fs::remove_file(&tmp_out);
        }
        let stats = result?;

        Ok(stats)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_final(
        &mut self,
        out_path: &Path,
        tmp_out: &Path,
        keys: &[u8],
        key_meta: &[(u64, u32)],
        index_off: u64,
        index_len: u64,
        keys_off: u64,
        blobs_off: u64,
    ) -> Result<IndexStats> {
        let file = File::create(tmp_out)?;
        let mut out = BufWriter::with_capacity(1024 * 1024, file);
        out.write_all(&[0u8; HEADER_SIZE])?;

        for (pending, (key_off, key_len)) in self.pending.iter().zip(key_meta.iter()) {
            let mut record = [0u8; RECORD_SIZE];
            record[0..8].copy_from_slice(&key_off.to_le_bytes());
            record[8..12].copy_from_slice(&key_len.to_le_bytes());
            record[12..20].copy_from_slice(&pending.blob_off.to_le_bytes());
            record[20..24].copy_from_slice(&pending.blob_len.to_le_bytes());
            record[24..28].copy_from_slice(&pending.raw_len.to_le_bytes());
            record[28..32].copy_from_slice(&pending.flags.to_le_bytes());
            out.write_all(&record)?;
        }
        out.write_all(keys)?;

        let mut blob_in = File::open(&self.tmp_blob_path)?;
        let copied = std::io::copy(&mut blob_in, &mut out)?;
        if copied != self.blobs_len {
            return Err(Error::index(
                "temporary blob file changed while writing index",
            ));
        }
        out.flush()?;
        let mut file = out
            .into_inner()
            .map_err(|_| Error::other("failed to flush index writer"))?;

        let header = IndexHeader {
            version: FORMAT_VERSION,
            entry_count: self.pending.len() as u64,
            index_off,
            index_len,
            keys_off,
            keys_len: keys.len() as u64,
            blobs_off,
            blobs_len: self.blobs_len,
            source_size: self.source_size,
            source_mtime: self.source_mtime,
            created_at: unix_millis(),
        };
        let mut header_buf = [0u8; HEADER_SIZE];
        header.encode(&mut header_buf);
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header_buf)?;
        file.sync_all()?;
        drop(file);

        replace_file(tmp_out, out_path)?;

        Ok(IndexStats {
            entries: self.pending.len(),
            raw_bytes: self.raw_bytes,
            stored_bytes: self.blobs_len,
            index_bytes: index_len + keys.len() as u64,
            file_bytes: blobs_off + self.blobs_len,
        })
    }
}

impl Drop for IndexBuilder {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.tmp_blob_path);
    }
}

/// Rename with retries, which matters on Windows where the destination may
/// still be memory-mapped by a query for a few milliseconds.
fn replace_file(tmp: &Path, target: &Path) -> Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match fs::rename(tmp, target) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Err(Error::Io(last_error.unwrap_or_else(|| {
        std::io::Error::other("failed to replace index file")
    })))
}
