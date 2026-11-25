//! Perish Song is a field hit with an independently timed knockout per target.

use super::{turn::Position, *};

impl Battle {
    pub(super) fn perish_song(&mut self, source: Position, priority: i8) -> bool {
        let mut resolved = false;
        let mut added = false;
        for target in self.positions() {
            let mon = self.mon(target);
            if mon.fainted {
                continue;
            }
            if mon.commanding
                || (mon.semi_invulnerable()
                    && self.event_ability(source) != "noguard"
                    && self.event_ability(target) != "noguard")
            {
                self.add(format!(
                    "|-miss|{}|{}",
                    self.ident(source),
                    self.ident(target)
                ));
            } else if self.terrain() == Some(Terrain::Psychic)
                && priority > 0
                && source.player != target.player
                && self.grounded(target)
            {
                self.terrain_activate(target, Terrain::Psychic);
            } else if source != target
                && let Some(ability) = match self.event_ability(target) {
                    "soundproof" => Some("Soundproof"),
                    "goodasgold" => Some("Good as Gold"),
                    _ => None,
                }
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: {ability}",
                    self.ident(target)
                ));
            } else if self.mon(target).perish_count == 0 && self.mon(target).hp != 0 {
                let mon = self.mon_mut(target);
                mon.perish_count = 4;
                mon.track_volatile("perishsong");
                self.add(format!("|-start|{}|perish3|[silent]", self.ident(target)));
                added = true;
            } else {
                continue;
            }
            resolved = true;
        }
        if added {
            self.add("|-fieldactivate|move: Perish Song");
        }
        resolved
    }

    pub(super) fn tick_perish_song(&mut self, pos: Position) -> bool {
        let mon = self.mon_mut(pos);
        mon.perish_count -= 1;
        let count = mon.perish_count;
        if count == 0 {
            mon.untrack_volatile("perishsong");
            mon.hp = 0;
        }
        self.add(format!("|-start|{}|perish{count}", self.ident(pos)));
        count == 0
    }
}
