//! Hypothetical damage uses the same native hit computation as a live battle.
//! It runs in its own two-party battle and cannot access any existing game.

use super::{damage::RegularHit, field_effects::LegacyCondition, *};
use crate::{
    dex::{Accuracy, Category, HitCount},
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

pub(crate) struct PreparedDamageProbe {
    battle: Battle,
    unrestored: Vec<String>,
    unknown_substitute: bool,
}

impl PreparedDamageProbe {
    pub(crate) fn calculate(&mut self, move_name: &str) -> Result<DamagePreview, BattleError> {
        // The reference restores combatants between queries, but keeps the
        // RNG position of that prepared scenario (e.g. Fickle Beam).
        let mut battle = self.battle.clone();
        let result = battle.preview_damage(
            &to_id(move_name),
            self.unrestored.clone(),
            self.unknown_substitute,
        );
        self.battle.rng = battle.rng;
        result
    }
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
    pub rows: Vec<(i64, f64)>,
    pub maxhp: u32,
    pub accuracy: f64,
    pub speed: [u32; 2],
    pub priority: i8,
    pub survive_full: bool,
    pub survive_chance: f64,
    pub reliable: bool,
    pub unsupported: Vec<String>,
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
    pub fn calculate(self) -> Result<DamagePreview, BattleError> {
        let (battle, move_name) = self.prepare()?;
        let PreparedDamageProbe {
            mut battle,
            unrestored,
            unknown_substitute,
        } = battle;
        battle.preview_damage(&to_id(&move_name), unrestored, unknown_substitute)
    }

    pub(crate) fn prepare(mut self) -> Result<(PreparedDamageProbe, String), BattleError> {
        let mut unrestored = Vec::new();
        let unknown_substitute = self
            .target
            .volatiles
            .get("substitute")
            .is_some_and(|state| state.get("hp").is_none());
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
        self.actor.apply(&mut battle, ACTOR, &mut unrestored)?;
        self.target.apply(&mut battle, TARGET, &mut unrestored)?;
        battle.weather = match to_id(&self.weather).as_str() {
            "" => None,
            "raindance" => Some((Weather::Rain, 5)),
            "sunnyday" => Some((Weather::Sun, 5)),
            "sandstorm" => Some((Weather::Sand, 5)),
            "snowscape" => Some((Weather::Snow, 5)),
            "hail" => Some((Weather::Hail, 5)),
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
        if self.pseudo.keys().any(|id| {
            !matches!(
                id.as_str(),
                "trickroom" | "gravity" | "wonderroom" | "magicroom"
            )
        }) {
            return Err(unsupported("field conditions"));
        }
        battle.gravity = u8::from(self.pseudo.contains_key("gravity"));
        battle.wonder_room = u8::from(self.pseudo.contains_key("wonderroom"));
        battle.set_items_suppressed(self.pseudo.contains_key("magicroom"));
        if battle.mon(ACTOR).effective_item() == "metronome" {
            return Err(unsupported("Metronome's consecutive move history"));
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
        Ok((
            PreparedDamageProbe {
                battle,
                unrestored,
                unknown_substitute,
            },
            self.move_name,
        ))
    }
}

impl Combatant {
    fn apply(
        self,
        battle: &mut Battle,
        pos: Position,
        unrestored: &mut Vec<String>,
    ) -> Result<(), BattleError> {
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
        if let Some(error) = mon.entry_error() {
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
            mon.temporary_types = super::types::TypeOverride::from_types(&types)
                .ok_or_else(|| unsupported(format!("type override {types:?}")))?;
        }
        mon.times_attacked = self.times_attacked;
        mon.volatile_order.clear();
        for (id, state) in &self.volatiles {
            match id.as_str() {
                "substitute" => {
                    mon.substitute_hp = state["hp"]
                        .as_u64()
                        .and_then(|hp| u32::try_from(hp).ok())
                        .unwrap_or(mon.max_hp.get() / 4)
                }
                "flashfire" => mon.track_volatile("flashfire"),
                "charge" => mon.track_volatile("charge"),
                "focusenergy" => mon.track_volatile("focusenergy"),
                "roost" => mon.roosted = true,
                "unburden" => mon.track_volatile("unburden"),
                "magnetrise" => mon.magnet_rise = 1,
                "telekinesis" => {}
                "smackdown" => mon.track_volatile("smackdown"),
                "gastroacid" => mon.gastro_acid = true,
                "minimize" => mon.track_volatile("minimize"),
                "tarshot" => mon.track_volatile("tarshot"),
                id if safe_ignored_volatile(id) => {}
                _ => unrestored.push(format!("unrestored_volatile:{id}")),
            }
        }
        if self.volatiles.contains_key("telekinesis") {
            battle.start_legacy_condition(pos, LegacyCondition::Telekinesis);
        }
        Ok(())
    }
}

impl Battle {
    fn preview_damage(
        &mut self,
        move_id: &str,
        unrestored: Vec<String>,
        unknown_substitute: bool,
    ) -> Result<DamagePreview, BattleError> {
        let data = DEX.moves(self.format.dex_mod, move_id);
        let weather = self.effective_weather();
        let speed = [ACTOR, TARGET].map(|p| {
            self.mon(p).current_speed(
                weather,
                self.terrain(),
                self.sides[p.player.index()].as_ref().unwrap().tailwind > 0,
                self.pledge_active(p.player, super::pledges::Pledge::Grass),
            )
        });
        let mut unsupported_reasons: Vec<String> = Vec::new();
        if data.is_some_and(|data| data.ohko)
            || matches!(
                move_id,
                "counter"
                    | "mirrorcoat"
                    | "metalburst"
                    | "comeuppance"
                    | "beatup"
                    | "futuresight"
                    | "doomdesire"
                    | "assurance"
                    | "revenge"
                    | "avalanche"
                    | "payback"
                    | "lashout"
                    | "retaliate"
                    | "lastresort"
                    | "endeavor"
                    | "finalgambit"
                    | "bide"
                    | "round"
                    | "fusionbolt"
                    | "fusionflare"
            )
        {
            unsupported_reasons.push("conditional_move_execution_or_missing_party_history".into());
        }
        if data.is_some_and(|m| {
            m.handlers
                .iter()
                .any(|h| matches!(h.as_str(), "onPrepareHit" | "onTryMove" | "onTry"))
        }) {
            unsupported_reasons.push("move_preparation_not_simulated".into());
        }
        if DEX.ability(&self.mon(ACTOR).ability).is_some_and(|a| {
            a.handlers
                .iter()
                .any(|h| matches!(h.as_str(), "onPrepareHit" | "onBeforeMove"))
        }) {
            unsupported_reasons.push("attacker_preparation_ability_not_simulated".into());
        }
        if [ACTOR, TARGET]
            .into_iter()
            .any(|p| self.event_ability(p) == "rivalry")
        {
            unsupported_reasons.push("unobserved_gender".into());
        }
        unsupported_reasons.extend(unrestored);
        let multihit = data.is_some_and(|data| data.multihit.is_some());
        if multihit {
            unsupported_reasons.push("correlated_multihit_rolls_and_between_hit_effects".into());
            if unknown_substitute {
                unsupported_reasons.push("unknown_substitute_hp".into());
            }
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
        let fixed = match move_id {
            "seismictoss" | "nightshade" => Some(i64::from(self.mon(ACTOR).level)),
            "superfang" | "naturesmadness" | "ruination" => {
                Some(i64::from((self.mon(TARGET).hp / 2).max(1)))
            }
            "endeavor" => Some(i64::from(self.mon(TARGET).hp) - i64::from(self.mon(ACTOR).hp)),
            "finalgambit" => Some(i64::from(self.mon(ACTOR).hp)),
            "counter" | "mirrorcoat" | "metalburst" | "comeuppance" | "bide" => Some(0),
            _ if data.ohko => Some(i64::from(self.mon(TARGET).max_hp.get())),
            _ => None,
        };
        if fixed.is_none()
            && !data.generic
            && !multihit
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
                    | "throatchop"
                    | "flail"
                    | "reversal"
                    | "acrobatics"
                    | "hardpress"
                    | "risingvoltage"
                    | "thief"
                    | "covet"
                    | "psychicnoise"
                    | "gyroball"
                    | "electroball"
                    | "terrainpulse"
                    | "shellsidearm"
                    | "hiddenpower"
                    | "return"
                    | "frustration"
                    | "gravapple"
                    | "falseswipe"
                    | "holdback"
                    | "alluringvoice"
                    | "avalanche"
                    | "barbbarrage"
                    | "beakblast"
                    | "belch"
                    | "brickbreak"
                    | "burnup"
                    | "circlethrow"
                    | "dig"
                    | "direclaw"
                    | "dive"
                    | "doomdesire"
                    | "doubleshock"
                    | "dragontail"
                    | "electroshot"
                    | "explosion"
                    | "ficklebeam"
                    | "flipturn"
                    | "fly"
                    | "focuspunch"
                    | "futuresight"
                    | "hex"
                    | "highjumpkick"
                    | "icespinner"
                    | "infestation"
                    | "jawlock"
                    | "lastresort"
                    | "meteorbeam"
                    | "mortalspin"
                    | "pollenpuff"
                    | "poltergeist"
                    | "powertrip"
                    | "psychicfangs"
                    | "ragingbull"
                    | "rapidspin"
                    | "saltcure"
                    | "selfdestruct"
                    | "snaptrap"
                    | "sparklingaria"
                    | "spiritshackle"
                    | "steelbeam"
                    | "stoneaxe"
                    | "storedpower"
                    | "suckerpunch"
                    | "supercellslam"
                    | "uproar"
                    | "uturn"
                    | "voltswitch"
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
        let data = if move_id == "shellsidearm" {
            self.shell_side_arm(ACTOR, TARGET, data)
        } else {
            data
        };
        // The damage-only oracle runs ModifyMove but not the full move's
        // ModifyType phase. Weather Ball can double its power and still use
        // its declared Normal type, and Pixilate does not turn Tackle Fairy.
        let move_type = &data.move_type;
        if (move_type == "Fire" && self.event_ability(TARGET) == "flashfire")
            || (data.flags.contains_key("sound") && self.event_ability(TARGET) == "soundproof")
            || self.absorb_elemental(TARGET, ACTOR, move_type)
            || (move_type == "Ground" && !self.grounded(TARGET))
            || (data.ohko && self.event_ability(TARGET) == "sturdy")
        {
            return Ok(result);
        }
        let ignores_type = self.move_ignores_immunity(ACTOR, data, move_type);
        let Some(effect) = self.effectiveness_of(data, move_type, TARGET, ignores_type) else {
            return Ok(result);
        };
        let accuracy_weather = if move_id == "blizzard" {
            weather
        } else if self.event_ability(ACTOR) == "megasol" {
            Some(Weather::Sun)
        } else {
            self.mon(TARGET).personal_weather(weather)
        };
        result.accuracy = self.preview_accuracy(data, accuracy_weather);
        let item = self.mon(TARGET).effective_item();
        let sash = item == "focussash" || self.event_ability(TARGET) == "sturdy";
        result.survive_full = !multihit && sash;
        result.survive_chance =
            if !multihit && item == "focusband" && self.disguise_form(TARGET).is_none() {
                0.1
            } else {
                0.0
            };
        let attack = self.mon(ACTOR);
        let power = match move_id {
            "weatherball"
                if attack.personal_weather(weather).is_some()
                    || self.event_ability(ACTOR) == "megasol" =>
            {
                100
            }
            "ragefist" => 50 + 50 * attack.times_attacked.min(6) as u16,
            "storedpower" | "powertrip" => data.base_power + attack.boosts.positive() * 20,
            "hex"
                if self.mon(TARGET).status != Status::Healthy
                    || self.event_ability(TARGET) == "comatose" =>
            {
                data.base_power * 2
            }
            "barbbarrage"
                if matches!(self.mon(TARGET).status, Status::Poison | Status::Toxic(_)) =>
            {
                data.base_power * 2
            }
            "lastrespects" => 50 + 50 * u16::from(self.sides[0].as_ref().unwrap().total_fainted),
            "flail" | "reversal" => attack.reversal_power(),
            "return" => attack.friendship_power(false),
            "frustration" => attack.friendship_power(true),
            "eruption" | "waterspout" | "dragonenergy" => {
                (u32::from(data.base_power) * attack.hp / attack.max_hp).max(1) as u16
            }
            "grassknot" | "lowkick" | "heatcrash" | "heavyslam" => self
                .weight_move_power(&data.name, ACTOR, TARGET)
                .ok_or_else(|| unsupported(move_id))?,
            "acrobatics" | "hardpress" | "risingvoltage" | "gyroball" | "electroball" => {
                self.situational_base_power(ACTOR, TARGET, data, data.base_power)
            }
            "terrainpulse" if self.terrain().is_some() && self.grounded(ACTOR) => {
                data.base_power * 2
            }
            _ => data.base_power,
        };
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
        let contact = self.move_makes_contact(ACTOR, data);
        let solar_weak = self.event_ability(ACTOR) != "megasol"
            && matches!(move_id, "solarbeam" | "solarblade")
            && weather.is_some_and(|w| w != Weather::Sun);
        let expanded = move_id == "expandingforce"
            && self.terrain() == Some(Terrain::Psychic)
            && self.grounded(ACTOR);
        let bypasses_substitute =
            data.flags.contains_key("bypasssub") || self.event_ability(ACTOR) == "infiltrator";
        let fixed_count;
        let hits: &[(u8, f64)] = match data.multihit {
            None => &[(1, 1.0)],
            Some(HitCount::Fixed(n)) => {
                fixed_count = [(n, 1.0)];
                &fixed_count
            }
            Some(HitCount::Range([_, n])) if self.event_ability(ACTOR) == "skilllink" => {
                fixed_count = [(n, 1.0)];
                &fixed_count
            }
            Some(HitCount::Range(_)) if self.mon(ACTOR).effective_item() == "loadeddice" => {
                &[(4, 0.5), (5, 0.5)]
            }
            Some(HitCount::Range(_)) => &[(2, 0.35), (3, 0.35), (4, 0.15), (5, 0.15)],
        };
        let variable = multihit
            || self.mon(TARGET).substitute_hp > 0
            || self.disguise_form(TARGET).is_some()
            || self.ice_face_shields(TARGET, data.category)
            || [ACTOR, TARGET].into_iter().any(|pos| {
                DEX.item(self.format.dex_mod, &self.mon(pos).item)
                    .is_some_and(|item| item.is_berry)
            });
        let saved = variable.then(|| [self.mon(ACTOR).clone(), self.mon(TARGET).clone()]);
        let initial_hp = self.mon(TARGET).hp;
        let log_start = self.log.len();
        for &(hits, weight) in hits {
            for roll in (0..16).rev() {
                if let Some([actor, target]) = &saved {
                    self.mon_mut(ACTOR).clone_from(actor);
                    self.mon_mut(TARGET).clone_from(target);
                    self.log.truncate(log_start);
                } else {
                    self.mon_mut(TARGET).hp = initial_hp;
                }
                let mut damage = 0_i64;
                for hit in 1..=hits {
                    let power = match move_id {
                        "tripleaxel" | "triplekick" => power * u16::from(hit),
                        "ficklebeam" if self.rng.chance(3, 10) => power * 2,
                        _ => power,
                    };
                    let mut value = fixed.unwrap_or_else(|| {
                        i64::from(self.regular_hit(
                            ACTOR,
                            TARGET,
                            data,
                            RegularHit {
                                power,
                                move_type,
                                weather,
                                hit,
                                spread: false,
                                effect,
                                crit,
                                screen_handlers,
                                sheer_force_removed,
                                analytic_last,
                                bypassed_protect: false,
                                parental_second_hit: false,
                                contact,
                                converted: false,
                                force_stab: false,
                                roll: Some(roll),
                                ruin_cache: &mut super::ruin::RuinCache::default(),
                                solar_weak,
                                expanded,
                            },
                        ))
                    });
                    let remaining = i64::from(self.mon(TARGET).hp);
                    if matches!(move_id, "falseswipe" | "holdback") {
                        value = value.min(remaining - 1);
                    }
                    if !bypasses_substitute && self.mon(TARGET).substitute_hp > 0 {
                        let sub = &mut self.mon_mut(TARGET).substitute_hp;
                        *sub = (i64::from(*sub) - value).clamp(0, i64::from(u32::MAX)) as u32;
                        value = 0;
                    } else if value != 0
                        && let Some(form) = self.disguise_form(TARGET)
                    {
                        value = i64::from(result.maxhp / 8);
                        let species = DEX.species(self.format.dex_mod, form).unwrap();
                        let rules = self.stat_rules;
                        self.mon_mut(TARGET).set_current_species(species, rules);
                    } else if value != 0 && self.ice_face_shields(TARGET, data.category) {
                        value = 0;
                        let species = DEX.species(self.format.dex_mod, "eiscuenoice").unwrap();
                        let rules = self.stat_rules;
                        self.mon_mut(TARGET).set_current_species(species, rules);
                    } else if hits > 1 && sash && remaining == i64::from(result.maxhp) {
                        value = value.min(remaining - 1);
                    }
                    damage += value;
                    let hp = (i64::from(initial_hp) - damage).clamp(0, i64::from(u32::MAX)) as u32;
                    self.mon_mut(TARGET).hp = hp;
                    if hp == 0 {
                        break;
                    }
                }
                result.rows.push((damage, weight / 16.0));
            }
        }
        Ok(result)
    }

    fn preview_accuracy(&self, move_data: &Move, weather: Option<Weather>) -> f64 {
        let weather_adjusted = match (move_data.name.as_str(), weather) {
            ("Blizzard", Some(Weather::Snow | Weather::Hail))
            | ("Hurricane" | "Thunder", Some(Weather::Rain)) => Accuracy::Bypass(true),
            ("Hurricane" | "Thunder", Some(Weather::Sun)) => Accuracy::Percent(50),
            _ => move_data.accuracy,
        };
        let Accuracy::Percent(mut base) = weather_adjusted else {
            return 1.0;
        };
        if move_data.category == crate::dex::Category::Status
            && self.event_ability(TARGET) == "wonderskin"
        {
            base = 50;
        }
        if self.mon(TARGET).trampled_by(move_data)
            || !move_data.ohko
                && self.gravity == 0
                && self.mon(TARGET).under(LegacyCondition::Telekinesis)
            || [ACTOR, TARGET]
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
            TARGET,
            self.weather_evasion(TARGET, weather).0,
            move_data.category,
            self.mon(ACTOR).effective_item() == "zoomlens",
            None,
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
