//! Shared damage rolls and ordered final damage modifiers.

use super::{items::resistance_berry, turn::Position, *};
use crate::{
    dex::Category,
    mechanics::{BoostStat, Damage, boosted_stat, chain},
};

#[derive(Clone, Copy)]
enum Hook {
    Fixed(u32, &'static str),
    Ability(Position, u32, &'static str),
    HeldItem(u32),
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
        let source_active = self.active_effects(source);
        let mut hooks = [(0_i32, 0_u8, Hook::Fixed(4096, "")); 11];
        let mut count = 0;
        let mut add = |speed, suborder, hook| {
            hooks[count] = (speed, suborder, hook);
            count += 1;
        };
        if source_active && user.ability == "sniper" {
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
        match if source_active {
            user.item.as_str()
        } else {
            ""
        } {
            "lifeorb" => add(user.speed, 8, Hook::HeldItem(5324)),
            "expertbelt" => add(
                user.speed,
                8,
                Hook::HeldItem(if effectiveness > 0 { 4915 } else { 4096 }),
            ),
            _ => {}
        }
        if screen_handlers > 0 {
            add(0, 4, Hook::Screens);
        }
        if defender.glaive_drawback {
            add(defender.speed, 2, Hook::Fixed(8192, ""));
        }
        if let Some(modifier) = defender
            .semi_invulnerability()
            .and_then(|state| state.damage_modifier(&data.name))
        {
            add(defender.speed, 2, Hook::Fixed(modifier, ""));
        }
        match defender.ability.as_str() {
            "multiscale" => add(
                defender.speed,
                7,
                if defender.hp == defender.max_hp.get() {
                    Hook::Ability(target, 2048, "Multiscale weaken")
                } else {
                    Hook::Ability(target, 4096, "")
                },
            ),
            "auraguard" => add(
                defender.speed,
                7,
                Hook::Ability(
                    target,
                    if data.flags.contains_key("contact") {
                        2048
                    } else {
                        4096
                    },
                    "",
                ),
            ),
            id @ ("filter" | "solidrock" | "prismarmor") => {
                let debug = match id {
                    "filter" => "Filter neutralize",
                    "solidrock" => "Solid Rock neutralize",
                    _ => "Prism Armor neutralize",
                };
                add(
                    defender.speed,
                    7,
                    if effectiveness > 0 {
                        Hook::Ability(target, 3072, debug)
                    } else {
                        Hook::Ability(target, 4096, "")
                    },
                );
            }
            _ => {}
        }
        for holder in self.positions() {
            let mon = self.mon(holder);
            if !mon.fainted && mon.ability == "friendguard" {
                let (factor, debug) = if holder.player == target.player && holder != target {
                    (3072, "Friend Guard weaken")
                } else {
                    (4096, "")
                };
                add(mon.speed, 7, Hook::Ability(holder, factor, debug));
            }
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
        let mut ripened = false;
        for &(_, _, hook) in hooks.iter() {
            let (next, debug) = match hook {
                Hook::Fixed(value, debug) => (value, debug),
                Hook::HeldItem(value) => {
                    if self.debug_ignored_item(source, "ModifyDamage") {
                        continue;
                    }
                    (value, "")
                }
                Hook::Ability(holder, value, debug) => {
                    if self.ability_suppressed(holder) {
                        if self.format.report_exact_hp {
                            self.add("|debug|ModifyDamage handler suppressed by Mold Breaker");
                        }
                        continue;
                    }
                    (value, debug)
                }
                Hook::Berry(..) if self.debug_ignored_item(target, "ModifyDamage") => continue,
                Hook::Berry(name, true) if !self.foe_unnerve(target) => {
                    ripened = self.event_ability(target) == "ripen";
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
        if ripened {
            // Ripen's damage handler runs after the berry and other
            // priority-zero damage modifiers.
            modifier = chain(modifier, 2048);
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
    pub sheer_force_removed: bool,
    pub analytic_last: bool,
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
            sheer_force_removed,
            analytic_last,
            roll,
        } = args;
        let tr = self.stat_rules.truncation;
        if solar_weak && self.format.report_exact_hp {
            self.add("|debug|weakened by weather");
        }
        if expanded && self.format.report_exact_hp {
            self.add("|debug|terrain buff");
        }
        if analytic_last && self.format.report_exact_hp {
            self.add("|debug|Analytic boost");
        }
        let source_active = self.active_effects(source);
        let rivalry = if source_active && self.mon(source).ability == "rivalry" {
            match (self.mon(source).gender, self.mon(target).gender) {
                (b'N', _) | (_, b'N') => 4096,
                (a, b) if a == b => 5120,
                _ => 3072,
            }
        } else {
            4096
        };
        if rivalry != 4096 && self.format.report_exact_hp {
            self.add(if rivalry > 4096 {
                "|debug|Rivalry boost"
            } else {
                "|debug|Rivalry weaken"
            });
        }
        let helping = self.mon(source).helping_hand;
        let helping = (helping != 0 && source_active).then(|| 1.5_f64.powi(i32::from(helping)));
        if let Some(multiplier) = helping
            && self.format.report_exact_hp
        {
            self.add(format!("|debug|Boosting from Helping Hand: {multiplier}"));
        }
        let electric_charge = source_active
            && move_type == "Electric"
            && self.mon(source).volatile_ids().contains(&"charge");
        if electric_charge && self.format.report_exact_hp {
            self.add("|debug|charge boost");
        }
        let terrain_power = self.terrain_power_modifier(source, target, move_data, move_type);
        let lash_out = move_data.name == "Lash Out" && self.mon(source).stats_lowered;
        if lash_out && self.format.report_exact_hp {
            self.add("|debug|lashout buff");
        }
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
        if move_type == "Fire"
            && source_active
            && self.mon(source).ability == "firemane"
            && self.format.report_exact_hp
        {
            self.add("|debug|Fire Mane boost");
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
        let stakeout = source_active
            && self.mon(source).ability == "stakeout"
            && self.mon(target).active_turns == 0;
        if stakeout && self.format.report_exact_hp {
            self.add("|debug|Stakeout boost");
        }
        let fallen = self.mon(source).fallen_allies;
        let supreme_overlord = [4096, 4506, 4915, 5325, 5734, 6144][usize::from(fallen)];
        if fallen > 0 && self.format.report_exact_hp {
            self.add(format!(
                "|debug|Supreme Overlord boost: {supreme_overlord}/4096"
            ));
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
        let mut atk = boosted_stat(atk, if crit { atk_stage.max(0) } else { atk_stage });
        if move_data.category == Category::Physical && self.event_ability(source) == "hustle" {
            // Hustle directly modifies the stat before chained item effects.
            atk = tr.modify(atk, 6144);
        }
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
        let mut attack_modifier = if source_active {
            user.attack_modifier(move_data.category, move_type)
        } else {
            4096
        };
        if move_data.category == Category::Special && self.plus_minus_boost(source) {
            attack_modifier = chain(6144, attack_modifier);
        }
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
        let mut power_modifier = if source_active {
            chain(
                chain(
                    supreme_overlord,
                    user.ability_power_modifier(move_data, move_type, power, weather),
                ),
                user.item_power_modifier(move_type, move_data.category),
            )
        } else {
            4096
        };
        if solar_weak {
            power_modifier = crate::mechanics::chain(2048, power_modifier);
        }
        if expanded {
            power_modifier = crate::mechanics::chain(6144, power_modifier);
        }
        if rivalry != 4096 {
            power_modifier = chain(rivalry, power_modifier);
        }
        if analytic_last
            || ((sheer_force_removed || move_data.has_sheer_force_boost)
                && self.event_ability(source) == "sheerforce")
        {
            power_modifier = chain(5325, power_modifier);
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
        if let Some(multiplier) = helping {
            power_modifier = chain(power_modifier, (multiplier * 4096.0) as u32);
        }
        if electric_charge {
            power_modifier = chain(power_modifier, 8192);
        }
        power_modifier = crate::mechanics::chain(power_modifier, terrain_power);
        if lash_out {
            power_modifier = chain(power_modifier, 8192);
        }
        if mon
            .semi_invulnerability()
            .is_some_and(|state| state.doubles_power(&move_data.name))
        {
            power_modifier = chain(power_modifier, 8192);
        }
        if move_data.name == "Facade" && !matches!(user.status, Status::Healthy | Status::Sleep(_))
        {
            power_modifier = chain(power_modifier, 8192);
        }
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
            } else if source_active && user.ability == "adaptability" {
                8192
            } else {
                6144
            },
            effectiveness: effect,
            burned: user.status == Status::Burn
                && move_data.name != "Facade"
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
