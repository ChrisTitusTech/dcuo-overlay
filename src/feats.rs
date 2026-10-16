//! Bounded, sourced solo feat catalog and character-bound completion display.
use crate::{api::CharacterReference, feat_progress::Progress};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub reviewed_on: String,
    pub sources: Vec<String>,
    pub missions: Vec<Mission>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub key: String,
    pub name: String,
    pub min_cr: Option<u32>,
    pub min_level: Option<u32>,
    pub source: String,
    pub coverage: String,
    pub feats: Vec<Feat>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feat {
    pub key: String,
    pub id: Option<String>,
    pub id_source: Option<String>,
    pub name: String,
    pub points: Option<u32>,
    pub requirement: String,
    pub alignment: Option<String>,
    pub source: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Completed,
    Incomplete,
    Unknown,
}
impl Completion {
    pub fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::Incomplete => "Incomplete",
            Self::Unknown => "Completion unknown",
        }
    }
}
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn source(value: &str) -> bool {
    value.len() <= 2048
        && url::Url::parse(value).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https")
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
        })
}
fn faction(value: Option<&str>) -> Option<bool> {
    match value {
        Some(s) if s.eq_ignore_ascii_case("Hero") => Some(true),
        Some(s) if s.eq_ignore_ascii_case("Villain") => Some(false),
        _ => None,
    }
}
pub fn completion(
    feat: &Feat,
    reference: Option<&CharacterReference>,
    progress: Option<&Progress>,
) -> Completion {
    match (feat.id.as_deref(), reference, progress) {
        (Some(id), Some(character), Some(progress)) if progress.character_id == character.id => {
            if progress.completed.contains(id) {
                Completion::Completed
            } else {
                Completion::Incomplete
            }
        }
        _ => Completion::Unknown,
    }
}
/// Names and rewards must identify one client definition. Never use a fuzzy
/// title match or treat absence from the client's partial catalog as incomplete.
pub fn memory_entry<'a>(
    feat: &Feat,
    live: &'a crate::feat_memory::Snapshot,
) -> Option<&'a crate::feat_memory::Entry> {
    let mut matches = live.entries.iter().filter(|entry| {
        feat.points == Some(entry.points)
            && feat.name.eq_ignore_ascii_case(&entry.title)
            && feat
                .id
                .as_ref()
                .is_none_or(|id| id == &entry.id.to_string())
    });
    let entry = matches.next()?;
    matches.next().is_none().then_some(entry)
}

impl Catalog {
    pub fn bundled() -> Result<Self, String> {
        Self::parse(include_str!("../assets/solo-feats.json"))
    }
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > 2 * 1024 * 1024 {
            return Err("Feat catalog exceeds its size limit.".into());
        }
        let catalog: Self =
            serde_json::from_str(json).map_err(|_| "Invalid feat catalog.".to_string())?;
        if !text(&catalog.reviewed_on, 32)
            || catalog.sources.is_empty()
            || catalog.sources.len() > 256
            || catalog.sources.iter().any(|s| !source(s))
            || catalog.missions.len() > 512
        {
            return Err("Invalid feat catalog metadata.".into());
        }
        let mut keys = HashSet::new();
        let mut total = 0;
        for mission in &catalog.missions {
            if !text(&mission.key, 128)
                || !keys.insert(mission.key.as_str())
                || !text(&mission.name, 256)
                || !text(&mission.coverage, 2048)
                || !source(&mission.source)
                || mission.min_cr.is_some_and(|n| n > 2000)
                || mission.min_level.is_some_and(|n| !(1..=30).contains(&n))
                || mission.feats.len() > 256
            {
                return Err("Invalid solo mission catalog entry.".into());
            }
            total += mission.feats.len();
            if total > 8192 {
                return Err("Too many catalog feats.".into());
            }
            for feat in &mission.feats {
                if !text(&feat.key, 128)
                    || !keys.insert(feat.key.as_str())
                    || !text(&feat.name, 256)
                    || !text(&feat.requirement, 4096)
                    || !source(&feat.source)
                    || feat
                        .id_source
                        .as_deref()
                        .is_some_and(|value| !source(value))
                    || feat
                        .points
                        .is_some_and(|n| ![0, 10, 25, 50, 100].contains(&n))
                    || feat.id.as_ref().is_some_and(|id| {
                        id.is_empty() || id.len() > 32 || !id.bytes().all(|b| b.is_ascii_digit())
                    })
                    || feat.alignment.as_deref().is_some_and(|s| !text(s, 64))
                {
                    return Err("Invalid solo feat catalog entry.".into());
                }
            }
        }
        Ok(catalog)
    }
    pub fn sorted_missions(&self) -> Vec<&Mission> {
        let mut missions: Vec<_> = self.missions.iter().collect();
        missions.sort_by(|a, b| {
            (
                a.min_cr.or(a.min_level.map(|_| 0)).unwrap_or(u32::MAX),
                a.name.to_lowercase(),
                &a.name,
                &a.key,
            )
                .cmp(&(
                    b.min_cr.or(b.min_level.map(|_| 0)).unwrap_or(u32::MAX),
                    b.name.to_lowercase(),
                    &b.name,
                    &b.key,
                ))
        });
        missions
    }
}
impl Mission {
    pub fn visible_feats<'a>(
        &'a self,
        query: &str,
        show_completed: bool,
        reference: Option<&CharacterReference>,
        progress: Option<&Progress>,
    ) -> Vec<(&'a Feat, Completion)> {
        self.visible_feats_with_memory(query, show_completed, reference, progress, None)
    }
    pub fn visible_feats_with_memory<'a>(
        &'a self,
        query: &str,
        show_completed: bool,
        reference: Option<&CharacterReference>,
        progress: Option<&Progress>,
        live: Option<&crate::feat_memory::Snapshot>,
    ) -> Vec<(&'a Feat, Completion)> {
        let query = query.trim().to_lowercase();
        self.feats
            .iter()
            .filter_map(|feat| {
                let memory = live.and_then(|live| memory_entry(feat, live));
                if memory.is_some_and(|entry| !entry.eligible && !entry.completed) {
                    return None;
                }
                if live.is_none()
                    && let (Some(character), Some(required)) = (
                        reference.and_then(|r| faction(r.alignment.as_deref())),
                        faction(feat.alignment.as_deref()),
                    )
                    && character != required
                {
                    return None;
                }
                let state = if live.is_some() {
                    match memory {
                        Some(entry) if entry.completed => Completion::Completed,
                        Some(_) => Completion::Incomplete,
                        None => Completion::Unknown,
                    }
                } else {
                    completion(feat, reference, progress)
                };
                if !show_completed && state == Completion::Completed {
                    return None;
                }
                if !query.is_empty()
                    && ![
                        self.name.as_str(),
                        feat.name.as_str(),
                        feat.requirement.as_str(),
                    ]
                    .iter()
                    .any(|s| s.to_lowercase().contains(&query))
                {
                    return None;
                }
                Some((feat, state))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    fn character() -> CharacterReference {
        CharacterReference {
            id: "123".into(),
            name: "Synthetic".into(),
            world_id: "2".into(),
            power: None,
            movement: None,
            alignment: Some("Hero".into()),
            combat_rating: Some(100),
            skill_points: None,
            health: None,
            power_max: None,
            league: None,
        }
    }
    fn live(entries: Vec<crate::feat_memory::Entry>) -> crate::feat_memory::Snapshot {
        crate::feat_memory::Snapshot {
            entity: 0x10000,
            entity_id: 7,
            entries,
            observed_at: Instant::now(),
        }
    }
    fn live_entry(id: u32, title: &str, completed: bool) -> crate::feat_memory::Entry {
        crate::feat_memory::Entry {
            id,
            title: title.into(),
            description: "Synthetic requirement".into(),
            points: 10,
            completed,
            eligible: true,
        }
    }
    #[test]
    fn live_completion_requires_unique_definitions_and_ignores_other_api_character() {
        let catalog = Catalog::parse(&fixture().to_string()).unwrap();
        let mission = catalog.missions.iter().find(|m| m.key == "level").unwrap();
        let mut feat: Feat =
            serde_json::from_value(fixture()["missions"][3]["feats"][0].clone()).unwrap();
        feat.id = None;
        let snapshot = live(vec![live_entry(10, &feat.name, false)]);
        assert!(!memory_entry(&feat, &snapshot).unwrap().completed);
        let duplicate = live(vec![
            live_entry(10, &feat.name, false),
            live_entry(11, &feat.name, true),
        ]);
        assert!(memory_entry(&feat, &duplicate).is_none());
        feat.id = Some("11".into());
        assert!(memory_entry(&feat, &duplicate).unwrap().completed);
        feat.points = Some(25);
        assert!(memory_entry(&feat, &duplicate).is_none());

        let mut character = character();
        character.alignment = Some("Villain".into());
        let progress = Progress {
            character_id: character.id.clone(),
            completed: ["1".into()].into(),
            received_at: Instant::now(),
        };
        let snapshot = live(vec![live_entry(1, &mission.feats[0].name, false)]);
        let rows = mission.visible_feats_with_memory(
            "",
            false,
            Some(&character),
            Some(&progress),
            Some(&snapshot),
        );
        assert!(
            rows.iter()
                .any(|(f, s)| f.id.as_deref() == Some("1") && *s == Completion::Incomplete)
        );
        assert!(rows.iter().any(|(_, s)| *s == Completion::Unknown));
        let completed = live(vec![live_entry(1, &mission.feats[0].name, true)]);
        assert!(
            !mission
                .visible_feats_with_memory("", false, None, None, Some(&completed))
                .iter()
                .any(|(f, _)| f.id.as_deref() == Some("1"))
        );
        assert!(
            mission
                .visible_feats_with_memory("", true, None, None, Some(&completed))
                .iter()
                .any(|(f, s)| f.id.as_deref() == Some("1") && *s == Completion::Completed)
        );
    }
    fn fixture() -> serde_json::Value {
        serde_json::json!({"reviewed_on":"2026-10-16","sources":["https://example.com/catalog"],"missions":[
            {"key":"unknown","name":"Unknown","min_cr":null,"min_level":null,"source":"https://example.com/mission","coverage":"Partial","feats":[]},
            {"key":"b","name":"Bravo","min_cr":43,"min_level":30,"source":"https://example.com/mission","coverage":"Partial","feats":[]},
            {"key":"a","name":"Alpha","min_cr":43,"min_level":30,"source":"https://example.com/mission","coverage":"Partial","feats":[]},
            {"key":"level","name":"Level 30","min_cr":null,"min_level":30,"source":"https://example.com/mission","coverage":"Partial","feats":[
                {"key":"f1","id":"1","name":"Hero test","points":10,"requirement":"Finish solo","alignment":"Hero","source":"https://example.com/feat"},
                {"key":"f2","id":"2","name":"Villain test","points":25,"requirement":"Finish solo","alignment":"Villain","source":"https://example.com/feat"},
                {"key":"f3","id":null,"name":"Unknown test","points":null,"requirement":"Finish solo","alignment":null,"source":"https://example.com/feat"}
            ]}
        ]})
    }
    #[test]
    fn cr_order_is_ascending_with_unknown_last_and_name_ties() {
        let catalog = Catalog::parse(&fixture().to_string()).unwrap();
        assert_eq!(
            catalog
                .sorted_missions()
                .iter()
                .map(|m| m.key.as_str())
                .collect::<Vec<_>>(),
            ["level", "a", "b", "unknown"]
        );
    }
    #[test]
    fn completion_requires_matching_character_and_known_id() {
        let catalog = Catalog::parse(&fixture().to_string()).unwrap();
        let feat = &catalog.missions[3].feats[0];
        let reference = character();
        let mut progress = Progress {
            character_id: "123".into(),
            completed: HashSet::new(),
            received_at: Instant::now(),
        };
        assert_eq!(
            completion(feat, Some(&reference), None),
            Completion::Unknown
        );
        assert_eq!(completion(feat, None, Some(&progress)), Completion::Unknown);
        assert_eq!(
            completion(feat, Some(&reference), Some(&progress)),
            Completion::Incomplete
        );
        progress.completed.insert("1".into());
        assert_eq!(
            completion(feat, Some(&reference), Some(&progress)),
            Completion::Completed
        );
        assert_eq!(
            completion(
                &catalog.missions[3].feats[2],
                Some(&reference),
                Some(&progress)
            ),
            Completion::Unknown
        );
        progress.character_id = "999".into();
        assert_eq!(
            completion(feat, Some(&reference), Some(&progress)),
            Completion::Unknown
        );
    }
    #[test]
    fn filters_hide_only_proven_completed_and_opposing_faction() {
        let catalog = Catalog::parse(&fixture().to_string()).unwrap();
        let mission = &catalog.missions[3];
        let mut reference = character();
        let progress = Progress {
            character_id: "123".into(),
            completed: HashSet::from(["1".into()]),
            received_at: Instant::now(),
        };
        assert_eq!(
            mission
                .visible_feats("", false, Some(&reference), Some(&progress))
                .len(),
            1
        );
        assert_eq!(
            mission
                .visible_feats("", true, Some(&reference), Some(&progress))
                .len(),
            2
        );
        assert_eq!(
            mission
                .visible_feats("HERO", true, Some(&reference), None)
                .len(),
            1
        );
        assert_eq!(
            mission
                .visible_feats("LEVEL 30", true, Some(&reference), None)
                .len(),
            2
        );
        reference.alignment = Some("Unproven".into());
        assert_eq!(
            mission
                .visible_feats("", true, Some(&reference), None)
                .len(),
            3
        );
        assert_eq!(
            mission
                .visible_feats("", false, None, Some(&progress))
                .len(),
            3
        );
    }
    #[test]
    fn catalog_rejects_duplicate_keys_bad_ids_points_and_sources() {
        for (field, bad) in [
            ("key", serde_json::json!("level")),
            ("id", serde_json::json!("x1")),
            ("points", serde_json::json!(1000)),
            ("source", serde_json::json!("file:///tmp/feat")),
            ("id_source", serde_json::json!("javascript:alert(1)")),
        ] {
            let mut value = fixture();
            value["missions"][3]["feats"][0][field] = bad;
            assert!(Catalog::parse(&value.to_string()).is_err(), "{field}");
        }
    }
    #[test]
    fn optional_mapping_source_accepts_public_http_metadata() {
        let mut value = fixture();
        value["missions"][3]["feats"][0]["id_source"] =
            serde_json::json!("https://example.com/public/feats.json");
        let catalog = Catalog::parse(&value.to_string()).unwrap();
        assert_eq!(
            catalog.missions[3].feats[0].id_source.as_deref(),
            Some("https://example.com/public/feats.json")
        );
        value["missions"][3]["feats"][0]["id_source"] =
            serde_json::json!("https://secret@example.com/private");
        assert!(Catalog::parse(&value.to_string()).is_err());
    }
    #[test]
    fn bundled_catalog_validates() {
        Catalog::bundled().unwrap();
    }
}
