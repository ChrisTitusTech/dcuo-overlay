//! Explicit pairing, local capture preview, and background wiki submission.
use crate::{
    session,
    wiki_items::{self, MissingItems, Receipt, Submission},
};
use eframe::egui;
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    pid: i32,
    start_ticks: u64,
    build: String,
    character: wiki_items::Character,
    owner: Option<(usize, u64)>,
}
fn binding(view: &session::View) -> Option<Binding> {
    if !view.connected || view.paused {
        return None;
    }
    let native = view.native.as_ref()?;
    if native.observed_at.elapsed() > std::time::Duration::from_secs(1) {
        return None;
    }
    let owner = native.feats_binding?;
    let build = view.build.as_ref()?;
    let api = view.api.as_ref()?;
    Some(Binding {
        pid: build.pid,
        start_ticks: build.start_ticks,
        build: build.sha256.clone(),
        character: wiki_items::Character {
            id: api.id.clone(),
            name: api.name.to_ascii_lowercase(),
            world_id: api.world_id.clone(),
        },
        owner: Some(owner),
    })
}
fn matches_character(left: &wiki_items::Character, right: &wiki_items::Character) -> bool {
    left.id == right.id
        && left.world_id == right.world_id
        && left.name.eq_ignore_ascii_case(&right.name)
}
fn accepts_result(
    request_epoch: u64,
    epoch: u64,
    request: &Binding,
    current: Option<&Binding>,
) -> bool {
    request_epoch == epoch && Some(request) == current
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
    binding: Binding,
    receiver: Receiver<ResultMessage>,
}
#[derive(Default)]
pub struct WikiView {
    token: String,
    epoch: u64,
    binding: Option<Binding>,
    missing: Option<MissingItems>,
    preview: Option<Submission>,
    capture_request: Option<u64>,
    preview_request: Option<u64>,
    pending: Option<Pending>,
    status: String,
}
impl WikiView {
    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.missing = None;
        self.preview = None;
        self.capture_request = None;
        self.preview_request = None;
        // Keep at most one bounded worker alive even after invalidation.
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: &session::View,
        controller: &mut session::Controller,
    ) {
        let current = binding(view);
        if self.binding != current {
            self.invalidate();
            self.binding = current.clone();
            self.status = "Session changed. Refresh the wiki item list.".into();
        }
        if let Some(pending) = &self.pending {
            match pending.receiver.try_recv() {
                Ok(result) => {
                    let accept = accepts_result(
                        pending.epoch,
                        self.epoch,
                        &pending.binding,
                        current.as_ref(),
                    );
                    self.pending = None;
                    if accept {
                        match result {
                            ResultMessage::Fetched(Ok(missing)) => {
                                if current.as_ref().is_some_and(|b| {
                                    matches_character(&b.character, &missing.character)
                                }) {
                                    self.status = format!(
                                        "{} unresolved items available.",
                                        missing.items.len()
                                    );
                                    self.missing = Some(missing);
                                } else {
                                    self.status="Linked wiki character must match the current API character and world.".into();
                                }
                            }
                            ResultMessage::Sent(Ok(receipt)) => {
                                self.status = format!(
                                    "Published {} item names; {} skipped.",
                                    receipt.submitted, receipt.skipped
                                );
                                self.missing = None;
                                self.preview = None;
                            }
                            ResultMessage::Fetched(Err(error))
                            | ResultMessage::Sent(Err(error)) => {
                                self.status = error;
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.status = "Wiki worker stopped. Refresh to retry.".into();
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if self.preview_request.is_some_and(|id| {
            view.wiki_items.as_ref().is_none_or(|capture| {
                capture.request_id != id || !capture_fresh(capture.captured_at)
            })
        }) {
            self.preview = None;
            self.preview_request = None;
            self.status = "Capture expired or the session changed. Capture again.".into();
        }
        if let Some(request) = self.capture_request
            && let Some(capture) = view.wiki_items.as_ref().filter(|v| v.request_id == request)
        {
            self.capture_request = None;
            self.preview_request = Some(request);
            self.preview = self.missing.as_ref().and_then(|missing| {
                if !capture_fresh(capture.captured_at)
                    || current.as_ref().is_none_or(|b| b.build != capture.build)
                {
                    self.status = "Capture build changed. Capture again.".into();
                    return None;
                }
                match missing.submission(capture) {
                    Ok(value) => {
                        self.status = "Preview ready. Check names before publishing.".into();
                        Some(value)
                    }
                    Err(error) => {
                        self.status = error;
                        None
                    }
                }
            });
        }
        if self.capture_request.is_some() && !view.busy && view.wiki_items.is_none() {
            self.capture_request = None;
            self.status = view.wiki_status.clone();
        }
        ui.heading("Wiki item names");
        ui.label("Sending publishes these item names directly to the shared wiki catalog for everyone. Your wiki account history records the contribution.");
        ui.hyperlink_to(
            "Open wiki token settings",
            "https://dcuo-wiki.com/equipment-identification/#overlay-access",
        );
        ui.label("Create a scoped token in your wiki account, then paste it here. It is kept only until this app closes or you clear it.");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.token)
                .password(true)
                .char_limit(64)
                .hint_text("Wiki token"),
        );
        if edit.changed() {
            self.invalidate();
            self.status = "Token changed. Refresh the wiki item list.".into();
        }
        if ui.button("Clear token").clicked() {
            self.token.clear();
            self.invalidate();
        }
        let ready =
            current.is_some() && wiki_items::is_hex64(&self.token) && self.pending.is_none();
        if ui
            .add_enabled(ready, egui::Button::new("Refresh wiki items"))
            .clicked()
        {
            self.invalidate();
            let token = self.token.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            self.pending = Some(Pending {
                epoch: self.epoch,
                binding: current.clone().expect("ready binding"),
                receiver,
            });
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Fetched(wiki_items::fetch(&token)));
            });
            self.status = "Loading linked wiki items...".into();
        }
        let capture_ready = ready && self.missing.as_ref().is_some_and(|m| !m.items.is_empty());
        if ui
            .add_enabled(capture_ready, egui::Button::new("Capture item names"))
            .clicked()
        {
            self.preview = None;
            let ids = self
                .missing
                .as_ref()
                .expect("capture list")
                .items
                .iter()
                .map(|v| v.item_id)
                .collect();
            let id = controller.capture_wiki_items(ids);
            self.capture_request = (id != 0).then_some(id);
            self.status = if id == 0 {
                "Capture could not be queued."
            } else {
                "Reading requested item names..."
            }
            .into();
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
        if ui
            .add_enabled(
                ready && self.preview.is_some(),
                egui::Button::new("Send names to wiki"),
            )
            .clicked()
        {
            let submission = self.preview.take().expect("send preview");
            let token = self.token.clone();
            let (sender, receiver) = mpsc::sync_channel(1);
            self.pending = Some(Pending {
                epoch: self.epoch,
                binding: current.expect("ready binding"),
                receiver,
            });
            std::thread::spawn(move || {
                let _ = sender.send(ResultMessage::Sent(wiki_items::submit(&token, &submission)));
            });
            self.status = "Publishing item names...".into();
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
    #[test]
    fn world_and_character_must_match() {
        let a = wiki_items::Character {
            id: "1".into(),
            name: "Hero".into(),
            world_id: "2".into(),
        };
        let mut b = a.clone();
        b.name = "HERO".into();
        assert!(matches_character(&a, &b));
        b.world_id = "4".into();
        assert!(!matches_character(&a, &b));
        b.world_id = "2".into();
        b.id = "3".into();
        assert!(!matches_character(&a, &b));
    }
    #[test]
    fn stale_token_process_owner_and_api_results_are_rejected() {
        let original = Binding {
            pid: 1,
            start_ticks: 3,
            build: "a".repeat(64),
            character: wiki_items::Character {
                id: "1".into(),
                name: "hero".into(),
                world_id: "2".into(),
            },
            owner: Some((10, 20)),
        };
        assert!(accepts_result(1, 1, &original, Some(&original)));
        assert!(!accepts_result(1, 2, &original, Some(&original)));
        assert!(!accepts_result(1, 1, &original, None));
        let mut changed = original.clone();
        changed.start_ticks += 1;
        assert!(!accepts_result(1, 1, &original, Some(&changed)));
        changed = original.clone();
        changed.character.world_id = "4".into();
        assert!(!accepts_result(1, 1, &original, Some(&changed)));
        changed = original.clone();
        changed.owner = Some((30, 40));
        assert!(!accepts_result(1, 1, &original, Some(&changed)));
    }
}
