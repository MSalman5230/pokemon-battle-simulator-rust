//! Hypothetical damage uses the same native hit computation as a live battle.
//! It runs in its own two-party battle and cannot access any existing game.

use super::{damage::RegularHit, *};
use crate::{
    dex::{Accuracy, Category},
    mechanics::BoostStat,
};
use serde::Deserialize;
use std::collections::BTreeMap;

type Effects = BTreeMap<String, serde_json::Value>;

#[derive(Deserialize)]
pub struct DamageProbe {
    #[serde(rename = "move")]
    move_name: String,
    actor: Combatant,
    target: Combatant,
    #[serde(default)]
    weather: String,
    #[serde(default)]
    terrain: String,
    #[serde(default)]
    mega: bool,
    #[serde(default)]
    target_mega: bool,
    #[serde(default)]
    conditions: [Effects; 2],
    #[serde(default)]
    pseudo: Effects,
    #[serde(default)]
    faints: [u8; 2],
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Combatant {
    set: Set,
    species: Option<String>,
    item: Option<String>,
    ability: Option<String>,
    status: String,
    hp: Option<f64>,
    max_hp: Option<NonZeroU32>,
    types: Option<Vec<String>>,
    boosts: BTreeMap<String, i8>,
    stats: BTreeMap<String, u16>,
    volatiles: Effects,
    times_attacked: u32,
}

#[derive(Debug, Serialize)]
pub struct DamagePreview {
    pub rows: Vec<(u32, f64)>,
    pub maxhp: u32,
    pub accuracy: f64,
    pub speed: [u32; 2],
    pub priority: i8,
    pub survive_full: bool,
    pub survive_chance: f64,
    pub reliable: bool,
    pub unsupported: Vec<&'static str>,
    pub method: &'static str,
}

const ACTOR: Position = Position {
    player: Player::P1,
    slot: 0,
};
const TARGET: Position = Position {
    player: Player::P2,
    slot: 0,
};
pub(super) const BOOSTS: [BoostStat; 7] = [
    BoostStat::Atk,
    BoostStat::Def,
    BoostStat::Spa,
    BoostStat::Spd,
    BoostStat::Spe,
    BoostStat::Accuracy,
    BoostStat::Evasion,
];
pub(super) const TYPES: &[&str] = &[
    "Normal", "Fire", "Water", "Electric", "Grass", "Ice", "Fighting", "Poison", "Ground",
    "Flying", "Psychic", "Bug", "Rock", "Ghost", "Dragon", "Dark", "Steel", "Fairy",
];

fn unsupported(detail: impl fmt::Display) -> BattleError {
    BattleError::Unsupported(format!("hypothetical damage {detail}"))
}

fn safe_ignored_volatile(id: &str) -> bool {
    matches!(
        id,
        "confusion"
            | "taunt"
            | "encore"
            | "disable"
            | "leechseed"
            | "yawn"
            | "partiallytrapped"
            | "trapped"
            | "perishsong"
            | "aquaring"
            | "ingrain"
            | "protect"
            | "mustrecharge"
            | "choicelock"
    ) || id.starts_with("fallen")
}

impl DamageProbe {
    pub fn calculate(mut self) -> Result<DamagePreview, BattleError> {
        // Use native setup for enter-triggered effects, then overwrite only
        // the public fields supplied by the two hypothetical combatants.
        let mut battle = Battle::new(Start {
            formatid: "gen9championsbssregmc".into(),
            seed: Some(Seed::Gen5([11, 22, 33, 44])),
            debug: false,
            force_random_chance: None,
        })?;
        for (pos, mon) in [(ACTOR, &mut self.actor), (TARGET, &mut self.target)] {
            mon.set.level = Some(50);
            mon.set.name = format!("probe{}", pos.player.index());
            let mut set = std::mem::take(&mut mon.set);
            let original_item = set.item.clone();
            mon.ability.get_or_insert_with(|| set.ability.clone());
            mon.item.get_or_insert_with(|| original_item.clone());
            if to_id(&original_item) == "focusband" {
                // The calculator exposes the probability without simulating
                // whether a particular hit triggers a Focus Band.
                set.item.clear();
            }
            battle.set_player(
                pos.player,
                PlayerOptions {
                    name: set.name.clone(),
                    team: Some(Team::Sets(vec![set])),
                    ..PlayerOptions::default()
                },
            )?;
        }
        for pos in [ACTOR, TARGET] {
            battle.choose(pos.player, "team 1".parse().unwrap())?;
        }
        self.actor.apply(&mut battle, ACTOR)?;
        self.target.apply(&mut battle, TARGET)?;
        battle.weather = match to_id(&self.weather).as_str() {
            "" => None,
            "raindance" => Some((Weather::Rain, 5)),
            "sunnyday" => Some((Weather::Sun, 5)),
            "sandstorm" => Some((Weather::Sand, 5)),
            "snowscape" => Some((Weather::Snow, 5)),
            _ => return Err(unsupported(self.weather)),
        };
        battle.terrain = match to_id(&self.terrain).as_str() {
            "" => None,
            "electricterrain" => Some((Terrain::Electric, 5)),
            "grassyterrain" => Some((Terrain::Grassy, 5)),
            "mistyterrain" => Some((Terrain::Misty, 5)),
            "psychicterrain" => Some((Terrain::Psychic, 5)),
            _ => return Err(unsupported(self.terrain)),
        };
        if self.pseudo.keys().any(|id| id != "trickroom") {
            return Err(unsupported("field conditions"));
        }
        for (i, conditions) in self.conditions.iter().enumerate() {
            let side = battle.sides[i].as_mut().unwrap();
            side.total_fainted = self.faints[i];
            for id in conditions.keys() {
                match id.as_str() {
                    "reflect" => side.screens[Screen::Reflect as usize] = 5,
                    "lightscreen" => side.screens[Screen::Light as usize] = 5,
                    "auroraveil" => side.screens[Screen::AuroraVeil as usize] = 5,
                    "tailwind" => side.tailwind = 4,
                    "safeguard" | "mist" | "spikes" | "stealthrock" | "toxicspikes"
                    | "stickyweb" | "wish" => {}
                    _ => return Err(unsupported(format!("side condition {id}"))),
                }
            }
        }
        if self.mega {
            battle.validate_mega(ACTOR)?;
            battle.mega_evolve(ACTOR);
        }
        if self.target_mega {
            battle.validate_mega(TARGET)?;
            battle.mega_evolve(TARGET);
        }
        battle.preview_damage(&to_id(&self.move_name))
    }
}

impl Combatant {
    fn apply(self, battle: &mut Battle, pos: Position) -> Result<(), BattleError> {
        let rules = battle.stat_rules;
        let dex = battle.format.dex_mod;
        let mon = battle.mon_mut(pos);
        if let Some(name) = self.species.filter(|n| n != &mon.species.name) {
            let species = DEX
                .species(dex, &to_id(&name))
                .ok_or_else(|| unsupported(name))?;
            mon.set_current_species(species, rules);
        }
        if let Some(max) = self.max_hp {
            if max.get() > u16::MAX.into() {
                return Err(unsupported("HP above u16 range"));
            }
            mon.max_hp = max;
        }
        let ratio = self.hp.unwrap_or(1.0);
        if !(0.0..=1.0).contains(&ratio) {
            return Err(unsupported("HP ratio outside 0..=1"));
        }
        mon.hp = ((ratio * f64::from(mon.max_hp.get())).round() as u32).max(1);
        mon.status = match self.status.as_str() {
            "" => Status::Healthy,
            "brn" | "par" | "slp" | "psn" | "tox" | "frz" => Status::from_id(&self.status),
            id => return Err(unsupported(format!("status {id}"))),
        };
        mon.boosts = Boosts::default();
        for (id, amount) in self.boosts {
            let stat = BOOSTS
                .into_iter()
                .find(|s| s.id() == id)
                .ok_or_else(|| unsupported(id))?;
            mon.boosts.add(stat, amount);
        }
        mon.ability = to_id(self.ability.as_deref().unwrap_or_default());
        mon.item = to_id(self.item.as_deref().unwrap_or_default());
        if let Some(error) = mon.entry_error().filter(|_| mon.item != "focusband") {
            return Err(error);
        }
        if !Pokemon::supported_ability(&mon.ability) {
            return Err(unsupported(&mon.ability));
        }
        for (stat, value) in self.stats {
            if value == 0 {
                return Err(unsupported(format!("zero {stat} stat")));
            }
            let base = match stat.as_str() {
                "atk" => &mut mon.stats.atk,
                "def" => &mut mon.stats.def,
                "spa" => &mut mon.stats.spa,
                "spd" => &mut mon.stats.spd,
                "spe" => &mut mon.stats.spe,
                _ => return Err(unsupported(format!("stat {stat}"))),
            };
            *base = value.into();
        }
        if let Some(types) = self.types.filter(|types| *types != mon.species.types) {
            let [single] = types.as_slice() else {
                return Err(unsupported("dual type override"));
            };
            mon.temporary_type = Some(
                *TYPES
                    .iter()
                    .find(|&&t| t == single)
                    .ok_or_else(|| unsupported(single))?,
            );
        }
        mon.times_attacked = self.times_attacked;
        mon.volatile_order.clear();
        for id in self.volatiles.keys() {
            match id.as_str() {
                "flashfire" => mon.track_volatile("flashfire"),
                "charge" => mon.track_volatile("charge"),
                "focusenergy" => mon.track_volatile("focusenergy"),
                "roost" => mon.roosted = true,
                id if safe_ignored_volatile(id) => {}
                _ => return Err(unsupported(format!("volatile {id}"))),
            }
        }
        Ok(())
    }
}

impl Battle {
    fn preview_damage(&mut self, move_id: &str) -> Result<DamagePreview, BattleError> {
        let data = DEX.moves(self.format.dex_mod, move_id);
        let weather = self.effective_weather();
        let speed = [ACTOR, TARGET].map(|p| {
            self.mon(p).current_speed(
                weather,
                self.terrain(),
                self.sides[p.player.index()].as_ref().unwrap().tailwind > 0,
            )
        });
        let mut unsupported_reasons = Vec::new();
        if data.is_some_and(|m| {
            m.handlers
                .iter()
                .any(|h| matches!(h.as_str(), "onPrepareHit" | "onTryMove" | "onTry"))
        }) {
            unsupported_reasons.push("move_preparation_not_simulated");
        }
        if DEX.ability(&self.mon(ACTOR).ability).is_some_and(|a| {
            a.handlers
                .iter()
                .any(|h| matches!(h.as_str(), "onPrepareHit" | "onBeforeMove"))
        }) {
            unsupported_reasons.push("attacker_preparation_ability_not_simulated");
        }
        let mut result = DamagePreview {
            rows: vec![(0, 1.0)],
            maxhp: self.mon(TARGET).max_hp.get(),
            accuracy: 1.0,
            speed,
            priority: data.map_or(0, |m| self.move_priority(ACTOR, m)),
            survive_full: false,
            survive_chance: 0.0,
            reliable: unsupported_reasons.is_empty(),
            unsupported: unsupported_reasons,
            method: "native-conditional-damage-v6",
        };
        let Some(data) = data.filter(|m| m.category != Category::Status) else {
            return Ok(result);
        };
        if data.multihit.is_some() {
            return Err(unsupported(format!("multihit move {move_id}")));
        }
        let fixed = match move_id {
            "seismictoss" | "nightshade" => Some(u32::from(self.mon(ACTOR).level)),
            "superfang" => Some((self.mon(TARGET).hp / 2).max(1)),
            _ => None,
        };
        if fixed.is_none()
            && !data.generic
            && !matches!(
                move_id,
                "weatherball"
                    | "hurricane"
                    | "thunder"
                    | "blizzard"
                    | "freezedry"
                    | "ragefist"
                    | "lastrespects"
                    | "eruption"
                    | "waterspout"
                    | "dragonenergy"
                    | "foulplay"
                    | "bodypress"
                    | "knockoff"
                    | "grassyglide"
                    | "expandingforce"
                    | "mistyexplosion"
                    | "solarblade"
                    | "solarbeam"
                    | "grassknot"
                    | "lowkick"
                    | "heatcrash"
                    | "heavyslam"
                    | "fakeout"
                    | "firstimpression"
                    | "wickedblow"
                    | "scald"
                    | "glaiverush"
                    | "facade"
                    | "flyingpress"
            )
        {
            return Err(unsupported(format!("move {move_id}")));
        }
        if matches!(
            self.mon(ACTOR).ability.as_str(),
            "moldbreaker" | "turboblaze" | "teravolt"
        ) {
            self.ability_breaker = Some(ACTOR);
        }
        self.stance_change(ACTOR, data);
        // The damage-only oracle runs ModifyMove but not the full move's
        // ModifyType phase. Weather Ball can double its power and still use
        // its declared Normal type, and Pixilate does not turn Tackle Fairy.
        let move_type = &data.move_type;
        if (move_type == "Fire" && self.event_ability(TARGET) == "flashfire")
            || (data.flags.contains_key("sound") && self.event_ability(TARGET) == "soundproof")
            || self.absorb_elemental(TARGET, ACTOR, move_type)
            || (move_type == "Ground" && !self.grounded(TARGET))
        {
            return Ok(result);
        }
        let ignores_type = self.move_ignores_immunity(ACTOR, data, move_type);
        let Some(effect) = self.effectiveness_of(data, move_type, TARGET, ignores_type) else {
            return Ok(result);
        };
        result.accuracy = self.preview_accuracy(data, weather);
        let item = &self.mon(TARGET).item;
        let disguise = self.disguise_form(TARGET).is_some();
        result.survive_full = item == "focussash" || self.event_ability(TARGET) == "sturdy";
        result.survive_chance = if item == "focusband" && !disguise {
            0.1
        } else {
            0.0
        };
        if disguise {
            result.rows = vec![(result.maxhp / 8, 1.0 / 16.0); 16];
            return Ok(result);
        }
        if let Some(value) = fixed {
            result.rows = vec![(value, 1.0 / 16.0); 16];
            return Ok(result);
        }
        let attack = self.mon(ACTOR);
        let power = match move_id {
            "weatherball" if weather.is_some() => 100,
            "ragefist" => 50 + 50 * attack.times_attacked.min(6) as u16,
            "lastrespects" => 50 + 50 * u16::from(self.sides[0].as_ref().unwrap().total_fainted),
            "eruption" | "waterspout" | "dragonenergy" => {
                (u32::from(data.base_power) * attack.hp / attack.max_hp).max(1) as u16
            }
            "grassknot" | "lowkick" | "heatcrash" | "heavyslam" => self
                .weight_move_power(&data.name, ACTOR, TARGET)
                .ok_or_else(|| unsupported(move_id))?,
            _ => data.base_power,
        };
        let item = self.mon(TARGET).item.clone();
        let crit = data.will_crit == Some(true)
            && !matches!(self.event_ability(TARGET), "battlearmor" | "shellarmor");
        let screen_handlers = self
            .sides
            .iter()
            .flatten()
            .flat_map(|s| s.screens)
            .filter(|&turns| turns > 0)
            .count() as u32;
        result.rows.clear();
        let sheer_force_removed = self.sheer_force_removes_secondaries(ACTOR, data);
        let analytic_last = self.event_ability(ACTOR) == "analytic";
        for roll in (0..16).rev() {
            let value = self.regular_hit(
                ACTOR,
                TARGET,
                data,
                RegularHit {
                    power,
                    move_type,
                    weather,
                    hit: 1,
                    spread: false,
                    effect,
                    crit,
                    screen_handlers,
                    sheer_force_removed,
                    analytic_last,
                    converted: false,
                    roll: Some(roll),
                    solar_weak: matches!(move_id, "solarbeam" | "solarblade")
                        && weather.is_some_and(|w| w != Weather::Sun),
                    expanded: move_id == "expandingforce"
                        && self.terrain() == Some(Terrain::Psychic)
                        && self.grounded(ACTOR),
                },
            );
            self.mon_mut(TARGET).item.clone_from(&item);
            result.rows.push((value, 1.0 / 16.0));
        }
        Ok(result)
    }

    fn preview_accuracy(&self, move_data: &Move, weather: Option<Weather>) -> f64 {
        let weather_adjusted = match (move_data.name.as_str(), weather) {
            ("Blizzard", Some(Weather::Snow)) | ("Hurricane" | "Thunder", Some(Weather::Rain)) => {
                Accuracy::Bypass(true)
            }
            ("Hurricane" | "Thunder", Some(Weather::Sun)) => Accuracy::Percent(50),
            _ => move_data.accuracy,
        };
        let Accuracy::Percent(base) = weather_adjusted else {
            return 1.0;
        };
        if [ACTOR, TARGET]
            .into_iter()
            .any(|p| self.event_ability(p) == "noguard")
        {
            return 1.0;
        }
        let evasion = if self.move_ignores_evasion(ACTOR, move_data)
            || self.event_ability(ACTOR) == "unaware"
        {
            0
        } else {
            self.mon(TARGET).boosts.get(BoostStat::Evasion)
        };
        let aim = if self.event_ability(TARGET) == "unaware" {
            0
        } else {
            self.mon(ACTOR).boosts.get(BoostStat::Accuracy)
        };
        let stage = (aim - evasion).clamp(-6, 6);
        let modifier = self.accuracy_modifier(
            ACTOR,
            self.weather_evasion(TARGET, weather).0,
            move_data.category,
            self.mon(ACTOR).item == "zoomlens",
        );
        // The isolated Showdown calculation calls ModifyAccuracy after its
        // stage relay and applies chained modifiers only to integer values.
        let mut accuracy =
            f64::from(base) * (f64::from(3 + stage.max(0)) / f64::from(3 - stage.min(0)));
        if modifier != 4096 && accuracy.fract() == 0.0 {
            accuracy = f64::from(self.stat_rules.truncation.modify(accuracy as u32, modifier));
        }
        (accuracy / 100.0).clamp(0.0, 1.0)
    }
}
