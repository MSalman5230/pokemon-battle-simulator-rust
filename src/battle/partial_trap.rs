//! Binding moves keep a reference to the trapping Pokémon, not its field slot.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct PartialTrap {
    pub(super) source: Player,
    pub(super) trapper: usize,
    pub(super) move_name: &'static str,
    pub(super) turns: u8,
    pub(super) divisor: u32,
}

pub(super) fn binding_move(name: &str) -> Option<&'static str> {
    Some(match name {
        "Bind" => "Bind",
        "Clamp" => "Clamp",
        "Fire Spin" => "Fire Spin",
        "Infestation" => "Infestation",
        "Magma Storm" => "Magma Storm",
        "Sand Tomb" => "Sand Tomb",
        "Snap Trap" => "Snap Trap",
        "Thunder Cage" => "Thunder Cage",
        "Whirlpool" => "Whirlpool",
        "Wrap" => "Wrap",
        _ => return None,
    })
}

impl Battle {
    pub(super) fn start_partial_trap(
        &mut self,
        target: Position,
        source: Position,
        move_name: &'static str,
    ) {
        if self.mon(target).hp == 0 || self.mon(target).partial_trap.is_some() {
            return;
        }
        let user = self.mon(source);
        let trapper = user.roster_index;
        let divisor = if user.item == "bindingband" { 6 } else { 8 };
        let turns = if user.item == "gripclaw" {
            8
        } else {
            5 + self.rng.below(2) as u8
        };
        let affected = self.mon_mut(target);
        affected.partial_trap = Some(PartialTrap {
            source: source.player,
            trapper,
            move_name,
            turns,
            divisor,
        });
        affected.track_volatile("partiallytrapped");
        self.add(format!(
            "|-activate|{}|move: {move_name}|[of] {}",
            self.ident(target),
            self.ident(source)
        ));
    }

    fn trapper(&self, trap: PartialTrap) -> Option<&Pokemon> {
        let slots = self.format.game_type.active_slots();
        self.party(trap.source)?[..slots]
            .iter()
            .find(|m| m.roster_index == trap.trapper && m.hp > 0)
    }

    pub(super) fn is_bound(&self, target: Position) -> bool {
        self.mon(target)
            .partial_trap
            .is_some_and(|effect| self.trapper(effect).is_some())
    }

    pub(super) fn end_partial_trap(&mut self, pos: Position, silent: bool) {
        let mon = self.mon_mut(pos);
        if let Some(effect) = mon.partial_trap.take() {
            mon.untrack_volatile("partiallytrapped");
            let silence = if silent { "|[silent]" } else { "" };
            self.add(format!(
                "|-end|{}|{}|[partiallytrapped]{silence}",
                self.ident(pos),
                effect.move_name
            ));
        }
    }

    pub(super) fn tick_partial_trap(&mut self, target: Position) {
        let Some(mut trap) = self.mon(target).partial_trap else {
            return;
        };
        trap.turns -= 1;
        if trap.turns == 0 {
            self.end_partial_trap(target, false);
        } else if self.trapper(trap).is_none_or(|m| m.active_turns == 0) {
            self.end_partial_trap(target, true);
        } else {
            self.mon_mut(target).partial_trap = Some(trap);
            self.indirect_damage(
                target,
                trap.divisor,
                &format!("|[from] move: {}|[partiallytrapped]", trap.move_name),
            );
            if self.mon(target).hp == 0 {
                self.mark_fainted(target);
            }
        }
    }
}
