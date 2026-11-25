//! In-battle form changes preserve the roster identity and preview details.

use super::{turn::Position, *};

impl Pokemon {
    pub(super) fn set_current_species(&mut self, species: &'static Species, rules: StatRules) {
        self.species = species;
        self.temporary_type = None;
        self.spent_elements = 0;
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
        if let Some(f) = self.transformed.take() {
            self.set_current_species(self.base_species, rules);
            self.moves = f.original_moves;
            self.stats = f.original_stats;
        } else if !std::ptr::eq(self.species, self.base_species) {
            self.set_current_species(self.base_species, rules);
        }
    }
}

impl Battle {
    pub(super) fn disguise_form(&self, pos: Position) -> Option<&'static str> {
        if self.event_ability(pos) != "disguise" {
            return None;
        }
        match self.mon(pos).species.name.as_str() {
            "Mimikyu" => Some("mimikyubusted"),
            "Mimikyu-Totem" => Some("mimikyubustedtotem"),
            _ => None,
        }
    }

    pub(super) fn disguise_update(&mut self, pos: Position) {
        if !self.mon(pos).disguise_pending || self.mon(pos).hp == 0 {
            return;
        }
        let Some(form) = self.disguise_form(pos) else {
            return;
        };
        let species = DEX.species(self.format.dex_mod, form).unwrap();
        let rules = self.stat_rules;
        let mon = self.mon_mut(pos);
        mon.disguise_pending = false;
        let old_speed = mon.speed;
        mon.set_current_species(species, rules);
        mon.speed = old_speed;
        mon.base_species = species;
        let details_suffix = mon.details.find(',').unwrap_or(mon.details.len());
        mon.details.replace_range(..details_suffix, &species.name);
        self.add(format!(
            "|detailschange|{}|{}",
            self.ident(pos),
            self.mon(pos).details
        ));
        self.indirect_damage(pos, 8, &format!("|[from] pokemon: {}", species.name));
    }

    pub(super) fn stance_change(&mut self, pos: Position, data: &Move) {
        let mon = self.mon(pos);
        if mon.ability != "stancechange"
            || mon.species.base_species != "Aegislash"
            || mon.transformed.is_some()
        {
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
