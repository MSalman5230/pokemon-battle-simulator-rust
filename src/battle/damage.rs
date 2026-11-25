//! Shared damage rolls and ordered final damage modifiers.

use super::{items::resistance_berry, turn::Position, *};
use crate::{
    dex::Category,
    mechanics::{BoostStat, Damage, boosted_stat, chain},
};

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
            let matches = berry_type == typing
                && (effectiveness > 0 || typing == "Normal")
                && !self.behind_substitute(source, target, data);
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

pub(super) struct RegularHit<'a> {
    pub power: u16,
    pub move_type: &'a str,
    pub weather: Option<Weather>,
    pub solar_weak: bool,
    pub expanded: bool,
    pub converted: bool,
    pub hit: u8,
    pub spread: bool,
    pub effect: i8,
    pub crit: bool,
    pub screen_handlers: u32,
    /// When provided by a hypothetical query, use this native 0–15 roll
    /// instead of consuming the battle PRNG.
    pub roll: Option<u32>,
}

impl Battle {
    pub(super) fn regular_hit(
        &mut self,
        source: Position,
        target: Position,
        move_data: &'static Move,
        args: RegularHit<'_>,
    ) -> u32 {
        let RegularHit {
            power,
            move_type,
            weather,
            solar_weak,
            expanded,
            converted,
            hit,
            spread,
            effect,
            crit,
            screen_handlers,
            roll,
        } = args;
        let tr = self.stat_rules.truncation;
        if solar_weak && self.format.report_exact_hp {
            self.add("|debug|weakened by weather");
        }
        if expanded && self.format.report_exact_hp {
            self.add("|debug|terrain buff");
        }
        let terrain_power = self.terrain_power_modifier(source, target, move_data, move_type);
        let dry_skin = move_type == "Fire" && self.event_ability(target) == "dryskin";
        let misty_explosion = move_data.name == "Misty Explosion"
            && self.terrain() == Some(Terrain::Misty)
            && self.grounded(source);
        if misty_explosion && self.format.report_exact_hp {
            self.add("|debug|misty terrain boost");
        }
        let purifying_salt = move_type == "Ghost" && self.event_ability(target) == "purifyingsalt";
        let flash_fire = move_type == "Fire"
            && self.mon(source).ability == "flashfire"
            && self.mon(source).volatile_ids().contains(&"flashfire");
        if flash_fire && self.format.report_exact_hp {
            self.add("|debug|Flash Fire boost");
        }
        if purifying_salt && self.format.report_exact_hp {
            self.add("|debug|Purifying Salt weaken");
        }
        if self.mon(source).ability == "reckless"
            && self.format.report_exact_hp
            && (move_data.recoil.is_some() || super::abilities::crash_move(&move_data.name))
        {
            self.add("|debug|Reckless boost");
        }
        let stakeout = self.mon(source).ability == "stakeout" && self.mon(target).active_turns == 0;
        if stakeout && self.format.report_exact_hp {
            self.add("|debug|Stakeout boost");
        }
        if move_data.ignore_defensive && self.format.report_exact_hp {
            self.add("|debug|Negating (sp)def boost/penalty.");
        }
        self.sort_unaware_handlers();
        self.sort_unaware_handlers();
        let user = self.mon(source);
        let mon = self.mon(target);
        let offensive = if move_data.name == "Foul Play" {
            mon
        } else {
            user
        };
        let (atk, mut atk_stage, def, mut def_stage) = if move_data.category == Category::Physical {
            (
                if move_data.name == "Body Press" {
                    user.stats.def
                } else {
                    offensive.stats.atk
                },
                offensive.boosts.get(if move_data.name == "Body Press" {
                    BoostStat::Def
                } else {
                    BoostStat::Atk
                }),
                mon.stats.def,
                mon.boosts.get(BoostStat::Def),
            )
        } else {
            (
                user.stats.spa,
                user.boosts.get(BoostStat::Spa),
                mon.stats.spd,
                mon.boosts.get(BoostStat::Spd),
            )
        };
        if source != target && self.event_ability(target) == "unaware" {
            atk_stage = 0;
        }
        if move_data.ignore_defensive || source != target && self.event_ability(source) == "unaware"
        {
            def_stage = 0;
        }
        let atk = boosted_stat(atk, if crit { atk_stage.max(0) } else { atk_stage });
        let def = boosted_stat(def, if crit { def_stage.min(0) } else { def_stage });
        let damage_weather = self.effective_weather();
        let def = if (damage_weather == Some(Weather::Sand)
            && move_data.category == Category::Special
            && mon.has_type("Rock"))
            || (damage_weather == Some(Weather::Snow)
                && move_data.category == Category::Physical
                && mon.has_type("Ice"))
        {
            tr.modify(def, 6144)
        } else {
            def
        };
        let mut attack_modifier = user.attack_modifier(move_data.category, move_type);
        let def = if move_data.category == Category::Physical
            && self.event_ability(target) == "furcoat"
        {
            tr.modify(def, 8192)
        } else {
            def
        };
        if stakeout {
            attack_modifier = crate::mechanics::chain(8192, attack_modifier);
        }
        if flash_fire {
            attack_modifier = crate::mechanics::chain(6144, attack_modifier);
        }
        if (matches!(self.event_ability(target), "heatproof" | "waterbubble")
            && move_type == "Fire")
            || (self.event_ability(target) == "thickfat" && matches!(move_type, "Fire" | "Ice"))
            || purifying_salt
        {
            attack_modifier = crate::mechanics::chain(attack_modifier, 2048);
        }
        let atk = tr.modify(atk, attack_modifier);
        let stab = user.has_type(move_type);
        let mut power_modifier = crate::mechanics::chain(
            user.ability_power_modifier(move_data, move_type, power, weather),
            user.item_power_modifier(move_type, move_data.category),
        );
        if solar_weak {
            power_modifier = crate::mechanics::chain(2048, power_modifier);
        }
        if expanded {
            power_modifier = crate::mechanics::chain(6144, power_modifier);
        }
        if converted && (hit == 1 || user.converted_move_type(&move_data.move_type).1) {
            power_modifier = crate::mechanics::chain(4915, power_modifier);
        }
        if dry_skin {
            power_modifier = crate::mechanics::chain(5120, power_modifier);
        }
        if misty_explosion {
            power_modifier = crate::mechanics::chain(6144, power_modifier);
        }
        if move_data.name == "Knock Off" && mon.transferable_item(self.format.dex_mod).is_some() {
            power_modifier = crate::mechanics::chain(power_modifier, 6144);
        }
        power_modifier = crate::mechanics::chain(power_modifier, terrain_power);
        let mut hit_damage = Damage {
            level: user.level,
            power: tr.modify(u32::from(power), power_modifier),
            attack: atk,
            defense: NonZeroU32::new(def.max(1)).unwrap(),
            spread_or_bond: if spread { 3072 } else { 4096 },
            weather: damage_weather.map_or(4096, |w| w.damage_modifier(move_type)),
            critical: crit,
            stab: if !stab {
                4096
            } else if user.ability == "adaptability" {
                8192
            } else {
                6144
            },
            effectiveness: effect,
            burned: user.status == Status::Burn
                && user.ability != "guts"
                && move_data.category == Category::Physical,
            final_modifier: 4096,
            bypassed_protect: false,
        };
        let randomness = roll.unwrap_or_else(|| self.rng.below(16));
        if spread && self.format.report_exact_hp {
            self.add("|debug|Spread modifier: 0.75");
        }
        if effect > 0 {
            self.add(format!(
                "|-supereffective|{}|{}",
                self.ident(target),
                effect.min(2)
            ));
        } else if effect < 0 {
            self.add(format!(
                "|-resisted|{}|{}",
                self.ident(target),
                (-effect).min(2)
            ));
        }
        if crit {
            self.add(format!("|-crit|{}", self.ident(target)));
        }
        hit_damage.final_modifier = self.final_damage_modifier(
            source,
            target,
            move_data,
            (move_type, effect, crit),
            screen_handlers,
        );
        hit_damage.roll_with_randomness(tr, randomness)
    }
}
