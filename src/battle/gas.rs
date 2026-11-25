//! Neutralizing Gas suppresses ability callbacks, without replacing the
//! submitted ability. The active field is cached on the affected Pokémon
//! because stat and item calculations often access them without a battle.

use super::{turn::Position, *};

impl Pokemon {
    pub(super) fn gas_shield_is_up(&self) -> bool {
        // Ability Shield ignores Klutz. Avoid a recursive ability/item query.
        self.item == "abilityshield"
            && !self.items_suppressed
            && !self.under(field_effects::LegacyCondition::Embargo)
    }

    #[inline]
    pub(super) fn ignores_own_ability(&self) -> bool {
        let suppressed = self.gastro_acid
            || self.gas_exposed
                && self.ability != "neutralizinggas"
                && !self.commanding
                && !self.gas_shield_is_up();
        suppressed
            && !DEX
                .ability(&self.ability)
                .is_some_and(|ability| ability.cannot_suppress)
    }

    fn emits_neutralizing_gas(&self) -> bool {
        self.ability == "neutralizinggas"
            && !self.fainted
            && !self.gastro_acid
            && !self.gas_ending
            && self.transformed.is_none()
    }
}

impl Battle {
    pub(super) fn refresh_gas_exposure(&mut self, active_mask: u8) {
        if !self.possible_neutralizing_gas {
            return;
        }
        let exposed = self
            .positions()
            .any(|pos| active_mask & pos.mask() != 0 && self.mon(pos).emits_neutralizing_gas());
        for pos in self.positions() {
            self.mon_mut(pos).gas_exposed = exposed;
        }
    }

    pub(super) fn start_neutralizing_gas(&mut self, source: Position) {
        self.mon_mut(source).gas_ending = false;
        if !self.mon(source).emits_neutralizing_gas() {
            return;
        }
        self.refresh_gas_exposure(u8::MAX);
        self.add(format!("|-ability|{}|Neutralizing Gas", self.ident(source)));
        for pos in self.positions() {
            if self.mon(pos).fainted {
                continue;
            }
            if self.mon(pos).gas_shield_is_up() {
                self.add(format!("|-block|{}|item: Ability Shield", self.ident(pos)));
                continue;
            }
            if self.mon(pos).commanding {
                continue;
            }
            if self.mon(pos).illusion.is_some() {
                self.end_illusion(pos);
            }
            if matches!(
                self.mon(pos).ability.as_str(),
                "desolateland" | "primordialsea" | "deltastream"
            ) {
                self.weather_ability_changed(pos, true);
            }
        }
    }

    pub(super) fn end_neutralizing_gas(&mut self, source: Position) {
        if self.mon(source).ability != "neutralizinggas"
            || self.mon(source).transformed.is_some()
            || self
                .positions()
                .any(|pos| pos != source && self.mon(pos).emits_neutralizing_gas())
        {
            return;
        }
        self.add(format!(
            "|-end|{}|ability: Neutralizing Gas",
            self.ident(source)
        ));
        if std::mem::replace(&mut self.mon_mut(source).gas_ending, true) {
            return;
        }
        self.refresh_gas_exposure(u8::MAX);
        let (order, count) = self.ordered_actives(false);
        for (pos, _) in order.into_iter().take(count) {
            if pos == source
                || self.mon(pos).gas_shield_is_up()
                || DEX
                    .ability(&self.mon(pos).ability)
                    .is_some_and(|a| a.cannot_suppress)
            {
                continue;
            }
            if let Some(name) = self.weather_suppressor(pos) {
                self.add(format!("|-ability|{}|{name}", self.ident(pos)));
            }
            self.start_ability(pos);
            if self.mon(pos).ability == "gluttony" {
                self.mon_mut(pos).gluttony_ready = false;
            }
        }
    }
}
