//! Complete, character-bound Census completion IDs. Retrieval time is not source freshness.
use serde::Deserialize;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const PAGE_SIZE: usize = 1000;
const MAX_PAGES: usize = 20;
const MAX_BODY: u64 = 2 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct Progress {
    pub character_id: String,
    pub completed: HashSet<String>,
    pub received_at: Instant,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    characters_completed_feat_list: Vec<Row>,
    returned: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    character_id: String,
    feat_id: String,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit())
}
fn check(cancel: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Feat lookup cancelled.".into());
    }
    if started.elapsed() >= DEADLINE {
        return Err("Feat lookup exceeded its time limit.".into());
    }
    Ok(())
}
fn accept_page(
    bytes: &[u8],
    character_id: &str,
    completed: &mut HashSet<String>,
) -> Result<bool, String> {
    if bytes.len() as u64 > MAX_BODY {
        return Err("Feat response exceeds the size limit.".into());
    }
    let page: Page =
        serde_json::from_slice(bytes).map_err(|_| "Invalid Census feat response.".to_string())?;
    if page.returned != page.characters_completed_feat_list.len() || page.returned > PAGE_SIZE {
        return Err("Invalid Census feat page count.".into());
    }
    for row in page.characters_completed_feat_list {
        if row.character_id != character_id
            || !valid_id(&row.feat_id)
            || !completed.insert(row.feat_id)
        {
            return Err(
                "Census feat page has mismatched identity or duplicate/invalid IDs.".into(),
            );
        }
    }
    Ok(page.returned < PAGE_SIZE)
}

/// Returns only a complete set; failed or interrupted pagination yields no progress.
pub fn fetch(character_id: &str, cancel: &AtomicBool) -> Result<Progress, String> {
    if !valid_id(character_id) {
        return Err("Invalid Census character ID.".into());
    }
    let started = Instant::now();
    let mut completed = HashSet::new();
    for page in 0..MAX_PAGES {
        check(cancel, started)?;
        let timeout = Duration::from_secs(8).min(DEADLINE.saturating_sub(started.elapsed()));
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .new_agent();
        let mut url = url::Url::parse(
            "https://census.daybreakgames.com/s:example/get/dcuo/characters_completed_feat",
        )
        .expect("constant Census URL");
        url.query_pairs_mut()
            .append_pair("character_id", character_id)
            .append_pair("c:limit", &PAGE_SIZE.to_string())
            .append_pair("c:start", &(page * PAGE_SIZE).to_string())
            .append_pair("c:sort", "feat_id");
        let mut response = agent
            .get(url.as_str())
            .header("Accept", "application/json")
            .header(
                "User-Agent",
                concat!("DCUO-Companion/", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(|_| "Census feat request failed.".to_string())?;
        check(cancel, started)?;
        if response.status().as_u16() != 200 {
            return Err("Census feat request returned an unsuccessful status.".into());
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|_| "Census feat response could not be read within its bounds.".to_string())?;
        check(cancel, started)?;
        let complete = accept_page(&bytes, character_id, &mut completed)?;
        check(cancel, started)?;
        if complete {
            return Ok(Progress {
                character_id: character_id.into(),
                completed,
                received_at: Instant::now(),
            });
        }
    }
    Err("Census feat pagination exceeded its limit; completion is unknown.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(rows: usize, start: usize) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"characters_completed_feat_list": (start..start+rows).map(|i| serde_json::json!({"character_id":"123", "feat_id":i.to_string()})).collect::<Vec<_>>(), "returned":rows})).unwrap()
    }
    #[test]
    fn full_page_requires_next_page_and_short_page_completes() {
        let mut ids = HashSet::new();
        assert!(!accept_page(&page(1000, 0), "123", &mut ids).unwrap());
        assert!(accept_page(&page(2, 1000), "123", &mut ids).unwrap());
        assert_eq!(ids.len(), 1002);
    }
    #[test]
    fn empty_page_is_complete() {
        assert!(accept_page(&page(0, 0), "123", &mut HashSet::new()).unwrap());
    }
    #[test]
    fn rejects_wrong_identity_duplicates_and_malformed_pages() {
        let mut ids = HashSet::new();
        assert!(accept_page(&page(1, 0), "456", &mut ids).is_err());
        accept_page(&page(1, 0), "123", &mut ids).unwrap();
        assert!(accept_page(&page(1, 0), "123", &mut ids).is_err());
        for bytes in [br#"{"error":"No data found"}"#.as_slice(), br#"{"characters_completed_feat_list":[],"returned":1}"#, br#"{"characters_completed_feat_list":[{"character_id":"123","feat_id":5}],"returned":1}"#, br#"{"characters_completed_feat_list":[{"character_id":"123","feat_id":""}],"returned":1}"#] {
            assert!(accept_page(bytes, "123", &mut HashSet::new()).is_err());
        }
        assert!(accept_page(&page(1001, 0), "123", &mut HashSet::new()).is_err());
    }
    #[test]
    fn rejects_bad_id_and_cancellation_without_network() {
        assert!(fetch("?invalid", &AtomicBool::new(false)).is_err());
        assert!(fetch("123", &AtomicBool::new(true)).is_err());
    }
}
