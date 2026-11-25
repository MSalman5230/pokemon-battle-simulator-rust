//! Legacy Z-Move metadata retained by the Champions mod.

use super::{turn::Position, *};
use crate::dex::{Category, Item, ItemZMove};

fn generic_name(typing: &str) -> Option<&'static str> {
    Some(match typing {
        "Normal" => "Breakneck Blitz",
        "Fighting" => "All-Out Pummeling",
        "Flying" => "Supersonic Skystrike",
        "Poison" => "Acid Downpour",
        "Ground" => "Tectonic Rage",
        "Rock" => "Continental Crush",
        "Bug" => "Savage Spin-Out",
        "Ghost" => "Never-Ending Nightmare",
        "Steel" => "Corkscrew Crash",
        "Fire" => "Inferno Overdrive",
        "Water" => "Hydro Vortex",
        "Grass" => "Bloom Doom",
        "Electric" => "Gigavolt Havoc",
        "Psychic" => "Shattered Psyche",
        "Ice" => "Subzero Slammer",
        "Dragon" => "Devastating Drake",
        "Dark" => "Black Hole Eclipse",
        "Fairy" => "Twinkle Tackle",
        _ => return None,
    })
}

impl Battle {
    fn z_crystal(&self, pos: Position) -> Option<&'static Item> {
        let side = self.sides[pos.player.index()].as_ref().unwrap();
        let mon = self.mon(pos);
        if side.z_move_used || mon.transformed.is_some() && mon.species.transformed_z_restricted {
            return None;
        }
        let item = DEX.item(self.format.dex_mod, &mon.item)?;
        (item.z_move.is_some()
            && item
                .item_user
                .as_ref()
                .is_none_or(|users| users.contains(&mon.species.name)))
        .then_some(item)
    }

    fn z_move_data(&self, crystal: &'static Item, base: &'static Move) -> Option<&'static Move> {
        match crystal.z_move.as_ref()? {
            ItemZMove::Signature(name) if crystal.z_move_from.as_deref() == Some(&base.name) => {
                DEX.moves(self.format.dex_mod, &to_id(name))
            }
            ItemZMove::Generic(true) if crystal.z_move_type.as_deref() == Some(&base.move_type) => {
                if base.category == Category::Status {
                    return Some(base);
                }
                base.z_power
                    .as_ref()?
                    .base_power
                    .filter(|&power| power > 0)?;
                DEX.moves(self.format.dex_mod, &to_id(generic_name(&base.move_type)?))
            }
            _ => None,
        }
    }

    pub(super) fn z_move_requests(&self, pos: Position) -> Option<Vec<serde_json::Value>> {
        let crystal = self.z_crystal(pos)?;
        let mon = self.mon(pos);
        if !mon
            .moves
            .iter()
            .enumerate()
            .any(|(slot, _)| !self.actual_disabled(pos, slot))
        {
            return None;
        }
        let mut exists = false;
        let moves = mon
            .moves
            .iter()
            .map(|slot| {
                let Some(data) = self
                    .z_move_data(crystal, slot.move_data)
                    .filter(|_| slot.pp > 0)
                else {
                    return serde_json::Value::Null;
                };
                exists = true;
                let name = if data.category == Category::Status && !data.is_z_move {
                    format!("Z-{}", data.name)
                } else {
                    data.name.clone()
                };
                serde_json::json!({"move":name, "target":data.target})
            })
            .collect();
        exists.then_some(moves)
    }
}
