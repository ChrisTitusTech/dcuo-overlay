//! Explicit, one-shot price entry using observed offers and a user-selected field.
use crate::broker_hotkey::Hotkey;
use eframe::egui;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemKey {
    pub id: u32,
    pub name: String,
    pub quantity: u32,
}
pub struct Offer {
    pub item: ItemKey,
    pub total: u64,
    pub at: Instant,
}
#[derive(Default)]
pub struct PriceEntry {
    selected: Option<ItemKey>,
    price: String,
    confirmed: bool,
    hotkey: Option<Hotkey>,
    armed_pid: Option<i32>,
    status: String,
}
impl PriceEntry {
    pub fn cancel(&mut self) {
        if self.hotkey.take().is_some() {
            self.status = "Price entry cancelled".into();
        }
        self.armed_pid = None;
    }
    pub fn reset(&mut self) {
        self.cancel();
        *self = Self::default();
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, pid: Option<i32>, offers: &[Offer]) {
        if self.hotkey.is_some() && self.armed_pid != pid {
            self.cancel();
        }
        let previous = self.selected.clone();
        ui.strong("Fill a listing price");
        egui::ComboBox::from_id_salt("broker-price-item")
            .selected_text(
                self.selected
                    .as_ref()
                    .map_or("Choose an observed item", |s| &s.name),
            )
            .show_ui(ui, |ui| {
                for offer in offers {
                    ui.selectable_value(
                        &mut self.selected,
                        Some(offer.item.clone()),
                        format!(
                            "{} (client quantity {})",
                            offer.item.name, offer.item.quantity
                        ),
                    );
                }
            });
        let offer = offers
            .iter()
            .find(|o| Some(&o.item) == self.selected.as_ref());
        if previous != self.selected {
            self.cancel();
            self.confirmed = false;
            self.price = offer
                .and_then(|o| suggested(o.total))
                .map_or_else(String::new, |v| v.to_string());
            self.status.clear();
        }
        if offer.is_none() {
            self.cancel();
            self.confirmed = false;
        }
        if let Some(offer) = offer {
            ui.label(format!(
                "Lowest observed competing total: {} ({} seconds ago)",
                offer.total,
                offer.at.elapsed().as_secs()
            ));
            ui.small("Suggestion is $1 below that offer. Observed pages may not contain the market's lowest price.");
        } else {
            ui.label("Search for the item in the game's Broker to observe competing offers.");
        }
        ui.horizontal(|ui| {
            ui.label("Price to type");
            if ui.text_edit_singleline(&mut self.price).changed() {
                self.cancel();
            }
            if ui
                .add_enabled(
                    offer.and_then(|o| suggested(o.total)).is_some(),
                    egui::Button::new("Use suggestion"),
                )
                .clicked()
            {
                self.cancel();
                self.price = offer.and_then(|o| suggested(o.total)).unwrap().to_string();
            }
        });
        if ui.checkbox(&mut self.confirmed, "I checked the exact item and stack, searched All with Usable by me off and Price Ascending").changed() {
            self.cancel();
        }
        ui.small("In game, focus the listing's price field and select ALL existing text. Press and release Ctrl+Alt+P to type once. Turn off Caps Lock, Num Lock and Sticky Keys. Review the price before listing; this never presses Enter or confirms a sale.");
        if let Some(result) = self.hotkey.as_mut().and_then(Hotkey::poll) {
            self.hotkey = None;
            self.armed_pid = None;
            self.status = match result {
                Ok(()) => {
                    "Price typed. Check the game's field before confirming the listing.".into()
                }
                Err(error) => format!("Price entry stopped: {error}"),
            };
        }
        let price = parse_price(&self.price);
        if !self.price.is_empty() && price.is_none() {
            ui.label("Enter a whole price from 1 to 4294967295, without commas.");
        }
        ui.horizontal(|ui| {
            let eligible = offer.is_some_and(|o| o.at.elapsed() < Duration::from_secs(15 * 60));
            if ui.add_enabled(self.hotkey.is_none() && pid.is_some() && price.is_some() && self.confirmed && eligible, egui::Button::new("Arm Ctrl+Alt+P (60 seconds)")).clicked() {
                match crate::broker_hotkey::arm(pid.unwrap(), price.unwrap()) {
                    Ok(hotkey) => {
                        self.hotkey = Some(hotkey);
                        self.armed_pid = pid;
                        self.status = "Armed once for 60 seconds. Select all text in the game's price field, then press and release Ctrl+Alt+P.".into();
                    }
                    Err(error) => self.status = error,
                }
            }
            if ui.add_enabled(self.hotkey.is_some(), egui::Button::new("Cancel price entry")).clicked() {
                self.cancel();
            }
        });
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
fn parse_price(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 10 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>()
        .ok()
        .filter(|v| *v > 0 && *v <= u64::from(u32::MAX))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prices_never_underflow_or_accept_commands_or_out_of_range_values() {
        for total in [0, 1, u64::MAX] {
            assert_eq!(suggested(total), None);
        }
        assert_eq!(suggested(100), Some(99));
        assert_eq!(
            suggested(u64::from(u32::MAX) + 1),
            Some(u64::from(u32::MAX))
        );
        for text in [
            "",
            "0",
            "-1",
            "+1",
            "1\n",
            "1,000",
            "1.5",
            "4294967296",
            "123;abc",
        ] {
            assert_eq!(parse_price(text), None);
        }
        assert_eq!(parse_price("4294967295"), Some(4294967295));
    }
}
