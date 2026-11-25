//! Roster Ranch Rumble team construction from the pinned Champions generator.
//! See docs/LICENSE.pokemon-showdown for the original MIT license.

mod items;

use super::{index, sample};
use crate::{
    choice::to_id,
    dex::{Category, DEX, Format},
    input::{Set, SetStats},
    mechanics::{Stat, Stat::*},
    rng::Rng,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

static RANCH: LazyLock<Catalog> = LazyLock::new(|| {
    let data: Catalog = serde_json::from_str(include_str!("../../data/roster-ranch.json"))
        .expect("checked-in Roster Ranch catalogue must deserialize");
    assert_eq!(data.revision, DEX.revision);
    data
});

#[derive(Deserialize)]
struct Catalog {
    revision: String,
    natures: Vec<String>,
    sets: Vec<Recipe>,
    moves: HashMap<String, MoveFacts>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recipe {
    id: String,
    flagship_move: Option<String>,
    secondary_move: Option<String>,
    #[serde(default)]
    other_moves: Vec<String>,
    abilities: Vec<String>,
    mega_stones: Vec<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveFacts {
    fixed_damage: bool,
    sheer_force: bool,
    crash: bool,
    partially_trapped: bool,
}

const STATS: [Stat; 6] = [Hp, Atk, Def, Spa, Spd, Spe];
const NON_OFFENSIVE: [Stat; 4] = [Hp, Def, Spd, Spe];

pub(super) fn generate(format: &Format, rng: &mut Rng) -> Vec<Set> {
    let mut team = Vec::with_capacity(format.max_team_size);
    let mut pool: Vec<_> = RANCH.sets.iter().collect();
    let mut families = HashSet::new();
    while team.len() < format.max_team_size {
        let recipe = pool.swap_remove(index(rng, pool.len()));
        let species = DEX.species(format.dex_mod, &recipe.id).unwrap();
        if !families.insert(species.base_species.as_str()) {
            continue;
        }
        let ability = &recipe.abilities[index(rng, recipe.abilities.len())];
        let mut moves: Vec<_> = [&recipe.flagship_move, &recipe.secondary_move]
            .into_iter()
            .filter_map(Option::as_deref)
            .collect();
        let mut other: Vec<_> = recipe.other_moves.iter().map(String::as_str).collect();
        while moves.len() < format.max_move_count && !other.is_empty() {
            moves.push(other.swap_remove(index(rng, other.len())));
        }
        let move_data: Vec<_> = moves
            .iter()
            .map(|m| {
                let data = DEX.moves(format.dex_mod, &to_id(m)).unwrap();
                (to_id(&data.name), data)
            })
            .collect();
        let evs = evs(rng, &move_data);
        let nature = nature(rng, evs);
        // The item selector counts unique moves, preserving the order of the
        // first occurrence of each move, exactly as the JS Set does.
        let mut seen = HashSet::new();
        let unique = move_data
            .iter()
            .filter(|(id, _)| seen.insert(id.as_str()))
            .map(|(id, data)| (id.as_str(), *data, RANCH.moves[id]))
            .collect::<Vec<_>>();
        let item = items::choose(
            rng,
            items::SetInfo {
                id: &recipe.id,
                species,
                ability,
                moves: &unique,
                mega_stones: &recipe.mega_stones,
                evs,
                nature: DEX.nature(nature),
            },
        );
        let shiny = rng.chance(1, 1024);
        let gender = species
            .gender
            .as_deref()
            .filter(|g| !g.is_empty())
            .unwrap_or_else(|| if rng.below(2) == 0 { "M" } else { "F" });
        team.push(Set {
            name: species.base_species.clone(),
            species: species.name.clone(),
            gender: gender.into(),
            item: item.into(),
            ability: ability.clone(),
            moves: moves.into_iter().map(str::to_owned).collect(),
            evs,
            ivs: Some(SetStats::map(|_| 31)),
            nature: nature.clone(),
            level: Some(i32::from(format.adjust_level.unwrap_or(50))),
            happiness: Some(255),
            shiny,
            ..Set::default()
        });
    }
    team
}

fn evs(rng: &mut Rng, moves: &[(String, &crate::dex::Move)]) -> SetStats {
    let first = match moves[0].1.category {
        Category::Physical => Atk,
        Category::Special => Spa,
        Category::Status => sample(rng, &NON_OFFENSIVE),
    };
    let non_offensive = NON_OFFENSIVE
        .into_iter()
        .filter(|&s| s != first)
        .collect::<Vec<_>>();
    let second = sample(rng, &non_offensive);
    let mut last = non_offensive
        .into_iter()
        .filter(|&s| s != second)
        .collect::<Vec<_>>();
    for (stat, category) in [(Atk, Category::Physical), (Spa, Category::Special)] {
        if stat != first && moves.iter().any(|(_, m)| m.category == category) {
            last.push(stat);
        }
    }
    let third = sample(rng, &last);
    SetStats::map(|s| {
        if s == first || s == second {
            32
        } else if s == third {
            2
        } else {
            0
        }
    })
}

fn nature(rng: &mut Rng, evs: SetStats) -> &'static String {
    let roll = |rng: &mut Rng, points| {
        let eligible = STATS
            .into_iter()
            .filter(|&s| s != Hp && evs.raw(s) == points)
            .collect::<Vec<_>>();
        sample(rng, &eligible)
    };
    let plus = roll(rng, 32);
    let minus = roll(rng, 0);
    let mut nature = &RANCH.natures[index(rng, RANCH.natures.len())];
    for candidate in &RANCH.natures {
        let data = DEX.nature(candidate);
        if data.plus == Some(plus) && data.minus == Some(minus) {
            nature = candidate;
        }
    }
    nature
}
