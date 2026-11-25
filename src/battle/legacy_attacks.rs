//! Inherited attacks that Champions National Dex Draft can admit.

use super::{turn::Position, *};

pub(super) fn fixed_damage(move_name: &str, target_hp: u32) -> Option<u32> {
    Some(match move_name {
        "Dragon Rage" => 40,
        "Sonic Boom" => 20,
        "Super Fang" | "Nature's Madness" | "Ruination" => (target_hp / 2).max(1),
        _ => return None,
    })
}

impl Battle {
    pub(super) fn legacy_base_power(
        &mut self,
        target: Position,
        name: &str,
        power: u16,
        pending: bool,
    ) -> u16 {
        let mon = self.mon(target);
        let label = match name {
            "Wake-Up Slap"
                if matches!(mon.status, Status::Sleep(_))
                    || self.event_ability(target) == "comatose" =>
            {
                "BP doubled on sleeping target"
            }
            "Smelling Salts" if mon.status == Status::Paralysis => "BP doubled on paralyzed target",
            "Bolt Beak" | "Fishious Rend" => {
                let doubled = pending || mon.active_turns == 0;
                if self.format.report_exact_hp {
                    self.add(format!(
                        "|debug|{name} {}",
                        if doubled {
                            "damage boost"
                        } else {
                            "NOT boosted"
                        }
                    ));
                }
                return if doubled { power * 2 } else { power };
            }
            "Crush Grip" | "Wring Out" => {
                let (hp, max_hp) = (mon.hp, mon.max_hp.get());
                let ratio = hp * 4096 / max_hp;
                let power = ((120 * (100 * ratio + 2047)) / 4096 / 100).max(1);
                if self.format.report_exact_hp {
                    self.add(format!("|debug|BP for {hp}/{max_hp} HP: {power}"));
                }
                return power as u16;
            }
            _ => return power,
        };
        if self.format.report_exact_hp {
            self.add(format!("|debug|{label}"));
        }
        power * 2
    }

    pub(super) fn legacy_attack_hit(&mut self, target: Position, name: &str) {
        let mon = self.mon(target);
        match (name, mon.status) {
            ("Smelling Salts", Status::Paralysis) | ("Wake-Up Slap", Status::Sleep(_))
                if mon.hp > 0 =>
            {
                self.cure_status(target, "[msg]");
            }
            ("Incinerate", _) => {
                let item = DEX.item(self.format.dex_mod, &mon.item);
                if item.is_some_and(|item| item.is_berry || mon.item == "normalgem")
                    && let Some(label) = self.can_take_item(target)
                {
                    self.lose_held_item(target);
                    self.add(format!(
                        "|-enditem|{}|{label}|[from] move: Incinerate",
                        self.ident(target)
                    ));
                }
            }
            _ => {}
        }
    }
}
