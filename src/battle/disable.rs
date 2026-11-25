//! Imprison and per-owner knowledge of hidden disabled moves.

use super::{turn::Position, *};

impl Battle {
    pub(super) fn foe_imprison(&self, pos: Position) -> impl Iterator<Item = &Pokemon> {
        let slots = self.format.game_type.active_slots();
        self.party(pos.player.foe()).unwrap()[..slots]
            .iter()
            .filter(|p| p.imprisoned && !p.fainted)
    }

    pub(super) fn imprisoned_move(&self, pos: Position, name: &str) -> bool {
        name != "Struggle"
            && self
                .foe_imprison(pos)
                .any(|p| p.moves.iter().any(|m| m.move_data.name == name))
    }

    fn disabled_by_imprison(&self, pos: Position, slot: usize) -> bool {
        self.mon(pos)
            .moves
            .get(slot)
            .is_some_and(|m| self.imprisoned_move(pos, &m.move_data.name))
    }

    /// True move-slot disable flags, available only for the requesting
    /// player's own observation. Zero PP does not set the underlying flag.
    pub fn actual_move_disabled(
        &self,
        player: Player,
        party_slot: usize,
        move_slot: usize,
    ) -> bool {
        let pos = Position {
            player,
            slot: party_slot,
        };
        self.is_active(player, party_slot)
            && (self
                .mon(pos)
                .disabled_slot
                .is_some_and(|(slot, _)| slot == move_slot)
                || self.mon(pos).disabled_by_choice(move_slot)
                || self.disabled_by_imprison(pos, move_slot))
    }

    pub(super) fn visible_disable(&self, pos: Position, slot: usize) -> bool {
        let mon = self.mon(pos);
        mon.disabled_by_choice(slot)
            || mon
                .disabled_slot
                .is_some_and(|(disabled, _)| disabled == slot)
    }

    pub(super) fn actual_disabled(&self, pos: Position, slot: usize) -> bool {
        let mon = self.mon(pos);
        mon.moves[slot].pp == 0
            || mon.disabled_by_choice(slot)
            || self.disabled_by_imprison(pos, slot)
    }

    pub(super) fn disable_source(&self, pos: Position, slot: usize) -> Option<&'static str> {
        let mon = self.mon(pos);
        (mon.revealed_disables & (1_u32 << slot) != 0).then_some("")
    }

    pub(super) fn any_move_in_request(&self, pos: Position) -> bool {
        self.mon(pos)
            .moves
            .iter()
            .enumerate()
            .any(|(slot, _)| !self.move_disabled_in_request(pos.player, pos.slot, slot))
    }

    pub fn move_disabled_in_request(
        &self,
        player: Player,
        party_slot: usize,
        move_slot: usize,
    ) -> bool {
        let pos = Position {
            player,
            slot: party_slot,
        };
        let mon = self.mon(pos);
        mon.moves[move_slot].pp == 0
            || self.visible_disable(pos, move_slot)
            || self.disable_source(pos, move_slot).is_some()
            || ((!self.last_active(pos) || mon.revealed_all_disables)
                && self.disabled_by_imprison(pos, move_slot))
    }

    pub(super) fn reveal_disabled_move(&mut self, pos: Position, slot: usize) -> BattleError {
        if self.disable_source(pos, slot).is_some() {
            return BattleError::Choice("move is disabled");
        }
        let doubles = self.format.game_type.active_slots() > 1;
        let mon = self.mon_mut(pos);
        mon.revealed_disables |= 1_u32 << slot;
        mon.revealed_all_disables = doubles;
        BattleError::Unavailable("move is disabled")
    }
}

impl Battle {
    pub(super) fn disable_last_move(
        &mut self,
        target: Position,
        source: Position,
        cursed: bool,
    ) -> bool {
        let mon = self.mon(target);
        if mon.hp == 0 || mon.disabled_slot.is_some() {
            return false;
        }
        let Some(slot) = mon.last_move_slot.filter(|&slot| mon.moves[slot].pp > 0) else {
            return false;
        };
        let duration = if cursed || !mon.acted { 4 } else { 5 };
        let name = mon.moves[slot].move_data.name.as_str();
        let mon = self.mon_mut(target);
        mon.disabled_slot = Some((slot, duration));
        mon.track_volatile("disable");
        let reason = if cursed {
            format!("|[from] ability: Cursed Body|[of] {}", self.ident(source))
        } else {
            String::new()
        };
        self.add(format!(
            "|-start|{}|Disable|{name}{reason}",
            self.ident(target)
        ));
        true
    }

    pub(super) fn tick_disable(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        let (slot, turns) = mon.disabled_slot.unwrap();
        if turns > 1 {
            mon.disabled_slot = Some((slot, turns - 1));
            return;
        }
        mon.disabled_slot = None;
        mon.untrack_volatile("disable");
        self.add(format!("|-end|{}|Disable", self.ident(target)));
    }
}
