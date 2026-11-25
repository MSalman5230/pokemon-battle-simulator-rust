//! Restore an independent singles battle from an explicitly hypothetical state.
//! No live battle or hidden state is consulted during reconstruction.

use super::{
    charge::ChargeSpec,
    support::{FutureMove, Wish},
    tactical::{BOOSTS, TYPES},
    *,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub(crate) struct WorldInput {
    format: String,
    kind: String,
    pub turn: u32,
    pub sides: [WorldSide; 2],
    field: WorldField,
    #[serde(default)]
    wishes: BTreeMap<String, WorldWish>,
    #[serde(default)]
    future_moves: BTreeMap<String, WorldFutureMove>,
}

#[derive(Deserialize)]
struct WorldWish {
    source: Option<String>,
    amount: Option<f64>,
    turns: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorldFutureMove {
    source: String,
    source_side: String,
    #[serde(rename = "move")]
    move_name: String,
    turns: u32,
}

#[derive(Deserialize)]
pub(crate) struct WorldSide {
    pub pokemon: Vec<WorldMon>,
    observation: WorldObservation,
    fresh: bool,
    last_move: Option<String>,
    mega_used: bool,
    #[serde(default)]
    total_fainted: u8,
    #[serde(default)]
    boosts: BTreeMap<String, i8>,
    #[serde(default)]
    conditions: Map<String, Value>,
    #[serde(default)]
    volatiles: Vec<WorldVolatile>,
}

#[derive(Deserialize)]
pub(crate) struct WorldMon {
    identity: String,
    pub set: Set,
    #[serde(skip)]
    pub roster_slot: usize,
    stats: Stats,
    hp: u32,
    maxhp: NonZeroU32,
    types: Vec<String>,
    status: String,
    ability: String,
    base_ability: String,
    item: String,
    moves: Vec<WorldMove>,
    #[serde(default)]
    progress: Progress,
    #[serde(default)]
    times_attacked: u32,
    #[serde(default)]
    transformed: Option<Value>,
}

#[derive(Deserialize)]
struct WorldMove {
    id: String,
    pp: u8,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Progress {
    sleep_elapsed: u8,
    sleep_source: String,
    toxic_stage: u8,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct WorldObservation {
    charging: Option<Charging>,
    protect_streak: u8,
    protect_turn: Option<u32>,
    substitute_hp: Option<u32>,
}

#[derive(Deserialize)]
struct Charging {
    phase: String,
    #[serde(rename = "move")]
    move_name: String,
}

#[derive(Deserialize)]
struct WorldVolatile {
    id: String,
    #[serde(default)]
    elapsed: u8,
    remaining: Option<u8>,
    #[serde(rename = "move")]
    move_name: Option<String>,
    #[serde(alias = "sourceSide")]
    source_side: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct WorldField {
    weather: String,
    weather_turns: u8,
    terrain: String,
    terrain_turns: u8,
    #[serde(default)]
    trickroom: u8,
    #[serde(default)]
    extra: BTreeMap<String, Value>,
}

fn unsupported(detail: impl fmt::Display) -> BattleError {
    BattleError::Unsupported(format!("hypothetical state {detail}"))
}

pub(crate) fn hydrate(mut input: WorldInput) -> Result<(BattleStream, Vec<Value>), BattleError> {
    if input.format != "gen9championsbssregmc" || input.kind != "turn" || input.turn == 0 {
        return Err(unsupported("requires a simultaneous BSS move decision"));
    }
    if !input.field.extra.is_empty() {
        return Err(unsupported("extra field conditions"));
    }
    let mut battle = Battle::new(Start {
        formatid: std::mem::take(&mut input.format),
        seed: Some(Seed::Gen5([1, 2, 3, 4])),
        debug: false,
        force_random_chance: None,
    })?;
    for (player, side) in Player::ALL.into_iter().zip(&mut input.sides) {
        if !(1..=3).contains(&side.pokemon.len()) || side.pokemon[0].hp == 0 {
            return Err(unsupported(
                "requires a live active and at most three selected Pokémon per side",
            ));
        }
        // Like the reference, initialize the battle without triggering any of
        // the hidden set's entry abilities or held items a second time.
        let sets = side
            .pokemon
            .iter_mut()
            .map(|mon| {
                let mut set = std::mem::take(&mut mon.set);
                set.ability.clear();
                set.item.clear();
                set
            })
            .collect();
        battle.set_player(
            player,
            PlayerOptions {
                name: format!("player{}", player.index()),
                team: Some(Team::Sets(sets)),
                ..PlayerOptions::default()
            },
        )?;
    }
    for player in Player::ALL {
        battle.choose(player, "team".parse().unwrap())?;
    }

    battle.turn = input.turn;
    battle.log.clear();
    let mut assumptions = Vec::new();
    // Apply all Pokémon snapshots before resolving source-linked effects.
    for (player, side) in Player::ALL.into_iter().zip(&input.sides) {
        let native = battle.sides[player.index()].as_mut().unwrap();
        native.total_fainted = side
            .total_fainted
            .max(side.pokemon.iter().filter(|mon| mon.hp == 0).count() as u8);
        native.mega_used = side.mega_used;
        for (j, (pokemon, given)) in native.party.iter_mut().zip(&side.pokemon).enumerate() {
            given.apply(pokemon, &mut assumptions)?;
            pokemon.active_turns = if j != 0 {
                0
            } else if side.fresh {
                1
            } else {
                2
            };
            pokemon.active_move_actions = u32::from(j == 0 && !side.fresh);
        }
        for (id, value) in &side.conditions {
            let value = value
                .as_u64()
                .and_then(|count| u8::try_from(count).ok())
                .ok_or_else(|| unsupported(format!("side condition {id} count")))?;
            match id.as_str() {
                "reflect" => native.screens[Screen::Reflect as usize] = value,
                "lightscreen" => native.screens[Screen::Light as usize] = value,
                "auroraveil" => native.screens[Screen::AuroraVeil as usize] = value,
                "tailwind" => native.tailwind = value,
                _ => {
                    let hazard = match id.as_str() {
                        "spikes" if (1..=3).contains(&value) => Hazard::Spikes,
                        "toxicspikes" if (1..=2).contains(&value) => Hazard::ToxicSpikes,
                        "stealthrock" if value == 1 => Hazard::StealthRock,
                        "stickyweb" if value == 1 => Hazard::StickyWeb,
                        _ => return Err(unsupported(format!("side condition {id}"))),
                    };
                    native.hazards.push((hazard, value));
                }
            }
        }
        // A move or switch may now involve an unsupported effect even though
        // setup itself deliberately skipped all entry callbacks.
        if let Some(err) = native.party[0].entry_error() {
            return Err(err);
        }
    }
    for (player, side) in Player::ALL.into_iter().zip(&input.sides) {
        let fallen = battle.sides[player.index()]
            .as_ref()
            .unwrap()
            .total_fainted
            .min(5);
        let p = battle.mon_mut(Position { player, slot: 0 });
        side.restore_active(player, p, &input.sides, input.turn, &mut assumptions)?;
        if p.ability == "supremeoverlord" {
            p.fallen_allies = fallen;
        }
    }
    battle.weather = match input.field.weather.as_str() {
        "" => None,
        "raindance" => Some((Weather::Rain, input.field.weather_turns)),
        "sunnyday" => Some((Weather::Sun, input.field.weather_turns)),
        "sandstorm" => Some((Weather::Sand, input.field.weather_turns)),
        "snowscape" => Some((Weather::Snow, input.field.weather_turns)),
        other => return Err(unsupported(format!("weather {other}"))),
    };
    battle.terrain = match input.field.terrain.as_str() {
        "" => None,
        "electricterrain" => Some((Terrain::Electric, input.field.terrain_turns)),
        "grassyterrain" => Some((Terrain::Grassy, input.field.terrain_turns)),
        "mistyterrain" => Some((Terrain::Misty, input.field.terrain_turns)),
        "psychicterrain" => Some((Terrain::Psychic, input.field.terrain_turns)),
        other => return Err(unsupported(format!("terrain {other}"))),
    };
    battle.trick_room = input.field.trickroom;
    restore_delayed_effects(&mut battle, &input)?;
    battle.possible_unaware = Player::ALL.into_iter().any(|player| {
        battle
            .party(player)
            .unwrap()
            .iter()
            .any(|p| p.ability == "unaware" || p.base_ability == "unaware")
    });
    battle.update_speed();
    for player in Player::ALL {
        battle.refresh_observed_disables(Position { player, slot: 0 });
    }
    Ok((
        BattleStream {
            battle: Some(battle),
        },
        assumptions,
    ))
}

fn restore_delayed_effects(battle: &mut Battle, input: &WorldInput) -> Result<(), BattleError> {
    let player = |key: &str| match key {
        "me" => Ok(Player::P1),
        "opp" => Ok(Player::P2),
        _ => Err(unsupported(format!("delayed effect side {key}"))),
    };
    let find = |side: Player, identity: &str| {
        input.sides[side.index()]
            .pokemon
            .iter()
            .position(|p| p.identity == identity)
    };
    let now = battle.overflowed_turn();
    for (side, wish) in &input.wishes {
        let side = player(side)?;
        let source = wish.source.as_deref().and_then(|name| find(side, name));
        let native = battle.sides[side.index()].as_mut().unwrap();
        let amount = wish
            .amount
            .or_else(|| source.map(|index| f64::from(native.party[index].max_hp.get()) / 2.0));
        let amount = amount
            .filter(|&a| a >= 0.0)
            .ok_or_else(|| unsupported("Wish source HP unavailable"))?;
        native.wishes[0] = Some(Wish {
            turn: i64::from(now) + i64::from(wish.turns) - 2,
            hp: if amount == 0.0 {
                0
            } else {
                amount.max(1.0) as u32
            },
            wisher: native.party[source.unwrap_or(0)].roster_index,
        });
    }
    for (target, future) in &input.future_moves {
        let target = player(target)?;
        let source_side = player(&future.source_side)?;
        let source = find(source_side, &future.source)
            .ok_or_else(|| unsupported("delayed attack source unavailable"))?;
        let id = to_id(&future.move_name);
        let data = DEX
            .moves(battle.format.dex_mod, &id)
            .filter(|_| matches!(id.as_str(), "doomdesire" | "futuresight"))
            .ok_or_else(|| unsupported(format!("delayed attack {id}")))?;
        let roster_index =
            battle.sides[source_side.index()].as_ref().unwrap().party[source].roster_index;
        battle.sides[target.index()].as_mut().unwrap().future_moves[0] = Some(FutureMove {
            ending_turn: now.saturating_add(future.turns).saturating_sub(1),
            source_side,
            roster_index,
            data,
        });
    }
    Ok(())
}

impl WorldMon {
    fn apply(&self, mon: &mut Pokemon, assumptions: &mut Vec<Value>) -> Result<(), BattleError> {
        if self.hp > self.maxhp.get()
            || self.maxhp.get() > u16::MAX.into()
            || self.transformed.is_some()
        {
            return Err(unsupported("invalid HP or transformed Pokémon"));
        }
        if [
            self.stats.atk,
            self.stats.def,
            self.stats.spa,
            self.stats.spd,
            self.stats.spe,
        ]
        .iter()
        .any(|n| !(1..=u32::from(u16::MAX)).contains(n))
        {
            return Err(unsupported("stored stat outside supported range"));
        }
        mon.roster_index = self.roster_slot;
        mon.stats = self.stats;
        mon.max_hp = self.maxhp;
        mon.hp = self.hp;
        mon.fainted = self.hp == 0;
        mon.ability = to_id(&self.ability);
        mon.base_ability = to_id(&self.base_ability);
        mon.item = to_id(&self.item);
        mon.times_attacked = self.times_attacked;
        if mon.species.types != self.types {
            let [ty] = self.types.as_slice() else {
                return Err(unsupported("dual type override"));
            };
            mon.temporary_type = Some(
                *TYPES
                    .iter()
                    .find(|&&t| t == ty)
                    .ok_or_else(|| unsupported(ty))?,
            );
        }
        mon.status = match self.status.as_str() {
            "" => Status::Healthy,
            "fnt" => Status::Fainted,
            "brn" | "par" | "psn" | "frz" => Status::from_id(&self.status),
            "tox" => Status::Toxic(self.progress.toxic_stage.min(15)),
            "slp" => {
                let t = 3_u8.saturating_sub(self.progress.sleep_elapsed).max(1);
                if self.progress.sleep_source != "rest" {
                    assumptions.push(json!({"fact":"sleep duration","species":self.identity,
                        "elapsed":self.progress.sleep_elapsed,"remaining":t,"assumption":"conditional three-check duration"}));
                }
                Status::Sleep(t)
            }
            _ => return Err(unsupported(format!("status {}", self.status))),
        };
        if mon.moves.len() != self.moves.len() {
            return Err(BattleError::Input("Hydrated move slot identity mismatch"));
        }
        mon.used_moves = 0;
        for (i, (m, input)) in mon.moves.iter_mut().zip(&self.moves).enumerate() {
            if input.id != m.id {
                return Err(BattleError::Input("Hydrated move slot identity mismatch"));
            }
            if input.pp > m.max_pp {
                return Err(BattleError::Input("Hydrated PP outside Champions maximum"));
            }
            m.pp = input.pp;
            if m.pp < m.max_pp {
                mon.used_moves |= 1 << i;
            }
        }
        Ok(())
    }
}

impl WorldSide {
    fn restore_active(
        &self,
        player: Player,
        mon: &mut Pokemon,
        sides: &[WorldSide; 2],
        turn: u32,
        assumptions: &mut Vec<Value>,
    ) -> Result<(), BattleError> {
        for (id, &amount) in &self.boosts {
            let stat = BOOSTS
                .into_iter()
                .find(|s| s.id() == id)
                .ok_or_else(|| unsupported(id))?;
            mon.boosts.add(stat, amount);
        }
        let slot_for = |mon: &Pokemon, name: Option<&str>| {
            name.and_then(|nm| mon.moves.iter().position(|m| m.id == to_id(nm)))
        };
        mon.last_move_slot = slot_for(mon, self.last_move.as_deref());
        for e in &self.volatiles {
            let remaining = |base: u8| {
                e.remaining
                    .unwrap_or_else(|| base.saturating_sub(e.elapsed).max(1))
            };
            let move_slot = |mon: &Pokemon| {
                slot_for(mon, e.move_name.as_deref().or(self.last_move.as_deref()))
                    .ok_or_else(|| unsupported(format!("{} has no observed move slot", e.id)))
            };
            match e.id.as_str() {
                "typechange" | "stall" | "lightscreen" | "reflect" => {}
                "curse" => mon.track_volatile("curse"),
                "taunt" => {
                    mon.taunt = remaining(3);
                    mon.track_volatile("taunt");
                }
                "encore" => {
                    mon.encore = Some((move_slot(mon)?, remaining(3)));
                    mon.track_volatile("encore");
                }
                "disable" => {
                    mon.disabled_slot = Some((move_slot(mon)?, remaining(4)));
                    mon.track_volatile("disable");
                }
                "choicelock" => {
                    mon.choice_lock = Some(move_slot(mon)?);
                    mon.track_volatile("choicelock");
                }
                "confusion" => {
                    mon.confusion = 4_u8.saturating_sub(e.elapsed).max(1);
                    mon.track_volatile("confusion");
                    assumptions.push(json!({"fact":"confusion remaining","assumed":mon.confusion,"elapsed":e.elapsed}));
                }
                "leechseed" => {
                    let source = match e.source_side.as_deref() {
                        Some("me") => Player::P1,
                        Some("opp") => Player::P2,
                        None => player.foe(),
                        _ => return Err(unsupported("Leech Seed source side")),
                    };
                    let slot = e
                        .source
                        .as_ref()
                        .and_then(|name| {
                            sides[source.index()]
                                .pokemon
                                .iter()
                                .position(|p| &p.identity == name)
                        })
                        .unwrap_or(0);
                    mon.seeded_from = Some(Position {
                        player: source,
                        slot,
                    });
                    mon.track_volatile("leechseed");
                }
                "substitute" => {
                    let owner = e
                        .source
                        .as_ref()
                        .and_then(|name| self.pokemon.iter().find(|p| &p.identity == name))
                        .unwrap_or(&self.pokemon[0]);
                    let hp = self
                        .observation
                        .substitute_hp
                        .unwrap_or(owner.maxhp.get() / 4);
                    if hp == 0 {
                        return Err(unsupported("nonpositive Substitute HP"));
                    }
                    if self.observation.substitute_hp.is_none() {
                        assumptions.push(
                            json!({"fact":"substitute HP", "species":self.pokemon[0].identity,
                            "source":owner.identity, "assumed":hp}),
                        );
                    }
                    mon.substitute_hp = hp;
                    mon.track_volatile("substitute");
                }
                "partiallytrapped" => {
                    let source = match e.source_side.as_deref() {
                        Some("me") => Player::P1,
                        Some("opp") => Player::P2,
                        None => player.foe(),
                        _ => return Err(unsupported("partial trap source side")),
                    };
                    let party = &sides[source.index()].pokemon;
                    let trapper = e
                        .source
                        .as_ref()
                        .and_then(|name| party.iter().find(|p| &p.identity == name))
                        .unwrap_or(&party[0]);
                    let move_id = to_id(e.move_name.as_deref().unwrap_or("Infestation"));
                    let move_name = DEX
                        .moves(crate::dex::Mod::Champions, &move_id)
                        .and_then(|data| partial_trap::binding_move(&data.name))
                        .ok_or_else(|| unsupported(format!("partial trap move {move_id}")))?;
                    let turns = remaining(5);
                    if turns == 0 {
                        return Err(unsupported("partial trap duration"));
                    }
                    mon.partial_trap = Some(partial_trap::PartialTrap {
                        source,
                        trapper: trapper.roster_slot,
                        move_name,
                        turns,
                        divisor: if to_id(&trapper.item) == "bindingband" {
                            6
                        } else {
                            8
                        },
                    });
                    mon.track_volatile("partiallytrapped");
                    assumptions.push(json!({"fact":"partial trap duration", "assumed":turns, "elapsed":e.elapsed}));
                }
                "yawn" => {
                    // The reference assumes a same-side source unless the input
                    // explicitly provides the opposing side for this volatile.
                    let source = match e.source_side.as_deref() {
                        None => player,
                        Some("me") => Player::P1,
                        Some("opp") => Player::P2,
                        _ => return Err(unsupported("Yawn source side")),
                    };
                    mon.yawn = Some(status::Yawn {
                        turns: remaining(2),
                        source,
                    });
                    mon.track_volatile("yawn");
                }
                "flashfire" => mon.track_volatile("flashfire"),
                "focusenergy" => mon.track_volatile("focusenergy"),
                "saltcure" => mon.track_volatile("saltcure"),
                "imprison" => {
                    mon.imprisoned = true;
                    mon.track_volatile("imprison");
                }
                "roost" => {
                    mon.roosted = true;
                    mon.track_volatile("roost");
                }
                "glaiverush" => {
                    mon.glaive_drawback = true;
                    mon.track_volatile("glaiverush");
                }
                "destinybond" => {
                    mon.destiny_bond = true;
                    mon.track_volatile("destinybond");
                }
                "mustrecharge" => {
                    mon.recharge = 1;
                    mon.track_volatile("mustrecharge");
                }
                "twoturnmove" if self.observation.charging.is_some() => {}
                _ => return Err(unsupported(format!("volatile {}", e.id))),
            }
        }
        if let Some(charge) = &self.observation.charging
            && charge.phase != "accelerated"
        {
            let slot = slot_for(mon, Some(&charge.move_name))
                .ok_or_else(|| unsupported("charge move slot"))?;
            let spec = ChargeSpec::for_move(&mon.moves[slot].move_data.name)
                .ok_or_else(|| unsupported("charge move"))?;
            mon.begin_charge(spec, slot, 1);
        }
        if self.observation.protect_streak > 0 || self.observation.protect_turn == Some(turn - 1) {
            mon.stall = Some((
                3_u16.pow(self.observation.protect_streak.clamp(1, 6).into()),
                2,
            ));
            mon.track_volatile("stall");
        }
        Ok(())
    }
}
