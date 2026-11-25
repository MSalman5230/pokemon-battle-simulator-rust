//! Turn-scoped side protection.

use super::{turn::Position, *};
use crate::choice::TargetKind;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SideGuard {
    Wide,
    Quick,
}

impl SideGuard {
    pub(super) fn for_move(name: &str) -> Option<Self> {
        Some(match name {
            "Wide Guard" => Self::Wide,
            "Quick Guard" => Self::Quick,
            _ => return None,
        })
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Wide => "Wide Guard",
            Self::Quick => "Quick Guard",
        }
    }

    fn blocks(self, kind: TargetKind, priority: i8) -> bool {
        match self {
            Self::Wide => matches!(kind, TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes),
            Self::Quick => priority > 0,
        }
    }
}

impl Battle {
    pub(super) fn start_side_guard(&mut self, pos: Position, guard: SideGuard) -> bool {
        let side = self.sides[pos.player.index()].as_mut().unwrap();
        if side.guards.contains(&Some(guard)) {
            return false;
        }
        *side.guards.iter_mut().find(|g| g.is_none()).unwrap() = Some(guard);
        self.add(format!("|-singleturn|{}|{}", self.ident(pos), guard.name()));
        true
    }

    pub(super) fn blocking_side_guard(
        &self,
        player: Player,
        kind: TargetKind,
        priority: i8,
    ) -> Option<SideGuard> {
        self.sides[player.index()]
            .as_ref()
            .unwrap()
            .guards
            .iter()
            .copied()
            .flatten()
            .find(|guard| guard.blocks(kind, priority))
    }

    pub(super) fn clear_side_guard(&mut self, player: Player, guard: SideGuard) {
        let guards = &mut self.sides[player.index()].as_mut().unwrap().guards;
        for entry in guards {
            if *entry == Some(guard) {
                *entry = None;
            }
        }
    }

    pub(super) fn clear_side_guards(&mut self, player: Player) -> bool {
        let guards = &mut self.sides[player.index()].as_mut().unwrap().guards;
        std::mem::take(guards).iter().any(Option::is_some)
    }
}
