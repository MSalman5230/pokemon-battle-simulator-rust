//! Static, mod-resolved data from the pinned oracle. This is not team validation
//! and does not run the handlers named in a record.

use crate::choice::{TargetKind, to_id};
use crate::mechanics::{BoostStat, Nature, StatRules, Stats, Truncation};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

type Table<T> = HashMap<String, T>;

pub static DEX: LazyLock<Dex> = LazyLock::new(|| {
    let mut dex: Dex = serde_json::from_str(include_str!("../data/champions.json"))
        .expect("checked-in Champions dex must deserialize");
    dex.type_by_name = TYPE_IDS.map(|id| dex.types.get(id).cloned());
    dex
});

pub(crate) fn physical_shell_side_arm(dex_mod: Mod) -> &'static Move {
    static VARIANTS: LazyLock<[Move; 2]> = LazyLock::new(|| {
        [Mod::Champions, Mod::RegMb].map(|mod_id| {
            let mut data = DEX.moves(mod_id, "shellsidearm").unwrap().clone();
            data.category = Category::Physical;
            data.flags.insert("contact".into(), 1);
            data
        })
    });
    &VARIANTS[usize::from(dex_mod == Mod::RegMb)]
}

pub(crate) fn physical_psychic_power(dex_mod: Mod, name: &str) -> &'static Move {
    static VARIANTS: LazyLock<[[Move; 2]; 2]> = LazyLock::new(|| {
        [Mod::Champions, Mod::RegMb].map(|dex_mod| {
            ["photongeyser", "lightthatburnsthesky"].map(|id| {
                let mut data = DEX.moves(dex_mod, id).unwrap().clone();
                data.category = Category::Physical;
                data
            })
        })
    });
    &VARIANTS[usize::from(dex_mod == Mod::RegMb)][usize::from(name != "Photon Geyser")]
}

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
    #[serde(skip)]
    type_by_name: [Option<TypeData>; TYPE_IDS.len()],
    aliases: Table<String>,
    regmb: Override,
    #[serde(rename = "metronomeMoves")]
    metronome_moves: Vec<String>,
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
    pub best_of: Option<usize>,
    pub default_level: u16,
    pub adjust_level: Option<u16>,
    pub adjust_level_down: Option<u16>,
    pub max_level: u16,
    pub unbounded_truncation: bool,
    pub report_exact_hp: bool,
    pub rule_log: Vec<String>,
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
    pub weighthg: u16,
    pub gender: Option<String>,
    pub abilities: Table<String>,
    pub is_nonstandard: Option<String>,
    #[serde(rename = "maxHP")]
    pub max_hp: Option<u32>,
    pub required_item: Option<String>,
    pub has_condition: bool,
    #[serde(default)]
    pub can_evolve: bool,
    #[serde(default)]
    pub is_paradox: bool,
    pub move_mega: Option<MoveMega>,
    #[serde(default)]
    pub transformed_z_restricted: bool,
    pub unreleased_hidden: bool,
    pub packed_ability_slots: Option<Table<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveMega {
    pub target: String,
    pub move_id: String,
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

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(untagged)]
pub enum HitCount {
    Fixed(u8),
    Range([u8; 2]),
}

/// The known dex flags are immutable after construction, except for the
/// synthetic contact variants. Keep the complete map for unknown flags and
/// callers inspecting metadata, but use an indexed bit for battle checks.
#[derive(Debug, Clone)]
pub struct MoveFlags {
    entries: Table<u8>,
    known: u64,
}

fn flag_bit(key: &str) -> u64 {
    match key {
        "allyanim" => 1_u64 << 0,
        "bite" => 1_u64 << 1,
        "bullet" => 1_u64 << 2,
        "bypasssub" => 1_u64 << 3,
        "cantusetwice" => 1_u64 << 4,
        "charge" => 1_u64 << 5,
        "contact" => 1_u64 << 6,
        "dance" => 1_u64 << 7,
        "defrost" => 1_u64 << 8,
        "distance" => 1_u64 << 9,
        "failcopycat" => 1_u64 << 10,
        "failencore" => 1_u64 << 11,
        "failinstruct" => 1_u64 << 12,
        "failmefirst" => 1_u64 << 13,
        "failmimic" => 1_u64 << 14,
        "futuremove" => 1_u64 << 15,
        "gravity" => 1_u64 << 16,
        "heal" => 1_u64 << 17,
        "metronome" => 1_u64 << 18,
        "minimize" => 1_u64 << 19,
        "mirror" => 1_u64 << 20,
        "mustpressure" => 1_u64 << 21,
        "noassist" => 1_u64 << 22,
        "nonsky" => 1_u64 << 23,
        "noparentalbond" => 1_u64 << 24,
        "nosketch" => 1_u64 << 25,
        "nosleeptalk" => 1_u64 << 26,
        "pledgecombo" => 1_u64 << 27,
        "powder" => 1_u64 << 28,
        "protect" => 1_u64 << 29,
        "pulse" => 1_u64 << 30,
        "punch" => 1_u64 << 31,
        "recharge" => 1_u64 << 32,
        "reflectable" => 1_u64 << 33,
        "slicing" => 1_u64 << 34,
        "snatch" => 1_u64 << 35,
        "sound" => 1_u64 << 36,
        "wind" => 1_u64 << 37,
        _ => 0,
    }
}

impl<'de> Deserialize<'de> for MoveFlags {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = Table::deserialize(deserializer)?;
        let known = entries.keys().fold(0, |bits, key| bits | flag_bit(key));
        Ok(Self { entries, known })
    }
}

impl MoveFlags {
    #[inline]
    pub fn contains_key(&self, key: &str) -> bool {
        let bit = flag_bit(key);
        if bit != 0 {
            self.known & bit != 0
        } else {
            self.entries.contains_key(key)
        }
    }

    pub fn insert(&mut self, key: String, value: u8) -> Option<u8> {
        self.known |= flag_bit(&key);
        self.entries.insert(key, value)
    }

    pub fn remove(&mut self, key: &str) -> Option<u8> {
        self.known &= !flag_bit(key);
        self.entries.remove(key)
    }
}

impl std::ops::Deref for MoveFlags {
    type Target = Table<u8>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

#[derive(Debug, Clone, Deserialize)]
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
    pub multihit: Option<HitCount>,
    #[serde(default, rename = "multiaccuracy")]
    pub multi_accuracy: bool,
    pub crit_ratio: Option<u8>,
    pub flags: MoveFlags,
    pub is_nonstandard: Option<String>,
    pub handlers: Vec<String>,
    pub secondary: bool,
    #[serde(rename = "self")]
    pub self_effect: bool,
    pub condition: bool,
    pub generic: bool,
    pub will_crit: Option<bool>,
    #[serde(default)]
    pub always_hit: bool,
    #[serde(default)]
    pub ignore_defensive: bool,
    #[serde(default)]
    pub targets_defense: bool,
    #[serde(default)]
    pub ohko: bool,
    #[serde(default)]
    pub fixed_damage: bool,
    #[serde(default)]
    pub ignore_evasion: bool,
    #[serde(default)]
    pub tracks_target: bool,
    #[serde(default)]
    pub cant_use_twice: bool,
    #[serde(default)]
    pub calls_move: bool,
    pub z_power: Option<ZPower>,
    #[serde(default)]
    pub is_z_move: bool,
    #[serde(default)]
    pub ignore_ability: bool,
    #[serde(default)]
    pub has_sheer_force_boost: bool,
    pub boosts: Vec<(BoostStat, i8)>,
    pub self_boosts: Vec<(BoostStat, i8)>,
    pub self_chance: Option<u8>,
    #[serde(default)]
    pub after_hit_boosts: Vec<(BoostStat, i8)>,
    #[serde(default)]
    pub self_recharge: bool,
    #[serde(default)]
    pub self_rampage: bool,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secondary {
    pub chance: Option<u16>,
    pub status: Option<String>,
    pub volatile_status: Option<String>,
    pub boosts: Vec<(BoostStat, i8)>,
    #[serde(default)]
    pub self_boosts: Vec<(BoostStat, i8)>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub name: String,
    pub is_nonstandard: Option<String>,
    pub handlers: Vec<String>,
    #[serde(default)]
    pub no_trace: bool,
    #[serde(default)]
    pub cannot_suppress: bool,
    #[serde(default)]
    pub breakable: bool,
    #[serde(default)]
    pub no_skill_swap: bool,
    #[serde(default)]
    pub no_entrain: bool,
    #[serde(default)]
    pub no_role_play: bool,
    #[serde(default)]
    pub no_receiver: bool,
    #[serde(default)]
    pub no_transform: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(flatten)]
    pub effect: Effect,
    pub mega_stone: Option<Table<String>>,
    pub forced_forme: Option<String>,
    #[serde(default)]
    pub is_berry: bool,
    #[serde(default)]
    pub ignore_klutz: bool,
    #[serde(default)]
    pub inert_held_item: bool,
    #[serde(default)]
    pub unremovable: bool,
    #[serde(default)]
    pub is_primal_orb: bool,
    pub gem_type: Option<String>,
    pub plate_type: Option<String>,
    #[serde(default)]
    pub plate_boost: bool,
    pub drive_type: Option<String>,
    pub memory_type: Option<String>,
    pub mask_form: Option<String>,
    pub protected_form_base: Option<String>,
    pub natural_gift: Option<NaturalGift>,
    pub z_move: Option<ItemZMove>,
    pub z_move_type: Option<String>,
    pub z_move_from: Option<String>,
    pub item_user: Option<Vec<String>>,
    pub fling: Option<Fling>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NaturalGift {
    pub base_power: u16,
    #[serde(rename = "type")]
    pub typing: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZPower {
    pub base_power: Option<u16>,
    #[serde(default)]
    pub boost: Vec<(BoostStat, i8)>,
    pub effect: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ItemZMove {
    Generic(bool),
    Signature(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fling {
    pub base_power: u16,
    pub status: Option<String>,
    pub volatile_status: Option<String>,
    pub effect: Option<FlingEffect>,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlingEffect {
    MentalHerb,
    WhiteHerb,
}

// A dense type table avoids allocating normalized type IDs and hashing
// the same chart keys repeatedly during effectiveness and immunity checks.
const TYPE_IDS: [&str; 19] = [
    "normal", "fire", "water", "electric", "grass", "ice", "fighting", "poison", "ground",
    "flying", "psychic", "bug", "rock", "ghost", "dragon", "dark", "steel", "fairy", "stellar",
];

#[inline]
fn type_slot(name: &str) -> Option<usize> {
    match name {
        "Normal" => Some(0),
        "Fire" => Some(1),
        "Water" => Some(2),
        "Electric" => Some(3),
        "Grass" => Some(4),
        "Ice" => Some(5),
        "Fighting" => Some(6),
        "Poison" => Some(7),
        "Ground" => Some(8),
        "Flying" => Some(9),
        "Psychic" => Some(10),
        "Bug" => Some(11),
        "Rock" => Some(12),
        "Ghost" => Some(13),
        "Dragon" => Some(14),
        "Dark" => Some(15),
        "Steel" => Some(16),
        "Fairy" => Some(17),
        "Stellar" => Some(18),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct DamageChart {
    entries: Table<u8>,
    types: [Option<u8>; TYPE_IDS.len()],
}

impl<'de> Deserialize<'de> for DamageChart {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries: Table<u8> = Table::deserialize(deserializer)?;
        let mut types = [None; TYPE_IDS.len()];
        for (name, &effect) in &entries {
            if let Some(index) = type_slot(name) {
                types[index] = Some(effect);
            }
        }
        Ok(Self { entries, types })
    }
}

impl DamageChart {
    #[inline]
    pub fn get(&self, name: &str) -> Option<&u8> {
        if let Some(index) = type_slot(name) {
            self.types[index].as_ref()
        } else {
            self.entries.get(name)
        }
    }
}

impl std::ops::Deref for DamageChart {
    type Target = Table<u8>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeData {
    /// Showdown codes: 1 weak, 2 resists, 3 immune, 0 neutral.
    pub damage_taken: DamageChart,
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
        let id = if id.starts_with("hiddenpower") {
            id.trim_end_matches(|ch: char| ch.is_ascii_digit())
        } else {
            id
        };
        self.lookup(&self.moves, dex_mod, Some(&self.regmb.moves), id)
    }

    pub(crate) fn move_entries(&self, dex_mod: Mod) -> impl Iterator<Item = &Move> {
        self.moves
            .keys()
            .filter_map(move |id| self.moves(dex_mod, id))
    }

    pub(crate) fn metronome_moves(&self) -> &[String] {
        &self.metronome_moves
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

    pub(crate) fn has_nature(&self, id: &str) -> bool {
        self.natures.contains_key(id)
    }

    pub fn type_data(&self, id: &str) -> Option<&TypeData> {
        self.types.get(id)
    }

    #[inline]
    pub(crate) fn type_data_named(&self, name: &str) -> Option<&TypeData> {
        if let Some(index) = type_slot(name) {
            self.type_by_name[index]
                .as_ref()
                .or_else(|| self.type_data(TYPE_IDS[index]))
        } else {
            self.type_data(&to_id(name))
        }
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

#[cfg(test)]
mod fast_lookup_tests {
    use super::*;

    #[test]
    fn move_flag_bits_match_the_complete_dex_and_insertions() {
        for effect in DEX.moves.values().chain(DEX.regmb.moves.values()) {
            for key in [
                "contact",
                "sound",
                "protect",
                "heal",
                "failcopycat",
                "future_flag",
            ] {
                assert_eq!(
                    effect.flags.contains_key(key),
                    effect.flags.entries.contains_key(key)
                );
            }
            for key in effect.flags.entries.keys() {
                assert!(effect.flags.contains_key(key));
            }
        }
        let mut flags: MoveFlags = serde_json::from_str(r#"{"contact":0,"unlisted":2}"#).unwrap();
        assert!(flags.contains_key("contact"));
        assert_eq!(flags.remove("contact"), Some(0));
        assert!(!flags.contains_key("contact"));
        assert_eq!(flags.insert("sound".into(), 1), None);
        assert!(flags.contains_key("sound") && flags.contains_key("unlisted"));
    }

    #[test]
    fn dense_type_values_match_raw_maps_including_missing_and_special_keys() {
        for chart in DEX.types.values() {
            for key in chart
                .damage_taken
                .entries
                .keys()
                .map(String::as_str)
                .chain(["Stellar", "electric", "Paralysis", "unlisted"])
            {
                assert_eq!(
                    chart.damage_taken.get(key),
                    chart.damage_taken.entries.get(key)
                );
            }
        }
        for name in ["Fire", "Electric", "Stellar", "???", " FIrE ", "ghost"] {
            let actual = DEX.type_data_named(name).map(|ty| &ty.damage_taken.entries);
            let expected = DEX
                .type_data(&to_id(name))
                .map(|ty| &ty.damage_taken.entries);
            assert_eq!(actual, expected);
        }
    }
}
