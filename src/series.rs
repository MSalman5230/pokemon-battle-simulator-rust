//! Local best-of-three orchestration. Games keep the same submitted rosters
//! and independent replay seeds, with no server, clocks, or room state.

use crate::{
    battle::{Battle, BattleError, BattleStream, Phase},
    choice::to_id,
    dex::DEX,
    input::{Input, Player, PlayerOptions, Start},
    rng::Seed,
};

const GAMES: usize = 3;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SeriesState {
    Playing,
    BetweenGames,
    Ended { winner: Option<Player> },
}

pub struct Series {
    formatid: String,
    players: [PlayerOptions; 2],
    seeds: [Seed; GAMES],
    game: BattleStream,
    game_number: usize,
    wins: [u8; 2],
    ties: u8,
    state: SeriesState,
}

impl Series {
    /// Start game one of an explicit Champions best-of-three format.
    pub fn new(
        formatid: &str,
        players: [PlayerOptions; 2],
        seeds: [Seed; GAMES],
    ) -> Result<Self, BattleError> {
        let formatid = to_id(formatid);
        let format = DEX
            .format(&formatid)
            .ok_or_else(|| BattleError::Unknown(format!("format {formatid}")))?;
        if format.best_of != Some(GAMES) {
            return Err(BattleError::Input(
                "series requires a Champions best-of-three format",
            ));
        }
        let game = Self::start(&formatid, &players, seeds[0])?;
        Ok(Self {
            formatid,
            players,
            seeds,
            game,
            game_number: 1,
            wins: [0; 2],
            ties: 0,
            state: SeriesState::Playing,
        })
    }

    fn start(
        formatid: &str,
        players: &[PlayerOptions; 2],
        seed: Seed,
    ) -> Result<BattleStream, BattleError> {
        let mut stream = BattleStream::default();
        stream.apply(Input::Start(Start {
            formatid: formatid.into(),
            seed: Some(seed),
            debug: false,
            force_random_chance: None,
        }))?;
        for (player, options) in Player::ALL.into_iter().zip(players) {
            stream.apply(Input::SetPlayer(player, options.clone()))?;
        }
        Ok(stream)
    }

    pub fn state(&self) -> SeriesState {
        self.state
    }

    pub fn score(&self) -> [u8; 2] {
        self.wins
    }

    pub fn ties(&self) -> u8 {
        self.ties
    }

    pub fn win_threshold(&self) -> u8 {
        (GAMES as u8 - self.ties) / 2 + 1
    }

    pub fn game_number(&self) -> usize {
        self.game_number
    }

    /// The current or just-completed game remains readable until `next_game`.
    pub fn game(&self) -> &Battle {
        self.game.battle().unwrap()
    }

    pub fn apply(&mut self, input: Input) -> Result<(), BattleError> {
        if self.state != SeriesState::Playing {
            return Err(BattleError::Choice("no active series game"));
        }
        if matches!(input, Input::Start(_) | Input::SetPlayer(..)) {
            return Err(BattleError::Input("the series owns its format and players"));
        }
        self.game.apply(input)?;
        if self.game().phase() == Phase::Ended {
            if let Some(winner) = self.game().winner() {
                self.wins[winner.index()] += 1;
            } else {
                self.ties += 1;
            }
            let winner = Player::ALL
                .into_iter()
                .find(|&p| self.wins[p.index()] >= self.win_threshold());
            self.state = if winner.is_some() || self.game_number == GAMES {
                SeriesState::Ended { winner }
            } else {
                SeriesState::BetweenGames
            };
        }
        Ok(())
    }

    pub fn next_game(&mut self) -> Result<(), BattleError> {
        if self.state != SeriesState::BetweenGames {
            return Err(BattleError::Choice("the series is not between games"));
        }
        let game = Self::start(&self.formatid, &self.players, self.seeds[self.game_number])?;
        self.game = game;
        self.game_number += 1;
        self.state = SeriesState::Playing;
        Ok(())
    }
}
