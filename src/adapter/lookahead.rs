//! One-turn search over caller-supplied public-belief worlds.
//! This interface has no game handle and cannot read stored battles.

use super::{Game, OriginalSet, PublicState, wire};
use crate::{
    battle::hydration::{WorldInput, hydrate},
    input::Player,
    rng::{Rng, Seed},
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
struct Search {
    observation: Value,
    worlds: Vec<Value>,
    candidates: Vec<usize>,
    reply_probabilities: HashMap<String, f64>,
    replies: usize,
}

#[derive(Deserialize)]
struct World {
    input: WorldInput,
    teams: [Vec<Value>; 2],
    seed: WireSeed,
    rollout_seeds: Vec<WireSeed>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireSeed {
    Gen5([u16; 4]),
    Text(String),
}

impl WireSeed {
    fn parse(&self) -> Result<Seed, String> {
        match self {
            Self::Gen5(words) => Ok(Seed::Gen5(*words)),
            Self::Text(seed) => seed.parse::<Seed>().map_err(|err| err.to_string()),
        }
    }
}

#[derive(Default, Serialize)]
pub struct SearchResult {
    rows: Vec<SearchRow>,
    errors: Vec<WorldError>,
}

#[derive(Serialize)]
struct SearchRow {
    action: usize,
    world: usize,
    reply: usize,
    observation: Value,
    critic_observation: Value,
    ended: bool,
    winner: Option<usize>,
    continuations: u8,
    #[serde(serialize_with = "serialize_assumptions")]
    assumptions: Arc<Vec<Value>>,
}

#[derive(Serialize)]
struct WorldError {
    world: usize,
    error: String,
}

fn serialize_assumptions<S: Serializer>(
    values: &Arc<Vec<Value>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    values.as_ref().serialize(serializer)
}

pub fn evaluate(query: &Value) -> Result<SearchResult, String> {
    let search = Search::deserialize(query).map_err(|err| err.to_string())?;
    let opponent = search.observation["opponent"]
        .as_array()
        .ok_or("Missing public opponent roster")?;
    if opponent
        .iter()
        .any(|m| !m["set"].is_null() || !m["stats"].is_null() || m.get("selected").is_some())
    {
        return Err("Search requires a public opponent view".into());
    }
    let observed = PublicState::from_search(&search.observation)?;
    let mut result = SearchResult::default();
    for (i, value) in search.worlds.iter().enumerate() {
        if let Err(error) = search.simulate(i, value, &observed, &mut result.rows) {
            result.errors.push(WorldError { world: i, error });
        }
    }
    Ok(result)
}

impl Search {
    fn simulate(
        &self,
        world: usize,
        value: &Value,
        observed: &PublicState,
        rows: &mut Vec<SearchRow>,
    ) -> Result<(), String> {
        let mut data = World::deserialize(value).map_err(|err| err.to_string())?;
        if data.input.turn != observed.turn {
            return Err("Hydrated and observed turns disagree".into());
        }
        for (team, input) in data.teams.iter().zip(&mut data.input.sides) {
            if team.len() != 6
                || team.iter().any(|set| {
                    !set["species"].is_string()
                        || !set["moves"].as_array().is_some_and(|moves| {
                            moves.iter().all(|m| {
                                m.as_str().is_some_and(|id| {
                                    super::DEX
                                        .moves(super::Mod::Champions, &super::to_id(id))
                                        .is_some()
                                })
                            })
                        })
                })
            {
                return Err("Invalid hypothetical roster".into());
            }
            let mut used = 0;
            for mon in &mut input.pokemon {
                let i = team
                    .iter()
                    .position(|set| set["name"] == mon.set.name)
                    .ok_or("Missing hypothetical roster slot")?;
                if used & (1 << i) != 0 {
                    return Err("Duplicate hypothetical roster slot".into());
                }
                used |= 1 << i;
                mon.roster_slot = i;
            }
        }
        let (stream, assumptions) = hydrate(data.input).map_err(|err| err.to_string())?;
        let assumptions = Arc::new(assumptions);
        let originals =
            std::array::from_fn(|i| data.teams[i].iter().map(OriginalSet::new).collect());
        let base = Game {
            stream,
            teams: Arc::new(data.teams),
            originals: Arc::new(originals),
            public: [observed.clone(), observed.clone()],
            log_pos: 0,
            max_turns: 200,
            training: true,
            truncated: false,
        };
        let replies = base.legal(Player::P2);
        if replies.is_empty() {
            return Err("Opponent has no simultaneous reply".into());
        }
        let labels: Vec<_> = replies
            .iter()
            .map(|r| {
                if r.public["kind"] == "move" {
                    format!("move:{}", r.public["move"].as_str().unwrap())
                } else {
                    format!("switch:{}", r.public["slot"].as_u64().unwrap())
                }
            })
            .collect();
        let weights: Vec<_> = labels
            .iter()
            .map(|label| {
                self.reply_probabilities
                    .get(label)
                    .copied()
                    .unwrap_or_default()
                    .max(0.0001)
                    / labels.iter().filter(|l| *l == label).count() as f64
            })
            .collect();
        let legal = base.legal(Player::P1);
        if self
            .candidates
            .iter()
            .any(|a| !legal.iter().any(|choice| choice.id == *a))
        {
            return Err("Public and hydrated legal choices disagree".into());
        }
        let mut rng = Rng::new(data.seed.parse()?);
        for reply in 0..self.replies {
            let selected = weighted(&weights, &mut rng);
            let seed = data
                .rollout_seeds
                .get(reply)
                .map(WireSeed::parse)
                .transpose()?;
            for &action in &self.candidates {
                let mut branch = base.clone();
                if let Some(seed) = seed {
                    branch.stream.reseed_rollout(seed);
                }
                branch.step([Some(action), Some(replies[selected].id)])?;
                let mut continuations = 0;
                while !branch.ended() && branch.battle().turn() <= observed.turn {
                    continuations += 1;
                    if continuations > 8 {
                        return Err("Could not resolve one full turn".into());
                    }
                    let mut picks = [None, None];
                    for (i, player) in Player::ALL.into_iter().enumerate() {
                        let party = branch.battle().party(player).unwrap();
                        let mut strongest = 0.0;
                        for action in branch.legal(player) {
                            if action.public["kind"] != "switch" {
                                return Err("Unsupported interrupted move request".into());
                            }
                            let slot = action.public["party_slot"].as_u64().unwrap() as usize;
                            let hp =
                                f64::from(party[slot].hp) / f64::from(party[slot].max_hp.get());
                            if hp > strongest {
                                strongest = hp;
                                picks[i] = Some(action.id);
                            }
                        }
                    }
                    branch.step(picks)?;
                }
                let [observation, critic_observation] =
                    wire::search_views(&branch).map_err(|e| e.to_string())?;
                rows.push(SearchRow {
                    action,
                    world,
                    reply,
                    observation,
                    critic_observation,
                    ended: branch.ended(),
                    winner: branch.battle().winner().map(Player::index),
                    continuations,
                    assumptions: Arc::clone(&assumptions),
                });
            }
        }
        Ok(())
    }
}

fn weighted(weights: &[f64], rng: &mut Rng) -> usize {
    let mut value = f64::from(rng.next_word()) / 4_294_967_296.0 * weights.iter().sum::<f64>();
    for (i, weight) in weights.iter().enumerate() {
        value -= weight;
        if value <= 0.0 {
            return i;
        }
    }
    weights.len() - 1
}
