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

#[derive(Clone, Copy)]
pub(super) struct Uproar {
    pub(super) slot: usize,
    pub(super) remaining: u8,
}

impl Pokemon {
    pub(super) fn hard_locked(&self) -> bool {
        self.recharge > 0
            || self.rampage.is_some()
            || self.charge.is_some()
            || self.uproar.is_some()
            || self.rolling_attack.is_some()
            || self.bide.is_some()
    }
}

impl Battle {
    pub(super) fn wake_from_uproar(&mut self, target: Position) {
        for slot in 0..self.format.game_type.active_slots() {
            for player in [target.player, target.player.foe()] {
                let pos = Position { player, slot };
                if matches!(self.mon(pos).status, Status::Sleep(_)) {
                    self.cure_status(pos, "[msg]");
                }
            }
        }
    }

    pub(super) fn begin_uproar(&mut self, pos: Position, slot: usize) {
        let mon = self.mon_mut(pos);
        if mon.hp != 0 && mon.uproar.is_none() {
            mon.uproar = Some(Uproar { slot, remaining: 3 });
            mon.track_volatile("uproar");
            self.add(format!("|-start|{}|Uproar", self.ident(pos)));
        }
    }

    pub(super) fn tick_uproar(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if let Some(state) = &mut mon.uproar {
            state.remaining -= 1;
            if state.remaining == 0 || mon.throat_chop != 0 {
                mon.uproar = None;
                mon.untrack_volatile("uproar");
                self.add(format!("|-end|{}|Uproar", self.ident(pos)));
            } else {
                self.add(format!("|-start|{}|Uproar|[upkeep]", self.ident(pos)));
            }
        }
    }

    pub(super) fn uproar_blocks_sleep(&mut self, target: Position) -> bool {
        let mut effects = [(target, 0); 4];
        let mut count = 0;
        for pos in self.positions() {
            let mon = self.mon(pos);
            if !mon.fainted && mon.uproar.is_some() {
                effects[count] = (pos, mon.speed);
                count += 1;
            }
        }
        let effects = &mut effects[..count];
        speed_sort_by(effects, &mut self.rng, |a, b| b.1.cmp(&a.1));
        let Some(&(holder, _)) = effects.first() else {
            return false;
        };
        self.add(format!(
            "|-fail|{}|slp|[from] Uproar{}",
            self.ident(target),
            if holder == target { "|[msg]" } else { "" }
        ));
        true
    }

    pub(super) fn stop_protected_rampage(&mut self, pos: Position) {
        if self.mon(pos).rampage.is_some_and(|lock| lock.duration == 2) {
            self.end_rampage(pos, false);
        }
    }

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
