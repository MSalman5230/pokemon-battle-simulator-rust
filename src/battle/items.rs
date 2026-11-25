//! Held-item hooks for the currently supported damage and status pipeline.

use super::turn::Position;
use super::*;
use crate::mechanics::BoostStat;

pub(super) fn resistance_berry(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "chilanberry" => ("Normal", "Chilan Berry"),
        "occaberry" => ("Fire", "Occa Berry"),
        "passhoberry" => ("Water", "Passho Berry"),
        "wacanberry" => ("Electric", "Wacan Berry"),
        "rindoberry" => ("Grass", "Rindo Berry"),
        "yacheberry" => ("Ice", "Yache Berry"),
        "chopleberry" => ("Fighting", "Chople Berry"),
        "kebiaberry" => ("Poison", "Kebia Berry"),
        "shucaberry" => ("Ground", "Shuca Berry"),
        "cobaberry" => ("Flying", "Coba Berry"),
        "payapaberry" => ("Psychic", "Payapa Berry"),
        "tangaberry" => ("Bug", "Tanga Berry"),
        "chartiberry" => ("Rock", "Charti Berry"),
        "kasibberry" => ("Ghost", "Kasib Berry"),
        "habanberry" => ("Dragon", "Haban Berry"),
        "colburberry" => ("Dark", "Colbur Berry"),
        "babiriberry" => ("Steel", "Babiri Berry"),
        "roseliberry" => ("Fairy", "Roseli Berry"),
        _ => return None,
    })
}

impl Battle {
    pub(super) fn on_switch_in_item(&mut self, pos: Position) {
        if self.mon(pos).item == "airballoon" {
            self.add(format!("|-item|{}|Air Balloon", self.ident(pos)));
        }
    }

    fn on_gained_item(&mut self, pos: Position) {
        self.on_switch_in_item(pos);
        self.white_herb(pos);
    }

    pub(super) fn white_herb(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.item != "whiteherb" || !mon.boosts.clear_negative() {
            return;
        }
        self.consume(pos, "White Herb", false);
        self.add(format!("|-clearnegativeboost|{}|[silent]", self.ident(pos)));
        self.symbiosis_after_item(pos);
    }

    pub(super) fn white_herbs_after_action(&mut self) {
        let mut holders = [(
            Position {
                player: Player::P1,
                slot: 0,
            },
            0,
        ); 4];
        let mut count = 0;
        for pos in self.positions() {
            let mon = self.mon(pos);
            if !mon.fainted && mon.item == "whiteherb" {
                holders[count] = (pos, mon.speed);
                count += 1;
            }
        }
        let holders = &mut holders[..count];
        speed_sort_by(holders, &mut self.rng, |a, b| b.1.cmp(&a.1));
        for &(pos, _) in holders.iter() {
            self.white_herb(pos);
        }
    }

    pub(super) fn focus_sash(&mut self, pos: Position, damage: u32) -> u32 {
        let mon = self.mon(pos);
        if mon.item == "focussash" && mon.hp == mon.max_hp.get() && damage >= mon.hp {
            let remaining = mon.hp - 1;
            self.consume(pos, "Focus Sash", false);
            self.symbiosis_after_item(pos);
            remaining
        } else {
            damage
        }
    }

    pub(super) fn on_update_item(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp > 0 && mon.item == "mentalherb" {
            self.mental_herb(pos);
            return;
        }
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        if mon.item == "sitrusberry" && mon.hp * 2 <= mon.max_hp.get() {
            let amount = mon.max_hp.get() / 4;
            self.consume(pos, "Sitrus Berry", true);
            self.item_heal(pos, amount, "Sitrus Berry");
            self.symbiosis_after_item(pos);
        }
        self.pinch_stat_berry(pos);
        self.cure_with_berry(pos);
    }

    pub(super) fn cure_with_berry(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        let berry = match (mon.item.as_str(), mon.status) {
            ("lumberry", _) if mon.confusion > 0 => "Lum Berry",
            (_, Status::Healthy | Status::Fainted) => return,
            ("lumberry", _) => "Lum Berry",
            ("chestoberry", Status::Sleep(_)) => "Chesto Berry",
            _ => return,
        };
        self.consume(pos, berry, true);
        self.cure_status(pos, "[msg]");
        if berry == "Lum Berry" && self.mon(pos).confusion > 0 {
            self.cure_confusion(pos);
        }
        self.symbiosis_after_item(pos);
    }

    fn pinch_stat_berry(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let (name, stat) = match mon.item.as_str() {
            "liechiberry" => ("Liechi Berry", BoostStat::Atk),
            "ganlonberry" => ("Ganlon Berry", BoostStat::Def),
            "salacberry" => ("Salac Berry", BoostStat::Spe),
            "petayaberry" => ("Petaya Berry", BoostStat::Spa),
            "apicotberry" => ("Apicot Berry", BoostStat::Spd),
            _ => return,
        };
        if mon.hp
            * if self.event_ability(pos) == "gluttony" {
                2
            } else {
                4
            }
            > mon.max_hp.get()
        {
            return;
        }
        self.consume(pos, name, true);
        let amount = if self.event_ability(pos) == "contrary" {
            -1
        } else {
            1
        };
        let mon = self.mon_mut(pos);
        let changed = mon.boosts.add(stat, amount);
        let reason = if changed != 0 {
            mon.stats_raised |= changed > 0;
            format!("|[from] item: {name}")
        } else {
            String::new()
        };
        self.add(format!(
            "|{}|{}|{}|{}{reason}",
            if amount < 0 { "-unboost" } else { "-boost" },
            self.ident(pos),
            stat.id(),
            changed.abs(),
        ));
        self.symbiosis_after_item(pos);
    }

    pub(super) fn symbiosis_after_item(&mut self, recipient: Position) {
        if self.format.game_type.active_slots() == 1
            || self.mon(recipient).hp == 0
            || self.mon(recipient).pivot_move.is_some()
        {
            return;
        }
        let ally = Position {
            slot: 1 - recipient.slot,
            ..recipient
        };
        let mon = self.mon(ally);
        if mon.hp == 0 || self.event_ability(ally) != "symbiosis" {
            return;
        }
        let Some(item) = mon.transferable_item(self.format.dex_mod) else {
            return;
        };
        if !self.mon(recipient).can_transfer_item(item) {
            return;
        }
        let id = self.lose_held_item(ally);
        self.mon_mut(recipient).item = id;
        self.on_gained_item(recipient);
        self.add(format!(
            "|-activate|{}|ability: Symbiosis|{}|[of] {}",
            self.ident(ally),
            item.effect.name,
            self.ident(recipient)
        ));
    }

    pub(super) fn magician_after_hit(&mut self, user: Position, targets: &[Position]) {
        let mon = self.mon(user);
        if mon.ability != "magician" || mon.hp == 0 || !mon.item.is_empty() {
            return;
        }
        let mut ordered: Vec<_> = targets
            .iter()
            .map(|&target| (target, self.mon(target).speed))
            .collect();
        speed_sort_by(&mut ordered, &mut self.rng, |a, b| b.1.cmp(&a.1));
        for (target, _) in ordered {
            if target == user {
                continue;
            }
            let victim = self.mon(target);
            let Some(name) = self.can_take_item(target) else {
                if victim.hp > 0
                    && self.event_ability(target) == "stickyhold"
                    && !victim.item.is_empty()
                {
                    self.add(format!(
                        "|-activate|{}|ability: Sticky Hold",
                        self.ident(target)
                    ));
                }
                continue;
            };
            let target_name = if victim.fainted {
                format!("{}: {}", target.player.id(), victim.name)
            } else {
                self.ident(target)
            };
            let id = self.lose_held_item(target);
            self.mon_mut(user).item = id;
            self.on_gained_item(user);
            self.add(format!(
                "|-item|{}|{name}|[from] ability: Magician|[of] {target_name}",
                self.ident(user)
            ));
            break;
        }
    }

    pub(super) fn swap_items(&mut self, user: Position, target: Position, move_name: &str) -> bool {
        let user_item = DEX.item(self.format.dex_mod, &self.mon(user).item);
        let foe_item = DEX.item(self.format.dex_mod, &self.mon(target).item);
        if (user_item.is_none() && foe_item.is_none())
            || user_item.is_some_and(|item| {
                !self.mon(user).can_transfer_item(item) || !self.mon(target).can_transfer_item(item)
            })
            || foe_item.is_some_and(|item| {
                !self.mon(target).can_transfer_item(item) || !self.mon(user).can_transfer_item(item)
            })
        {
            return false;
        }
        let from_user = self.lose_held_item(user);
        let from_target = self.lose_held_item(target);
        self.add(format!(
            "|-activate|{}|move: Trick|[of] {}",
            self.ident(user),
            self.ident(target)
        ));
        for (pos, id, new, previous) in [
            (target, from_user, user_item, foe_item),
            (user, from_target, foe_item, user_item),
        ] {
            if let Some(item) = new {
                self.mon_mut(pos).item = id;
                self.on_gained_item(pos);
                self.add(format!(
                    "|-item|{}|{}|[from] move: {move_name}",
                    self.ident(pos),
                    item.effect.name
                ));
            } else if let Some(item) = previous {
                self.add(format!(
                    "|-enditem|{}|{}|[silent]|[from] move: {move_name}",
                    self.ident(pos),
                    item.effect.name
                ));
            }
        }
        true
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
        let id = self.lose_held_item(pos);
        // Air Balloon runs AfterUseItem directly without recording lastItem.
        if !id.is_empty() && id != "airballoon" {
            self.mon_mut(pos).last_item = id;
        }
        let reason = if eaten { "|[eat]" } else { "" };
        self.add(format!("|-enditem|{}|{item}{reason}", self.ident(pos)));
    }

    pub(super) fn lose_held_item(&mut self, pos: Position) -> String {
        let mon = self.mon_mut(pos);
        let item = std::mem::take(&mut mon.item);
        if !item.is_empty() && mon.ability == "unburden" {
            mon.track_volatile("unburden");
        }
        item
    }

    pub(super) fn recycle_item(&mut self, pos: Position) -> bool {
        let mon = self.mon(pos);
        if !mon.item.is_empty() {
            return false;
        }
        let Some(item) = DEX.item(self.format.dex_mod, &mon.last_item) else {
            return false;
        };
        let mon = self.mon_mut(pos);
        mon.item = std::mem::take(&mut mon.last_item);
        self.add(format!(
            "|-item|{}|{}|[from] move: Recycle",
            self.ident(pos),
            item.effect.name
        ));
        self.on_gained_item(pos);
        true
    }

    pub(super) fn indirect_damage(&mut self, pos: Position, divisor: u32, cause: &str) {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.ability == "magicguard" {
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

    pub(super) fn item_power_modifier(
        &self,
        move_type: &str,
        category: crate::dex::Category,
    ) -> u32 {
        use crate::dex::Category;
        if matches!(
            (self.item.as_str(), category),
            ("wiseglasses", Category::Special) | ("muscleband", Category::Physical)
        ) {
            4505
        } else if matches!(
            (self.item.as_str(), move_type),
            ("mysticwater", "Water")
                | ("miracleseed", "Grass")
                | ("charcoal", "Fire")
                | ("fairyfeather", "Fairy")
                | ("spelltag", "Ghost")
                | ("softsand", "Ground")
                | ("magnet", "Electric")
                | ("blackbelt", "Fighting")
                | ("blackglasses", "Dark")
                | ("dragonfang", "Dragon")
                | ("hardstone", "Rock")
                | ("metalcoat", "Steel")
                | ("nevermeltice", "Ice")
                | ("poisonbarb", "Poison")
                | ("sharpbeak", "Flying")
                | ("silkscarf", "Normal")
                | ("silverpowder", "Bug")
                | ("twistedspoon", "Psychic")
        ) {
            4915
        } else {
            4096
        }
    }
}
