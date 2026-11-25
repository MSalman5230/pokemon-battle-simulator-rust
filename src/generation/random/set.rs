//! Random Champions set assembly. Move selection runs before the final item,
//! effort values, move shuffle, gender, and shininess rolls.

use super::{Candidate, DATA, TeamDetails, effectiveness};
use crate::{
    dex::{Category, DEX, Format, HitCount, Mod, Move},
    generation::{index, sample},
    input::{Set, SetStats},
    rng::Rng,
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecipeSet {
    role: String,
    #[serde(rename = "movepool")]
    pool: Vec<String>,
    abilities: Vec<String>,
    preferred_types: Option<Vec<String>>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MoveFacts {
    fixed_damage: bool,
    sheer_force: bool,
    crash: bool,
}

pub(super) struct BuildState<'a> {
    pub candidate: Candidate,
    pub details: &'a TeamDetails,
    pub lead: bool,
    pub doubles: bool,
    pub max_moves: usize,
    pub pool: &'static [String],
    pub preferred: &'static str,
    pub role: &'static str,
    pub abilities: &'static [String],
}

pub(super) fn build(
    format: &Format,
    candidate: Candidate,
    details: &TeamDetails,
    lead: bool,
    doubles: bool,
    rng: &mut Rng,
    choose_moves: impl FnOnce(&BuildState<'_>, &mut Rng) -> Vec<&'static str>,
) -> Set {
    let recipe = candidate.recipe;
    let species = candidate.species;
    let form = pick_form(candidate, rng);
    let choices = &recipe.sets[index(rng, recipe.sets.len())];
    let preferred = choices
        .preferred_types
        .as_ref()
        .map_or("", |t| t[index(rng, t.len())].as_str());
    let state = BuildState {
        candidate,
        details,
        lead,
        doubles,
        max_moves: format.max_move_count,
        pool: &choices.pool,
        preferred,
        role: &choices.role,
        abilities: recipe
            .move_abilities
            .as_deref()
            .unwrap_or(&choices.abilities),
    };
    let mut moves = choose_moves(&state, rng);
    let counts = Counter::measure(&state, &moves);
    let ability = ability(&state, &choices.abilities, &moves, &counts, rng);
    let item = item(&state, ability, &moves, &counts, rng);
    let level = format
        .adjust_level
        .or(recipe.level)
        .filter(|&l| l != 0)
        .unwrap_or(80);
    let ivs = SetStats::map(|_| 31);
    let mut evs = SetStats::map(|_| 11);
    let uses_attack = moves.iter().any(|&id| {
        let (_, m, facts) = move_ref(id);
        !facts.fixed_damage
            && (id == "shellsidearm"
                || (m.category == Category::Physical && !matches!(id, "bodypress" | "foulplay")))
    });
    if !uses_attack && !moves.contains(&"transform") {
        evs.atk = 0;
    }
    let mut rock = if ability == "Magic Guard" {
        0
    } else {
        effectiveness("Rock", species)
    };
    if recipe.id == "charizardmegax"
        || moves
            .iter()
            .any(|&m| matches!(m, "axekick" | "highjumpkick" | "jumpkick" | "supercellslam"))
    {
        rock = 2;
    }
    while evs.hp > 0 {
        let hp = ((2 * species.base_stats.hp + 31 + (2 * evs.hp - 1).max(0) as u32 + 100)
            * u32::from(level))
            / 100
            + 10;
        let valid = if moves.contains(&"substitute") && item != "Leftovers" {
            let divisible = hp.is_multiple_of(4);
            divisible == matches!(item, "Sitrus Berry" | "Salac Berry")
        } else if item == "Sitrus Berry"
            && (moves.contains(&"bellydrum") || moves.contains(&"shedtail"))
        {
            hp.is_multiple_of(2)
        } else if rock <= 0
            || ability == "Regenerator"
            || (rock == 1 && (item == "Leftovers" || doubles))
        {
            true
        } else {
            hp.is_multiple_of(4 / rock as u32) == (item == "Sitrus Berry")
        };
        if valid {
            break;
        }
        evs.hp -= 1;
    }
    if moves
        .iter()
        .any(|&id| matches!(id, "gyroball" | "metalburst" | "trickroom"))
    {
        evs.spe = 0;
    }
    rng.shuffle(&mut moves);
    let gender = species
        .gender
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| if rng.below(2) == 0 { "M" } else { "F" });
    let shiny = rng.chance(1, 1024);
    Set {
        name: species.base_species.clone(),
        species: form,
        level: Some(i32::from(level)),
        gender: gender.into(),
        shiny,
        ability: ability.into(),
        item: item.into(),
        evs,
        ivs: Some(ivs),
        moves: moves.into_iter().map(str::to_owned).collect(),
        ..Set::default()
    }
}

fn pick_form(candidate: Candidate, rng: &mut Rng) -> String {
    let Candidate { recipe, species } = candidate;
    if let Some(form) = &recipe.battle_form {
        return form.clone();
    }
    if let Some(forms) = &recipe.cosmetics {
        let i = index(rng, 1 + forms.len());
        return if i == 0 {
            species.name.clone()
        } else {
            forms[i - 1].clone()
        };
    }
    if let Some(base) = species.name.strip_suffix("-Gmax") {
        return base.into();
    }
    match species.base_species.as_str() {
        "Dudunsparce" | "Maushold" | "Polteageist" | "Sinistcha" | "Zarude" => {
            let forms = recipe.other_formes.as_deref().unwrap_or(&[]);
            let i = index(rng, 1 + forms.len());
            if i == 0 {
                species.name.clone()
            } else {
                forms[i - 1].clone()
            }
        }
        "Basculin" => sample(rng, &["Basculin", "Basculin-Blue-Striped"]).into(),
        "Magearna" => sample(rng, &["Magearna", "Magearna-Original"]).into(),
        "Squawkabilly" => sample(
            rng,
            &[
                "Squawkabilly",
                "Squawkabilly-Blue",
                "Squawkabilly-White",
                "Squawkabilly-Yellow",
            ],
        )
        .into(),
        _ => species.name.clone(),
    }
}

pub(super) fn move_ref(id: &str) -> (&'static str, &'static Move, MoveFacts) {
    let (key, facts) = DATA
        .moves
        .get_key_value(id)
        .unwrap_or_else(|| panic!("missing metadata for {id}"));
    (key, DEX.moves(Mod::Champions, key).unwrap(), *facts)
}

pub(super) fn move_type(
    state: &BuildState<'_>,
    id: &str,
    move_data: &'static Move,
) -> &'static str {
    let species = state.candidate.species;
    let has = |name: &str| state.abilities.iter().any(|a| a == name);
    match id {
        "terablast" => return state.preferred,
        "judgment" | "multiattack" | "revelationdance" => return &species.types[0],
        "ragingbull" if species.name.starts_with("Tauros-Paldea") => {
            for (form, ty) in [("Combat", "Fighting"), ("Blaze", "Fire"), ("Aqua", "Water")] {
                if species.name.ends_with(form) {
                    return ty;
                }
            }
        }
        "ivycudgel" if species.name.starts_with("Ogerpon") => {
            for (form, ty) in [
                ("Wellspring", "Water"),
                ("Hearthflame", "Fire"),
                ("Cornerstone", "Rock"),
            ] {
                if species.name.ends_with(form) {
                    return ty;
                }
            }
        }
        "terrainpulse" => {
            if has("Grassy Surge") || has("Seed Sower") {
                return "Grass";
            }
            if has("Psychic Surge") {
                return "Psychic";
            }
            if has("Electric Surge") {
                return "Electric";
            }
        }
        "weatherball" => {
            const SUN: &[&str] = &["Chlorophyll", "Drought", "Mega Sol", "Orichalcum Pulse"];
            if SUN.iter().any(|&a| has(a))
                || (state.candidate.recipe.mega
                    && species
                        .abilities
                        .values()
                        .any(|a| SUN.contains(&a.as_str())))
            {
                return "Fire";
            }
            if has("Drizzle") {
                return "Water";
            }
        }
        _ => {}
    }
    if move_data.move_type == "Normal" {
        for (ability, ty) in [
            ("Aerilate", "Flying"),
            ("Galvanize", "Electric"),
            ("Pixilate", "Fairy"),
            ("Refrigerate", "Ice"),
        ] {
            if has(ability) {
                return ty;
            }
        }
    }
    &move_data.move_type
}

pub(super) fn base_power(data: &Move) -> bool {
    data.base_power != 0 || data.handlers.iter().any(|h| h == "basePowerCallback")
}

pub(super) const RECOVERY: &[&str] = &[
    "healorder",
    "milkdrink",
    "moonlight",
    "morningsun",
    "recover",
    "roost",
    "shoreup",
    "slackoff",
    "softboiled",
    "strengthsap",
    "synthesis",
];
const QUERY_SETUP: &[&str] = &[
    "acidarmor",
    "agility",
    "autotomize",
    "bellydrum",
    "bulkup",
    "calmmind",
    "clangoroussoul",
    "coil",
    "cosmicpower",
    "curse",
    "dragondance",
    "flamecharge",
    "growth",
    "honeclaws",
    "howl",
    "irondefense",
    "meditate",
    "nastyplot",
    "noretreat",
    "poweruppunch",
    "quiverdance",
    "rockpolish",
    "shellsmash",
    "shelter",
    "shiftgear",
    "swordsdance",
    "tailglow",
    "takeheart",
    "tidyup",
    "trailblaze",
    "workup",
    "victorydance",
];
pub(super) const HAZARDS: &[&str] = &["spikes", "stealthrock", "stickyweb", "toxicspikes"];
pub(super) const PRIORITY_POKEMON: &[&str] = &[
    "golisopod",
    "golisopodmega",
    "mimikyu",
    "palafin",
    "scizor",
    "scizormega",
];
pub(super) const NO_STAB: &[&str] = &[
    "acidspray",
    "accelerock",
    "aquajet",
    "bounce",
    "breakingswipe",
    "bulletpunch",
    "chatter",
    "chloroblast",
    "clearsmog",
    "covet",
    "dragontail",
    "doomdesire",
    "electroweb",
    "eruption",
    "explosion",
    "fakeout",
    "feint",
    "fellstinger",
    "flamecharge",
    "flipturn",
    "futuresight",
    "grassyglide",
    "iceshard",
    "icywind",
    "incinerate",
    "infestation",
    "machpunch",
    "meteorbeam",
    "mortalspin",
    "nuzzle",
    "pluck",
    "pursuit",
    "quickattack",
    "rapidspin",
    "reversal",
    "selfdestruct",
    "shadowsneak",
    "skydrop",
    "snarl",
    "snaptrap",
    "strugglebug",
    "suckerpunch",
    "trailblaze",
    "uturn",
    "vacuumwave",
    "voltswitch",
    "watershuriken",
    "waterspout",
];

#[derive(Default)]
pub(super) struct Counter {
    types: HashMap<&'static str, usize>,
    pub damaging: Vec<&'static str>,
    pub stab: bool,
    pub setup: bool,
    recovery: bool,
    priority: bool,
    recoil: bool,
    hazards: bool,
    skill_link: bool,
    sheer_force: bool,
    physical: usize,
    special: usize,
    status: usize,
}

impl Counter {
    pub(super) fn type_count(&self, ty: &str) -> usize {
        self.types.get(ty).copied().unwrap_or_default()
    }
    pub(super) fn measure(state: &BuildState<'_>, moves: &[&str]) -> Self {
        let mut c = Self::default();
        let species = state.candidate.species;
        for &id in moves {
            let (source, m, facts) = move_ref(if id == "naturepower" { "triattack" } else { id });
            if facts.fixed_damage {
                c.damaging.push(source);
            } else {
                match m.category {
                    Category::Status => c.status += 1,
                    Category::Physical => c.physical += 1,
                    Category::Special => c.special += 1,
                }
            }
            c.skill_link |= matches!(m.multihit, Some(HitCount::Range([_, 5])));
            c.recoil |= m.recoil.is_some() || facts.crash;
            if base_power(m) {
                if !NO_STAB.contains(&id)
                    || (PRIORITY_POKEMON.contains(&state.candidate.recipe.id.as_str())
                        && m.priority > 0)
                {
                    let ty = move_type(state, id, m);
                    *c.types.entry(ty).or_default() += 1;
                    c.stab |= species.types.iter().any(|t| t == ty);
                    if !c.damaging.contains(&source) {
                        c.damaging.push(source);
                    }
                }
                c.priority |= m.priority > 0
                    || (id == "grassyglide" && state.abilities.iter().any(|a| a == "Grassy Surge"));
            }
            c.sheer_force |= facts.sheer_force;
            c.recovery |= RECOVERY.contains(&id);
            c.setup |= QUERY_SETUP.contains(&id);
            c.hazards |= HAZARDS.contains(&id);
        }
        c
    }
}

fn ability(
    state: &BuildState<'_>,
    options: &'static [String],
    moves: &[&str],
    c: &Counter,
    rng: &mut Rng,
) -> &'static str {
    if options.len() <= 1 {
        return &options[0];
    }
    if state.candidate.recipe.id == "toucannon" {
        if c.skill_link {
            return "Skill Link";
        }
        if c.sheer_force {
            return "Sheer Force";
        }
    }
    if options.iter().any(|a| a == "Slush Rush") && moves.contains(&"snowscape") {
        return "Slush Rush";
    }
    let d = state.details;
    let allowed: Vec<_> = options
        .iter()
        .map(String::as_str)
        .filter(|&ability| match ability {
            "Chlorophyll" | "Solar Power" => d.sun != 0,
            "Hydration" | "Swift Swim" => d.rain != 0,
            "Overgrow" => c.type_count("Grass") != 0,
            "Sand Force" | "Sand Rush" => d.sand != 0,
            "Slush Rush" => d.snow != 0,
            "Sheer Force" => c.sheer_force,
            "Skill Link" => c.skill_link,
            _ => true,
        })
        .collect();
    if !allowed.is_empty() {
        return sample(rng, &allowed);
    }
    let weather: Vec<_> = options
        .iter()
        .map(String::as_str)
        .filter(|a| {
            matches!(
                *a,
                "Chlorophyll" | "Sand Rush" | "Slush Rush" | "Solar Power" | "Swift Swim"
            )
        })
        .collect();
    if !weather.is_empty() {
        sample(rng, &weather)
    } else {
        &options[index(rng, options.len())]
    }
}

fn item(
    state: &BuildState<'_>,
    ability: &str,
    moves: &[&str],
    c: &Counter,
    rng: &mut Rng,
) -> &'static str {
    let Candidate { recipe, species } = state.candidate;
    let (id, role) = (recipe.id.as_str(), state.role);
    let has = |id: &str| moves.contains(&id);
    let any = |ids: &[&str]| ids.iter().any(|id| has(id));
    if let Some(items) = &recipe.required_items {
        return &items[index(rng, items.len())];
    }
    match id {
        "pikachu" => return "Light Ball",
        "farfetchd" | "sirfetchd" => return "Leek",
        _ => {}
    }
    if role == "Choice Item user" {
        return "Choice Scarf";
    }
    if matches!(
        ability,
        "Cheek Pouch" | "Cud Chew" | "Emergency Exit" | "Harvest" | "Ripen"
    ) || has("bellydrum")
    {
        return "Sitrus Berry";
    }
    if id == "alakazam" && rng.chance(1, 2) || id == "glimmora" {
        return "Focus Sash";
    }
    if id == "rampardos" && role == "Fast Attacker" || id == "ditto" && !state.doubles {
        return "Choice Scarf";
    }
    if id == "inteleon" && has("focusenergy") {
        return "Scope Lens";
    }
    if any(&["healingwish", "switcheroo", "trick"]) {
        return "Choice Scarf";
    }
    if ability == "Unburden" {
        return if any(&["closecombat", "leafstorm"]) {
            "White Herb"
        } else {
            "Sitrus Berry"
        };
    }
    if has("shellsmash") {
        return "White Herb";
    }
    if matches!(ability, "Magic Guard" | "Sheer Force") && id != "toucannon" {
        return "Life Orb";
    }
    if has("acrobatics") {
        return "";
    }
    if has("auroraveil") || (has("lightscreen") && has("reflect")) {
        return "Light Clay";
    }
    if has("rest") && !has("sleeptalk") && !matches!(ability, "Natural Cure" | "Shed Skin") {
        return "Chesto Berry";
    }
    if species.types.iter().any(|t| t == "Normal") && has("doubleedge") && has("fakeout") {
        return "Silk Scarf";
    }
    if id == "froslass" && has("tripleaxel")
        || has("populationbomb")
        || ability == "Hustle" && rng.chance(1, 2)
        || id == "tsareena" && role == "Offensive Protect"
    {
        return "Wide Lens";
    }
    if species.types.iter().any(|ty| ty == state.preferred) {
        return crate::generation::type_booster(state.preferred);
    }
    if state.doubles {
        if role == "Doubles Fast Attacker" {
            return "Focus Sash";
        }
        if species.base_stats.spe <= 70 && any(&["ragepowder", "followme"]) {
            return "Rocky Helmet";
        }
        if role == "Doubles Bulky Setup" && !has("dragondance") {
            return "Leftovers";
        }
        if matches!(
            role,
            "Offensive Protect" | "Doubles Wallbreaker" | "Doubles Setup Sweeper"
        ) {
            return "Life Orb";
        }
        return "Sitrus Berry";
    }
    if role == "Fast Attacker"
        && !c.priority
        && !any(&["fakeout", "trailblaze"])
        && (c.status == 0 || (c.status == 1 && has("partingshot")))
    {
        return "Choice Scarf";
    }
    if any(&["flamecharge", "kingsshield", "nuzzle", "substitute"]) {
        return "Leftovers";
    }
    if has("outrage") && c.setup {
        return "Lum Berry";
    }
    let physical_bulk = species.base_stats.hp + species.base_stats.def;
    if ability == "Rough Skin"
        || (ability == "Regenerator"
            && matches!(role, "Bulky Support" | "Bulky Attacker")
            && physical_bulk >= 180
            && rng.chance(1, 2))
        || (ability != "Regenerator"
            && !c.setup
            && c.recovery
            && physical_bulk > 200
            && rng.chance(1, 2))
    {
        return "Rocky Helmet";
    }
    if role.contains("Bulky") {
        return "Leftovers";
    }
    let rock = effectiveness("Rock", species);
    if matches!(role, "Fast Attacker" | "Setup Sweeper" | "Wallbreaker") && rock < 2 {
        return "Life Orb";
    }
    if role == "Fast Support" {
        if state.lead && !c.recovery && !c.recoil && c.hazards {
            return "Focus Sash";
        }
        if c.physical + c.special >= 3 && rock < 2 {
            return "Life Orb";
        }
    }
    "Leftovers"
}
