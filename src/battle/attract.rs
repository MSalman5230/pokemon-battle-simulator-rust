//! Contact-induced infatuation. The condition tracks a stable roster identity,
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
            let mon = self.mon_mut(target);
            mon.attract = None;
            mon.untrack_volatile("attract");
            self.add(format!("|-end|{}|Attract|[silent]", self.ident(target)));
        }
    }

    pub(super) fn cute_charm(&mut self, holder: Position, attacker: Position) {
        if !self.rng.chance(3, 10)
            || self.mon(attacker).hp == 0
            || self.mon(attacker).attract.is_some()
        {
            return;
        }
        if !matches!(
            (self.mon(holder).gender, self.mon(attacker).gender),
            (b'M', b'F') | (b'F', b'M')
        ) {
            return;
        }
        let roster = self.mon(holder).roster_index;
        let mon = self.mon_mut(attacker);
        mon.attract = Some((holder.player, roster));
        mon.track_volatile("attract");
        self.add(format!(
            "|-start|{}|Attract|[from] ability: Cute Charm|[of] {}",
            self.ident(attacker),
            self.ident(holder)
        ));
    }
}
