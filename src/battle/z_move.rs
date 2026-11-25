//! Legacy Z-Moves retained by the Champions mod.

use super::{
    abilities::{Attention, BoostOrigin},
    support::ReplacementHeal,
    turn::Position,
    *,
};
use crate::dex::{Category, Item, ItemZMove, Mod};
use crate::mechanics::BoostStat;
use std::{collections::HashMap, sync::LazyLock};

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

type Variants = HashMap<(&'static str, &'static str), Move>;

fn generic_attack(dex_mod: Mod, base: &'static Move, typing: &'static str) -> &'static Move {
    // Keep the normal battle path allocation-free: only build these variants
    // when a battle actually tries to spend a Z-Crystal.
    static VARIANTS: LazyLock<[Variants; 2]> = LazyLock::new(|| {
        [Mod::Champions, Mod::RegMb].map(|dex_mod| {
            let mut result = HashMap::new();
            for base in DEX.move_entries(dex_mod) {
                let Some(power) = base.z_power.as_ref().and_then(|p| p.base_power) else {
                    continue;
                };
                if power == 0 || base.category == Category::Status {
                    continue;
                }
                let types: &[&str] = if base.name == "Weather Ball" {
                    &["Normal", "Fire", "Water", "Rock", "Ice"]
                } else {
                    &[base.move_type.as_str()]
                };
                for &typing in types {
                    if let Some(template) =
                        generic_name(typing).and_then(|name| DEX.moves(dex_mod, &to_id(name)))
                    {
                        let mut variant = template.clone();
                        variant.base_power = power;
                        variant.category = base.category;
                        variant.priority = base.priority;
                        result.insert((base.name.as_str(), typing), variant);
                    }
                }
            }
            result
        })
    });
    &VARIANTS[usize::from(dex_mod == Mod::RegMb)][&(base.name.as_str(), typing)]
}

impl Battle {
    pub(super) fn z_crystal(&self, pos: Position) -> Option<&'static Item> {
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

    pub(super) fn z_move_data(
        &self,
        crystal: &'static Item,
        base: &'static Move,
    ) -> Option<&'static Move> {
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

    pub(super) fn z_named_base_slot(&self, pos: Position, name: &str) -> Option<usize> {
        let crystal = self.z_crystal(pos)?;
        self.mon(pos).moves.iter().position(|slot| {
            slot.pp > 0
                && self
                    .z_move_data(crystal, slot.move_data)
                    .is_some_and(|data| {
                        let id = to_id(&data.name);
                        if data.category == Category::Status && !data.is_z_move {
                            name.strip_prefix('z') == Some(&id)
                        } else {
                            name == id
                        }
                    })
        })
    }

    pub(super) fn z_active_move(
        &self,
        pos: Position,
        base: &'static Move,
        weather_ball_type: Option<&'static str>,
    ) -> &'static Move {
        // The selection already validated the crystal. Its native item rule
        // prevents removal or swapping between the choice and the attempt.
        let item = DEX.item(self.format.dex_mod, &self.mon(pos).item).unwrap();
        if matches!(item.z_move, Some(ItemZMove::Signature(_))) || base.category == Category::Status
        {
            self.z_move_data(item, base).unwrap()
        } else {
            generic_attack(
                self.format.dex_mod,
                base,
                weather_ball_type.unwrap_or(&base.move_type),
            )
        }
    }

    pub(super) fn spend_z_move(&mut self, pos: Position) {
        self.end_illusion(pos);
        self.add(format!("|-zpower|{}", self.ident(pos)));
        self.sides[pos.player.index()].as_mut().unwrap().z_move_used = true;
    }

    pub(super) fn apply_z_power(&mut self, pos: Position, base: &'static Move) {
        let Some(power) = &base.z_power else {
            return;
        };
        if !power.boost.is_empty() {
            self.boost_from(pos, pos, &power.boost, BoostOrigin::ZPower);
            return;
        }
        match power.effect.as_deref() {
            Some("heal") => self.heal_from_z_power(pos),
            Some("healreplacement") => {
                self.prepare_replacement_heal(pos, ReplacementHeal::ZPower(&base.name))
            }
            Some("clearnegativeboost") => {
                self.mon_mut(pos).boosts.clear_negative();
                self.add(format!(
                    "|-clearnegativeboost|{}|[zeffect]",
                    self.ident(pos)
                ));
            }
            Some("redirect") if self.slots() > 1 && self.mon(pos).attention.is_none() => {
                let mon = self.mon_mut(pos);
                mon.attention = Some(Attention::FollowMe);
                mon.track_volatile("followme");
                self.add(format!(
                    "|-singleturn|{}|move: Follow Me|[zeffect]",
                    self.ident(pos)
                ));
            }
            Some("crit2")
                if !self.mon(pos).volatile_ids().contains(&"focusenergy")
                    && self.mon(pos).dragon_cheer == 0 =>
            {
                self.mon_mut(pos).track_volatile("focusenergy");
                self.add(format!(
                    "|-start|{}|move: Focus Energy|[zeffect]",
                    self.ident(pos)
                ));
            }
            Some("curse") if self.mon(pos).has_type("Ghost") => self.heal_from_z_power(pos),
            Some("curse") => {
                self.boost_from(pos, pos, &[(BoostStat::Atk, 1)], BoostOrigin::ZPower);
            }
            _ => {}
        }
    }

    fn heal_from_z_power(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.hp != 0 && mon.hp < mon.max_hp.get() {
            mon.hp = mon.max_hp.get();
            self.add_health_line_with("-heal", pos, false, "|[zeffect]");
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
