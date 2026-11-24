mod commands;
mod indexer;
mod protocol;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

pub struct AppState {
    pub store: Arc<mdict_core::DictionaryStore>,
    pub indexer: Arc<indexer::IndexManager>,
    pub resource_base: String,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("moedict".to_string()),
                    },
                ))
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .register_uri_scheme_protocol("mdict", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            commands::list_dictionaries,
            commands::query,
            commands::suggest,
            commands::lucky,
            commands::get_settings,
            commands::set_settings,
            commands::add_dictionary_paths,
            commands::pick_dictionary_files,
            commands::pick_dictionary_folder,
            commands::remove_dictionary,
            commands::set_dictionary_enabled,
            commands::rename_dictionary,
            commands::set_dictionary_tags,
            commands::reorder_dictionaries,
            commands::reindex_dictionary,
            commands::reindex_all,
            commands::cancel_index,
            commands::open_dictionary_folder,
            commands::open_data_dir,
            commands::open_external,
            commands::app_info,
        ])
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            log::info!(
                "MoeDict starting; config={}, data={}",
                config_dir.display(),
                data_dir.display()
            );

            let store = Arc::new(mdict_core::DictionaryStore::new(&config_dir, &data_dir)?);
            let indexer = Arc::new(indexer::IndexManager::new(
                app.handle().clone(),
                store.clone(),
            ));
            let resource_base = if cfg!(windows) {
                "http://mdict.localhost".to_string()
            } else {
                "mdict://localhost".to_string()
            };

            let _ = app.manage(AppState {
                store: store.clone(),
                indexer: indexer.clone(),
                resource_base,
            });

            if let Some(dir) = dev_mdx_dir() {
                match store.add_paths(&[dir.clone()]) {
                    Ok(outcome) => {
                        log::info!(
                            "dev: added {} dictionaries from {}",
                            outcome.added.len(),
                            dir.display()
                        );
                        let ids: Vec<String> = outcome
                            .added
                            .iter()
                            .filter(|dict| {
                                matches!(
                                    dict.status,
                                    mdict_core::DictStatus::Unindexed
                                        | mdict_core::DictStatus::Stale
                                )
                            })
                            .map(|dict| dict.id.clone())
                            .collect();
                        indexer.enqueue(ids);
                    }
                    Err(error) => log::warn!("dev: failed to add {}: {error}", dir.display()),
                }
            }

            if store.settings().auto_index {
                indexer.enqueue_stale();
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running MoeDict");
}

/// `--mdx-dir <path>` (or `MOEDICT_MDX_DIR`) adds a folder at startup.  This is
/// handy for development against the bundled sample dictionaries.
fn dev_mdx_dir() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--mdx-dir" {
            return args.next().map(PathBuf::from);
        }
        if let Some(value) = arg.strip_prefix("--mdx-dir=") {
            return Some(PathBuf::from(value));
        }
    }
    std::env::var_os("MOEDICT_MDX_DIR").map(PathBuf::from)
}
