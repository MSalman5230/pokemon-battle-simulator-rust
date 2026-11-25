//! Origin constraints shared by event-only Pokémon and move sources.

use super::LEGALITY;
use crate::{
    choice::to_id,
    dex::{DEX, Mod, Species},
    mechanics::{HIDDEN_POWER_TYPES, hidden_power_type_index},
};
use serde_json::Value;

pub(super) const STATS: [(&str, &str); 6] = [
    ("hp", "HP"),
    ("atk", "Attack"),
    ("def", "Defense"),
    ("spa", "Special Attack"),
    ("spd", "Special Defense"),
    ("spe", "Speed"),
];

pub(super) fn iv(ivs: &Value, stat: &str) -> Option<i64> {
    if ivs.is_null() {
        Some(31)
    } else {
        ivs[stat].as_i64()
    }
}

pub(super) fn hidden_power(ivs: &Value) -> &'static str {
    let values = ["hp", "atk", "def", "spe", "spa", "spd"].map(|stat| iv(ivs, stat).unwrap_or(0));
    HIDDEN_POWER_TYPES[usize::from(hidden_power_type_index(values))]
}

pub(super) fn perfect_ivs(ivs: &Value) -> usize {
    if ivs.is_null() {
        6
    } else {
        ivs.as_object()
            .into_iter()
            .flat_map(|stats| stats.values())
            .filter(|iv| iv.as_i64().is_some_and(|n| n >= 31))
            .count()
    }
}

pub(super) struct Origin<'a> {
    pub set: &'a Value,
    pub display: &'a str,
    pub gender: Option<&'a str>,
    pub hp_type: &'a str,
    pub level: i64,
}

impl Origin<'_> {
    pub(super) fn event_problems(
        &self,
        event: &Value,
        because: &str,
        event_name: &str,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let name = self.display;
        let from = event["from"].as_str().unwrap_or(event_name);
        let suffix = format!("{because} from {from}");
        if event["generation"]
            .as_u64()
            .is_some_and(|generation| generation > 9)
        {
            problems.push(format!(
                "This format is in gen 9 and {name} is from gen {}{suffix}.",
                event["generation"]
            ));
        }
        if event["japan"] == true {
            problems.push(format!("{name} has moves from Japan-only events, but this format simulates International Yellow/Crystal which can't trade with Japanese games."));
        }
        if let Some(required) = event["level"]
            .as_i64()
            .filter(|&required| self.level < required)
        {
            problems.push(format!("{name} must be at least level {required}{suffix}."));
        }
        if event["shiny"] == true && self.set["shiny"] != true {
            problems.push(format!("{name} must be shiny{suffix}."));
        } else if (event["shiny"].is_null() || event["shiny"] == false || event["shiny"] == 0)
            && self.set["shiny"] == true
        {
            problems.push(format!("{name} must not be shiny{suffix}."));
        }
        if let (Some(actual), Some(required)) = (self.gender, event["gender"].as_str())
            && actual != required
        {
            problems.push(format!("{name}'s gender must be {required}{suffix}."));
        }
        let ivs = &self.set["ivs"];
        let event_ivs = &event["ivs"];
        if let Some(values) = event_ivs.as_object() {
            for (stat, label) in STATS {
                if let Some(required) = values.get(stat).and_then(Value::as_i64) {
                    let actual = iv(ivs, stat);
                    if actual != Some(required) && !(self.level >= 50 && actual == Some(31)) {
                        problems.push(format!("{name} must have {required} {label} IVs{suffix}."));
                    }
                }
            }
            if self.level >= 50 && values.len() >= 6 {
                let required = hidden_power(event_ivs);
                if !self.hp_type.is_empty() && self.hp_type != required {
                    problems.push(format!(
                        "{name} can only have Hidden Power {required}{suffix}."
                    ));
                }
            }
        } else if let Some(required) = event["perfectIVs"]
            .as_u64()
            .filter(|&required| required > 0)
        {
            if (perfect_ivs(ivs) as u64) < required {
                problems.push(format!(
                    "{name} must have at least {required} perfect IVs{suffix}."
                ));
            }
            if required >= 3 && self.hp_type == "Fighting" {
                problems.push(format!("{name} can't use Hidden Power Fighting because it must have at least three perfect IVs{suffix}."));
            } else if required >= 5
                && !matches!(
                    self.hp_type,
                    "" | "Dark" | "Dragon" | "Electric" | "Steel" | "Ice"
                )
            {
                problems.push(format!("{name} can only use Hidden Power Dark/Dragon/Electric/Steel/Ice because it must have at least 5 perfect IVs{suffix}."));
            }
        }
        problems
    }
}

pub(super) fn go_problems(
    dex: Mod,
    species: &Species,
    set: &Value,
    level: i64,
    name: &str,
) -> Option<Vec<String>> {
    let species_id = to_id(&species.name);
    let rules = LEGALITY.species(dex, &species_id)?;
    let Some(go) = &rules.pokemon_go else {
        let other = rules.changes_from.as_deref().unwrap_or(&rules.prevo);
        if rules.evo_level.is_none() && !other.is_empty() {
            return go_problems(dex, DEX.species(dex, &to_id(other))?, set, level, name);
        }
        return None;
    };
    let has = |source: &str| go.encounters.iter().any(|entry| entry == source);
    let shiny = set["shiny"] == true;
    let name = if shiny {
        format!("Shiny {name}")
    } else {
        name.to_owned()
    };
    if shiny && has("noshiny") {
        return Some(vec![format!("{name} is not obtainable from Pokemon GO.")]);
    }
    let mut min_level = 50;
    let mut min_ivs = 15;
    let mut source = |l: i64, iv: i64| {
        min_level = min_level.min(l);
        min_ivs = min_ivs.min(iv);
    };
    for (method, level, iv) in [
        ("wild", 1, 0),
        ("egg", 2, 10),
        ("12kmegg", 8, 10),
        ("raid", 20, 10),
        (
            "research",
            if species_id == "cresselia" { 10 } else { 15 },
            10,
        ),
    ] {
        if has(method) {
            source(level, iv);
        }
    }
    if species_id == "mewtwo" && level >= 20 {
        source(50, 0);
    }
    if !shiny && has("giovanni") {
        source(8, 1);
    }
    if !(has("notrade") || shiny && species_id == "deoxys") {
        source(12, i64::from(has("specialtrade") || shiny));
    }
    let mut problems = Vec::new();
    if level < min_level {
        problems.push(format!(
            "{name} must be at least level {min_level} to be from Pokemon GO."
        ));
    }
    let min = min_ivs * 2 + 1;
    let ivs = &set["ivs"];
    if STATS[..5]
        .iter()
        .any(|(stat, _)| iv(ivs, stat).is_some_and(|value| value < min))
    {
        let unit = if min == 1 { "IV" } else { "IVs" };
        problems.push(format!(
            "{name} must have at least {min} {unit} in non-Speed stats to be from Pokemon GO."
        ));
    }
    if STATS[..5]
        .iter()
        .any(|(stat, _)| iv(ivs, stat).is_some_and(|value| value % 2 == 0))
    {
        problems.push(format!(
            "{name} must have odd non-Speed IVs to be from Pokemon GO."
        ));
    }
    for (first, second, label) in [
        ("atk", "spa", "Atk and Sp. Atk"),
        ("def", "spd", "Def and Sp. Def"),
    ] {
        let first = iv(ivs, first);
        let second = iv(ivs, second);
        if first != second && !(level >= 50 && (first == Some(31) || second == Some(31))) {
            problems.push(format!(
                "{name}'s {label} IVs must match to be from Pokemon GO."
            ));
        }
    }
    Some(problems)
}
