//! Showdown's pipe-delimited battle team format, not the human-readable export.

use super::{Set, SetStats};
use crate::{
    choice::to_id,
    dex::{DEX, Mod},
};

pub(crate) fn unpack(data: &str) -> Result<Vec<Set>, &'static str> {
    let data = data.trim();
    if data.starts_with('[') && data.ends_with(']') {
        // Teams.unpack also accepts JSON team exports. Normalizing to a packed
        // team means the default ability is filled from its slot.
        let mut sets: Vec<Set> =
            serde_json::from_str(data).map_err(|_| "invalid JSON team export")?;
        for set in &mut sets {
            if set.ability.is_empty() {
                set.ability = resolve_ability(&set.species, "")?;
            }
        }
        return Ok(sets);
    }
    if data.is_empty() {
        return Err("empty packed team");
    }
    data.split(']').take(24).map(unpack_set).collect()
}

fn unpack_set(text: &str) -> Result<Set, &'static str> {
    let mut fields = text.splitn(12, '|');
    let mut next = || fields.next().ok_or("missing packed team field");
    let [
        name,
        species,
        item,
        ability,
        moves,
        nature,
        points,
        gender,
        ivs,
        shiny,
        level,
        misc,
    ] = [
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
        next()?,
    ];
    let species = if species.is_empty() { name } else { species };
    let species = DEX
        .species(Mod::Champions, &to_id(species))
        .ok_or("unknown packed species")?;
    let misc: Vec<_> = misc.split(',').take(6).collect();
    let get = |i: usize| misc.get(i).copied().filter(|s| !s.is_empty());
    Ok(Set {
        name: name.to_owned(),
        species: species.name.clone(),
        item: item.to_owned(),
        ability: resolve_ability(&species.name, ability)?,
        moves: moves.split(',').take(24).map(str::to_owned).collect(),
        nature: nature.to_owned(),
        gender: gender.to_owned(),
        evs: stats(points, 0),
        ivs: (!ivs.is_empty()).then(|| stats(ivs, 31)),
        shiny: !shiny.is_empty(),
        level: level.parse().ok(),
        happiness: get(0).and_then(|s| s.parse().ok()),
        hp_type: get(1).map(str::to_owned),
        pokeball: get(2).unwrap_or_default().to_owned(),
        gigantamax: get(3).is_some(),
        dynamax_level: get(4).and_then(|s| s.parse().ok()),
        tera_type: get(5).map(str::to_owned),
        ..Set::default()
    })
}

fn stats(data: &str, empty: i32) -> SetStats {
    let mut fields = data.split(',');
    let mut next = || match fields.next() {
        Some("") => empty,
        Some(value) => value.parse().unwrap_or(0),
        None => 0,
    };
    SetStats {
        hp: next(),
        atk: next(),
        def: next(),
        spa: next(),
        spd: next(),
        spe: next(),
    }
}

fn resolve_ability(species: &str, value: &str) -> Result<String, &'static str> {
    if !matches!(value, "" | "0" | "1" | "H" | "S") {
        return Ok(value.to_owned());
    }
    let species = DEX
        .species(Mod::Champions, &to_id(species))
        .ok_or("unknown packed species")?;
    let slot = if value.is_empty() { "0" } else { value };
    let abilities = species
        .packed_ability_slots
        .as_ref()
        .unwrap_or(&species.abilities);
    Ok(abilities.get(slot).cloned().unwrap_or_else(|| {
        if value.is_empty() {
            String::new()
        } else {
            "!!!ERROR!!!".into()
        }
    }))
}
