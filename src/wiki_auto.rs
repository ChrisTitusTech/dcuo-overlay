//! Throttled missing-only contributions through the existing session reader.
use super::{Binding, binding, capture_fresh};
use crate::{
    session,
    wiki_items::{self, MissingItems, Receipt, Submission},
};
use std::{
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(90);
pub(super) enum Reply {
    Checked(Result<MissingItems, String>),
    Sent(Result<Receipt, String>),
}
struct Job {
    binding: Binding,
    epoch: u64,
    request: Option<u64>,
    submission: Option<Submission>,
    next_cursor: u32,
    pending: Option<Receiver<Reply>>,
    invalidated: bool,
}
#[derive(Default)]
pub(super) struct Automatic {
    pub paused: bool,
    pub status: String,
    cursor: u32,
    due: Option<Instant>,
    job: Option<Job>,
    owner: Option<Binding>,
    epoch: u64,
}
pub(super) enum Work {
    Check(Vec<u32>, SyncSender<Reply>),
    Send(Submission, SyncSender<Reply>),
}
impl Work {
    pub fn run(self, token: String, source: Option<std::path::PathBuf>) {
        std::thread::spawn(move || match self {
            Self::Check(ids, sender) => {
                let _ = sender.send(Reply::Checked(wiki_items::fetch_missing(&token, &ids)));
            }
            Self::Send(submission, sender) => {
                let _ = sender.send(Reply::Sent(wiki_items::submit_missing_with_icons(
                    &token,
                    &submission,
                    source.as_deref(),
                )));
            }
        });
    }
}
impl Automatic {
    fn finish(&mut self, now: Instant, status: String) {
        self.job = None;
        self.due = Some(now + INTERVAL);
        self.status = status;
    }
    /// Called on every application update, independently of the selected tab.
    pub fn step(
        &mut self,
        view: &session::View,
        epoch: u64,
        linked: Option<&MissingItems>,
        blocked: bool,
        now: Instant,
        queue: impl FnOnce(u32) -> u64,
    ) -> Option<Work> {
        let current = binding(view);
        if self.owner != current || self.epoch != epoch {
            self.owner = current.clone();
            self.epoch = epoch;
            self.cursor = 0;
            if let Some(job) = &mut self.job {
                job.invalidated = true;
            }
        }
        if (self.paused || blocked || linked.is_none() || current.is_none())
            && let Some(job) = &mut self.job
        {
            job.invalidated = true;
        }
        if let Some(job) = &mut self.job {
            if current.as_ref() != Some(&job.binding) || job.epoch != epoch {
                job.invalidated = true;
            }
            // Drain an in-flight worker before starting another, even after invalidation.
            if let Some(receiver) = &job.pending {
                let reply = match receiver.try_recv() {
                    Ok(reply) => reply,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.finish(
                            now,
                            "Automatic upload worker stopped. Will retry after a pause.".into(),
                        );
                        return None;
                    }
                };
                job.pending = None;
                if job.invalidated {
                    self.finish(
                        now,
                        "Automatic upload cancelled after the session or token changed.".into(),
                    );
                    return None;
                }
                match reply {
                    Reply::Checked(Ok(missing)) => {
                        let submission = job.submission.as_mut().expect("checked capture");
                        if missing.character != submission.character
                            || !capture_fresh(submission.captured_at)
                        {
                            self.finish(now, "Wiki character changed or capture expired; automatic upload skipped.".into());
                            return None;
                        }
                        submission.items.retain(|item| {
                            missing.items.iter().any(|id| id.item_id == item.item_id)
                        });
                        if submission.items.is_empty() {
                            self.cursor = job.next_cursor;
                            self.finish(now, "Observed item IDs are already on the wiki.".into());
                            return None;
                        }
                        let submission = job.submission.take().unwrap();
                        let (sender, receiver) = mpsc::sync_channel(1);
                        job.pending = Some(receiver);
                        self.status = format!(
                            "Automatically uploading {} missing item definitions...",
                            submission.items.len()
                        );
                        return Some(Work::Send(submission, sender));
                    }
                    Reply::Sent(Ok(receipt)) => {
                        self.cursor = job.next_cursor;
                        self.finish(now, format!("Automatically uploaded {} missing items; {} already known. Icons uploaded: {}; unavailable: {}.", receipt.submitted, receipt.skipped, receipt.icons_uploaded, receipt.icons_missing));
                    }
                    Reply::Checked(Err(error)) | Reply::Sent(Err(error)) => {
                        self.finish(now, format!("Automatic upload paused: {error}"))
                    }
                }
                return None;
            }
            if job.invalidated {
                self.finish(now, "Waiting for a fresh linked game session.".into());
                return None;
            }
            if let Some(request) = job.request {
                if let Some(capture) = view
                    .wiki_items
                    .as_ref()
                    .filter(|capture| capture.request_id == request)
                {
                    job.request = None;
                    if !capture.catalog
                        || capture.build != job.binding.build
                        || !capture_fresh(capture.captured_at)
                    {
                        self.finish(now, "Automatic capture was stale or unsupported.".into());
                        return None;
                    }
                    if capture.items.is_empty() {
                        self.cursor = if capture.next_cursor == self.cursor {
                            0
                        } else {
                            capture.next_cursor
                        };
                        self.finish(now, "Waiting for more loaded item definitions.".into());
                        return None;
                    }
                    match linked.expect("valid job target").submission(capture) {
                        Ok(submission) => {
                            let ids = submission.items.iter().map(|item| item.item_id).collect();
                            job.next_cursor = capture.next_cursor;
                            job.submission = Some(submission);
                            let (sender, receiver) = mpsc::sync_channel(1);
                            job.pending = Some(receiver);
                            self.status =
                                "Checking observed Census item IDs against the wiki...".into();
                            return Some(Work::Check(ids, sender));
                        }
                        Err(error) => {
                            self.finish(now, format!("Automatic capture skipped: {error}"))
                        }
                    }
                } else if !view.busy {
                    self.finish(
                        now,
                        format!("Automatic capture unavailable: {}", view.wiki_status),
                    );
                }
            }
            return None;
        }
        if self.paused
            || blocked
            || linked.is_none()
            || current.is_none()
            || view.busy
            || self.due.is_some_and(|due| now < due)
        {
            return None;
        }
        let request = queue(self.cursor);
        self.due = Some(now + INTERVAL);
        if request != 0 {
            self.job = Some(Job {
                binding: current.unwrap(),
                epoch,
                request: Some(request),
                submission: None,
                next_cursor: self.cursor,
                pending: None,
                invalidated: false,
            });
            self.status = "Reading loaded items for automatic wiki contribution...".into();
        }
        None
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> session::View {
        let mut native = crate::native::Sample::synthetic(1);
        native.feats_binding = Some((10, 20));
        session::View {
            connected: true,
            selected_pid: Some(1),
            build: Some(session::BuildInfo {
                pid: 1,
                start_ticks: 2,
                sha256: "a".repeat(64),
                executable_bytes: 100,
                machine: 0x8664,
                header_access: true,
                map_source: None,
            }),
            native: Some(native),
            ..Default::default()
        }
    }
    fn missing() -> MissingItems {
        MissingItems {
            next_cursor: None,
            character: wiki_items::Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            items: vec![wiki_items::ItemId { item_id: 7 }],
        }
    }
    fn capture() -> wiki_items::WikiCapture {
        wiki_items::WikiCapture {
            catalog: true,
            next_cursor: 70,
            equipped: vec![],
            request_id: 1,
            player_name: "Hero".into(),
            items: vec![wiki_items::NamedItem {
                item_id: 7,
                name: "Item name".into(),
                details: wiki_items::ItemDetails {
                    description: Some("Details".into()),
                    ..Default::default()
                },
            }],
            build: "a".repeat(64),
            captured_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    fn step(state: &mut Automatic, view: &session::View, epoch: u64, now: Instant) -> Option<Work> {
        state.step(view, epoch, Some(&missing()), false, now, |_| {
            panic!("unexpected capture")
        })
    }
    fn checking(now: Instant) -> (Automatic, session::View, SyncSender<Reply>) {
        let mut state = Automatic::default();
        let mut view = live();
        assert!(
            state
                .step(&view, 1, Some(&missing()), false, now, |cursor| {
                    assert_eq!(cursor, 0);
                    1
                })
                .is_none()
        );
        assert!(state.busy());
        view.wiki_items = Some(capture());
        let Some(Work::Check(ids, sender)) = step(&mut state, &view, 1, now) else {
            panic!("expected check")
        };
        assert_eq!(ids, vec![7]);
        (state, view, sender)
    }
    #[test]
    fn captures_checks_sends_and_throttles_repeated_frames() {
        let now = Instant::now();
        let (mut state, view, sender) = checking(now);
        for _ in 0..5 {
            assert!(step(&mut state, &view, 1, now).is_none());
        }
        sender.send(Reply::Checked(Ok(missing()))).unwrap();
        let Some(Work::Send(submission, sender)) = step(&mut state, &view, 1, now) else {
            panic!("expected send")
        };
        assert_eq!(submission.scope, "catalog");
        assert!(submission.equipped.is_empty());
        assert_eq!(submission.items[0].item_id, 7);
        sender
            .send(Reply::Sent(Ok(Receipt {
                submitted: 1,
                ..Default::default()
            })))
            .unwrap();
        assert!(step(&mut state, &view, 1, now).is_none());
        assert!(!state.busy());
        assert_eq!(state.cursor, 70);
        for offset in [0, 1, 89] {
            assert!(step(&mut state, &view, 1, now + Duration::from_secs(offset)).is_none());
        }
        state.step(
            &view,
            1,
            Some(&missing()),
            false,
            now + INTERVAL,
            |cursor| {
                assert_eq!(cursor, 70);
                2
            },
        );
        assert!(state.busy());
    }
    #[test]
    fn known_items_skip_submission_and_advance_cursor() {
        let now = Instant::now();
        let (mut state, view, sender) = checking(now);
        let mut known = missing();
        known.items.clear();
        sender.send(Reply::Checked(Ok(known))).unwrap();
        assert!(step(&mut state, &view, 1, now).is_none());
        assert!(!state.busy());
        assert_eq!(state.cursor, 70);
        assert!(state.status.contains("already on the wiki"));
    }
    #[test]
    fn session_token_pause_and_block_changes_prevent_checked_capture_writes() {
        for change in 0..6 {
            let now = Instant::now();
            let (mut state, mut view, sender) = checking(now);
            let mut epoch = 1;
            let mut linked = Some(missing());
            let mut blocked = false;
            match change {
                0 => view.build.as_mut().unwrap().start_ticks += 1,
                1 => epoch += 1,
                2 => state.paused = true,
                3 => view.paused = true,
                4 => blocked = true,
                _ => linked = None,
            }
            // The old worker remains the only job until its reply is drained.
            assert!(
                state
                    .step(&view, epoch, linked.as_ref(), blocked, now, |_| panic!(
                        "capture while draining"
                    ))
                    .is_none()
            );
            assert!(state.busy());
            sender.send(Reply::Checked(Ok(missing()))).unwrap();
            assert!(
                state
                    .step(&view, epoch, linked.as_ref(), blocked, now, |_| panic!(
                        "invalidated capture"
                    ))
                    .is_none()
            );
            assert!(!state.busy());
            assert!(state.status.contains("cancelled"));
        }
    }
    #[test]
    fn wrong_character_capture_and_changed_check_identity_are_skipped() {
        let now = Instant::now();
        let mut state = Automatic::default();
        let mut view = live();
        state.step(&view, 1, Some(&missing()), false, now, |_| 1);
        let mut wrong = capture();
        wrong.player_name = "Other".into();
        view.wiki_items = Some(wrong);
        assert!(step(&mut state, &view, 1, now).is_none());
        assert!(!state.busy());
        assert!(state.status.contains("skipped"));
        let (mut state, view, sender) = checking(now);
        let mut wrong = missing();
        wrong.character.id = "3".into();
        sender.send(Reply::Checked(Ok(wrong))).unwrap();
        assert!(step(&mut state, &view, 1, now).is_none());
        assert!(!state.busy());
        assert!(state.status.contains("character changed"));
    }
    #[test]
    fn check_and_send_failures_back_off_without_advancing_cursor() {
        for fail_send in [false, true] {
            let now = Instant::now();
            let (mut state, view, sender) = checking(now);
            if fail_send {
                sender.send(Reply::Checked(Ok(missing()))).unwrap();
                let Some(Work::Send(_, sender)) = step(&mut state, &view, 1, now) else {
                    panic!("expected send")
                };
                sender.send(Reply::Sent(Err("unavailable".into()))).unwrap();
            } else {
                sender
                    .send(Reply::Checked(Err("unavailable".into())))
                    .unwrap();
            }
            assert!(step(&mut state, &view, 1, now).is_none());
            assert!(!state.busy());
            assert_eq!(state.cursor, 0);
            assert!(state.status.contains("unavailable"));
            assert!(
                step(
                    &mut state,
                    &view,
                    1,
                    now + INTERVAL - Duration::from_secs(1)
                )
                .is_none()
            );
            state.step(&view, 1, Some(&missing()), false, now + INTERVAL, |_| 2);
            assert!(state.busy());
        }
    }
    #[test]
    fn empty_batches_advance_and_exhaustion_resets_for_rescan() {
        let now = Instant::now();
        let mut state = Automatic::default();
        let mut view = live();
        for (index, expected_cursor, next_cursor) in [(0, 0, 70), (1, 70, 70), (2, 0, 0)] {
            let at = now + INTERVAL * index;
            state.step(&view, 1, Some(&missing()), false, at, |cursor| {
                assert_eq!(cursor, expected_cursor);
                1
            });
            let mut empty = capture();
            empty.items.clear();
            empty.next_cursor = next_cursor;
            view.wiki_items = Some(empty);
            assert!(step(&mut state, &view, 1, at).is_none());
            assert!(!state.busy());
            assert_eq!(
                state.cursor,
                if expected_cursor == next_cursor {
                    0
                } else {
                    next_cursor
                }
            );
        }
    }
}
