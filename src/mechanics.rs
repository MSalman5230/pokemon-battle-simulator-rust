//! Champions stat, PP, health, and numeric damage calculations.
//! From Pokémon Showdown's sim/battle{,-actions}.ts, sim/pokemon.ts, and
//! data/mods/champions/scripts.ts (MIT, see docs/LICENSE.pokemon-showdown).

use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stat {
    Hp,
    Atk,
    Def,
    Spa,
    Spd,
    Spe,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub hp: u32,
    pub atk: u32,
    pub def: u32,
    pub spa: u32,
    pub spd: u32,
    pub spe: u32,
}

impl Stats {
    pub fn get(self, stat: Stat) -> u32 {
        match stat {
            Stat::Hp => self.hp,
            Stat::Atk => self.atk,
            Stat::Def => self.def,
            Stat::Spa => self.spa,
            Stat::Spd => self.spd,
            Stat::Spe => self.spe,
        }
    }

    pub fn map(f: impl Fn(Stat) -> u32) -> Self {
        Self {
            hp: f(Stat::Hp),
            atk: f(Stat::Atk),
            def: f(Stat::Def),
            spa: f(Stat::Spa),
            spd: f(Stat::Spd),
            spe: f(Stat::Spe),
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq, Deserialize)]
pub struct Nature {
    pub plus: Option<Stat>,
    pub minus: Option<Stat>,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct StatRules {
    pub level_clause: bool,
    pub overflow_stat: bool,
    pub truncation: Truncation,
}

impl StatRules {
    pub fn stat(self, base: u32, points: u8, level: u16, nature: Nature, stat: Stat) -> u32 {
        let points = u32::from(points);
        let level = u32::from(level);
        let mut value = if self.level_clause {
            let scaled = (2 * base + 31 + (2 * points).saturating_sub(1)) * level / 100;
            if stat == Stat::Hp {
                return scaled + level + 10;
            }
            scaled + 5
        } else if stat == Stat::Hp {
            return base + points + 75;
        } else {
            base + points + 20
        };
        let multiplier = if nature.plus == Some(stat) {
            if self.overflow_stat {
                value = value.min(595);
            }
            110
        } else if nature.minus == Some(stat) {
            if self.overflow_stat {
                value = value.min(728);
            }
            90
        } else {
            return value;
        };
        self.truncation.bits16(value * multiplier) / 100
    }
}

pub fn max_pp(pp: u8, no_pp_boosts: bool) -> u8 {
    if no_pp_boosts { pp } else { (pp / 5 + 1) * 4 }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum Truncation {
    #[default]
    Cartridge,
    /// Custom Game replaces Dex#trunc with Math.trunc, ignoring the bit count.
    Unbounded,
}

impl Truncation {
    fn bits16(self, value: u32) -> u32 {
        match self {
            Self::Cartridge => value & 0xffff,
            Self::Unbounded => value,
        }
    }

    fn word(self, value: u64) -> u64 {
        match self {
            Self::Cartridge => value & 0xffff_ffff,
            Self::Unbounded => value,
        }
    }

    /// Apply a fixed-point modifier (4096 = 1.0), rounding ties down.
    pub fn modify(self, value: u32, modifier: u32) -> u32 {
        ((self.word(u64::from(value) * u64::from(modifier)) + 2047) / 4096) as u32
    }
}

/// Chain event modifiers before applying them to the value (ties round up).
pub fn chain(previous: u32, next: u32) -> u32 {
    ((previous.wrapping_mul(next).wrapping_add(2048) as i32) >> 12) as u32
}

pub fn boosted_stat(value: u32, stage: i8) -> u32 {
    let stage = i32::from(stage.clamp(-6, 6));
    let numerator = (2 + stage.max(0)) as u32;
    let denominator = (2 - stage.min(0)) as u32;
    value * numerator / denominator
}

pub fn action_speed(stat: u32, trick_room: bool, twisted_dimension: bool) -> i32 {
    if trick_room ^ twisted_dimension {
        -(stat as i32)
    } else {
        stat as i32
    }
}

/// The numeric inputs to damage after resolving scripted events for this hit.
/// Immunities and fixed-damage moves bypass this calculation.
#[derive(Debug, Copy, Clone)]
pub struct Damage {
    pub level: u16,
    pub power: u32,
    pub attack: u32,
    pub defense: NonZeroU32,
    pub spread_or_bond: u32,
    pub weather: u32,
    pub critical: bool,
    pub stab: u32,
    pub effectiveness: i8,
    pub burned: bool,
    pub final_modifier: u32,
    pub bypassed_protect: bool,
}

impl Damage {
    pub fn roll(self, truncation: Truncation, rng: &mut Rng) -> u32 {
        let t = truncation;
        let level_factor = u64::from(self.level) * 2 / 5 + 2;
        let base = t.word(level_factor * u64::from(self.power) * u64::from(self.attack));
        let mut damage = (base / u64::from(self.defense.get()) / 50 + 2) as u32;
        damage = t.modify(damage, self.spread_or_bond);
        damage = t.modify(damage, self.weather);
        if self.critical {
            damage = t.word(u64::from(damage) * 3 / 2) as u32;
        }
        damage = (t.word(u64::from(damage) * (100 - u64::from(rng.below(16)))) / 100) as u32;
        damage = t.modify(damage, self.stab);
        let effectiveness = self.effectiveness.clamp(-6, 6);
        if effectiveness >= 0 {
            damage <<= effectiveness;
        } else {
            damage >>= -effectiveness;
        }
        if self.burned {
            damage = t.modify(damage, 2048);
        }
        damage = t.modify(damage, self.final_modifier);
        if self.bypassed_protect {
            damage = t.modify(damage, 1024);
        }
        t.bits16(damage.max(1))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Health {
    pub secret: String,
    pub shared: String,
}

/// Showdown Champions rounds public HP down, but a living Pokémon always
/// shows at least 1%. At the color boundaries the suffix disambiguates HP.
pub fn health(hp: u32, max_hp: NonZeroU32, status: &str, report_exact_hp: bool) -> Health {
    if hp == 0 {
        return Health {
            secret: "0 fnt".into(),
            shared: "0 fnt".into(),
        };
    }
    let max_hp = max_hp.get();
    let mut secret = format!("{hp}/{max_hp}");
    let mut shared = if report_exact_hp {
        secret.clone()
    } else {
        let percent = (100 * u64::from(hp) / u64::from(max_hp)).max(1);
        let suffix = match percent {
            20 if u64::from(hp) * 5 > u64::from(max_hp) => "y",
            20 => "r",
            50 if u64::from(hp) * 2 > u64::from(max_hp) => "g",
            50 => "y",
            _ => "",
        };
        format!("{percent}/100{suffix}")
    };
    if !status.is_empty() {
        secret.push(' ');
        secret.push_str(status);
        shared.push(' ');
        shared.push_str(status);
    }
    Health { secret, shared }
}
