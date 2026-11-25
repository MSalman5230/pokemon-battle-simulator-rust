//! Poke-RL compatibility layer. Public observations come only from the
//! viewer's native channel. Critic observations are a separate opt-in result.

mod public;
mod wire;
pub use wire::GameResult;

use crate::{
    battle::{Battle, BattleStream, Phase},
    choice::to_id,
    dex::{DEX, Mod},
    input::{Input, Player, PlayerOptions, Start, Team},
    mechanics::{BoostStat, max_pp},
    rng::Seed,
};
use public::{PublicState, fresh_mon};
use serde_json::{Value, json};
use std::sync::LazyLock;

pub const FORMAT: &str = "gen9championsbssregmc";

const STATS: [BoostStat; 7] = [
    BoostStat::Atk,
    BoostStat::Def,
    BoostStat::Spa,
    BoostStat::Spd,
    BoostStat::Spe,
    BoostStat::Accuracy,
    BoostStat::Evasion,
];

static PREVIEW: LazyLock<Vec<Legal>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for a in 0..6 {
        for b in 0..6 {
            for c in 0..6 {
                if a != b && a != c && b != c {
                    let id = 14 + out.len();
                    let choice = format!("team {},{},{}", a + 1, b + 1, c + 1);
                    out.push(Legal::new(
                        id,
                        choice.clone(),
                        json!({"id":id,"kind":"preview","picks":[a,b,c],"choice":choice}),
                    ));
                }
            }
        }
    }
    out
});

#[derive(Clone)]
struct Legal {
    id: usize,
    input: String,
    public: Value,
}

impl Legal {
    fn new(id: usize, input: String, public: Value) -> Self {
        Self { id, input, public }
    }
}

pub struct Game {
    stream: BattleStream,
    teams: [Vec<Value>; 2],
    originals: [Vec<OriginalSet>; 2],
    public: [PublicState; 2],
    log_pos: usize,
    max_turns: u32,
    training: bool,
    truncated: bool,
}

struct OriginalSet {
    fresh: Value,
    item: String,
    ability: String,
    moves: Vec<Value>,
}

impl OriginalSet {
    fn new(set: &Value) -> Self {
        let species = set["species"].as_str().unwrap();
        Self {
            fresh: fresh_mon(species),
            item: to_id(set["item"].as_str().unwrap_or("")),
            ability: to_id(set["ability"].as_str().unwrap_or("")),
            moves: set["moves"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| {
                    let data = DEX
                        .moves(Mod::Champions, &to_id(m.as_str().unwrap()))
                        .unwrap();
                    let pp = max_pp(data.pp, data.no_pp_boosts);
                    json!({"id":to_id(&data.name),"pp":pp,"maxpp":pp,"disabled":false})
                })
                .collect(),
        }
    }
}

impl Game {
    /// Construct a game from rosters already checked by a Champions team
    /// validator. The current worker only calls this for the pinned test corpus.
    pub fn new_prevalidated(
        mut teams: [Vec<Value>; 2],
        seed: Seed,
        max_turns: u32,
        training: bool,
    ) -> Result<Self, String> {
        let mut stream = BattleStream::default();
        stream
            .apply(Input::Start(Start {
                formatid: FORMAT.into(),
                seed: Some(seed),
                debug: false,
                force_random_chance: None,
            }))
            .map_err(|e| e.to_string())?;
        for (index, (player, team)) in Player::ALL.into_iter().zip(&mut teams).enumerate() {
            if team.len() != 6 {
                return Err("Poke-RL expects six-member preview rosters".into());
            }
            for (slot, set) in team.iter_mut().enumerate() {
                set.as_object_mut()
                    .ok_or("invalid roster member")?
                    .insert("name".into(), json!(format!("unit{slot}")));
            }
            let sets = team
                .iter()
                .cloned()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            stream
                .apply(Input::SetPlayer(
                    player,
                    PlayerOptions {
                        name: format!("player{index}"),
                        team: Some(Team::Sets(sets)),
                        ..PlayerOptions::default()
                    },
                ))
                .map_err(|e| e.to_string())?;
        }
        let originals = std::array::from_fn(|i| teams[i].iter().map(OriginalSet::new).collect());
        let mut game = Self {
            stream,
            originals,
            teams,
            public: Default::default(),
            log_pos: 0,
            max_turns,
            training,
            truncated: false,
        };
        game.sync()?;
        Ok(game)
    }

    fn battle(&self) -> &Battle {
        self.stream.battle().unwrap()
    }

    fn ended(&self) -> bool {
        self.battle().phase() == Phase::Ended
    }

    fn sync(&mut self) -> Result<(), String> {
        let battle = self.stream.battle().unwrap();
        for line in &battle.log()[self.log_pos..] {
            for player in Player::ALL {
                self.public[player.index()].feed(line.view(Some(player)))?;
            }
        }
        self.log_pos = battle.log().len();
        if battle.turn() > self.max_turns && battle.phase() != Phase::Ended {
            self.truncated = true;
        }
        Ok(())
    }

    fn legal(&self, player: Player) -> Vec<Legal> {
        let battle = self.battle();
        if battle.waiting(player) || self.truncated {
            return Vec::new();
        }
        if battle.phase() == Phase::Preview {
            return PREVIEW.to_vec();
        }
        let party = battle.party(player).unwrap();
        let active = &party[0];
        let mut out = Vec::with_capacity(10);
        if battle.phase() == Phase::Move {
            let mega = battle.can_mega_in_slot(player, 0);
            let mut add = |slot: usize, name: &str| {
                for evolve in [false, true] {
                    if evolve && !mega {
                        continue;
                    }
                    let id = slot + if evolve { 4 } else { 0 };
                    let choice = format!("move {}{}", slot + 1, if evolve { " mega" } else { "" });
                    out.push(Legal::new(id, choice.clone(),
                        json!({"id":id,"kind":"move","slot":slot,"move":name,"mega":evolve,"choice":choice})));
                }
            };
            if active.moves.iter().all(|m| m.pp == 0) {
                add(0, "struggle");
            } else {
                for (slot, m) in active.moves.iter().enumerate() {
                    if m.pp != 0 {
                        add(slot, &m.id);
                    }
                }
            }
        }
        for (party_slot, mon) in party.iter().enumerate().skip(1) {
            if mon.hp == 0 {
                continue;
            }
            let slot = mon.roster_index;
            let id = slot + 8;
            let choice = format!("switch {}", party_slot + 1);
            let species = mon.details.split(',').next().unwrap();
            out.push(Legal::new(
                id,
                choice.clone(),
                json!({ "id":id,"kind":"switch","slot":slot,
                "party_slot":party_slot,"revive":false,"species":species,"choice":choice }),
            ));
        }
        out
    }

    pub fn result(&self) -> GameResult<'_> {
        GameResult {
            game: self,
            legal: Player::ALL.map(|p| self.legal(p)),
        }
    }

    pub fn step(&mut self, actions: [Option<usize>; 2]) -> Result<(), String> {
        let mut choices = [None, None];
        for (index, player) in Player::ALL.into_iter().enumerate() {
            if self.battle().phase() == Phase::Preview
                && !self.battle().waiting(player)
                && !self.truncated
            {
                let choice = actions[index]
                    .and_then(|id| id.checked_sub(14))
                    .and_then(|i| PREVIEW.get(i))
                    .ok_or_else(|| format!("illegal preview action for player {index}"))?;
                choices[index] = Some(choice.input.clone());
                continue;
            }
            let legal = self.legal(player);
            if !legal.is_empty() {
                let choice = legal
                    .into_iter()
                    .find(|choice| Some(choice.id) == actions[index])
                    .ok_or_else(|| {
                        format!("illegal action for player {index}: {:?}", actions[index])
                    })?;
                choices[index] = Some(choice.input);
            }
        }
        if choices.iter().all(Option::is_none) && !self.ended() && !self.truncated {
            return Err("no actionable request in nonterminal battle".into());
        }
        for (player, choice) in Player::ALL.into_iter().zip(choices) {
            if let Some(choice) = choice {
                self.stream
                    .apply(Input::Choose(
                        player,
                        choice
                            .parse::<crate::choice::Choice>()
                            .map_err(|e| e.to_string())?,
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        self.sync()
    }

    pub fn trace(&self) -> Vec<&str> {
        self.battle()
            .log()
            .iter()
            .map(|line| line.view(None))
            .collect()
    }
}
