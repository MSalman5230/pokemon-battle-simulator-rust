//! Held-item hooks for the currently supported damage and status pipeline.

use super::turn::Position;
use super::*;

impl Battle {
    pub(super) fn on_switch_in_item(&mut self, pos: Position) {
        if self.mon(pos).item == "airballoon" {
            self.add(format!("|-item|{}|Air Balloon", self.ident(pos)));
        }
    }

    pub(super) fn focus_sash(&mut self, pos: Position, damage: u32) -> u32 {
        let mon = self.mon(pos);
        if mon.item == "focussash" && mon.hp == mon.max_hp.get() && damage >= mon.hp {
            let remaining = mon.hp - 1;
            self.consume(pos, "Focus Sash", false);
            remaining
        } else {
            damage
        }
    }

    pub(super) fn on_update_item(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        if mon.item == "sitrusberry" && mon.hp * 2 <= mon.max_hp.get() {
            let amount = mon.max_hp.get() / 4;
            self.consume(pos, "Sitrus Berry", true);
            self.item_heal(pos, amount, "Sitrus Berry");
        }
        self.cure_with_berry(pos);
    }

    pub(super) fn cure_with_berry(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        let berry = match (mon.item.as_str(), mon.status) {
            (_, Status::Healthy | Status::Fainted) => return,
            ("lumberry", _) => "Lum Berry",
            ("chestoberry", Status::Sleep(_)) => "Chesto Berry",
            _ => return,
        };
        self.consume(pos, berry, true);
        self.cure_status(pos, "[msg]");
    }

    pub(super) fn item_heal(&mut self, pos: Position, amount: u32, item: &str) {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.hp == mon.max_hp.get() {
            return;
        }
        mon.hp = (mon.hp + amount.max(1)).min(mon.max_hp.get());
        self.add_health_line_with("-heal", pos, false, &format!("|[from] item: {item}"));
    }

    pub(super) fn consume(&mut self, pos: Position, item: &str, eaten: bool) {
        self.mon_mut(pos).item.clear();
        let reason = if eaten { "|[eat]" } else { "" };
        self.add(format!("|-enditem|{}|{item}{reason}", self.ident(pos)));
    }

    pub(super) fn indirect_damage(&mut self, pos: Position, divisor: u32, cause: &str) {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 {
            return;
        }
        mon.hp = mon.hp.saturating_sub((mon.max_hp.get() / divisor).max(1));
        self.add_health_line_with("-damage", pos, false, cause);
    }
}

impl Pokemon {
    pub(super) fn holds_choice_item(&self) -> bool {
        matches!(
            self.item.as_str(),
            "choicescarf" | "choiceband" | "choicespecs"
        )
    }

    pub(super) fn disabled_by_choice(&self, slot: usize) -> bool {
        self.holds_choice_item()
            && self
                .choice_lock
                .is_some_and(|i| self.moves[i].id != self.moves[slot].id)
    }

    pub(super) fn item_power_modifier(&self, move_type: &str) -> u32 {
        if matches!(
            (self.item.as_str(), move_type),
            ("mysticwater", "Water")
                | ("miracleseed", "Grass")
                | ("charcoal", "Fire")
                | ("fairyfeather", "Fairy")
                | ("spelltag", "Ghost")
                | ("softsand", "Ground")
                | ("magnet", "Electric")
        ) {
            4915
        } else {
            4096
        }
    }
}
