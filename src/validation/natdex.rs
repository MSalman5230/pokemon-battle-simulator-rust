//! National Dex move sources are checked individually before Champions fallback.

use super::{DEX, ModRules, to_id};
use serde::Deserialize;
use std::{collections::HashSet, sync::LazyLock};

pub(super) static NATDEX: LazyLock<NatDexCatalog> = LazyLock::new(|| {
    let catalog: NatDexCatalog =
        serde_json::from_str(include_str!("../../data/champions-natdex-legality.json"))
            .expect("checked-in National Dex sources must deserialize");
    assert_eq!(catalog.revision, DEX.revision);
    catalog
});

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NatDexCatalog {
    revision: String,
    #[serde(flatten)]
    data: ModRules,
    transferable_items: HashSet<String>,
    missing_moves: HashSet<String>,
    unsketchable: HashSet<String>,
}

impl NatDexCatalog {
    pub(super) fn tier(&self, species: &str) -> Option<&str> {
        self.data.species.get(species)?.nat_dex_tier.as_deref()
    }

    pub(super) fn can_transfer_item(&self, item: &str) -> bool {
        self.transferable_items.contains(item)
    }

    pub(super) fn missing_move(&self, id: &str) -> bool {
        self.missing_moves.contains(id)
    }

    pub(super) fn can_learn(&self, species: &str, move_id: &str, level: i64) -> bool {
        let Some(base) = self.data.species.get(species) else {
            return false;
        };
        let Some(parents) = &base.parents else {
            return false;
        };
        for parent in parents {
            let (Some(learnset), Some(mon)) = (
                self.data.learnsets.get(&parent.species),
                self.data.species.get(&parent.species),
            ) else {
                continue;
            };
            let basic = learnset.moves.get(move_id).into_iter().flatten();
            let sketched = (!self.unsketchable.contains(move_id) && move_id != "sketch")
                .then(|| learnset.moves.get("sketch"))
                .flatten()
                .into_iter()
                .flatten();
            for source in basic.chain(sketched) {
                let [digit @ b'1'..=b'9', method, ..] = source.as_bytes() else {
                    continue;
                };
                let generation = digit - b'0';
                if parent.egg_only && (*method != b'E' || generation < 9) {
                    continue;
                }
                if base.evo_region.as_deref() == Some("Alola")
                    && base.num != mon.num
                    && generation >= 8
                    && *method != b'E'
                {
                    continue;
                }
                if !mon.tier.starts_with("CAP") && is_blocked_hm(move_id, generation) {
                    continue;
                }
                if *method == b'L' && generation != 7 {
                    let required = source[2..].parse::<i64>();
                    if !required.is_ok_and(|required| level >= required) {
                        let can_inherit_lvl_move = mon.can_hatch
                            && generation >= 2
                            && mon.gender.as_deref() != Some("N")
                            && (mon.gender.as_deref() != Some("M")
                                || mon.mother.as_deref().is_some_and(|mother| {
                                    self.mother_can_learn(&to_id(mother), move_id)
                                }));
                        if !can_inherit_lvl_move {
                            continue;
                        }
                    }
                }
                // The NatDex override supplies fresh unrestricted sources for
                // each move, so successful Egg and Event sources do not limit
                // the other moves in the submitted Champions set.
                if matches!(*method, b'L' | b'M' | b'T' | b'R' | b'E' | b'S' | b'D')
                    || (*method == b'V' && generation > 1)
                {
                    return true;
                }
            }
        }
        false
    }

    fn mother_can_learn(&self, mother: &str, move_id: &str) -> bool {
        self.data
            .species
            .get(mother)
            .and_then(|rules| rules.parents.as_ref())
            .into_iter()
            .flatten()
            .any(|parent| {
                self.data
                    .learnsets
                    .get(&parent.species)
                    .is_some_and(|pool| pool.moves.contains_key(move_id))
            })
    }
}

fn is_blocked_hm(move_id: &str, generation: u8) -> bool {
    (generation <= 4
        && matches!(
            move_id,
            "cut" | "fly" | "surf" | "strength" | "rocksmash" | "waterfall" | "rockclimb"
        ))
        || (generation <= 3 && matches!(move_id, "flash" | "dive"))
}
