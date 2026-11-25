//! Forced consecutive attacks such as Outrage and Petal Dance.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct Rampage {
    pub(super) slot: usize,
    // This timer can expire on an unsuccessful or aborted continuation.
    duration: u8,
    // This separate counter decides whether ending the lock causes fatigue.
    remaining: u8,
}

impl Pokemon {
    pub(super) fn hard_locked(&self) -> bool {
        self.recharge > 0 || self.rampage.is_some() || self.charge.is_some()
    }
}

impl Battle {
    pub(super) fn rampage_hit(&mut self, pos: Position, slot: usize) {
        if let Some(lock) = &mut self.mon_mut(pos).rampage {
            if lock.remaining >= 2 {
                lock.duration = 2;
            }
        } else if self.mon(pos).hp > 0 {
            let remaining = 2 + self.rng.below(2) as u8;
            let mon = self.mon_mut(pos);
            mon.rampage = Some(Rampage {
                slot,
                duration: 2,
                remaining,
            });
            mon.track_volatile("lockedmove");
        }
    }

    pub(super) fn end_rampage_attempt(&mut self, pos: Position) {
        if self.mon(pos).rampage.is_some_and(|lock| lock.duration == 1) {
            self.end_rampage(pos, true);
        }
    }

    pub(super) fn tick_rampage(&mut self, pos: Position) {
        let Some(lock) = self.mon(pos).rampage else {
            return;
        };
        if lock.duration == 1 {
            self.end_rampage(pos, true);
        } else if matches!(self.mon(pos).status, Status::Sleep(_)) {
            self.end_rampage(pos, false);
        } else {
            let lock = self.mon_mut(pos).rampage.as_mut().unwrap();
            lock.duration -= 1;
            lock.remaining -= 1;
        }
    }

    fn end_rampage(&mut self, pos: Position, allow_fatigue: bool) {
        let mon = self.mon_mut(pos);
        let fatigue = mon.rampage.take().is_some_and(|lock| lock.remaining <= 1);
        mon.untrack_volatile("lockedmove");
        if allow_fatigue && fatigue {
            self.apply_confusion(pos, true);
        }
    }
}
