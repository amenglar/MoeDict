use std::path::PathBuf;

use mdict_core::{
    AddOutcome, DictStatus, DictionaryInfo, DictionaryStore, QueryHit, Settings, Suggestion,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::indexer::IndexManager;

fn stringify<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn empty_outcome() -> AddOutcome {
    AddOutcome {
        added: Vec::new(),
        skipped: Vec::new(),
    }
}

fn add_paths_blocking(
    store: &DictionaryStore,
    indexer: &IndexManager,
    paths: Vec<PathBuf>,
) -> Result<AddOutcome, String> {
    let outcome = store.add_paths(&paths).map_err(stringify)?;
    let ids: Vec<String> = outcome
        .added
        .iter()
        .filter(|dict| matches!(dict.status, DictStatus::Unindexed | DictStatus::Stale))
        .map(|dict| dict.id.clone())
        .collect();
    indexer.enqueue(ids);
    Ok(outcome)
}

#[tauri::command]
pub fn list_dictionaries(state: State<'_, AppState>) -> Vec<DictionaryInfo> {
    state.store.list()
}

#[tauri::command]
pub fn query(state: State<'_, AppState>, word: String, tags: Option<Vec<String>>) -> Vec<QueryHit> {
    state
        .store
        .query_with_tags(&word, tags.as_deref().unwrap_or(&[]))
}

#[tauri::command]
pub fn query_nth(
    state: State<'_, AppState>,
    word: String,
    nth: usize,
    dict_id: Option<String>,
    tags: Option<Vec<String>>,
) -> Option<QueryHit> {
    state.store.query_nth(
        &word,
        nth,
        dict_id.as_deref(),
        tags.as_deref().unwrap_or(&[]),
    )
}

#[tauri::command]
pub fn suggest(
    state: State<'_, AppState>,
    prefix: String,
    tags: Option<Vec<String>>,
) -> Vec<Suggestion> {
    state
        .store
        .suggest_with_tags(&prefix, tags.as_deref().unwrap_or(&[]))
}

#[tauri::command]
pub fn lucky(state: State<'_, AppState>, tags: Option<Vec<String>>) -> Option<String> {
    state.store.lucky_with_tags(tags.as_deref().unwrap_or(&[]))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.store.settings()
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    state.store.update_settings(settings).map_err(stringify)
}

#[tauri::command]
pub async fn add_dictionary_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<AddOutcome, String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        return Ok(empty_outcome());
    }
    let store = state.store.clone();
    let indexer = state.indexer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        add_paths_blocking(store.as_ref(), indexer.as_ref(), paths)
    })
    .await
    .map_err(stringify)?
}

#[tauri::command]
pub async fn pick_dictionary_files(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AddOutcome, String> {
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("MDX 词典", &["mdx"])
            .blocking_pick_files()
    })
    .await
    .map_err(stringify)?;

    let paths: Vec<PathBuf> = picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file| file.into_path().ok())
        .collect();
    if paths.is_empty() {
        return Ok(empty_outcome());
    }

    let store = state.store.clone();
    let indexer = state.indexer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        add_paths_blocking(store.as_ref(), indexer.as_ref(), paths)
    })
    .await
    .map_err(stringify)?
}

#[tauri::command]
pub async fn pick_dictionary_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AddOutcome, String> {
    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("选择包含 MDX 词典的文件夹")
            .blocking_pick_folder()
    })
    .await
    .map_err(stringify)?;

    let Some(folder) = picked.and_then(|file| file.into_path().ok()) else {
        return Ok(empty_outcome());
    };

    let store = state.store.clone();
    let indexer = state.indexer.clone();
    tauri::async_runtime::spawn_blocking(move || {
        add_paths_blocking(store.as_ref(), indexer.as_ref(), vec![folder])
    })
    .await
    .map_err(stringify)?
}

#[tauri::command]
pub fn remove_dictionary(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.indexer.cancel(&id);
    state.store.remove(&id).map_err(stringify)
}

#[tauri::command]
pub fn set_dictionary_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state.store.set_enabled(&id, enabled).map_err(stringify)
}

#[tauri::command]
pub fn rename_dictionary(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    state.store.rename(&id, &name).map_err(stringify)
}

#[tauri::command]
pub fn set_dictionary_tags(
    state: State<'_, AppState>,
    id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    state.store.set_tags(&id, &tags).map_err(stringify)
}

#[tauri::command]
pub fn reorder_dictionaries(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    state.store.reorder(&ids).map_err(stringify)
}

#[tauri::command]
pub fn reindex_dictionary(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.indexer.enqueue(vec![id]);
    Ok(())
}

#[tauri::command]
pub fn reindex_all(state: State<'_, AppState>) -> Result<(), String> {
    state.indexer.enqueue_all();
    Ok(())
}

#[tauri::command]
pub fn cancel_index(state: State<'_, AppState>, id: String) {
    state.indexer.cancel(&id);
}

#[tauri::command]
pub fn open_dictionary_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let dir = state.store.dictionary_dir(&id).map_err(stringify)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(stringify)
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dir = state
        .store
        .config_path()
        .parent()
        .ok_or_else(|| "找不到配置目录".to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(stringify)
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"]
        .iter()
        .any(|prefix| lower.starts_with(*prefix));
    if !allowed {
        return Err("不支持的链接协议".to_string());
    }
    app.opener()
        .open_url(url.to_string(), None::<&str>)
        .map_err(stringify)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub config_path: String,
    pub index_dir: String,
    pub log_dir: String,
    pub resource_base: String,
    pub platform: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    let log_dir = app
        .path()
        .app_log_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    AppInfo {
        version: app.package_info().version.to_string(),
        config_path: state.store.config_path().display().to_string(),
        index_dir: state.store.index_dir().display().to_string(),
        log_dir,
        resource_base: state.resource_base.clone(),
        platform: std::env::consts::OS.to_string(),
    }
}
