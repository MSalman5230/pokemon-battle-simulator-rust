//! Intersect the possible origins of restricted moves in one submitted set.

use super::{LEGALITY, MoveSources, flat::move_id};
use crate::{
    choice::to_id,
    dex::{DEX, Mod, Move, Species},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExactSource {
    Event {
        generation: u8,
        species: &'static str,
        index: usize,
    },
    Egg {
        generation: u8,
        lineage: &'static str,
    },
    DreamWorld {
        species: &'static str,
    },
    VirtualConsole(u8),
}

impl ExactSource {
    pub(super) fn generation(self) -> u8 {
        match self {
            Self::Event { generation, .. }
            | Self::Egg { generation, .. }
            | Self::VirtualConsole(generation) => generation,
            Self::DreamWorld { .. } => 5,
        }
    }
}

#[derive(Default)]
struct Pool {
    /// A transferable move allows any origin through the source generation.
    through: u8,
    exact: Vec<ExactSource>,
}

impl Pool {
    fn accepts(&self, source: ExactSource) -> bool {
        source.generation() <= self.through || self.exact.contains(&source)
    }

    fn intersect(&self, other: &Self) -> Self {
        let mut exact: Vec<_> = self
            .exact
            .iter()
            .copied()
            .filter(|&source| other.accepts(source))
            .collect();
        for &source in &other.exact {
            if self.accepts(source) && !exact.contains(&source) {
                exact.push(source);
            }
        }
        Self {
            through: self.through.min(other.through),
            exact,
        }
    }

    fn add(&mut self, source: ExactSource) {
        if !self.exact.contains(&source) {
            self.exact.push(source);
        }
    }

    fn empty(&self) -> bool {
        self.through == 0 && self.exact.is_empty()
    }
}

pub(super) struct SetSources {
    pool: Pool,
    restricted: Vec<&'static str>,
    go_incompatible: Vec<&'static str>,
    transfer_hm: bool,
    sketch: Option<&'static str>,
}

impl Default for SetSources {
    fn default() -> Self {
        Self {
            pool: Pool {
                through: 9,
                exact: Vec::new(),
            },
            restricted: Vec::new(),
            go_incompatible: Vec::new(),
            transfer_hm: false,
            sketch: None,
        }
    }
}

impl SetSources {
    pub(super) fn require(&mut self, source: ExactSource) {
        self.pool.through = 0;
        self.pool.exact.clear();
        self.pool.add(source);
    }

    pub(super) fn locked(&self) -> &[ExactSource] {
        if self.pool.through == 0 {
            &self.pool.exact
        } else {
            &[]
        }
    }

    pub(super) fn go_incompatible_moves(&self) -> &[&'static str] {
        &self.go_incompatible
    }

    pub(super) fn check_move(
        &mut self,
        dex: Mod,
        species: &Species,
        level: i64,
        min_gen: u8,
        data: &'static Move,
    ) -> Option<String> {
        let species_id = to_id(&species.name);
        let id = move_id(data);
        let level_u16 = level.clamp(1, i64::from(u16::MAX)) as u16;
        let source_kind = LEGALITY.move_sources(dex, &species_id, &id, level_u16);
        match source_kind {
            MoveSources::Unrestricted => return None,
            MoveSources::Absent => return Some(format!(" can't learn {}.", data.name)),
            _ => {}
        }
        let Some(rules) = LEGALITY.species(dex, &species_id) else {
            return Some(" can't learn any moves at all.".into());
        };
        let Some(parents) = rules.parents.as_ref().filter(|parents| !parents.is_empty()) else {
            return Some(" can't learn any moves at all.".into());
        };
        let mut pool = Pool::default();
        let mut reason = None;
        let mut sketch = id == "sketch";
        let mut ordinary = false;
        let mut blocked_hm = false;
        let mut pre_evolution = false;
        let minimum = rules.generation.max(min_gen).max(3);
        for parent in parents {
            let Some(learnset) = LEGALITY.learnset(dex, &parent.species) else {
                continue;
            };
            let Some(mon) = DEX.species(dex, &parent.species) else {
                continue;
            };
            let parent_rules = LEGALITY.species(dex, &parent.species).unwrap();
            pre_evolution |= mon.base_species != species.base_species && pool.empty();
            let direct = learnset.moves.get(&id).into_iter().flatten();
            let sketched = learnset.moves.get("sketch").filter(|_| id != "sketch");
            let sketched = if sketched.is_some()
                && (data.flags.contains_key("nosketch") || data.is_z_move)
            {
                reason = Some("can't be Sketched.".into());
                None
            } else {
                sketch |= sketched.is_some() && (direct.clone().next().is_none() || pool.empty());
                sketched
            };
            for source in sketched.into_iter().flatten().chain(direct) {
                let [generation @ b'1'..=b'9', mut method, ..] = *source.as_bytes() else {
                    continue;
                };
                let generation = generation - b'0';
                let mut natural_egg = method == b'E' && !matches!(&source[2..], "any" | "pomeg");
                if parent.egg_only && (generation < 9 || method != b'E') {
                    continue;
                }
                if generation < min_gen {
                    reason.get_or_insert_with(|| {
                        format!("can't be transferred from Gen {generation} to {min_gen}.")
                    });
                    continue;
                }
                if !parent_rules.tier.starts_with("CAP") {
                    if generation <= 3
                        && matches!(
                            id.as_str(),
                            "cut"
                                | "fly"
                                | "surf"
                                | "strength"
                                | "flash"
                                | "rocksmash"
                                | "waterfall"
                                | "dive"
                        )
                    {
                        reason = Some(
                            "can't be transferred from Gen 3 to 4 because it's an HM move.".into(),
                        );
                        continue;
                    }
                    if generation <= 4
                        && matches!(
                            id.as_str(),
                            "cut"
                                | "fly"
                                | "surf"
                                | "strength"
                                | "rocksmash"
                                | "waterfall"
                                | "rockclimb"
                        )
                    {
                        reason = Some(
                            "can't be transferred from Gen 4 to 5 because it's an HM move.".into(),
                        );
                        continue;
                    }
                    blocked_hm |= generation <= 4 && matches!(id.as_str(), "defog" | "whirlpool");
                }
                if method == b'L'
                    && generation != 7
                    && let Ok(required) = source[2..].parse::<i64>()
                    && level < required
                {
                    let can_inherit = parent_rules.can_hatch
                        && generation >= 2
                        && mon.gender.as_deref() != Some("N")
                        && (mon.gender.as_deref() != Some("M")
                            || self::mother_can_learn(dex, parent_rules.mother.as_deref(), &id));
                    if !can_inherit {
                        reason = Some(format!("is learned at level {required}."));
                        continue;
                    }
                    method = b'E';
                    natural_egg = false;
                }
                if generation == 9
                    && !pre_evolution
                    && (matches!(method, b'L' | b'M' | b'T') || natural_egg)
                {
                    return None;
                }
                if matches!(method, b'L' | b'M' | b'T' | b'R') || natural_egg && generation >= 8 {
                    ordinary = true;
                    if generation >= minimum {
                        pool.through = pool.through.max(generation);
                    }
                } else {
                    match method {
                        b'S' => {
                            if let Ok(index) = source[2..].parse() {
                                pool.add(ExactSource::Event {
                                    generation,
                                    species: &parent.species,
                                    index,
                                });
                            }
                        }
                        b'E' => pool.add(ExactSource::Egg {
                            generation,
                            lineage: if parent_rules.prevo.is_empty() {
                                ""
                            } else {
                                &parent.species
                            },
                        }),
                        b'D' => pool.add(ExactSource::DreamWorld {
                            species: &parent.species,
                        }),
                        b'V' if generation > min_gen => {
                            pool.add(ExactSource::VirtualConsole(generation))
                        }
                        _ => {}
                    }
                }
            }
        }
        if sketch && !ordinary {
            if let Some(first) = self.sketch {
                return Some(format!(
                    " can't Sketch {} and {first} because it can only Sketch 1 move.",
                    data.name
                ));
            }
            self.sketch = Some(&data.name);
        }
        if blocked_hm && std::mem::replace(&mut self.transfer_hm, true) {
            return Some(
                " can't simultaneously transfer Defog and Whirlpool from Gen 4 to 5.".into(),
            );
        }
        if !self.restricted.contains(&data.name.as_str()) {
            self.restricted.push(&data.name);
        }
        if pool.empty() {
            return Some(if let Some(reason) = reason {
                format!("'s move {} {reason}", data.name)
            } else {
                format!(" can't learn {}.", data.name)
            });
        }
        if pool.through < 8 && !pool.exact.contains(&ExactSource::VirtualConsole(8)) {
            self.go_incompatible.push(&data.name);
        }
        let intersection = self.pool.intersect(&pool);
        if intersection.empty() {
            return Some(format!(
                "'s moves {} are incompatible.",
                self.restricted.join(", ")
            ));
        }
        self.pool = intersection;
        None
    }
}

fn mother_can_learn(dex: Mod, mother: Option<&str>, move_id: &str) -> bool {
    mother
        .and_then(|name| LEGALITY.species(dex, &to_id(name)))
        .and_then(|rules| rules.parents.as_ref())
        .into_iter()
        .flatten()
        .any(|parent| {
            LEGALITY
                .learnset(dex, &parent.species)
                .is_some_and(|learnset| learnset.moves.contains_key(move_id))
        })
}
