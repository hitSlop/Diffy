//! One coalescing writer: disk work is serialized and never runs on the UI thread.
use super::{SavedState, save};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
};

struct Queue {
    pending: Option<(u64, SavedState)>,
    requested: u64,
    completed: u64,
    result: Result<(), String>,
    closed: bool,
}
impl Default for Queue {
    fn default() -> Self {
        Self {
            pending: None,
            requested: 0,
            completed: 0,
            result: Ok(()),
            closed: false,
        }
    }
}
struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}
pub struct Writer {
    shared: Arc<Shared>,
    events: async_channel::Receiver<()>,
}
impl Writer {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                result: Ok(()),
                ..Default::default()
            }),
            ready: Condvar::new(),
        });
        let (tx, events) = async_channel::bounded(1);
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("diffy-preferences".into())
            .spawn(move || {
                loop {
                    let (sequence, state) = {
                        let mut queue = worker.queue.lock().unwrap_or_else(|e| e.into_inner());
                        while queue.pending.is_none() && !queue.closed {
                            queue = worker.ready.wait(queue).unwrap_or_else(|e| e.into_inner());
                        }
                        let Some(job) = queue.pending.take() else {
                            break;
                        };
                        job
                    };
                    let _span = tracing::info_span!("save_preferences").entered();
                    let result = save(&path, &state);
                    let mut queue = worker.queue.lock().unwrap_or_else(|e| e.into_inner());
                    queue.completed = sequence;
                    queue.result = result.clone();
                    worker.ready.notify_all();
                    let _ = tx.try_send(());
                }
            })
            .map_err(|e| format!("Couldn't start preference writer: {e}"))?;
        Ok(Self { shared, events })
    }
    pub fn enqueue(&self, state: SavedState) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.requested += 1;
        queue.pending = Some((queue.requested, state));
        self.shared.ready.notify_all();
    }
    pub fn events(&self) -> async_channel::Receiver<()> {
        self.events.clone()
    }
    pub fn result(&self) -> Result<(), String> {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .result
            .clone()
    }
    /// Obtain a flush handle to move to a background executor before waiting.
    pub fn flush_handle(&self) -> Flush {
        Flush(self.shared.clone())
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.closed = true;
        self.shared.ready.notify_all();
    }
}
pub struct Flush(Arc<Shared>);
impl Flush {
    pub fn wait(self) -> Result<(), String> {
        let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
        while queue.completed < queue.requested {
            queue = self.0.ready.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
        queue.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newest_save_wins_and_flush_observes_errors() {
        let root = std::env::temp_dir().join(format!(
            "diffy-writer-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let path = root.join("state.json");
        let writer = Writer::new(path.clone()).unwrap();
        for width in 240..460 {
            writer.enqueue(SavedState {
                sidebar_width: Some(width as f32),
                ..Default::default()
            });
        }
        writer.flush_handle().wait().unwrap();
        assert_eq!(super::super::load(&path).unwrap().sidebar_width, Some(459.));
        let bad = Writer::new(path.join("state.json")).unwrap();
        bad.enqueue(SavedState::default());
        assert!(bad.flush_handle().wait().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
