//! Team and set validation for the standard Champions competitive formats.

use super::{
    FormatRules, LEGALITY, MoveSources,
    natdex::{NATDEX, NatDexCatalog},
};
use crate::{
    choice::to_id,
    dex::{DEX, Effect, Format, Item, Move, Species},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

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
    natdex: Option<&'static NatDexCatalog>,
}

struct CheckedSet {
    problems: Vec<String>,
    species: Option<&'static Species>,
    nickname: String,
    item_id: String,
    item_label: &'static str,
    facts: Vec<String>,
}

impl CheckedSet {
    fn invalid(message: String) -> Self {
        Self {
            problems: vec![message],
            species: None,
            nickname: String::new(),
            item_id: String::new(),
            item_label: "",
            facts: Vec::new(),
        }
    }
}

fn string<'a>(set: &'a Value, key: &str) -> &'a str {
    set.get(key).and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn move_id(data: &Move) -> String {
    if data.name.starts_with("Hidden Power") {
        "hiddenpower".into()
    } else {
        to_id(&data.name)
    }
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
        if !["flatrules", "standard", "standarddraft", "standarddoubles"]
            .into_iter()
            .any(|r| format.has_rule(r))
        {
            return Err(ValidationError(format!(
                "team validation for {}",
                format.name
            )));
        }
        let natdex = format.has_rule("natdexmod").then(|| &*NATDEX);
        Ok(Self {
            format,
            rules,
            natdex,
        })
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
        for super::Combination(rule, source, limit, bans) in &self.rules.complex_team_bans {
            let limit = limit.unwrap_or(0) as usize;
            let count: usize = bans
                .iter()
                .map(|ban| {
                    let matches = checked.iter().filter(|s| s.facts.contains(ban)).count();
                    if limit == 0 {
                        usize::from(matches > 0)
                    } else {
                        matches
                    }
                })
                .sum();
            let clause = rule_suffix(source);
            if limit > 0 && count > limit {
                problems.push(format!("You are limited to {limit} of {rule}{clause}."));
            } else if limit == 0 && count >= bans.len() {
                problems.push(format!(
                    "Your team has the combination of {rule}, which is banned{clause}."
                ));
            }
        }
        for rule in &self.format.rules {
            match rule.as_str() {
                "speciesclause" => {
                    let mut nums = HashSet::new();
                    if let Some(duplicate) = checked
                        .iter()
                        .filter_map(|s| s.species)
                        .find(|species| !nums.insert(self.species_rules(species).num))
                    {
                        problems.extend([
                            "You are limited to one of each Pokemon by Species Clause.".into(),
                            format!("(You have more than one {})", duplicate.base_species),
                        ]);
                    }
                }
                "nicknameclause" => {
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
                }
                "itemclause" if self.rules.item_limit > 0 => {
                    let limit = self.rules.item_limit;
                    let mut items = HashMap::new();
                    for set in checked.iter().filter(|s| !s.item_id.is_empty()) {
                        *items.entry(set.item_id.as_str()).or_insert(0) += 1;
                    }
                    if let Some(repeated) = checked
                        .iter()
                        .find(|s| items.get(s.item_id.as_str()).copied().unwrap_or(0) > limit)
                    {
                        problems.extend([
                            format!("You are limited to {limit} of each item by Item Clause."),
                            format!("(You have more than {limit} {})", repeated.item_label),
                        ]);
                    }
                }
                _ => {}
            }
        }
        Ok((!problems.is_empty()).then_some(problems))
    }

    fn species_rules(&self, species: &Species) -> &'static super::SpeciesRules {
        LEGALITY
            .species(self.format.dex_mod, &to_id(&species.name))
            .expect("dex and legality names must agree")
    }

    fn named_species(&self, name: &str) -> &'static Species {
        DEX.species(self.format.dex_mod, &to_id(name))
            .expect("projected form refers to an existing species")
    }

    fn validate_forme(
        &self,
        original: &'static Species,
        item: Option<&Item>,
        ability: &mut &'static Effect,
        moves: &mut [&'static Move],
        name: &str,
        problems: &mut Vec<String>,
    ) -> &'static Species {
        let rules = self.species_rules(original);
        let item_name = item.map_or("", |item| item.effect.name.as_str());
        let knows = |name: &str| moves.iter().any(|data| data.name == name);
        let missing_item = rules
            .required_items
            .as_ref()
            .filter(|items| !items.iter().any(|name| name == item_name));
        let missing_move = rules.required_move.as_deref().filter(|name| !knows(name));
        let mut species = original;
        if original.name == "Necrozma-Ultra" {
            if item_name != "Ultranecrozium Z" {
                problems
                    .push("Necrozma-Ultra must start the battle holding Ultranecrozium Z.".into());
            } else {
                let start = match (knows("Sunsteel Strike"), knows("Moongeist Beam")) {
                    (true, false) => Some("Necrozma-Dusk-Mane"),
                    (false, true) => Some("Necrozma-Dawn-Wings"),
                    _ => None,
                };
                if let Some(start) = start {
                    species = self.named_species(start);
                    *ability = DEX.ability("prismarmor").unwrap();
                } else {
                    problems.push("Necrozma-Ultra must start the battle as Necrozma-Dusk-Mane or Necrozma-Dawn-Wings holding Ultranecrozium Z. Please specify which Necrozma it should start as.".into());
                }
            }
        } else if original.base_species == "Zygarde" {
            if matches!(original.name.as_str(), "Zygarde-Complete" | "Zygarde-Mega") {
                problems.push(format!("{} must start the battle as Zygarde or Zygarde-10% with Power Construct. Please specify which Zygarde it should start as.", original.name));
            }
            if item_name == "Zygardite" && ability.name != "Power Construct" {
                problems.push("Zygarde holding Zygardite can only Mega Evolve with the Power Construct ability.".into());
            }
        } else if original.base_species == "Terapagos" {
            species = self.named_species("Terapagos");
            *ability = DEX.ability("terashift").unwrap();
        } else if let Some(battle_only) = &rules.battle_only {
            if let Some(required) = &rules.required_ability
                && ability.name != *required
            {
                problems.push(format!(
                    "{} transforms in-battle with {required}, please fix its ability.",
                    original.name
                ));
            }
            if let Some(items) = missing_item {
                problems.push(format!(
                    "{} transforms in-battle with {}, please fix its item.",
                    original.name,
                    items.join(" or ")
                ));
            }
            if let Some(required) = missing_move {
                problems.push(format!(
                    "{} transforms in-battle with {required}, please fix its moves.",
                    original.name
                ));
            }
            species = self.named_species(
                battle_only
                    .as_str()
                    .expect("alternative starting forms handled above"),
            );
        } else {
            let original_form = || {
                self.species_rules(
                    self.named_species(
                        rules
                            .changes_from
                            .as_deref()
                            .unwrap_or(&original.base_species),
                    ),
                )
            };
            if let Some(items) = missing_item {
                let items = items.join(" or ");
                if matches!(original.base_species.as_str(), "Arceus" | "Silvally") {
                    problems.extend([
                        format!("{name} needs to hold {items}."),
                        "(It will revert to its Normal forme if you remove the item or give it a different item.)".into(),
                    ]);
                } else {
                    let base = &original_form().base_forme;
                    let base = if base.is_empty() { "base" } else { base };
                    problems.extend([
                        format!("{name} needs to hold {items} to be in its {} forme.", rules.forme),
                        format!("(It will revert to its {base} forme if you remove the item or give it a different item.)"),
                    ]);
                }
            }
            if let Some(required) = missing_move {
                let base = &original_form().base_forme;
                problems.extend([
                    format!(
                        "{name} needs to know the move {required} to be in its {} forme.",
                        rules.forme
                    ),
                    format!("(It will revert to its {base} forme if it forgets the move.)"),
                ]);
            }
            if let Some(forced) = item
                .and_then(|item| item.forced_forme.as_deref())
                .map(|form| self.named_species(form))
                && original.name == forced.base_species
            {
                species = forced;
            }
        }
        if original.name == "Keldeo" && knows("Secret Sword") {
            species = self.named_species("Keldeo-Resolute");
        }
        if original.base_species == "Greninja" && ability.name == "Battle Bond" {
            species = self.named_species("Greninja-Bond");
        }
        if original.base_species == "Rockruff" && ability.name == "Own Tempo" {
            species = self.named_species("Rockruff-Dusk");
        }
        if (original.name == "Hoopa" && knows("Hyperspace Fury"))
            || (original.name == "Hoopa-Unbound" && knows("Hyperspace Hole"))
        {
            let (wrong, right) = if original.name == "Hoopa" {
                ("Hyperspace Fury", "Hyperspace Hole")
            } else {
                ("Hyperspace Hole", "Hyperspace Fury")
            };
            problems.push(format!("In Generation 9, {} cannot run {wrong} because it gets replaced with {right} upon changing forme.", original.name));
        }
        let behemoth = match original.name.as_str() {
            "Zacian-Crowned" => "Behemoth Blade",
            "Zamazenta-Crowned" => "Behemoth Bash",
            _ => return species,
        };
        if let Some(data) = moves.iter_mut().find(|data| data.name == behemoth) {
            *data = DEX.moves(self.format.dex_mod, "ironhead").unwrap();
        }
        species
    }

    fn tier_forme(
        &self,
        species: &'static Species,
        item: Option<&Item>,
        moves: &[&Move],
    ) -> &'static Species {
        if let Some(mega) = item
            .and_then(|item| item.mega_stone.as_ref())
            .and_then(|table| table.get(&species.name))
        {
            return self.named_species(mega);
        }
        match (
            species.name.as_str(),
            item.map(|item| item.effect.name.as_str()),
        ) {
            ("Groudon", Some("Red Orb")) => self.named_species("Groudon-Primal"),
            ("Kyogre", Some("Blue Orb")) => self.named_species("Kyogre-Primal"),
            ("Zacian", Some("Rusted Sword")) => self.named_species("Zacian-Crowned"),
            ("Zamazenta", Some("Rusted Shield")) => self.named_species("Zamazenta-Crowned"),
            ("Rayquaza", _)
                if self.rules.rule_sources.contains_key("+tag:past")
                    && !self.format.has_rule("megarayquazaclause")
                    && moves.iter().any(|data| data.name == "Dragon Ascent") =>
            {
                self.named_species("Rayquaza-Mega")
            }
            _ => species,
        }
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

    fn allows_nonstandard(&self, label: &str) -> bool {
        self.natdex.is_some() && matches!(label, "Past" | "Unobtainable")
    }

    fn species_ban(&self, species: &Species, tier: &Species, display: &str) -> Option<String> {
        let species = if matches!(tier.name.as_str(), "Zacian-Crowned" | "Zamazenta-Crowned") {
            tier
        } else {
            species
        };
        for forme in [species, tier] {
            if let Some(reason) = self.ban_reason(&format!("pokemon:{}", to_id(&forme.name))) {
                return Some(format!("{} is {reason}.", forme.name));
            }
        }
        if let Some(reason) =
            self.ban_reason(&format!("basepokemon:{}", to_id(&species.base_species)))
        {
            return Some(format!("{} is {reason}.", species.name));
        }
        let rules = self.species_rules(tier);
        for tag in self
            .format
            .rules
            .iter()
            .rev()
            .filter_map(|r| r.strip_prefix("-tag:"))
        {
            let label = match tag {
                "ag" if matches!(rules.tier.as_str(), "AG" | "(AG)") => Some("AG"),
                "uber" if matches!(rules.tier.as_str(), "Uber" | "AG" | "(AG)") => Some("Uber"),
                "ou" if matches!(rules.tier.as_str(), "OU" | "(OU)") => Some("OU"),
                "uubl" if rules.tier == "UUBL" => Some("UUBL"),
                "duber" if matches!(rules.doubles_tier.as_deref(), Some("DUber" | "(DUber)")) => {
                    Some("DUber")
                }
                _ => rules
                    .tags
                    .iter()
                    .find(|label| to_id(label) == tag)
                    .map(String::as_str),
            };
            if let Some(label) = label {
                let reason = self.ban_reason(&format!("tag:{tag}")).unwrap();
                return Some(format!("{display} is tagged {label}, which is {reason}."));
            }
        }
        tier.is_nonstandard
            .as_ref()
            .filter(|label| !self.allows_nonstandard(label))
            .map(|label| nonexistent(display, label))
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
        let Some(mut ability) = ability else {
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
            || !set["dynamaxLevel"].is_null()
        {
            return Err(ValidationError(format!(
                "special set options for {}",
                original.name
            )));
        }
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
        let mut hp_type = string(set, "hpType").to_owned();
        if !hp_type.is_empty() {
            let id = to_id(&hp_type);
            if matches!(id.as_str(), "normal" | "fairy" | "stellar") || DEX.type_data(&id).is_none()
            {
                problems.push(format!(
                    "{display}'s Hidden Power type ({hp_type}) is invalid."
                ));
            } else {
                hp_type = capitalize(&id);
            }
        }
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
        let species = self.validate_forme(
            original,
            item,
            &mut ability,
            &mut moves,
            &name,
            &mut problems,
        );
        let tier_species = self.tier_forme(species, item, &moves);
        let species_id = to_id(&species.name);
        let nature = to_id(string(set, "nature"));
        let display_species = if name == tier_species.name {
            name.clone()
        } else {
            format!("{name} ({})", tier_species.name)
        };
        if let Some(problem) = self.species_ban(species, tier_species, &display_species) {
            problems.push(problem);
        }
        if let Some(reason) = self.ban_reason(&format!(
            "item:{}",
            if item_id.is_empty() {
                "noitem"
            } else {
                &item_id
            }
        )) {
            if item_id.is_empty() {
                problems.push(format!("{name} not holding an item is {reason}."));
            } else {
                problems.push(format!("{name}'s item {item_label} is {reason}."));
            }
        } else if let Some(item) = item
            && let Some(label) = &item.effect.is_nonstandard
            && !self.allows_nonstandard(label)
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
        if let Some(reason) = self.ban_reason(&format!("ability:{}", to_id(&ability.name))) {
            problems.push(format!("{name}'s ability {} is {reason}.", ability.name));
        } else if let Some(label) = &ability.is_nonstandard
            && !self.allows_nonstandard(label)
        {
            problems.push(nonexistent(
                &format!("{name}'s ability {}", ability.name),
                label,
            ));
        }
        if !nature.is_empty() && !DEX.has_nature(&nature) {
            problems.push(format!("\"{nature}\" is an invalid nature."));
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
            for data in &moves {
                if data.name.starts_with("Hidden Power ") {
                    if hp_type.is_empty() {
                        hp_type.clone_from(&data.move_type);
                    } else if hp_type != data.move_type {
                        problems.push(format!(
                            "{display}'s Hidden Power type {hp_type} is incompatible with Hidden Power {}",
                            data.move_type
                        ));
                    }
                }
            }
            self.check_stats(set, &name, level, &nature, &mut problems)?;
            let mut seen = HashSet::new();
            for data in &moves {
                if !seen.insert(data.name.as_str()) {
                    problems.push(format!("{display} has multiple copies of {}.", data.name));
                }
            }
            for data in &moves {
                if let Some(reason) = self.ban_reason(&format!("move:{}", move_id(data))) {
                    problems.push(format!("{name}'s move {} is {reason}.", data.name));
                } else if let Some(label) = &data.is_nonstandard
                    && !self.allows_nonstandard(label)
                {
                    problems.push(nonexistent(&format!("{name}'s move {}", data.name), label));
                }
            }
            for data in &moves {
                let id = move_id(data);
                if self
                    .natdex
                    .is_some_and(|natdex| natdex.can_learn(&species_id, &id, level))
                {
                    continue;
                }
                match LEGALITY.move_sources(
                    dex,
                    &species_id,
                    &id,
                    level.clamp(1, i64::from(u16::MAX)) as u16,
                ) {
                    MoveSources::Unrestricted => {}
                    MoveSources::Absent => {
                        problems.push(format!("{display} can't learn {}.", data.name));
                        break;
                    }
                    MoveSources::Conditional | MoveSources::Missing => {
                        if species.name == "Smeargle"
                            && (data.flags.contains_key("nosketch") || data.is_z_move)
                        {
                            problems
                                .push(format!("{display}'s move {} can't be Sketched.", data.name));
                            break;
                        }
                        return Err(ValidationError(format!(
                            "learnset prerequisites for {} using {}",
                            species.name, data.name
                        )));
                    }
                }
            }
        }
        let mut facts = Vec::new();
        if !self.rules.complex_team_bans.is_empty() || !self.rules.complex_bans.is_empty() {
            facts.extend([
                format!("pokemon:{species_id}"),
                format!("pokemon:{}", to_id(&tier_species.name)),
                format!("basepokemon:{}", to_id(&species.base_species)),
                format!("item:{item_id}"),
                format!("ability:{}", to_id(&ability.name)),
            ]);
            facts.extend(moves.iter().map(|m| format!("move:{}", move_id(m))));
            facts.extend(species.types.iter().map(|ty| format!("type:{}", to_id(ty))));
            let rules = self.species_rules(tier_species);
            facts.push(format!("tag:{}", to_id(&rules.tier)));
            facts.extend(
                rules
                    .doubles_tier
                    .iter()
                    .chain(&rules.tags)
                    .map(|tag| format!("tag:{}", to_id(tag))),
            );
        }
        for super::Combination(rule, source, limit, bans) in &self.rules.complex_bans {
            let count = bans.iter().filter(|&ban| facts.contains(ban)).count();
            let clause = rule_suffix(source);
            let limit = limit.unwrap_or(0) as usize;
            if limit > 0 && count > limit {
                problems.push(format!(
                    "{display} is limited to {limit} of {rule}{clause}."
                ));
            } else if limit == 0 && count >= bans.len() {
                let reason = if source == "Obtainable Moves" {
                    "impossible to obtain legitimately".into()
                } else {
                    format!("banned{clause}")
                };
                problems.push(format!(
                    "{display} has the combination of {rule}, which is {reason}."
                ));
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
            let event_display = if name == species.name || name == species.base_species {
                species.name.clone()
            } else {
                format!("{name} ({})", species.name)
            };
            self.check_modern_events(set, species, &event_display, level, events, &mut problems)?;
        }
        if let Some(natdex) = self.natdex {
            if self.species_rules(species).tier == "Illegal"
                && natdex.tier(&species_id) == Some("Illegal")
            {
                problems.push(format!("{name} does not exist in the National Dex."));
            } else if let Some(data) = moves
                .iter()
                .find(|data| natdex.missing_move(&move_id(data)))
            {
                problems.push(format!(
                    "{name}'s move {} does not exist in the National Dex.",
                    data.name
                ));
            } else if item.is_some_and(|item| {
                item.effect.is_nonstandard.is_some() && !natdex.can_transfer_item(&item_id)
            }) {
                problems.push(format!(
                    "{name}'s item {item_label} does not exist in Gen 9."
                ));
            }
        }
        Ok(CheckedSet {
            problems,
            species: Some(species),
            nickname: name,
            item_id,
            item_label,
            facts,
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
            let can_hyper_train = level >= 50
                && string(set, "hpType").is_empty()
                && set["ivs"]
                    .as_object()
                    .is_none_or(|ivs| ivs.values().all(|v| v.as_i64() == Some(31)));
            if event["japan"] == true
                || ((!event["ivs"].is_null() || !event["perfectIVs"].is_null()) && !can_hyper_train)
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
                .filter(|gender| !gender.is_empty())
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

fn capitalize(id: &str) -> String {
    let mut first = id.chars();
    first.next().map_or_else(String::new, |c| {
        c.to_ascii_uppercase().to_string() + first.as_str()
    })
}

fn rule_suffix(source: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        format!(" by {source}")
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
