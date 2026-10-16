//! Character reference and solo feats; all retrieval remains outside the view.
use crate::{
    api::CharacterReference,
    feat_progress::Progress,
    feats::{Catalog, Completion},
};
use eframe::egui;

pub struct CharacterView {
    pub show_completed: bool,
    pub search: String,
    catalog: Result<Catalog, String>,
}
impl Default for CharacterView {
    fn default() -> Self {
        Self {
            show_completed: false,
            search: String::new(),
            catalog: Catalog::bundled(),
        }
    }
}
impl CharacterView {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        reference: Option<&CharacterReference>,
        progress: Option<&Progress>,
        status: &str,
    ) -> bool {
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            if let Some(character) = reference {
                ui.strong(&character.name);
                ui.label(format!(
                    "CR {}  |  Total SP {}",
                    number(character.combat_rating),
                    number(character.skill_points)
                ));
            } else {
                ui.label("Look up a character to see completion status.");
            }
            refresh = ui
                .add_enabled(reference.is_some(), egui::Button::new("Refresh feats"))
                .clicked();
        });
        if reference.is_some() {
            ui.small("API character reference; not proof of the currently controlled character.");
        }
        if let Some(character) = reference {
            ui.collapsing("Character details", |ui| {
                egui::Grid::new("character-reference-details").show(ui, |ui| {
                    for (label, value) in [
                        ("Power", character.power.as_deref()),
                        ("Movement", character.movement.as_deref()),
                        ("Alignment", character.alignment.as_deref()),
                        ("League", character.league.as_deref()),
                    ] {
                        ui.label(label);
                        ui.label(value.unwrap_or("Unknown"));
                        ui.end_row();
                    }
                    ui.label("Reference maximum health");
                    ui.label(
                        character
                            .health
                            .map_or_else(|| "Unknown".into(), |n| n.to_string()),
                    );
                    ui.end_row();
                    ui.label("Reference maximum power");
                    ui.label(
                        character
                            .power_max
                            .map_or_else(|| "Unknown".into(), |n| n.to_string()),
                    );
                    ui.end_row();
                });
            });
        }
        match (reference, progress) {
            (Some(character), Some(progress)) if progress.character_id == character.id => {
                ui.small(format!(
                    "Completion received {}s ago. Source freshness unknown.",
                    progress.received_at.elapsed().as_secs()
                ));
            }
            _ => {
                ui.small("Completion unavailable. Source freshness unknown.");
            }
        }
        if !status.is_empty() {
            ui.label(status);
        }
        ui.separator();
        ui.heading("Solo On Duty feats");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_completed, "Show completed");
            ui.add(
                egui::TextEdit::singleline(&mut self.search).hint_text("Search missions and feats"),
            );
        });
        let catalog = match &self.catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                ui.label(error);
                return refresh;
            }
        };
        let total_feats: usize = catalog
            .missions
            .iter()
            .map(|mission| mission.feats.len())
            .sum();
        let identified_feats = catalog
            .missions
            .iter()
            .flat_map(|mission| &mission.feats)
            .filter(|feat| feat.id.is_some())
            .count();
        ui.small(format!(
            "Catalog incomplete: {} missions, {} documented feats. Completion available for {} feats. Reviewed {}.",
            catalog.missions.len(), total_feats, identified_feats, catalog.reviewed_on
        ));
        let mut counts = [0usize; 3];
        for mission in &catalog.missions {
            for (_, state) in mission.visible_feats("", true, reference, progress) {
                counts[match state {
                    Completion::Incomplete => 0,
                    Completion::Completed => 1,
                    Completion::Unknown => 2,
                }] += 1;
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.small("All applicable catalog feats:");
            ui.colored_label(egui::Color32::GOLD, format!("{} incomplete", counts[0]));
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                format!("{} completed", counts[1]),
            );
            ui.weak(format!("{} unknown", counts[2]));
        });
        egui::ScrollArea::vertical()
            .id_salt("character-solo-feats")
            .show(ui, |ui| {
                let mut count = 0;
                for mission in catalog.sorted_missions() {
                    let feats = mission.visible_feats(
                        &self.search,
                        self.show_completed,
                        reference,
                        progress,
                    );
                    let coverage_gap = mission.feats.is_empty()
                        && (self.search.trim().is_empty()
                            || mission
                                .name
                                .to_lowercase()
                                .contains(&self.search.trim().to_lowercase()));
                    if feats.is_empty() && !coverage_gap {
                        continue;
                    }
                    count += 1;
                    ui.push_id(&mission.key, |ui| {
                        let entry = match (mission.min_cr, mission.min_level) {
                            (Some(cr), Some(level)) => {
                                format!("Normal minimum CR {cr}; minimum level {level}")
                            }
                            (Some(cr), None) => {
                                format!("Normal minimum CR {cr}; minimum level unknown")
                            }
                            (None, Some(level)) => {
                                format!("Minimum level {level}; no normal minimum CR")
                            }
                            _ => "Normal minimum CR and level unknown".into(),
                        };
                        ui.separator();
                        ui.strong(format!("{} - {}", mission.name, entry));
                        if coverage_gap {
                            ui.label("Feat list not yet available.");
                        }
                        for (feat, state) in &feats {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&feat.name);
                                ui.strong(feat.points.map_or_else(
                                    || "Points unknown".into(),
                                    |points| format!("{points} feat points"),
                                ));
                                match state {
                                    Completion::Completed => {
                                        ui.colored_label(egui::Color32::LIGHT_GREEN, state.label());
                                    }
                                    Completion::Incomplete => {
                                        ui.colored_label(egui::Color32::GOLD, state.label());
                                    }
                                    Completion::Unknown => {
                                        ui.weak(state.label());
                                    }
                                }
                                if let Some(alignment) = &feat.alignment {
                                    ui.small(alignment);
                                }
                            });
                            ui.label(&feat.requirement);
                        }
                        ui.collapsing("Details", |ui| {
                            ui.label(&mission.coverage);
                            ui.hyperlink_to("Mission source", &mission.source);
                            for (feat, _) in &feats {
                                ui.hyperlink_to(format!("{} - source", feat.name), &feat.source);
                                if let Some(source) = &feat.id_source {
                                    ui.hyperlink_to(
                                        format!("{} - completion mapping source", feat.name),
                                        source,
                                    );
                                }
                            }
                        });
                    });
                }
                if count == 0 {
                    ui.label("No feats match the current filters.");
                }
            });
        refresh
    }
}
fn number(value: Option<u32>) -> String {
    value.map_or_else(|| "Unknown".into(), |n| n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_filters_and_offline_render() {
        let mut view = CharacterView::default();
        assert!(!view.show_completed);
        assert!(view.search.is_empty());
        let context = egui::Context::default();
        let mut refresh = false;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            refresh = view.ui(ui, None, None, "Offline");
        });
        assert!(!refresh);
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
    }
    #[test]
    fn coverage_gap_stays_visible_when_mission_matches_search() {
        let catalog = Catalog::parse(r#"{"reviewed_on":"2026-10-16","sources":["https://example.com"],"missions":[{"key":"gap","name":"Uncatalogued Solo","min_cr":null,"min_level":30,"source":"https://example.com","coverage":"No documented feat list","feats":[]}]}"#).unwrap();
        let mut view = CharacterView {
            show_completed: false,
            search: "uncatalogued".into(),
            catalog: Ok(catalog),
        };
        let mut output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            view.ui(ui, None, None, "");
        });
        let rendered = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(rendered.contains("Catalog incomplete"));
        assert!(rendered.contains("Feat list not yet available"));
        assert!(rendered.contains("Completion available for 0 feats"));
        assert!(rendered.contains("0 incomplete"));
        assert!(rendered.contains("0 completed"));
        assert!(rendered.contains("0 unknown"));
        assert!(rendered.contains("Details"));
        assert!(!rendered.contains("No documented feat list"));
        assert!(!rendered.contains("Mission source"));
        output.textures_delta.clear();
        assert!(rendered.contains("Minimum level 30"));
    }
}
