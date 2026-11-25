//! Base, changed, and added Pokémon typing.

use super::{Battle, BattleError, DEX, Pokemon, SetStats, to_id, turn::Position};
use crate::mechanics::{HIDDEN_POWER_TYPES, hidden_power_type_index};

pub(super) const TYPES: &[&str] = &[
    "Normal", "Fire", "Water", "Electric", "Grass", "Ice", "Fighting", "Poison", "Ground",
    "Flying", "Psychic", "Bug", "Rock", "Ghost", "Dragon", "Dark", "Steel", "Fairy",
];

pub(super) fn hidden_power_index(
    explicit: Option<&str>,
    ivs: Option<SetStats>,
) -> Result<u8, BattleError> {
    if let Some(explicit) = explicit.filter(|entry| !entry.is_empty()) {
        let id = to_id(explicit);
        return HIDDEN_POWER_TYPES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(&id))
            .map(|i| i as u8)
            .ok_or_else(|| BattleError::Unsupported(format!("Hidden Power type {explicit}")));
    }
    let Some(ivs) = ivs else { return Ok(15) };
    Ok(hidden_power_type_index(
        [ivs.hp, ivs.atk, ivs.def, ivs.spe, ivs.spa, ivs.spd].map(|iv| i64::from(iv.clamp(0, 31))),
    ))
}

// A copied or reconstructed type list may include three types. Store type
// indices rather than allocating or adding three pointers to every Pokémon.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TypeOverride([u8; 3]);

impl TypeOverride {
    pub(super) fn from_types<S: AsRef<str>>(source: &[S]) -> Option<Self> {
        if source.len() > 3 {
            return None;
        }
        let mut types = Self::default();
        if source.is_empty() {
            types.0[0] = 2; // getTypes defaults an empty list to Normal.
        }
        for (slot, ty) in types.0.iter_mut().zip(source) {
            *slot = match ty.as_ref() {
                "???" => 1,
                ty => 2 + TYPES.iter().position(|&t| t == ty)? as u8,
            };
        }
        Some(types)
    }

    pub(super) fn is_empty(self) -> bool {
        self.0[0] == 0
    }

    fn iter(&self) -> impl Iterator<Item = &'static str> + Clone {
        self.0.iter().copied().take_while(|&id| id != 0).map(|id| {
            if id == 1 {
                "???"
            } else {
                TYPES[usize::from(id - 2)]
            }
        })
    }
}

impl Pokemon {
    pub(super) fn hidden_power_type(&self) -> &'static str {
        HIDDEN_POWER_TYPES[usize::from(self.hidden_power_index)]
    }

    pub(super) fn set_temporary_type(&mut self, ty: Option<&str>) {
        self.temporary_types = ty.map_or_else(TypeOverride::default, |ty| {
            TypeOverride::from_types(&[ty]).expect("moves change to a known type")
        });
        self.added_type = None;
        self.spent_elements = 0;
        self.unrevealed_types = TypeOverride::default();
        self.unknown_type = false;
    }

    pub(super) fn element_bit(ty: &str) -> u8 {
        match ty {
            "Fire" => 1,
            "Electric" => 2,
            _ => 0,
        }
    }

    pub(super) fn has_type(&self, name: &str) -> bool {
        self.types().any(|t| t == name)
    }

    pub fn types(&self) -> impl Iterator<Item = &'static str> {
        self.base_types().chain(self.added_type)
    }

    pub(super) fn item_species_type(&self) -> Option<&'static str> {
        if self.base_species.has_condition && self.transformed.is_none() {
            match (self.base_species.name.as_str(), self.ability.as_str()) {
                ("Arceus", "multitype") => Some(
                    DEX.item(crate::dex::Mod::Champions, &self.item)
                        .and_then(|item| item.plate_type.as_deref())
                        .unwrap_or("Normal"),
                ),
                ("Silvally", "rkssystem") => Some(
                    DEX.item(crate::dex::Mod::Champions, &self.item)
                        .and_then(|item| item.memory_type.as_deref())
                        .unwrap_or("Normal"),
                ),
                _ => None,
            }
        } else {
            None
        }
    }

    fn base_types(&self) -> impl Iterator<Item = &'static str> {
        let item_type = self.item_species_type();
        let original_types: &'static Vec<String> = if self
            .transformed
            .as_ref()
            .is_some_and(|tr| tr.uses_base_types)
        {
            &self.base_species.types
        } else {
            &self.species.types
        };
        let base = if self.temporary_types.is_empty() && item_type.is_none() {
            original_types.as_slice()
        } else {
            &[]
        };
        let raw = base
            .iter()
            .map(String::as_str)
            .chain(
                self.temporary_types
                    .iter()
                    .filter(move |_| item_type.is_none()),
            )
            .chain(item_type);
        let normal = self.roosted && raw.clone().all(|t| t == "Flying");
        raw.filter(move |&ty| !self.roosted || ty != "Flying")
            .chain(normal.then_some("Normal"))
            .map(|ty| {
                if self.spent_elements & Self::element_bit(ty) != 0 {
                    "???"
                } else {
                    ty
                }
            })
    }
}

impl Battle {
    pub(super) fn set_camouflage_type(&mut self, target: Position, conversion: bool) -> bool {
        let mon = self.mon(target);
        let typing = if conversion {
            let Some(first) = mon.moves.first() else {
                return false;
            };
            DEX.moves(self.format.dex_mod, &first.id)
                .unwrap_or(first.move_data)
                .move_type
                .as_str()
        } else {
            self.terrain().map_or("Normal", |terrain| terrain.element())
        };
        if matches!(mon.species.base_species.as_str(), "Arceus" | "Silvally")
            || typing == "Stellar"
            || if conversion {
                mon.has_type(typing)
            } else {
                mon.types().eq([typing])
            }
        {
            return false;
        }
        self.mon_mut(target).set_temporary_type(Some(typing));
        self.add(format!(
            "|-start|{}|typechange|{typing}",
            self.ident(target)
        ));
        true
    }

    pub(super) fn mark_identified(&mut self, pos: Position, miracle: bool) -> bool {
        let mon = self.mon_mut(pos);
        if mon.is_identified() {
            return false;
        }
        let (id, label) = if miracle {
            ("miracleeye", "Miracle Eye")
        } else {
            ("foresight", "Foresight")
        };
        mon.track_volatile(id);
        self.add(format!("|-start|{}|{label}", self.ident(pos)));
        true
    }

    pub(super) fn reflect_type(&mut self, source: Position, target: Position) -> bool {
        let user = self.mon(source);
        let model = self.mon(target);
        if matches!(user.species.base_species.as_str(), "Arceus" | "Silvally") {
            return false;
        }
        let copied: Vec<_> = model.base_types().filter(|&ty| ty != "???").collect();
        let added = model.added_type;
        if copied.is_empty() && added.is_none() {
            return false;
        }
        let known = source.player == target.player && !model.unknown_type;
        let old = if known {
            TypeOverride::default()
        } else if user.unrevealed_types.is_empty() {
            let old: Vec<_> = user.base_types().collect();
            TypeOverride::from_types(&old).unwrap()
        } else {
            user.unrevealed_types
        };
        let copied = TypeOverride::from_types(&copied).unwrap();
        let user = self.mon_mut(source);
        user.temporary_types = copied;
        user.unrevealed_types = old;
        user.unknown_type = !known;
        user.added_type = added;
        user.spent_elements = 0;
        self.add(format!(
            "|-start|{}|typechange|[from] move: Reflect Type|[of] {}",
            self.ident(source),
            self.ident(target)
        ));
        true
    }

    pub(super) fn announce_reflected_type(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.unrevealed_types.is_empty() || mon.illusion.is_some() || mon.fainted {
            return;
        }
        if !mon.base_types().eq(mon.unrevealed_types.iter()) {
            let types = mon.base_types().collect::<Vec<_>>().join("/");
            let added = mon.added_type;
            let name = self.ident(pos);
            self.add(format!("|-start|{name}|typechange|{types}|[silent]"));
            if let Some(added) = added {
                self.add(format!("|-start|{name}|typeadd|{added}|[silent]"));
            }
        }
        self.mon_mut(pos).unrevealed_types = TypeOverride::default();
    }
}

impl Pokemon {
    pub(super) fn is_identified(&self) -> bool {
        self.volatile_ids()
            .iter()
            .any(|&id| matches!(id, "miracleeye" | "foresight"))
    }

    pub(super) fn exposed_immunity(&self, typing: &str, move_type: &str) -> bool {
        match (typing, move_type) {
            ("Ghost", "Normal" | "Fighting") => self.volatile_ids().contains(&"foresight"),
            ("Dark", "Psychic") => self.volatile_ids().contains(&"miracleeye"),
            _ => false,
        }
    }
}
