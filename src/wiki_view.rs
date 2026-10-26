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
fn equipped_preview_lines(preview: &Submission) -> Vec<String> {
    let mut lines = Vec::new();
    for row in &preview.equipped {
        let name = preview
            .items
            .iter()
            .find(|item| item.item_id == row.item_id)
            .map(|item| item.name.as_str())
            .unwrap_or("Name unavailable");
        lines.push(format!("Slot {} | {}: {}", row.slot, row.item_id, name));
        lines.push(match row.item_level {
            Some(level) => format!("Item level: {level}"),
            None => "Item level: unavailable".into(),
        });
        for stat in &row.base_stats {
            lines.push(format!("Base {}: {}", stat.name, stat.value));
        }
        for effect in &row.effects {
            lines.push(format!("Effect: {effect}"));
        }
    }
    lines
}
enum ResultMessage {
    Fetched(Result<MissingItems, String>),
    Sent(Result<Receipt, String>),
}
struct Pending {
    epoch: u64,
    page_cursor: u32,
    page_binding: Option<Binding>,
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
    equipment_cursor: u32,
    catalog: bool,
    catalog_cursor: u32,
    batch_next_cursor: u32,
    catalog_exhausted: bool,
}
impl WikiView {
    fn reset_equipment_page(&mut self) {
        if self.equipment_cursor != 0 {
            self.missing = None;
            self.fetch_attempted = false;
        }
        self.equipment_cursor = 0;
    }
    fn reset_catalog_cursor(&mut self) {
        self.catalog_cursor = 0;
        self.batch_next_cursor = 0;
        self.catalog_exhausted = false;
    }
    fn change_mode(&mut self, catalog: bool) {
        if self.preview.is_some() || self.pending.is_some() || self.capture_request.is_some() {
            return;
        }
        if self.catalog != catalog {
            self.reset_equipment_page();
            self.catalog = catalog;
            self.reset_catalog_cursor();
            self.clear_capture();
        }
    }
    fn clear_capture(&mut self) {
        self.preview = None;
        self.capture_request = None;
        self.preview_request = None;
    }
    fn token_changed(&mut self) {
        self.token = self.token.trim().chars().take(256).collect();
        self.epoch = self.epoch.wrapping_add(1);
        self.fetch_attempted = false;
        self.reset_equipment_page();
        self.reset_catalog_cursor();
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
        self.begin_fetch_page(0)
    }
    fn begin_equipment_page(&mut self) -> Option<(String, u32, SyncSender<ResultMessage>)> {
        if self.catalog
            || self.pending.is_some()
            || self.capture_request.is_some()
            || self.preview.is_some()
        {
            return None;
        }
        let cursor = self.missing.as_ref()?.next_cursor?;
        if cursor <= self.equipment_cursor {
            return None;
        }
        let (token, sender) = self.begin_fetch_page(cursor)?;
        Some((token, cursor, sender))
    }
    fn begin_fetch_page(&mut self, cursor: u32) -> Option<(String, SyncSender<ResultMessage>)> {
        if !wiki_items::is_hex64(&self.token)
            || self.pending.is_some()
            || self.preview.is_some()
            || self.capture_request.is_some()
        {
            return None;
        }
        self.fetch_attempted = true;
        self.missing = None;
        self.clear_capture();
        self.status = "Loading linked wiki items...".into();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some(Pending {
            epoch: self.epoch,
            page_cursor: cursor,
            page_binding: self.binding.clone(),
            receiver,
        });
        Some((self.token.clone(), sender))
    }
    fn poll(&mut self, view: &session::View) {
        let current = binding(view);
        if self.binding != current {
            self.reset_equipment_page();
            self.reset_catalog_cursor();
            self.clear_capture();
            self.binding = current;
        }
        if let Some(pending) = &self.pending {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    let cursor = pending.page_cursor;
                    let accept = pending.epoch == self.epoch
                        && (cursor == 0 || pending.page_binding == self.binding);
                    self.pending = None;
                    if !accept && cursor != 0 {
                        self.fetch_attempted = false;
                    }
                    if accept {
                        match result {
                            ResultMessage::Fetched(Ok(missing)) => match missing.validate() {
                                Ok(()) => {
                                    self.equipment_cursor = cursor;
                                    self.status = format!(
                                        "Linked to {} (world {}): {} equipped item definitions.",
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
                                    "Published {} item details; {} skipped. Icons uploaded: {}; missing: {}.{}",
                                    receipt.submitted,
                                    receipt.skipped,
                                    receipt.icons_uploaded,
                                    receipt.icons_missing,
                                    if receipt.icons_missing > 0 {
                                        " Artwork is incomplete for this batch."
                                    } else {
                                        ""
                                    }
                                );
                                if self.catalog {
                                    self.catalog_cursor = self.batch_next_cursor;
                                }
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
            } else if capture.catalog != self.catalog {
                self.status = "Capture mode changed. Capture again.".into();
            } else if capture.catalog && capture.items.is_empty() {
                self.catalog_exhausted = capture.next_cursor == self.catalog_cursor;
                self.catalog_cursor = capture.next_cursor;
                self.status=if self.catalog_exhausted {"No more loaded item IDs were found after this cursor. This is not a complete game catalog."} else {"This batch had no loaded substantive item details. Capture the next batch to continue."}.into();
            } else if let Some(missing) = &self.missing {
                match missing.submission(capture) {
                    Ok(preview) => {
                        self.batch_next_cursor = capture.next_cursor;
                        self.preview = Some(preview);
                        self.preview_request = Some(request);
                        self.status = "Preview ready. Check details before publishing.".into();
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
        if !self.catalog && missing.items.is_empty() {
            return Some("No equipped items. Refresh equipment on the wiki if needed.");
        }
        if !view.connected {
            return Some("Connect to the running game to capture item details.");
        }
        if view.paused {
            return Some("Resume the game connection to capture item details.");
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
                .then_some("Capture item details and check the preview before sending.")
        })
    }
    fn begin_capture(&mut self, view: &session::View, queue: impl FnOnce(Vec<u32>) -> u64) {
        if self.preview.is_some() || self.capture_reason(view).is_some() {
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
            "Reading requested item details..."
        }
        .into();
    }
    fn begin_catalog_capture(&mut self, view: &session::View, queue: impl FnOnce(u32) -> u64) {
        if !self.catalog
            || self.catalog_exhausted
            || self.preview.is_some()
            || self.capture_reason(view).is_some()
        {
            return;
        }
        self.clear_capture();
        let request = queue(self.catalog_cursor);
        self.capture_request = (request != 0).then_some(request);
        self.status = if request == 0 {
            "Capture could not be queued."
        } else {
            "Reading at most 50 loaded item IDs..."
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
            page_cursor: 0,
            page_binding: None,
            receiver,
        });
        self.status = "Publishing item details...".into();
        Some((self.token.clone(), submission, sender))
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &session::View,
        controller: &mut session::Controller,
    ) {
        self.poll(view);
        ui.heading("Wiki item details");
        ui.label("Sending publishes these item details directly to the shared wiki catalog for everyone. Your wiki account history records the contribution.");
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
                wiki_items::is_hex64(&self.token)
                    && self.pending.is_none()
                    && self.preview.is_none(),
                egui::Button::new("Refresh wiki items"),
            )
            .clicked();
        if let Some((token, sender)) = self.next_fetch(retry) {
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Fetched(wiki_items::fetch(&token)));
            });
        }
        let next_enabled = !self.catalog
            && self
                .missing
                .as_ref()
                .and_then(|page| page.next_cursor)
                .is_some()
            && self.pending.is_none()
            && self.capture_request.is_none()
            && self.preview.is_none();
        if ui
            .add_enabled(next_enabled, egui::Button::new("Next equipment batch"))
            .clicked()
            && let Some((token, cursor, sender)) = self.begin_equipment_page()
        {
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Fetched(wiki_items::fetch_page(
                    &token,
                    Some(cursor),
                )));
            });
        }
        if let Some(missing) = &self.missing {
            ui.label(format!(
                "Wiki character: {} | world {} | {} equipped item definitions",
                missing.character.name,
                missing.character.world_id,
                missing.items.len()
            ));
        }
        let mode_enabled =
            self.preview.is_none() && self.pending.is_none() && self.capture_request.is_none();
        let mut catalog = self.catalog;
        ui.add_enabled_ui(mode_enabled, |ui| {
            ui.radio_value(&mut catalog, false, "Equipped items");
            ui.radio_value(&mut catalog, true, "Loaded item catalog");
        });
        self.change_mode(catalog);
        if self.catalog {
            ui.small("Each batch reads up to 50 IDs already loaded by the game. It may include items beyond your equipment and does not cover the whole game catalog.");
        }
        let reason = self.capture_reason(view);
        let label = if self.catalog {
            if self.catalog_cursor == 0 {
                "Capture loaded items"
            } else {
                "Capture next loaded items"
            }
        } else {
            "Capture item details"
        };
        if ui
            .add_enabled(
                reason.is_none() && self.preview.is_none() && !self.catalog_exhausted,
                egui::Button::new(label),
            )
            .clicked()
        {
            if self.catalog {
                self.begin_catalog_capture(view, |cursor| {
                    controller.capture_wiki_catalog_items(cursor)
                });
            } else {
                self.begin_capture(view, |ids| controller.capture_wiki_items(ids));
            }
        }
        if self.preview.is_some() && self.pending.is_none() && ui.button("Clear preview").clicked()
        {
            self.catalog_cursor = self.batch_next_cursor;
            self.clear_capture();
        }
        if let Some(reason) = reason {
            ui.small(reason);
        }
        if self.capture_request.is_some() {
            ui.label(&view.wiki_status);
        }
        if let Some(preview) = &self.preview {
            ui.label(format!(
                "{} | world {} | {} items",
                preview.character.name,
                preview.character.world_id,
                preview.items.len()
            ));
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for line in equipped_preview_lines(preview) {
                        ui.label(line);
                    }
                    if !preview.equipped.is_empty() {
                        ui.separator();
                    }
                    for item in &preview.items {
                        ui.label(format!("{}: {}", item.item_id, item.name));
                        if let Some(description) = &item.details.description {
                            ui.label(description);
                        }
                        if let Some(style) = &item.details.style {
                            ui.small(format!("Style: {style}"));
                        }
                        if let Some(icon) = &item.details.icon_resource {
                            ui.small(format!("Icon: {icon}"));
                        }
                        if let Some(level) = item.details.required_level {
                            ui.small(format!("Required level: {level}"));
                        }
                        if let Some(cr) = item.details.required_combat_rating {
                            ui.small(format!("Required combat rating: {cr}"));
                        }
                        if let Some(quality) = item.details.quality_id {
                            ui.small(format!("Quality ID: {quality}"));
                        }
                        if let Some(category) = item.details.category_id {
                            ui.small(format!("Category ID: {category}"));
                        }
                        ui.separator();
                    }
                });
        }
        let reason = self.send_reason(view);
        if ui
            .add_enabled(
                reason.is_none(),
                egui::Button::new("Send item details to wiki"),
            )
            .clicked()
            && let Some((token, submission, sender)) = self.begin_send(view)
        {
            let icon_source = view.build.as_ref().and_then(|b| b.map_source.clone());
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Sent(wiki_items::submit_with_icons(
                    &token,
                    &submission,
                    icon_source.as_deref(),
                )));
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
            next_cursor: None,
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
            catalog: false,
            next_cursor: 0,
            equipped: Vec::new(),
            request_id: request,
            player_name: name.into(),
            items: vec![wiki_items::NamedItem {
                item_id: 7,
                name: "Item name".into(),
                details: Default::default(),
            }],
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
                ..Default::default()
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
                _ => result.items[0].item_id = 9,
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
        assert!(state.capture_reason(&view).unwrap().contains("No equipped"));
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
                ..Default::default()
            })))
            .unwrap();
        state.poll(&view);
        assert!(!state.status.contains("Published"));
        assert!(state.missing.is_none());
        assert!(!state.fetch_attempted);
    }
    #[test]
    fn catalog_preview_requires_send_or_clear_and_resets_on_owner_change() {
        let mut view = live();
        let mut state = paired(&view);
        state.change_mode(true);
        state.begin_catalog_capture(&view, |cursor| {
            assert_eq!(cursor, 0);
            17
        });
        let mut result = capture(17, "Hero");
        result.catalog = true;
        result.next_cursor = 999;
        result.items[0].item_id = 999;
        result.items[0].details.description = Some("Details".into());
        view.wiki_items = Some(result);
        state.poll(&view);
        assert!(state.preview.is_some());
        assert_eq!(state.catalog_cursor, 0);
        assert_eq!(state.batch_next_cursor, 999);
        state.begin_catalog_capture(&view, |_| panic!("must retain unsent preview"));
        state.change_mode(false);
        assert!(state.catalog);
        assert!(state.preview.is_some());
        view.native.as_mut().unwrap().feats_binding = Some((0x20000, 9));
        state.poll(&view);
        assert!(state.preview.is_none());
        assert_eq!(state.catalog_cursor, 0);
        assert_eq!(state.batch_next_cursor, 0);
    }
    #[test]
    fn empty_catalog_batches_advance_cursor_without_claiming_complete_catalog() {
        let mut view = live();
        let mut state = paired(&view);
        state.change_mode(true);
        state.begin_catalog_capture(&view, |_| 1);
        let mut result = capture(1, "Hero");
        result.catalog = true;
        result.next_cursor = 50;
        result.items.clear();
        view.wiki_items = Some(result);
        state.poll(&view);
        assert_eq!(state.catalog_cursor, 50);
        assert!(state.preview.is_none());
        assert!(!state.catalog_exhausted);
        state.begin_catalog_capture(&view, |cursor| {
            assert_eq!(cursor, 50);
            2
        });
        let mut result = capture(2, "Hero");
        result.catalog = true;
        result.next_cursor = 50;
        result.items.clear();
        view.wiki_items = Some(result);
        state.poll(&view);
        assert!(state.catalog_exhausted);
        assert!(state.status.contains("not a complete game catalog"));
    }
    #[test]
    fn successful_catalog_send_advances_without_losing_pairing_and_token_resets_cursor() {
        let mut view = live();
        let mut state = paired(&view);
        state.change_mode(true);
        state.begin_catalog_capture(&view, |_| 1);
        let mut result = capture(1, "Hero");
        result.catalog = true;
        result.next_cursor = 123;
        result.items[0].details.description = Some("Details".into());
        view.wiki_items = Some(result);
        state.poll(&view);
        let (_, submission, sender) = state.begin_send(&view).unwrap();
        assert_eq!(submission.scope, "catalog");
        assert!(submission.equipped.is_empty());
        sender
            .send(ResultMessage::Sent(Ok(Receipt {
                submitted: 1,
                skipped: 0,
                ..Default::default()
            })))
            .unwrap();
        state.poll(&view);
        assert_eq!(state.catalog_cursor, 123);
        assert!(state.missing.is_some());
        assert!(state.preview.is_none());
        state.token_changed();
        assert_eq!(state.catalog_cursor, 0);
        assert_eq!(state.batch_next_cursor, 0);
    }
    #[test]
    fn partial_icon_receipt_keeps_incomplete_artwork_visible() {
        let mut view = live();
        let mut state = paired(&view);
        state.begin_capture(&view, |_| 1);
        view.wiki_items = Some(capture(1, "Hero"));
        state.poll(&view);
        let (_, _, sender) = state.begin_send(&view).unwrap();
        sender
            .send(ResultMessage::Sent(Ok(Receipt {
                submitted: 1,
                skipped: 0,
                icons_uploaded: 0,
                icons_missing: 1,
            })))
            .unwrap();
        state.poll(&view);
        assert!(state.status.contains("missing: 1"));
        assert!(state.status.contains("Artwork is incomplete"));
    }
    #[test]
    fn equipped_preview_discloses_distinct_slot_levels_stats_and_effects() {
        let missing = missing();
        let mut capture = capture(1, "Hero");
        capture.equipped = vec![
            crate::equipped_items::EquippedItem {
                slot: "24".into(),
                item_id: 7,
                item_level: Some(381),
                effects: vec!["First effect".into()],
                base_stats: vec![crate::equipped_items::EquippedStat {
                    name: "Health".into(),
                    value: 100,
                }],
            },
            crate::equipped_items::EquippedItem {
                slot: "25".into(),
                item_id: 7,
                item_level: Some(407),
                effects: vec![],
                base_stats: vec![crate::equipped_items::EquippedStat {
                    name: "Health".into(),
                    value: 200,
                }],
            },
        ];
        let preview = missing.submission(&capture).unwrap();
        let lines = equipped_preview_lines(&preview);
        assert_eq!(
            lines,
            vec![
                "Slot 24 | 7: Item name",
                "Item level: 381",
                "Base Health: 100",
                "Effect: First effect",
                "Slot 25 | 7: Item name",
                "Item level: 407",
                "Base Health: 200"
            ]
        );
    }
    #[test]
    fn next_equipment_batch_uses_cursor_and_preserves_requested_id_membership() {
        let mut view = live();
        let mut state = paired(&view);
        state.missing.as_mut().unwrap().next_cursor = Some(50);
        let (_, cursor, sender) = state.begin_equipment_page().unwrap();
        assert_eq!(cursor, 50);
        let mut page = missing();
        page.items = vec![wiki_items::ItemId { item_id: 51 }];
        page.next_cursor = Some(100);
        sender.send(ResultMessage::Fetched(Ok(page))).unwrap();
        state.poll(&view);
        assert_eq!(state.equipment_cursor, 50);
        state.begin_capture(&view, |ids| {
            assert_eq!(ids, vec![51]);
            2
        });
        let mut result = capture(2, "Hero");
        result.items[0].item_id = 51;
        view.wiki_items = Some(result);
        state.poll(&view);
        assert!(state.preview.is_some());
        assert!(state.begin_equipment_page().is_none());
        assert!(state.begin_fetch().is_none());
        state.clear_capture();
        let (_, cursor, _) = state.begin_equipment_page().unwrap();
        assert_eq!(cursor, 100);
    }
    #[test]
    fn stale_owner_cannot_restore_pending_equipment_page_and_refresh_starts_at_zero() {
        let mut view = live();
        let mut state = paired(&view);
        state.missing.as_mut().unwrap().next_cursor = Some(50);
        let (_, _, sender) = state.begin_equipment_page().unwrap();
        view.native.as_mut().unwrap().feats_binding = Some((0x30000, 9));
        state.poll(&view);
        let mut page = missing();
        page.items[0].item_id = 51;
        page.next_cursor = Some(100);
        sender.send(ResultMessage::Fetched(Ok(page))).unwrap();
        state.poll(&view);
        assert_eq!(state.equipment_cursor, 0);
        assert!(state.missing.is_none());
        assert!(!state.fetch_attempted);
        let (_, sender) = state.begin_fetch().unwrap();
        sender.send(ResultMessage::Fetched(Ok(missing()))).unwrap();
        state.poll(&view);
        assert_eq!(state.equipment_cursor, 0);
        assert_eq!(state.missing.as_ref().unwrap().items[0].item_id, 7);
    }
}
