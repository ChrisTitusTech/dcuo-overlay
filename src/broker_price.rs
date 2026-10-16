//! One-shot Browse search followed by a fresh, matching price copied to the clipboard.
use crate::{
    broker_hotkey::{Event, Hotkey},
    broker_memory::Snapshot,
};
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemKey {
    pub id: u32,
    pub name: String,
    pub quantity: u32,
}
struct Pending {
    item: ItemKey,
    owner: (usize, String),
    baseline: u64,
    started: Option<Instant>,
    finished_at: Option<Instant>,
    ready_at: Option<Instant>,
    preflight: bool,
}
impl Pending {
    fn price(&self, snapshot: &Snapshot, now: Instant) -> Result<Option<u64>, String> {
        if self.owner != (snapshot.entity, snapshot.player_name.clone()) {
            return Err("Character changed; search cancelled".into());
        }
        let Some(started) = self.started else {
            return Ok(None);
        };
        if now.saturating_duration_since(started) > Duration::from_secs(30) {
            return Err(
                "No verified matching search result within 30 seconds; nothing copied".into(),
            );
        }
        let expected = self
            .baseline
            .checked_add(1)
            .ok_or("Broker counter overflow")?;
        if snapshot.request_token > expected {
            return Err("Another Broker query intervened; nothing copied".into());
        }
        if !self.preflight
            || self.finished_at.is_none_or(|at| snapshot.observed_at < at)
            || snapshot.request_token != expected
            || snapshot.observed_at < started
        {
            return Ok(None);
        }
        let rows: Vec<_> = snapshot
            .listings
            .iter()
            .filter(|r| r.item_id == self.item.id && r.quantity_candidate == self.item.quantity)
            .collect();
        // Wait for late-loaded names rather than miss a lower unnamed matching offer.
        if rows.iter().any(|r| r.name.is_none()) {
            return Ok(None);
        }
        let lowest = rows
            .into_iter()
            .filter(|r| r.name.as_deref() == Some(&self.item.name))
            .map(|r| r.total_price)
            .min();
        match lowest {
            None => Ok(None),
            Some(value) => suggested(value).map(Some).ok_or_else(|| {
                "Lowest matching price cannot be undercut by $1; nothing copied".into()
            }),
        }
    }
}
#[derive(Default)]
pub struct PriceEntry {
    selected: Option<ItemKey>,
    confirmed: bool,
    hotkey: Option<Hotkey>,
    pending: Option<Pending>,
    armed_pid: Option<i32>,
    copied: Option<(ItemKey, u64, Instant)>,
    status: String,
}
impl PriceEntry {
    pub fn cancel(&mut self) {
        if self.hotkey.take().is_some() || self.pending.is_some() {
            self.status = "Search cancelled; no new price copied".into();
        }
        self.pending = None;
        self.armed_pid = None;
    }
    pub fn reset(&mut self) {
        self.cancel();
        *self = Self::default();
    }
    fn tick(&mut self, ctx: &egui::Context, pid: Option<i32>, snapshot: Option<&Snapshot>) {
        while let Some(event) = self.hotkey.as_mut().and_then(Hotkey::poll) {
            match event {
                Event::Started(at) => {
                    if let Some(pending) = &mut self.pending {
                        pending.started = Some(at);
                        self.status = format!("Searching for {}...", pending.item.name);
                    } else {
                        self.status = "Filling the focused price field...".into();
                    }
                }
                Event::ReadyToSearch(at) => {
                    if let Some(pending) = &mut self.pending {
                        pending.ready_at = Some(at);
                        self.status =
                            "Checking Broker response state before submitting search...".into();
                    }
                }
                Event::Finished(result) => {
                    self.hotkey = None;
                    match result {
                        Ok(()) => {
                            if let Some(pending) = &mut self.pending {
                                pending.finished_at = Some(Instant::now());
                                self.status =
                                    "Search entered; waiting for fresh matching prices...".into();
                            } else {
                                self.armed_pid = None;
                                self.status = "Price typed once. Verify the amount before listing; no listing was submitted.".into();
                            }
                        }
                        Err(error) => {
                            self.cancel();
                            self.status = format!(
                                "Hotkey stopped: {error}. Check the field before retrying."
                            );
                        }
                    }
                }
            }
        }
        let Some(pending) = &self.pending else {
            return;
        };
        // A missing snapshot must still allow the response wait to expire.
        if pending
            .started
            .is_some_and(|at| at.elapsed() > Duration::from_secs(30))
        {
            self.cancel();
            self.status = "Search timed out; nothing copied".into();
            return;
        }
        let Some(snapshot) = snapshot else {
            return;
        };
        if pending.finished_at.is_none() && snapshot.request_token != pending.baseline {
            self.cancel();
            self.status =
                "Broker results changed before search completion. Arm the search again.".into();
            return;
        }
        if let Some(at) = pending.ready_at
            && snapshot.observed_at >= at
        {
            if snapshot.request_token != pending.baseline
                || (snapshot.entity, snapshot.player_name.clone()) != pending.owner
            {
                self.cancel();
                self.status = "Broker changed before submission; search cancelled".into();
                return;
            }
            if let Some(hotkey) = &self.hotkey {
                hotkey.confirm_search();
            }
            if let Some(pending) = &mut self.pending {
                pending.preflight = true;
                pending.ready_at = None;
            }
        }
        let Some(pending) = &self.pending else {
            return;
        };
        match pending.price(snapshot, Instant::now()) {
            Ok(Some(price)) => {
                let item = pending.item.clone();
                ctx.copy_text(price.to_string());
                self.copied = Some((item.clone(), price, Instant::now()));
                self.cancel();
                self.status = format!(
                    "Copied {price} for {} (lowest matching observed total minus $1).",
                    item.name
                );
                if let Some(pid) = pid {
                    match crate::broker_hotkey::arm_price(pid, price) {
                        Ok(hotkey) => {
                            self.hotkey = Some(hotkey);
                            self.armed_pid = Some(pid);
                            self.status.push_str(" Next Ctrl+Alt+P fills this amount in your focused My Wares price field. Armed for 60 seconds; no further subtraction or submission.");
                        }
                        Err(error) => self
                            .status
                            .push_str(&format!(" Price hotkey unavailable: {error}")),
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.cancel();
                self.status = error;
            }
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pid: Option<i32>,
        items: &[ItemKey],
        snapshot: Option<&Snapshot>,
    ) {
        if (self.pending.is_some() || self.hotkey.is_some()) && self.armed_pid != pid {
            self.cancel();
        }
        let previous = self.selected.clone();
        ui.strong("Search item and copy price");
        egui::ComboBox::from_id_salt("broker-price-item")
            .selected_text(
                self.selected
                    .as_ref()
                    .map_or("Choose an observed item", |s| &s.name),
            )
            .show_ui(ui, |ui| {
                for item in items {
                    ui.selectable_value(
                        &mut self.selected,
                        Some(item.clone()),
                        format!("{} (client quantity {})", item.name, item.quantity),
                    );
                }
            });
        if previous != self.selected {
            self.cancel();
            self.confirmed = false;
            self.copied = None;
            self.status.clear();
        }
        let selected_present = self.selected.as_ref().is_some_and(|s| items.contains(s));
        if !selected_present {
            self.cancel();
            self.confirmed = false;
        }
        if ui.checkbox(&mut self.confirmed, "I will focus the Browse search field; All categories, Price Ascending, Usable by me OFF").changed() { self.cancel(); }
        ui.small("Arm, then click the game's Browse search item field and press/release Ctrl+Alt+P. It replaces the text with the selected item and presses Enter to search. Once the new price is copied, the NEXT press replaces your focused My Wares price field with that amount, without Enter. Watch the status to know which step is armed. Turn off Caps Lock, Num Lock and Sticky Keys.");
        ui.small("Fresh matching results copy the lowest observed total minus $1, including your own offers. Only the selected client quantity is compared; confirm the actual stack. Observed pages may not cover the whole market. No old cached price is copied on failure.");
        self.tick(ui.ctx(), pid, snapshot);
        ui.horizontal(|ui| {
            let fresh = snapshot.is_some_and(|s| s.observed_at.elapsed() < Duration::from_secs(5));
            if ui.add_enabled(self.pending.is_none() && self.hotkey.is_none() && pid.is_some() && selected_present && self.confirmed && fresh, egui::Button::new("Arm search: Ctrl+Alt+P (60 seconds)")).clicked() {
                let item = self.selected.clone().unwrap();
                let snapshot = snapshot.unwrap();
                match crate::broker_hotkey::arm(pid.unwrap(), &item.name) {
                    Ok(hotkey) => {
                        self.hotkey = Some(hotkey);
                        self.pending = Some(Pending { item, owner: (snapshot.entity, snapshot.player_name.clone()), baseline: snapshot.request_token, started: None, finished_at: None, ready_at: None, preflight: false });
                        self.armed_pid = pid;
                        self.copied = None;
                        self.status = "Armed once. Focus the Browse search field, then press and release Ctrl+Alt+P.".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            if ui.add_enabled(self.pending.is_some() || self.hotkey.is_some(), egui::Button::new("Cancel hotkey")).clicked() { self.cancel(); }
        });
        if let Some((item, price, at)) = &self.copied {
            ui.label(format!(
                "Copied: {price} for {} ({} seconds ago)",
                item.name,
                at.elapsed().as_secs()
            ));
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        ui.separator();
    }
}
fn suggested(total: u64) -> Option<u64> {
    total
        .checked_sub(1)
        .filter(|v| *v > 0 && *v <= u64::from(u32::MAX))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker_memory::Listing;
    fn fixture() -> (Pending, Snapshot) {
        let at = Instant::now();
        let item = ItemKey {
            id: 42,
            name: "Synthetic Research".into(),
            quantity: 1,
        };
        let pending = Pending {
            item: item.clone(),
            owner: (123, "Synthetic Player".into()),
            baseline: 5,
            started: Some(at),
            finished_at: Some(at),
            ready_at: None,
            preflight: true,
        };
        let row = Listing {
            id: 1,
            item_id: item.id,
            name: Some(item.name),
            seller: "Other".into(),
            total_price: 100,
            quantity_candidate: 1,
            is_own_candidate: false,
        };
        (
            pending,
            Snapshot {
                entity: 123,
                player_name: "Synthetic Player".into(),
                listings: vec![row],
                request_token: 6,
                response_count: 1,
                observed_at: at,
            },
        )
    }
    #[test]
    fn only_fresh_exact_response_after_typing_is_accepted() {
        let (mut p, mut s) = fixture();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        p.preflight = false;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        p.preflight = true;
        p.finished_at = None;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        p.finished_at = p.started;
        s.request_token = 5;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.request_token = 7;
        assert!(p.price(&s, Instant::now()).is_err());
        s.request_token = 6;
        s.observed_at -= Duration::from_secs(1);
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.player_name = "Different".into();
        assert!(p.price(&s, Instant::now()).is_err());
    }
    #[test]
    fn response_during_typing_cannot_arm_price_fill() {
        let (mut p, mut s) = fixture();
        p.finished_at = Some(s.observed_at + Duration::from_millis(500));
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.observed_at = p.finished_at.unwrap();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        s.request_token += 1;
        assert!(p.price(&s, Instant::now()).is_err());
    }
    #[test]
    fn exact_item_stack_and_all_loaded_names_required() {
        let (p, mut s) = fixture();
        let mut cheaper = s.listings[0].clone();
        cheaper.id = 2;
        cheaper.total_price = 50;
        cheaper.name = None;
        s.listings.push(cheaper);
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.listings[1].name = Some(p.item.name.clone());
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(49));
        s.listings[1].quantity_candidate = 2;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        s.listings[0].name = Some("Other item".into());
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.listings.clear();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        assert!(
            p.price(&s, Instant::now() + Duration::from_secs(31))
                .is_err()
        );
    }
    #[test]
    fn lowest_includes_owned_offers_and_never_underflows() {
        let (p, mut s) = fixture();
        s.listings[0].seller = s.player_name.clone();
        s.listings[0].is_own_candidate = true;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        for value in [0, 1, u64::MAX] {
            s.listings[0].total_price = value;
            assert!(p.price(&s, Instant::now()).is_err());
        }
    }
}
