use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::audio;
use crate::error::{Error, Result};
use crate::index::{
    FORMAT_VERSION, IndexBuilder, IndexFile, IndexProgress, IndexStats, read_index_header,
};
use crate::mdd::MddSet;
use crate::mdict::{Mdict, read_title};
use crate::util::{clean_resource_path, display_path, hash_id, path_is_within, unix_millis};

const CONFIG_VERSION: u32 = 1;
const INDEX_CACHE_SIZE: usize = 8;
const MDD_CACHE_SIZE: usize = 2;
const MAX_SCAN_DEPTH: usize = 8;
const MAX_SCAN_FILES: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_font_size")]
    pub font_size: u16,
    #[serde(default = "default_true")]
    pub auto_index: bool,
    #[serde(default = "default_suggestion_limit")]
    pub suggestion_limit: usize,
    #[serde(default)]
    pub show_all_dicts: bool,
    #[serde(default = "default_default_expanded")]
    pub default_expanded: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            font_size: default_font_size(),
            auto_index: true,
            suggestion_limit: default_suggestion_limit(),
            show_all_dicts: false,
            default_expanded: default_default_expanded(),
        }
    }
}

fn default_theme() -> String {
    "system".to_string()
}
fn default_font_size() -> u16 {
    16
}
fn default_true() -> bool {
    true
}
fn default_suggestion_limit() -> usize {
    12
}
fn default_default_expanded() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryConfig {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub source_size: u64,
    #[serde(default)]
    pub source_mtime: i64,
    #[serde(default)]
    pub entry_count: u64,
    #[serde(default)]
    pub indexed_at: Option<i64>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DictStatus {
    Ready,
    Stale,
    Unindexed,
    Missing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub order: i64,
    pub status: DictStatus,
    pub entry_count: u64,
    pub source_size: u64,
    pub index_size: u64,
    pub indexed_at: Option<i64>,
    pub error: Option<String>,
    pub has_mdd: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHit {
    pub dict_id: String,
    pub dict_name: String,
    pub headword: String,
    pub html: String,
    pub order: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub word: String,
    pub dicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPath {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOutcome {
    pub added: Vec<DictionaryInfo>,
    pub skipped: Vec<SkippedPath>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    #[serde(default = "default_config_version")]
    version: u32,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    dictionaries: Vec<DictionaryConfig>,
}

fn default_config_version() -> u32 {
    CONFIG_VERSION
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            dictionaries: Vec::new(),
        }
    }
}

struct Inner {
    settings: Settings,
    dicts: Vec<DictionaryConfig>,
}

struct LruCache<T> {
    map: HashMap<String, Arc<T>>,
    order: VecDeque<String>,
    max: usize,
}

impl<T> LruCache<T> {
    fn new(max: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max: max.max(1),
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<T>> {
        let value = self.map.get(key)?.clone();
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(value)
    }

    fn insert(&mut self, key: String, value: Arc<T>) {
        if self.map.contains_key(&key) {
            self.order.retain(|k| k != &key);
        } else {
            while self.map.len() >= self.max {
                match self.order.pop_front() {
                    Some(old) => {
                        self.map.remove(&old);
                    }
                    None => break,
                }
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    fn remove(&mut self, key: &str) {
        self.map.remove(key);
        self.order.retain(|k| k != key);
    }
}

/// Owns the dictionary configuration, indexes and open file handles.
///
/// All methods are synchronous and safe to call from a background thread.
pub struct DictionaryStore {
    config_path: PathBuf,
    index_dir: PathBuf,
    tmp_dir: PathBuf,
    inner: RwLock<Inner>,
    index_cache: Mutex<LruCache<IndexFile>>,
    mdd_cache: Mutex<LruCache<MddSet>>,
}

impl DictionaryStore {
    pub fn new(config_dir: &Path, data_dir: &Path) -> Result<Self> {
        let index_dir = data_dir.join("index");
        fs::create_dir_all(&index_dir)?;
        let tmp_dir = index_dir.join(".tmp");
        fs::create_dir_all(&tmp_dir)?;
        fs::create_dir_all(config_dir)?;
        cleanup_stale_temp_files(&index_dir, &tmp_dir);
        let config_path = config_dir.join("config.json");

        let mut config = if config_path.exists() {
            let bytes = fs::read(&config_path)?;
            match serde_json::from_slice::<ConfigFile>(&bytes) {
                Ok(config) => config,
                Err(error) => {
                    log::warn!("config.json is invalid ({error}); starting with defaults");
                    let backup = config_path.with_extension("json.bak");
                    let _ = fs::copy(&config_path, backup);
                    ConfigFile::default()
                }
            }
        } else {
            ConfigFile::default()
        };

        normalize_orders(&mut config.dictionaries);

        Ok(Self {
            config_path,
            index_dir,
            tmp_dir,
            inner: RwLock::new(Inner {
                settings: config.settings,
                dicts: config.dictionaries,
            }),
            index_cache: Mutex::new(LruCache::new(INDEX_CACHE_SIZE)),
            mdd_cache: Mutex::new(LruCache::new(MDD_CACHE_SIZE)),
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn index_dir(&self) -> &Path {
        &self.index_dir
    }

    pub fn settings(&self) -> Settings {
        self.inner.read().unwrap().settings.clone()
    }

    pub fn update_settings(&self, settings: Settings) -> Result<Settings> {
        let mut settings = settings;
        if !matches!(settings.theme.as_str(), "system" | "light" | "dark") {
            settings.theme = "system".to_string();
        }
        settings.font_size = settings.font_size.clamp(10, 32);
        settings.suggestion_limit = settings.suggestion_limit.clamp(1, 50);
        settings.default_expanded = settings.default_expanded.clamp(0, 20);
        let mut inner = self.inner.write().unwrap();
        inner.settings = settings.clone();
        self.save_locked(&inner)?;
        Ok(settings)
    }

    pub fn list(&self) -> Vec<DictionaryInfo> {
        let dicts = {
            let inner = self.inner.read().unwrap();
            let mut dicts = inner.dicts.clone();
            dicts.sort_by_key(|d| d.order);
            dicts
        };
        dicts.iter().map(|cfg| self.info_for(cfg)).collect()
    }

    pub fn lookup(&self, id: &str) -> Result<DictionaryConfig> {
        let inner = self.inner.read().unwrap();
        inner
            .dicts
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("词典 {id}")))
    }

    /// Add `.mdx` files and/or directories (scanned recursively).
    pub fn add_paths(&self, paths: &[PathBuf]) -> Result<AddOutcome> {
        let mut files = Vec::new();
        let mut skipped = Vec::new();

        for path in paths {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() => {
                    let mut found = Vec::new();
                    collect_mdx(path, &mut found, 0);
                    if found.is_empty() {
                        skipped.push(SkippedPath {
                            path: display_path(path),
                            reason: "目录中没有找到 .mdx 文件".to_string(),
                        });
                    }
                    files.extend(found);
                }
                Ok(_) if is_mdx(path) => files.push(path.clone()),
                Ok(_) => skipped.push(SkippedPath {
                    path: display_path(path),
                    reason: "不是 .mdx 文件".to_string(),
                }),
                Err(error) => skipped.push(SkippedPath {
                    path: display_path(path),
                    reason: error.to_string(),
                }),
            }
        }

        let mut canonical = Vec::new();
        let mut seen = HashSet::new();
        for file in files {
            match fs::canonicalize(&file) {
                Ok(path) => {
                    if seen.insert(path.clone()) {
                        canonical.push(path);
                    }
                }
                Err(error) => skipped.push(SkippedPath {
                    path: display_path(&file),
                    reason: error.to_string(),
                }),
            }
        }

        let mut added = Vec::new();
        {
            let mut inner = self.inner.write().unwrap();
            let mut existing: HashSet<PathBuf> =
                inner.dicts.iter().map(|d| d.path.clone()).collect();
            let mut next_order = inner.dicts.iter().map(|d| d.order).max().unwrap_or(-1) + 1;

            for path in canonical {
                if existing.contains(&path) {
                    skipped.push(SkippedPath {
                        path: display_path(&path),
                        reason: "词典已添加".to_string(),
                    });
                    continue;
                }
                let metadata = match fs::metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        skipped.push(SkippedPath {
                            path: display_path(&path),
                            reason: error.to_string(),
                        });
                        continue;
                    }
                };
                let source_size = metadata.len();
                let source_mtime = mtime_millis(&metadata);
                let id = hash_id(&display_path(&path));
                let name = read_title(&path)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| file_stem(&path));

                let (entry_count, indexed_at) = match read_index_header(&self.index_path(&id)) {
                    Ok(header)
                        if header.version == FORMAT_VERSION
                            && header.source_size == source_size
                            && header.source_mtime == source_mtime =>
                    {
                        (header.entry_count, Some(header.created_at))
                    }
                    _ => (0, None),
                };

                let cfg = DictionaryConfig {
                    id,
                    name,
                    path: path.clone(),
                    enabled: true,
                    order: next_order,
                    source_size,
                    source_mtime,
                    entry_count,
                    indexed_at,
                    last_error: None,
                    tags: Vec::new(),
                };
                next_order += 1;
                existing.insert(path);
                added.push(cfg.clone());
                inner.dicts.push(cfg);
            }

            if !added.is_empty() {
                self.save_locked(&inner)?;
            }
        }

        let added = added.iter().map(|cfg| self.info_for(cfg)).collect();
        Ok(AddOutcome { added, skipped })
    }

    pub fn add_directory(&self, dir: &Path) -> Result<AddOutcome> {
        self.add_paths(&[dir.to_path_buf()])
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        let position = inner
            .dicts
            .iter()
            .position(|d| d.id == id)
            .ok_or_else(|| Error::NotFound(format!("词典 {id}")))?;
        inner.dicts.remove(position);
        normalize_orders(&mut inner.dicts);
        self.save_locked(&inner)?;
        drop(inner);

        self.index_cache.lock().unwrap().remove(id);
        self.mdd_cache.lock().unwrap().remove(id);
        if is_valid_id(id) {
            let _ = fs::remove_file(self.index_path(id));
        }
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        let dict = inner
            .dicts
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::NotFound(format!("词典 {id}")))?;
        dict.enabled = enabled;
        self.save_locked(&inner)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::other("词典名称不能为空"));
        }
        let name: String = name.chars().take(80).collect();
        let mut inner = self.inner.write().unwrap();
        let dict = inner
            .dicts
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::NotFound(format!("词典 {id}")))?;
        dict.name = name;
        self.save_locked(&inner)
    }

    pub fn set_tags(&self, id: &str, tags: &[String]) -> Result<()> {
        let mut unique: Vec<String> = Vec::new();
        for tag in tags {
            let tag: String = tag.trim().chars().take(24).collect();
            if !tag.is_empty() && !unique.contains(&tag) {
                unique.push(tag);
            }
        }
        unique.truncate(8);
        let mut inner = self.inner.write().unwrap();
        let dict = inner
            .dicts
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or_else(|| Error::NotFound(format!("词典 {id}")))?;
        dict.tags = unique;
        self.save_locked(&inner)
    }

    pub fn all_tags(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        let mut tags: Vec<String> = inner
            .dicts
            .iter()
            .flat_map(|dict| dict.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    pub fn reorder(&self, ids: &[String]) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        let positions: HashMap<&str, usize> = ids
            .iter()
            .enumerate()
            .map(|(index, id)| (id.as_str(), index))
            .collect();
        inner
            .dicts
            .sort_by_key(|d| positions.get(d.id.as_str()).copied().unwrap_or(usize::MAX));
        for (index, dict) in inner.dicts.iter_mut().enumerate() {
            dict.order = index as i64;
        }
        self.save_locked(&inner)
    }

    pub fn dictionary_dir(&self, id: &str) -> Result<PathBuf> {
        let cfg = self.lookup(id)?;
        cfg.path
            .parent()
            .map(|path| path.to_path_buf())
            .ok_or_else(|| Error::NotFound(format!("词典目录 {id}")))
    }

    pub fn open_index(&self, id: &str) -> Result<Arc<IndexFile>> {
        if !is_valid_id(id) {
            return Err(Error::NotFound("无效的词典 id".to_string()));
        }
        if let Some(cached) = self.index_cache.lock().unwrap().get(id) {
            return Ok(cached);
        }
        let configured = {
            let inner = self.inner.read().unwrap();
            inner.dicts.iter().any(|d| d.id == id)
        };
        if !configured {
            return Err(Error::NotFound(format!("词典 {id}")));
        }
        let index = Arc::new(IndexFile::open(&self.index_path(id))?);
        self.index_cache
            .lock()
            .unwrap()
            .insert(id.to_string(), index.clone());
        Ok(index)
    }

    pub fn query(&self, word: &str) -> Vec<QueryHit> {
        self.query_with_tags(word, &[])
    }

    pub fn query_with_tags(&self, word: &str, tags: &[String]) -> Vec<QueryHit> {
        let word = word.trim();
        if word.is_empty() {
            return Vec::new();
        }
        let dicts = self.enabled_dicts_for(tags);
        let mut hits = Vec::new();
        for cfg in dicts {
            match self.open_index(&cfg.id).and_then(|index| index.exact(word)) {
                Ok(Some((headword, html))) => hits.push(QueryHit {
                    dict_id: cfg.id.clone(),
                    dict_name: cfg.name.clone(),
                    headword,
                    html,
                    order: cfg.order,
                }),
                Ok(None) => {}
                Err(error) => log::debug!("query '{}' in {} failed: {error}", word, cfg.name),
            }
        }
        hits
    }

    /// Look up the nth homonym (duplicate headword) in one dictionary.
    pub fn query_nth(
        &self,
        word: &str,
        nth: usize,
        dict_id: Option<&str>,
        tags: &[String],
    ) -> Option<QueryHit> {
        let word = word.trim();
        if word.is_empty() {
            return None;
        }
        for cfg in self.enabled_dicts_for(tags) {
            if dict_id.is_some_and(|id| id != cfg.id) {
                continue;
            }
            if let Ok(Some((headword, html))) = self
                .open_index(&cfg.id)
                .and_then(|index| index.exact_nth(word, nth))
            {
                return Some(QueryHit {
                    dict_id: cfg.id.clone(),
                    dict_name: cfg.name.clone(),
                    headword,
                    html,
                    order: cfg.order,
                });
            }
        }
        None
    }

    pub fn suggest(&self, prefix: &str) -> Vec<Suggestion> {
        self.suggest_with_tags(prefix, &[])
    }

    pub fn suggest_with_tags(&self, prefix: &str, tags: &[String]) -> Vec<Suggestion> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Vec::new();
        }
        let dicts = self.enabled_dicts_for(tags);
        let limit = self
            .inner
            .read()
            .unwrap()
            .settings
            .suggestion_limit
            .clamp(1, 50);

        let per_dict = 8;
        let mut out: Vec<Suggestion> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        for cfg in dicts {
            let Ok(index) = self.open_index(&cfg.id) else {
                continue;
            };
            let Ok(words) = index.prefix(prefix, per_dict) else {
                continue;
            };
            for word in words {
                let key = word.to_lowercase();
                if let Some(&position) = positions.get(&key) {
                    out[position].dicts.push(cfg.name.clone());
                } else {
                    positions.insert(key, out.len());
                    out.push(Suggestion {
                        word,
                        dicts: vec![cfg.name.clone()],
                    });
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    pub fn lucky(&self) -> Option<String> {
        self.lucky_with_tags(&[])
    }

    pub fn lucky_with_tags(&self, tags: &[String]) -> Option<String> {
        let dicts = self.enabled_dicts_for(tags);
        if dicts.is_empty() {
            return None;
        }
        for _ in 0..8 {
            let cfg = &dicts[fastrand::usize(..dicts.len())];
            let Ok(index) = self.open_index(&cfg.id) else {
                continue;
            };
            let Ok(Some(key)) = index.random_key() else {
                continue;
            };
            if plausible_headword(&key) {
                return Some(key);
            }
        }
        None
    }

    /// Build (or rebuild) one dictionary index.
    ///
    /// `progress` returns `false` to cancel.
    pub fn build_index<F>(&self, id: &str, mut progress: F) -> Result<IndexStats>
    where
        F: FnMut(IndexProgress) -> bool,
    {
        let cfg = self.lookup(id)?;
        let result = (|| -> Result<IndexStats> {
            let metadata = fs::metadata(&cfg.path)?;
            let source_size = metadata.len();
            let source_mtime = mtime_millis(&metadata);
            let mdict = Mdict::open(&cfg.path)?;
            let total = mdict.entry_count() as u64;

            if !progress(IndexProgress::new("parsing", 0, total)) {
                return Err(Error::Cancelled);
            }

            let mut builder = IndexBuilder::new(&self.tmp_dir, source_size, source_mtime)?;
            let mut processed = 0u64;
            mdict.for_each_entry(|key, definition| {
                builder.add(key, definition)?;
                processed += 1;
                if processed % 256 == 0
                    && !progress(IndexProgress::new("parsing", processed, total))
                {
                    return Err(Error::Cancelled);
                }
                Ok(())
            })?;

            let after = fs::metadata(&cfg.path)?;
            if after.len() != source_size || mtime_millis(&after) != source_mtime {
                return Err(Error::other("源词典在索引过程中发生变化，请重试"));
            }
            if !progress(IndexProgress::new("writing", total, total)) {
                return Err(Error::Cancelled);
            }

            // Drop any mapped view before the atomic replace (notably on Windows).
            self.index_cache.lock().unwrap().remove(&cfg.id);
            let stats = builder.finish(&self.index_path(&cfg.id))?;

            let mut inner = self.inner.write().unwrap();
            if let Some(dict) = inner.dicts.iter_mut().find(|d| d.id == cfg.id) {
                dict.source_size = source_size;
                dict.source_mtime = source_mtime;
                dict.entry_count = stats.entries as u64;
                dict.indexed_at = Some(unix_millis());
                dict.last_error = None;
            }
            self.save_locked(&inner)?;
            drop(inner);
            self.index_cache.lock().unwrap().remove(&cfg.id);
            Ok(stats)
        })();

        if let Err(error) = &result {
            if !matches!(error, Error::Cancelled) {
                log::warn!("indexing {} failed: {error}", cfg.name);
                let mut inner = self.inner.write().unwrap();
                if let Some(dict) = inner.dicts.iter_mut().find(|d| d.id == cfg.id) {
                    dict.last_error = Some(error.to_string());
                    let _ = self.save_locked(&inner);
                }
            }
        }
        result
    }

    /// Read a CSS/image/audio resource belonging to a dictionary.  Files next
    /// to the MDX take precedence over resources inside the MDD.
    pub fn read_resource(
        &self,
        id: &str,
        raw_path: &str,
    ) -> Result<Option<(Vec<u8>, &'static str)>> {
        let cfg = self.lookup(id)?;
        let canonical_source = fs::canonicalize(&cfg.path)?;
        let dict_dir = canonical_source
            .parent()
            .ok_or_else(|| Error::NotFound("词典目录".to_string()))?
            .to_path_buf();

        let original = clean_resource_path(raw_path);
        let normalized = original.to_lowercase();
        if normalized.is_empty() {
            return Ok(None);
        }

        // Files next to the MDX are looked up case-sensitively first (Linux),
        // then with the lower-cased MDD spelling as a fallback.
        let mut file_variants = resource_variants(&original);
        file_variants.extend(resource_variants(&normalized));
        file_variants.dedup();
        for variant in &file_variants {
            let candidate = dict_dir.join(variant);
            if let Ok(canonical) = fs::canonicalize(&candidate) {
                if canonical.is_file() && path_is_within(&dict_dir, &canonical) {
                    let bytes = fs::read(&canonical)?;
                    return Ok(Some(audio::playable(bytes, variant)));
                }
            }
        }

        let mdd_paths = self.find_mdd_paths(&cfg);
        if mdd_paths.is_empty() {
            return Ok(None);
        }
        let set = self.open_mdd(&cfg.id, &mdd_paths)?;
        for variant in resource_variants(&normalized) {
            if let Some(bytes) = set.read(&variant)? {
                return Ok(Some(audio::playable(bytes, &variant)));
            }
        }
        Ok(None)
    }

    fn enabled_dicts(&self) -> Vec<DictionaryConfig> {
        let inner = self.inner.read().unwrap();
        let mut dicts: Vec<DictionaryConfig> =
            inner.dicts.iter().filter(|d| d.enabled).cloned().collect();
        dicts.sort_by_key(|d| d.order);
        dicts
    }

    fn enabled_dicts_for(&self, tags: &[String]) -> Vec<DictionaryConfig> {
        let mut dicts = self.enabled_dicts();
        if !tags.is_empty() {
            dicts.retain(|dict| {
                tags.iter()
                    .any(|tag| dict.tags.iter().any(|item| item == tag))
            });
        }
        dicts
    }

    fn info_for(&self, cfg: &DictionaryConfig) -> DictionaryInfo {
        let index_path = self.index_path(&cfg.id);
        let index_size = fs::metadata(&index_path).map(|m| m.len()).unwrap_or(0);
        let (status, entry_count, indexed_at) = self.status_for(cfg, &index_path);
        let source_size = fs::metadata(&cfg.path)
            .map(|m| m.len())
            .unwrap_or(cfg.source_size);
        DictionaryInfo {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            path: display_path(&cfg.path),
            enabled: cfg.enabled,
            order: cfg.order,
            status,
            entry_count,
            source_size,
            index_size,
            indexed_at,
            error: cfg.last_error.clone(),
            has_mdd: !self.find_mdd_paths(cfg).is_empty(),
            tags: cfg.tags.clone(),
        }
    }

    fn status_for(
        &self,
        cfg: &DictionaryConfig,
        index_path: &Path,
    ) -> (DictStatus, u64, Option<i64>) {
        let Ok(metadata) = fs::metadata(&cfg.path) else {
            return (DictStatus::Missing, cfg.entry_count, cfg.indexed_at);
        };
        match read_index_header(index_path) {
            Ok(header) => {
                let stale = header.version != FORMAT_VERSION
                    || header.source_size != metadata.len()
                    || header.source_mtime != mtime_millis(&metadata);
                let status = if stale {
                    DictStatus::Stale
                } else {
                    DictStatus::Ready
                };
                (status, header.entry_count, Some(header.created_at))
            }
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                (DictStatus::Unindexed, cfg.entry_count, cfg.indexed_at)
            }
            Err(_) => (DictStatus::Error, cfg.entry_count, cfg.indexed_at),
        }
    }

    fn find_mdd_paths(&self, cfg: &DictionaryConfig) -> Vec<PathBuf> {
        let Some(dir) = cfg.path.parent() else {
            return Vec::new();
        };
        let stem = cfg
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let prefix = format!("{stem}.");
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut exact = None;
        let mut split = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_mdd = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("mdd"))
                .unwrap_or(false);
            if !is_mdd {
                continue;
            }
            let file_stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if file_stem == stem {
                exact = Some(path);
            } else if file_stem.starts_with(&prefix) {
                split.push(path);
            }
        }
        split.sort();
        let mut out = Vec::new();
        if let Some(path) = exact {
            out.push(path);
        }
        out.extend(split);
        out
    }

    fn open_mdd(&self, id: &str, paths: &[PathBuf]) -> Result<Arc<MddSet>> {
        if let Some(cached) = self.mdd_cache.lock().unwrap().get(id) {
            return Ok(cached);
        }
        let set = Arc::new(MddSet::open(paths)?);
        self.mdd_cache
            .lock()
            .unwrap()
            .insert(id.to_string(), set.clone());
        Ok(set)
    }

    fn index_path(&self, id: &str) -> PathBuf {
        self.index_dir.join(format!("{id}.idx"))
    }

    fn save_locked(&self, inner: &Inner) -> Result<()> {
        let file = ConfigFile {
            version: CONFIG_VERSION,
            settings: inner.settings.clone(),
            dictionaries: inner.dicts.clone(),
        };
        let json = serde_json::to_vec_pretty(&file)
            .map_err(|error| Error::other(format!("无法序列化配置: {error}")))?;
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.config_path.with_extension("json.tmp");
        {
            let mut out = File::create(&tmp)?;
            out.write_all(&json)?;
            out.sync_all()?;
        }
        replace_with_retry(&tmp, &self.config_path)
    }
}

/// Remove leftovers from a previous run that was interrupted while writing an
/// index.  Only files older than a few hours are touched.
fn cleanup_stale_temp_files(index_dir: &Path, tmp_dir: &Path) {
    let cutoff = Duration::from_secs(6 * 3600);
    let now = SystemTime::now();
    let clean = |path: &Path| {
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        let Ok(modified) = metadata.modified() else {
            return;
        };
        let Ok(age) = now.duration_since(modified) else {
            return;
        };
        if age > cutoff {
            let _ = fs::remove_file(path);
        }
    };
    if let Ok(entries) = fs::read_dir(tmp_dir) {
        for entry in entries.flatten() {
            clean(&entry.path());
        }
    }
    if let Ok(entries) = fs::read_dir(index_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .map(|name| name.to_string_lossy().ends_with(".idx.tmp"))
                .unwrap_or(false)
            {
                clean(&path);
            }
        }
    }
}

fn normalize_orders(dicts: &mut [DictionaryConfig]) {
    dicts.sort_by_key(|d| d.order);
    for (index, dict) in dicts.iter_mut().enumerate() {
        dict.order = index as i64;
    }
}

fn is_mdx(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mdx"))
        .unwrap_or(false)
}

fn is_valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "未命名词典".to_string())
}

fn mtime_millis(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn collect_mdx(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_SCAN_DEPTH || out.len() >= MAX_SCAN_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let hidden = path
                .file_name()
                .map(|name| name.to_string_lossy().starts_with('.'))
                .unwrap_or(false);
            if !hidden {
                collect_mdx(&path, out, depth + 1);
            }
        } else if is_mdx(&path) {
            out.push(path);
            if out.len() >= MAX_SCAN_FILES {
                return;
            }
        }
    }
}

fn resource_variants(normalized: &str) -> Vec<String> {
    let basename = normalized
        .rsplit('/')
        .next()
        .unwrap_or(normalized)
        .to_string();
    let safe: String = normalized
        .split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .collect::<Vec<_>>()
        .join("/");
    let mut out = vec![normalized.to_string(), safe];
    if basename != normalized {
        out.push(basename);
    }
    out.dedup();
    out
}

fn plausible_headword(word: &str) -> bool {
    let word = word.trim();
    if word.is_empty() || word.chars().count() > 40 {
        return false;
    }
    if word.chars().any(|c| c.is_control()) {
        return false;
    }
    word.chars()
        .any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

fn replace_with_retry(tmp: &Path, target: &Path) -> Result<()> {
    let mut last_error = None;
    for _ in 0..10 {
        match fs::rename(tmp, target) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
    Err(Error::Io(last_error.unwrap_or_else(|| {
        std::io::Error::other("failed to replace config file")
    })))
}
