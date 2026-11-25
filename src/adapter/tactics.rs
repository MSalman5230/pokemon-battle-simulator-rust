//! Poke-RL's versioned metadata for hypothetical damage queries.

use crate::{
    battle::{DamagePreview, DamageProbe},
    dex::DEX,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;

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
    queries
        .as_array()
        .ok_or("calculate must contain queries")?
        .iter()
        .map(|query| {
            let probe = DamageProbe::deserialize(query).map_err(|err| err.to_string())?;
            probe.calculate().map_err(|err| err.to_string())
        })
        .collect()
}
