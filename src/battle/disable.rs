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

    /// The underlying move-slot flags that Showdown last computed at the
    /// start of a turn. A mid-turn pivot or faint does not refresh them.
    pub fn actual_move_disabled(
        &self,
        player: Player,
        party_slot: usize,
        move_slot: usize,
    ) -> bool {
        self.party(player)
            .and_then(|party| party.get(party_slot))
            .is_some_and(|mon| {
                move_slot < mon.moves.len() && mon.observed_disables & (1 << move_slot) != 0
            })
    }

    pub(super) fn refresh_observed_disables(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let own_handlers = usize::from(mon.choice_lock.is_some())
            + usize::from(mon.taunt > 0)
            + usize::from(mon.encore.is_some())
            + usize::from(mon.disabled_slot.is_some());
        let mut handlers = [mon.speed; 6];
        let mut count = own_handlers;
        for foe in self.foe_imprison(pos) {
            handlers[count] = foe.speed;
            count += 1;
        }
        // DisableMove sorts these condition handlers together, even when the
        // final flags do not depend on which tied condition runs first.
        speed_sort_by(&mut handlers[..count], &mut self.rng, |a, b| b.cmp(a));
        let mon = self.mon_mut(pos);
        if !mon.holds_choice_item() {
            mon.choice_lock = None;
            mon.untrack_volatile("choicelock");
        }
        let flags = (0..self.mon(pos).moves.len()).fold(0, |flags, slot| {
            flags
                | (u32::from(
                    self.visible_disable(pos, slot) || self.disabled_by_imprison(pos, slot),
                ) << slot)
        });
        self.mon_mut(pos).observed_disables = flags;
    }

    pub(super) fn visible_disable(&self, pos: Position, slot: usize) -> bool {
        let mon = self.mon(pos);
        mon.disabled_by_choice(slot)
            || mon
                .disabled_slot
                .is_some_and(|(disabled, _)| disabled == slot)
            || (mon.moves[slot].id == "lastresort" && !mon.can_use_last_resort())
            || (mon.active_move_actions > 0
                && matches!(mon.moves[slot].id.as_str(), "fakeout" | "firstimpression"))
            || (mon.taunt > 0
                && mon.moves[slot].move_data.category == crate::dex::Category::Status
                && mon.moves[slot].id != "mefirst")
            || mon
                .encore
                .is_some_and(|(locked, _)| mon.moves[locked].id != mon.moves[slot].id)
    }

    pub(super) fn actual_disabled(&self, pos: Position, slot: usize) -> bool {
        let mon = self.mon(pos);
        mon.moves[slot].pp == 0
            || self.visible_disable(pos, slot)
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

impl Pokemon {
    pub(super) fn can_use_last_resort(&self) -> bool {
        self.moves.len() >= 2
            && self
                .moves
                .iter()
                .enumerate()
                .all(|(slot, m)| m.id == "lastresort" || self.used_moves & (1 << slot) != 0)
    }
}

impl Battle {
    pub(super) fn taunt(&mut self, target: Position, will_move: bool) -> bool {
        let mon = self.mon_mut(target);
        if mon.taunt > 0 {
            return false;
        }
        mon.taunt = 3 + u8::from(mon.active_turns > 0 && !will_move);
        mon.track_volatile("taunt");
        self.add(format!("|-start|{}|move: Taunt", self.ident(target)));
        true
    }

    pub(super) fn tick_taunt(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        mon.taunt -= 1;
        if mon.taunt == 0 {
            self.end_taunt(target);
        }
    }

    fn end_taunt(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        mon.taunt = 0;
        mon.untrack_volatile("taunt");
        self.add(format!("|-end|{}|move: Taunt", self.ident(target)));
    }

    pub(super) fn start_encore(&mut self, target: Position, will_move: bool) -> Option<usize> {
        let mon = self.mon(target);
        if mon.encore.is_some() || mon.hp == 0 {
            return None;
        }
        let slot = mon.last_move_slot.filter(|&slot| {
            let m = &mon.moves[slot];
            m.pp > 0 && !m.move_data.flags.contains_key("failencore")
        })?;
        let mon = self.mon_mut(target);
        mon.encore = Some((slot, 3 + u8::from(!will_move)));
        mon.track_volatile("encore");
        self.add(format!("|-start|{}|Encore", self.ident(target)));
        Some(slot)
    }

    pub(super) fn tick_encore(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        let (slot, turns) = mon.encore.unwrap();
        if turns > 1 && mon.moves[slot].pp > 0 {
            mon.encore = Some((slot, turns - 1));
        } else {
            self.end_encore(target);
        }
    }

    fn end_encore(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        mon.encore = None;
        mon.untrack_volatile("encore");
        self.add(format!("|-end|{}|Encore", self.ident(target)));
    }

    pub(super) fn mental_herb(&mut self, target: Position) {
        let mon = self.mon(target);
        let (attract, taunt, encore, disabled) = (
            mon.attract.is_some(),
            mon.taunt > 0,
            mon.encore.is_some(),
            mon.disabled_slot.is_some(),
        );
        if !(attract || taunt || encore || disabled) {
            return;
        }
        self.consume(target, "Mental Herb", false);
        self.symbiosis_after_item(target);
        if attract {
            let mon = self.mon_mut(target);
            mon.attract = None;
            mon.untrack_volatile("attract");
            let ident = self.ident(target);
            self.add(format!("|-end|{ident}|Attract|[silent]"));
            self.add(format!(
                "|-end|{ident}|move: Attract|[from] item: Mental Herb"
            ));
        }
        if taunt {
            self.end_taunt(target);
        }
        if encore {
            self.end_encore(target);
        }
        if disabled {
            self.end_disable(target);
        }
    }

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
        self.end_disable(target);
    }

    fn end_disable(&mut self, target: Position) {
        let mon = self.mon_mut(target);
        mon.disabled_slot = None;
        mon.untrack_volatile("disable");
        self.add(format!("|-end|{}|Disable", self.ident(target)));
    }
}
