use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::mdict::Mdict;
use crate::util::normalize_resource_path;

/// Random-access reader for the resources of one MDD file.
pub struct MddReader {
    path: PathBuf,
    mdict: Mdict,
    map: HashMap<String, usize>,
    ends: Vec<u64>,
}

impl MddReader {
    pub fn open(path: &Path) -> Result<Self> {
        let mdict = Mdict::open(path)?;
        if !mdict.is_mdd() {
            return Err(Error::format(format!(
                "{} is not an MDD file",
                path.display()
            )));
        }
        let entries = mdict.entries();
        let total = mdict.total_decompressed_size();
        let mut ends = vec![0u64; entries.len()];
        let mut map = HashMap::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let next = entries
                .get(index + 1)
                .map(|next| next.offset)
                .unwrap_or(total);
            let block_end = mdict.block_end_for(entry.offset)?;
            ends[index] = next.min(block_end).min(total);
            map.entry(normalize_resource_path(&entry.text))
                .or_insert(index);
        }
        Ok(Self {
            path: path.to_path_buf(),
            mdict,
            map,
            ends,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains(&self, resource: &str) -> bool {
        self.map.contains_key(&normalize_resource_path(resource))
    }

    pub fn read(&self, resource: &str) -> Result<Option<Vec<u8>>> {
        let key = normalize_resource_path(resource);
        let Some(&index) = self.map.get(&key) else {
            return Ok(None);
        };
        let start = self.mdict.entries()[index].offset;
        let end = self.ends[index];
        if end <= start {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(self.mdict.read_range(start, end)?))
    }
}

/// One or more split MDD files belonging to the same dictionary.
pub struct MddSet {
    readers: Vec<MddReader>,
}

impl MddSet {
    /// Open all MDD files in `paths`.  Files that fail to parse are skipped so
    /// one broken split file does not make every resource unavailable.
    pub fn open(paths: &[PathBuf]) -> Result<Self> {
        let mut readers = Vec::new();
        for path in paths {
            match MddReader::open(path) {
                Ok(reader) => readers.push(reader),
                Err(error) => log::warn!("skip MDD {}: {error}", path.display()),
            }
        }
        if readers.is_empty() {
            return Err(Error::NotFound("没有可用的 MDD 文件".to_string()));
        }
        Ok(Self { readers })
    }

    pub fn len(&self) -> usize {
        self.readers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }

    pub fn read(&self, resource: &str) -> Result<Option<Vec<u8>>> {
        for reader in &self.readers {
            if let Some(bytes) = reader.read(resource)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }
}
