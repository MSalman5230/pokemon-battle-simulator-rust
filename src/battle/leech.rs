//! Leech Seed follows the seeder's field slot after switches.

use super::*;

impl Battle {
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
        mon.hp -= loss;
        self.add_health_line_with(
            "-damage",
            target,
            false,
            &format!("|[from] Leech Seed|[of] {}", self.ident(source)),
        );
        let mon = self.mon_mut(source);
        if loss > 0 && mon.hp > 0 && mon.hp < mon.max_hp.get() {
            mon.hp = (mon.hp + loss).min(mon.max_hp.get());
            self.add_health_line_with("-heal", source, false, "|[silent]");
        }
        if self.mon(target).hp == 0 {
            self.mark_fainted(target);
        }
    }
}
