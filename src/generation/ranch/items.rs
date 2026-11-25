//! Ordered item selection for the pinned Roster Ranch generator.

use super::MoveFacts;
use crate::{
    choice::{TargetKind, to_id},
    dex::{Accuracy, Category, DEX, HitCount, Move, Species},
    input::SetStats,
    mechanics::{Nature, Stat},
    rng::Rng,
};

const RECOVERY: &[&str] = &[
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
// The base Gen 9 queryMoves uses this list, not the Champions move culler’s.
const SETUP: &[&str] = &[
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
const PROTECT: &[&str] = &[
    "banefulbunker",
    "burningbulwark",
    "detect",
    "kingsshield",
    "protect",
    "silktrap",
    "spikyshield",
];
const WEIGHT_MOVES: &[&str] = &["bodypress", "heatcrash", "heavyslam"];
const ROOT_MOVES: &[&str] = &["aquaring", "ingrain", "leechseed", "strengthsap"];

type MoveRow<'a> = (&'a str, &'static Move, MoveFacts);

pub(super) struct SetInfo<'a> {
    pub id: &'a str,
    pub species: &'a Species,
    pub ability: &'a str,
    pub moves: &'a [MoveRow<'a>],
    pub mega_stones: &'a [String],
    pub evs: SetStats,
    pub nature: Nature,
}

#[derive(Default)]
struct Counter {
    physical: usize,
    special: usize,
    status: usize,
    punch: usize,
    bite: usize,
    recoil: usize,
    drain: usize,
    skill_link: usize,
    sheer_force: usize,
    setup: bool,
    recovery: bool,
    priority: bool,
}

impl Counter {
    fn query(moves: &[MoveRow<'_>], ability: &str) -> Self {
        let mut c = Self::default();
        for &(id, data, facts) in moves {
            if !facts.fixed_damage {
                match data.category {
                    Category::Physical => c.physical += 1,
                    Category::Special => c.special += 1,
                    Category::Status => c.status += 1,
                }
            }
            c.skill_link += usize::from(matches!(data.multihit, Some(HitCount::Range([_, 5]))));
            c.recoil += usize::from(data.recoil.is_some() || facts.crash);
            c.drain += usize::from(data.drain.is_some());
            if data.base_power > 0 || data.handlers.iter().any(|h| h == "basePowerCallback") {
                c.bite += usize::from(data.flags.contains_key("bite"));
                c.punch += usize::from(data.flags.contains_key("punch"));
                c.priority |=
                    data.priority > 0 || (id == "grassyglide" && ability == "Grassy Surge");
            }
            c.sheer_force += usize::from(facts.sheer_force);
            c.recovery |= RECOVERY.contains(&id);
            c.setup |= SETUP.contains(&id);
        }
        c
    }
}

fn attack(row: &MoveRow<'_>) -> bool {
    row.1.category != Category::Status && !row.2.fixed_damage
}

fn inaccurate(data: &Move) -> bool {
    matches!(data.accuracy, Accuracy::Percent(n) if n < 100)
}

fn high_crit(data: &Move) -> bool {
    data.crit_ratio.unwrap_or(1) >= 2
}

fn weakness(species: &Species, ability: &str, typing: &str) -> i8 {
    if ability == "Levitate" && typing == "Ground" {
        return 0;
    }
    let mut sum = 0;
    for ty in &species.types {
        let chart = &DEX.type_data(&to_id(ty)).unwrap().damage_taken;
        match chart.get(typing) {
            Some(3) => return 0,
            Some(1) => sum += 1,
            Some(2) => sum -= 1,
            _ => {}
        }
    }
    sum
}

pub(super) fn choose<'a>(rng: &mut Rng, set: SetInfo<'a>) -> &'a str {
    let SetInfo {
        id,
        species,
        ability,
        moves,
        mega_stones,
        evs,
        nature,
    } = set;
    let c = Counter::query(moves, ability);
    let has = |id: &str| moves.iter().any(|row| row.0 == id);
    let any = |ids: &[&str]| ids.iter().any(|id| has(id));
    let attacks: Vec<_> = moves.iter().filter(|m| attack(m)).collect();
    let n = attacks.len();
    let types = |ty: &str| species.types.iter().any(|t| t == ty);
    let count = |ty| attacks.iter().filter(|m| m.1.move_type == ty).count();
    let contacts = moves
        .iter()
        .filter(|m| m.1.flags.contains_key("contact"))
        .count();
    let inaccurate_moves = moves.iter().filter(|m| inaccurate(m.1)).count();
    let high_crit_attacks = attacks.iter().filter(|m| high_crit(m.1)).count();
    let speed = species.base_stats.spe;
    let physical_bulk = species.base_stats.hp + species.base_stats.def;
    let bulk = physical_bulk + species.base_stats.spd;

    match id {
        "pikachu" => return "Light Ball",
        "farfetchd" | "sirfetchd" => return "Leek",
        "ditto" => return crate::generation::sample(rng, &["Choice Scarf", "Sitrus Berry"]),
        _ => {}
    }
    if any(&["switcheroo", "trick"]) {
        return if has("thief") {
            ""
        } else if speed < 60 || n <= 1 || ability == "Klutz" {
            "Iron Ball"
        } else {
            "Choice Scarf"
        };
    }
    if any(&["belch", "stuffcheeks"])
        || matches!(ability, "Cheek Pouch" | "Cud Chew" | "Harvest" | "Ripen")
        || (has("bellydrum")
            && attacks.iter().any(|m| {
                m.1.category == Category::Physical && !matches!(m.0, "bodypress" | "foulplay")
            }))
    {
        return "Sitrus Berry";
    }
    if has("fling") {
        return if speed <= 100 {
            "Iron Ball"
        } else {
            "King's Rock"
        };
    }
    if has("recycle") {
        return if ability == "Regenerator" {
            "Eject Button"
        } else {
            "Red Card"
        };
    }
    if matches!(ability, "Magician" | "Pickpocket" | "Pickup" | "Unburden")
        || (has("acrobatics") && ability != "Klutz")
    {
        if let Some(seed) = moves.iter().find_map(|m| match m.0 {
            "electricterrain" => Some("Electric Seed"),
            "grassyterrain" => Some("Grassy Seed"),
            "mistyterrain" => Some("Misty Seed"),
            "psychicterrain" => Some("Psychic Seed"),
            _ => None,
        }) {
            return seed;
        }
        if has("fakeout") {
            return "Normal Gem";
        }
        if moves
            .iter()
            .any(|m| m.1.self_boosts.iter().any(|(_, amount)| *amount < 0))
        {
            return "White Herb";
        }
        return if weakness(species, ability, "Ground") > 0 {
            "Air Balloon"
        } else {
            "Sitrus Berry"
        };
    }
    if has("thief") || (has("acrobatics") && ability == "Klutz") {
        return "";
    }
    if ability == "Magic Guard" {
        return "Life Orb";
    }
    if (has("lightscreen") && has("reflect"))
        || (has("auroraveil") && (ability == "Snow Warning" || has("snowscape")))
    {
        return "Light Clay";
    }

    // Species-specific rules must beat the random Mega draw and general rules.
    match id {
        "abomasnow" if ability != "Snow Warning" && !has("solarbeam") => return "Abomasite",
        "absol" => {
            if ability == "Super Luck" && high_crit_attacks > 0 {
                return "Scope Lens";
            }
            if attacks
                .iter()
                .any(|m| m.1.flags.contains_key("slicing") || m.1.move_type == "Ghost")
            {
                return "Absolite Z";
            }
            if c.special > 0 {
                return "Absolite";
            }
        }
        "aerodactyl" if contacts >= 2 => return "Aerodactylite",
        "aggron" => {
            if count("Rock") >= 2 || (ability == "Rock Head" && c.recoil > 0) {
                return "Life Orb";
            }
            if has("heavyslam") {
                return if ability == "Heavy Metal" {
                    "Life Orb"
                } else {
                    "Aggronite"
                };
            }
            if has("bodypress") {
                return "Aggronite";
            }
        }
        "alakazam" if n >= 3 => return "Alakazite",
        "altaria" if count("Normal") > 0 => return "Altarianite",
        "ampharos" if count("Dragon") > 0 => return "Ampharosite",
        "audino" => {
            return if ability == "Regenerator" {
                "Leftovers"
            } else {
                "Audinite"
            };
        }
        "banette" if c.status >= 2 => return "Banettite",
        "barbaracle" => {
            if ability == "Sniper" && high_crit_attacks >= 2 {
                return "Scope Lens";
            }
            if count("Water") >= 2 {
                return if has("shellsmash") {
                    "White Herb"
                } else {
                    "Life Orb"
                };
            }
            if count("Fighting") >= 2 {
                return "Barbaracite";
            }
        }
        "baxcalibur" if has("dracometeor") => return "Baxcalibrite",
        "beedrill" if count("Bug") >= 2 => return "Beedrillite",
        "blastoise"
            if attacks
                .iter()
                .filter(|m| m.1.flags.contains_key("pulse"))
                .count()
                >= 2 =>
        {
            return "Blastoisinite";
        }
        "blaziken" if ability != "Speed Boost" => return "Blazikenite",
        "camerupt" if c.sheer_force >= 2 || any(WEIGHT_MOVES) => return "Cameruptite",
        "charizard" => {
            if c.physical >= 2 || count("Dragon") > 0 {
                return "Charizardite X";
            }
            if has("sunnyday") {
                return if ability == "Solar Power" {
                    "Life Orb"
                } else {
                    "Heat Rock"
                };
            }
            if c.physical == 0 && ability != "Solar Power" {
                return "Charizardite Y";
            }
        }
        "chesnaught" => {
            if has("trailblaze") {
                return if c.recovery {
                    "Rocky Helmet"
                } else {
                    "Leftovers"
                };
            }
            if any(&["gyroball", "irondefense"]) {
                return "Chesnaughtite";
            }
        }
        "chimecho" if count("Steel") > 0 || has("cosmicpower") => return "Chimechite",
        "clefable" if ability == "Cute Charm" || has("airslash") => return "Clefablite",
        "crabominable" if ability != "Iron Fist" && c.punch > 0 => return "Crabominite",
        "delphox" if has("psychicterrain") => return "Terrain Extender",
        "dragalge" if has("flipturn") || ability != "Adaptability" => return "Dragalgite",
        "dragonite" if ability == "Multiscale" && (has("dragondance") || c.special == 0) => {
            return if has("outrage") {
                "Lum Berry"
            } else {
                "Leftovers"
            };
        }
        "drampa" if has("roost") && ability != "Berserk" => return "Drampanite",
        "eelektross" if c.physical >= 3 || any(&["bulkup", "coil", "heavyslam"]) => {
            return "Eelektrossite";
        }
        "emboar" => {
            if ability == "Reckless" && c.recoil >= 2 {
                return "Life Orb";
            }
            if any(WEIGHT_MOVES) {
                return "Emboarite";
            }
        }
        "excadrill" => {
            if ability == "Sand Rush" && has("sandstorm") {
                return "Life Orb";
            }
            if has("irondefense") || c.special > 0 || contacts >= 2 {
                return "Excadrite";
            }
        }
        "falinks" if has("bodypress") || ability != "Defiant" => return "Falinksite",
        "feraligatr" => {
            if ability == "Sheer Force" {
                return if any(&[
                    "breakingswipe",
                    "bulldoze",
                    "chillingwater",
                    "rocktomb",
                    "snarl",
                ]) {
                    "Feraligite"
                } else {
                    "Life Orb"
                };
            }
            if count("Normal") > 0 || count("Dragon") > 0 {
                return "Feraligite";
            }
        }
        "froslass" => {
            if has("poltergeist") && has("tripleaxel") {
                return "Wide Lens";
            }
            if has("auroraveil") {
                return "Froslassite";
            }
        }
        "gallade"
            if ability == "Sharpness"
                && attacks
                    .iter()
                    .filter(|m| m.1.flags.contains_key("slicing"))
                    .count()
                    >= 2 =>
        {
            return "Life Orb";
        }
        "garchomp" => {
            if has("dracometeor") {
                return "Garchompite Z";
            }
            if has("sandstorm") && ability != "Sand Veil" {
                return "Garchompite";
            }
            if ability == "Rough Skin" && (evs.hp == 32 || evs.def == 32) {
                return "Rocky Helmet";
            }
        }
        "gardevoir" if count("Normal") > 0 || ability == "Synchronize" => return "Gardevoirite",
        "medicham" if ability != "Pure Power" => return "Medichamite",
        "chandelure" | "gallade" | "gardevoir" | "medicham" if has("trickroom") => {
            return if evs.spe == 32 {
                "Focus Sash"
            } else {
                "Mental Herb"
            };
        }
        "gengar" if any(&["hypnosis", "perishsong"]) => return "Gengarite",
        "glalie" if count("Normal") > 0 => return "Glalitite",
        "glimmora" => {
            if c.physical >= 2 || count("Rock") >= 2 {
                return "Glimmoranite";
            }
            if ability == "Toxic Debris" && c.status >= 2 {
                return "Focus Sash";
            }
        }
        "golisopod" => {
            if count("Water") >= 2 {
                return "Sitrus Berry";
            }
            if any(&[
                "bulkup",
                "endure",
                "irondefense",
                "snarl",
                "strugglebug",
                "swordsdance",
            ]) || count("Steel") > 0
                || count("Water") == 0
            {
                return "Golisopite";
            }
        }
        "golurk" => {
            if (ability == "No Guard" && inaccurate_moves > 0)
                || (ability == "Iron Fist" && c.punch > 0)
            {
                return "Life Orb";
            }
            if contacts >= 2 || ability == "Klutz" {
                return "Golurkite";
            }
        }
        "greninja" if ability != "Protean" => return "Greninjite",
        "gyarados" if count("Dark") > 0 => return "Gyaradosite",
        "hawlucha" if ability == "Limber" || has("bodypress") || inaccurate_moves >= 2 => {
            return "Hawluchanite";
        }
        "heracross" if ability == "Swarm" || c.skill_link > 0 => return "Heracronite",
        "houndoom" if has("sunnyday") => return "Houndoominite",
        "kangaskhan"
            if attacks
                .iter()
                .filter(|m| {
                    matches!(
                        m.1.target,
                        TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes
                    )
                })
                .count()
                >= 2 =>
        {
            return "Life Orb";
        }
        "lopunny" if ability == "Klutz" => return "Lopunnite",
        "lucario" => {
            if has("finalgambit") {
                return if c.status == 0 {
                    "Choice Scarf"
                } else {
                    "Lucarionite Z"
                };
            }
            if attacks.iter().filter(|m| types(&m.1.move_type)).count() >= 2 {
                return "Lucarionite";
            }
            if has("swordsdance") {
                return if rng.chance(1, 2) {
                    "Lucarionite"
                } else {
                    "Lucarionite Z"
                };
            }
        }
        "malamar" if ability != "Contrary" && has("superpower") => return "Malamarite",
        "manectric" if any(&["charge", "eerieimpulse", "snarl"]) => return "Manectite",
        "mawile" => {
            if (ability != "Sheer Force" && c.physical >= 2)
                || (ability == "Sheer Force" && any(&["icywind", "rocktomb"]))
            {
                return "Mawilite";
            }
            if ability == "Sheer Force" && has("fireblast") {
                return "Life Orb";
            }
        }
        "meganium" if any(&["dazzlinggleam", "solarbeam", "synthesis", "weatherball"]) => {
            return "Meganiumite";
        }
        "meowstic" | "meowsticf"
            if (n >= 2 && ability == "Keen Eye")
                || (ability == "Prankster" && (n == 4 || (n == 3 && c.setup))) =>
        {
            return "Meowsticite";
        }
        "metagross" if contacts >= 2 || ability == "Light Metal" => return "Metagrossite",
        "pidgeot" if c.special >= 2 => return "Pidgeotite",
        "pinsir"
            if attacks
                .iter()
                .any(|m| m.1.move_type == "Normal" && m.0 != "guillotine") =>
        {
            return "Pinsirite";
        }
        "pyroar" if count("Fire") >= 2 => return "Pyroarite",
        "raichu" => {
            if any(&["risingvoltage", "volttackle"]) || c.physical >= 2 {
                return "Raichunite X";
            }
            if has("zapcannon") || inaccurate_moves >= 2 {
                return "Raichunite Y";
            }
        }
        "sableye" if ability != "Prankster" || n == 4 || (n == 3 && c.setup) => return "Sablenite",
        "salamence" if count("Normal") >= 2 || count("Flying") == 0 => return "Salamencite",
        "sceptile" => {
            if count("Dragon") > 0 {
                return "Sceptilite";
            }
            if has("shedtail") {
                return "Sitrus Berry";
            }
        }
        "scizor" if ability != "Technician" => return "Scizorite",
        "scolipede"
            if (ability != "Speed Boost" && evs.spe != 32)
                || (has("irondefense") && !has("batonpass")) =>
        {
            return "Scolipite";
        }
        "scovillain" if ability == "Chlorophyll" && has("sunnyday") => return "Life Orb",
        "scrafty" if ability == "Intimidate" => return "Scraftinite",
        "sharpedo" if c.bite >= 2 => return "Sharpedonite",
        "skarmory" => {
            if has("swordsdance") {
                return "Skarmorite";
            }
            if c.status >= 2 || has("bodypress") {
                return if c.recovery {
                    "Rocky Helmet"
                } else {
                    "Leftovers"
                };
            }
        }
        "slowbro"
            if ability != "Regenerator" || has("bodypress") || (c.setup && has("slackoff")) =>
        {
            return "Slowbronite";
        }
        "staraptor" => {
            if has("finalgambit") && c.status == 0 {
                return "Choice Scarf";
            }
            if ability == "Reckless" && c.recoil >= 2 {
                return "Life Orb";
            }
            if has("closecombat") {
                return "Staraptite";
            }
        }
        "starmie"
            if has("skillswap")
                || attacks
                    .iter()
                    .any(|m| m.1.category == Category::Physical && m.0 != "rapidspin") =>
        {
            return "Starminite";
        }
        "steelix" => {
            if (ability == "Sheer Force" && any(&["breakingswipe", "rocktomb"])) || has("sandstorm")
            {
                return "Steelixite";
            }
            if ability == "Rock Head" && c.recoil > 0 {
                return "Life Orb";
            }
            if any(&["bodypress", "heavyslam"]) {
                return "Steelixite";
            }
        }
        "swampert" if any(&["bulkup", "raindance"]) => return "Swampertite",
        "tyranitar" if ability != "Sand Stream" || has("bodypress") => return "Tyranitarite",
        "victreebel" => {
            if ability == "Gluttony" {
                return "Victreebelite";
            }
            if ability == "Chlorophyll" && has("sunnyday") {
                return "Life Orb";
            }
            if evs.hp == 32 && matches!(nature.minus, Some(Stat::Def | Stat::Spd)) {
                return "Victreebelite";
            }
        }
        _ => {}
    }

    if !mega_stones.is_empty() && rng.chance(1, 2) {
        return &mega_stones[crate::generation::index(rng, mega_stones.len())];
    }
    if ability == "Klutz" {
        return "";
    }
    if ability == "Sheer Force" && c.sheer_force >= 2 {
        return "Life Orb";
    }
    if (ability == "Snow Cloak" && has("snowscape")) || (ability == "Sand Veil" && has("sandstorm"))
    {
        return "Bright Powder";
    }
    match ability {
        "Drizzle" => return "Damp Rock",
        "Drought" => return "Heat Rock",
        "Sand Stream" => return "Smooth Rock",
        "Snow Warning" => return "Icy Rock",
        "Electric Surge" | "Grassy Surge" | "Misty Surge" | "Psychic Surge" => {
            return "Terrain Extender";
        }
        _ => {}
    }
    if has("tailwind") && bulk <= 220 {
        return "Focus Sash";
    }
    if has("trickroom") {
        return if bulk <= 220 {
            "Focus Sash"
        } else {
            "Mental Herb"
        };
    }
    if ability == "Emergency Exit" {
        return "Sitrus Berry";
    }
    if ability == "Quick Draw" {
        return "Quick Claw";
    }
    if has("populationbomb") {
        return "Wide Lens";
    }
    if ability == "Skill Link" && c.skill_link > 0 {
        return "King's Rock";
    }
    if has("shellsmash") {
        return "White Herb";
    }
    if has("rest") {
        return "Chesto Berry";
    }
    if any(&["outrage", "ragingfury", "thrash"]) {
        return "Lum Berry";
    }
    if has("curse") && types("Ghost") && !c.recovery {
        return "Sitrus Berry";
    }
    if moves.iter().any(|m| m.2.partially_trapped) && (any(PROTECT) || c.recovery) {
        return "Binding Band";
    }
    if weakness(species, ability, "Ground") >= 2 && rng.chance(1, 2) {
        return "Air Balloon";
    }
    let berries: Vec<_> = RESIST_BERRIES
        .iter()
        .filter_map(|&(ty, item)| (weakness(species, ability, ty) >= 2).then_some(item))
        .collect();
    if !berries.is_empty() && rng.chance(1, 2) {
        return crate::generation::sample(rng, &berries);
    }
    if n <= 1 {
        return "Mental Herb";
    }
    if c.drain + moves.iter().filter(|m| ROOT_MOVES.contains(&m.0)).count() >= 2 {
        return "Big Root";
    }
    if speed <= 60 && inaccurate_moves >= 2 {
        return "Zoom Lens";
    }
    if n == 4 && !c.priority && (60..=100).contains(&speed) {
        return "Choice Scarf";
    }
    if any(PROTECT) || moves[0].1.category == Category::Status || (bulk >= 275 && !c.recovery) {
        return "Leftovers";
    }
    if physical_bulk > 200 {
        return "Rocky Helmet";
    }
    if n == 4 || (n == 3 && (c.setup || c.recovery)) {
        return "Life Orb";
    }
    if attack(&moves[0]) {
        type_booster(&moves[0].1.move_type)
    } else {
        "Leftovers"
    }
}

const RESIST_BERRIES: &[(&str, &str)] = &[
    ("Bug", "Tanga Berry"),
    ("Dark", "Colbur Berry"),
    ("Dragon", "Haban Berry"),
    ("Electric", "Wacan Berry"),
    ("Fairy", "Roseli Berry"),
    ("Fighting", "Chople Berry"),
    ("Fire", "Occa Berry"),
    ("Flying", "Coba Berry"),
    ("Ghost", "Kasib Berry"),
    ("Grass", "Rindo Berry"),
    ("Ground", "Shuca Berry"),
    ("Ice", "Yache Berry"),
    ("Poison", "Kebia Berry"),
    ("Psychic", "Payapa Berry"),
    ("Rock", "Charti Berry"),
    ("Steel", "Babiri Berry"),
    ("Water", "Passho Berry"),
];

fn type_booster(typing: &str) -> &'static str {
    match typing {
        "Bug" => "Silver Powder",
        "Dark" => "Black Glasses",
        "Dragon" => "Dragon Fang",
        "Electric" => "Magnet",
        "Fairy" => "Fairy Feather",
        "Fighting" => "Black Belt",
        "Fire" => "Charcoal",
        "Flying" => "Sharp Beak",
        "Ghost" => "Spell Tag",
        "Grass" => "Miracle Seed",
        "Ground" => "Soft Sand",
        "Ice" => "Never-Melt Ice",
        "Normal" => "Silk Scarf",
        "Poison" => "Poison Barb",
        "Psychic" => "Twisted Spoon",
        "Rock" => "Hard Stone",
        "Steel" => "Metal Coat",
        "Water" => "Mystic Water",
        _ => "Leftovers",
    }
}
