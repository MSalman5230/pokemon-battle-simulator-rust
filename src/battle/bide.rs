//! Bide stores damage from move hits, keyed by attacker identity rather than slot.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct Bide {
    pub(super) slot: usize,
    pub(super) remaining: u8,
    pub(super) damage: u32,
    pub(super) attacker: Option<(Player, u8)>,
}

impl Battle {
    pub(super) fn start_bide(&mut self, pos: Position, slot: usize) {
        let mon = self.mon_mut(pos);
        mon.bide = Some(Bide {
            slot,
            remaining: 3,
            damage: 0,
            attacker: None,
        });
        mon.track_volatile("bide");
        self.add(format!("|-start|{}|move: Bide", self.ident(pos)));
    }

    pub(super) fn end_bide(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.bide.take().is_some() {
            mon.untrack_volatile("bide");
            self.add(format!("|-end|{}|move: Bide|[silent]", self.ident(pos)));
        }
    }
}
