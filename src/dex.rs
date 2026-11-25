//! Static, mod-resolved data from the pinned oracle. This is not team validation
//! and does not run the handlers named in a record.

use crate::choice::{TargetKind, to_id};
use crate::mechanics::{BoostStat, Nature, StatRules, Stats, Truncation};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

type Table<T> = HashMap<String, T>;

pub static DEX: LazyLock<Dex> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/champions.json"))
        .expect("checked-in Champions dex must deserialize")
});

#[derive(Debug, Deserialize)]
pub struct Dex {
    pub revision: String,
    formats: Table<Format>,
    species: Table<Species>,
    moves: Table<Move>,
    abilities: Table<Effect>,
    items: Table<Item>,
    natures: Table<Nature>,
    types: Table<TypeData>,
    aliases: Table<String>,
    regmb: Override,
}

#[derive(Debug, Deserialize)]
struct Override {
    species: Table<Species>,
    moves: Table<Move>,
    items: Table<Item>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameType {
    Singles,
    Doubles,
}

impl GameType {
    pub fn active_slots(self) -> usize {
        match self {
            Self::Singles => 1,
            Self::Doubles => 2,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize)]
pub enum Mod {
    #[serde(rename = "champions")]
    Champions,
    #[serde(rename = "championsregmb")]
    RegMb,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub name: String,
    #[serde(rename = "mod")]
    pub dex_mod: Mod,
    pub game_type: GameType,
    pub generator: Option<String>,
    pub rules: Vec<String>,
    pub min_team_size: usize,
    pub max_team_size: usize,
    pub max_move_count: usize,
    pub picked_team_size: Option<usize>,
    pub default_level: u16,
    pub adjust_level: Option<u16>,
    pub adjust_level_down: Option<u16>,
    pub max_level: u16,
    pub unbounded_truncation: bool,
}

impl Format {
    pub fn has_rule(&self, id: &str) -> bool {
        self.rules.iter().any(|r| r == id)
    }

    pub fn stat_rules(&self) -> StatRules {
        StatRules {
            level_clause: self.has_rule("levelclausemod"),
            overflow_stat: self.has_rule("overflowstatmod"),
            truncation: if self.unbounded_truncation {
                Truncation::Unbounded
            } else {
                Truncation::Cartridge
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Species {
    pub name: String,
    pub base_species: String,
    pub types: Vec<String>,
    pub base_stats: Stats,
    pub gender: Option<String>,
    pub abilities: Table<String>,
    pub is_nonstandard: Option<String>,
    #[serde(rename = "maxHP")]
    pub max_hp: Option<u32>,
    pub required_item: Option<String>,
    pub has_condition: bool,
    pub unreleased_hidden: bool,
    pub packed_ability_slots: Option<Table<String>>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
pub enum Category {
    Physical,
    Special,
    Status,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(untagged)]
pub enum Accuracy {
    Percent(u8),
    /// Showdown represents a bypassed accuracy check as true.
    Bypass(bool),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub name: String,
    #[serde(rename = "type")]
    pub move_type: String,
    pub category: Category,
    pub target: TargetKind,
    pub base_power: u16,
    pub priority: i8,
    pub pp: u8,
    #[serde(default, rename = "noPPBoosts")]
    pub no_pp_boosts: bool,
    pub accuracy: Accuracy,
    pub crit_ratio: Option<u8>,
    pub flags: Table<u8>,
    pub is_nonstandard: Option<String>,
    pub handlers: Vec<String>,
    pub secondary: bool,
    #[serde(rename = "self")]
    pub self_effect: bool,
    pub condition: bool,
    pub boosts: Vec<(BoostStat, i8)>,
    pub self_boosts: Vec<(BoostStat, i8)>,
    pub self_chance: Option<u8>,
    pub heal: Option<[u16; 2]>,
    pub drain: Option<[u16; 2]>,
    pub recoil: Option<[u16; 2]>,
    #[serde(default)]
    pub thaws_target: bool,
    pub status: Option<String>,
    #[serde(default)]
    pub ignore_immunity: serde_json::Value,
    pub secondaries: Vec<Secondary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secondary {
    pub chance: Option<u16>,
    pub status: Option<String>,
    pub volatile_status: Option<String>,
    pub boosts: Vec<(BoostStat, i8)>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub name: String,
    pub is_nonstandard: Option<String>,
    pub handlers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(flatten)]
    pub effect: Effect,
    pub mega_stone: Option<Table<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeData {
    /// Showdown codes: 1 weak, 2 resists, 3 immune, 0 neutral.
    pub damage_taken: Table<u8>,
}

impl Dex {
    pub fn format(&self, id: &str) -> Option<&Format> {
        self.formats.get(id)
    }

    pub fn formats(&self) -> impl Iterator<Item = (&str, &Format)> {
        self.formats
            .iter()
            .map(|(id, format)| (id.as_str(), format))
    }

    pub fn species(&self, dex_mod: Mod, id: &str) -> Option<&Species> {
        self.lookup(&self.species, dex_mod, Some(&self.regmb.species), id)
    }

    pub fn moves(&self, dex_mod: Mod, id: &str) -> Option<&Move> {
        self.lookup(&self.moves, dex_mod, Some(&self.regmb.moves), id)
    }

    pub fn item(&self, dex_mod: Mod, id: &str) -> Option<&Item> {
        self.lookup(&self.items, dex_mod, Some(&self.regmb.items), id)
    }

    pub fn ability(&self, id: &str) -> Option<&Effect> {
        self.lookup(&self.abilities, Mod::Champions, None, id)
    }

    pub fn nature(&self, name: &str) -> Nature {
        self.natures.get(&to_id(name)).copied().unwrap_or_default()
    }

    pub fn type_data(&self, id: &str) -> Option<&TypeData> {
        self.types.get(id)
    }

    fn lookup<'a, T>(
        &self,
        base: &'a Table<T>,
        dex_mod: Mod,
        overlay: Option<&'a Table<T>>,
        id: &str,
    ) -> Option<&'a T> {
        let lookup = |key: &str| {
            if dex_mod == Mod::RegMb
                && let Some(value) = overlay.and_then(|table| table.get(key))
            {
                return Some(value);
            }
            base.get(key)
        };
        lookup(id).or_else(|| self.aliases.get(id).and_then(|alias| lookup(alias)))
    }
}
