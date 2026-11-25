//! Substitute HP is independent of its owner's health and damage hooks.

use super::turn::Position;
use super::*;

impl Battle {
    pub(super) fn behind_substitute(
        &self,
        source: Position,
        target: Position,
        data: &Move,
    ) -> bool {
        source != target
            && self.mon(target).substitute_hp != 0
            && !data.flags.contains_key("bypasssub")
            && self.event_ability(source) != "infiltrator"
    }

    pub(super) fn make_substitute(&mut self, pos: Position, shed_tail: bool) -> bool {
        let mon = self.mon(pos);
        let max_hp = mon.max_hp.get();
        let cost = if shed_tail {
            max_hp.div_ceil(2)
        } else {
            max_hp / 4
        };
        let name = if shed_tail { "Shed Tail" } else { "Substitute" };
        let ident = self.ident(pos);
        if mon.substitute_hp != 0 || mon.hp <= cost || max_hp == 1 {
            let weak = if mon.substitute_hp == 0 {
                "|[weak]"
            } else {
                ""
            };
            self.add(format!("|-fail|{ident}|move: {name}{weak}"));
            return false;
        }
        let reason = if shed_tail {
            "|[from] move: Shed Tail"
        } else {
            ""
        };
        self.add(format!("|-start|{ident}|Substitute{reason}"));
        let mon = self.mon_mut(pos);
        mon.substitute_hp = max_hp / 4;
        mon.track_volatile("substitute");
        self.end_partial_trap(pos, true);
        self.mon_mut(pos).hp -= cost;
        self.add_health_line_with("-damage", pos, false, "");
        true
    }

    pub(super) fn end_substitute(&mut self, pos: Position) -> bool {
        let mon = self.mon_mut(pos);
        if std::mem::take(&mut mon.substitute_hp) == 0 {
            return false;
        }
        mon.untrack_volatile("substitute");
        self.add(format!("|-end|{}|Substitute", self.ident(pos)));
        true
    }

    pub(super) fn strike_substitute(
        &mut self,
        source: Position,
        target: Position,
        move_data: &Move,
        damage: u32,
        ohko: bool,
    ) {
        let mon = self.mon_mut(target);
        let dealt = damage.min(mon.substitute_hp);
        mon.substitute_hp -= dealt;
        let intact = mon.substitute_hp > 0;
        if !intact {
            mon.untrack_volatile("substitute");
        }
        let ident = self.ident(target);
        if intact {
            self.add(format!("|-activate|{ident}|move: Substitute|[damage]"));
        } else {
            if ohko {
                self.add("|-ohko");
            }
            self.add(format!("|-end|{ident}|Substitute"));
        }
        if dealt > 0 && self.mon(source).hp > 0 {
            let original_hp = self.mon(source).hp;
            if move_data.name == "Struggle" {
                let mon = self.mon_mut(source);
                mon.hp = mon.hp.saturating_sub((mon.max_hp.get() + 2) / 4);
                self.add_health_line_with("-damage", source, false, "|[from] recoil");
            } else if matches!(move_data.name.as_str(), "Mind Blown" | "Steel Beam") {
                self.half_max_hp_recoil(source, &to_id(&move_data.name));
            } else if let Some([num, den]) = move_data.recoil
                && !matches!(self.mon(source).ability.as_str(), "rockhead" | "magicguard")
            {
                let recoil = (dealt * u32::from(num) + u32::from(den) / 2) / u32::from(den);
                let mon = self.mon_mut(source);
                mon.hp = mon.hp.saturating_sub(recoil.max(1));
                self.add_health_line_with("-damage", source, false, "|[from] Recoil");
            }
            self.emergency_exit(source, original_hp);
        }
        if let Some([num, den]) = move_data.drain {
            let restored = (dealt * u32::from(num)).div_ceil(u32::from(den));
            let mon = self.mon_mut(source);
            if restored > 0 && mon.hp > 0 && mon.hp < mon.max_hp.get() {
                mon.hp = (mon.hp + restored).min(mon.max_hp.get());
                self.add_health_line_with(
                    "-heal",
                    source,
                    false,
                    &format!("|[from] drain|[of] {ident}"),
                );
            }
        }
        if self.mon(source).hp > 0 {
            match move_data.name.as_str() {
                "Rapid Spin" | "Mortal Spin" => {
                    self.clear_spinning_hazards(source, &move_data.name)
                }
                "Ice Spinner" => {
                    self.clear_terrain();
                }
                name => {
                    if let Some(hazard) = Hazard::from_attack(name) {
                        self.add_hazard(source.player.foe(), hazard);
                    }
                }
            }
        }
        if self.mon(target).item == "airballoon" {
            if self.format.report_exact_hp {
                self.add(format!("|debug|effect: {}", to_id(&move_data.name)));
            }
            self.consume(target, "Air Balloon", false);
        }
    }
}
