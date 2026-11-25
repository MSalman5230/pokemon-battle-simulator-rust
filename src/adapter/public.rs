//! Poke-RL's public observations are reconstructed from the viewer's log.
//! Do not read the opponent's battle state from this module.

use crate::{
    choice::to_id,
    dex::{DEX, Mod},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;

type Key = (usize, usize);

#[derive(Clone)]
struct Stint {
    previous: Value,
    moves: Vec<String>,
    pp_used: Map<String, Value>,
    first_native_move: Option<Value>,
    revealed_item: bool,
    revealed_ability: bool,
}

impl Stint {
    fn new(previous: &Value) -> Self {
        Self {
            previous: previous.clone(),
            moves: Vec::new(),
            pp_used: Map::new(),
            first_native_move: None,
            revealed_item: false,
            revealed_ability: false,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct PublicState {
    pub turn: u32,
    pub teams: [Vec<Value>; 2],
    active: [Option<usize>; 2],
    pub conditions: [Map<String, Value>; 2],
    pub weather: String,
    pub terrain: String,
    pub weather_since: Option<u32>,
    pub terrain_since: Option<u32>,
    pub pseudo: Map<String, Value>,
    pub mega: [bool; 2],
    pub history: VecDeque<Value>,
    pub faints: [Vec<Value>; 2],
    last_attacker: Option<usize>,
    stints: [Option<Stint>; 2],
}

fn side(ident: &str) -> Option<usize> {
    match ident.as_bytes().get(1) {
        Some(b'1') => Some(0),
        Some(b'2') => Some(1),
        _ => None,
    }
}

fn nickname(ident: &str) -> &str {
    ident.split_once(": ").map_or("", |(_, name)| name)
}

fn species(details: &str) -> &str {
    details.split(',').next().unwrap_or("")
}

fn effect(text: &str) -> String {
    to_id(
        text.strip_prefix("move: ")
            .or_else(|| text.strip_prefix("ability: "))
            .or_else(|| text.strip_prefix("item: "))
            .unwrap_or(text),
    )
}

fn types(name: &str) -> &[String] {
    DEX.species(Mod::Champions, &to_id(name))
        .map_or(&[], |s| &s.types)
}

fn has_illusion(name: &str) -> bool {
    DEX.species(Mod::Champions, &to_id(name))
        .is_some_and(|s| s.abilities.values().any(|a| a == "Illusion"))
}

pub(super) fn fresh_mon(species: &str) -> Value {
    json!({
        "base_species": species, "species": species, "hp": 1.0, "status": "", "boosts": {},
        "moves": [], "item": null, "ability": null, "seen": false, "active": false,
        "fainted": false, "types": types(species), "volatiles": {}, "last_move": null,
        "times_attacked": 0, "entry_turn": 0, "moves_this_stint": [], "pp_used": {},
        "first_native_move": null, "status_progress": {}, "identity_uncertain": false
    })
}

fn hp(mon: &mut Value, condition: &str) {
    if condition.is_empty() {
        return;
    }
    let (fraction, status) = condition.split_once(' ').unwrap_or((condition, ""));
    if fraction == "0" || status == "fnt" {
        mon["hp"] = json!(0.0);
        mon["fainted"] = json!(true);
        return;
    }
    // The locked worker uses Number on both pieces. A color suffix on 20%
    // or 50% leaves the previous public fraction intact.
    if let Some((hp, max)) = fraction.split_once('/')
        && let (Ok(hp), Ok(max)) = (hp.parse::<f64>(), max.parse::<f64>())
        && max != 0.0
    {
        mon["hp"] = json!(hp / max);
        mon["fainted"] = json!(hp == 0.0);
    }
    mon["status"] = json!(status);
}

fn inc(value: &mut Value) {
    *value = json!(value.as_u64().unwrap_or(0) + 1);
}

impl PublicState {
    pub(super) fn from_search(observation: &Value) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct HistoryFields {
            turn: u32,
            own: Vec<Value>,
            opponent: Vec<Value>,
            conditions: [Map<String, Value>; 2],
            weather: String,
            terrain: String,
            weather_since: Option<u32>,
            terrain_since: Option<u32>,
            pseudo: Map<String, Value>,
            mega: [bool; 2],
            history: VecDeque<Value>,
            faints: [Vec<Value>; 2],
        }
        let fields = HistoryFields::deserialize(observation).map_err(|e| e.to_string())?;
        if fields.history.iter().any(|entry| !entry.is_object()) {
            return Err("invalid public history entry".into());
        }
        let mut teams = [fields.own, fields.opponent];
        for team in &mut teams {
            if team.len() != 6 {
                return Err("search expects six public roster slots".into());
            }
            for (i, member) in team.iter_mut().enumerate() {
                let mon = member
                    .as_object_mut()
                    .ok_or("invalid public roster member")?;
                if !["species", "base_species"]
                    .iter()
                    .all(|id| mon.get(*id).is_some_and(Value::is_string))
                    || !["boosts", "volatiles"]
                        .iter()
                        .all(|id| mon.get(*id).is_some_and(Value::is_object))
                {
                    return Err("invalid public Pokémon identity, boosts, or volatiles".into());
                }
                mon.insert("nickname".into(), json!(format!("unit{i}")));
                let moves = mon
                    .get("moves")
                    .and_then(Value::as_array)
                    .ok_or("invalid public moves")?;
                let moves = moves
                    .iter()
                    .map(|m| {
                        m.as_str()
                            .or_else(|| m["id"].as_str())
                            .ok_or("invalid public move ID")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                mon.insert("moves".into(), json!(moves));
            }
        }
        Ok(Self {
            active: std::array::from_fn(|i| teams[i].iter().position(|m| m["active"] == true)),
            teams,
            turn: fields.turn,
            conditions: fields.conditions,
            weather: fields.weather,
            terrain: fields.terrain,
            weather_since: fields.weather_since,
            terrain_since: fields.terrain_since,
            pseudo: fields.pseudo,
            mega: fields.mega,
            history: fields.history,
            faints: fields.faints,
            ..Self::default()
        })
    }

    fn switch_mon(
        &mut self,
        team: usize,
        ident: &str,
        name: &str,
        condition: &str,
        replace: bool,
    ) -> Result<(), String> {
        let new_base = DEX
            .species(Mod::Champions, &to_id(name))
            .map(|d| d.base_species.as_str());
        let index = self.teams[team]
            .iter()
            .position(|mon| mon["base_species"] == name || mon["species"] == name)
            .or_else(|| {
                self.teams[team].iter().position(|mon| {
                    let id = to_id(mon["base_species"].as_str().unwrap());
                    DEX.species(Mod::Champions, &id)
                        .map(|s| s.base_species.as_str())
                        == new_base
                })
            })
            .ok_or_else(|| format!("Public species not found in preview: {name}"))?;
        if let Some(old) = self.active[team] {
            let mon = &mut self.teams[team][old];
            mon["active"] = json!(false);
            if replace {
                if old != index {
                    self.transfer_illusion_stint(team, old, index);
                }
            } else {
                mon["boosts"] = json!({});
                mon["volatiles"] = json!({});
                mon["last_move"] = Value::Null;
                mon["moves_this_stint"] = json!([]);
                if mon["status"] == "tox" {
                    mon["status_progress"]["toxicStage"] = json!(0);
                }
            }
        }
        let uncertain = !replace
            && self.teams[team].iter().enumerate().any(|(i, other)| {
                i != index
                    && other["fainted"] == false
                    && has_illusion(other["base_species"].as_str().unwrap())
            });
        self.active[team] = Some(index);
        let mon = &mut self.teams[team][index];
        if !replace {
            // A roster without another Illusion candidate cannot need rollback.
            self.stints[team] = uncertain.then(|| Stint::new(mon));
        }
        mon["nickname"] = json!(nickname(ident));
        mon["active"] = json!(true);
        mon["seen"] = json!(true);
        mon["species"] = json!(name);
        mon["types"] = json!(types(name));
        mon["entry_turn"] = json!(self.turn);
        mon["identity_uncertain"] = json!(uncertain);
        hp(mon, condition);
        self.last_attacker = None;
        Ok(())
    }

    fn transfer_illusion_stint(&mut self, team: usize, old: usize, new: usize) {
        let old_state = self.teams[team][old].clone();
        let target = &mut self.teams[team][new];
        for field in [
            "hp",
            "status",
            "boosts",
            "volatiles",
            "last_move",
            "times_attacked",
            "status_progress",
        ] {
            target[field] = old_state[field].clone();
        }
        let stint = self.stints[team].as_ref();
        let moves = stint.map_or(&[][..], |s| s.moves.as_slice());
        let known = target["moves"].as_array_mut().unwrap();
        for id in moves {
            if !known.iter().any(|value| value == id) {
                known.push(json!(id));
            }
        }
        target["moves_this_stint"] = json!(moves);
        if target["first_native_move"].is_null() {
            if let Some(first) = stint.and_then(|info| info.first_native_move.as_ref()) {
                target["first_native_move"] = first.clone();
            } else {
                // The locked worker assigns undefined here and omits the field.
                target.as_object_mut().unwrap().remove("first_native_move");
            }
        }
        if let Some(stint) = stint {
            for (id, cost) in &stint.pp_used {
                let used = target["pp_used"][id].as_u64().unwrap_or(0);
                target["pp_used"][id] = json!(used + cost.as_u64().unwrap());
            }
            if stint.revealed_item {
                target["item"] = old_state["item"].clone();
            }
            if stint.revealed_ability {
                target["ability"] = old_state["ability"].clone();
            }
            let decoy = &mut self.teams[team][old];
            // The reference restores with Object.assign. Fields discovered
            // this stint that were absent from the snapshot are not deleted.
            decoy
                .as_object_mut()
                .unwrap()
                .extend(stint.previous.as_object().unwrap().clone());
            decoy["active"] = json!(false);
        }
    }

    fn mark_revealed(&mut self, side: usize, field: &str) {
        if let Some(stint) = &mut self.stints[side] {
            if field == "item" {
                stint.revealed_item = true;
            } else {
                stint.revealed_ability = true;
            }
        }
    }

    fn mon_key(&self, ident: &str) -> Option<Key> {
        let side = side(ident)?;
        let nickname = nickname(ident);
        self.teams[side]
            .iter()
            .position(|mon| mon["nickname"].as_str() == Some(nickname))
            .or_else(|| {
                (ident.as_bytes().get(2) != Some(&b':'))
                    .then_some(self.active[side])
                    .flatten()
            })
            .map(|index| (side, index))
    }

    fn mon(&self, (side, index): Key) -> &Value {
        &self.teams[side][index]
    }

    fn mon_mut(&mut self, (side, index): Key) -> &mut Value {
        &mut self.teams[side][index]
    }

    pub fn feed(&mut self, line: &str) -> Result<(), String> {
        let parts: Vec<_> = line.split('|').collect();
        let get = |i: usize| parts.get(i).copied().unwrap_or("");
        let ident = get(2);
        let team = side(ident);
        let key = self.mon_key(ident);
        let reveal_from = parts.iter().any(|s| s.starts_with("[from]"));
        let turn = self.turn;
        match get(1) {
            "poke" => {
                if let Some(team) = team {
                    self.teams[team].push(fresh_mon(species(get(3))));
                }
            }
            "switch" | "drag" | "replace" => self.switch_mon(
                team.ok_or("invalid switch side")?,
                ident,
                species(get(3)),
                get(4),
                get(1) == "replace",
            )?,
            "turn" => {
                self.turn = get(2).parse().map_err(|_| "wrong turn number")?;
                self.last_attacker = None;
                for mon in self.teams.iter_mut().flatten() {
                    mon["volatiles"].as_object_mut().unwrap().retain(|_, v| {
                        v["single"] != true || v["turn"].as_u64().unwrap() >= u64::from(self.turn)
                    });
                }
            }
            "move" => {
                if let Some(key) = key {
                    let (side, index) = key;
                    let move_id = to_id(get(3));
                    let target = self.mon_key(get(4));
                    let cost = 1 + usize::from(
                        target.is_some_and(|k| k != key && self.mon(k)["ability"] == "pressure"),
                    );
                    let mega = self.mega[side];
                    let mon = self.mon_mut(key);
                    let moves = mon["moves"].as_array_mut().unwrap();
                    if (!reveal_from
                        || parts.contains(&"[from]move: Sleep Talk")
                        || parts.contains(&"[from] move: Sleep Talk"))
                        && !moves.iter().any(|m| m == &move_id)
                    {
                        moves.push(json!(move_id));
                    }
                    if !reveal_from {
                        mon["moves_this_stint"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(move_id));
                        let prior = mon["pp_used"][&move_id].as_u64().unwrap_or(0);
                        mon["pp_used"][&move_id] = json!(prior + cost as u64);
                        if mon["first_native_move"].is_null() {
                            mon["first_native_move"] = json!({"turn":turn, "without_mega":!mega});
                        }
                    }
                    mon["last_move"] = json!(move_id);
                    let species = mon["species"].clone();
                    if !reveal_from && let Some(stint) = &mut self.stints[side] {
                        stint.moves.push(move_id.clone());
                        let used = stint
                            .pp_used
                            .get(&move_id)
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        stint
                            .pp_used
                            .insert(move_id.clone(), json!(used + cost as u64));
                        stint
                            .first_native_move
                            .get_or_insert_with(|| json!({"turn":turn, "without_mega":!mega}));
                    }
                    self.last_attacker = Some(side);
                    let order = self.history.iter().filter(|h| h["turn"] == turn).count();
                    if self.history.len() >= 12 {
                        self.history.pop_front();
                    }
                    self.history.push_back(json!({
                    "turn":turn,"side":side,"move":move_id,"actor":index,"species":species,"order":order
                }));
                }
            }
            "-damage" | "-heal" | "-sethp" => {
                if let Some(key) = key {
                    let was_attacked = self.last_attacker.is_some_and(|attacker| attacker != key.0);
                    let mon = self.mon_mut(key);
                    hp(mon, get(3));
                    if get(1) == "-damage" {
                        if mon["status"] == "tox" && parts.contains(&"[from] psn") {
                            let stage = mon["status_progress"]["toxicStage"].as_u64().unwrap_or(0);
                            mon["status_progress"]["toxicStage"] = json!((stage + 1).min(15));
                        }
                        if was_attacked && !reveal_from {
                            inc(&mut mon["times_attacked"]);
                        }
                    }
                }
                if get(1) == "-sethp"
                    && let Some(key) = self.mon_key(get(4))
                {
                    hp(self.mon_mut(key), get(5));
                }
            }
            "faint" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    mon["hp"] = json!(0);
                    mon["fainted"] = json!(true);
                    let species = mon["base_species"].clone();
                    self.faints[key.0].push(json!({"species":species,"turn":turn}));
                }
            }
            "-status" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    mon["status"] = json!(get(3));
                    mon["status_since"] = json!(turn);
                    mon["status_progress"] = if get(3) == "slp" {
                        json!({"sleepSource": if mon["last_move"] == "rest" { "rest" } else { "sleep" }, "sleepElapsed":0})
                    } else {
                        json!({})
                    };
                }
            }
            "cant" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    match get(3) {
                        "slp" => inc(&mut mon["status_progress"]["sleepElapsed"]),
                        "frz" => inc(&mut mon["status_progress"]["freezeElapsed"]),
                        _ => {}
                    }
                }
            }
            "-curestatus" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    mon["status"] = json!("");
                    mon["status_progress"] = json!({});
                }
            }
            "-boost" | "-unboost" | "-setboost" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    let stage = mon["boosts"][get(3)].as_i64().unwrap_or(0);
                    let amount = get(4).parse::<i64>().unwrap_or(0);
                    mon["boosts"][get(3)] = json!(match get(1) {
                        "-setboost" => amount,
                        "-boost" => (stage + amount).clamp(-6, 6),
                        _ => (stage - amount).clamp(-6, 6),
                    });
                }
            }
            "-clearnegativeboost" => {
                if let Some(key) = key {
                    let boosts = self.mon_mut(key)["boosts"].as_object_mut().unwrap();
                    for stage in boosts.values_mut() {
                        if stage.as_i64().is_some_and(|value| value < 0) {
                            *stage = json!(0);
                        }
                    }
                }
            }
            "-clearboost" => {
                if let Some(key) = key {
                    self.mon_mut(key)["boosts"] = json!({});
                }
            }
            "-clearallboost" => {
                for mon in self.teams.iter_mut().flatten() {
                    mon["boosts"] = json!({});
                }
            }
            "-item" => {
                if let Some(key) = key {
                    self.mon_mut(key)["item"] = json!(to_id(get(3)));
                    self.mark_revealed(key.0, "item");
                }
            }
            "-enditem" => {
                if let Some(key) = key {
                    self.mon_mut(key)["item"] = json!("");
                    self.mark_revealed(key.0, "item");
                }
            }
            "-ability" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    let ability = to_id(get(3));
                    if mon["entry_turn"] == turn
                        && mon["moves_this_stint"].as_array().unwrap().is_empty()
                        && !reveal_from
                        && ability != "illusion"
                        && DEX
                            .species(Mod::Champions, &to_id(mon["species"].as_str().unwrap()))
                            .is_some_and(|s| s.abilities.values().any(|a| to_id(a) == ability))
                    {
                        mon["identity_uncertain"] = json!(false);
                    }
                    mon["ability"] = json!(ability);
                    self.mark_revealed(key.0, "ability");
                }
            }
            "-mega" => {
                if let Some(key) = key {
                    self.mega[key.0] = true;
                    self.mon_mut(key)["item"] = json!(to_id(get(4)));
                }
            }
            "detailschange" | "-formechange" => {
                if let Some(key) = key {
                    let mon = self.mon_mut(key);
                    let name = species(get(3));
                    mon["species"] = json!(name);
                    mon["types"] = json!(types(name));
                }
            }
            "-transform" => {
                if let Some(key) = key
                    && let Some(other) = self.mon_key(get(3))
                {
                    let model = &self.teams[other.0][other.1];
                    let [species, types, boosts] =
                        ["species", "types", "boosts"].map(|k| model[k].clone());
                    let mon = self.mon_mut(key);
                    mon["species"] = species;
                    mon["types"] = types;
                    mon["boosts"] = boosts;
                    mon["volatiles"]["transform"] = json!({ "turn": turn });
                }
            }
            "-start" if get(3) == "typechange" => {
                if let Some(key) = key {
                    self.mon_mut(key)["types"] = json!(get(4).split('/').collect::<Vec<_>>());
                }
            }
            "-start" if get(3) == "typeadd" => {
                if let Some(key) = key {
                    let types = self.mon_mut(key)["types"].as_array_mut().unwrap();
                    if !types.iter().any(|ty| ty == get(4)) {
                        types.push(json!(get(4)));
                    }
                }
            }
            "-start" => {
                if let Some(key) = key {
                    let id = effect(get(3));
                    let volatiles = self.mon_mut(key)["volatiles"].as_object_mut().unwrap();
                    for prefix in ["stockpile", "perish"] {
                        if id.starts_with(prefix) {
                            volatiles.retain(|key, _| !key.starts_with(prefix));
                        }
                    }
                    volatiles.insert(
                        id,
                        json!({"turn":turn,"move":(!get(4).is_empty()).then(|| to_id(get(4)))}),
                    );
                }
            }
            "-end" => {
                if let Some(key) = key {
                    let id = effect(get(3));
                    let volatiles = self.mon_mut(key)["volatiles"].as_object_mut().unwrap();
                    volatiles.remove(&id);
                    if matches!(id.as_str(), "stockpile" | "perishsong") {
                        volatiles.retain(|key, _| {
                            !key.starts_with("stockpile") && !key.starts_with("perish")
                        });
                    }
                }
            }
            "-singleturn" => {
                if let Some(key) = key {
                    self.mon_mut(key)["volatiles"][effect(get(3))] =
                        json!({"turn":turn,"single":true});
                }
            }
            "-weather" => {
                self.weather = if get(2) == "none" {
                    String::new()
                } else {
                    to_id(get(2))
                };
                if !parts.contains(&"[upkeep]") {
                    self.weather_since = Some(turn);
                }
            }
            "-fieldstart" | "-fieldend" => {
                let id = effect(get(2));
                if id.ends_with("terrain") {
                    if get(1) == "-fieldstart" {
                        self.terrain = id;
                        self.terrain_since = Some(turn);
                    } else {
                        self.terrain.clear();
                    }
                } else if get(1) == "-fieldstart" {
                    self.pseudo.insert(id, json!({"turn":turn}));
                } else {
                    self.pseudo.remove(&id);
                }
            }
            "-sidestart" => {
                if let Some(team) = team {
                    let id = effect(get(3));
                    let max = match id.as_str() {
                        "spikes" => 3,
                        "toxicspikes" => 2,
                        _ => 1,
                    };
                    let previous = self.conditions[team]
                        .get(&id)
                        .and_then(|s| s["layers"].as_u64())
                        .unwrap_or(0);
                    self.conditions[team]
                        .insert(id, json!({"turn":turn,"layers":(previous + 1).min(max)}));
                }
            }
            "-sideend" => {
                if let Some(team) = team {
                    self.conditions[team].remove(&effect(get(3)));
                }
            }
            "-swapsideconditions" => self.conditions.swap(0, 1),
            _ => {}
        }

        if let Some((field, label)) = parts.iter().find_map(|part| {
            part.strip_prefix("[from] ability: ")
                .map(|name| ("ability", name))
                .or_else(|| {
                    part.strip_prefix("[from] item: ")
                        .map(|name| ("item", name))
                })
        }) {
            let mon = parts
                .iter()
                .find_map(|p| p.strip_prefix("[of] "))
                .map_or(key, |ident| self.mon_key(ident));
            if let Some(mon) = mon {
                self.mon_mut(mon)[field] = json!(to_id(label));
                if self.active[mon.0] == Some(mon.1) {
                    self.mark_revealed(mon.0, field);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PublicState;

    #[test]
    fn haze_clears_the_observed_boosts_on_both_sides() {
        let mut state = PublicState::default();
        for line in [
            "|poke|p1|Espeon, L50|",
            "|poke|p2|Umbreon, L50|",
            "|switch|p1a: unit0|Espeon, L50|100/100",
            "|switch|p2a: unit0|Umbreon, L50|100/100",
            "|-boost|p1a: unit0|spa|2",
            "|-unboost|p2a: unit0|spd|1",
            "|-clearallboost",
        ] {
            state.feed(line).unwrap();
        }
        for team in state.teams {
            assert_eq!(team[0]["boosts"], serde_json::json!({}));
        }
    }

    #[test]
    fn illusion_reveal_restores_the_decoy_and_transfers_only_this_stint() {
        let mut state = PublicState::default();
        for line in [
            "|poke|p1|Espeon, L50|",
            "|poke|p1|Zoroark-Hisui, L50|",
            "|poke|p1|Vaporeon, L50|",
            "|switch|p1a: decoy|Espeon, L50|100/100",
            "|turn|1",
            "|move|p1a: decoy|Recover|p1a: decoy",
            "|switch|p1a: reserve|Vaporeon, L50|100/100",
            "|turn|2",
            "|switch|p1a: decoy|Espeon, L50|100/100",
            "|move|p1a: decoy|Nasty Plot|p1a: decoy",
            "|-boost|p1a: decoy|spa|2",
            "|-status|p1a: decoy|brn",
            "|-damage|p1a: decoy|92/100 brn",
            "|-enditem|p1a: decoy|Lum Berry|[eat]",
            "|replace|p1a: real|Zoroark-Hisui, L50",
        ] {
            state.feed(line).unwrap();
        }
        let [decoy, actual, _] = &state.teams[0][..] else {
            panic!("three preview Pokémon")
        };
        assert_eq!(decoy["moves"], serde_json::json!(["recover"]));
        assert_eq!(decoy["hp"], 1.0);
        assert!(decoy["item"].is_null());
        assert_eq!(decoy["status_since"], 2);
        assert_eq!(actual["moves"], serde_json::json!(["nastyplot"]));
        assert_eq!(actual["pp_used"]["nastyplot"], 1);
        assert_eq!(actual["first_native_move"]["turn"], 2);
        assert_eq!(actual["boosts"]["spa"], 2);
        assert_eq!(actual["hp"], 0.92);
        assert_eq!(actual["status"], "brn");
        assert_eq!(actual["item"], "");
        assert_eq!(actual["identity_uncertain"], false);
    }
}
