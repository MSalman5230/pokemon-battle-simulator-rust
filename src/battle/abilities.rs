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
    pub(super) fn ability_heal(
        &mut self,
        pos: Position,
        divisor: u32,
        label: &str,
        source: Option<Position>,
    ) -> bool {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.hp == mon.max_hp.get() {
            return false;
        }
        mon.hp = (mon.hp + (mon.max_hp.get() / divisor).max(1)).min(mon.max_hp.get());
        let mut cause = format!("|[from] ability: {label}");
        if let Some(source) = source.filter(|source| *source != pos) {
            cause.push_str(&format!("|[of] {}", self.ident(source)));
        }
        self.add_health_line_with("-heal", pos, false, &cause);
        true
    }

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
                self.boost_from(target, pos, &[(BoostStat::Atk, -1)], true, false);
            }
        }
    }

    pub(super) fn boost_from(
        &mut self,
        target: Position,
        source: Position,
        changes: &[(BoostStat, i8)],
        quiet: bool,
        reflected: bool,
    ) -> bool {
        if self.mon(target).hp == 0 {
            return false;
        }
        let mut success = false;
        let mirror = self.mon(target).ability == "mirrorarmor" && target != source && !reflected;
        for &(stat, amount) in changes {
            let current = self.mon(target).boosts.get(stat);
            if mirror && amount < 0 && current > -6 {
                if self.mon(source).hp != 0 {
                    self.add(format!("|-ability|{}|Mirror Armor", self.ident(target)));
                    self.boost_from(
                        source,
                        target,
                        &[(stat, amount.max(-6 - current))],
                        true,
                        true,
                    );
                }
                continue;
            }
            if self.boost(target, &[(stat, amount)], quiet) {
                success = true;
                if amount < 0
                    && target.player != source.player
                    && self.mon(target).ability == "competitive"
                {
                    self.ability_boost(target, "Competitive", BoostStat::Spa, 2, true);
                }
            }
        }
        success
    }

    pub(super) fn ability_boost(
        &mut self,
        pos: Position,
        label: &str,
        stat: BoostStat,
        amount: i8,
        quiet: bool,
    ) {
        if self.mon(pos).hp == 0
            || !self
                .party(pos.player.foe())
                .unwrap()
                .iter()
                .any(|p| !p.fainted)
        {
            return;
        }
        let change = self.mon_mut(pos).boosts.add(stat, amount);
        if change > 0 {
            self.add(format!("|-ability|{}|{label}|boost", self.ident(pos)));
            self.add(format!(
                "|-boost|{}|{}|{change}",
                self.ident(pos),
                stat.id()
            ));
        } else if quiet {
            self.add(format!("|-boost|{}|{}|0", self.ident(pos), stat.id()));
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
