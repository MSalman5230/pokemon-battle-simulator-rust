//! Input messages for the headless subset of Showdown's battle stream.

mod packed;
pub(crate) use packed::unpack;

use crate::choice::{Choice, ChoiceError};
use crate::rng::{InvalidSeed, Seed};
use serde::Deserialize;
use std::{error::Error, fmt, str::FromStr};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Player {
    P1,
    P2,
}

impl Player {
    pub const ALL: [Self; 2] = [Self::P1, Self::P2];

    pub fn index(self) -> usize {
        match self {
            Self::P1 => 0,
            Self::P2 => 1,
        }
    }

    pub fn foe(self) -> Self {
        match self {
            Self::P1 => Self::P2,
            Self::P2 => Self::P1,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::P1 => "p1",
            Self::P2 => "p2",
        }
    }
}

#[derive(Debug)]
pub enum Input {
    Start(Start),
    SetPlayer(Player, PlayerOptions),
    Choose(Player, Choice),
    Reseed(Seed),
}

#[derive(Debug, Deserialize)]
pub struct Start {
    pub formatid: String,
    #[serde(default, deserialize_with = "seed_input")]
    pub seed: Option<Seed>,
    #[serde(default)]
    pub debug: bool,
    #[serde(default, rename = "forceRandomChance")]
    pub force_random_chance: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PlayerOptions {
    pub name: String,
    pub avatar: Option<serde_json::Value>,
    pub rating: Option<u32>,
    pub team: Option<Team>,
    #[serde(deserialize_with = "seed_input")]
    pub seed: Option<Seed>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Team {
    Sets(Vec<Set>),
    Packed(String),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Set {
    pub name: String,
    pub species: String,
    pub item: String,
    pub ability: String,
    pub moves: Vec<String>,
    pub nature: String,
    pub gender: String,
    pub evs: SetStats,
    pub ivs: Option<SetStats>,
    pub level: Option<i32>,
    pub adjust_level: Option<i32>,
    pub shiny: bool,
    pub happiness: Option<u8>,
    pub pokeball: String,
    pub hp_type: Option<String>,
    pub tera_type: Option<String>,
    pub gigantamax: bool,
    pub dynamax_level: Option<u8>,
}

#[derive(Debug, Copy, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SetStats {
    pub hp: i32,
    pub atk: i32,
    pub def: i32,
    pub spa: i32,
    pub spd: i32,
    pub spe: i32,
}

impl SetStats {
    pub fn raw(self, stat: crate::mechanics::Stat) -> i32 {
        use crate::mechanics::Stat::*;
        match stat {
            Hp => self.hp,
            Atk => self.atk,
            Def => self.def,
            Spa => self.spa,
            Spd => self.spd,
            Spe => self.spe,
        }
    }
}

fn seed_input<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Seed>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum WireSeed {
        String(String),
        Words([u16; 4]),
    }
    Option::<WireSeed>::deserialize(d)?
        .map(|value| match value {
            WireSeed::String(s) => s.parse().map_err(serde::de::Error::custom),
            WireSeed::Words(words) => Ok(Seed::Gen5(words)),
        })
        .transpose()
}

#[derive(Debug)]
pub enum InputError {
    Syntax(&'static str),
    Json(serde_json::Error),
    Choice(ChoiceError),
    Seed(InvalidSeed),
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(msg) => f.write_str(msg),
            Self::Json(err) => write!(f, "invalid battle input JSON: {err}"),
            Self::Choice(err) => write!(f, "invalid choice syntax: {err}"),
            Self::Seed(err) => write!(f, "invalid seed: {err}"),
        }
    }
}

impl Error for InputError {}

impl FromStr for Input {
    type Err = InputError;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        fn split(input: &str) -> (&str, &str) {
            input
                .split_once(' ')
                .map_or((input, ""), |(a, b)| (a, b.trim_start()))
        }
        let (command, data) = split(line.trim_end());
        match command {
            ">start" => serde_json::from_str(data)
                .map(Self::Start)
                .map_err(InputError::Json),
            ">player" => {
                let (side, data) = split(data);
                let player = match side {
                    "p1" => Player::P1,
                    "p2" => Player::P2,
                    _ => return Err(InputError::Syntax("expected p1 or p2")),
                };
                serde_json::from_str(data)
                    .map(|options| Self::SetPlayer(player, options))
                    .map_err(InputError::Json)
            }
            ">p1" | ">p2" => {
                let player = if command == ">p1" {
                    Player::P1
                } else {
                    Player::P2
                };
                data.parse()
                    .map(|choice| Self::Choose(player, choice))
                    .map_err(InputError::Choice)
            }
            ">reseed" => data.parse().map(Self::Reseed).map_err(InputError::Seed),
            _ => Err(InputError::Syntax("unsupported headless battle command")),
        }
    }
}
