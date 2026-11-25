//! Champions rule and learnset inputs, separate from battle resolution.
//!
//! A move's individual learnability is only one part of set legality. Event
//! availability, bans, and move combinations can impose further restrictions.

use crate::{
    choice::to_id,
    dex::{DEX, Mod},
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, sync::LazyLock};

mod flat;
mod natdex;
pub use flat::{TeamValidator, ValidationError};

type Table<T> = HashMap<String, T>;

pub static LEGALITY: LazyLock<LegalityCatalog> = LazyLock::new(|| {
    let db: LegalityCatalog = serde_json::from_str(include_str!("../data/champions-legality.json"))
        .expect("checked-in legality rules must deserialize");
    assert_eq!(
        db.revision, DEX.revision,
        "legality and battle dex must share a revision"
    );
    db
});

#[derive(Deserialize)]
pub struct LegalityCatalog {
    pub revision: String,
    formats: Table<FormatRules>,
    #[serde(flatten)]
    champions: ModRules,
    regmb: ModRules,
}

#[derive(Deserialize)]
struct ModRules {
    species: Table<SpeciesRules>,
    learnsets: Table<Learnset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatRules {
    pub min_level: u16,
    pub max_total_level: Option<u16>,
    pub min_source_gen: u8,
    pub ev_limit: Option<u16>,
    pub item_limit: u8,
    pub rule_sources: Table<String>,
    pub complex_bans: Vec<Combination>,
    pub complex_team_bans: Vec<Combination>,
}

#[derive(Deserialize)]
pub struct Combination(pub String, pub String, pub Option<u16>, pub Vec<String>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeciesRules {
    pub num: i16,
    #[serde(rename = "gen")]
    pub generation: u8,
    pub tags: Vec<String>,
    pub tier: String,
    pub doubles_tier: Option<String>,
    pub forme: String,
    pub base_forme: String,
    pub battle_only: Option<Value>,
    pub changes_from: Option<String>,
    pub required_ability: Option<String>,
    pub required_items: Option<Vec<String>>,
    pub required_move: Option<String>,
    pub evo_level: Option<u16>,
    pub can_hatch: bool,
    pub mother: Option<String>,
    pub prevo: String,
    pub nfe: bool,
    pub egg_groups: Vec<String>,
    pub parents: Option<Vec<LearnsetParent>>,
    pub nat_dex_tier: Option<String>,
    pub gender: Option<String>,
    pub evo_region: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnsetParent {
    pub species: String,
    #[serde(default)]
    pub egg_only: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Learnset {
    pub moves: Table<Vec<String>>,
    pub events: Option<Vec<Value>>,
    pub encounters: Option<Vec<Value>>,
    #[serde(default)]
    pub event_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveSources {
    /// A modern source can teach the move without requiring a special origin.
    Unrestricted,
    /// Level, Sketch, an event, or a transfer needs separate validation.
    Conditional,
    /// Neither the form nor its legal parents list the move or Sketch.
    Absent,
    /// Upstream has no learnset for this form.
    Missing,
}

impl LegalityCatalog {
    pub fn format(&self, format_id: &str) -> Option<&FormatRules> {
        self.formats.get(format_id)
    }

    pub fn species(&self, dex_mod: Mod, species_id: &str) -> Option<&SpeciesRules> {
        (dex_mod == Mod::RegMb)
            .then(|| self.regmb.species.get(species_id))
            .flatten()
            .or_else(|| self.champions.species.get(species_id))
    }

    pub fn learnset(&self, dex_mod: Mod, species_id: &str) -> Option<&Learnset> {
        (dex_mod == Mod::RegMb)
            .then(|| self.regmb.learnsets.get(species_id))
            .flatten()
            .or_else(|| self.champions.learnsets.get(species_id))
    }

    pub fn move_sources(
        &self,
        dex_mod: Mod,
        species_id: &str,
        move_id: &str,
        level: u16,
    ) -> MoveSources {
        let Some(rules) = self.species(dex_mod, species_id) else {
            return MoveSources::Missing;
        };
        let Some(parents) = rules.parents.as_ref().filter(|p| !p.is_empty()) else {
            return MoveSources::Missing;
        };
        let mut conditional = false;
        for parent in parents {
            let Some(learnset) = self.learnset(dex_mod, &parent.species) else {
                return MoveSources::Missing;
            };
            conditional |= learnset.moves.contains_key("sketch") && move_id != "sketch";
            let Some(sources) = learnset.moves.get(move_id) else {
                continue;
            };
            for source in sources {
                let modern_egg =
                    source.starts_with("9E") && !matches!(source.as_str(), "9Eany" | "9Epomeg");
                if parent.egg_only && !modern_egg {
                    continue;
                }
                if parent.species == species_id
                    || self
                        .species(dex_mod, &parent.species)
                        .is_some_and(|s| s.num == rules.num)
                {
                    let enough_level = source
                        .strip_prefix("9L")
                        .and_then(|n| n.parse::<u16>().ok())
                        .is_some_and(|required| level >= required);
                    if modern_egg || matches!(source.as_str(), "9M" | "9T") || enough_level {
                        return MoveSources::Unrestricted;
                    }
                }
                conditional = true;
            }
        }
        if conditional {
            MoveSources::Conditional
        } else {
            MoveSources::Absent
        }
    }

    /// Resolve display names via the same Dex aliases used by battle input.
    pub fn move_sources_by_name(
        &self,
        dex_mod: Mod,
        species_name: &str,
        move_name: &str,
        level: u16,
    ) -> MoveSources {
        let species_id = DEX
            .species(dex_mod, &to_id(species_name))
            .map(|s| to_id(&s.name));
        let move_id = DEX.moves(dex_mod, &to_id(move_name)).map(flat::move_id);
        match (species_id, move_id) {
            (Some(species), Some(move_id)) => self.move_sources(dex_mod, &species, &move_id, level),
            _ => MoveSources::Missing,
        }
    }
}
