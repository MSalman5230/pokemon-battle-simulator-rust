//! Shared ability hooks used by move effects and held-item modifiers.

use super::{turn::Position, *};
use crate::{
    dex::Category,
    mechanics::{BoostStat, chain},
};

impl Pokemon {
    pub(super) fn ability_power_modifier(
        &self,
        move_data: &Move,
        move_type: &str,
        weather: Option<Weather>,
    ) -> u32 {
        let flags = &move_data.flags;
        match self.ability.as_str() {
            "technician" if move_data.base_power <= 60 => 6144,
            "toughclaws" if flags.contains_key("contact") => 5325,
            "sharpness" if flags.contains_key("slicing") => 6144,
            "ironfist" if flags.contains_key("punch") => 4915,
            "sandforce"
                if weather == Some(Weather::Sand)
                    && matches!(move_type, "Rock" | "Ground" | "Steel") =>
            {
                5325
            }
            _ => 4096,
        }
    }

    pub(super) fn attack_modifier(&self, category: Category, move_type: &str) -> u32 {
        let ability = if self.attack_boosts_type(move_type)
            || (category == Category::Physical
                && self.ability == "guts"
                && self.status != Status::Healthy)
        {
            6144
        } else {
            4096
        };
        let item = if matches!(
            (self.item.as_str(), category),
            ("choiceband", Category::Physical) | ("choicespecs", Category::Special)
        ) {
            6144
        } else {
            4096
        };
        chain(ability, item)
    }

    pub(super) fn can_take_item(&self, dex_mod: crate::dex::Mod) -> Option<&'static str> {
        if self.ability == "stickyhold" {
            return None;
        }
        let item = DEX.item(dex_mod, &self.item)?;
        let protected = item.mega_stone.as_ref().is_some_and(|m| {
            m.contains_key(&self.species.name) || m.contains_key(&self.species.base_species)
        });
        (!protected).then_some(item.effect.name.as_str())
    }
}

impl Battle {
    pub(super) fn intimidate(&mut self, pos: Position) {
        let foe = pos.player.foe();
        let slots = self.format.game_type.active_slots();
        let mut started = false;
        for slot in 0..slots {
            let target = Position { player: foe, slot };
            let mon = self.mon(target);
            if mon.hp == 0 {
                continue;
            }
            if !started {
                self.add(format!("|-ability|{}|Intimidate|boost", self.ident(pos)));
                started = true;
            }
            if self.mon(target).ability == "innerfocus" {
                let ident = self.ident(target);
                self.add(format!(
                    "|-fail|{ident}|unboost|atk|[from] ability: Inner Focus|[of] {ident}"
                ));
            } else {
                self.boost(target, &[(BoostStat::Atk, -1)], true);
            }
        }
    }

    pub(super) fn speed_boost(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.active_turns == 0 {
            return;
        }
        let change = mon.boosts.add(BoostStat::Spe, 1);
        if change > 0 {
            self.add(format!("|-ability|{}|Speed Boost|boost", self.ident(pos)));
            self.add(format!("|-boost|{}|spe|1", self.ident(pos)));
        }
    }
}
