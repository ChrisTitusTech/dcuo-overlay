//! Session-only, partial Broker observations. Missing rows never establish a sale.
use crate::broker_memory::{Listing, Snapshot};
use eframe::egui;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
const MAX_LISTINGS: usize = 2000;
const MAX_AGE: Duration = Duration::from_secs(15 * 60);

struct Observed {
    listing: Listing,
    at: Instant,
}
#[derive(Default)]
pub struct BrokerView {
    owner: Option<(usize, String)>,
    last_token: Option<u64>,
    rows: BTreeMap<u64, Observed>,
    price_entry: crate::broker_price::PriceEntry,
}
impl BrokerView {
    pub fn cancel_hotkey(&mut self) {
        self.price_entry.cancel();
    }
    pub fn clear(&mut self) {
        self.price_entry.reset();
        *self = Self::default();
    }
    fn prune(&mut self, now: Instant) {
        self.rows
            .retain(|_, row| now.saturating_duration_since(row.at) < MAX_AGE);
    }
    pub fn observe(&mut self, snapshot: &Snapshot) {
        self.observe_at(snapshot, Instant::now());
    }
    fn observe_at(&mut self, snapshot: &Snapshot, now: Instant) {
        let owner = (snapshot.entity, snapshot.player_name.clone());
        if self.owner.as_ref() != Some(&owner) {
            self.clear();
            self.owner = Some(owner);
        }
        self.prune(now);
        if self.last_token == Some(snapshot.request_token) {
            for incoming in &snapshot.listings {
                if let Some(saved) = self.rows.get_mut(&incoming.id) {
                    let old = &mut saved.listing;
                    if old.name.is_none()
                        && old.item_id == incoming.item_id
                        && old.seller == incoming.seller
                        && old.total_price == incoming.total_price
                        && old.quantity_candidate == incoming.quantity_candidate
                        && old.is_own_candidate == incoming.is_own_candidate
                    {
                        old.name = incoming.name.clone();
                    }
                }
            }
        }
        // Request tokens increase within one entity/session. A repeated old
        // snapshot must never renew the age of cached rows or repopulate expiry.
        if self
            .last_token
            .is_some_and(|token| snapshot.request_token <= token)
        {
            return;
        }
        self.last_token = Some(snapshot.request_token);
        if now.saturating_duration_since(snapshot.observed_at) >= MAX_AGE {
            return;
        }
        for listing in snapshot.listings.iter().take(MAX_LISTINGS) {
            self.rows.insert(
                listing.id,
                Observed {
                    listing: listing.clone(),
                    at: snapshot.observed_at,
                },
            );
        }
        while self.rows.len() > MAX_LISTINGS {
            let oldest = self
                .rows
                .iter()
                .min_by_key(|(id, row)| (row.at, **id))
                .map(|(id, _)| *id);
            if let Some(id) = oldest {
                self.rows.remove(&id);
            } else {
                break;
            }
        }
    }
    fn own(&self, listing: &Listing) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|(_, name)| listing.seller == *name && listing.is_own_candidate)
    }
    fn competitor(&self, own: &Listing) -> Option<&Observed> {
        let (_, name) = self.owner.as_ref()?;
        self.rows
            .values()
            .filter(|row| {
                row.listing.seller != *name
                    && own.item_id != 0
                    && own
                        .name
                        .as_ref()
                        .is_some_and(|name| !name.trim().is_empty())
                    && row.listing.name == own.name
                    && row.listing.item_id == own.item_id
                    && row.listing.quantity_candidate == own.quantity_candidate
                    && own.quantity_candidate != 0
            })
            .min_by_key(|row| (row.listing.total_price, row.listing.id))
    }
    fn price_offers(&self) -> Vec<crate::broker_price::Offer> {
        use crate::broker_price::{ItemKey, Offer};
        let Some((_, owner)) = &self.owner else {
            return Vec::new();
        };
        let mut offers: BTreeMap<ItemKey, Offer> = BTreeMap::new();
        for row in self.rows.values() {
            let listing = &row.listing;
            let Some(name) = listing.name.as_ref().filter(|s| !s.trim().is_empty()) else {
                continue;
            };
            if listing.seller == *owner
                || listing.item_id == 0
                || listing.quantity_candidate == 0
                || listing.total_price == 0
                || row.at.elapsed() >= MAX_AGE
            {
                continue;
            }
            let item = ItemKey {
                id: listing.item_id,
                name: name.clone(),
                quantity: listing.quantity_candidate,
            };
            let entry = offers.entry(item.clone()).or_insert(Offer {
                item,
                total: listing.total_price,
                at: row.at,
            });
            if listing.total_price < entry.total {
                entry.total = listing.total_price;
                entry.at = row.at;
            }
        }
        offers.into_values().collect()
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, status: &str, pid: Option<i32>) {
        let now = Instant::now();
        self.prune(now);
        ui.heading("Broker");
        ui.label(status);
        let offers = self.price_offers();
        self.price_entry.ui(ui, pid, &offers);
        ui.label("Partial coverage: Broker API unavailable. Visit My Wares, then Browse manually to observe listings.");
        ui.small("Prices are total asking prices. Stack sizes and item variants are not verified. These are comparisons of observed offers, not confirmed sales.");
        if let Some((_, name)) = &self.owner {
            ui.label(format!("Observed character: {name}"));
        }
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} last-observed listings / {} maximum",
                self.rows.len(),
                MAX_LISTINGS
            ));
            if ui.button("Clear observed listings").clicked() {
                self.rows.clear(); // Keep consumed token so old snapshots stay cleared.
                self.price_entry.reset();
            }
        });
        ui.small("Observations expire after 15 minutes and may already have changed in game.");
        ui.separator();
        ui.strong("Observed own listings");
        egui::ScrollArea::both()
            .id_salt("broker-own-scroll")
            .max_height(340.0)
            .show(ui, |ui| {
                egui::Grid::new("broker-own-listings")
                    .striped(true)
                    .show(ui, |ui| {
                        for label in [
                            "Item",
                            "Your total",
                            "Other observed total*",
                            "Difference*",
                            "Last observed",
                        ] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for row in self.rows.values().filter(|row| self.own(&row.listing)) {
                            ui.label(item_name(&row.listing));
                            ui.label(money(row.listing.total_price));
                            if let Some(other) = self.competitor(&row.listing) {
                                ui.label(money(other.listing.total_price))
                                    .on_hover_text(format!(
                                        "Other listing last observed {} ago",
                                        age(now, other.at)
                                    ));
                                ui.label(difference(
                                    row.listing.total_price,
                                    other.listing.total_price,
                                ));
                            } else {
                                ui.label("Unknown");
                                ui.label("Unknown");
                            }
                            ui.label(format!("{} ago", age(now, row.at)));
                            ui.end_row();
                        }
                    });
            });
        ui.small("* Same item type and matching client quantity value; confirm stack size and item details in game.");
        if !self.rows.values().any(|row| self.own(&row.listing)) {
            ui.label("No current own-listing observations. Visit My Wares in game.");
        }
        ui.collapsing("All observed listings", |ui| {
            egui::ScrollArea::both()
                .id_salt("broker-all-scroll")
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("broker-all-listings")
                        .striped(true)
                        .show(ui, |ui| {
                            for label in ["Item", "Seller", "Total price", "Last observed"] {
                                ui.strong(label);
                            }
                            ui.end_row();
                            for row in self.rows.values() {
                                ui.label(item_name(&row.listing));
                                ui.label(&row.listing.seller);
                                ui.label(money(row.listing.total_price));
                                ui.label(format!("{} ago", age(now, row.at)));
                                ui.end_row();
                            }
                        });
                });
        });
    }
}
fn item_name(listing: &Listing) -> String {
    listing
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("Item {}", listing.item_id))
}
fn money(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}
fn difference(own: u64, other: u64) -> String {
    if own < other {
        format!("-{}", money(other - own))
    } else {
        format!("+{}", money(own - other))
    }
}
fn age(now: Instant, at: Instant) -> String {
    let seconds = now.saturating_duration_since(at).as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn listing(id: u64, seller: &str, own: bool, quantity: u32, price: u64) -> Listing {
        Listing {
            id,
            item_id: 42,
            seller: seller.into(),
            total_price: price,
            quantity_candidate: quantity,
            is_own_candidate: own,
            name: Some("Synthetic item".into()),
        }
    }
    fn snapshot(at: Instant, token: u64, listings: Vec<Listing>) -> Snapshot {
        Snapshot {
            entity: 0x10000,
            player_name: "Synthetic Player".into(),
            response_count: listings.len() as u32,
            request_token: token,
            observed_at: at,
            listings,
        }
    }
    #[test]
    fn price_choices_include_new_items_exclude_self_and_keep_stack_groups_separate() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        let mut unknown = listing(9, "Other", false, 1, 1);
        unknown.name = None;
        view.observe_at(
            &snapshot(
                at,
                1,
                vec![
                    listing(1, "Synthetic Player", true, 1, 1),
                    listing(2, "Synthetic Player", false, 1, 2),
                    listing(3, "Other", false, 1, 100),
                    listing(4, "Other", false, 1, 80),
                    listing(5, "Other", false, 2, 120),
                    listing(6, "Other", false, 0, 1),
                    listing(7, "Other", false, 1, 0),
                    unknown,
                ],
            ),
            at,
        );
        let offers = view.price_offers();
        assert_eq!(offers.len(), 2);
        assert_eq!(offers[0].total, 80);
        assert_eq!(offers[0].item.quantity, 1);
        assert_eq!(offers[1].total, 120);
        view.prune(at + MAX_AGE);
        assert!(view.price_offers().is_empty());
    }
    #[test]
    fn late_names_enrich_matching_rows_without_refreshing_or_reviving_prices() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        let mut row = listing(1, "Other", false, 1, 10);
        row.name = None;
        view.observe_at(&snapshot(at, 1, vec![row.clone()]), at);
        row.name = Some("Resolved item".into());
        let later = at + Duration::from_secs(60);
        let enriched = snapshot(later, 1, vec![row.clone()]);
        view.observe_at(&enriched, later);
        assert_eq!(view.rows[&1].listing.name, row.name);
        assert_eq!(view.rows[&1].at, at);
        view.rows.get_mut(&1).unwrap().listing.name = None;
        row.total_price += 1;
        view.observe_at(&snapshot(later, 1, vec![row]), later);
        assert!(view.rows[&1].listing.name.is_none());
        view.observe_at(&enriched, at + MAX_AGE);
        assert!(view.rows.is_empty());
    }
    #[test]
    fn money_groups_full_range_and_signed_differences() {
        assert_eq!(money(0), "0");
        assert_eq!(money(999), "999");
        assert_eq!(money(1000), "1,000");
        assert_eq!(money(123456789), "123,456,789");
        assert_eq!(money(u64::MAX), "18,446,744,073,709,551,615");
        assert_eq!(difference(0, u64::MAX), "-18,446,744,073,709,551,615");
        assert_eq!(difference(5000, 1000), "+4,000");
        assert_eq!(difference(1, 1), "+0");
    }
    #[test]
    fn comparisons_require_nonzero_item_and_matching_present_names() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        view.observe_at(
            &snapshot(
                at,
                1,
                vec![
                    listing(1, "Synthetic Player", true, 1, 100),
                    listing(2, "Other", false, 1, 50),
                ],
            ),
            at,
        );
        let mut own = view.rows[&1].listing.clone();
        assert!(view.competitor(&own).is_some());
        own.name = None;
        assert!(view.competitor(&own).is_none());
        own.name = Some("Variant".into());
        assert!(view.competitor(&own).is_none());
        own.name = Some("Synthetic item".into());
        own.item_id = 0;
        assert!(view.competitor(&own).is_none());
        view.rows.get_mut(&2).unwrap().listing.name = None;
        own.item_id = 42;
        assert!(view.competitor(&own).is_none());
    }
    #[test]
    fn excludes_every_self_seller_and_requires_both_ownership_signals() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        view.observe_at(
            &snapshot(
                at,
                1,
                vec![
                    listing(1, "Synthetic Player", true, 1, 100),
                    listing(2, "Synthetic Player", false, 1, 1),
                    listing(3, "Other", true, 1, 70),
                ],
            ),
            at,
        );
        assert!(view.own(&view.rows[&1].listing));
        assert!(!view.own(&view.rows[&2].listing));
        assert!(!view.own(&view.rows[&3].listing));
        assert_eq!(
            view.competitor(&view.rows[&1].listing).unwrap().listing.id,
            3
        );
    }
    #[test]
    fn changed_entity_or_name_discards_previous_rows() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        let mut first = snapshot(at, 1, vec![listing(1, "Other", false, 1, 10)]);
        view.observe_at(&first, at);
        first.entity += 1;
        first.listings.clear();
        view.observe_at(&first, at);
        assert!(view.rows.is_empty());
        first.listings.push(listing(2, "Other", false, 1, 10));
        first.request_token = 2;
        view.observe_at(&first, at);
        first.player_name = "Different".into();
        first.listings.clear();
        view.observe_at(&first, at);
        assert!(view.rows.is_empty());
    }
    #[test]
    fn repeated_and_older_tokens_do_not_renew_age_or_revive_expiry() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        let mut first = snapshot(at, 1, vec![listing(1, "Other", false, 1, 10)]);
        view.observe_at(&first, at);
        first.observed_at = at + Duration::from_secs(60);
        view.observe_at(&first, first.observed_at);
        assert_eq!(view.rows[&1].at, at);
        view.observe_at(&snapshot(at, 2, vec![]), at);
        view.observe_at(&first, first.observed_at);
        assert_eq!(view.rows[&1].at, at);
        view.observe_at(&first, at + MAX_AGE);
        assert!(view.rows.is_empty());
    }
    #[test]
    fn partial_pages_retain_previous_listings_and_bounded_cache_evicts_oldest() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        view.observe_at(
            &snapshot(at, 1, vec![listing(1, "Other", false, 1, 10)]),
            at,
        );
        view.observe_at(
            &snapshot(at, 2, vec![listing(2, "Other", false, 1, 20)]),
            at,
        );
        assert_eq!(view.rows.len(), 2);
        let newer = at + Duration::from_secs(1);
        let rows = (3..2003)
            .map(|id| listing(id, "Other", false, 1, 30))
            .collect();
        view.observe_at(&snapshot(newer, 3, rows), newer);
        assert_eq!(view.rows.len(), MAX_LISTINGS);
        assert!(!view.rows.contains_key(&1));
        assert!(!view.rows.contains_key(&2));
    }
    #[test]
    fn different_or_zero_candidate_stack_fields_are_not_compared() {
        let at = Instant::now();
        let mut view = BrokerView::default();
        view.observe_at(
            &snapshot(
                at,
                1,
                vec![
                    listing(1, "Synthetic Player", true, 2, 100),
                    listing(2, "Other", false, 1, 10),
                ],
            ),
            at,
        );
        assert!(view.competitor(&view.rows[&1].listing).is_none());
        let zero = listing(9, "Synthetic Player", true, 0, 100);
        assert!(view.competitor(&zero).is_none());
    }
}
