//! Transport-independent sequential Broker observations. No input, network or game calls.
//! A controller must confirm the query/response token before accepting a page.
use crate::broker_memory::Snapshot;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
const MAX_TARGETS: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub item_id: u32,
    pub name: String,
    pub quantity_candidate: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbortReason {
    OwnerChanged,
    TimedOut,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Complete,
    Cancelled,
    Aborted(AbortReason),
}
#[derive(Clone, Debug)]
pub struct PriceObservation {
    pub target: Target,
    /// Lowest matching competitor asking total on this accepted page, not a market minimum.
    pub lowest_observed_competing_total: Option<u64>,
    /// None means the controller explicitly skipped this target without a proven response.
    pub request_token: Option<u64>,
    pub matching_rows: usize,
    pub competing_rows: usize,
    pub recorded_at: Instant,
}
#[derive(Clone, Debug)]
struct Request {
    baseline_token: u64,
    started_at: Instant,
}
#[derive(Debug)]
pub struct Scan {
    owner: String,
    targets: Vec<Target>,
    index: usize,
    request: Option<Request>,
    target_started_at: Instant,
    results: Vec<PriceObservation>,
    state: State,
}
fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn same_owner(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}
impl Scan {
    /// Preserve first occurrence order; duplicate identity/name/quantity targets are scanned once.
    pub fn start(owner: String, targets: Vec<Target>, now: Instant) -> Result<Self, String> {
        if !valid_text(&owner, 128) {
            return Err("Invalid Broker scan owner".into());
        }
        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for target in targets {
            if target.item_id == 0
                || target.quantity_candidate == 0
                || !valid_text(&target.name, 512)
            {
                return Err("Invalid Broker scan target".into());
            }
            if seen.insert(target.clone()) {
                if unique.len() == MAX_TARGETS {
                    return Err("Broker scan exceeds 100 unique targets".into());
                }
                unique.push(target);
            }
        }
        let state = if unique.is_empty() {
            State::Complete
        } else {
            State::Running
        };
        Ok(Self {
            owner,
            targets: unique,
            index: 0,
            request: None,
            target_started_at: now,
            results: Vec::new(),
            state,
        })
    }
    pub fn current(&self) -> Option<&Target> {
        (self.state == State::Running)
            .then(|| self.targets.get(self.index))
            .flatten()
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn results(&self) -> &[PriceObservation] {
        &self.results
    }
    /// Number resolved or explicitly skipped, and number of unique targets.
    pub fn progress(&self) -> (usize, usize) {
        (self.index, self.targets.len())
    }
    pub fn waiting_for_response(&self) -> bool {
        self.state == State::Running && self.request.is_some()
    }
    /// `baseline_token` is the last response token observed BEFORE the controller issues its query.
    /// This records a request intent only; it never sends a request.
    pub fn mark_requested(&mut self, baseline_token: u64, now: Instant) -> Result<(), String> {
        if self.tick(now) || self.state != State::Running {
            return Err("Broker scan is not running".into());
        }
        if self.request.is_some() {
            return Err("Broker target already has a pending request".into());
        }
        if now < self.target_started_at {
            return Err("Broker request time precedes target".into());
        }
        self.request = Some(Request {
            baseline_token,
            started_at: now,
        });
        self.target_started_at = now;
        Ok(())
    }
    /// Return true only when this target advances. The controller must independently
    /// prove `expected_request_token` belongs to its current query, not just pass the
    /// latest arbitrary Broker token. Empty/unrelated pages remain pending.
    pub fn accept_response(
        &mut self,
        snapshot: &Snapshot,
        expected_request_token: u64,
        now: Instant,
    ) -> bool {
        if self.tick(now) || self.state != State::Running {
            return false;
        }
        if !same_owner(&self.owner, &snapshot.player_name) {
            self.state = State::Aborted(AbortReason::OwnerChanged);
            self.request = None;
            return false;
        }
        let Some(request) = &self.request else {
            return false;
        };
        if expected_request_token == 0
            || expected_request_token <= request.baseline_token
            || snapshot.request_token != expected_request_token
            || snapshot.observed_at < request.started_at
            || snapshot.observed_at > now
            || snapshot.listings.len() > MAX_TARGETS
        {
            return false;
        }
        let Some(target) = self.targets.get(self.index) else {
            return false;
        };
        let mut matching_rows = 0;
        let mut competing_rows = 0;
        let mut lowest = None;
        for row in &snapshot.listings {
            if row.item_id != target.item_id
                || row.name.as_deref() != Some(target.name.as_str())
                || row.quantity_candidate != target.quantity_candidate
            {
                continue;
            }
            matching_rows += 1;
            if same_owner(&self.owner, &row.seller) {
                continue;
            }
            competing_rows += 1;
            lowest =
                Some(lowest.map_or(row.total_price, |before: u64| before.min(row.total_price)));
        }
        if matching_rows == 0 {
            return false;
        }
        self.advance(
            PriceObservation {
                target: target.clone(),
                lowest_observed_competing_total: lowest,
                request_token: Some(expected_request_token),
                matching_rows,
                competing_rows,
                recorded_at: now,
            },
            now,
        );
        true
    }
    /// Explicitly record unknown and advance when the controller cannot prove a result.
    pub fn skip_current(&mut self, now: Instant) -> Result<(), String> {
        if now < self.target_started_at {
            return Err("Broker skip time precedes target".into());
        }
        let target = self.current().ok_or("Broker scan is not running")?.clone();
        self.advance(
            PriceObservation {
                target,
                lowest_observed_competing_total: None,
                request_token: None,
                matching_rows: 0,
                competing_rows: 0,
                recorded_at: now,
            },
            now,
        );
        Ok(())
    }
    fn advance(&mut self, result: PriceObservation, now: Instant) {
        self.results.push(result);
        self.index += 1;
        self.request = None;
        self.target_started_at = now;
        if self.index == self.targets.len() {
            self.state = State::Complete;
        }
    }
    /// Poll from the controller. A stalled target aborts after twenty seconds;
    /// no unknown page is silently treated as a successful search.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.state == State::Running
            && now
                .checked_duration_since(self.target_started_at)
                .is_some_and(|elapsed| elapsed >= TIMEOUT)
        {
            self.state = State::Aborted(AbortReason::TimedOut);
            self.request = None;
            return true;
        }
        false
    }
    pub fn cancel(&mut self) {
        if self.state == State::Running {
            self.state = State::Cancelled;
            self.request = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker_memory::Listing;
    fn target(id: u32) -> Target {
        Target {
            item_id: id,
            name: format!("Item {id}"),
            quantity_candidate: 1,
        }
    }
    fn row(id: u32, seller: &str, price: u64) -> Listing {
        Listing {
            id: price,
            item_id: id,
            name: Some(format!("Item {id}")),
            seller: seller.into(),
            total_price: price,
            is_own_candidate: false,
            quantity_candidate: 1,
        }
    }
    fn page(token: u64, rows: Vec<Listing>, now: Instant) -> Snapshot {
        Snapshot {
            entity: 0x10000,
            player_name: "Owner".into(),
            listings: rows,
            request_token: token,
            response_count: 100,
            observed_at: now,
        }
    }
    #[test]
    fn deduplicates_exact_targets_preserving_order_and_bounds() {
        let now = Instant::now();
        let mut other = target(1);
        other.quantity_candidate = 2;
        let scan = Scan::start(
            "Owner".into(),
            vec![target(1), target(1), other, target(2)],
            now,
        )
        .unwrap();
        assert_eq!(scan.progress(), (0, 3));
        assert_eq!(scan.current(), Some(&target(1)));
        assert!(Scan::start("Owner".into(), (1..=101).map(target).collect(), now).is_err());
        assert_eq!(
            Scan::start("Owner".into(), vec![], now).unwrap().state(),
            &State::Complete
        );
    }
    #[test]
    fn sequential_pages_record_lowest_observed_competitor_and_exclude_self() {
        let now = Instant::now();
        let mut scan = Scan::start("Owner".into(), vec![target(1), target(2)], now).unwrap();
        scan.mark_requested(7, now).unwrap();
        let snapshot = page(
            8,
            vec![
                row(1, "OWNER", 1),
                row(1, "Seller", 90),
                row(1, "Other", 80),
                row(2, "Other", 2),
            ],
            now,
        );
        assert!(scan.accept_response(&snapshot, 8, now));
        assert_eq!(scan.current(), Some(&target(2)));
        assert_eq!(scan.results()[0].lowest_observed_competing_total, Some(80));
        assert_eq!(
            (
                scan.results()[0].matching_rows,
                scan.results()[0].competing_rows
            ),
            (3, 2)
        );
        assert!(!scan.accept_response(&snapshot, 8, now));
        scan.mark_requested(8, now).unwrap();
        assert!(scan.accept_response(&page(9, vec![row(2, "Owner", 40)], now), 9, now));
        assert_eq!(scan.results()[1].lowest_observed_competing_total, None);
        assert_eq!(scan.state(), &State::Complete);
    }
    #[test]
    fn old_wrong_unrelated_empty_quantity_or_name_mismatches_do_not_advance() {
        let now = Instant::now();
        let mut scan = Scan::start("Owner".into(), vec![target(1)], now).unwrap();
        scan.mark_requested(7, now).unwrap();
        assert!(!scan.accept_response(&page(7, vec![row(1, "Seller", 10)], now), 7, now));
        assert!(!scan.accept_response(&page(8, vec![row(1, "Seller", 10)], now), 9, now));
        assert!(!scan.accept_response(&page(8, vec![row(2, "Seller", 10)], now), 8, now));
        assert!(!scan.accept_response(&page(8, vec![], now), 8, now));
        let mut wrong = row(1, "Seller", 10);
        wrong.quantity_candidate = 2;
        assert!(!scan.accept_response(&page(8, vec![wrong], now), 8, now));
        let mut wrong = row(1, "Seller", 10);
        wrong.name = Some("Similar Item 1".into());
        assert!(!scan.accept_response(&page(8, vec![wrong], now), 8, now));
        let stale = page(8, vec![row(1, "Seller", 10)], now - Duration::from_secs(1));
        assert!(!scan.accept_response(&stale, 8, now));
        assert_eq!(scan.progress(), (0, 1));
        assert!(scan.results().is_empty());
        scan.skip_current(now).unwrap();
        assert_eq!(scan.results()[0].request_token, None);
    }
    #[test]
    fn cancellation_timeout_and_owner_change_stop_without_fabricated_results() {
        let now = Instant::now();
        for reason in 0..3 {
            let mut scan = Scan::start("Owner".into(), vec![target(1)], now).unwrap();
            scan.mark_requested(7, now).unwrap();
            match reason {
                0 => scan.cancel(),
                1 => {
                    assert!(scan.tick(now + TIMEOUT));
                }
                _ => {
                    let mut changed = page(8, vec![row(1, "Seller", 10)], now);
                    changed.player_name = "Another".into();
                    assert!(!scan.accept_response(&changed, 8, now));
                }
            }
            assert!(scan.current().is_none());
            assert!(scan.results().is_empty());
            assert!(!scan.accept_response(&page(8, vec![row(1, "Seller", 10)], now), 8, now));
            assert_eq!(
                scan.state(),
                &match reason {
                    0 => State::Cancelled,
                    1 => State::Aborted(AbortReason::TimedOut),
                    _ => State::Aborted(AbortReason::OwnerChanged),
                }
            );
        }
    }
    #[test]
    fn request_cannot_reset_timeout_and_explicit_skip_starts_next_target() {
        let now = Instant::now();
        let mut scan = Scan::start("Owner".into(), vec![target(1), target(2)], now).unwrap();
        scan.mark_requested(0, now).unwrap();
        assert!(
            scan.mark_requested(1, now + Duration::from_secs(19))
                .is_err()
        );
        assert!(scan.tick(now + TIMEOUT));
        assert_eq!(scan.progress(), (0, 2));
        let mut scan = Scan::start("Owner".into(), vec![target(1), target(2)], now).unwrap();
        scan.skip_current(now + Duration::from_secs(10)).unwrap();
        assert!(!scan.tick(now + Duration::from_secs(29)));
        assert_eq!(scan.current(), Some(&target(2)));
    }
}
