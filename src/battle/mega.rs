//! Item-based permanent Champions Mega Evolution.

use super::turn::Position;
use super::*;

impl Battle {
    pub(super) fn can_mega(&self, pos: Position) -> bool {
        let side = self.sides[pos.player.index()].as_ref().unwrap();
        !side.mega_used && self.mega_species(pos).is_some()
    }

    fn mega_species(&self, pos: Position) -> Option<&'static Species> {
        let mon = self.mon(pos);
        let item = DEX.item(self.format.dex_mod, &mon.item)?;
        let target = item.mega_stone.as_ref()?.get(&mon.species.name)?;
        DEX.species(self.format.dex_mod, &to_id(target))
    }

    pub(super) fn validate_mega(&self, pos: Position) -> Result<(), BattleError> {
        if !self.can_mega(pos) {
            return Err(BattleError::Choice("this Pokémon cannot Mega Evolve"));
        }
        let species = self.mega_species(pos).unwrap();
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
        let species = self.mega_species(pos).expect("validated Mega form");
        let rules = self.format.stat_rules();
        let mon = self.mon_mut(pos);
        mon.species = species;
        mon.stats = Stats::map(|s| {
            rules.stat(
                species.base_stats.get(s),
                mon.points.raw(s).clamp(0, 255) as u8,
                mon.level,
                mon.nature,
                s,
            )
        });
        mon.speed = mon.stats.spe;
        let suffix = mon.details.split_once(',').map(|(_, suffix)| suffix);
        mon.details =
            suffix.map_or_else(|| species.name.clone(), |s| format!("{},{s}", species.name));
        let changed_hp = mon.stats.hp != mon.max_hp.get();
        if changed_hp {
            let lost_hp = mon.max_hp.get() - mon.hp;
            mon.hp = mon.stats.hp.saturating_sub(lost_hp).max(1);
            mon.max_hp = NonZeroU32::new(mon.stats.hp).unwrap();
        }
        let details = mon.details.clone();
        self.add(format!("|detailschange|{}|{details}", self.ident(pos)));
        if changed_hp {
            self.add_health_line_with("-heal", pos, false, "|[silent]");
        }
        self.add(format!(
            "|-mega|{}|{}|{}",
            self.ident(pos),
            species.base_species,
            species.required_item.as_deref().unwrap_or("")
        ));
        self.sides[pos.player.index()].as_mut().unwrap().mega_used = true;
        self.mon_mut(pos).ability = species
            .abilities
            .get("0")
            .map_or_else(String::new, |name| to_id(name));
        self.start_ability(pos);
    }
}
