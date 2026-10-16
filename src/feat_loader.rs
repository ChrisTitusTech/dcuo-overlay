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
            self.progress = None;
            self.refresh();
        }
    }
    pub fn refresh(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        // A refresh replaces transport work, not proven same-character facts.
        if self.progress.as_ref().is_some_and(|progress| {
            Some(progress.character_id.as_str()) != self.selected.as_deref()
        }) {
            self.progress = None;
        }
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
            Ok(mut progress)
                if Some(progress.character_id.as_str()) == self.selected.as_deref() =>
            {
                if let Some(previous) = self
                    .progress
                    .as_ref()
                    .filter(|previous| previous.character_id == progress.character_id)
                {
                    progress
                        .completed
                        .extend(previous.completed.iter().cloned());
                }
                self.status =
                    "Completion checked with Daybreak Census; source freshness unknown.".into();
                self.progress = Some(progress);
            }
            Ok(_) => {
                self.status = "Ignored completion response for another character.".into();
            }
            Err(error) => {
                self.status = if self.progress.is_some() {
                    format!("{error} Showing the last completion result.")
                } else {
                    error
                };
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
        assert_eq!(loader.progress.as_ref().unwrap().character_id, "2");
        loader.accept(current, Err("late".into()));
        assert_eq!(loader.status, "Checking completed feats...");
        loader.select(None);
        assert!(loader.pending.is_none());
        assert!(loader.progress.is_none());
    }
    fn progress(id: &str, completed: &[&str], received_at: std::time::Instant) -> Progress {
        Progress {
            character_id: id.into(),
            completed: completed.iter().map(|id| (*id).into()).collect(),
            received_at,
        }
    }
    #[test]
    fn same_character_refresh_and_error_preserve_prior_result_and_receipt() {
        let mut loader = Loader::default();
        loader.select(Some("1"));
        let received = std::time::Instant::now();
        loader.accept(loader.generation, Ok(progress("1", &["10"], received)));
        let old_generation = loader.generation;
        loader.refresh();
        assert_eq!(loader.progress.as_ref().unwrap().received_at, received);
        loader.accept(loader.generation, Err("Temporary failure.".into()));
        assert_eq!(
            loader.progress.as_ref().unwrap().completed,
            ["10".to_owned()].into()
        );
        assert_eq!(loader.progress.as_ref().unwrap().received_at, received);
        assert!(loader.status.contains("Showing the last completion result"));
        loader.accept(old_generation, Ok(progress("1", &["99"], received)));
        assert!(!loader.progress.as_ref().unwrap().completed.contains("99"));
    }
    #[test]
    fn partial_refresh_unions_completions_and_uses_new_receipt() {
        let mut loader = Loader::default();
        loader.select(Some("1"));
        let first = std::time::Instant::now();
        let later = first + std::time::Duration::from_secs(1);
        loader.accept(loader.generation, Ok(progress("1", &["10", "20"], first)));
        loader.refresh();
        loader.accept(loader.generation, Ok(progress("1", &["20", "30"], later)));
        let current = loader.progress.as_ref().unwrap();
        assert_eq!(
            current.completed,
            ["10".into(), "20".into(), "30".into()].into()
        );
        assert_eq!(current.received_at, later);
        loader.refresh();
        loader.accept(loader.generation, Ok(progress("1", &[], later)));
        assert_eq!(loader.progress.as_ref().unwrap().completed.len(), 3);
    }
    #[test]
    fn selected_identity_change_clears_immediately_and_rejects_stale_or_wrong_identity() {
        let mut loader = Loader::default();
        loader.select(Some("1"));
        let received = std::time::Instant::now();
        loader.accept(loader.generation, Ok(progress("1", &["10"], received)));
        let stale = loader.generation;
        loader.select(Some("2"));
        assert!(loader.progress.is_none());
        loader.accept(stale, Ok(progress("1", &["11"], received)));
        assert!(loader.progress.is_none());
        loader.accept(loader.generation, Ok(progress("2", &["20"], received)));
        loader.accept(loader.generation, Ok(progress("1", &["10"], received)));
        let current = loader.progress.as_ref().unwrap();
        assert_eq!(current.character_id, "2");
        assert_eq!(current.completed, ["20".into()].into());
        loader.select(None);
        assert!(loader.progress.is_none());
    }
}
