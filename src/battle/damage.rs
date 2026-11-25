//! Ordered handlers for the final damage modifier event.

use super::{items::resistance_berry, turn::Position, *};
use crate::mechanics::chain;

#[derive(Clone, Copy)]
enum Hook {
    Fixed(u32, &'static str),
    DefenderAbility(u32, &'static str),
    Berry(&'static str, bool),
    Screens,
}

impl Battle {
    pub(super) fn final_damage_modifier(
        &mut self,
        source: Position,
        target: Position,
        data: &Move,
        (typing, effectiveness, crit): (&str, i8, bool),
        screen_handlers: u32,
    ) -> u32 {
        let user = self.mon(source);
        let defender = self.mon(target);
        let mut hooks = [(0_i32, 0_u8, Hook::Fixed(4096, "")); 7];
        let mut count = 0;
        let mut add = |speed, suborder, hook| {
            hooks[count] = (speed, suborder, hook);
            count += 1;
        };
        if user.ability == "sniper" {
            add(
                user.speed,
                7,
                if crit {
                    Hook::Fixed(6144, "Sniper boost")
                } else {
                    Hook::Fixed(4096, "")
                },
            );
        }
        match user.item.as_str() {
            "lifeorb" => add(user.speed, 8, Hook::Fixed(5324, "")),
            "expertbelt" => add(
                user.speed,
                8,
                Hook::Fixed(if effectiveness > 0 { 4915 } else { 4096 }, ""),
            ),
            _ => {}
        }
        if screen_handlers > 0 {
            add(0, 4, Hook::Screens);
        }
        if defender.glaive_drawback {
            add(defender.speed, 2, Hook::Fixed(8192, ""));
        }
        if let Some(state) = defender.semi_invulnerability() {
            let modifier = if state.exposed_to(&data.name) {
                8192
            } else {
                4096
            };
            add(defender.speed, 2, Hook::Fixed(modifier, ""));
        }
        match defender.ability.as_str() {
            "multiscale" => add(
                defender.speed,
                7,
                if defender.hp == defender.max_hp.get() {
                    Hook::DefenderAbility(2048, "Multiscale weaken")
                } else {
                    Hook::DefenderAbility(4096, "")
                },
            ),
            "auraguard" => add(
                defender.speed,
                7,
                Hook::DefenderAbility(
                    if data.flags.contains_key("contact") {
                        2048
                    } else {
                        4096
                    },
                    "",
                ),
            ),
            _ => {}
        }
        if let Some((berry_type, name)) = resistance_berry(&defender.item) {
            let matches = berry_type == typing && (effectiveness > 0 || typing == "Normal");
            add(defender.speed, 8, Hook::Berry(name, matches));
        }
        let hooks = &mut hooks[..count];
        speed_sort_by(hooks, &mut self.rng, |a, b| {
            b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1))
        });
        let mut modifier = 4096;
        for &(_, _, hook) in hooks.iter() {
            let (next, debug) = match hook {
                Hook::Fixed(value, debug) => (value, debug),
                Hook::DefenderAbility(value, debug) => {
                    if self.ability_suppressed(target) {
                        if self.format.report_exact_hp {
                            self.add("|debug|ModifyDamage handler suppressed by Mold Breaker");
                        }
                        continue;
                    }
                    (value, debug)
                }
                Hook::Berry(name, true) if !self.foe_unnerve(target) => {
                    self.consume(target, name, true);
                    self.symbiosis_after_item(target);
                    if self.format.report_exact_hp {
                        self.add("|debug|-50% reduction");
                    }
                    self.add(format!("|-enditem|{}|{name}|[weaken]", self.ident(target)));
                    (2048, "")
                }
                Hook::Berry(..) => continue,
                Hook::Screens => {
                    // Each screen registers even when on the wrong side or category.
                    for choices in (2..=screen_handlers).rev() {
                        self.rng.below(choices);
                    }
                    self.screens_damage_modifier(source, target, data.category, crit)
                }
            };
            if !debug.is_empty() && self.format.report_exact_hp {
                self.add(format!("|debug|{debug}"));
            }
            modifier = chain(modifier, next);
        }
        modifier
    }
}
