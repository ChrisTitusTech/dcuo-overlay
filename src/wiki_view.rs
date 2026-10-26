//! Explicit pairing, local capture preview, and background wiki publication.
use crate::{
    session,
    wiki_items::{self, MissingItems, Receipt, Submission},
};
use eframe::egui;
use std::sync::mpsc::{self, Receiver, SyncSender};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    pid: i32,
    start_ticks: u64,
    build: String,
    owner: (usize, u64),
}
fn binding(view: &session::View) -> Option<Binding> {
    if !view.connected || view.paused {
        return None;
    }
    let native = view.native.as_ref()?;
    if native.observed_at.elapsed() > std::time::Duration::from_secs(1) {
        return None;
    }
    let build = view.build.as_ref()?;
    if view.selected_pid != Some(build.pid) {
        return None;
    }
    Some(Binding {
        pid: build.pid,
        start_ticks: build.start_ticks,
        build: build.sha256.clone(),
        owner: native.feats_binding?,
    })
}
fn capture_fresh(timestamp: u64) -> bool {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|now| now.as_secs().checked_sub(timestamp))
        .is_some_and(|age| age <= 300)
}
enum ResultMessage {
    Fetched(Result<MissingItems, String>),
    Sent(Result<Receipt, String>),
}
struct Pending {
    epoch: u64,
    receiver: Receiver<ResultMessage>,
}
#[derive(Default)]
pub struct WikiView {
    token: String,
    epoch: u64,
    fetch_attempted: bool,
    binding: Option<Binding>,
    missing: Option<MissingItems>,
    preview: Option<Submission>,
    capture_request: Option<u64>,
    preview_request: Option<u64>,
    pending: Option<Pending>,
    status: String,
}
impl WikiView {
    fn clear_capture(&mut self) {
        self.preview = None;
        self.capture_request = None;
        self.preview_request = None;
    }
    fn token_changed(&mut self) {
        self.token = self.token.trim().chars().take(256).collect();
        self.epoch = self.epoch.wrapping_add(1);
        self.fetch_attempted = false;
        self.missing = None;
        self.clear_capture();
        self.status = if wiki_items::is_hex64(&self.token) {
            "Loading linked wiki items..."
        } else {
            "Paste the complete 64-character wiki token."
        }
        .into();
    }
    fn next_fetch(&mut self, retry: bool) -> Option<(String, SyncSender<ResultMessage>)> {
        if retry || !self.fetch_attempted {
            self.begin_fetch()
        } else {
            None
        }
    }
    fn begin_fetch(&mut self) -> Option<(String, SyncSender<ResultMessage>)> {
        if !wiki_items::is_hex64(&self.token) || self.pending.is_some() {
            return None;
        }
        self.fetch_attempted = true;
        self.missing = None;
        self.clear_capture();
        self.status = "Loading linked wiki items...".into();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some(Pending {
            epoch: self.epoch,
            receiver,
        });
        Some((self.token.clone(), sender))
    }
    fn poll(&mut self, view: &session::View) {
        let current = binding(view);
        if self.binding != current {
            self.clear_capture();
            self.binding = current;
        }
        if let Some(pending) = &self.pending {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    let accept = pending.epoch == self.epoch;
                    self.pending = None;
                    if accept {
                        match result {
                            ResultMessage::Fetched(Ok(missing)) => match missing.validate() {
                                Ok(()) => {
                                    self.status = format!(
                                        "Linked to {} (world {}): {} unresolved items.",
                                        missing.character.name,
                                        missing.character.world_id,
                                        missing.items.len()
                                    );
                                    self.missing = Some(missing);
                                }
                                Err(error) => self.status = error,
                            },
                            ResultMessage::Sent(Ok(receipt)) => {
                                self.status = format!(
                                    "Published {} item names; {} skipped.",
                                    receipt.submitted, receipt.skipped
                                );
                                self.missing = None;
                                self.clear_capture();
                            }
                            ResultMessage::Fetched(Err(error))
                            | ResultMessage::Sent(Err(error)) => self.status = error,
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let accept = pending.epoch == self.epoch;
                    self.pending = None;
                    if accept {
                        self.status = "Wiki worker stopped. Refresh to retry.".into();
                    }
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if self.preview_request.is_some_and(|id| {
            view.wiki_items.as_ref().is_none_or(|capture| {
                capture.request_id != id || !capture_fresh(capture.captured_at)
            })
        }) {
            self.clear_capture();
            self.status = "Capture expired or the session changed. Capture again.".into();
        }
        if let Some(request) = self.capture_request
            && let Some(capture) = view
                .wiki_items
                .as_ref()
                .filter(|capture| capture.request_id == request)
        {
            self.capture_request = None;
            if !capture_fresh(capture.captured_at)
                || self
                    .binding
                    .as_ref()
                    .is_none_or(|b| b.build != capture.build)
            {
                self.status = "Capture expired or the build changed. Capture again.".into();
            } else if let Some(missing) = &self.missing {
                match missing.submission(capture) {
                    Ok(preview) => {
                        self.preview = Some(preview);
                        self.preview_request = Some(request);
                        self.status = "Preview ready. Check names before publishing.".into();
                    }
                    Err(error) => self.status = error,
                }
            }
        }
        if self.capture_request.is_some() && !view.busy && view.wiki_items.is_none() {
            self.capture_request = None;
            self.status = view.wiki_status.clone();
        }
    }
    fn capture_reason(&self, view: &session::View) -> Option<&'static str> {
        if !wiki_items::is_hex64(&self.token) {
            return Some("Paste a complete wiki token first.");
        }
        if self.pending.is_some() {
            return Some("Wait for the wiki request to finish.");
        }
        let Some(missing) = &self.missing else {
            return Some("Load the linked wiki item list first. Use Refresh to retry.");
        };
        if missing.items.is_empty() {
            return Some("No unresolved equipped items. Refresh equipment on the wiki if needed.");
        }
        if !view.connected {
            return Some("Connect to the running game to capture item names.");
        }
        if view.paused {
            return Some("Resume the game connection to capture item names.");
        }
        if view.build.is_none() {
            return Some("Wait for the game build to be verified.");
        }
        if self.binding.is_none() || binding(view) != self.binding {
            return Some("Wait for fresh character memory from the selected game session.");
        }
        if self.capture_request.is_some() {
            return Some("Wait for the current item capture to finish.");
        }
        if view.busy {
            return Some("Wait for the current session operation to finish.");
        }
        None
    }
    fn send_reason(&self, view: &session::View) -> Option<&'static str> {
        self.capture_reason(view).or_else(|| {
            self.preview
                .is_none()
                .then_some("Capture item names and check the preview before sending.")
        })
    }
    fn begin_capture(&mut self, view: &session::View, queue: impl FnOnce(Vec<u32>) -> u64) {
        if self.capture_reason(view).is_some() {
            return;
        }
        self.clear_capture();
        let ids = self
            .missing
            .as_ref()
            .expect("ready account")
            .items
            .iter()
            .map(|v| v.item_id)
            .collect();
        let id = queue(ids);
        self.capture_request = (id != 0).then_some(id);
        self.status = if id == 0 {
            "Capture could not be queued."
        } else {
            "Reading requested item names..."
        }
        .into();
    }
    fn begin_send(
        &mut self,
        view: &session::View,
    ) -> Option<(String, Submission, SyncSender<ResultMessage>)> {
        if self.send_reason(view).is_some() {
            return None;
        }
        let submission = self.preview.take()?;
        self.preview_request = None;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some(Pending {
            epoch: self.epoch,
            receiver,
        });
        self.status = "Publishing item names...".into();
        Some((self.token.clone(), submission, sender))
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &session::View,
        controller: &mut session::Controller,
    ) {
        self.poll(view);
        ui.heading("Wiki item names");
        ui.label("Sending publishes these item names directly to the shared wiki catalog for everyone. Your wiki account history records the contribution.");
        ui.hyperlink_to(
            "Open wiki token settings",
            "https://dcuo-wiki.com/equipment-identification/#overlay-access",
        );
        ui.label("Paste your scoped wiki token. Account items load automatically; the token stays only in this app's memory.");
        if ui
            .add(
                egui::TextEdit::singleline(&mut self.token)
                    .password(true)
                    .char_limit(256)
                    .hint_text("Wiki token"),
            )
            .changed()
        {
            self.token_changed();
        }
        if ui.button("Clear token").clicked() {
            self.token.clear();
            self.token_changed();
        }
        let retry = ui
            .add_enabled(
                wiki_items::is_hex64(&self.token) && self.pending.is_none(),
                egui::Button::new("Refresh wiki items"),
            )
            .clicked();
        if let Some((token, sender)) = self.next_fetch(retry) {
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Fetched(wiki_items::fetch(&token)));
            });
        }
        if let Some(missing) = &self.missing {
            ui.label(format!(
                "Wiki character: {} | world {} | {} unresolved items",
                missing.character.name,
                missing.character.world_id,
                missing.items.len()
            ));
        }
        let reason = self.capture_reason(view);
        if ui
            .add_enabled(reason.is_none(), egui::Button::new("Capture item names"))
            .clicked()
        {
            self.begin_capture(view, |ids| controller.capture_wiki_items(ids));
        }
        if let Some(reason) = reason {
            ui.small(reason);
        }
        if self.capture_request.is_some() {
            ui.label(&view.wiki_status);
        }
        if let Some(preview) = &self.preview {
            ui.label(format!(
                "{} | world {} | {} names",
                preview.character.name,
                preview.character.world_id,
                preview.items.len()
            ));
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for item in &preview.items {
                        ui.label(format!("{}: {}", item.item_id, item.name));
                    }
                });
        }
        let reason = self.send_reason(view);
        if ui
            .add_enabled(reason.is_none(), egui::Button::new("Send names to wiki"))
            .clicked()
            && let Some((token, submission, sender)) = self.begin_send(view)
        {
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Sent(wiki_items::submit(&token, &submission)));
            });
        }
        if let Some(reason) = reason {
            ui.small(reason);
        }
        ui.label(&self.status);
        if self.pending.is_some() || self.capture_request.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
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
            character: wiki_items::Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            items: vec![wiki_items::ItemId { item_id: 7 }],
        }
    }
    fn capture(request: u64, name: &str) -> wiki_items::WikiCapture {
        wiki_items::WikiCapture {
            request_id: request,
            player_name: name.into(),
            items: vec![(7, "Item name".into())],
            build: "a".repeat(64),
            captured_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    fn paired(view: &session::View) -> WikiView {
        let mut state = WikiView {
            token: format!(" {}\n", "a".repeat(64)),
            ..Default::default()
        };
        state.token_changed();
        let (_, sender) = state.next_fetch(false).unwrap();
        sender.send(ResultMessage::Fetched(Ok(missing()))).unwrap();
        state.poll(view);
        assert!(state.next_fetch(false).is_none());
        state
    }
    #[test]
    fn token_only_fetches_without_game_or_reference() {
        let mut state = paired(&session::View::default());
        assert_eq!(state.token, "a".repeat(64));
        assert!(state.fetch_attempted);
        assert!(state.missing.is_some());
        assert!(state.pending.is_none());
        assert!(state.next_fetch(false).is_none());
        assert!(
            state
                .capture_reason(&session::View::default())
                .unwrap()
                .contains("Connect")
        );
    }
    #[test]
    fn no_reference_capture_then_explicit_send() {
        let mut view = live();
        assert!(view.api.is_none());
        let mut state = paired(&view);
        assert!(state.capture_reason(&view).is_none());
        assert!(state.send_reason(&view).is_some());
        state.begin_capture(&view, |ids| {
            assert_eq!(ids, vec![7]);
            8
        });
        assert!(
            state
                .capture_reason(&view)
                .unwrap()
                .contains("current item capture")
        );
        view.wiki_items = Some(capture(8, "hERO"));
        state.poll(&view);
        assert!(state.preview.is_some());
        assert!(state.pending.is_none());
        let (_, submission, sender) = state.begin_send(&view).unwrap();
        assert_eq!(submission.character.world_id, "2");
        assert!(state.begin_send(&view).is_none());
        assert!(state.preview.is_none());
        view.connected = false;
        sender
            .send(ResultMessage::Sent(Ok(Receipt {
                submitted: 1,
                skipped: 0,
            })))
            .unwrap();
        state.poll(&view);
        assert!(state.status.contains("Published 1"));
    }
    #[test]
    fn transient_game_gaps_keep_account_but_destroy_previews_and_requests() {
        let mut view = live();
        let mut state = paired(&view);
        state.begin_capture(&view, |_| 1);
        view.wiki_items = Some(capture(1, "Hero"));
        state.poll(&view);
        assert!(state.preview.is_some());
        view.native = None;
        state.poll(&view);
        assert!(state.missing.is_some());
        assert!(state.preview.is_none());
        view = live();
        view.wiki_items = Some(capture(1, "Hero"));
        state.poll(&view);
        assert!(state.preview.is_none());
        assert!(state.capture_reason(&view).is_none());
        state.begin_capture(&view, |_| 2);
        view.connected = false;
        state.poll(&view);
        assert!(state.capture_request.is_none());
        assert!(state.missing.is_some());
    }
    #[test]
    fn stale_token_results_rejected_and_retry_is_explicit() {
        let view = session::View::default();
        let mut state = WikiView {
            token: "a".repeat(64),
            ..Default::default()
        };
        state.token_changed();
        let (_, old) = state.begin_fetch().unwrap();
        state.token = "b".repeat(64);
        state.token_changed();
        assert!(state.begin_fetch().is_none());
        old.send(ResultMessage::Fetched(Ok(missing()))).unwrap();
        state.poll(&view);
        assert!(state.missing.is_none());
        assert!(!state.fetch_attempted);
        let (_, new) = state.begin_fetch().unwrap();
        new.send(ResultMessage::Fetched(Err("Expired token".into())))
            .unwrap();
        state.poll(&view);
        assert!(state.fetch_attempted);
        assert_eq!(state.status, "Expired token");
        assert!(state.next_fetch(false).is_none());
        assert!(state.next_fetch(true).is_some());
    }
    #[test]
    fn wrong_character_empty_and_stale_captures_do_not_enable_send() {
        for kind in [0, 1, 2, 3] {
            let mut view = live();
            let mut state = paired(&view);
            state.begin_capture(&view, |_| 8);
            let mut result = capture(8, "Hero");
            match kind {
                0 => result.player_name = "Other".into(),
                1 => result.items.clear(),
                2 => result.captured_at = 1,
                _ => result.items[0].0 = 9,
            };
            view.wiki_items = Some(result);
            state.poll(&view);
            assert!(state.preview.is_none());
            assert!(state.begin_send(&view).is_none());
        }
    }
    #[test]
    fn busy_and_old_request_cannot_advance_capture_sequence() {
        let mut view = live();
        let mut state = paired(&view);
        view.busy = true;
        state.begin_capture(&view, |_| panic!("must not queue while busy"));
        assert!(state.capture_request.is_none());
        view.busy = false;
        state.begin_capture(&view, |_| 8);
        view.wiki_items = Some(capture(7, "Hero"));
        state.poll(&view);
        assert!(state.preview.is_none());
        assert!(state.begin_send(&view).is_none());
        view.wiki_items = Some(capture(8, "Hero"));
        state.poll(&view);
        assert!(state.preview.is_some());
        view.wiki_items = None;
        state.poll(&view);
        assert!(state.preview.is_none());
    }
    #[test]
    fn oversized_token_is_not_accepted_as_a_valid_prefix() {
        let mut state = WikiView {
            token: "a".repeat(65),
            ..Default::default()
        };
        state.token_changed();
        assert!(state.begin_fetch().is_none());
    }
    #[test]
    fn readiness_explains_game_pause_freshness_and_empty_items() {
        let mut view = live();
        let mut state = paired(&view);
        view.paused = true;
        state.poll(&view);
        assert!(state.capture_reason(&view).unwrap().contains("Resume"));
        view.paused = false;
        view.native = None;
        state.poll(&view);
        assert!(state.capture_reason(&view).unwrap().contains("fresh"));
        state.missing.as_mut().unwrap().items.clear();
        assert!(
            state
                .capture_reason(&view)
                .unwrap()
                .contains("No unresolved")
        );
    }
    #[test]
    fn stale_token_publication_receipt_cannot_change_new_pairing() {
        let mut view = live();
        let mut state = paired(&view);
        state.begin_capture(&view, |_| 1);
        view.wiki_items = Some(capture(1, "Hero"));
        state.poll(&view);
        let (_, _, sender) = state.begin_send(&view).unwrap();
        state.token = "b".repeat(64);
        state.token_changed();
        sender
            .send(ResultMessage::Sent(Ok(Receipt {
                submitted: 1,
                skipped: 0,
            })))
            .unwrap();
        state.poll(&view);
        assert!(!state.status.contains("Published"));
        assert!(state.missing.is_none());
        assert!(!state.fetch_attempted);
    }
}
