//! Leech Seed follows the seeder's field slot after switches.

use super::*;

#[derive(Clone, Copy)]
pub(super) enum DrainKind {
    Move,
    LeechSeed,
}

impl Battle {
    pub(super) fn transfer_health(
        &mut self,
        recipient: Position,
        donor: Position,
        amount: u32,
        kind: DrainKind,
    ) {
        if self.mon(recipient).heal_block > 0 {
            return;
        }
        if self.event_ability(donor) == "liquidooze" {
            if self.format.report_exact_hp {
                let effect = match kind {
                    DrainKind::Move => "drain",
                    DrainKind::LeechSeed => "leechseed",
                };
                self.add(format!(
                    "|debug|Heal is occurring: {} <- {} :: {effect}",
                    self.ident(recipient),
                    self.ident(donor)
                ));
            }
            if amount == 0 || self.mon(recipient).hp == 0 || !self.active_effects(recipient) {
                return;
            }
            if self.event_ability(recipient) == "magicguard" {
                self.add(format!(
                    "|-activate|{}|ability: Liquid Ooze",
                    self.ident(donor)
                ));
                if self.format.report_exact_hp {
                    self.add("|debug|damage event failed");
                }
                return;
            }
            let mon = self.mon_mut(recipient);
            let original_hp = mon.hp;
            mon.half_hp_berry_deferred = false;
            mon.damage_hp(amount);
            self.add_health_line_with(
                "-damage",
                recipient,
                false,
                &format!("|[from] ability: Liquid Ooze|[of] {}", self.ident(donor)),
            );
            self.emergency_exit(recipient, original_hp);
            return;
        }
        if amount == 0 || !self.active_effects(recipient) {
            return;
        }
        let mon = self.mon_mut(recipient);
        if mon.hp == 0 || mon.hp >= mon.max_hp.get() {
            return;
        }
        mon.hp = (mon.hp + amount).min(mon.max_hp.get());
        match kind {
            DrainKind::Move => self.add_health_line_with(
                "-heal",
                recipient,
                false,
                &format!("|[from] drain|[of] {}", self.ident(donor)),
            ),
            DrainKind::LeechSeed => {
                self.add_health_line_with("-heal", recipient, false, "|[silent]")
            }
        }
    }

    pub(super) fn leech_seed(&mut self, source: Position, target: Position) -> bool {
        let mon = self.mon_mut(target);
        if mon.seeded_from.is_some() {
            return false;
        }
        mon.seeded_from = Some(source);
        mon.track_volatile("leechseed");
        self.add(format!("|-start|{}|move: Leech Seed", self.ident(target)));
        true
    }

    pub(super) fn tick_leech_seed(&mut self, target: Position) {
        let Some(source) = self.mon(target).seeded_from else {
            return;
        };
        if source.slot >= self.format.game_type.active_slots() || self.mon(source).hp == 0 {
            if self.format.report_exact_hp {
                self.add("|debug|Nothing to leech into");
            }
            return;
        }
        let mon = self.mon_mut(target);
        if mon.ability == "magicguard" {
            return;
        }
        let loss = (mon.max_hp.get() / 8).max(1).min(mon.hp);
        mon.half_hp_berry_deferred = false;
        mon.damage_hp(loss);
        self.add_health_line_with(
            "-damage",
            target,
            false,
            &format!("|[from] Leech Seed|[of] {}", self.ident(source)),
        );
        if loss > 0 {
            self.transfer_health(source, target, loss, DrainKind::LeechSeed);
        }
        if self.mon(target).hp == 0 {
            self.mark_fainted(target);
        }
        if self.mon(source).hp == 0 && !self.mon(source).fainted {
            self.mark_fainted(source);
        }
    }
}
