use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    Pending,
    Downloading,
    Inspecting,
    Loading,
    Validating,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEvent {
    pub stage: JobStage,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

pub struct JobState {
    pub id: Uuid,
    pub stage: JobStage,
    pub log: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub tx: broadcast::Sender<SseEvent>,
}

pub type JobStore = Arc<DashMap<Uuid, Arc<Mutex<JobState>>>>;

pub fn new_store() -> JobStore {
    Arc::new(DashMap::new())
}

pub fn create_job(store: &JobStore) -> (Uuid, broadcast::Receiver<SseEvent>) {
    let id = Uuid::new_v4();
    let (tx, rx) = broadcast::channel(128);
    let state = Arc::new(Mutex::new(JobState {
        id,
        stage: JobStage::Pending,
        log: Vec::new(),
        created_at: Utc::now(),
        tx,
    }));
    store.insert(id, state);
    (id, rx)
}

pub async fn emit(store: &JobStore, id: Uuid, event: SseEvent) {
    if let Some(entry) = store.get(&id) {
        let mut job = entry.lock().await;
        job.stage = event.stage.clone();
        job.log.push(event.message.clone());
        let _ = job.tx.send(event); // ignore if no listeners yet
    }
}

/// Spawn a background task that cleans up jobs older than 1 hour.
pub fn spawn_cleanup(store: JobStore) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
            let cutoff = Utc::now() - chrono::Duration::hours(1);
            store.retain(|_, v| {
                // Try to get the created_at without blocking; keep if locked
                if let Ok(job) = v.try_lock() {
                    job.created_at > cutoff
                } else {
                    true
                }
            });
        }
    });
}
