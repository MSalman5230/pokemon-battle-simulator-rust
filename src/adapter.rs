//! Poke-RL compatibility layer. Public observations come only from the
//! viewer's native channel. Critic observations are a separate opt-in result.

pub mod lookahead;
mod public;
pub mod tactics;
mod wire;
pub use wire::{GameResult, ObservationDelta};

use crate::{
    battle::{Battle, BattleStream, Phase},
    choice::{Choice, MoveChoice, SlotChoice, SpecialChoice, to_id},
    dex::{DEX, Mod},
    input::{Input, Player, PlayerOptions, Start, Team},
    mechanics::{BoostStat, max_pp},
    rng::Seed,
    validation::TeamValidator,
};
use public::{PublicLog, PublicState, fresh_mon};
use serde::Serialize;
use serde_json::{Value, json, value::RawValue};
use std::{
    borrow::Cow,
    num::NonZeroUsize,
    sync::{Arc, LazyLock, OnceLock},
};

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

static PREVIEW: LazyLock<Vec<Legal<'static>>> = LazyLock::new(|| {
    let mut out = Vec::new();
    for a in 0..6 {
        for b in 0..6 {
            for c in 0..6 {
                if a != b && a != c && b != c {
                    let id = 14 + out.len();
                    let choice = format!("team {},{},{}", a + 1, b + 1, c + 1);
                    out.push(Legal::Preview {
                        id,
                        input: choice,
                        picks: [a, b, c],
                    });
                }
            }
        }
    }
    out
});

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Legal<'a> {
    Preview {
        id: usize,
        picks: [usize; 3],
        #[serde(rename = "choice")]
        input: String,
    },
    Move {
        id: usize,
        slot: usize,
        #[serde(rename = "move")]
        name: &'a str,
        mega: bool,
        #[serde(rename = "choice")]
        input: Cow<'static, str>,
    },
    Switch {
        id: usize,
        slot: usize,
        party_slot: usize,
        revive: bool,
        species: &'a str,
        #[serde(rename = "choice")]
        input: Cow<'static, str>,
    },
}

impl Legal<'_> {
    fn id(&self) -> usize {
        match self {
            Self::Preview { id, .. } | Self::Move { id, .. } | Self::Switch { id, .. } => *id,
        }
    }

    fn choice(&self) -> Result<crate::choice::Choice, String> {
        if let Some(token) = self.token() {
            return Ok(token.choice());
        }
        let Self::Preview { input, .. } = self else {
            unreachable!()
        };
        input.parse::<Choice>().map_err(|e| e.to_string())
    }

    fn token(&self) -> Option<ActionToken> {
        match *self {
            Self::Preview { .. } => None,
            Self::Move { slot, mega, .. } => Some(ActionToken::Move { slot, mega }),
            Self::Switch { party_slot, .. } => Some(ActionToken::Switch(party_slot)),
        }
    }
}

#[derive(Clone, Copy)]
enum ActionToken {
    Move { slot: usize, mega: bool },
    Switch(usize),
}

impl ActionToken {
    fn choice(self) -> Choice {
        let action = match self {
            Self::Move { slot, mega } => SlotChoice::Move {
                selection: MoveChoice::Slot(NonZeroUsize::new(slot + 1).unwrap()),
                target: None,
                special: mega.then_some(SpecialChoice::Mega),
            },
            Self::Switch(slot) => SlotChoice::Switch(NonZeroUsize::new(slot + 1)),
        };
        Choice::Actions(vec![action])
    }
}

enum RequestActions {
    Wait,
    Preview,
    Normal(Vec<(usize, ActionToken)>),
}

/// The exact legal actions of a result, for the next request on that game.
/// Discard it after stepping or replacing the game.
pub struct ActionPlan([RequestActions; 2], bool);

impl ActionPlan {
    pub fn accepts(&self, actions: [Option<usize>; 2]) -> bool {
        self.1
            && self.0.iter().zip(actions).all(|(list, action)| match list {
                RequestActions::Wait => true,
                RequestActions::Preview => {
                    action.is_some_and(|id| (14..14 + PREVIEW.len()).contains(&id))
                }
                RequestActions::Normal(rows) => rows.iter().any(|(id, _)| Some(*id) == action),
            })
    }
}

const MOVE_CHOICES: [[&str; 4]; 2] = [
    ["move 1", "move 2", "move 3", "move 4"],
    ["move 1 mega", "move 2 mega", "move 3 mega", "move 4 mega"],
];
const SWITCH_CHOICES: [&str; 6] = [
    "switch 1", "switch 2", "switch 3", "switch 4", "switch 5", "switch 6",
];

#[derive(Clone)]
enum Projections {
    Shared(Box<PublicState>),
    Split(Box<[PublicState; 2]>),
}

impl Default for Projections {
    fn default() -> Self {
        Self::Shared(Box::default())
    }
}

impl Projections {
    fn view(&self, player: Player) -> &PublicState {
        match self {
            Self::Shared(state) => state,
            Self::Split(states) => &states[player.index()],
        }
    }
}

#[derive(Clone)]
pub struct Game {
    stream: BattleStream,
    teams: Arc<[Vec<Value>; 2]>,
    originals: Arc<[Vec<OriginalSet>; 2]>,
    public: Projections,
    log_pos: usize,
    max_turns: u32,
    training: bool,
    truncated: bool,
}

struct OriginalSet {
    fresh: public::PublicMon,
    item: String,
    ability: String,
    moves: Vec<Value>,
    wire_set: OnceLock<Box<RawValue>>,
    wire_unselected: [OnceLock<Box<RawValue>>; 2],
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
            wire_set: OnceLock::new(),
            wire_unselected: Default::default(),
        }
    }
}

impl Game {
    /// Validate the submitted rosters before assigning private slot nicknames.
    pub fn new(
        teams: [Vec<Value>; 2],
        seed: Seed,
        max_turns: u32,
        training: bool,
    ) -> Result<Self, String> {
        let validator = TeamValidator::new(FORMAT).map_err(|e| e.to_string())?;
        for team in &teams {
            if let Some(problems) = validator.validate_team(team).map_err(|e| e.to_string())? {
                return Err(problems.join("; "));
            }
        }
        Self::new_prevalidated(teams, seed, max_turns, training)
    }

    /// Construct a game from rosters already checked by a Champions team
    /// validator, such as the reference-verified benchmark corpus.
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
            originals: Arc::new(originals),
            teams: Arc::new(teams),
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
        let feed_split = |states: &mut [PublicState; 2], log: &[crate::battle::LogLine]| {
            for line in log {
                for player in Player::ALL {
                    states[player.index()]
                        .feed_log(&PublicLog::new(line.view(Some(player))), None)?;
                }
            }
            Ok::<(), String>(())
        };
        match &mut self.public {
            Projections::Shared(state) => {
                for line in &battle.log()[self.log_pos..] {
                    state.feed_log(&PublicLog::new(line.view(None)), line.private_health())?;
                    if state.needs_split {
                        let mut states = Box::<[PublicState; 2]>::default();
                        feed_split(&mut states, battle.log())?;
                        self.public = Projections::Split(states);
                        break;
                    }
                }
            }
            Projections::Split(states) => feed_split(states, &battle.log()[self.log_pos..])?,
        }
        self.log_pos = battle.log().len();
        if battle.turn() > self.max_turns && battle.phase() != Phase::Ended {
            self.truncated = true;
        }
        Ok(())
    }

    fn legal(&self, player: Player) -> Cow<'_, [Legal<'_>]> {
        let battle = self.battle();
        if battle.waiting(player) || self.truncated {
            return Cow::Borrowed(&[]);
        }
        if battle.phase() == Phase::Preview {
            return Cow::Borrowed(&PREVIEW);
        }
        let party = battle.party(player).unwrap();
        let active = &party[0];
        let mut out = Vec::with_capacity(10);
        if battle.phase() == Phase::Move {
            let mega = battle.can_mega_in_slot(player, 0);
            let mut add = |slot: usize, name| {
                for evolve in [false, true] {
                    if evolve && !mega {
                        continue;
                    }
                    let id = slot + if evolve { 4 } else { 0 };
                    let input = MOVE_CHOICES[usize::from(evolve)].get(slot).map_or_else(
                        || {
                            Cow::Owned(format!(
                                "move {}{}",
                                slot + 1,
                                if evolve { " mega" } else { "" }
                            ))
                        },
                        |&choice| Cow::Borrowed(choice),
                    );
                    out.push(Legal::Move {
                        id,
                        input,
                        slot,
                        name,
                        mega: evolve,
                    });
                }
            };
            if let Some(move_id) = battle.forced_move_id(player, 0) {
                add(0, move_id);
            } else {
                let mut found = false;
                for (slot, m) in active.moves.iter().enumerate() {
                    if !battle.move_disabled_in_request(player, 0, slot) {
                        found = true;
                        add(slot, &m.id);
                    }
                }
                if !found {
                    add(0, "struggle");
                }
            }
        }
        if battle.phase() == Phase::Move && battle.trapped_in_request(player, 0) {
            return out.into();
        }
        let revive = battle.is_reviving(player, 0);
        for (party_slot, mon) in party.iter().enumerate().skip(1) {
            if revive != (mon.hp == 0) {
                continue;
            }
            let slot = mon.roster_index;
            let id = slot + 8;
            let input = SWITCH_CHOICES.get(party_slot).map_or_else(
                || Cow::Owned(format!("switch {}", party_slot + 1)),
                |&choice| Cow::Borrowed(choice),
            );
            let species = mon.details.split(',').next().unwrap();
            out.push(Legal::Switch {
                id,
                input,
                slot,
                party_slot,
                revive,
                species,
            });
        }
        out.into()
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
                choices[index] = Some(choice.choice()?);
                continue;
            }
            let legal = self.legal(player);
            if !legal.is_empty() {
                let choice = legal
                    .iter()
                    .find(|choice| Some(choice.id()) == actions[index])
                    .ok_or_else(|| {
                        format!("illegal action for player {index}: {:?}", actions[index])
                    })?;
                choices[index] = Some(choice.choice()?);
            }
        }
        self.advance(choices)
    }

    /// Step using a plan from this game's immediately preceding observation.
    pub fn step_planned(
        &mut self,
        plan: ActionPlan,
        actions: [Option<usize>; 2],
    ) -> Result<(), String> {
        let mut choices = [None, None];
        for (index, (list, action)) in plan.0.into_iter().zip(actions).enumerate() {
            choices[index] = match list {
                RequestActions::Wait => None,
                RequestActions::Preview => Some(
                    action
                        .and_then(|id| id.checked_sub(14))
                        .and_then(|i| PREVIEW.get(i))
                        .ok_or_else(|| format!("illegal preview action for player {index}"))?
                        .choice()?,
                ),
                RequestActions::Normal(rows) => Some(
                    rows.iter()
                        .find(|(id, _)| Some(*id) == action)
                        .ok_or_else(|| format!("illegal action for player {index}: {action:?}"))?
                        .1
                        .choice(),
                ),
            };
        }
        self.advance(choices)
    }

    fn advance(&mut self, choices: [Option<Choice>; 2]) -> Result<(), String> {
        if choices.iter().all(Option::is_none) && !self.ended() && !self.truncated {
            return Err("no actionable request in nonterminal battle".into());
        }
        for (player, choice) in Player::ALL.into_iter().zip(choices) {
            if let Some(choice) = choice {
                let result = self.stream.apply(Input::Choose(player, choice));
                match result {
                    Err(crate::battle::BattleError::Unavailable(_)) | Ok(()) => {}
                    Err(err) => return Err(err.to_string()),
                }
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
