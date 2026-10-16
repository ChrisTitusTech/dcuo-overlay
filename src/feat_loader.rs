//! One bounded background completion reader; stale results cannot cross character selections.
use crate::feat_progress::{self, Progress};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};

struct Job {
    generation: u64,
    id: String,
    cancel: Arc<AtomicBool>,
}
type Reply = (u64, Result<Progress, String>);

pub struct Loader {
    jobs: SyncSender<Job>,
    replies: Receiver<Reply>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    selected: Option<String>,
    pending: Option<Job>,
    pub progress: Option<Progress>,
    pub status: String,
}
impl Default for Loader {
    fn default() -> Self {
        let (jobs, requests) = mpsc::sync_channel::<Job>(1);
        let (results, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(job) = requests.recv() {
                let result = feat_progress::fetch(&job.id, &job.cancel);
                if results.send((job.generation, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            replies,
            cancel: Arc::new(AtomicBool::new(false)),
            generation: 0,
            selected: None,
            pending: None,
            progress: None,
            status: "Look up a character to check completed feats.".into(),
        }
    }
}
impl Loader {
    pub fn select(&mut self, id: Option<&str>) {
        if self.selected.as_deref() != id {
            self.selected = id.map(str::to_owned);
            self.refresh();
        }
    }
    pub fn refresh(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        self.progress = None;
        self.pending = self.selected.as_ref().map(|id| Job {
            generation: self.generation,
            id: id.clone(),
            cancel: self.cancel.clone(),
        });
        self.status = if self.pending.is_some() {
            "Checking completed feats..."
        } else {
            "Look up a character to check completed feats."
        }
        .into();
    }
    fn accept(&mut self, generation: u64, result: Result<Progress, String>) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(progress) if Some(progress.character_id.as_str()) == self.selected.as_deref() => {
                self.status =
                    "Completion checked with Daybreak Census; source freshness unknown.".into();
                self.progress = Some(progress);
            }
            Ok(_) => {
                self.progress = None;
                self.status = "Character changed; completion unknown.".into();
            }
            Err(error) => {
                self.progress = None;
                self.status = error;
            }
        }
    }
    pub fn poll(&mut self) {
        while let Ok((generation, result)) = self.replies.try_recv() {
            self.accept(generation, result);
        }
        if let Some(job) = self.pending.take() {
            match self.jobs.try_send(job) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(job)) => self.pending = Some(job),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.status = "Feat reader unavailable.".into()
                }
            }
        }
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_character_discards_old_results_and_cancels_work() {
        let mut loader = Loader::default();
        loader.select(Some("1"));
        let old = loader.generation;
        let token = loader.cancel.clone();
        loader.select(Some("2"));
        assert!(token.load(Ordering::Relaxed));
        loader.accept(
            old,
            Ok(Progress {
                character_id: "1".into(),
                completed: Default::default(),
                received_at: std::time::Instant::now(),
            }),
        );
        assert!(loader.progress.is_none());
        let current = loader.generation;
        loader.accept(
            current,
            Ok(Progress {
                character_id: "2".into(),
                completed: Default::default(),
                received_at: std::time::Instant::now(),
            }),
        );
        assert!(loader.progress.is_some());
        loader.refresh();
        assert!(loader.progress.is_none());
        loader.accept(current, Err("late".into()));
        assert_eq!(loader.status, "Checking completed feats...");
        loader.select(None);
        assert!(loader.pending.is_none());
    }
}
