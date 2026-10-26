//! Bounded HTTPS transport for explicitly published wiki item details.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, time::Duration};

const ENDPOINT: &str = "https://dcuo-wiki.com/api/overlay/items";
pub const MAX_ITEMS: usize = 50;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiCapture {
    pub request_id: u64,
    pub catalog: bool,
    pub next_cursor: u32,
    pub player_name: String,
    pub items: Vec<NamedItem>,
    pub equipped: Vec<crate::equipped_items::EquippedItem>,
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
#[serde(rename_all = "camelCase")]
pub struct MissingItems {
    pub character: Character,
    pub items: Vec<ItemId>,
    #[serde(default, deserialize_with = "deserialize_optional_item_id")]
    pub next_cursor: Option<u32>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_png: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_combat_rating: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_id: Option<u32>,
}
impl ItemDetails {
    pub fn substantive(&self) -> bool {
        self.description.is_some()
            || self.style.is_some()
            || self.icon_resource.is_some()
            || self.required_level.is_some_and(|v| v > 0)
            || self.required_combat_rating.is_some_and(|v| v > 0)
    }
    fn valid(&self) -> bool {
        [
            (&self.description, 4096, true),
            (&self.style, 512, false),
            (&self.icon_resource, 512, false),
        ]
        .into_iter()
        .all(|(value, max, whitespace)| {
            value.as_ref().is_none_or(|text| {
                !text.trim().is_empty()
                    && text.len() <= max
                    && !text.chars().any(|ch| {
                        ch.is_control() && !(whitespace && matches!(ch, '\n' | '\r' | '\t'))
                    })
            })
        })
    }
}
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NamedItem {
    #[serde(serialize_with = "serialize_item_id")]
    pub item_id: u32,
    pub name: String,
    pub details: ItemDetails,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub schema_version: u32,
    pub scope: String,
    pub character: Character,
    pub build: String,
    pub profile: &'static str,
    pub captured_at: u64,
    pub items: Vec<NamedItem>,
    pub equipped: Vec<crate::equipped_items::EquippedItem>,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub submitted: usize,
    pub skipped: usize,
    #[serde(default)]
    pub icons_uploaded: usize,
    #[serde(default)]
    pub icons_missing: usize,
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
            || self.next_cursor == Some(0)
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
        if capture.items.is_empty() {
            return Err(
                "No requested item details are loaded. Open your inventory and try capturing again."
                    .into(),
            );
        }
        let requested: HashSet<_> = self.items.iter().map(|v| v.item_id).collect();
        let mut seen = HashSet::new();
        if !capture
            .player_name
            .eq_ignore_ascii_case(&self.character.name)
            || !is_hex64(&capture.build)
            || capture.captured_at == 0
            || (capture.catalog && !capture.equipped.is_empty())
            || capture.items.is_empty()
            || capture.items.len() > MAX_ITEMS
            || capture.items.iter().any(|item| {
                let id = &item.item_id;
                let name = &item.name;
                (!capture.catalog && !requested.contains(id))
                    || (capture.catalog && !item.details.substantive())
                    || !seen.insert(*id)
                    || !item.details.valid()
                    || name.trim().is_empty()
                    || name.len() > 512
                    || !(2..=160).contains(&name.encode_utf16().count())
                    || name.chars().any(char::is_control)
            })
        {
            return Err("Capture does not match the linked character and requested items.".into());
        }
        Ok(Submission {
            schema_version: 2,
            scope: if capture.catalog {
                "catalog"
            } else {
                "equipment"
            }
            .into(),
            character: self.character.clone(),
            build: capture.build.clone(),
            profile: "item-details-v1",
            captured_at: capture.captured_at,
            equipped: capture.equipped.clone(),
            items: capture.items.clone(),
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
    fetch_page(token, None)
}
pub fn fetch_page(token: &str, cursor: Option<u32>) -> Result<MissingItems, String> {
    let url = match cursor {
        Some(cursor) => format!("{ENDPOINT}?cursor={cursor}"),
        None => ENDPOINT.into(),
    };
    let response = agent()
        .get(&url)
        .header("Authorization", authorization(token)?)
        .header("Accept", "application/json")
        .call()
        .map_err(|_| "Could not reach the wiki.".to_string())?;
    let result: MissingItems = decode(response)?;
    result.validate()?;
    Ok(result)
}
#[cfg(feature = "window")]
pub fn submit_with_icons(
    token: &str,
    submission: &Submission,
    source: Option<&std::path::Path>,
) -> Result<Receipt, String> {
    let mut submission = submission.clone();
    let resources: Vec<_> = submission
        .items
        .iter()
        .filter_map(|item| item.details.icon_resource.clone())
        .collect();
    if let Some(source) = source.filter(|_| !resources.is_empty()) {
        let icons = crate::item_icons::decode_icons(source, &resources);
        attach_available_icons(&mut submission.items, &icons);
    }
    submit(token, &submission)
}
#[cfg(feature = "window")]
fn attach_available_icons(
    items: &mut [NamedItem],
    icons: &std::collections::BTreeMap<String, Result<Vec<u8>, String>>,
) {
    use base64::Engine;
    let mut encoded_bytes = 0usize;
    for item in items {
        if let Some(resource) = &item.details.icon_resource
            && let Some(Ok(png)) = icons.get(resource)
            && !png.is_empty()
            && png.len() <= 65536
            && encoded_bytes + png.len().div_ceil(3) * 4 <= 3 * 1024 * 1024
        {
            let encoded = base64::prelude::BASE64_STANDARD.encode(png);
            encoded_bytes += encoded.len();
            item.details.icon_png = Some(encoded);
        }
    }
}
fn bounded_submission_body(submission: &Submission) -> Result<Vec<u8>, String> {
    let mut bounded = submission.clone();
    let mut total = 0usize;
    for item in &mut bounded.items {
        if let Some(icon) = &item.details.icon_png {
            if icon.len() > 3 * 1024 * 1024 - total {
                item.details.icon_png = None;
            } else {
                total += icon.len();
            }
        }
    }
    let mut body = serde_json::to_vec(&bounded).map_err(|_| "Could not prepare submission.")?;
    if body.len() > 4 * 1024 * 1024 {
        for index in (0..bounded.items.len()).rev() {
            if bounded.items[index].details.icon_png.take().is_some() {
                body = serde_json::to_vec(&bounded).map_err(|_| "Could not prepare submission.")?;
                if body.len() <= 4 * 1024 * 1024 {
                    return Ok(body);
                }
            }
        }
        return Err("Item details exceed submission size limit.".into());
    }
    Ok(body)
}
pub fn submit(token: &str, submission: &Submission) -> Result<Receipt, String> {
    if !matches!(submission.scope.as_str(), "equipment" | "catalog")
        || (submission.scope == "catalog" && !submission.equipped.is_empty())
        || submission.schema_version != 2
        || submission.profile != "item-details-v1"
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
        (submission.scope == "catalog" && !item.details.substantive())
            || item.item_id == 0
            || !seen.insert(item.item_id)
            || !item.details.valid()
            || item.name.trim().is_empty()
            || item.name.len() > 512
            || !(2..=160).contains(&item.name.encode_utf16().count())
            || item.name.chars().any(char::is_control)
    }) {
        return Err("Invalid wiki item details.".into());
    }
    let body = bounded_submission_body(submission)?;
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

fn deserialize_optional_item_id<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<u32>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    value
        .map(|value| {
            let id = value.parse::<u32>().map_err(serde::de::Error::custom)?;
            if id == 0 || id.to_string() != value {
                return Err(serde::de::Error::custom("invalid next cursor"));
            }
            Ok(id)
        })
        .transpose()
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
        assert_eq!((receipt.icons_uploaded, receipt.icons_missing), (0, 0));
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
                name: "Name".into(),
                details: ItemDetails::default(),
            })
            .unwrap(),
            r#"{"itemId":"7","name":"Name","details":{}}"#
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
            next_cursor: None,
            character: Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            items: vec![ItemId { item_id: 7 }],
        };
        let mut capture = WikiCapture {
            catalog: false,
            next_cursor: 0,
            equipped: Vec::new(),
            request_id: 1,
            player_name: "hERO".into(),
            items: vec![NamedItem {
                item_id: 7,
                name: "Name".into(),
                details: ItemDetails::default(),
            }],
            build: "b".repeat(64),
            captured_at: 1,
        };
        assert!(missing.submission(&capture).is_ok());
        let empty = WikiCapture {
            items: vec![],
            ..capture.clone()
        };
        assert!(
            missing
                .submission(&empty)
                .unwrap_err()
                .contains("No requested item details are loaded")
        );
        capture.player_name = "Other".into();
        assert!(missing.submission(&capture).is_err());
        capture.player_name = "Hero".into();
        capture.items[0].item_id = 8;
        assert!(missing.submission(&capture).is_err());
        capture.items = vec![
            NamedItem {
                item_id: 7,
                name: "Name".into(),
                details: Default::default()
            };
            2
        ];
        assert!(missing.submission(&capture).is_err());
    }
    #[test]
    fn catalog_allows_unknown_ids_only_with_substantive_details_and_same_character() {
        let missing = MissingItems {
            next_cursor: None,
            character: Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            items: vec![ItemId { item_id: 7 }],
        };
        let mut capture = WikiCapture {
            catalog: true,
            next_cursor: 999,
            request_id: 1,
            player_name: "Hero".into(),
            items: vec![NamedItem {
                item_id: 999,
                name: "New item".into(),
                details: ItemDetails {
                    description: Some("Useful sourced details".into()),
                    ..Default::default()
                },
            }],
            equipped: vec![],
            build: "a".repeat(64),
            captured_at: 1,
        };
        assert_eq!(missing.submission(&capture).unwrap().scope, "catalog");
        capture.catalog = false;
        assert!(missing.submission(&capture).is_err());
        capture.catalog = true;
        capture.player_name = "Other".into();
        assert!(missing.submission(&capture).is_err());
        capture.player_name = "Hero".into();
        capture.items[0].details = Default::default();
        assert!(missing.submission(&capture).is_err());
    }
    #[test]
    fn receipt_reports_partial_artwork_counts() {
        let receipt: Receipt = decode(response(
            201,
            br#"{"submitted":2,"skipped":0,"iconsUploaded":1,"iconsMissing":1}"#.to_vec(),
        ))
        .unwrap();
        assert_eq!((receipt.icons_uploaded, receipt.icons_missing), (1, 1));
    }
    #[cfg(feature = "window")]
    #[test]
    fn unavailable_icon_does_not_drop_valid_definition_or_other_artwork() {
        let mut items = vec![
            NamedItem {
                item_id: 1,
                name: "First".into(),
                details: ItemDetails {
                    description: Some("Details".into()),
                    icon_resource: Some("Available".into()),
                    ..Default::default()
                },
            },
            NamedItem {
                item_id: 2,
                name: "Second".into(),
                details: ItemDetails {
                    icon_resource: Some("Unavailable".into()),
                    ..Default::default()
                },
            },
            NamedItem {
                item_id: 3,
                name: "Third".into(),
                details: ItemDetails {
                    icon_resource: Some("Oversized".into()),
                    ..Default::default()
                },
            },
        ];
        let icons = std::collections::BTreeMap::from([
            ("Available".into(), Ok(vec![1, 2, 3])),
            ("Unavailable".into(), Err("Unsupported texture".into())),
            ("Oversized".into(), Ok(vec![0; 65537])),
        ]);
        attach_available_icons(&mut items, &icons);
        assert!(items[0].details.icon_png.is_some());
        assert_eq!(items[0].details.description.as_deref(), Some("Details"));
        assert!(items[1].details.icon_png.is_none());
        assert_eq!(
            items[1].details.icon_resource.as_deref(),
            Some("Unavailable")
        );
        assert!(items[2].details.icon_png.is_none());
    }
    #[cfg(feature = "window")]
    #[test]
    fn fifty_maximum_icons_keep_aggregate_and_final_body_within_server_limit() {
        let mut items = (1..=50)
            .map(|item_id| NamedItem {
                item_id,
                name: "Item".into(),
                details: ItemDetails {
                    icon_resource: Some("Shared".into()),
                    description: Some("Details".into()),
                    ..Default::default()
                },
            })
            .collect::<Vec<_>>();
        let icons = std::collections::BTreeMap::from([("Shared".into(), Ok(vec![0; 65536]))]);
        attach_available_icons(&mut items, &icons);
        let total: usize = items
            .iter()
            .filter_map(|item| item.details.icon_png.as_ref())
            .map(String::len)
            .sum();
        assert!(total <= 3 * 1024 * 1024);
        assert!(items.iter().any(|item| item.details.icon_png.is_none()));
        assert_eq!(items.len(), 50);
        let submission = Submission {
            schema_version: 2,
            scope: "catalog".into(),
            character: Character {
                id: "1".into(),
                name: "Hero".into(),
                world_id: "2".into(),
            },
            build: "a".repeat(64),
            profile: "item-details-v1",
            captured_at: 1,
            items,
            equipped: vec![],
        };
        let body = bounded_submission_body(&submission).unwrap();
        assert!(body.len() <= 4 * 1024 * 1024);
        let decoded: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(decoded["items"].as_array().unwrap().len(), 50);
    }
    #[test]
    fn equipment_page_cursor_is_optional_canonical_numeric_text() {
        let valid = r#"{"character":{"id":"1","name":"Hero","worldId":"2"},"items":[{"itemId":"51"}],"nextCursor":"51"}"#;
        let page: MissingItems = serde_json::from_str(valid).unwrap();
        assert_eq!(page.next_cursor, Some(51));
        page.validate().unwrap();
        for cursor in ["0", "051", "-1", "4294967296"] {
            assert!(
                serde_json::from_str::<MissingItems>(&valid.replace(
                    "\"nextCursor\":\"51\"",
                    &format!("\"nextCursor\":\"{cursor}\"")
                ))
                .is_err()
            );
        }
        assert!(
            serde_json::from_str::<MissingItems>(
                &valid.replace("\"nextCursor\":\"51\"", "\"nextCursor\":51")
            )
            .is_err()
        );
        assert_eq!(
            serde_json::from_str::<MissingItems>(&valid.replace(",\"nextCursor\":\"51\"", ""))
                .unwrap()
                .next_cursor,
            None
        );
    }
    #[test]
    fn descriptions_allow_line_whitespace_without_relaxing_other_text() {
        let mut details = ItemDetails {
            description: Some("Line one\nLine two\r\n\tIndented".into()),
            ..Default::default()
        };
        assert!(details.valid());
        details.style = Some("Bad\nstyle".into());
        assert!(!details.valid());
        details.style = None;
        details.icon_resource = Some("Bad\ticon".into());
        assert!(!details.valid());
        details.icon_resource = None;
        details.description = Some("Bad\u{0001}control".into());
        assert!(!details.valid());
    }
}
