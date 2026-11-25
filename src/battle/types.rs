//! Base, changed, and added Pokémon typing.

use super::Pokemon;

pub(super) const TYPES: &[&str] = &[
    "Normal", "Fire", "Water", "Electric", "Grass", "Ice", "Fighting", "Poison", "Ground",
    "Flying", "Psychic", "Bug", "Rock", "Ghost", "Dragon", "Dark", "Steel", "Fairy",
];

// A copied or reconstructed type list may include three types. Store type
// indices rather than allocating or adding three pointers to every Pokémon.
#[derive(Clone, Copy, Default)]
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
    pub(super) fn set_temporary_type(&mut self, ty: Option<&str>) {
        self.temporary_types = ty.map_or_else(TypeOverride::default, |ty| {
            TypeOverride::from_types(&[ty]).expect("moves change to a known type")
        });
        self.added_type = None;
        self.spent_elements = 0;
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

    pub fn types(&self) -> impl Iterator<Item = &str> {
        let original_types = if self
            .transformed
            .as_ref()
            .is_some_and(|tr| tr.uses_base_types)
        {
            &self.base_species.types
        } else {
            &self.species.types
        };
        let base = if self.temporary_types.is_empty() {
            original_types.as_slice()
        } else {
            &[]
        };
        let raw = base
            .iter()
            .map(String::as_str)
            .chain(self.temporary_types.iter());
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
            .chain(self.added_type)
    }
}
