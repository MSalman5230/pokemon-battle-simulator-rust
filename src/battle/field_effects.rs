//! Short-lived inherited field conditions independent of weather and terrain.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) enum LegacyCondition {
    Telekinesis,
    Embargo,
    Powder,
    Spotlight,
}

#[derive(Clone, Copy, Default)]
pub(super) struct LegacyConditions(pub(super) [u8; 4]);

impl LegacyCondition {
    pub(super) const ALL: [Self; 4] = [
        Self::Telekinesis,
        Self::Embargo,
        Self::Powder,
        Self::Spotlight,
    ];

    pub(super) fn for_move(name: &str) -> Self {
        match name {
            "Telekinesis" => Self::Telekinesis,
            "Embargo" => Self::Embargo,
            "Powder" => Self::Powder,
            "Spotlight" => Self::Spotlight,
            _ => unreachable!("not a legacy condition"),
        }
    }

    pub(super) fn from_volatile(id: &str) -> Option<Self> {
        match id {
            "telekinesis" => Some(Self::Telekinesis),
            "embargo" => Some(Self::Embargo),
            "powder" => Some(Self::Powder),
            _ => None,
        }
    }

    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::Telekinesis => ("Telekinesis", "telekinesis"),
            Self::Embargo => ("Embargo", "embargo"),
            Self::Powder => ("Powder", "powder"),
            Self::Spotlight => ("Spotlight", "spotlight"),
        }
    }

    pub(super) fn residual_order(self) -> u32 {
        match self {
            Self::Telekinesis => 19,
            Self::Embargo => 21,
            Self::Powder | Self::Spotlight => 0,
        }
    }
}

impl Pokemon {
    pub(super) fn under(&self, condition: LegacyCondition) -> bool {
        self.legacy_conditions.0[condition as usize] != 0
    }

    pub(super) fn clear_legacy_condition(&mut self, condition: LegacyCondition) -> bool {
        if std::mem::take(&mut self.legacy_conditions.0[condition as usize]) == 0 {
            return false;
        }
        self.untrack_volatile(condition.labels().1);
        true
    }
}

impl Battle {
    /// Like the native Start event, `None` is an announced immunity and
    /// `Some(false)` is an ordinary failure with no side effect.
    pub(super) fn start_legacy_condition(
        &mut self,
        pos: Position,
        condition: LegacyCondition,
    ) -> Option<bool> {
        let mon = self.mon(pos);
        if mon.under(condition) {
            return Some(false);
        }
        let duration = match condition {
            LegacyCondition::Embargo => 5,
            LegacyCondition::Powder | LegacyCondition::Spotlight => 1,
            LegacyCondition::Telekinesis => {
                if matches!(
                    mon.base_species.base_species.as_str(),
                    "Diglett" | "Dugtrio" | "Palossand" | "Sandygast"
                ) || mon.base_species.name == "Gengar-Mega"
                {
                    self.add(format!("|-immune|{}", self.ident(pos)));
                    return None;
                }
                if self.forces_grounded(pos) {
                    return Some(false);
                }
                3
            }
        };
        let (name, id) = condition.labels();
        let mon = self.mon_mut(pos);
        mon.legacy_conditions.0[condition as usize] = duration;
        mon.track_volatile(id);
        let event = match condition {
            LegacyCondition::Powder => "singleturn",
            LegacyCondition::Spotlight => "singleturn",
            _ => "start",
        };
        let label = if matches!(condition, LegacyCondition::Spotlight) {
            "move: Spotlight"
        } else {
            name
        };
        self.add(format!("|-{event}|{}|{label}", self.ident(pos)));
        if matches!(condition, LegacyCondition::Embargo) && !self.mon(pos).item.is_empty() {
            self.debug_ignored_item(pos, "End");
        }
        Some(true)
    }

    pub(super) fn tick_legacy_condition(&mut self, pos: Position, condition: LegacyCondition) {
        let (name, id) = condition.labels();
        let mon = self.mon_mut(pos);
        let turns = &mut mon.legacy_conditions.0[condition as usize];
        *turns -= 1;
        if *turns == 0 {
            mon.untrack_volatile(id);
            if !matches!(
                condition,
                LegacyCondition::Powder | LegacyCondition::Spotlight
            ) {
                self.add(format!("|-end|{}|{name}", self.ident(pos)));
            }
        }
    }

    pub(super) fn stop_fire_with_powder(&mut self, pos: Position, move_name: &str) {
        self.add(format!(
            "|-activate|{}|move: Powder|[move] {move_name}",
            self.ident(pos)
        ));
        let hp = self.mon(pos).max_hp.get();
        let amount = ((hp + 2) / 4).max(1);
        self.indirect_damage_amount(pos, amount, "|[silent]");
    }

    pub(super) fn mist_active(&self, player: Player) -> bool {
        self.sides[player.index()].as_ref().unwrap().mist != 0
    }

    pub(super) fn start_mist(&mut self, player: Player) -> bool {
        if self.mist_active(player) {
            return false;
        }
        self.sides[player.index()].as_mut().unwrap().mist = 5;
        self.add(format!("|-sidestart|{}|Mist", self.side_ident(player)));
        true
    }

    pub(super) fn tick_mist(&mut self, player: Player) {
        let mist = &mut self.sides[player.index()].as_mut().unwrap().mist;
        *mist -= 1;
        if *mist == 0 {
            self.add(format!("|-sideend|{}|Mist", self.side_ident(player)));
        }
    }

    pub(super) fn clear_mist(&mut self, player: Player) {
        if self.mist_active(player) {
            self.sides[player.index()].as_mut().unwrap().mist = 0;
            self.add(format!("|-sideend|{}|Mist", self.side_ident(player)));
        }
    }
}

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
