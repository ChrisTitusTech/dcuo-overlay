//! Bounded HTTPS transport for explicitly published wiki item names.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Duration};

const ENDPOINT: &str = "https://dcuo-wiki.com/api/overlay/items";
pub const MAX_ITEMS: usize = 50;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiCapture {
    pub request_id: u64,
    pub player_name: String,
    pub items: Vec<(u32, String)>,
    pub build: String,
    pub captured_at: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub name: String,
    pub world_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemId {
    #[serde(deserialize_with = "deserialize_item_id")]
    pub item_id: u32,
}
#[derive(Clone, Debug, Deserialize)]
pub struct MissingItems {
    pub character: Character,
    pub items: Vec<ItemId>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedItem {
    #[serde(serialize_with = "serialize_item_id")]
    pub item_id: u32,
    pub name: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub schema_version: u32,
    pub character: Character,
    pub build: String,
    pub profile: &'static str,
    pub captured_at: u64,
    pub items: Vec<NamedItem>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Receipt {
    pub submitted: usize,
    pub skipped: usize,
}
pub fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn valid_character(c: &Character) -> bool {
    !c.id.is_empty()
        && c.id.len() <= 32
        && c.id.bytes().all(|v| v.is_ascii_digit())
        && !c.world_id.is_empty()
        && c.world_id.len() <= 16
        && c.world_id.bytes().all(|v| v.is_ascii_digit())
        && !c.name.is_empty()
        && c.name.len() <= 512
        && !c.name.chars().any(char::is_control)
}
impl MissingItems {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = HashSet::new();
        if !valid_character(&self.character)
            || self.items.len() > MAX_ITEMS
            || self
                .items
                .iter()
                .any(|v| v.item_id == 0 || !seen.insert(v.item_id))
        {
            return Err("Wiki returned invalid character or item data.".into());
        }
        Ok(())
    }
    pub fn submission(&self, capture: &WikiCapture) -> Result<Submission, String> {
        self.validate()?;
        let requested: HashSet<_> = self.items.iter().map(|v| v.item_id).collect();
        let mut seen = HashSet::new();
        if !capture
            .player_name
            .eq_ignore_ascii_case(&self.character.name)
            || !is_hex64(&capture.build)
            || capture.captured_at == 0
            || capture.items.is_empty()
            || capture.items.len() > MAX_ITEMS
            || capture.items.iter().any(|(id, name)| {
                !requested.contains(id)
                    || !seen.insert(*id)
                    || name.trim().is_empty()
                    || name.len() > 512
                    || !(2..=160).contains(&name.encode_utf16().count())
                    || name.chars().any(char::is_control)
            })
        {
            return Err("Capture does not match the linked character and requested items.".into());
        }
        Ok(Submission {
            schema_version: 1,
            character: self.character.clone(),
            build: capture.build.clone(),
            profile: "item-names-v1",
            captured_at: capture.captured_at,
            items: capture
                .items
                .iter()
                .map(|(id, name)| NamedItem {
                    item_id: *id,
                    name: name.clone(),
                })
                .collect(),
        })
    }
}
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent()
}
fn authorization(token: &str) -> Result<String, String> {
    if !is_hex64(token) {
        return Err("Enter the 64-character wiki token.".into());
    }
    Ok(format!("Bearer {token}"))
}
fn decode<T: serde::de::DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<T, String> {
    match response.status().as_u16() {
        200 | 201 => (),
        401 | 403 => {
            return Err("Wiki token expired, revoked, or no longer linked. Pair again.".into());
        }
        409 => {
            return Err(
                "Wiki equipment or linked character changed. Refresh the wiki item list.".into(),
            );
        }
        429 => return Err("Wiki rate limit reached. Wait before retrying.".into()),
        _ => return Err("Wiki rejected the request. Refresh the linked item list.".into()),
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_vec()
        .map_err(|_| "Wiki response exceeded limits or could not be read.".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "Wiki returned invalid data.".into())
}
pub fn fetch(token: &str) -> Result<MissingItems, String> {
    let response = agent()
        .get(ENDPOINT)
        .header("Authorization", authorization(token)?)
        .header("Accept", "application/json")
        .call()
        .map_err(|_| "Could not reach the wiki.".to_string())?;
    let result: MissingItems = decode(response)?;
    result.validate()?;
    Ok(result)
}
pub fn submit(token: &str, submission: &Submission) -> Result<Receipt, String> {
    if submission.schema_version != 1
        || submission.profile != "item-names-v1"
        || !valid_character(&submission.character)
        || !is_hex64(&submission.build)
        || submission.captured_at == 0
        || submission.items.is_empty()
        || submission.items.len() > MAX_ITEMS
    {
        return Err("Invalid wiki submission.".into());
    }
    let mut seen = HashSet::new();
    if submission.items.iter().any(|item| {
        item.item_id == 0
            || !seen.insert(item.item_id)
            || item.name.trim().is_empty()
            || item.name.len() > 512
            || !(2..=160).contains(&item.name.encode_utf16().count())
            || item.name.chars().any(char::is_control)
    }) {
        return Err("Invalid wiki item names.".into());
    }
    let body =
        serde_json::to_vec(submission).map_err(|_| "Could not prepare submission.".to_string())?;
    let response = agent()
        .post(ENDPOINT)
        .header("Authorization", authorization(token)?)
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|_| "Submission response unavailable. Refresh before retrying.".to_string())?;
    let receipt: Receipt = decode(response)?;
    if receipt.submitted.checked_add(receipt.skipped) != Some(submission.items.len()) {
        return Err(
            "Wiki returned inconsistent submission counts. Refresh before retrying.".into(),
        );
    }
    Ok(receipt)
}

fn deserialize_item_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = String::deserialize(d)?;
    let id = value.parse::<u32>().map_err(serde::de::Error::custom)?;
    if id == 0 || id.to_string() != value {
        return Err(serde::de::Error::custom("invalid item ID"));
    }
    Ok(id)
}
fn serialize_item_id<S: serde::Serializer>(id: &u32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(status: u16, data: impl Into<Vec<u8>>) -> ureq::http::Response<ureq::Body> {
        ureq::http::Response::builder()
            .status(status)
            .body(ureq::Body::builder().data(data))
            .unwrap()
    }
    #[test]
    fn created_receipt_is_accepted() {
        let receipt: Receipt =
            decode(response(201, br#"{"submitted":2,"skipped":1}"#.to_vec())).unwrap();
        assert_eq!((receipt.submitted, receipt.skipped), (2, 1));
    }
    #[test]
    fn stale_equipment_requires_refresh() {
        let error = decode::<Receipt>(response(409, Vec::new())).unwrap_err();
        assert!(error.contains("changed"));
        assert!(error.contains("Refresh"));
    }
    #[test]
    fn oversized_response_is_rejected_before_json_decoding() {
        let error = decode::<Receipt>(response(201, vec![b' '; 64 * 1024 + 1])).unwrap_err();
        assert_eq!(error, "Wiki response exceeded limits or could not be read.");
    }
    #[test]
    fn wire_item_ids_are_canonical_strings() {
        assert_eq!(
            serde_json::from_str::<ItemId>(r#"{"itemId":"7"}"#)
                .unwrap()
                .item_id,
            7
        );
        for value in [
            r#"{"itemId":7}"#,
            r#"{"itemId":"07"}"#,
            r#"{"itemId":"0"}"#,
            r#"{"itemId":"4294967296"}"#,
        ] {
            assert!(serde_json::from_str::<ItemId>(value).is_err());
        }
        assert_eq!(
            serde_json::to_string(&NamedItem {
                item_id: 7,
                name: "Name".into()
            })
            .unwrap(),
            r#"{"itemId":"7","name":"Name"}"#
        );
    }
    #[test]
    fn token_is_strict() {
        assert!(is_hex64(&"a".repeat(64)));
        for v in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
            assert!(!is_hex64(&v));
        }
    }
    #[test]
    fn evidence_is_bound_to_requested_identity_and_ids() {
        let missing = MissingItems {
            character: Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            items: vec![ItemId { item_id: 7 }],
        };
        let mut capture = WikiCapture {
            request_id: 1,
            player_name: "hERO".into(),
            items: vec![(7, "Name".into())],
            build: "b".repeat(64),
            captured_at: 1,
        };
        assert!(missing.submission(&capture).is_ok());
        capture.player_name = "Other".into();
        assert!(missing.submission(&capture).is_err());
        capture.player_name = "Hero".into();
        capture.items[0].0 = 8;
        assert!(missing.submission(&capture).is_err());
        capture.items = vec![(7, "Name".into()), (7, "Name".into())];
        assert!(missing.submission(&capture).is_err());
    }
}
