use std::fs::File;
use std::io::Read;
use std::path::Path;

use memmap2::Mmap;

use crate::error::{Error, Result};

pub const MAGIC: &[u8; 8] = b"MOEDCT01";
pub const FORMAT_VERSION: u32 = 1;
pub const HEADER_SIZE: usize = 128;
pub const RECORD_SIZE: usize = 32;
const FLAG_COMPRESSED: u32 = 1;
const MAX_ENTRIES: u64 = 100_000_000;
const MAX_DEF_SIZE: u32 = 256 * 1024 * 1024;
/// How many `@@@LINK=` hops are followed before giving up (guards against
/// redirect cycles in malformed dictionaries).
const MAX_REDIRECT_DEPTH: usize = 8;
const REDIRECT_PREFIX: &str = "@@@LINK=";

#[derive(Debug, Clone)]
pub struct IndexHeader {
    pub version: u32,
    pub entry_count: u64,
    pub index_off: u64,
    pub index_len: u64,
    pub keys_off: u64,
    pub keys_len: u64,
    pub blobs_off: u64,
    pub blobs_len: u64,
    pub source_size: u64,
    pub source_mtime: i64,
    pub created_at: i64,
}

impl IndexHeader {
    pub fn encode(&self, buf: &mut [u8; HEADER_SIZE]) {
        buf.fill(0);
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..12].copy_from_slice(&self.version.to_le_bytes());
        buf[12..16].copy_from_slice(&0u32.to_le_bytes());
        buf[16..24].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[24..32].copy_from_slice(&self.index_off.to_le_bytes());
        buf[32..40].copy_from_slice(&self.index_len.to_le_bytes());
        buf[40..48].copy_from_slice(&self.keys_off.to_le_bytes());
        buf[48..56].copy_from_slice(&self.keys_len.to_le_bytes());
        buf[56..64].copy_from_slice(&self.blobs_off.to_le_bytes());
        buf[64..72].copy_from_slice(&self.blobs_len.to_le_bytes());
        buf[72..80].copy_from_slice(&self.source_size.to_le_bytes());
        buf[80..88].copy_from_slice(&self.source_mtime.to_le_bytes());
        buf[88..96].copy_from_slice(&self.created_at.to_le_bytes());
    }

    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < HEADER_SIZE {
            return Err(Error::index("index header is truncated"));
        }
        if &buf[0..8] != MAGIC {
            return Err(Error::index("not a MoeDict index file"));
        }
        let read_u32 = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let read_u64 = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().unwrap());
        let read_i64 = |o: usize| i64::from_le_bytes(buf[o..o + 8].try_into().unwrap());
        Ok(Self {
            version: read_u32(8),
            entry_count: read_u64(16),
            index_off: read_u64(24),
            index_len: read_u64(32),
            keys_off: read_u64(40),
            keys_len: read_u64(48),
            blobs_off: read_u64(56),
            blobs_len: read_u64(64),
            source_size: read_u64(72),
            source_mtime: read_i64(80),
            created_at: read_i64(88),
        })
    }
}

pub fn read_index_header(path: &Path) -> Result<IndexHeader> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; HEADER_SIZE];
    file.read_exact(&mut buf)?;
    IndexHeader::decode(&buf)
}

/// Extracts the target of an `@@@LINK=target` entry.
///
/// Some converters wrap the marker in a paragraph tag, and targets may carry a
/// trailing fragment or extra lines; all of those are tolerated.
fn redirect_target(definition: &str) -> Option<String> {
    let text = definition.trim().trim_start_matches('\u{feff}').trim();
    let text = text
        .strip_prefix("<p>")
        .and_then(|inner| inner.strip_suffix("</p>"))
        .map(str::trim)
        .unwrap_or(text);
    let prefix = text.get(..REDIRECT_PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(REDIRECT_PREFIX) {
        return None;
    }
    let target = text[REDIRECT_PREFIX.len()..]
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    let target = target.split('#').next().unwrap_or("").trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// A read-only, memory-mapped dictionary index.
pub struct IndexFile {
    map: Mmap,
    header: IndexHeader,
}

impl IndexFile {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        // SAFETY: the index is written to a temporary file and atomically
        // replaced; it is never modified in place while mapped.
        let map = unsafe { Mmap::map(&file)? };
        if map.len() < HEADER_SIZE {
            return Err(Error::index(format!(
                "index file is truncated: {} bytes",
                map.len()
            )));
        }
        let header = IndexHeader::decode(&map[..HEADER_SIZE])?;
        validate_header(&header, map.len())?;
        Ok(Self { map, header })
    }

    pub fn header(&self) -> &IndexHeader {
        &self.header
    }

    pub fn len(&self) -> usize {
        self.header.entry_count as usize
    }

    pub fn is_empty(&self) -> bool {
        self.header.entry_count == 0
    }

    pub fn source_size(&self) -> u64 {
        self.header.source_size
    }

    pub fn source_mtime(&self) -> i64 {
        self.header.source_mtime
    }

    pub fn key(&self, index: usize) -> Result<&str> {
        if index >= self.len() {
            return Err(Error::index("entry index out of range"));
        }
        let record_offset = self.header.index_off as usize + index * RECORD_SIZE;
        let record = self
            .map
            .get(record_offset..record_offset + RECORD_SIZE)
            .ok_or_else(|| Error::index("index record out of bounds"))?;
        let key_off = u64::from_le_bytes(record[0..8].try_into().unwrap());
        let key_len = u32::from_le_bytes(record[8..12].try_into().unwrap()) as usize;
        let start = self.header.keys_off as usize + key_off as usize;
        let end = start
            .checked_add(key_len)
            .ok_or_else(|| Error::index("key length overflow"))?;
        let bytes = self
            .map
            .get(start..end)
            .ok_or_else(|| Error::index("key out of bounds"))?;
        std::str::from_utf8(bytes).map_err(|_| Error::index("index contains invalid UTF-8 key"))
    }

    /// Case-insensitive exact lookup.  Prefers a case-exact match when several
    /// keys differ only by case.
    ///
    /// MDX dictionaries store forward-only entries as `@@@LINK=target`; these
    /// are followed here so callers always receive real article HTML.  The
    /// returned headword stays the key the user actually matched.
    pub fn exact(&self, word: &str) -> Result<Option<(String, String)>> {
        let Some(index) = self.find_index(word)? else {
            return Ok(None);
        };
        self.resolve_redirects(index).map(Some)
    }

    /// Case-insensitive exact lookup of the nth duplicate key (0-based).
    /// Dictionaries such as Oxford store homonyms under the same headword.
    pub fn exact_nth(&self, word: &str, nth: usize) -> Result<Option<(String, String)>> {
        let Some(index) = self.find_index_nth(word, nth)? else {
            return Ok(None);
        };
        self.resolve_redirects(index).map(Some)
    }

    fn resolve_redirects(&self, index: usize) -> Result<(String, String)> {
        let headword = self.key(index)?.to_string();
        let mut html = self.definition(index)?;
        let mut visited = vec![index];
        for _ in 0..MAX_REDIRECT_DEPTH {
            let Some(target) = redirect_target(&html) else {
                break;
            };
            let Some(next) = self.find_index(&target)? else {
                break;
            };
            // Entries are identified by position: case variants such as
            // "buch" -> "Buch" are distinct entries, not a cycle.
            if visited.contains(&next) {
                break;
            }
            visited.push(next);
            html = self.definition(next)?;
        }
        Ok((headword, html))
    }

    fn find_index(&self, word: &str) -> Result<Option<usize>> {
        let word = word.trim().trim_start_matches('\u{feff}').trim();
        if word.is_empty() || self.is_empty() {
            return Ok(None);
        }
        let lower = word.to_lowercase();
        let mut index = self.lower_bound(&lower)?;
        let mut fallback = None;
        while index < self.len() {
            let key = self.key(index)?;
            if key.to_lowercase() != lower {
                break;
            }
            if key == word {
                return Ok(Some(index));
            }
            if fallback.is_none() {
                fallback = Some(index);
            }
            index += 1;
        }
        Ok(fallback)
    }

    fn find_index_nth(&self, word: &str, nth: usize) -> Result<Option<usize>> {
        let word = word.trim().trim_start_matches('\u{feff}').trim();
        if word.is_empty() || self.is_empty() {
            return Ok(None);
        }
        let lower = word.to_lowercase();
        let mut index = self.lower_bound(&lower)?;
        let mut exact = Vec::new();
        let mut loose = Vec::new();
        while index < self.len() && exact.len() + loose.len() < 256 {
            let key = self.key(index)?;
            if key.to_lowercase() != lower {
                break;
            }
            if key == word {
                exact.push(index);
            } else {
                loose.push(index);
            }
            index += 1;
        }
        let matches = if exact.is_empty() { loose } else { exact };
        Ok(matches.get(nth).copied())
    }

    pub fn prefix(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let prefix = prefix.trim();
        if prefix.is_empty() || limit == 0 || self.is_empty() {
            return Ok(Vec::new());
        }
        let lower = prefix.to_lowercase();
        let mut index = self.lower_bound(&lower)?;
        let mut out = Vec::with_capacity(limit);
        let mut previous: Option<String> = None;
        let mut scanned = 0usize;
        while index < self.len() && out.len() < limit && scanned < 10_000 {
            let key = self.key(index)?;
            let key_lower = key.to_lowercase();
            if !key_lower.starts_with(&lower) {
                break;
            }
            if previous.as_deref() != Some(key_lower.as_str()) {
                out.push(key.to_string());
                previous = Some(key_lower);
            }
            index += 1;
            scanned += 1;
        }
        Ok(out)
    }

    pub fn definition(&self, index: usize) -> Result<String> {
        let record_offset = self.header.index_off as usize + index * RECORD_SIZE;
        let record = self
            .map
            .get(record_offset..record_offset + RECORD_SIZE)
            .ok_or_else(|| Error::index("index record out of bounds"))?;
        let def_off = u64::from_le_bytes(record[12..20].try_into().unwrap());
        let def_len = u32::from_le_bytes(record[20..24].try_into().unwrap()) as usize;
        let raw_len = u32::from_le_bytes(record[24..28].try_into().unwrap());
        let flags = u32::from_le_bytes(record[28..32].try_into().unwrap());
        if raw_len > MAX_DEF_SIZE {
            return Err(Error::index("definition is too large"));
        }
        let start = self.header.blobs_off as usize + def_off as usize;
        let end = start
            .checked_add(def_len)
            .ok_or_else(|| Error::index("definition length overflow"))?;
        let blob = self
            .map
            .get(start..end)
            .ok_or_else(|| Error::index("definition out of bounds"))?;
        let raw = if flags & FLAG_COMPRESSED != 0 {
            zstd::bulk::decompress(blob, raw_len as usize)
                .map_err(|e| Error::index(format!("zstd decompression failed: {e}")))?
        } else {
            blob.to_vec()
        };
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    pub fn random_key(&self) -> Result<Option<String>> {
        if self.is_empty() {
            return Ok(None);
        }
        let index = fastrand::usize(..self.len());
        Ok(Some(self.key(index)?.to_string()))
    }

    pub fn random(&self) -> Result<Option<(String, String)>> {
        if self.is_empty() {
            return Ok(None);
        }
        let index = fastrand::usize(..self.len());
        Ok(Some((
            self.key(index)?.to_string(),
            self.definition(index)?,
        )))
    }

    fn lower_bound(&self, lower: &str) -> Result<usize> {
        let mut lo = 0usize;
        let mut hi = self.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.key(mid)?.to_lowercase().as_str() < lower {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }
}

fn validate_header(header: &IndexHeader, file_len: usize) -> Result<()> {
    if header.version != FORMAT_VERSION {
        return Err(Error::index(format!(
            "unsupported index version {} (expected {})",
            header.version, FORMAT_VERSION
        )));
    }
    if header.entry_count > MAX_ENTRIES {
        return Err(Error::index("index declares too many entries"));
    }
    let expected_index_len = header
        .entry_count
        .checked_mul(RECORD_SIZE as u64)
        .ok_or_else(|| Error::index("index length overflow"))?;
    if header.index_len != expected_index_len {
        return Err(Error::index("index table length mismatch"));
    }
    if header.index_off < HEADER_SIZE as u64
        || header.keys_off < header.index_off + header.index_len
        || header.blobs_off < header.keys_off + header.keys_len
        || header.blobs_off + header.blobs_len > file_len as u64
    {
        return Err(Error::index("index section offsets are invalid"));
    }
    Ok(())
}
