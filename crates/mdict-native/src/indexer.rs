use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use mdict_core::{DictionaryStore, Error, IndexProgress};

#[derive(Clone, Debug)]
pub struct ProgressInfo {
    pub id: String,
    pub name: String,
    pub phase: String,
    pub percent: f32,
}

#[derive(Clone, Debug)]
pub enum IndexEvent {
    Progress(ProgressInfo),
    Done {
        id: String,
        name: String,
        ok: bool,
        cancelled: bool,
        error: Option<String>,
    },
}

struct Job {
    id: String,
    name: String,
    cancelled: Arc<AtomicBool>,
}

pub struct IndexManager {
    jobs: Sender<Job>,
    events: Mutex<Receiver<IndexEvent>>,
    flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl IndexManager {
    pub fn new(store: Arc<DictionaryStore>) -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (events_tx, events_rx) = mpsc::channel::<IndexEvent>();
        thread::Builder::new()
            .name("mdict-indexer".into())
            .spawn(move || worker(store, jobs_rx, events_tx))
            .expect("spawn index worker");
        Self {
            jobs: jobs_tx,
            events: Mutex::new(events_rx),
            flags: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn enqueue(&self, id: String, name: String) {
        let flag = Arc::new(AtomicBool::new(false));
        self.flags.lock().unwrap().insert(id.clone(), flag.clone());
        let _ = self.jobs.send(Job {
            id,
            name,
            cancelled: flag,
        });
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.flags.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn cancel_all(&self) {
        for flag in self.flags.lock().unwrap().values() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn poll(&self) -> Vec<IndexEvent> {
        let mut out = Vec::new();
        let rx = self.events.lock().unwrap();
        while let Ok(event) = rx.try_recv() {
            if let IndexEvent::Done { id, .. } = &event {
                self.flags.lock().unwrap().remove(id);
            }
            out.push(event);
        }
        out
    }
}

fn worker(store: Arc<DictionaryStore>, jobs: Receiver<Job>, events: Sender<IndexEvent>) {
    while let Ok(job) = jobs.recv() {
        let mut last_emit = Instant::now() - Duration::from_secs(60);
        let cancelled = job.cancelled.clone();
        let id = job.id.clone();
        let name = job.name.clone();
        let result = store.build_index(&id, |progress: IndexProgress| {
            if cancelled.load(Ordering::Relaxed) {
                return false;
            }
            let now = Instant::now();
            if now.duration_since(last_emit) >= Duration::from_millis(80)
                || progress.percent >= 99.9
            {
                last_emit = now;
                let _ = events.send(IndexEvent::Progress(ProgressInfo {
                    id: id.clone(),
                    name: name.clone(),
                    phase: progress.phase.to_string(),
                    percent: progress.percent,
                }));
            }
            true
        });
        let (ok, cancelled_result, error) = match result {
            Ok(_) => (true, false, None),
            Err(Error::Cancelled) => (false, true, None),
            Err(e) => (false, false, Some(e.to_string())),
        };
        let _ = events.send(IndexEvent::Done {
            id,
            name,
            ok,
            cancelled: cancelled_result,
            error,
        });
    }
}
