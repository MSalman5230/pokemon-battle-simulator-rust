//! The second Pledge combines with the first action's move, not its target.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pledge {
    Fire,
    Grass,
    Water,
}

impl Pledge {
    pub(super) const ALL: [Self; 3] = [Self::Fire, Self::Grass, Self::Water];

    pub(super) fn for_move(data: &Move) -> Option<Self> {
        Some(match data.name.as_str() {
            "Fire Pledge" => Self::Fire,
            "Grass Pledge" => Self::Grass,
            "Water Pledge" => Self::Water,
            _ => return None,
        })
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Fire => "firepledge",
            Self::Grass => "grasspledge",
            Self::Water => "waterpledge",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Fire => "Fire Pledge",
            Self::Grass => "Grass Pledge",
            Self::Water => "Water Pledge",
        }
    }

    pub(super) fn combo(self, previous: Self) -> Option<Self> {
        match (self, previous) {
            (Self::Fire, Self::Grass) | (Self::Grass, Self::Fire) => Some(Self::Fire),
            (Self::Grass, Self::Water) | (Self::Water, Self::Grass) => Some(Self::Grass),
            (Self::Fire, Self::Water) | (Self::Water, Self::Fire) => Some(Self::Water),
            _ => None,
        }
    }

    pub(super) fn element(self) -> &'static str {
        match self {
            Self::Fire => "Fire",
            Self::Grass => "Grass",
            Self::Water => "Water",
        }
    }

    pub(super) fn source_label(self) -> &'static str {
        match self {
            Self::Fire => "|[from] move: Fire Pledge",
            Self::Grass => "|[from] move: Grass Pledge",
            Self::Water => "|[from] move: Water Pledge",
        }
    }

    pub(super) fn residual_sub_order(self) -> i32 {
        match self {
            Self::Water => 7,
            Self::Fire => 8,
            Self::Grass => 9,
        }
    }
}

impl Battle {
    pub(super) fn pledge_active(&self, player: Player, pledge: Pledge) -> bool {
        self.sides[player.index()].as_ref().unwrap().pledges[pledge as usize] != 0
    }

    pub(super) fn start_pledge_condition(&mut self, player: Player, pledge: Pledge) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().pledges[pledge as usize];
        if *turns == 0 {
            *turns = 4;
            self.add(format!(
                "|-sidestart|{}|{}",
                self.side_ident(player),
                pledge.name()
            ));
        }
    }

    pub(super) fn tick_pledge(&mut self, player: Player, pledge: Pledge) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().pledges[pledge as usize];
        *turns -= 1;
        if *turns == 0 {
            self.add(format!(
                "|-sideend|{}|{}",
                self.side_ident(player),
                pledge.name()
            ));
        }
    }
}
