//! Copy positive stages at event boundaries, not during the originating boost.

use super::{turn::Position, *};
use crate::mechanics::BoostStat;

#[derive(Clone, Copy, Default)]
pub(super) struct PendingBoostCopy([Option<(BoostStat, i8)>; 7]);

impl PendingBoostCopy {
    pub(super) fn accumulate(&mut self, stat: BoostStat, amount: i8) {
        if amount <= 0 {
            return;
        }
        for slot in &mut self.0 {
            match slot {
                Some((key, value)) if *key == stat => {
                    *value = value.saturating_add(amount);
                    return;
                }
                None => {
                    *slot = Some((stat, amount));
                    return;
                }
                _ => {}
            }
        }
    }

    pub(super) fn entries(self) -> impl Iterator<Item = (BoostStat, i8)> {
        self.0.into_iter().flatten()
    }
}

impl Battle {
    pub(super) fn note_foe_boost(&mut self, target: Position, stat: BoostStat, amount: i8) {
        if self.possible_opportunist {
            let mut changes = PendingBoostCopy::default();
            changes.accumulate(stat, amount);
            self.note_foe_boosts(target, changes);
        }
    }

    pub(super) fn note_foe_boosts(&mut self, target: Position, changes: PendingBoostCopy) {
        if !self.possible_opportunist {
            return;
        }
        let (holders, count) =
            self.boost_copy_listeners(self.positions().filter(|p| p.player != target.player));
        for &(holder, _) in &holders[..count] {
            if self.event_ability(holder) == "opportunist" {
                let pending = &mut self.mon_mut(holder).opportunist;
                for (stat, amount) in changes.entries() {
                    pending.accumulate(stat, amount);
                }
            }
        }
    }

    pub(super) fn apply_pending_boost_copy(&mut self, pos: Position) {
        if self.mon(pos).ability != "opportunist" || self.event_ability(pos) != "opportunist" {
            return;
        }
        let pending = std::mem::take(&mut self.mon_mut(pos).opportunist);
        let mut changes = [(BoostStat::Atk, 0); 7];
        let mut count = 0;
        for change in pending.entries() {
            changes[count] = change;
            count += 1;
        }
        if count != 0 {
            self.ability_boosts(pos, "Opportunist", &changes[..count], false);
        }
    }

    pub(super) fn apply_pending_boost_copies(&mut self) {
        if !self.possible_opportunist {
            return;
        }
        let (holders, count) = self.boost_copy_listeners(self.positions());
        for &(holder, _) in &holders[..count] {
            self.apply_pending_boost_copy(holder);
        }
    }

    fn boost_copy_listeners(
        &mut self,
        candidates: impl Iterator<Item = Position>,
    ) -> ([(Position, i32); 4], usize) {
        let mut events = [(
            Position {
                player: Player::P1,
                slot: 0,
            },
            0,
        ); 4];
        let mut count = 0;
        for pos in candidates {
            let mon = self.mon(pos);
            if !mon.fainted && mon.ability == "opportunist" {
                events[count] = (pos, mon.speed);
                count += 1;
            }
        }
        speed_sort_by(&mut events[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        (events, count)
    }
}
