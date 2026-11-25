//! Poke-RL's public observations are reconstructed from the viewer's log.
//! Do not read the opponent's battle state from this module.

use crate::{
    choice::to_id,
    dex::{DEX, Mod},
};
use serde_json::{Map, Value, json};
use std::collections::VecDeque;

type Key = (usize, usize);

#[derive(Default)]
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
            "replace" => return Err("Poke-RL Illusion observation is not ported".into()),
            "switch" | "drag" => {
                let team = team.ok_or("invalid switch side")?;
                let name = species(get(3));
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
                    mon["boosts"] = json!({});
                    mon["volatiles"] = json!({});
                    mon["last_move"] = Value::Null;
                    mon["moves_this_stint"] = json!([]);
                    if mon["status"] == "tox" {
                        mon["status_progress"]["toxicStage"] = json!(0);
                    }
                }
                let uncertain = self.teams[team].iter().enumerate().any(|(i, other)| {
                    i != index
                        && other["fainted"] == false
                        && has_illusion(other["base_species"].as_str().unwrap())
                });
                self.active[team] = Some(index);
                let mon = &mut self.teams[team][index];
                mon["nickname"] = json!(nickname(ident));
                mon["active"] = json!(true);
                mon["seen"] = json!(true);
                mon["species"] = json!(name);
                mon["types"] = json!(types(name));
                mon["entry_turn"] = json!(turn);
                mon["identity_uncertain"] = json!(uncertain);
                hp(mon, get(4));
                self.last_attacker = None;
            }
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
            "-item" => {
                if let Some(key) = key {
                    self.mon_mut(key)["item"] = json!(to_id(get(3)));
                }
            }
            "-enditem" => {
                if let Some(key) = key {
                    self.mon_mut(key)["item"] = json!("");
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
            "-start" if matches!(get(3), "move: Imprison" | "move: Yawn") => {
                if let Some(key) = key {
                    self.mon_mut(key)["volatiles"][effect(get(3))] =
                        json!({"turn":turn,"move":(!get(4).is_empty()).then(|| to_id(get(4)))});
                }
            }
            "-end" if get(3) == "move: Yawn" => {
                if let Some(key) = key {
                    self.mon_mut(key)["volatiles"]
                        .as_object_mut()
                        .unwrap()
                        .remove("yawn");
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
            }
        }
        Ok(())
    }
}
