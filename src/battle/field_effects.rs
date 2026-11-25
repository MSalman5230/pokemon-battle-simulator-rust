//! Short-lived inherited field conditions independent of weather and terrain.

use super::{turn::Position, *};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LegacyField {
    IonDeluge,
    WaterSport,
    MudSport,
}

#[derive(Clone, Copy, Default)]
pub(super) struct LegacyFieldState {
    turns: u8,
    has_field_target: bool,
}

impl LegacyField {
    pub(super) const ALL: [Self; 3] = [Self::IonDeluge, Self::WaterSport, Self::MudSport];

    pub(super) fn for_move(name: &str) -> Option<Self> {
        Some(match name {
            "Ion Deluge" => Self::IonDeluge,
            "Water Sport" => Self::WaterSport,
            "Mud Sport" => Self::MudSport,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::IonDeluge => "Ion Deluge",
            Self::WaterSport => "Water Sport",
            Self::MudSport => "Mud Sport",
        }
    }

    pub(super) fn residual_priority(self) -> (u32, i32) {
        match self {
            Self::IonDeluge => (0, 5),
            Self::WaterSport => (27, 3),
            Self::MudSport => (27, 4),
        }
    }
}

impl Battle {
    pub(super) fn legacy_field_active(&self, effect: LegacyField) -> bool {
        self.legacy_fields[effect as usize].turns != 0
    }

    pub(super) fn start_legacy_field(
        &mut self,
        pos: Position,
        effect: LegacyField,
        move_name: &str,
    ) -> bool {
        let state = &mut self.legacy_fields[effect as usize];
        if state.turns != 0 {
            return false;
        }
        *state = LegacyFieldState {
            turns: if effect == LegacyField::IonDeluge {
                1
            } else {
                5
            },
            has_field_target: false,
        };
        if effect == LegacyField::IonDeluge {
            self.add("|-fieldactivate|move: Ion Deluge");
            self.add(format!(
                "|-hint|Normal-type moves become Electric-type after using {move_name}."
            ));
        } else {
            self.add(format!(
                "|-fieldstart|move: {}|[of] {}",
                effect.name(),
                self.ident(pos)
            ));
        }
        true
    }

    pub(super) fn tick_legacy_field(&mut self, effect: LegacyField) {
        let state = &mut self.legacy_fields[effect as usize];
        state.turns -= 1;
        if state.turns == 0 && effect != LegacyField::IonDeluge {
            self.add(format!("|-fieldend|move: {}", effect.name()));
        }
    }

    pub(super) fn legacy_sport(&mut self, move_type: &str) -> Option<&'static str> {
        let mud = &mut self.legacy_fields[LegacyField::MudSport as usize];
        let mud_active = mud.turns != 0;
        let mud_target = mud.has_field_target;
        mud.has_field_target = mud_active;
        let water = &mut self.legacy_fields[LegacyField::WaterSport as usize];
        let water_active = water.turns != 0;
        // Native handlers acquire their field target when first dispatched.
        // Until then they sort as ordinary conditions, so only equally
        // initialized sports form a speed tie and consume a shuffle draw.
        if mud_active && water_active && mud_target == water.has_field_target {
            self.rng.below(2);
        }
        water.has_field_target = water_active;
        match move_type {
            "Electric" if mud_active => Some("mud sport weaken"),
            "Fire" if water_active => Some("water sport weaken"),
            _ => None,
        }
    }
}
