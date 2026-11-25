//! Seeded Champions team generators, independent of the battle PRNG.

mod ranch;
mod random;

use crate::{
    choice::to_id,
    dex::{DEX, Format},
    input::Set,
    rng::Rng,
};
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct GenerationError(String);

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GenerationError {}

/// Generate a native Showdown team, advancing only the supplied team RNG.
pub fn generate_team(format_id: &str, rng: &mut Rng) -> Result<Vec<Set>, GenerationError> {
    let id = to_id(format_id);
    let format = DEX
        .format(&id)
        .ok_or_else(|| GenerationError(format!("unknown Champions format {format_id}")))?;
    generate_for_format(format, rng)
}

pub(crate) fn generate_for_format(
    format: &Format,
    rng: &mut Rng,
) -> Result<Vec<Set>, GenerationError> {
    match format.generator.as_deref() {
        Some("randomRRR") => Ok(ranch::generate(format, rng)),
        Some("random") => random::generate(format, rng),
        Some(_) => Err(GenerationError(format!(
            "not ported: team generation for {}",
            format.name
        ))),
        None => Err(GenerationError(format!(
            "{} requires a supplied team",
            format.name
        ))),
    }
}

fn index(rng: &mut Rng, len: usize) -> usize {
    assert!(len > 0, "cannot sample an empty source pool");
    rng.below(u32::try_from(len).expect("pool fits in u32")) as usize
}

fn sample<T: Copy>(rng: &mut Rng, pool: &[T]) -> T {
    pool[index(rng, pool.len())]
}

pub(super) fn type_booster(typing: &str) -> &'static str {
    match typing {
        "Bug" => "Silver Powder",
        "Dark" => "Black Glasses",
        "Dragon" => "Dragon Fang",
        "Electric" => "Magnet",
        "Fairy" => "Fairy Feather",
        "Fighting" => "Black Belt",
        "Fire" => "Charcoal",
        "Flying" => "Sharp Beak",
        "Ghost" => "Spell Tag",
        "Grass" => "Miracle Seed",
        "Ground" => "Soft Sand",
        "Ice" => "Never-Melt Ice",
        "Normal" => "Silk Scarf",
        "Poison" => "Poison Barb",
        "Psychic" => "Twisted Spoon",
        "Rock" => "Hard Stone",
        "Steel" => "Metal Coat",
        "Water" => "Mystic Water",
        _ => "Leftovers",
    }
}
