use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use mdict_core::{DictStatus, DictionaryStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub id: String,
    pub name: String,
    pub phase: String,
    pub current: u64,
    pub total: u64,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoneEvent {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub cancelled: bool,
    pub error: Option<String>,
}

struct Inner {
    app: AppHandle,
    store: Arc<DictionaryStore>,
    sender: Mutex<mpsc::Sender<String>>,
    pending: Mutex<HashSet<String>>,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// A single background worker that indexes one dictionary at a time.  One at a
/// time keeps peak memory bounded and avoids competing for disk I/O.
pub struct IndexManager {
    inner: Arc<Inner>,
}

impl IndexManager {
    pub fn new(app: AppHandle, store: Arc<DictionaryStore>) -> Self {
        let (sender, receiver) = mpsc::channel::<String>();
        let inner = Arc::new(Inner {
            app,
            store,
            sender: Mutex::new(sender),
            pending: Mutex::new(HashSet::new()),
            cancels: Mutex::new(HashMap::new()),
        });
        let worker = inner.clone();
        std::thread::Builder::new()
            .name("mdict-indexer".to_string())
            .spawn(move || worker.run(receiver))
            .expect("failed to spawn index worker");
        Self { inner }
    }

    pub fn enqueue(&self, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        let mut pending = self.inner.pending.lock().unwrap();
        let sender = self.inner.sender.lock().unwrap();
        for id in ids {
            if id.is_empty() {
                continue;
            }
            if pending.insert(id.clone()) && sender.send(id).is_err() {
                break;
            }
        }
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.inner.cancels.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn enqueue_stale(&self) {
        let ids: Vec<String> = self
            .inner
            .store
            .list()
            .into_iter()
            .filter(|dict| {
                dict.enabled && matches!(dict.status, DictStatus::Unindexed | DictStatus::Stale)
            })
            .map(|dict| dict.id)
            .collect();
        self.enqueue(ids);
    }

    pub fn enqueue_all(&self) {
        let ids: Vec<String> = self
            .inner
            .store
            .list()
            .into_iter()
            .filter(|dict| !matches!(dict.status, DictStatus::Missing))
            .map(|dict| dict.id)
            .collect();
        self.enqueue(ids);
    }
}

impl Inner {
    fn run(self: Arc<Self>, receiver: mpsc::Receiver<String>) {
        while let Ok(id) = receiver.recv() {
            self.pending.lock().unwrap().remove(&id);
            let name = self
                .store
                .lookup(&id)
                .map(|dict| dict.name)
                .unwrap_or_else(|_| id.clone());

            let cancel = Arc::new(AtomicBool::new(false));
            self.cancels
                .lock()
                .unwrap()
                .insert(id.clone(), cancel.clone());

            let app = self.app.clone();
            let store = self.store.clone();
            let event_id = id.clone();
            let event_name = name.clone();
            let mut last_emit = Instant::now() - Duration::from_secs(60);

            let started = Instant::now();
            let result = store.build_index(&id, |progress| {
                if cancel.load(Ordering::Relaxed) {
                    return false;
                }
                let now = Instant::now();
                if now.duration_since(last_emit) >= Duration::from_millis(80)
                    || progress.percent >= 99.9
                {
                    last_emit = now;
                    let _ = app.emit(
                        "index-progress",
                        ProgressEvent {
                            id: event_id.clone(),
                            name: event_name.clone(),
                            phase: progress.phase.to_string(),
                            current: progress.current,
                            total: progress.total,
                            percent: progress.percent,
                        },
                    );
                }
                true
            });

            self.cancels.lock().unwrap().remove(&id);
            log::info!(
                "index job {} ({}) finished in {:?}: {:?}",
                id,
                name,
                started.elapsed(),
                result.as_ref().map(|stats| stats.entries)
            );

            let cancelled = matches!(&result, Err(mdict_core::Error::Cancelled));
            let (ok, error) = match result {
                Ok(_) => (true, None),
                Err(error) => (false, Some(error.to_string())),
            };
            let _ = self.app.emit(
                "index-done",
                DoneEvent {
                    id: id.clone(),
                    name,
                    ok,
                    cancelled,
                    error,
                },
            );
            let _ = self.app.emit("dict-changed", ());
        }
    }
}
