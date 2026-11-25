//! Copy positive stages at event boundaries, not during the originating boost.

use super::{turn::Position, *};
use crate::mechanics::BoostStat;

#[derive(Clone, Copy, Default)]
pub(super) struct PendingBoostCopy([Option<(BoostStat, i8)>; 7]);

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum CopyHook {
    Opportunist,
    MirrorHerb,
}

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
        if self.possible_boost_copy {
            let mut changes = PendingBoostCopy::default();
            changes.accumulate(stat, amount);
            self.note_foe_boosts(target, changes);
        }
    }

    pub(super) fn note_foe_boosts(&mut self, target: Position, changes: PendingBoostCopy) {
        if !self.possible_boost_copy {
            return;
        }
        let (holders, count) =
            self.boost_copy_listeners(self.positions().filter(|p| p.player != target.player));
        for &(holder, _, hook) in &holders[..count] {
            let pending = match hook {
                CopyHook::Opportunist if self.event_ability(holder) == "opportunist" => {
                    &mut self.mon_mut(holder).opportunist
                }
                CopyHook::MirrorHerb if self.mon(holder).effective_item() == "mirrorherb" => {
                    &mut self.mon_mut(holder).mirror_herb
                }
                _ => continue,
            };
            for (stat, amount) in changes.entries() {
                pending.accumulate(stat, amount);
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

    pub(super) fn apply_pending_mirror_herb(&mut self, pos: Position) {
        if self.mon(pos).effective_item() != "mirrorherb" || self.mon(pos).hp == 0 {
            return;
        }
        let pending = std::mem::take(&mut self.mon_mut(pos).mirror_herb);
        let mut changes = [(BoostStat::Atk, 0); 7];
        let mut count = 0;
        for change in pending.entries() {
            changes[count] = change;
            count += 1;
        }
        if count != 0 {
            self.use_boosting_item(pos, "Mirror Herb", &changes[..count]);
        }
    }

    pub(super) fn apply_pending_boost_copies(&mut self) {
        if !self.possible_boost_copy {
            return;
        }
        let (holders, count) = self.boost_copy_listeners(self.positions());
        for &(holder, _, hook) in &holders[..count] {
            match hook {
                CopyHook::Opportunist => self.apply_pending_boost_copy(holder),
                CopyHook::MirrorHerb => self.apply_pending_mirror_herb(holder),
            }
        }
    }

    fn boost_copy_listeners(
        &mut self,
        candidates: impl Iterator<Item = Position>,
    ) -> ([(Position, i32, CopyHook); 8], usize) {
        let mut events = [(
            Position {
                player: Player::P1,
                slot: 0,
            },
            0,
            CopyHook::Opportunist,
        ); 8];
        let mut count = 0;
        for pos in candidates {
            let mon = self.mon(pos);
            if mon.fainted {
                continue;
            }
            if mon.ability == "opportunist" {
                events[count] = (pos, mon.speed, CopyHook::Opportunist);
                count += 1;
            }
            if mon.item == "mirrorherb" {
                events[count] = (pos, mon.speed, CopyHook::MirrorHerb);
                count += 1;
            }
        }
        speed_sort_by(&mut events[..count], &mut self.rng, |a, b| {
            b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2))
        });
        (events, count)
    }
}
