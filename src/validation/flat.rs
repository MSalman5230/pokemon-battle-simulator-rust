//! Team and set validation for standard Champions Flat Rules.

use super::{FormatRules, LEGALITY, MoveSources};
use crate::{
    choice::to_id,
    dex::{DEX, Format, Species},
};
use serde_json::Value;
use std::{collections::HashSet, fmt};

#[derive(Debug, PartialEq, Eq)]
pub struct ValidationError(String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not ported: {}", self.0)
    }
}

impl std::error::Error for ValidationError {}

pub struct TeamValidator {
    format: &'static Format,
    rules: &'static FormatRules,
}

struct CheckedSet {
    problems: Vec<String>,
    species: Option<&'static Species>,
    nickname: String,
    item_id: String,
    item_label: &'static str,
}

impl CheckedSet {
    fn invalid(message: String) -> Self {
        Self {
            problems: vec![message],
            species: None,
            nickname: String::new(),
            item_id: String::new(),
            item_label: "",
        }
    }
}

fn string<'a>(set: &'a Value, key: &str) -> &'a str {
    set.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn nickname(name: &str) -> String {
    // Dex.getName normalizes punctuation and whitespace before truncating to
    // 18 UTF-16 code units, then removes long runs of combining marks.
    let space_normalized: String = name
        .chars()
        .map(|ch| match ch {
            '|' | ',' | '[' | ']' | '\u{202e}' | '\u{feff}' => ' ',
            _ => ch,
        })
        .collect();
    let cleaned = space_normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let truncated = String::from_utf16_lossy(&cleaned.encode_utf16().take(18).collect::<Vec<_>>());
    let mut result = String::new();
    let mut marks = Vec::new();
    for ch in truncated.trim().chars().chain(std::iter::once('\u{239b}')) {
        if matches!(ch, '\u{0300}'..='\u{036f}' | '\u{0483}'..='\u{0489}' | '\u{0610}'..='\u{0615}' |
            '\u{064b}'..='\u{065f}' | '\u{0670}' | '\u{06d6}'..='\u{06dc}' | '\u{06df}'..='\u{06ed}' |
            '\u{0e31}' | '\u{0e34}'..='\u{0e3a}' | '\u{0e47}'..='\u{0e4e}')
        {
            marks.push(ch);
            continue;
        }
        if marks.len() < 3 {
            result.extend(&marks);
        }
        marks.clear();
        if !matches!(ch, '\u{239b}'..='\u{23b9}') {
            result.push(ch);
        }
    }
    result
}

impl TeamValidator {
    pub fn new(format_id: &str) -> Result<Self, ValidationError> {
        let id = to_id(format_id);
        let format = DEX
            .format(&id)
            .ok_or_else(|| ValidationError(format!("team validation for {format_id}")))?;
        let rules = LEGALITY
            .format(&id)
            .expect("Champions legality rules are projected");
        if !matches!(
            id.as_str(),
            "gen9championsbssregmc"
                | "gen9championsbssregmb"
                | "gen9championsvgc2026regmc"
                | "gen9championsvgc2026regmb"
                | "gen9championsvgc2026regmcbo3"
                | "gen9championsvgc2026regmbbo3"
        ) {
            return Err(ValidationError(format!(
                "team validation for {}",
                format.name
            )));
        }
        Ok(Self { format, rules })
    }

    pub fn validate_set(&self, value: &Value) -> Result<Option<Vec<String>>, ValidationError> {
        let problems = self.check_set(value)?.problems;
        Ok((!problems.is_empty()).then_some(problems))
    }

    pub fn validate_team(&self, team: &[Value]) -> Result<Option<Vec<String>>, ValidationError> {
        let mut problems = Vec::new();
        if team.len() > self.format.max_team_size {
            return Ok(Some(vec![format!(
                "You may only bring up to {} Pokémon (your team has {}).",
                self.format.max_team_size,
                team.len()
            )]));
        }
        if team.len() < self.format.min_team_size {
            problems.push(format!(
                "You must bring at least {} Pokémon (your team has {}).",
                self.format.min_team_size,
                team.len()
            ));
        }
        if team.iter().any(Value::is_null) {
            return Ok(Some(vec!["You sent invalid team data. If you're not using a custom client, please report this as a bug.".into()]));
        }
        let mut checked = Vec::with_capacity(team.len());
        for set in team {
            let mut result = self.check_set(set)?;
            problems.append(&mut result.problems);
            checked.push(result);
        }
        let mut nums = HashSet::new();
        if let Some(duplicate) = checked
            .iter()
            .filter_map(|set| set.species)
            .find(|species| {
                let data = self.species_rules(species);
                !nums.insert(data.num)
            })
        {
            problems.extend([
                "You are limited to one of each Pokemon by Species Clause.".into(),
                format!("(You have more than one {})", duplicate.base_species),
            ]);
        }
        let mut names = HashSet::new();
        if let Some(duplicate) = checked
            .iter()
            .filter(|set| {
                !set.nickname.is_empty()
                    && set
                        .species
                        .is_some_and(|species| species.base_species != set.nickname)
            })
            .find(|set| !names.insert(set.nickname.as_str()))
        {
            problems.extend([
                "Your Pokemon must have different nicknames.".into(),
                format!("(You have more than one {})", duplicate.nickname),
            ]);
        }
        if self.rules.item_limit > 0
            && let Some(repeated) =
                checked
                    .iter()
                    .filter(|set| !set.item_id.is_empty())
                    .find(|set| {
                        checked.iter().filter(|s| s.item_id == set.item_id).count()
                            > usize::from(self.rules.item_limit)
                    })
        {
            let limit = self.rules.item_limit;
            problems.extend([
                format!("You are limited to {limit} of each item by Item Clause."),
                format!("(You have more than {limit} {})", repeated.item_label),
            ]);
        }
        Ok((!problems.is_empty()).then_some(problems))
    }

    fn species_rules(&self, species: &Species) -> &'static super::SpeciesRules {
        LEGALITY
            .species(self.format.dex_mod, &to_id(&species.name))
            .expect("dex and legality names must agree")
    }

    fn ban_reason(&self, key: &str) -> Option<String> {
        self.rules.rule_sources.get(&format!("-{key}")).map(|rule| {
            if rule.is_empty() {
                "banned".into()
            } else {
                format!("banned by {rule}")
            }
        })
    }

    fn check_set(&self, set: &Value) -> Result<CheckedSet, ValidationError> {
        if set.is_null() {
            return Ok(CheckedSet::invalid("This is not a Pokemon.".into()));
        }
        let dex = self.format.dex_mod;
        let original_id = to_id(string(set, "species"));
        let Some(original) = DEX.species(dex, &original_id) else {
            return Ok(CheckedSet::invalid(format!(
                "The Pokemon \"{original_id}\" does not exist."
            )));
        };
        let item_input = to_id(string(set, "item"));
        let item = DEX.item(dex, &item_input);
        if !item_input.is_empty() && item.is_none() {
            return Ok(CheckedSet::invalid(format!(
                "\"{item_input}\" is an invalid item."
            )));
        }
        let item_label = item.map_or("", |item| item.effect.name.as_str());
        let item_id = to_id(item_label);
        let ability_input = to_id(string(set, "ability"));
        let ability = DEX.ability(if ability_input.is_empty() {
            "noability"
        } else {
            &ability_input
        });
        let Some(ability) = ability else {
            return Ok(CheckedSet::invalid(format!(
                "\"{ability_input}\" is an invalid ability."
            )));
        };
        let mut moves: Vec<_> = Vec::new();
        for value in set
            .get("moves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|v| !v.is_null())
        {
            let id = to_id(value.as_str().unwrap_or_default());
            if id.is_empty() {
                continue;
            }
            let Some(data) = DEX.moves(dex, &id) else {
                return Ok(CheckedSet::invalid(format!("\"{id}\" is an invalid move.")));
            };
            moves.push(data);
        }
        if set["gigantamax"] == true
            || original_id.ends_with("gmax")
            || !string(set, "hpType").is_empty()
            || !set["dynamaxLevel"].is_null()
        {
            return Err(ValidationError(format!(
                "special set options for {}",
                original.name
            )));
        }
        let original_rules = self.species_rules(original);
        let mut problems = Vec::new();
        let raw_name = string(set, "name");
        let mut name = nickname(raw_name);
        if name.is_empty() {
            name.clone_from(&original.base_species);
        }
        if raw_name.encode_utf16().count() > 18 && raw_name != original.name {
            problems.extend([format!("{}'s nickname \"{raw_name}\" is too long.", original.name), format!("(It's {} characters long, but should be 18 or less. Some characters, like emojis, may count as more than one.)", raw_name.encode_utf16().count())]);
        }
        let display = if name != original.name && name != original.base_species {
            format!("{name} ({})", original.name)
        } else {
            original.name.clone()
        };
        let mut level = set["level"]
            .as_i64()
            .filter(|&n| n != 0)
            .unwrap_or(i64::from(self.format.default_level));
        if self
            .format
            .adjust_level
            .is_some_and(|adjust| level == i64::from(adjust))
        {
            level = i64::from(self.format.max_level);
        }
        if level < i64::from(self.rules.min_level) {
            problems.push(format!(
                "{display} (level {level}) is below the minimum level of {}",
                self.rules.min_level
            ));
        }
        if level > i64::from(self.format.max_level) {
            problems.push(format!(
                "{display} (level {level}) is above the maximum level of {}",
                self.format.max_level
            ));
        }
        let mut species = original;
        if let Some(battle_only) = &original_rules.battle_only {
            let Some(base) = battle_only.as_str() else {
                return Err(ValidationError(format!(
                    "alternative starting forms of {}",
                    original.name
                )));
            };
            if let Some(required) = &original_rules.required_ability
                && ability.name != *required
            {
                problems.push(format!(
                    "{} transforms in-battle with {required}, please fix its ability.",
                    original.name
                ));
            }
            if let Some(required) = &original_rules.required_items
                && !required.iter().any(|name| name == item_label)
            {
                problems.push(format!(
                    "{} transforms in-battle with {}, please fix its item.",
                    original.name,
                    required.join(" or ")
                ));
            }
            if let Some(required) = &original_rules.required_move
                && !moves.iter().any(|data| data.name == *required)
            {
                problems.push(format!(
                    "{} transforms in-battle with {required}, please fix its moves.",
                    original.name
                ));
            }
            species = DEX
                .species(dex, &to_id(base))
                .expect("battle form start species exists");
        } else if original_rules
            .required_items
            .as_ref()
            .is_some_and(|items| !items.iter().any(|name| name == item_label))
            || original_rules
                .required_move
                .as_ref()
                .is_some_and(|m| !moves.iter().any(|data| data.name == *m))
        {
            return Err(ValidationError(format!(
                "required item or move for {}",
                original.name
            )));
        }
        if matches!(
            species.base_species.as_str(),
            "Zygarde" | "Terapagos" | "Necrozma" | "Arceus" | "Genesect" | "Giratina" | "Silvally"
        ) || ability.name == "Battle Bond"
            || (species.base_species == "Rockruff" && ability.name == "Own Tempo")
        {
            return Err(ValidationError(format!(
                "special form legality for {}",
                species.name
            )));
        }
        let tier_species = item
            .and_then(|it| it.mega_stone.as_ref())
            .and_then(|table| table.get(&species.name))
            .and_then(|form| DEX.species(dex, &to_id(form)))
            .unwrap_or(species);
        let species_id = to_id(&species.name);
        let nature = to_id(string(set, "nature"));
        if !nature.is_empty() && !DEX.has_nature(&nature) {
            problems.push(format!("\"{nature}\" is an invalid nature."));
        }
        let species_rules = self.species_rules(tier_species);
        let display_species = if name == tier_species.name {
            name.clone()
        } else {
            format!("{name} ({})", tier_species.name)
        };
        if let Some(reason) = self.ban_reason(&format!("pokemon:{species_id}")) {
            problems.push(format!("{} is {reason}.", species.name));
        } else if let Some(tag) = species_rules
            .tags
            .iter()
            .find(|tag| self.ban_reason(&format!("tag:{}", to_id(tag))).is_some())
        {
            let reason = self.ban_reason(&format!("tag:{}", to_id(tag))).unwrap();
            problems.push(format!(
                "{display_species} is tagged {tag}, which is {reason}."
            ));
        } else if let Some(label) = &tier_species.is_nonstandard {
            problems.push(nonexistent(&display_species, label));
        }
        if let Some(item) = item
            && let Some(label) = &item.effect.is_nonstandard
        {
            problems.push(nonexistent(&format!("{name}'s item {item_label}"), label));
        }
        if ability.name == "No Ability" {
            problems.push(format!("{display} needs to have an ability."));
        } else if !species
            .abilities
            .values()
            .any(|value| value == &ability.name)
            && tier_species.abilities.get("0") != Some(&ability.name)
        {
            problems.push(format!("{display} can't have {}.", ability.name));
        }
        if species.unreleased_hidden && species.abilities.get("H") == Some(&ability.name) {
            problems.push(format!("{display}'s Hidden Ability is unreleased."));
        }
        if let Some(label) = &ability.is_nonstandard {
            problems.push(nonexistent(
                &format!("{name}'s ability {}", ability.name),
                label,
            ));
        }
        if moves.is_empty() {
            problems.push(format!(
                "{display} has no moves (it must have at least one to be usable)."
            ));
        }
        if moves.len() > self.format.max_move_count {
            problems.push(format!(
                "{display} has {} moves, which is more than the limit of {}.",
                moves.len(),
                self.format.max_move_count
            ));
        } else {
            self.check_stats(set, &name, level, &nature, &mut problems)?;
            let mut seen = HashSet::new();
            for data in &moves {
                if !seen.insert(data.name.as_str()) {
                    problems.push(format!("{display} has multiple copies of {}.", data.name));
                }
            }
            for data in &moves {
                if let Some(reason) = self.ban_reason(&format!("move:{}", to_id(&data.name))) {
                    problems.push(format!("{name}'s move {} is {reason}.", data.name));
                } else if let Some(label) = &data.is_nonstandard {
                    problems.push(nonexistent(&format!("{name}'s move {}", data.name), label));
                }
            }
            for data in &moves {
                match LEGALITY.move_sources(
                    dex,
                    &species_id,
                    &to_id(&data.name),
                    level.clamp(1, i64::from(u16::MAX)) as u16,
                ) {
                    MoveSources::Unrestricted => {}
                    MoveSources::Absent => {
                        problems.push(format!("{display} can't learn {}.", data.name));
                        break;
                    }
                    MoveSources::Conditional | MoveSources::Missing => {
                        return Err(ValidationError(format!(
                            "learnset prerequisites for {} using {}",
                            species.name, data.name
                        )));
                    }
                }
            }
        }
        if let Some(other) = DEX
            .species(dex, &to_id(&name))
            .filter(|other| other.name.eq_ignore_ascii_case(&name))
        {
            if other.base_species == species.base_species {
                name.clone_from(&species.base_species);
            } else {
                problems.push(format!("{display} must not be nicknamed a different Pokémon species than what it actually is."));
            }
        }
        if problems.is_empty()
            && self
                .species_rules(species)
                .evo_level
                .is_some_and(|n| level < i64::from(n))
        {
            return Err(ValidationError(format!(
                "obtainability prerequisites for {}",
                species.name
            )));
        }
        if let Some(events) = self.event_for(species)? {
            self.check_modern_events(set, species, &display, level, events, &mut problems)?;
        }
        Ok(CheckedSet {
            problems,
            species: Some(species),
            nickname: name,
            item_id,
            item_label,
        })
    }

    fn event_for(&self, species: &Species) -> Result<Option<&'static [Value]>, ValidationError> {
        let dex = self.format.dex_mod;
        let rules = self.species_rules(species);
        let direct = LEGALITY
            .learnset(dex, &to_id(&species.name))
            .filter(|table| table.event_only);
        let source = direct.or_else(|| {
            LEGALITY
                .learnset(dex, &to_id(&rules.prevo))
                .filter(|table| table.event_only)
        });
        let Some(source) = source else {
            return Ok(None);
        };
        let events = source
            .events
            .as_deref()
            .or_else(|| {
                (!rules.forme.is_empty())
                    .then(|| LEGALITY.learnset(dex, &to_id(&species.base_species)))
                    .flatten()
                    .filter(|table| table.event_only)
                    .and_then(|table| table.events.as_deref())
            })
            .ok_or_else(|| ValidationError(format!("missing event data for {}", species.name)))?;
        Ok(Some(events))
    }

    fn check_modern_events(
        &self,
        set: &Value,
        species: &Species,
        display: &str,
        level: i64,
        events: &[Value],
        problems: &mut Vec<String>,
    ) -> Result<(), ValidationError> {
        let mut reasons = Vec::new();
        let mut unresolved = false;
        for (index, event) in events.iter().enumerate() {
            let generation = event["generation"].as_u64().unwrap_or(0);
            if generation > 9 {
                continue;
            }
            if generation < 8
                || event["japan"] == true
                || !event["from"].is_null()
                || !event["ivs"].is_null()
                || !event["perfectIVs"].is_null()
            {
                unresolved = true;
                continue;
            }
            let suffix = if events.len() > 1 {
                format!(" to be from its event #{}", index + 1)
            } else {
                " to be from its event".into()
            };
            let mut reasons_for_event = Vec::new();
            if let Some(required) = event["level"].as_i64().filter(|&required| level < required) {
                reasons_for_event.push(format!(
                    "{display} must be at least level {required}{suffix}."
                ));
            }
            let shiny = set["shiny"] == true;
            if event["shiny"] == true && !shiny {
                reasons_for_event.push(format!("{display} must be shiny{suffix}."));
            } else if (event["shiny"].is_null() || event["shiny"] == false) && shiny {
                reasons_for_event.push(format!("{display} must not be shiny{suffix}."));
            }
            let gender = species
                .gender
                .as_deref()
                .or_else(|| Some(string(set, "gender")).filter(|g| matches!(*g, "M" | "F")));
            if let (Some(actual), Some(required)) = (gender, event["gender"].as_str())
                && actual != required
            {
                reasons_for_event.push(format!("{display}'s gender must be {required}{suffix}."));
            }
            if reasons_for_event.is_empty() {
                return Ok(());
            }
            reasons.extend(reasons_for_event);
        }
        if unresolved {
            return Err(ValidationError(format!(
                "event constraints for {}",
                species.name
            )));
        }
        if reasons.is_empty() {
            return Err(ValidationError(format!(
                "event-only origin for {}",
                species.name
            )));
        }
        problems.push(format!(
            "{} is only obtainable from {}:",
            species.name,
            if events.len() == 1 {
                "an event - it needs to match its event"
            } else {
                "events - it needs to match one of its events"
            }
        ));
        problems.extend(reasons);
        Ok(())
    }

    fn check_stats(
        &self,
        set: &Value,
        name: &str,
        level: i64,
        nature: &str,
        problems: &mut Vec<String>,
    ) -> Result<(), ValidationError> {
        if set["ivs"]
            .as_object()
            .is_some_and(|ivs| ivs.values().any(|value| value.as_i64() != Some(31)))
        {
            problems.push(format!(
                "{name}'s IVs are not maxed out, but this format requires all IVs to be 31."
            ));
        }
        const STATS: [(&str, &str); 6] = [
            ("hp", "HP"),
            ("atk", "Attack"),
            ("def", "Defense"),
            ("spa", "Special Attack"),
            ("spd", "Special Defense"),
            ("spe", "Speed"),
        ];
        let mut points = [0_i64; 6];
        for (i, (id, _)) in STATS.iter().enumerate() {
            if let Some(value) = set.get("evs").and_then(|evs| evs.get(*id)) {
                points[i] = value.as_i64().ok_or_else(|| {
                    ValidationError(format!("nonintegral Stat Points for {name}"))
                })?;
            }
        }
        for (i, (_, label)) in STATS.iter().enumerate() {
            if points[i] < 0 {
                problems.push(format!(
                    "{name} has less than 0 Awakening Values in {label}."
                ));
            }
        }
        for (i, (_, label)) in STATS.iter().enumerate() {
            if points[i] > 32 {
                problems.push(format!("{name} has more than 32 Stat Points in {label}."));
            }
        }
        let total: i128 = points.into_iter().map(i128::from).sum();
        if level > 1 && total == 0 && matches!(nature, "" | "serious") {
            problems.push(format!("{name} has exactly 0 Stat Points - did you forget to invest it? (If this was intentional, change your Nature to a different neutral Nature, which won't change its stats but will tell us that it wasn't a mistake)."));
        }
        if let Some(cap) = self.rules.ev_limit
            && total > i128::from(cap)
        {
            problems.push(format!("{name} has {total} total Stat Points, which is more than this format's limit of {cap}."));
        }
        Ok(())
    }
}

fn nonexistent(display: &str, label: &str) -> String {
    let suffix = match label {
        "Past" | "Future" => "does not exist in Gen 9.",
        "Unobtainable" => "is not obtainable without hacking or glitches.",
        "CAP" => "is made up for Smogon CAP and does not exist in this game.",
        "LGPE" => "does not exist in this game, only in Let's Go Pikachu/Eevee.",
        "Legends" => "does not exist in this game, only in Pokémon Legends: Arceus/Z-A.",
        _ => "does not exist in this game.",
    };
    format!("{display} {suffix}")
}
