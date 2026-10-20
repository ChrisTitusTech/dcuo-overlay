//! Screen-selected item workflow. Clipboard and keyboard prices are subtracted once.
use crate::{
    broker_hotkey::{Event, Hotkey},
    broker_memory::Snapshot,
    broker_screen::{Monitor, Panel, Screen},
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
    name: String,
    owner: (usize, String),
    baseline: u64,
    ready_at: Option<Instant>,
    finished_at: Option<Instant>,
    baseline_verified: bool,
    unfiltered_after: Option<u64>,
    began: Instant,
}
impl Pending {
    fn price(&self, s: &Snapshot, now: Instant) -> Result<Option<u64>, String> {
        if (s.entity, s.player_name.clone()) != self.owner {
            return Err("Character changed".into());
        }
        if now.saturating_duration_since(self.began) > Duration::from_secs(120) {
            return Err("Search wait expired; press the item-name hotkey again".into());
        }
        if now.saturating_duration_since(s.observed_at) >= Duration::from_secs(5) {
            return Ok(None);
        }
        if self
            .unfiltered_after
            .is_none_or(|token| s.request_token <= token)
        {
            return Ok(None);
        }
        if !self.baseline_verified
            || self.finished_at.is_none_or(|at| s.observed_at < at)
            || s.request_token <= self.baseline
        {
            return Ok(None);
        }
        // User performs the search normally; intermediate filter/page queries are
        // allowed, but only fresh exact-name, unambiguous item/quantity rows qualify.
        let rows: Vec<_> = s
            .listings
            .iter()
            .filter(|r| r.name.as_deref() == Some(self.name.as_str()))
            .collect();
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        if first.item_id == 0 || first.quantity_candidate != 1 {
            return Err("Only single-item market prices are supported; no price prepared".into());
        }
        if rows
            .iter()
            .any(|r| r.item_id != first.item_id || r.quantity_candidate != first.quantity_candidate)
        {
            return Err("Different item variants or stack sizes; no price prepared".into());
        }
        if s.listings.iter().any(|r| r.name.is_none()) {
            return Ok(None);
        }
        let lowest = rows.iter().map(|r| r.total_price).min().unwrap();
        lowest
            .checked_sub(1)
            .filter(|p| *p > 0 && *p <= u64::from(u32::MAX))
            .map(Some)
            .ok_or_else(|| "Cannot undercut this price by $1".into())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Name(String),
    Price(String, u64),
}
#[derive(Default)]
pub struct PriceEntry {
    wake: Option<Hotkey>,
    queued: Option<Instant>,
    last_press: Option<Instant>,
    monitor: Option<Monitor>,
    pid: Option<i32>,
    screen: Option<Screen>,
    candidate: Option<(String, u8)>,
    selected: Option<String>,
    hotkey: Option<Hotkey>,
    action: Option<Action>,
    pending: Option<Pending>,
    copied: Option<(String, u64, Instant)>,
    retry_at: Option<Instant>,
    status: String,
}
impl PriceEntry {
    fn stop_input(&mut self) {
        if self.hotkey.is_some() && matches!(self.action, Some(Action::Name(_))) {
            self.pending = None;
        }
        self.hotkey = None;
        self.action = None;
    }
    fn clear_flow(&mut self) {
        self.stop_input();
        self.pending = None;
        self.copied = None;
    }
    pub fn cancel(&mut self) {
        self.clear_flow();
        self.wake = None;
        self.queued = None;
        self.last_press = None;
        self.monitor = None;
        self.pid = None;
        self.screen = None;
        self.candidate = None;
        self.selected = None;
    }
    pub fn reset(&mut self) {
        self.cancel();
        *self = Self::default();
    }
    fn observe_screen(&mut self, screen: Screen) {
        // Both supported panels share the same verified left-side filter control.
        if (screen.panel == Panel::Other || screen.usable_by_me != Some(false))
            && let Some(p) = &mut self.pending
        {
            p.unfiltered_after = None;
        }
        match screen.panel {
            Panel::Tradable => {
                if let Some(name) = &screen.selected_name {
                    let count = match &self.candidate {
                        Some((old, n)) if old == name => n.saturating_add(1),
                        _ => 1,
                    };
                    self.candidate = Some((name.clone(), count));
                    if self.selected.as_ref() != Some(name) {
                        self.clear_flow();
                        self.selected = None;
                        if count >= 2 {
                            self.selected = Some(name.clone());
                            self.status = format!("Selected in game: {name}");
                        }
                    }
                } else {
                    self.stop_input();
                    self.candidate = None;
                }
            }
            Panel::Browse => {
                self.candidate = None;
            }
            Panel::Other => {
                self.clear_flow();
                self.selected = None;
                self.candidate = None;
            }
        }
        self.screen = Some(screen);
    }
    fn desired(&self) -> Option<Action> {
        let screen = self
            .screen
            .as_ref()
            .filter(|s| s.observed_at.elapsed() < Duration::from_secs(3))?;
        let name = self.selected.as_ref()?;
        if screen.panel == Panel::Tradable
            && !self
                .candidate
                .as_ref()
                .is_some_and(|(n, count)| n == name && *count >= 2)
        {
            return None;
        }
        if self.pending.is_some() {
            return None;
        }
        if let Some((priced, value, at)) = &self.copied {
            if priced == name
                && at.elapsed() < Duration::from_secs(120)
                && screen.panel == Panel::Tradable
                && screen.selected_name.as_ref() == Some(name)
            {
                return Some(Action::Price(name.clone(), *value));
            }
            return None;
        }
        match screen.panel {
            Panel::Tradable if screen.selected_name.as_ref() == Some(name) => {
                Some(Action::Name(name.clone()))
            }
            Panel::Browse => Some(Action::Name(name.clone())),
            _ => None,
        }
    }
    fn tick(&mut self, ctx: &egui::Context, pid: i32, snapshot: Option<&Snapshot>) {
        if self.pid != Some(pid) {
            self.cancel();
            self.pid = Some(pid);
        }
        if self.monitor.is_none() && self.retry_at.is_none_or(|at| Instant::now() >= at) {
            match Monitor::start(pid) {
                Ok(m) => self.monitor = Some(m),
                Err(e) => {
                    self.status = e;
                    self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
        if let Some(result) = self.monitor.as_ref().and_then(Monitor::poll) {
            match result {
                Ok(screen) => self.observe_screen(screen),
                Err(error) => {
                    self.screen = None;
                    if let Some(p) = &mut self.pending {
                        p.unfiltered_after = None;
                    }
                    self.candidate = None;
                    self.stop_input();
                    self.monitor = None;
                    self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                    self.status = error;
                }
            }
        }
        let fresh = self.screen.as_ref().is_some_and(|s| {
            s.observed_at.elapsed() < Duration::from_secs(3) && s.panel != Panel::Other
        });
        if !fresh {
            if let Some(p) = &mut self.pending {
                p.unfiltered_after = None;
            }
            self.candidate = None;
            self.stop_input();
            return;
        }
        // Only Broker freshness gates input; physics has an independent lifetime.
        if snapshot.is_none_or(|s| s.observed_at.elapsed() >= Duration::from_secs(5)) {
            self.stop_input();
            self.status = "Waiting for a fresh Broker read".into();
            return;
        }
        // Cancel an in-progress price action immediately when its panel/selection changes.
        if matches!(self.action, Some(Action::Price(..))) && self.desired() != self.action {
            self.stop_input();
        }
        while let Some(event) = self.hotkey.as_mut().and_then(Hotkey::poll) {
            match event {
                Event::Listening => {
                    self.status =
                        "Hotkey listening: focus the intended game field, then press Ctrl+Alt+P"
                            .into()
                }
                Event::Detected => {
                    self.status = "Ctrl+Alt+P detected; release the keys to paste".into()
                }
                Event::Pressed(_) => {}
                Event::Started(at) => {
                    if let Some(Action::Name(name)) = &self.action {
                        if let Some(s) =
                            snapshot.filter(|s| s.observed_at.elapsed() < Duration::from_secs(5))
                        {
                            self.pending = Some(Pending {
                                name: name.clone(),
                                owner: (s.entity, s.player_name.clone()),
                                baseline: s.request_token,
                                ready_at: None,
                                finished_at: None,
                                baseline_verified: false,
                                unfiltered_after: None,
                                began: at,
                            });
                        } else {
                            self.stop_input();
                            self.status = "Waiting for a fresh Broker read".into();
                        }
                    }
                }
                Event::ReadyToSearch(at) => {
                    if let Some(p) = &mut self.pending {
                        p.ready_at = Some(at);
                    }
                }
                Event::Finished(result) => {
                    self.hotkey = None;
                    let action = self.action.take();
                    match result {
                        Ok(()) => match action {
                            Some(Action::Name(_)) => {
                                if let Some(p) = &mut self.pending {
                                    p.finished_at = Some(Instant::now());
                                }
                                self.status = "Item name pasted. Run the Broker search normally; waiting for matching prices.".into();
                            }
                            Some(Action::Price(_, value)) => {
                                self.status = format!(
                                    "Sell Price filled with {value}. Review and submit yourself."
                                );
                            }
                            None => {}
                        },
                        Err(error) => {
                            self.pending = None;
                            self.status = error;
                            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
                        }
                    }
                }
            }
        }
        if let Some(p) = &self.pending {
            if p.began.elapsed() > Duration::from_secs(120) {
                self.stop_input();
                self.pending = None;
                self.status = "Search wait expired".into();
            } else if let Some(s) = snapshot {
                if let Some(at) = p.ready_at
                    && s.observed_at >= at
                {
                    if s.request_token != p.baseline || (s.entity, s.player_name.clone()) != p.owner
                    {
                        self.stop_input();
                        self.pending = None;
                        self.status = "Broker changed while typing; repeat the name hotkey".into();
                    } else {
                        if let Some(h) = &self.hotkey {
                            h.confirm_search();
                        }
                        if let Some(p) = &mut self.pending {
                            p.baseline_verified = true;
                            p.ready_at = None;
                        }
                    }
                }
                if let Some(p) = &mut self.pending {
                    let off = self.screen.as_ref().is_some_and(|screen| {
                        matches!(screen.panel, Panel::Browse | Panel::Tradable)
                            && screen.usable_by_me == Some(false)
                    });
                    if !off {
                        p.unfiltered_after = None;
                    } else if p.unfiltered_after.is_none()
                        && self
                            .screen
                            .as_ref()
                            .is_some_and(|screen| s.observed_at >= screen.observed_at)
                    {
                        p.unfiltered_after = Some(s.request_token);
                        self.status =
                            "Unfiltered baseline confirmed. Run the item search now.".into();
                    }
                }
                if let Some(p) = &self.pending {
                    let unfiltered_browse = self.screen.as_ref().is_some_and(|screen| {
                        screen.panel == Panel::Browse
                            && screen.usable_by_me == Some(false)
                            && s.observed_at >= screen.observed_at
                    });
                    if unfiltered_browse {
                        match p.price(s, Instant::now()) {
                            Ok(Some(value)) => {
                                let name = p.name.clone();
                                ctx.copy_text(value.to_string());
                                self.copied = Some((name.clone(), value, Instant::now()));
                                self.pending = None;
                                self.status = format!(
                                    "Ready: {value} for {name}. Return to Tradable Inventory and focus Sell Price; Ctrl+Alt+P fills it."
                                );
                            }
                            Ok(None) => {}
                            Err(error) => {
                                self.pending = None;
                                self.status = error;
                            }
                        }
                    }
                }
            }
        }
        if self
            .copied
            .as_ref()
            .is_some_and(|(_, _, at)| at.elapsed() >= Duration::from_secs(120))
        {
            self.copied = None;
            self.stop_input();
        }
        if self.hotkey.is_none()
            && self.queued.is_some()
            && self.retry_at.is_none_or(|at| Instant::now() >= at)
            && snapshot.is_some_and(|s| s.observed_at.elapsed() < Duration::from_secs(5))
            && let Some(action) = self.desired()
        {
            let result = match &action {
                Action::Name(name) => crate::broker_hotkey::trigger_text(pid, name),
                Action::Price(_, price) => crate::broker_hotkey::trigger_price(pid, *price),
            };
            match result {
                Ok(h) => {
                    self.queued = None;
                    self.hotkey = Some(h);
                    self.action = Some(action);
                }
                Err(e) => {
                    self.status = e;
                    self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
    }
    pub fn needs_reads(&self, visible: bool) -> bool {
        visible || self.queued.is_some() || self.pending.is_some() || self.hotkey.is_some()
    }
    fn accept_press(&mut self, pressed: Instant, now: Instant) {
        if self.last_press.is_some_and(|at| pressed <= at) {
            return;
        }
        self.last_press = Some(pressed);
        if now.saturating_duration_since(pressed) >= Duration::from_secs(5)
            || self.pending.is_some()
            || self.hotkey.is_some()
            || self.queued.is_some()
        {
            return;
        }
        self.queued = Some(pressed);
        self.status = "Reading the selected Broker item for this hotkey".into();
    }
    fn expire_activation(&mut self, now: Instant) {
        if self
            .queued
            .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_secs(5))
        {
            self.queued = None;
            self.status = "Hotkey expired waiting for fresh Broker context; press again".into();
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| now.saturating_duration_since(p.began) > Duration::from_secs(120))
        {
            self.pending = None;
            self.stop_input();
            self.status = "Search wait expired".into();
        }
    }
    /// Idle service observes only the chord; OCR and Broker reads activate on demand.
    pub fn poll(
        &mut self,
        ctx: &egui::Context,
        pid: Option<i32>,
        snapshot: Option<&Snapshot>,
        visible: bool,
    ) {
        let Some(pid) = pid else {
            self.cancel();
            return;
        };
        if self.pid != Some(pid) {
            self.cancel();
            self.pid = Some(pid);
        }
        self.expire_activation(Instant::now());
        if self.queued.is_some()
            && let Err(error) = crate::broker_hotkey::session_focused(pid)
        {
            self.queued = None;
            self.status = error;
        }
        while let Some(event) = self.wake.as_mut().and_then(Hotkey::poll) {
            match event {
                Event::Detected => self.status = "Ctrl+Alt+P detected; release keys".into(),
                Event::Pressed(at) => self.accept_press(at, Instant::now()),
                Event::Finished(result) => {
                    self.wake = None;
                    if let Err(error) = result {
                        self.status = error;
                        self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                    }
                }
                _ => {}
            }
        }
        if self.needs_reads(visible) {
            self.tick(ctx, pid, snapshot);
        }
        if !self.needs_reads(visible) {
            self.monitor = None;
            self.screen = None;
            self.candidate = None;
        }
        if self.wake.is_none()
            && self.hotkey.is_none()
            && self.queued.is_none()
            && self.retry_at.is_none_or(|at| Instant::now() >= at)
        {
            match crate::broker_hotkey::listen(pid) {
                Ok(wake) => self.wake = Some(wake),
                Err(error) => {
                    self.status = error;
                    self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, _items: &[ItemKey]) {
        ui.strong("Broker hotkey: Ctrl+Alt+P");
        ui.small("Works from every companion tab. Outside this tab, Broker screen and memory reads run only after a hotkey press or while waiting for your search result.");
        ui.label(
            self.selected
                .as_ref()
                .map_or("Open Tradable Inventory and select an item".into(), |n| {
                    format!("Selected in game: {n}")
                }),
        );
        let action = match &self.action {
            Some(Action::Name(_)) => "Next press: replace focused search text with the item name",
            Some(Action::Price(_, value)) => {
                ui.label(format!("Next press: fill Sell Price with {value}"));
                "Price is lowest observed total minus $1, subtracted once"
            }
            None => "Waiting for a verified screen or search response",
        };
        ui.label(action);
        if self
            .screen
            .as_ref()
            .is_some_and(|s| s.usable_by_me != Some(false))
        {
            ui.label("Turn Usable by me OFF before searching; filtered results will not prepare a price.");
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
        ui.small("No arming button. Focus the intended field before pressing the hotkey. The first press pastes the item name; run the search yourself after the status says it is ready. On returning to the same selected item in Tradable Inventory, the next press fills Sell Price. Neither step presses Enter or submits a listing.");
        ui.small("Selected item is read from the visible English game screen (2560 x 1440). Prices come from fresh memory observations, including your own offers. Set All and Price Ascending; single-item market prices only. The selected inventory quantity is not read: list one item and verify the final amount. Coverage is partial. Turn off Caps Lock and Sticky Keys. Num Lock is supported.");
        ui.separator();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker_memory::Listing;
    #[test]
    fn released_press_is_bounded_and_consumed_once_even_after_delayed_delivery() {
        let now = Instant::now();
        let mut entry = PriceEntry::default();
        entry.accept_press(now - Duration::from_secs(6), now);
        assert!(!entry.needs_reads(false));
        entry.accept_press(now, now);
        assert_eq!(entry.queued, Some(now));
        // Taking the queued press starts exactly one action; replay cannot re-arm it.
        assert_eq!(entry.queued.take(), Some(now));
        entry.accept_press(now, now + Duration::from_millis(100));
        assert!(entry.queued.is_none());
        entry.accept_press(now + Duration::from_secs(1), now + Duration::from_secs(1));
        assert!(entry.needs_reads(false));
        entry.expire_activation(now + Duration::from_secs(6));
        assert!(!entry.needs_reads(false));
    }
    #[test]
    fn hidden_controls_read_only_during_a_bounded_press_or_search() {
        let now = Instant::now();
        let mut entry = PriceEntry::default();
        assert!(!entry.needs_reads(false));
        assert!(entry.needs_reads(true));
        entry.queued = Some(now);
        assert!(entry.needs_reads(false));
        entry.expire_activation(now + Duration::from_secs(5));
        assert!(!entry.needs_reads(false));

        let (pending, _) = fixture();
        entry.pending = Some(pending);
        assert!(entry.needs_reads(false));
        entry.expire_activation(now + Duration::from_secs(121));
        assert!(!entry.needs_reads(false));

        // A prepared price is retained for the next press without background OCR/reads.
        entry.copied = Some(("Synthetic Item".into(), 123, now));
        assert!(!entry.needs_reads(false));
        entry.queued = Some(now);
        entry.poll(&egui::Context::default(), None, None, false);
        assert!(!entry.needs_reads(false));
        assert!(entry.copied.is_none());
    }
    #[test]
    fn presentation_does_not_service_or_cancel_background_controls() {
        let ctx = egui::Context::default();
        let mut entry = PriceEntry {
            pid: Some(42),
            selected: Some("Synthetic Item".into()),
            candidate: Some(("Synthetic Item".into(), 2)),
            copied: Some(("Synthetic Item".into(), 123, Instant::now())),
            ..Default::default()
        };
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| entry.ui(ui, &[]));
        output.textures_delta.clear();
        assert_eq!(entry.pid, Some(42));
        assert_eq!(entry.selected.as_deref(), Some("Synthetic Item"));
        assert_eq!(entry.copied.as_ref().map(|(_, price, _)| *price), Some(123));

        // Pause/disconnect still cancel without needing the tab to render.
        entry.poll(&ctx, None, None, false);
        assert!(entry.pid.is_none());
        assert!(entry.selected.is_none());
        assert!(entry.copied.is_none());
        assert!(entry.candidate.is_none());
    }
    fn fixture() -> (Pending, Snapshot) {
        let at = Instant::now();
        (
            Pending {
                name: "Synthetic Item".into(),
                owner: (42, "Player".into()),
                baseline: 5,
                ready_at: None,
                finished_at: Some(at),
                baseline_verified: true,
                unfiltered_after: Some(5),
                began: at,
            },
            Snapshot {
                entity: 42,
                player_name: "Player".into(),
                request_token: 6,
                response_count: 1,
                observed_at: at,
                listings: vec![Listing {
                    id: 1,
                    item_id: 7,
                    name: Some("Synthetic Item".into()),
                    seller: "Other".into(),
                    total_price: 100,
                    quantity_candidate: 1,
                    is_own_candidate: false,
                }],
            },
        )
    }
    #[test]
    fn requires_fresh_verified_baseline_and_exact_unambiguous_rows() {
        let (mut p, mut s) = fixture();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        p.baseline_verified = false;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        p.baseline_verified = true;
        s.request_token = 5;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.request_token = 6;
        let mut other = s.listings[0].clone();
        other.quantity_candidate = 2;
        s.listings.push(other);
        assert!(p.price(&s, Instant::now()).is_err());
        s.listings.pop();
        s.listings[0].name = None;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.player_name = "Another".into();
        assert!(p.price(&s, Instant::now()).is_err());
    }
    #[test]
    fn refreshed_filtered_response_is_not_a_new_unfiltered_query() {
        let (mut p, mut s) = fixture();
        p.unfiltered_after = Some(s.request_token);
        s.observed_at = Instant::now();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
        s.request_token += 1;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        p.unfiltered_after = None;
        assert_eq!(p.price(&s, Instant::now()).unwrap(), None);
    }
    #[test]
    fn old_memory_and_non_single_stacks_never_prepare_a_price() {
        let (p, mut s) = fixture();
        assert_eq!(
            p.price(&s, Instant::now() + Duration::from_secs(6))
                .unwrap(),
            None
        );
        s.listings[0].quantity_candidate = 16;
        assert!(p.price(&s, Instant::now()).is_err());
    }
    #[test]
    fn undercuts_once_and_rejects_nonpositive_or_excessive_prices() {
        let (p, mut s) = fixture();
        assert_eq!(p.price(&s, Instant::now()).unwrap(), Some(99));
        for value in [0, 1, u64::MAX] {
            s.listings[0].total_price = value;
            assert!(p.price(&s, Instant::now()).is_err());
        }
    }
    fn screen(name: &str) -> Screen {
        Screen {
            panel: Panel::Tradable,
            selected_name: Some(name.into()),
            usable_by_me: Some(false),
            observed_at: Instant::now(),
        }
    }
    #[test]
    fn selection_needs_two_observations_and_changes_clear_price() {
        let mut p = PriceEntry::default();
        p.observe_screen(screen("A"));
        assert!(p.selected.is_none());
        p.observe_screen(screen("A"));
        assert_eq!(p.desired(), Some(Action::Name("A".into())));
        p.copied = Some(("A".into(), 99, Instant::now()));
        assert_eq!(p.desired(), Some(Action::Price("A".into(), 99)));
        p.observe_screen(screen("B"));
        assert!(p.copied.is_none());
        assert!(p.desired().is_none());
        p.observe_screen(screen("B"));
        assert_eq!(p.desired(), Some(Action::Name("B".into())));
    }
    #[test]
    fn price_only_in_matching_tradable_panel() {
        let mut p = PriceEntry::default();
        p.observe_screen(screen("A"));
        p.observe_screen(screen("A"));
        p.copied = Some(("A".into(), 99, Instant::now()));
        let mut browse = screen("A");
        browse.panel = Panel::Browse;
        browse.selected_name = None;
        p.observe_screen(browse);
        assert!(p.desired().is_none());
        p.observe_screen(screen("A"));
        p.observe_screen(screen("A"));
        assert_eq!(p.desired(), Some(Action::Price("A".into(), 99)));
    }
}
