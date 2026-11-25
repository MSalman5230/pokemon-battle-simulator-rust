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
                "perishsong" => self.perish_count = source.perish_count,
                "throatchop" => self.throat_chop = source.throat_chop,
                "confusion" => self.confusion = source.confusion,
                "stall" => self.stall = source.stall,
                "allyswitch" => self.ally_switch_stall = source.ally_switch_stall,
                "electrify" => self.electrified = source.electrified,
                "gastroacid"
                    if !DEX
                        .ability(&self.base_ability)
                        .is_some_and(|a| a.cannot_suppress) =>
                {
                    self.gastro_acid = source.gastro_acid;
                }
                "protect" | "kingsshield" | "banefulbunker" | "spikyshield" | "burningbulwark"
                | "obstruct" | "silktrap" => self.protection = source.protection,
                "healblock" => self.heal_block = source.heal_block,
                "embargo" | "telekinesis" | "powder" | "magiccoat" | "snatch" => {
                    self.copy_legacy_condition(source, id)
                }
                "octolock" => self.octolock = source.octolock,
                "dragoncheer" => self.dragon_cheer = source.dragon_cheer,
                "laserfocus" => self.laser_focus = source.laser_focus,
                "furycutter" => self.fury_cutter = source.fury_cutter,
                "gem" => self.gem_boost = source.gem_boost,
                "powertrick" | "powershift" => self.swap_attack_defense(),
                "aquaring" | "ingrain" | "tarshot" => {}
                "roost" => self.roosted = source.roosted,
                "followme" | "ragepowder" => self.attention = source.attention,
                "flinch" => self.flinched = source.flinched,
                "focuspunch" => self.lost_focus = source.lost_focus,
                "helpinghand" => self.helping_hand = source.helping_hand,
                "chillyreception" | "focusenergy" | "curse" | "noretreat" | "charge" | "grudge"
                | "rage" => {}
                // The source could not execute a voluntary pivot while it's
                // committed to an unfinished charge, rampage, or recharge.
                // All other known effects either have noCopy or are unrelated.
                _ => continue,
            }
            self.track_volatile(id);
        }
    }
}
