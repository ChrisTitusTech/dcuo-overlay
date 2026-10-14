//! Bounded API reference lookup, independent of live memory observations.
//!
//! Contract: dcuobot-api CharacterApi and CharacterResponse DTOs. Transport bounds
//! use ureq 3.4.2's global timeout and decoded body limit. Names compare exactly
//! except ASCII letter case; world IDs must match exactly. API stats are maxima,
//! never current resource readings. The API provides no reference freshness time.

use serde::Deserialize;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};
use url::Url;

pub const DEFAULT_BASE_URL: &str = "https://dcuo.bot/api";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CharacterReference {
    pub id: String,
    pub name: String,
    pub world_id: String,
    pub power: Option<String>,
    pub movement: Option<String>,
    pub alignment: Option<String>,
    pub combat_rating: Option<u32>,
    pub skill_points: Option<u32>,
    pub health: Option<u64>,
    pub power_max: Option<u64>,
    pub league: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    character_id: String,
    name: String,
    world_id: String,
    power_type: Option<String>,
    movement_mode: Option<String>,
    alignment: Option<String>,
    combat_rating: Option<u32>,
    skill_points: Option<u32>,
    stats: Option<Stats>,
    guild: Option<Guild>,
}

#[derive(Deserialize)]
struct Stats {
    health: Option<u64>,
    power: Option<u64>,
}

#[derive(Deserialize)]
struct Guild {
    name: Option<String>,
}

fn request_url(base_url: &str, name: &str, world: &str) -> Result<Url, String> {
    // The service deliberately switches to wildcard search below 3 characters.
    if name.chars().count() < 3
        || name.chars().count() > 128
        || name.trim() != name
        || name.chars().any(|c| c.is_control() || "*?^$\\".contains(c))
    {
        return Err("Enter an exact character name of 3 to 128 characters.".into());
    }
    if !matches!(world, "2" | "4" | "10" | "11" | "5001") {
        return Err("Select a supported world ID.".into());
    }
    let mut url = Url::parse(base_url).map_err(|_| "Invalid API base URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid API base URL.".into());
    }
    let path = format!("{}/v1/census/characters", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.query_pairs_mut()
        .append_pair("name", name)
        .append_pair("worldId", world);
    Ok(url)
}

fn decode(bytes: &[u8], name: &str, world: &str) -> Result<CharacterReference, String> {
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("API response exceeds the size limit.".into());
    }
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| "API returned an invalid character response.".to_string())?;
    if response.character_id.is_empty()
        || !response.character_id.bytes().all(|c| c.is_ascii_digit())
        || !response.name.eq_ignore_ascii_case(name)
        || response.world_id != world
    {
        return Err("API character identity does not match the requested name and world.".into());
    }
    Ok(CharacterReference {
        id: response.character_id,
        name: response.name,
        world_id: response.world_id,
        power: response.power_type,
        movement: response.movement_mode,
        alignment: response.alignment,
        combat_rating: response.combat_rating,
        skill_points: response.skill_points,
        health: response.stats.as_ref().and_then(|s| s.health),
        power_max: response.stats.and_then(|s| s.power),
        league: response.guild.and_then(|g| g.name),
    })
}

const CACHE_TTL: Duration = Duration::from_secs(300);
const REQUEST_BUDGET: Duration = Duration::from_secs(8);
const CACHE_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupSource {
    Network,
    Cache,
}
#[derive(Clone, Debug)]
pub struct LookupResult {
    pub reference: CharacterReference,
    pub received_at: Instant,
    pub source: LookupSource,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupErrorKind {
    InvalidRequest,
    Cancelled,
    NotFound,
    RateLimited,
    Redirect,
    Upstream,
    Transport,
    InvalidResponse,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookupError {
    pub kind: LookupErrorKind,
    pub retry_after: Option<Duration>,
}
impl LookupError {
    fn new(kind: LookupErrorKind) -> Self {
        Self {
            kind,
            retry_after: None,
        }
    }
    fn transient(&self) -> bool {
        matches!(
            self.kind,
            LookupErrorKind::Transport | LookupErrorKind::Upstream
        )
    }
}
impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.kind == LookupErrorKind::RateLimited
            && let Some(wait) = self.retry_after
        {
            let seconds = wait
                .as_secs()
                .saturating_add(u64::from(wait.subsec_nanos() > 0));
            return write!(f, "API rate limit reached; retry in {seconds} seconds.");
        }
        f.write_str(match self.kind {
            LookupErrorKind::InvalidRequest => "Invalid API URL, exact character name or world ID.",
            LookupErrorKind::Cancelled => "API request cancelled.",
            LookupErrorKind::NotFound => {
                "No API character was found for that exact name and world."
            }
            LookupErrorKind::RateLimited => "API rate limit reached; try again after the cooldown.",
            LookupErrorKind::Redirect => "API redirects are not accepted.",
            LookupErrorKind::Upstream => "API returned an unsuccessful status.",
            LookupErrorKind::Transport => "API request failed or timed out.",
            LookupErrorKind::InvalidResponse => {
                "API returned an invalid character response or exceeded the size limit."
            }
        })
    }
}
impl std::error::Error for LookupError {}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    base: String,
    name: String,
    world: String,
}
struct CacheEntry {
    key: CacheKey,
    result: LookupResult,
}
struct Cooldown {
    base: String,
    started_at: Instant,
    duration: Duration,
    kind: LookupErrorKind,
}
/// One API worker owns this bounded in-memory cache and service cooldowns.
/// Cache receipt time never implies freshness of upstream Census data.
#[derive(Default)]
pub struct ReferenceClient {
    cache: VecDeque<CacheEntry>,
    cooldowns: VecDeque<Cooldown>,
}
impl ReferenceClient {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn lookup(
        &mut self,
        base: &str,
        name: &str,
        world: &str,
        cancel: &AtomicBool,
    ) -> Result<LookupResult, LookupError> {
        self.lookup_attempts(base, name, world, cancel, 3)
    }
    fn cached(&mut self, key: &CacheKey, now: Instant) -> Option<LookupResult> {
        self.cache.retain(|entry| {
            now.checked_duration_since(entry.result.received_at)
                .is_some_and(|age| age < CACHE_TTL)
        });
        let index = self.cache.iter().position(|entry| &entry.key == key)?;
        let entry = self.cache.remove(index)?;
        let mut result = entry.result.clone();
        result.source = LookupSource::Cache;
        self.cache.push_back(entry);
        Some(result)
    }
    fn insert(&mut self, key: CacheKey, result: LookupResult) {
        self.cache.retain(|entry| entry.key != key);
        if self.cache.len() == CACHE_CAPACITY {
            self.cache.pop_front();
        }
        self.cache.push_back(CacheEntry { key, result });
    }
    fn cooldown(&mut self, base: &str, now: Instant) -> Option<LookupError> {
        self.cooldowns
            .retain(|entry| now.saturating_duration_since(entry.started_at) < entry.duration);
        self.cooldowns
            .iter()
            .find(|entry| entry.base == base)
            .map(|entry| LookupError {
                kind: entry.kind,
                retry_after: Some(
                    entry
                        .duration
                        .saturating_sub(now.saturating_duration_since(entry.started_at)),
                ),
            })
    }
    fn set_cooldown(
        &mut self,
        base: &str,
        duration: Duration,
        kind: LookupErrorKind,
        now: Instant,
    ) {
        self.cooldowns.retain(|entry| entry.base != base);
        if self.cooldowns.len() == CACHE_CAPACITY {
            self.cooldowns.pop_front();
        }
        self.cooldowns.push_back(Cooldown {
            base: base.into(),
            started_at: now,
            duration,
            kind,
        });
    }
    fn lookup_attempts(
        &mut self,
        base: &str,
        name: &str,
        world: &str,
        cancel: &AtomicBool,
        attempts: usize,
    ) -> Result<LookupResult, LookupError> {
        check_cancel(cancel)?;
        if base.len() > 2048 {
            return Err(LookupError::new(LookupErrorKind::InvalidRequest));
        }
        let url = request_url(base, name, world)
            .map_err(|_| LookupError::new(LookupErrorKind::InvalidRequest))?;
        // The canonical endpoint excludes the query; slash/default-port/case
        // aliases must not bypass service cooldowns or fragment the cache.
        let mut endpoint = url.clone();
        endpoint.set_query(None);
        let base = endpoint.as_str();
        let key = CacheKey {
            base: base.into(),
            name: name.into(),
            world: world.into(),
        };
        let started = Instant::now();
        if let Some(result) = self.cached(&key, started) {
            return Ok(result);
        }
        if let Some(error) = self.cooldown(base, started) {
            return Err(error);
        }
        for attempt in 0..attempts {
            check_cancel(cancel)?;
            let remaining = REQUEST_BUDGET.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(LookupError::new(LookupErrorKind::Transport));
            }
            match fetch(&url, name, world, remaining, cancel) {
                Ok(reference) => {
                    check_cancel(cancel)?;
                    let result = LookupResult {
                        reference,
                        received_at: Instant::now(),
                        source: LookupSource::Network,
                    };
                    self.cooldowns.retain(|entry| entry.base != base);
                    self.insert(key, result.clone());
                    return Ok(result);
                }
                Err(error) => {
                    check_cancel(cancel)?;
                    let backoff = Duration::from_millis(250 * (1 << attempt.min(2)));
                    let wait = error.retry_after.unwrap_or(backoff);
                    if error.kind == LookupErrorKind::RateLimited || error.transient() {
                        self.set_cooldown(base, wait, error.kind, Instant::now());
                    }
                    if !error.transient()
                        || attempt + 1 == attempts
                        || wait >= REQUEST_BUDGET.saturating_sub(started.elapsed())
                    {
                        return Err(error);
                    }
                    wait_cancelled(wait, cancel)?;
                }
            }
        }
        Err(LookupError::new(LookupErrorKind::Transport))
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<(), LookupError> {
    if cancel.load(Ordering::Acquire) {
        Err(LookupError::new(LookupErrorKind::Cancelled))
    } else {
        Ok(())
    }
}
fn wait_cancelled(duration: Duration, cancel: &AtomicBool) -> Result<(), LookupError> {
    let started = Instant::now();
    while started.elapsed() < duration {
        check_cancel(cancel)?;
        std::thread::sleep(
            duration
                .saturating_sub(started.elapsed())
                .min(Duration::from_millis(20)),
        );
    }
    check_cancel(cancel)
}
fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(date.duration_since(now).unwrap_or(Duration::ZERO))
}
fn fetch(
    url: &Url,
    name: &str,
    world: &str,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<CharacterReference, LookupError> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .get(url.as_str())
        .header(
            "User-Agent",
            concat!("DCUO-Companion/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/json")
        .call()
        .map_err(|_| LookupError::new(LookupErrorKind::Transport))?;
    check_cancel(cancel)?;
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| retry_after(value, SystemTime::now()));
    let kind = match response.status().as_u16() {
        200 => None,
        404 => Some(LookupErrorKind::NotFound),
        429 => Some(LookupErrorKind::RateLimited),
        300..=399 => Some(LookupErrorKind::Redirect),
        500..=599 | 408 => Some(LookupErrorKind::Upstream),
        _ => Some(LookupErrorKind::InvalidRequest),
    };
    if let Some(kind) = kind {
        return Err(LookupError {
            kind,
            retry_after: if kind == LookupErrorKind::RateLimited {
                Some(retry.unwrap_or(Duration::from_secs(60)))
            } else {
                retry
            },
        });
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|error| {
            LookupError::new(if matches!(error, ureq::Error::BodyExceedsLimit(_)) {
                LookupErrorKind::InvalidResponse
            } else {
                LookupErrorKind::Transport
            })
        })?;
    check_cancel(cancel)?;
    decode(&bytes, name, world).map_err(|_| LookupError::new(LookupErrorKind::InvalidResponse))
}
/// Compatibility helper: one uncached, bounded request with the original string errors.
/// Long-lived workers should retain ReferenceClient to reuse cache and cooldown state.
pub fn lookup(base_url: &str, name: &str, world: &str) -> Result<CharacterReference, String> {
    ReferenceClient::new()
        .lookup_attempts(base_url, name, world, &AtomicBool::new(false), 1)
        .map(|result| result.reference)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_once(
        status: &str,
        headers: &str,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response_headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
            body.len()
        );
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0 && request.len() < 8192);
                request.extend_from_slice(&chunk[..count]);
            }
            stream.write_all(response_headers.as_bytes()).unwrap();
            // A limited reader may close before accepting the entire body.
            let _ = stream.write_all(&body);
        });
        (url, thread)
    }

    fn synthetic_result(now: Instant) -> LookupResult {
        LookupResult {
            reference: decode(
                br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#,
                "Synthetic",
                "2",
            )
            .unwrap(),
            received_at: now,
            source: LookupSource::Network,
        }
    }
    fn key(base: &str, name: &str, world: &str) -> CacheKey {
        CacheKey {
            base: base.into(),
            name: name.into(),
            world: world.into(),
        }
    }
    #[test]
    fn bounded_cache_preserves_receipt_and_exact_identity_and_expires() {
        let now = Instant::now();
        let mut client = ReferenceClient::new();
        let original = key("https://synthetic.invalid/api", "Synthetic", "2");
        client.insert(original.clone(), synthetic_result(now));
        let hit = client
            .cached(&original, now + Duration::from_secs(299))
            .unwrap();
        assert_eq!(hit.received_at, now);
        assert_eq!(hit.source, LookupSource::Cache);
        for different in [
            key("https://different.invalid/api", "Synthetic", "2"),
            key("https://synthetic.invalid/api", "Other", "2"),
            key("https://synthetic.invalid/api", "Synthetic", "4"),
        ] {
            assert!(client.cached(&different, now).is_none());
        }
        assert!(client.cached(&original, now + CACHE_TTL).is_none());
        for index in 0..9 {
            client.insert(
                key("base", &format!("name{index}"), "2"),
                synthetic_result(now),
            );
        }
        assert_eq!(client.cache.len(), 8);
        assert!(client.cached(&key("base", "name0", "2"), now).is_none());
    }
    #[test]
    fn retry_after_dates_deltas_and_cooldowns_use_original_clock() {
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(retry_after("12", wall), Some(Duration::from_secs(12)));
        assert_eq!(
            retry_after(
                &httpdate::fmt_http_date(wall + Duration::from_secs(60)),
                wall
            ),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            retry_after(
                &httpdate::fmt_http_date(wall - Duration::from_secs(60)),
                wall
            ),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_after("private-invalid", wall), None);
        let now = Instant::now();
        let mut client = ReferenceClient::new();
        client.set_cooldown(
            "base",
            Duration::from_secs(30),
            LookupErrorKind::RateLimited,
            now,
        );
        assert_eq!(
            client
                .cooldown("base", now + Duration::from_secs(10))
                .unwrap()
                .retry_after,
            Some(Duration::from_secs(20))
        );
        assert!(client.cooldown("other", now).is_none());
        assert!(
            client
                .cooldown("base", now + Duration::from_secs(30))
                .is_none()
        );
    }
    #[test]
    fn persistent_client_caches_network_receipt_and_honors_service_cooldown() {
        let cancel = AtomicBool::new(false);
        let mut client = ReferenceClient::new();
        let (url, server) = serve_once(
            "200 OK",
            "",
            br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#.to_vec(),
        );
        let first = client.lookup(&url, "Synthetic", "2", &cancel).unwrap();
        server.join().unwrap();
        let cached = client
            .lookup(&format!("{url}/"), "Synthetic", "2", &cancel)
            .unwrap();
        assert_eq!(cached.received_at, first.received_at);
        assert_eq!(cached.source, LookupSource::Cache);
        cancel.store(true, Ordering::Release);
        assert_eq!(
            client
                .lookup(&url, "Synthetic", "2", &cancel)
                .unwrap_err()
                .kind,
            LookupErrorKind::Cancelled
        );
        cancel.store(false, Ordering::Release);
        let (url, server) = serve_once(
            "429 Too Many Requests",
            "Retry-After: 30\r\n",
            b"private body".to_vec(),
        );
        let error = client.lookup(&url, "Synthetic", "2", &cancel).unwrap_err();
        server.join().unwrap();
        assert_eq!(error.kind, LookupErrorKind::RateLimited);
        assert_eq!(error.retry_after, Some(Duration::from_secs(30)));
        let next = client
            .lookup(&format!("{url}/"), "OtherName", "4", &cancel)
            .unwrap_err();
        assert_eq!(next.kind, LookupErrorKind::RateLimited);
        assert!(next.retry_after.unwrap() > Duration::from_secs(29));
        assert!(!next.to_string().contains("private"));
    }
    #[test]
    fn cancellation_interrupts_backoff_promptly() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            trigger.store(true, Ordering::Release);
        });
        let started = Instant::now();
        assert_eq!(
            wait_cancelled(Duration::from_secs(5), &cancel)
                .unwrap_err()
                .kind,
            LookupErrorKind::Cancelled
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        thread.join().unwrap();
    }
    #[test]
    fn transient_retry_succeeds_without_changing_identity_or_exceeding_attempt_cap() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (status, body) in [
                ("503 Service Unavailable", b"private failure".as_slice()),
                (
                    "200 OK",
                    br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#.as_slice(),
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let _ = stream.read(&mut bytes).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let mut client = ReferenceClient::new();
        assert_eq!(
            client
                .lookup(&url, "Synthetic", "2", &AtomicBool::new(false))
                .unwrap()
                .reference
                .id,
            "123"
        );
        server.join().unwrap();
        assert!(client.cooldown(&url, Instant::now()).is_none());
    }
    #[test]
    fn transient_failures_stop_after_three_attempts_and_long_retry_after_does_not_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let _ = stream.read(&mut bytes).unwrap();
                stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        });
        let started = Instant::now();
        let error = ReferenceClient::new()
            .lookup(&url, "Synthetic", "2", &AtomicBool::new(false))
            .unwrap_err();
        assert_eq!(error.kind, LookupErrorKind::Upstream);
        assert!(started.elapsed() < REQUEST_BUDGET);
        server.join().unwrap();
        let (url, server) =
            serve_once("503 Service Unavailable", "Retry-After: 60\r\n", Vec::new());
        let started = Instant::now();
        let mut client = ReferenceClient::new();
        let error = client
            .lookup(&url, "Synthetic", "2", &AtomicBool::new(false))
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(error.retry_after, Some(Duration::from_secs(60)));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            client
                .lookup(&format!("{url}/"), "Another", "2", &AtomicBool::new(false))
                .unwrap_err()
                .kind,
            LookupErrorKind::Upstream
        );
    }
    #[test]
    fn fields_are_typed_and_maxima_are_preserved() {
        let value = decode(br#"{"character_id":"123","name":"Synthetic","world_id":"2","power_type":"Fire","movement_mode":"Flight","alignment":"Hero","combat_rating":450,"skill_points":600,"stats":{"health":100000,"power":50000},"guild":{"name":"Example"}}"#, "synthetic", "2").unwrap();
        assert_eq!(value.id, "123");
        assert_eq!(value.power.as_deref(), Some("Fire"));
        assert_eq!(value.movement.as_deref(), Some("Flight"));
        assert_eq!(value.alignment.as_deref(), Some("Hero"));
        assert_eq!(value.combat_rating, Some(450));
        assert_eq!(value.skill_points, Some(600));
        assert_eq!(value.health, Some(100000));
        assert_eq!(value.power_max, Some(50000));
        assert_eq!(value.league.as_deref(), Some("Example"));
    }

    #[test]
    fn missing_and_null_fields_remain_unknown() {
        for extra in [
            "",
            r#", "stats":null,"guild":null,"combat_rating":null,"power_type":null"#,
            r#", "stats":{"health":null},"guild":{}"#,
        ] {
            let json =
                format!(r#"{{"character_id":"123","name":"Synthetic","world_id":"2"{extra}}}"#);
            let value = decode(json.as_bytes(), "Synthetic", "2").unwrap();
            assert!(value.health.is_none() && value.power_max.is_none());
            assert!(value.combat_rating.is_none() && value.skill_points.is_none());
            assert!(value.power.is_none() && value.league.is_none());
        }
    }

    #[test]
    fn rejects_wrong_missing_ambiguous_or_malformed_identity() {
        for json in [
            r#"{"character_id":"123","name":"SyntheticOther","world_id":"2"}"#,
            r#"{"character_id":"123","name":"Synthetic","world_id":"4"}"#,
            r#"{"character_id":"12*","name":"Synthetic","world_id":"2"}"#,
            r#"{"character_id":"","name":"Synthetic","world_id":"2"}"#,
            r#"{"name":"Synthetic","world_id":"2"}"#,
            r#"[{"character_id":"123","name":"Synthetic","world_id":"2"}]"#,
            r#"{"character_id":"123","name":"Synthetic","world_id":"2","combat_rating":-1}"#,
        ] {
            assert!(decode(json.as_bytes(), "Synthetic", "2").is_err());
        }
    }

    #[test]
    fn rejects_oversized_response() {
        assert!(
            decode(
                &vec![b' '; MAX_RESPONSE_BYTES as usize + 1],
                "Synthetic",
                "2"
            )
            .is_err()
        );
    }

    #[test]
    fn transport_accepts_exact_reference_and_rejects_oversize() {
        let (url, server) = serve_once(
            "200 OK",
            "",
            br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#.to_vec(),
        );
        assert_eq!(lookup(&url, "Synthetic", "2").unwrap().id, "123");
        server.join().unwrap();
        let (url, server) = serve_once("200 OK", "", vec![b' '; MAX_RESPONSE_BYTES as usize + 1]);
        assert!(
            lookup(&url, "Synthetic", "2")
                .unwrap_err()
                .contains("size limit")
        );
        server.join().unwrap();
    }

    #[test]
    fn transport_errors_are_sanitized_and_redirects_refused() {
        for (status, headers, expected) in [
            (
                "302 Found",
                "Location: https://unknown.example/private\r\n",
                "redirects",
            ),
            ("404 Not Found", "", "No API character"),
            ("429 Too Many Requests", "", "rate limit"),
            ("500 Server Error", "", "unsuccessful status"),
            ("200 OK", "", "invalid character response"),
        ] {
            let (url, server) = serve_once(status, headers, b"private server details".to_vec());
            let error = lookup(&url, "Synthetic", "2").unwrap_err();
            assert!(error.contains(expected));
            assert!(!error.contains("private") && !error.contains(&url));
            server.join().unwrap();
        }
    }

    #[test]
    fn network_failure_does_not_echo_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            lookup(&url, "Synthetic", "2").unwrap_err(),
            "API request failed or timed out."
        );
    }

    #[test]
    fn encodes_name_and_world_as_query_values() {
        let url = request_url(DEFAULT_BASE_URL, "Synthetic & Test", "2").unwrap();
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(url.path(), "/api/v1/census/characters");
        assert_eq!(pairs[0], ("name".into(), "Synthetic & Test".into()));
        assert_eq!(pairs[1], ("worldId".into(), "2".into()));
        for (base, name, world) in [
            ("https://user:secret@example.test", "Synthetic", "2"),
            (DEFAULT_BASE_URL, "Sy", "2"),
            (DEFAULT_BASE_URL, "Synthetic*", "2"),
            (DEFAULT_BASE_URL, "Synthetic", "*"),
        ] {
            assert!(request_url(base, name, world).is_err());
        }
    }
}
