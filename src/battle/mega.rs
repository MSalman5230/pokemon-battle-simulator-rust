//! Permanent Mega Evolution and the inherited Ultra Burst.

use super::turn::Position;
use super::*;

enum PermanentEvolution {
    Mega,
    Ultra,
    Primal,
}

impl Battle {
    pub(super) fn can_mega(&self, pos: Position) -> bool {
        let side = self.sides[pos.player.index()].as_ref().unwrap();
        !side.mega_used && !self.mon(pos).hard_locked() && self.mega_species(pos).is_some()
    }

    fn mega_species(&self, pos: Position) -> Option<&'static Species> {
        let mon = self.mon(pos);
        let item = DEX.item(self.format.dex_mod, &mon.item);
        if let Some(evolution) = &mon.base_species.move_mega
            && item.is_none_or(|item| item.z_move.is_none())
            && (self.format.has_rule("+tag:past") || self.format.has_rule("+tag:future"))
            && mon
                .transformed
                .as_ref()
                .map_or(&mon.moves, |state| &state.original_moves)
                .iter()
                .any(|slot| slot.id == evolution.move_id)
        {
            return DEX.species(self.format.dex_mod, &evolution.target);
        }
        let target = item?.mega_stone.as_ref()?.get(&mon.base_species.name)?;
        DEX.species(self.format.dex_mod, &to_id(target))
    }

    fn ultra_species(&self, pos: Position) -> Option<&'static Species> {
        let mon = self.mon(pos);
        (mon.item == "ultranecroziumz"
            && matches!(
                mon.base_species.name.as_str(),
                "Necrozma-Dawn-Wings" | "Necrozma-Dusk-Mane"
            ))
        .then(|| DEX.species(self.format.dex_mod, "necrozmaultra").unwrap())
    }

    pub(super) fn can_ultra_burst(&self, pos: Position) -> bool {
        let side = self.sides[pos.player.index()].as_ref().unwrap();
        !side.ultra_used && !self.mon(pos).hard_locked() && self.ultra_species(pos).is_some()
    }

    pub(super) fn validate_ultra_burst(&self, pos: Position) -> Result<(), BattleError> {
        if !self.can_ultra_burst(pos) {
            return Err(BattleError::Choice("this Pokémon cannot Ultra Burst"));
        }
        self.validate_permanent_form(self.ultra_species(pos).unwrap())
    }

    pub(super) fn validate_mega(&self, pos: Position) -> Result<(), BattleError> {
        if !self.can_mega(pos) {
            return Err(BattleError::Choice("this Pokémon cannot Mega Evolve"));
        }
        self.validate_permanent_form(self.mega_species(pos).unwrap())
    }

    fn validate_permanent_form(&self, species: &'static Species) -> Result<(), BattleError> {
        if species.has_condition {
            return Err(BattleError::Unsupported(format!(
                "Mega forme effects for {}",
                species.name
            )));
        }
        let ability = species
            .abilities
            .get("0")
            .map_or_else(String::new, |name| to_id(name));
        if !Pokemon::supported_ability(&ability) {
            return Err(BattleError::Unsupported(format!("Mega ability {ability}")));
        }
        Ok(())
    }

    pub(super) fn mega_evolve(&mut self, pos: Position) {
        let (species, method) = self.mega_species(pos).map_or_else(
            || {
                (
                    self.ultra_species(pos).expect("validated Ultra form"),
                    PermanentEvolution::Ultra,
                )
            },
            |species| (species, PermanentEvolution::Mega),
        );
        self.permanent_evolution(pos, species, method);
    }

    pub(super) fn primal_reversion(&mut self, pos: Position) -> bool {
        let mon = self.mon(pos);
        if mon.transformed.is_some() {
            return false;
        }
        let id = match (mon.item.as_str(), mon.base_species.name.as_str()) {
            ("redorb", "Groudon") => "groudonprimal",
            ("blueorb", "Kyogre") => "kyogreprimal",
            _ => return false,
        };
        let form = DEX.species(self.format.dex_mod, id).unwrap();
        self.permanent_evolution(pos, form, PermanentEvolution::Primal);
        true
    }

    fn permanent_evolution(
        &mut self,
        pos: Position,
        species: &'static Species,
        method: PermanentEvolution,
    ) {
        let rules = self.stat_rules;
        let mon = self.mon_mut(pos);
        // Forme change stores the new raw Speed. The queue applies Speed
        // stages later, after the evolution and Update events have run.
        mon.set_current_species(species, rules);
        mon.base_species = species;
        let suffix = mon.details.split_once(',').map(|(_, suffix)| suffix);
        mon.details =
            suffix.map_or_else(|| species.name.clone(), |s| format!("{},{s}", species.name));
        let changed_hp = mon.stats.hp != mon.max_hp.get();
        if changed_hp {
            let lost_hp = mon.max_hp.get() - mon.hp;
            mon.hp = mon.stats.hp.saturating_sub(lost_hp).max(1);
            mon.max_hp = NonZeroU32::new(mon.stats.hp).unwrap();
        }
        let apparent = self.disguise(pos);
        let details = apparent.unwrap_or_else(|| self.mon(pos)).details.clone();
        let apparent_species = apparent.map_or(species.base_species.as_str(), |mon| {
            mon.species.name.as_str()
        });
        self.add(format!("|detailschange|{}|{details}", self.ident(pos)));
        if changed_hp {
            self.add_health_line_with("-heal", pos, false, "|[silent]");
        }
        let item = species.required_item.as_deref().unwrap_or("");
        let ident = self.ident(pos);
        match method {
            PermanentEvolution::Primal => self.add(format!("|-primal|{ident}|{item}")),
            PermanentEvolution::Ultra => {
                self.add(format!("|-burst|{ident}|{apparent_species}|{item}"))
            }
            PermanentEvolution::Mega => {
                self.add(format!("|-mega|{ident}|{apparent_species}|{item}"))
            }
        }
        let side = self.sides[pos.player.index()].as_mut().unwrap();
        match method {
            PermanentEvolution::Ultra => side.ultra_used = true,
            PermanentEvolution::Mega => side.mega_used = true,
            PermanentEvolution::Primal => {}
        }
        self.end_ability(pos);
        let mon = self.mon_mut(pos);
        mon.ability = species
            .abilities
            .get("0")
            .map_or_else(String::new, |name| to_id(name));
        mon.base_ability.clone_from(&mon.ability);
        self.possible_unaware |= self.mon(pos).ability == "unaware";
        self.reset_ability_order(pos);
        self.start_ability(pos);
    }
}
