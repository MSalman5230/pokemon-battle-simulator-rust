//! Voluntary switching and private discovery of trapping abilities.

use super::{turn::Position, *};
use crate::mechanics::BoostStat;

#[derive(Default, Copy, Clone)]
pub(super) struct Trap {
    pub actual: bool,
    pub possible: bool,
}

impl Battle {
    pub(super) fn start_linked_trap(&mut self, source: Position, target: Position) -> bool {
        if self.mon(source).hp == 0
            || self.mon(target).hp == 0
            || self.mon(target).has_type("Ghost")
            || self.mon(target).volatile_ids().contains(&"trapped")
        {
            return false;
        }
        let source_id = (source.player, self.mon(source).roster_index as u8);
        self.mon_mut(source).track_volatile("trapper");
        let mon = self.mon_mut(target);
        mon.linked_trap_from = Some(source_id);
        mon.track_volatile("trapped");
        self.add(format!("|-activate|{}|trapped", self.ident(target)));
        true
    }

    pub(super) fn start_mutual_trap(&mut self, source: Position, target: Position) {
        if [source, target].into_iter().any(|pos| {
            let mon = self.mon(pos);
            mon.has_type("Ghost") || mon.volatile_ids().contains(&"trapped")
        }) {
            return;
        }
        self.start_linked_trap(target, source);
        self.start_linked_trap(source, target);
    }

    pub(super) fn clear_linked_traps(&mut self, leaving: Position) {
        let trapped_by = self.mon_mut(leaving).linked_trap_from.take();
        if self.mon(leaving).volatile_ids().contains(&"trapper") {
            let source_id = (leaving.player, self.mon(leaving).roster_index as u8);
            for pos in self.positions() {
                let mon = self.mon_mut(pos);
                if mon.linked_trap_from == Some(source_id) {
                    mon.linked_trap_from = None;
                    mon.untrack_volatile("trapped");
                }
            }
        }
        if let Some((player, index)) = trapped_by
            && !self
                .positions()
                .any(|pos| self.mon(pos).linked_trap_from == trapped_by)
        {
            let trapper = self.positions().find(|&pos| {
                pos.player == player && self.mon(pos).roster_index == usize::from(index)
            });
            if let Some(trapper) = trapper {
                self.mon_mut(trapper).untrack_volatile("trapper");
            }
        }
    }

    pub(super) fn start_octolock(&mut self, source: Position, target: Position) -> bool {
        if self.mon(target).octolock.is_some() {
            return false;
        }
        let source_id = (source.player, self.mon(source).roster_index);
        let mon = self.mon_mut(target);
        mon.octolock = Some(source_id);
        mon.track_volatile("octolock");
        self.add(format!(
            "|-start|{}|move: Octolock|[of] {}",
            self.ident(target),
            self.ident(source)
        ));
        true
    }

    fn octolock_source(&self, target: Position) -> Option<Position> {
        let (player, roster_index) = self.mon(target).octolock?;
        self.positions().find(|&pos| {
            pos.player == player
                && !self.mon(pos).fainted
                && self.mon(pos).roster_index == roster_index
        })
    }

    pub(super) fn tick_octolock(&mut self, target: Position) {
        if let Some(source) = self.octolock_source(target)
            && self.mon(source).hp > 0
            && self.mon(source).active_turns > 0
        {
            self.boost_from(
                target,
                source,
                &[(BoostStat::Def, -1), (BoostStat::Spd, -1)],
                super::abilities::BoostOrigin::Octolock,
            );
        } else {
            let mon = self.mon_mut(target);
            mon.octolock = None;
            mon.untrack_volatile("octolock");
            self.add(format!(
                "|-end|{}|Octolock|[partiallytrapped]|[silent]",
                self.ident(target)
            ));
        }
    }

    fn ignores_trapping(&self, pos: Position) -> bool {
        let mon = self.mon(pos);
        mon.has_type("Ghost") || mon.effective_item() == "shedshell"
    }

    pub(super) fn trap_info(&self, pos: Position) -> Trap {
        let mon = self.mon(pos);
        if mon.hard_locked() {
            return Trap {
                actual: true,
                possible: true,
            };
        }
        if mon.fainted {
            return Trap::default();
        }
        let affected = |ability: &str| match ability {
            "arenatrap" => self.grounded(pos),
            "magnetpull" => mon.has_type("Steel"),
            "shadowtag" => mon.ability != "shadowtag",
            _ => false,
        };
        let foe = self.party(pos.player.foe()).unwrap();
        let active = &foe[..self.format.game_type.active_slots()];
        let live = active.iter().filter(|mon| !mon.fainted);
        let foe_can_trap = live.clone().any(|foe| affected(&foe.ability));
        let actual = !self.ignores_trapping(pos)
            && (mon.volatile_ids().contains(&"trapped")
                || mon.volatile_ids().contains(&"ingrain")
                || self.fairy_lock != 0
                || self.is_bound(pos)
                || self.octolock_source(pos).is_some()
                || foe_can_trap);
        let possible = actual
            || (foe_can_trap && mon.effective_item() != "shedshell")
            || live
                .flat_map(|foe| {
                    foe.species
                        .abilities
                        .iter()
                        .filter(|(slot, _)| **slot != "H" || !foe.species.unreleased_hidden)
                })
                .any(|(_, name)| {
                    let id = to_id(name);
                    affected(&id) && !self.format.has_rule(&format!("-ability:{id}"))
                });
        Trap { actual, possible }
    }

    pub(super) fn last_active(&self, pos: Position) -> bool {
        let slots = self.format.game_type.active_slots();
        self.party(pos.player).unwrap()[pos.slot + 1..slots]
            .iter()
            .all(|p| p.fainted)
    }

    /// Whether the owner currently has an affirmative ("trapped") request.
    /// This intentionally does not expose the true value of a hidden trap.
    pub fn trapped_in_request(&self, player: Player, slot: usize) -> bool {
        let pos = Position { player, slot };
        let mon = self.mon(pos);
        let visible_trap = (mon.volatile_ids().contains(&"trapped")
            || mon.volatile_ids().contains(&"ingrain")
            || self.fairy_lock != 0
            || self.is_bound(pos)
            || self.octolock_source(pos).is_some())
            && !self.ignores_trapping(pos);
        mon.hard_locked()
            || mon.revealed_trap
            || ((visible_trap || (!self.last_active(pos) && self.trap_info(pos).actual))
                && self.party(player).unwrap()[self.format.game_type.active_slots()..]
                    .iter()
                    .any(|p| !p.fainted))
    }
}
