//! In-battle form changes preserve the roster identity and preview details.

use super::{turn::Position, *};

impl Pokemon {
    pub(super) fn set_current_species(&mut self, species: &'static Species, rules: StatRules) {
        self.species = species;
        self.temporary_type = None;
        self.stats = Stats::map(|stat| {
            rules.stat(
                species.base_stats.get(stat),
                self.points.raw(stat).clamp(0, 255) as u8,
                self.level,
                self.nature,
                stat,
            )
        });
        self.speed = self.stats.spe as i32;
    }

    pub(super) fn reset_forme(&mut self, rules: StatRules) {
        if !std::ptr::eq(self.species, self.base_species) {
            self.set_current_species(self.base_species, rules);
        }
    }
}

impl Battle {
    pub(super) fn stance_change(&mut self, pos: Position, data: &Move) {
        let mon = self.mon(pos);
        if mon.ability != "stancechange" || mon.species.base_species != "Aegislash" {
            return;
        }
        let id = if data.name == "King's Shield" {
            "aegislash"
        } else if data.category != crate::dex::Category::Status {
            "aegislashblade"
        } else {
            return;
        };
        let species = DEX.species(self.format.dex_mod, id).unwrap();
        if !std::ptr::eq(mon.species, species) {
            let rules = self.stat_rules;
            self.mon_mut(pos).set_current_species(species, rules);
            self.add(format!(
                "|-formechange|{}|{}|",
                self.ident(pos),
                species.name
            ));
        }
    }
}
