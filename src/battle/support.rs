//! Slot- and side-owned support effects.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct Wish {
    pub(super) turn: i64,
    pub(super) hp: u32,
    pub(super) wisher: usize,
}

#[derive(Clone, Copy)]
pub(super) struct FutureMove {
    pub(super) ending_turn: u32,
    pub(super) source_side: Player,
    pub(super) roster_index: usize,
    pub(super) data: &'static Move,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacementHeal {
    HealingWish,
    LunarDance,
}

impl ReplacementHeal {
    pub(super) fn from_move(name: &str) -> Option<Self> {
        match name {
            "Healing Wish" => Some(Self::HealingWish),
            "Lunar Dance" => Some(Self::LunarDance),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::HealingWish => "Healing Wish",
            Self::LunarDance => "Lunar Dance",
        }
    }
}

impl Battle {
    pub(super) fn aqua_ring_heal(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.hp > 0 && mon.hp < mon.max_hp.get() && mon.heal_block == 0 {
            mon.hp = (mon.hp + (mon.max_hp.get() / 16).max(1)).min(mon.max_hp.get());
            self.add_health_line_with("-heal", pos, false, "|[from] Aqua Ring");
        }
    }

    pub(super) fn heal_pulse(&mut self, source: Position, target: Position) {
        let max_hp = self.mon(target).max_hp.get();
        let amount = if self.event_ability(source) == "megalauncher" {
            self.stat_rules.truncation.modify(max_hp, 3072).max(1)
        } else {
            max_hp.div_ceil(2)
        };
        let mon = self.mon_mut(target);
        if mon.hp == max_hp || mon.heal_block > 0 {
            self.add(format!("|-fail|{}|heal", self.ident(target)));
        } else {
            mon.hp = (mon.hp + amount).min(max_hp);
            self.add_health_line_with("-heal", target, false, "");
        }
    }

    pub(super) fn prepare_replacement_heal(&mut self, pos: Position, effect: ReplacementHeal) {
        let pending = &mut self.sides[pos.player.index()]
            .as_mut()
            .unwrap()
            .replacement_heals[pos.slot];
        if !pending.contains(&effect) {
            pending.push(effect);
        }
    }

    pub(super) fn apply_replacement_heals(&mut self, pos: Position) {
        let pending = &mut self.sides[pos.player.index()]
            .as_mut()
            .unwrap()
            .replacement_heals[pos.slot];
        if pending.is_empty() {
            return;
        }
        let mut pending = std::mem::take(pending);
        pending.retain(|&effect| {
            let mon = self.mon_mut(pos);
            if mon.fainted
                || mon.hp == 0
                || (mon.hp == mon.max_hp.get()
                    && mon.status == Status::Healthy
                    && (effect == ReplacementHeal::HealingWish
                        || mon.moves.iter().all(|slot| slot.pp == slot.max_pp)))
            {
                return true;
            }
            mon.hp = mon.max_hp.get();
            mon.status = Status::Healthy;
            mon.slept_by_foe = false;
            if effect == ReplacementHeal::LunarDance {
                for slot in &mut mon.moves {
                    slot.pp = slot.max_pp;
                }
            }
            self.add_health_line_with(
                "-heal",
                pos,
                false,
                &format!("|[from] move: {}", effect.name()),
            );
            false
        });
        self.sides[pos.player.index()]
            .as_mut()
            .unwrap()
            .replacement_heals[pos.slot] = pending;
    }

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

    pub(super) fn overflowed_turn(&self) -> u32 {
        let previous = self.turn - 1;
        match self.stat_rules.truncation {
            crate::mechanics::Truncation::Cartridge => previous & 255,
            crate::mechanics::Truncation::Unbounded => previous,
        }
    }

    pub(super) fn schedule_future_move(
        &mut self,
        source: Position,
        target: Position,
        data: &'static Move,
    ) -> bool {
        let side = self.sides[target.player.index()].as_ref().unwrap();
        if let Some(previous) = side.future_moves[target.slot] {
            if previous.ending_turn >= 256 {
                self.add(
                    "|-hint|A previous future attack is stuck on this slot and will never resolve.",
                );
            }
            return false;
        }
        let future = FutureMove {
            ending_turn: self.turn + 1,
            source_side: source.player,
            roster_index: self.mon(source).roster_index,
            data,
        };
        self.sides[target.player.index()]
            .as_mut()
            .unwrap()
            .future_moves[target.slot] = Some(future);
        if future.ending_turn >= 256 {
            self.add("|-hint|In Gen 8+, Future attacks will never resolve when used on the 255th turn or later.");
        }
        let label = if data.name == "Doom Desire" {
            "Doom Desire"
        } else {
            "move: Future Sight"
        };
        self.add(format!("|-start|{}|{label}", self.ident(source)));
        true
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
            turn: i64::from(self.overflowed_turn()),
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
        let turn = i64::from(self.overflowed_turn());
        let side = self.sides[pos.player.index()].as_mut().unwrap();
        let Some(wish) = side.wishes[pos.slot].filter(|wish| turn > wish.turn) else {
            return;
        };
        side.wishes[pos.slot] = None;
        let mon = &mut side.party[pos.slot];
        if wish.hp == 0 || mon.hp == 0 || mon.hp == mon.max_hp.get() || mon.heal_block > 0 {
            return;
        }
        mon.hp = mon.hp.saturating_add(wish.hp).min(mon.max_hp.get());
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
