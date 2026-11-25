//! Slot- and side-owned support effects.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct Wish {
    turn: u32,
    hp: u32,
    wisher: usize,
}

impl Battle {
    pub(super) fn start_salt_cure(&mut self, pos: Position) {
        if self.mon(pos).hp > 0 && !self.mon(pos).volatile_ids().contains(&"saltcure") {
            self.mon_mut(pos).track_volatile("saltcure");
            self.add(format!("|-start|{}|Salt Cure", self.ident(pos)));
        }
    }

    pub(super) fn tick_salt_cure(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let divisor = if mon.has_type("Water") || mon.has_type("Steel") {
            8
        } else {
            16
        };
        self.indirect_damage(pos, divisor, "|[from] Salt Cure");
        if self.mon(pos).hp == 0 && !self.mon(pos).fainted {
            self.mark_fainted(pos);
        }
    }

    fn overflowed_turn(&self) -> u32 {
        let previous = self.turn - 1;
        match self.stat_rules.truncation {
            crate::mechanics::Truncation::Cartridge => previous & 255,
            crate::mechanics::Truncation::Unbounded => previous,
        }
    }

    pub(super) fn start_wish(&mut self, pos: Position) -> bool {
        let side = self.sides[pos.player.index()].as_ref().unwrap();
        if let Some(wish) = side.wishes[pos.slot] {
            if wish.turn == 255 {
                self.add("|-hint|A previous Wish is stuck on this slot and will never resolve.");
            }
            return false;
        }
        let user = self.mon(pos);
        let wish = Wish {
            turn: self.overflowed_turn(),
            hp: (user.max_hp.get() / 2).max(1),
            wisher: user.roster_index,
        };
        self.sides[pos.player.index()].as_mut().unwrap().wishes[pos.slot] = Some(wish);
        if wish.turn == 255 {
            self.add(format!(
                "|-hint|In Gen 8+, Wish will never resolve when used on the {}th turn.",
                self.turn
            ));
        }
        true
    }

    pub(super) fn tick_wish(&mut self, pos: Position) {
        let turn = self.overflowed_turn();
        let side = self.sides[pos.player.index()].as_mut().unwrap();
        let Some(wish) = side.wishes[pos.slot].filter(|wish| turn > wish.turn) else {
            return;
        };
        side.wishes[pos.slot] = None;
        let mon = &mut side.party[pos.slot];
        if mon.hp == 0 || mon.hp == mon.max_hp.get() {
            return;
        }
        mon.hp = (mon.hp + wish.hp).min(mon.max_hp.get());
        let wisher = side
            .party
            .iter()
            .find(|m| m.roster_index == wish.wisher)
            .unwrap();
        let reason = format!("|[from] move: Wish|[wisher] {}", wisher.name);
        self.add_health_line_with("-heal", pos, false, &reason);
    }

    pub(super) fn start_tailwind(&mut self, player: Player) -> bool {
        let side = self.sides[player.index()].as_mut().unwrap();
        if side.tailwind != 0 {
            return false;
        }
        side.tailwind = 4;
        self.add(format!(
            "|-sidestart|{}|move: Tailwind",
            self.side_ident(player)
        ));
        true
    }

    pub(super) fn tick_tailwind(&mut self, player: Player) {
        let side = self.sides[player.index()].as_mut().unwrap();
        side.tailwind -= 1;
        if side.tailwind == 0 {
            self.add(format!(
                "|-sideend|{}|move: Tailwind",
                self.side_ident(player)
            ));
        }
    }
}
