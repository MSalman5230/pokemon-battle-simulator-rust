//! Timed effects tied to a roster identity rather than an active slot.

use super::{abilities::BoostOrigin, turn::Position, *};
use crate::mechanics::BoostStat;

#[derive(Clone, Copy)]
pub(super) struct TargetTimer {
    side: Player,
    roster: u8,
    turns: u8,
}

impl TargetTimer {
    fn matches(self, pos: Position, mon: &Pokemon) -> bool {
        self.side == pos.player && usize::from(self.roster) == mon.roster_index
    }

    pub(super) fn tick(timer: &mut Option<Self>) -> bool {
        let Some(effect) = timer else {
            return false;
        };
        effect.turns -= 1;
        if effect.turns != 0 {
            return false;
        }
        *timer = None;
        true
    }
}

impl Battle {
    fn target_timer(&self, pos: Position, turns: u8) -> TargetTimer {
        TargetTimer {
            side: pos.player,
            roster: self.mon(pos).roster_index as u8,
            turns,
        }
    }

    pub(super) fn set_items_suppressed(&mut self, suppressed: bool) {
        for side in self.sides.iter_mut().flatten() {
            for mon in &mut side.party {
                mon.items_suppressed = suppressed;
            }
        }
    }

    pub(super) fn toggle_magic_room(&mut self, source: Position) {
        if let Some(room) = self.magic_room.take() {
            self.end_magic_room(room);
        } else {
            self.magic_room = Some(self.target_timer(source, 5));
            self.set_items_suppressed(true);
            self.add(format!(
                "|-fieldstart|move: Magic Room|[of] {}",
                self.ident(source)
            ));
            for pos in self.positions() {
                if !self.mon(pos).fainted {
                    self.debug_ignored_item(pos, "End");
                }
            }
        }
    }

    pub(super) fn tick_magic_room(&mut self) {
        let room = self.magic_room.unwrap();
        if TargetTimer::tick(&mut self.magic_room) {
            self.end_magic_room(room);
        }
    }

    fn end_magic_room(&mut self, room: TargetTimer) {
        self.set_items_suppressed(false);
        let party = self.party(room.side).unwrap();
        let slot = party
            .iter()
            .position(|mon| mon.roster_index == usize::from(room.roster))
            .unwrap();
        let name = if slot < self.slots() && !party[slot].fainted {
            self.ident(Position {
                player: room.side,
                slot,
            })
        } else {
            format!("{}: {}", room.side.id(), party[slot].name)
        };
        self.add(format!("|-fieldend|move: Magic Room|[of] {name}"));
    }

    pub(super) fn start_lock_on(
        &mut self,
        source: Position,
        target: Position,
        move_name: &str,
    ) -> bool {
        if self.mon(source).lock_on.is_some() {
            return false;
        }
        let timer = self.target_timer(target, 2);
        let user = self.mon_mut(source);
        user.lock_on = Some(timer);
        user.track_volatile("lockon");
        self.add(format!(
            "|-activate|{}|move: {move_name}|[of] {}",
            self.ident(source),
            self.ident(target)
        ));
        true
    }

    pub(super) fn locked_on_to(&self, source: Position, target: Position) -> bool {
        self.mon(source)
            .lock_on
            .is_some_and(|timer| timer.matches(target, self.mon(target)))
    }

    pub(super) fn start_syrup_bomb(&mut self, source: Position, target: Position) {
        let timer = self.target_timer(source, 4);
        let mon = self.mon_mut(target);
        if mon.hp == 0 || mon.syrup_bomb.is_some() {
            return;
        }
        mon.syrup_bomb = Some(timer);
        mon.track_volatile("syrupbomb");
        self.add(format!("|-start|{}|Syrup Bomb", self.ident(target)));
    }

    fn syrup_source(&self, timer: TargetTimer) -> Option<Position> {
        (0..self.slots())
            .map(|slot| Position {
                player: timer.side,
                slot,
            })
            .find(|&pos| !self.mon(pos).fainted && timer.matches(pos, self.mon(pos)))
    }

    pub(super) fn update_syrup_bomb(&mut self, target: Position) {
        if self
            .mon(target)
            .syrup_bomb
            .is_some_and(|timer| self.syrup_source(timer).is_none())
        {
            self.end_syrup_bomb(target);
        }
    }

    pub(super) fn tick_syrup_bomb(&mut self, target: Position) {
        let Some(timer) = self.mon(target).syrup_bomb else {
            return;
        };
        if TargetTimer::tick(&mut self.mon_mut(target).syrup_bomb) {
            self.end_syrup_bomb(target);
        } else if let Some(source) = self.syrup_source(timer) {
            self.boost_from(
                target,
                source,
                &[(BoostStat::Spe, -1)],
                BoostOrigin::SyrupBomb,
            );
        }
    }

    fn end_syrup_bomb(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        mon.syrup_bomb = None;
        mon.untrack_volatile("syrupbomb");
        self.add(format!("|-end|{}|Syrup Bomb|[silent]", self.ident(target)));
    }
}
