//! Effects carried by Baton Pass or Shed Tail. Ordinary switches and
//! effects marked `noCopy` in the Champions rules do not use this path.

use super::*;

impl Pokemon {
    pub(super) fn receive_pass(&mut self, source: &Pokemon, shield_only: bool) {
        if !shield_only {
            self.boosts = source.boosts;
        }
        for &id in source.volatile_ids() {
            if shield_only && id != "substitute" {
                continue;
            }
            match id {
                "substitute" => self.substitute_hp = source.substitute_hp,
                "taunt" => self.taunt = source.taunt,
                "leechseed" => self.seeded_from = source.seeded_from,
                "partiallytrapped" => self.partial_trap = source.partial_trap,
                "confusion" => self.confusion = source.confusion,
                "stall" => self.stall = source.stall,
                "protect" | "kingsshield" => self.protection = source.protection,
                "roost" => self.roosted = source.roosted,
                "followme" | "ragepowder" => self.attention = source.attention,
                "flinch" => self.flinched = source.flinched,
                "focuspunch" => self.lost_focus = source.lost_focus,
                "helpinghand" => self.helping_hand = source.helping_hand,
                "chillyreception" | "focusenergy" | "curse" | "noretreat" => {}
                // The source could not execute a voluntary pivot while it's
                // committed to an unfinished charge, rampage, or recharge.
                // All other known effects either have noCopy or are unrelated.
                _ => continue,
            }
            self.track_volatile(id);
        }
    }
}
