//! In-battle form changes preserve the roster identity and preview details.

use super::{abilities::BoostOrigin, turn::Position, *};
use crate::{dex::Category, mechanics::BoostStat};

impl Pokemon {
    pub(super) fn set_current_species(&mut self, species: &'static Species, rules: StatRules) {
        self.species = species;
        self.weighthg = species.weighthg;
        self.stats_before_change = None;
        self.set_temporary_type(None);
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
        self.weighthg = self.base_species.weighthg;
        let saved_stats = self.stats_before_change.take();
        if let Some(f) = self.transformed.take() {
            self.set_current_species(self.base_species, rules);
            self.moves = f.original_moves;
            self.stats = saved_stats.map_or(f.original_stats, |stats| *stats);
        } else if !std::ptr::eq(self.species, self.base_species) {
            self.set_current_species(self.base_species, rules);
        } else if let Some(stats) = saved_stats {
            self.stats = *stats;
        }
    }
}

impl Battle {
    pub(super) fn ice_face_shields(&self, pos: Position, category: Category) -> bool {
        category == Category::Physical
            && self.mon(pos).species.name == "Eiscue"
            && self.event_ability(pos) == "iceface"
    }

    pub(super) fn break_ice_face(&mut self, pos: Position) {
        self.add(format!("|-activate|{}|ability: Ice Face", self.ident(pos)));
        self.mon_mut(pos).ice_face_busted = true;
    }

    pub(super) fn ice_face_update(&mut self, pos: Position) {
        if self.mon(pos).ice_face_busted && self.event_ability(pos) == "iceface" {
            self.mon_mut(pos).ice_face_busted = false;
            if self.mon(pos).species.name == "Eiscue" {
                let noice = DEX.species(self.format.dex_mod, "eiscuenoice").unwrap();
                self.permanent_ability_forme(pos, noice, false);
            }
        }
    }

    pub(super) fn restore_ice_face(&mut self, pos: Position, weather: Option<Weather>) {
        if matches!(weather, Some(Weather::Hail | Weather::Snow))
            && self.mon(pos).hp > 0
            && self.mon(pos).species.name == "Eiscue-Noice"
            && self.event_ability(pos) == "iceface"
        {
            self.add(format!("|-activate|{}|ability: Ice Face", self.ident(pos)));
            self.mon_mut(pos).ice_face_busted = false;
            let shield = DEX.species(self.format.dex_mod, "eiscue").unwrap();
            self.permanent_ability_forme(pos, shield, false);
        }
    }

    pub(super) fn gulp_missile_load(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.species.name == "Cramorant"
            && self.event_ability(pos) == "gulpmissile"
            && mon.transformed.is_none()
        {
            let form = if mon.hp * 2 <= mon.max_hp.get() {
                "cramorantgorging"
            } else {
                "cramorantgulping"
            };
            self.temporary_ability_forme(pos, form, "");
        }
    }

    pub(super) fn gulp_missile_hit(
        &mut self,
        holder: Position,
        attacker: Position,
        move_data: &Move,
        log: usize,
    ) {
        if self.mon(attacker).hp == 0
            || !self.active_effects(attacker)
            || self.mon(holder).semi_invulnerable()
        {
            return;
        }
        let loaded = match self.mon(holder).species.name.as_str() {
            "Cramorant-Gulping" => false,
            "Cramorant-Gorging" => true,
            _ => return,
        };
        let suffix = format!("|[from] ability: Gulp Missile|[of] {}", self.ident(holder));
        self.indirect_damage(attacker, 4, &suffix);
        if loaded {
            self.try_status(attacker, "par", holder, move_data, false, log);
        } else {
            self.boost_from(
                attacker,
                holder,
                &[(BoostStat::Def, -1)],
                BoostOrigin::DamagingAbility,
            );
        }
        self.temporary_ability_forme(holder, "cramorant", "");
    }

    pub(super) fn sync_flower_gift(&mut self, pos: Position, weather: Option<Weather>) {
        let mon = self.mon(pos);
        if mon.hp == 0
            || mon.transformed.is_some()
            || mon.base_species.base_species != "Cherrim"
            || self.event_ability(pos) != "flowergift"
        {
            return;
        }
        let form = if mon.personal_weather(weather).map(Weather::ordinary) == Some(Weather::Sun) {
            "cherrimsunshine"
        } else {
            "cherrim"
        };
        self.temporary_ability_forme(pos, form, "[msg]|[from] ability: Flower Gift");
    }

    pub(super) fn flower_gift_modifier(&mut self, pos: Position) -> u32 {
        if !self.possible_flower_gift {
            return 4096;
        }
        let mut holders = [(pos, 0); 2];
        let mut count = 0;
        for ally in self.positions().filter(|p| p.player == pos.player) {
            if self.mon(ally).ability == "flowergift" && self.active_effects(ally) {
                holders[count] = (ally, self.mon(ally).speed);
                count += 1;
            }
        }
        speed_sort_by(&mut holders[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        let sun = self
            .mon(pos)
            .personal_weather(self.effective_weather())
            .map(Weather::ordinary)
            == Some(Weather::Sun);
        let mut modifier = 4096;
        for (ally, _) in holders.into_iter().take(count) {
            if sun
                && self.event_ability(ally) == "flowergift"
                && self.mon(ally).base_species.base_species == "Cherrim"
            {
                modifier = crate::mechanics::chain(modifier, 6144);
            }
        }
        modifier
    }

    pub(super) fn tera_shell_active(&self, pos: Position) -> bool {
        let mon = self.mon(pos);
        mon.species.name == "Terapagos-Terastal"
            && mon.hp == mon.max_hp.get()
            && self.event_ability(pos) == "terashell"
            && !self.ignores_ability_of(pos)
    }
    fn permanent_ability_forme(
        &mut self,
        pos: Position,
        species: &'static Species,
        change_ability: bool,
    ) {
        let rules = self.stat_rules;
        let mon = self.mon_mut(pos);
        let hp_before = mon.max_hp.get();
        mon.set_current_species(species, rules);
        mon.base_species = species;
        let end = mon.details.find(',').unwrap_or(mon.details.len());
        mon.details.replace_range(..end, &species.name);
        let ident = self.ident(pos);
        self.add(format!(
            "|detailschange|{ident}|{}",
            self.displayed_details(pos)
        ));
        let mon = self.mon_mut(pos);
        let new_hp = species.max_hp.unwrap_or(mon.stats.hp).max(1);
        if new_hp != hp_before {
            mon.max_hp = NonZeroU32::new(new_hp).unwrap();
            if mon.hp != 0 {
                mon.hp = new_hp.saturating_sub(hp_before - mon.hp).max(1);
                self.add_health_line_with("-heal", pos, false, "|[silent]");
            }
        }
        if change_ability {
            let replacement = to_id(&species.abilities["0"]);
            if self.mon(pos).ability != replacement {
                self.end_ability(pos);
                let mon = self.mon_mut(pos);
                mon.base_ability.clone_from(&replacement);
                mon.ability = replacement;
                self.reset_ability_order(pos);
                self.start_ability(pos);
            }
        }
    }

    pub(super) fn tera_shift(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if self.event_ability(pos) != "terashift"
            || mon.base_species.base_species != "Terapagos"
            || mon.species.name == "Terapagos-Terastal"
            || mon.hp == 0
        {
            return;
        }
        let species = DEX
            .species(self.format.dex_mod, "terapagosterastal")
            .unwrap();
        self.add(format!(
            "|-activate|{}|ability: Tera Shift",
            self.ident(pos)
        ));
        self.permanent_ability_forme(pos, species, true);
    }

    pub(super) fn threshold_forme(&mut self, pos: Position, entering: bool) {
        let mon = self.mon(pos);
        if mon.hp == 0 || mon.transformed.is_some() {
            return;
        }
        let id = match (
            self.event_ability(pos),
            mon.base_species.base_species.as_str(),
        ) {
            ("schooling", "Wishiwashi") if mon.level >= 20 => {
                if mon.hp * 4 > mon.max_hp.get() {
                    "wishiwashischool"
                } else {
                    "wishiwashi"
                }
            }
            ("shieldsdown", "Minior") => {
                if mon.hp * 2 > mon.max_hp.get() {
                    "miniormeteor"
                } else if mon.species.name == "Minior-Meteor" {
                    return self.temporary_ability_forme_to(pos, mon.submitted_species, "");
                } else {
                    return;
                }
            }
            ("powerconstruct", "Zygarde")
                if !entering
                    && mon.species.name != "Zygarde-Complete"
                    && mon.hp * 2 <= mon.max_hp.get() =>
            {
                let complete = DEX.species(self.format.dex_mod, "zygardecomplete").unwrap();
                self.add(format!(
                    "|-activate|{}|ability: Power Construct",
                    self.ident(pos)
                ));
                self.permanent_ability_forme(pos, complete, true);
                return;
            }
            ("zenmode", "Darmanitan") if !entering => {
                let zen = mon.species.name.ends_with("-Zen");
                let galar = mon.species.name.contains("Galar");
                if mon.hp * 2 <= mon.max_hp.get() && !zen {
                    self.mon_mut(pos).track_volatile("zenmode");
                    if galar {
                        "darmanitangalarzen"
                    } else {
                        "darmanitanzen"
                    }
                } else if mon.hp * 2 > mon.max_hp.get() && zen {
                    self.mon_mut(pos).untrack_volatile("zenmode");
                    if galar {
                        "darmanitangalar"
                    } else {
                        "darmanitan"
                    }
                } else {
                    return;
                }
            }
            _ => return,
        };
        self.temporary_ability_forme(pos, id, "");
    }

    pub(super) fn end_zen_mode(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.ability == "zenmode" && mon.hp != 0 && mon.volatile_ids().contains(&"zenmode") {
            let id = if mon.species.name == "Darmanitan-Galar-Zen" {
                "darmanitangalar"
            } else {
                "darmanitan"
            };
            self.mon_mut(pos).untrack_volatile("zenmode");
            self.temporary_ability_forme(pos, id, "[silent]|[from] ability: Zen Mode");
        }
    }

    pub(super) fn regress_fainted_zygarde(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.base_species.name != "Zygarde-Complete"
            || mon.submitted_species.name == "Zygarde-Complete"
        {
            return;
        }
        let rules = self.stat_rules;
        let submitted = mon.submitted_species;
        let mon = self.mon_mut(pos);
        mon.set_current_species(submitted, rules);
        mon.base_species = submitted;
        mon.max_hp = NonZeroU32::new(mon.stats.hp.max(1)).unwrap();
        let end = mon.details.find(',').unwrap_or(mon.details.len());
        mon.details.replace_range(..end, &submitted.name);
        self.add(format!(
            "|detailschange|{}|{}|[silent]",
            self.ident(pos),
            self.displayed_details(pos)
        ));
    }

    pub(super) fn apply_crowned_forms(&mut self) {
        let rules = self.stat_rules;
        let dex = self.format.dex_mod;
        let mut changed = 0;
        for side in self.sides.iter_mut().flatten() {
            for mon in &mut side.party {
                let (species, special_move) = match (mon.species.name.as_str(), mon.item.as_str()) {
                    ("Zacian", "rustedsword") => ("zaciancrowned", "behemothblade"),
                    ("Zamazenta", "rustedshield") => ("zamazentacrowned", "behemothbash"),
                    _ => continue,
                };
                let species = DEX.species(dex, species).unwrap();
                mon.set_current_species(species, rules);
                mon.base_species = species;
                let end = mon.details.find(',').unwrap_or(mon.details.len());
                mon.details.replace_range(..end, &species.name);
                mon.ability = to_id(&species.abilities["0"]);
                mon.base_ability.clone_from(&mon.ability);
                changed += 1;
                if let Some(slot) = mon.moves.iter_mut().find(|slot| slot.id == "ironhead") {
                    let data = DEX.moves(dex, special_move).unwrap();
                    let pp = crate::mechanics::max_pp(data.pp, data.no_pp_boosts);
                    *slot = MoveSlot {
                        id: special_move.into(),
                        move_data: data,
                        pp,
                        max_pp: pp,
                    };
                }
            }
        }
        if self.format.report_exact_hp {
            for _ in 0..changed {
                self.add("|debug|Start handler suppressed by Gastro Acid or Neutralizing Gas");
            }
        }
    }

    pub(super) fn forecast(&mut self, pos: Position, weather: Option<Weather>) {
        let mon = self.mon(pos);
        if mon.hp == 0
            || self.event_ability(pos) != "forecast"
            || mon.base_species.base_species != "Castform"
            || mon.transformed.is_some()
        {
            return;
        }
        let weather = mon.personal_weather(weather).map(Weather::ordinary);
        let id = match weather {
            Some(Weather::Sun) => "castformsunny",
            Some(Weather::Rain) => "castformrainy",
            Some(Weather::Snow | Weather::Hail) => "castformsnowy",
            _ => "castform",
        };
        self.temporary_ability_forme(pos, id, "[msg]|[from] ability: Forecast");
    }

    pub(super) fn hunger_switch(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0
            || mon.species.base_species != "Morpeko"
            || self.event_ability(pos) != "hungerswitch"
        {
            return;
        }
        let id = if mon.species.name == "Morpeko" {
            "morpekohangry"
        } else {
            "morpeko"
        };
        self.temporary_ability_forme(pos, id, "");
    }

    pub(super) fn relic_song_forme(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0 || mon.base_species.base_species != "Meloetta" || mon.transformed.is_some() {
            return;
        }
        let id = if mon.species.name == "Meloetta-Pirouette" {
            "meloetta"
        } else {
            "meloettapirouette"
        };
        let species = DEX.species(self.format.dex_mod, id).unwrap();
        let rules = self.stat_rules;
        self.mon_mut(pos).set_current_species(species, rules);
        let apparent = self.disguise(pos).map_or(species, |mon| mon.species);
        self.add(format!(
            "|-formechange|{}|{}|[msg]",
            self.ident(pos),
            apparent.name
        ));
    }

    pub(super) fn temporary_ability_forme(&mut self, pos: Position, id: &str, message: &str) {
        let species = DEX.species(self.format.dex_mod, id).unwrap();
        self.temporary_ability_forme_to(pos, species, message);
    }

    fn temporary_ability_forme_to(
        &mut self,
        pos: Position,
        species: &'static Species,
        message: &str,
    ) {
        if std::ptr::eq(self.mon(pos).species, species) {
            return;
        }
        let rules = self.stat_rules;
        self.mon_mut(pos).set_current_species(species, rules);
        self.add(format!(
            "|-formechange|{}|{}|{message}",
            self.ident(pos),
            species.name
        ));
    }

    pub(super) fn zero_to_hero_on_switch(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.base_species.base_species != "Palafin" || mon.species.name == "Palafin-Hero" {
            return;
        }
        let hero = DEX.species(self.format.dex_mod, "palafinhero").unwrap();
        let rules = self.stat_rules;
        let mon = self.mon_mut(pos);
        mon.set_current_species(hero, rules);
        mon.base_species = hero;
        mon.announced_hero = false;
        let suffix = mon.details.find(',').unwrap_or(mon.details.len());
        mon.details.replace_range(..suffix, &hero.name);
        self.add(format!(
            "|detailschange|{}|{}",
            self.ident(pos),
            self.mon(pos).details
        ));
    }

    pub(super) fn announce_hero(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.species.name == "Palafin-Hero" && !mon.announced_hero {
            mon.announced_hero = true;
            self.add(format!(
                "|-activate|{}|ability: Zero to Hero",
                self.ident(pos)
            ));
        }
    }

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
        if mon.active_ability() != "stancechange"
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
