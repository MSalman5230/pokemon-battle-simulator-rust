//! Native Showdown choice syntax, independent of the Poke-RL action IDs.
//!
//! Parsing does not validate a choice against a team, active request, or format.

use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Team(TeamChoice),
    Actions(Vec<SlotChoice>),
    Undo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamChoice {
    /// Unsplit because e.g. "12" is two slots only when the team has <10 members.
    data: String,
}

impl TeamChoice {
    /// Resolve the native preview syntax to zero-based roster indices.
    pub fn resolve(&self, roster_size: usize, pick_size: usize) -> Result<Vec<usize>, ChoiceError> {
        let bracketed = self.data.starts_with('[') && self.data.ends_with(']');
        let data = if bracketed {
            self.data[1..self.data.len() - 1].trim()
        } else {
            &self.data
        };
        let mut picks: Vec<_> = if data.is_empty() {
            (1..=roster_size).map(Some).collect()
        } else if bracketed || roster_size >= 10 || data.contains(',') {
            data.split(',').map(parse_preview_position).collect()
        } else {
            data.bytes()
                .map(|b| b.is_ascii_digit().then(|| usize::from(b - b'0')))
                .collect()
        };
        let count = roster_size.min(pick_size);
        if !bracketed {
            picks.truncate(count);
        }
        for pos in 1..=count {
            if picks.len() >= count {
                break;
            }
            if !picks.contains(&Some(pos)) {
                picks.push(Some(pos));
            }
        }
        if picks.len() != count {
            return Err(ChoiceError("wrong team preview size"));
        }
        let mut result = Vec::with_capacity(count);
        for pos in picks {
            let pos = pos
                .filter(|pos| (1..=roster_size).contains(pos))
                .ok_or(ChoiceError("preview slot outside roster"))?
                - 1;
            if result.contains(&pos) {
                return Err(ChoiceError("duplicate preview slot"));
            }
            result.push(pos);
        }
        Ok(result)
    }
}

// Showdown uses parseInt for preview tokens, retaining a decimal prefix.
fn parse_preview_position(s: &str) -> Option<usize> {
    let s = s.trim_start();
    let s = s.strip_prefix('+').unwrap_or(s);
    if let Some(s) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        let end = s.bytes().take_while(u8::is_ascii_hexdigit).count();
        return usize::from_str_radix(&s[..end], 16).ok();
    }
    s[..s.bytes().take_while(u8::is_ascii_digit).count()]
        .parse()
        .ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotChoice {
    Move {
        selection: MoveChoice,
        /// Positive targets are opposing slots, negative targets allied slots.
        target: Option<i8>,
        special: Option<SpecialChoice>,
    },
    Switch(Option<NonZeroUsize>),
    Pass,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveChoice {
    Slot(NonZeroUsize),
    Name(String),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialChoice {
    Mega,
    MegaX,
    MegaY,
    ZMove,
    Ultra,
    Dynamax,
    Terastallize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ChoiceError(pub &'static str);

impl fmt::Display for ChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ChoiceError {}

impl FromStr for Choice {
    type Err = ChoiceError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input == "undo" {
            return Ok(Self::Undo);
        }
        let (verb, data) = split_first(input);
        if verb == "team" {
            return Ok(Self::Team(TeamChoice {
                data: data.to_owned(),
            }));
        }
        Ok(Self::Actions(
            input.split(',').map(parse_slot).collect::<Result<_, _>>()?,
        ))
    }
}

fn split_first(input: &str) -> (&str, &str) {
    input
        .split_once(' ')
        .map_or((input, ""), |(a, b)| (a, b.trim()))
}

fn parse_slot(input: &str) -> Result<SlotChoice, ChoiceError> {
    let (verb, data) = split_first(input.trim());
    match verb {
        "move" => parse_move(data),
        "testfight" => parse_move("testfight"),
        "switch" if data.is_empty() => Ok(SlotChoice::Switch(None)),
        "switch" => data
            .parse()
            .map(|slot| SlotChoice::Switch(Some(slot)))
            .map_err(|_| ChoiceError("invalid switch slot")),
        "pass" | "skip" if data.is_empty() => Ok(SlotChoice::Pass),
        "auto" | "default" => Ok(SlotChoice::Auto),
        _ => Err(ChoiceError("unknown slot choice")),
    }
}

fn parse_move(mut data: &str) -> Result<SlotChoice, ChoiceError> {
    let mut target = None;
    let mut special = None;
    while let Some((prefix, last)) = data.rsplit_once(' ') {
        let new_special = match last {
            "mega" => SpecialChoice::Mega,
            "megax" => SpecialChoice::MegaX,
            "megay" => SpecialChoice::MegaY,
            "zmove" => SpecialChoice::ZMove,
            "ultra" => SpecialChoice::Ultra,
            "dynamax" | "gigantamax" | "max" => SpecialChoice::Dynamax,
            "terastal" | "terastallize" => SpecialChoice::Terastallize,
            _ => {
                let digits = last.strip_prefix(['+', '-']).unwrap_or(last);
                if !matches!(digits, "1" | "2" | "3") || to_id(data) == "conversion2" {
                    break;
                }
                let new_target = last.parse().map_err(|_| ChoiceError("invalid target"))?;
                if target.replace(new_target).is_some() {
                    return Err(ChoiceError("conflicting move targets"));
                }
                data = prefix.trim_end();
                continue;
            }
        };
        if special.replace(new_special).is_some() {
            return Err(ChoiceError("conflicting move modifiers"));
        }
        data = prefix.trim_end();
    }

    let selection = if data.is_empty() {
        MoveChoice::Auto
    } else if let Ok(slot) = data.parse() {
        MoveChoice::Slot(slot)
    } else {
        MoveChoice::Name(to_id(data))
    };
    Ok(SlotChoice::Move {
        selection,
        target,
        special,
    })
}

/// Showdown IDs consist of lower-case ASCII letters and ASCII digits.
pub fn to_id(name: &str) -> String {
    name.bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|c| char::from(c.to_ascii_lowercase()))
        .collect()
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Normal,
    RandomNormal,
    Scripted,
    AdjacentAlly,
    AdjacentAllyOrSelf,
    AdjacentFoe,
    Any,
    #[serde(rename = "self")]
    User,
    All,
    AllAdjacent,
    AllAdjacentFoes,
    Allies,
    AllySide,
    AllyTeam,
    FoeSide,
}

impl TargetKind {
    pub fn is_choosable(self) -> bool {
        matches!(
            self,
            Self::Normal
                | Self::AdjacentAlly
                | Self::AdjacentAllyOrSelf
                | Self::AdjacentFoe
                | Self::Any
        )
    }
    /// Test the geometry of a chosen location, not whether a Pokémon is there.
    pub fn valid_location(self, slots: u8, user_slot: u8, target: i8) -> bool {
        if target == 0 {
            return true;
        }
        if target.unsigned_abs() > slots || user_slot >= slots {
            return false;
        }
        let target = i16::from(target);
        let user = -i16::from(user_slot) - 1;
        let own = user == target;
        let foe = target > 0;
        let adjacent = if foe {
            (target - i16::from(slots) - 1 - user).abs() <= 1
        } else {
            (target - user).abs() == 1
        };
        match self {
            Self::Normal | Self::RandomNormal | Self::Scripted => adjacent,
            Self::AdjacentAlly => adjacent && !foe,
            Self::AdjacentAllyOrSelf => (adjacent && !foe) || own,
            Self::AdjacentFoe => adjacent && foe,
            Self::Any => !own,
            _ => false,
        }
    }
}
