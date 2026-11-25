//! Poke-RL's versioned metadata for hypothetical damage queries.

use crate::{
    battle::{DamagePreview, DamageProbe},
    dex::DEX,
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Deserialize)]
struct MetadataFile {
    revision: String,
    metadata: Value,
}

static METADATA: LazyLock<MetadataFile> = LazyLock::new(|| {
    let data: MetadataFile = serde_json::from_str(include_str!("../../data/tactics-metadata.json"))
        .expect("checked-in tactics metadata must deserialize");
    assert_eq!(data.revision, DEX.revision);
    data
});

/// The pinned Poke-RL oracle's classification, independent of any game.
pub fn metadata() -> &'static Value {
    &METADATA.metadata
}

/// Resolve each public scenario in an isolated native battle, in input order.
pub fn calculate_many(queries: &Value) -> Result<Vec<DamagePreview>, String> {
    use serde::ser::{SerializeMap, Serializer};
    use std::collections::hash_map::Entry;

    let mut prepared = HashMap::new();
    let mut key = Vec::new();
    let queries = queries.as_array().ok_or("calculate must contain queries")?;
    queries
        .iter()
        .map(|query| {
            let object = query.as_object().ok_or("expected damage query object")?;
            let move_name = query["move"].as_str().ok_or("expected hypothetical move")?;
            key.clear();
            let mut serializer = serde_json::Serializer::new(&mut key);
            let mut map = serializer
                .serialize_map(None)
                .map_err(|err| err.to_string())?;
            for (name, value) in object.iter().filter(|(name, _)| *name != "move") {
                map.serialize_entry(name, value)
                    .map_err(|err| err.to_string())?;
            }
            map.end().map_err(|err| err.to_string())?;
            let entry = match prepared.entry(key.clone()) {
                Entry::Occupied(prior) => prior.into_mut(),
                Entry::Vacant(new) => {
                    let probe = DamageProbe::deserialize(query).map_err(|err| err.to_string())?;
                    new.insert(probe.prepare().map_err(|err| err.to_string())?.0)
                }
            };
            entry.calculate(move_name).map_err(|err| err.to_string())
        })
        .collect()
}
