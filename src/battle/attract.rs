//! Infatuation tracks a stable roster identity,
//! not a field slot that could be occupied by a different Pokémon after a switch.

use super::{turn::Position, *};

impl Battle {
    pub(super) fn attract_source(&self, target: Position) -> Option<Position> {
        let (player, roster) = self.mon(target).attract?;
        let slots = self.format.game_type.active_slots();
        self.party(player).unwrap()[..slots]
            .iter()
            .position(|p| p.roster_index == roster && !p.fainted)
            .map(|slot| Position { player, slot })
    }

    pub(super) fn attract_update(&mut self, target: Position) {
        if self.mon(target).attract.is_some() && self.attract_source(target).is_none() {
            if self.format.report_exact_hp {
                self.add(format!(
                    "|debug|Removing Attract volatile on {}",
                    self.ident(target)
                ));
            }
            let mon = self.mon_mut(target);
            mon.attract = None;
            mon.untrack_volatile("attract");
            self.add(format!("|-end|{}|Attract|[silent]", self.ident(target)));
        }
    }

    pub(super) fn cute_charm(&mut self, holder: Position, attacker: Position) {
        if !self.rng.chance(3, 10)
            || self.mon(attacker).hp == 0
            || self.mon(attacker).active_ability() == "oblivious"
            || self.mon(attacker).attract.is_some()
        {
            return;
        }
        if !self.compatible_attraction(holder, attacker) {
            return;
        }
        if self.aroma_veil_blocks(attacker, false) {
            return;
        }
        self.start_attraction(holder, attacker, true);
    }

    pub(super) fn compatible_attraction(&self, source: Position, target: Position) -> bool {
        matches!(
            (self.mon(source).gender, self.mon(target).gender),
            (b'M', b'F') | (b'F', b'M')
        )
    }

    pub(super) fn start_attraction(
        &mut self,
        source: Position,
        target: Position,
        cute_charm: bool,
    ) -> bool {
        self.apply_attraction(source, target, cute_charm.then_some("ability: Cute Charm"))
    }

    fn apply_attraction(
        &mut self,
        source: Position,
        target: Position,
        cause: Option<&str>,
    ) -> bool {
        let roster = self.mon(source).roster_index;
        let mon = self.mon_mut(target);
        if mon.hp == 0 || mon.attract.is_some() {
            return false;
        }
        mon.attract = Some((source.player, roster));
        mon.track_volatile("attract");
        if self.mon(target).item == "destinyknot" && !self.debug_ignored_item(target, "Attract") {
            if self.format.report_exact_hp {
                self.add(format!(
                    "|debug|attract intercepted: {} from {}",
                    self.ident(target),
                    self.ident(source)
                ));
            }
            if self.mon(source).attract.is_none()
                && self.event_ability(source) != "oblivious"
                && !self.aroma_veil_blocks(source, false)
            {
                self.apply_attraction(target, source, Some("item: Destiny Knot"));
            }
        }
        let cause = cause
            .map(|cause| format!("|[from] {cause}|[of] {}", self.ident(source)))
            .unwrap_or_default();
        self.add(format!("|-start|{}|Attract{cause}", self.ident(target)));
        true
    }
}
