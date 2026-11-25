//! Side protection against attacks and status effects.

use super::{turn::Position, *};
use crate::{choice::TargetKind, dex::Category};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SideGuard {
    Wide,
    Quick,
    Crafty,
    MatBlock,
}

impl SideGuard {
    pub(super) fn for_move(name: &str) -> Option<Self> {
        Some(match name {
            "Wide Guard" => Self::Wide,
            "Quick Guard" => Self::Quick,
            "Crafty Shield" => Self::Crafty,
            "Mat Block" => Self::MatBlock,
            _ => return None,
        })
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Wide => "Wide Guard",
            Self::Quick => "Quick Guard",
            Self::Crafty => "Crafty Shield",
            Self::MatBlock => "Mat Block",
        }
    }

    pub(super) fn checks_protect(self) -> bool {
        self != Self::Crafty
    }

    pub(super) fn grows_stall(self) -> bool {
        matches!(self, Self::Wide | Self::Quick)
    }

    fn blocks(self, data: &Move, kind: TargetKind, priority: i8) -> bool {
        match self {
            Self::Wide => matches!(kind, TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes),
            Self::Quick => priority > 0,
            Self::Crafty => {
                data.category == Category::Status
                    && !matches!(kind, TargetKind::All | TargetKind::User)
            }
            Self::MatBlock => kind != TargetKind::User && data.category != Category::Status,
        }
    }
}

impl Battle {
    pub(super) fn start_lucky_chant(&mut self, player: Player) -> bool {
        let side = self.sides[player.index()].as_mut().unwrap();
        if side.lucky_chant != 0 {
            return false;
        }
        side.lucky_chant = 5;
        self.add(format!(
            "|-sidestart|{}|move: Lucky Chant",
            self.side_ident(player)
        ));
        true
    }

    pub(super) fn tick_lucky_chant(&mut self, player: Player) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().lucky_chant;
        *turns = turns.saturating_sub(1);
        if *turns == 0 {
            self.add(format!(
                "|-sideend|{}|move: Lucky Chant",
                self.side_ident(player)
            ));
        }
    }

    pub(super) fn lucky_chant_active(&self, player: Player) -> bool {
        self.sides[player.index()].as_ref().unwrap().lucky_chant != 0
    }

    pub(super) fn start_safeguard(&mut self, player: Player) -> bool {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().safeguard;
        if *turns > 0 {
            return false;
        }
        *turns = 5;
        self.add(format!("|-sidestart|{}|Safeguard", self.side_ident(player)));
        true
    }

    pub(super) fn end_safeguard(&mut self, player: Player) {
        self.sides[player.index()].as_mut().unwrap().safeguard = 0;
        self.add(format!("|-sideend|{}|Safeguard", self.side_ident(player)));
    }

    pub(super) fn tick_safeguard(&mut self, player: Player) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().safeguard;
        if *turns > 0 {
            *turns -= 1;
            if *turns == 0 {
                self.end_safeguard(player);
            }
        }
    }

    pub(super) fn safeguard_blocks(
        &mut self,
        target: Position,
        source: Position,
        move_effect: bool,
        reveal: bool,
        volatile: bool,
    ) -> bool {
        if target == source
            || self.sides[target.player.index()]
                .as_ref()
                .unwrap()
                .safeguard
                == 0
        {
            return false;
        }
        if move_effect
            && target.player != source.player
            && self.event_ability(source) == "infiltrator"
        {
            return false;
        }
        if !volatile && self.format.report_exact_hp {
            self.add("|debug|interrupting setStatus");
        }
        if reveal {
            self.add(format!("|-activate|{}|move: Safeguard", self.ident(target)));
        }
        true
    }

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
        data: &Move,
        kind: TargetKind,
        priority: i8,
        damaging_z: bool,
    ) -> Option<SideGuard> {
        self.sides[player.index()]
            .as_ref()
            .unwrap()
            .guards
            .iter()
            .copied()
            .flatten()
            .filter(|guard| {
                guard.blocks(data, kind, priority)
                    && (!guard.checks_protect() || damaging_z || data.flags.contains_key("protect"))
            })
            .min_by_key(|guard| !guard.grows_stall())
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
