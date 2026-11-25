//! A hit sequence chooses its length up front but resolves each strike against
//! the current battle state. Items and abilities can change between strikes.

use super::{turn::Position, *};
use crate::{
    dex::{Accuracy, HitCount},
    mechanics::BoostStat,
};

impl Battle {
    pub(super) fn hits(&mut self, source: Position, data: &Move) -> (u8, bool) {
        let user = self.mon(source);
        let skill_link = user.active_ability() == "skilllink";
        let loaded_dice = user.effective_item() == "loadeddice";
        let mut count = match data.multihit {
            None => 1,
            Some(HitCount::Fixed(count)) => count,
            Some(HitCount::Range([_, high])) if skill_link => high,
            Some(HitCount::Range([2, 5])) => {
                let roll = self.rng.below(20);
                match roll {
                    0..=13 if loaded_dice => 5 - self.rng.below(2) as u8,
                    0..=6 => 2,
                    7..=13 => 3,
                    14..=16 => 4,
                    _ => 5,
                }
            }
            Some(HitCount::Range([low, high])) => {
                low + self.rng.below(u32::from(high - low + 1)) as u8
            }
        };
        if count == 10 && loaded_dice {
            count -= self.rng.below(7) as u8;
        }
        (count, data.multi_accuracy && !skill_link && !loaded_dice)
    }

    pub(super) fn next_strike_hits(
        &mut self,
        source: Position,
        target: Position,
        data: &Move,
        accuracy: Accuracy,
        zoom: bool,
    ) -> bool {
        let Accuracy::Percent(base) = accuracy else {
            return true;
        };
        self.sort_unaware_handlers();
        let ignores_evasion = self.move_ignores_evasion(source, data);
        if !ignores_evasion {
            self.sort_unaware_handlers();
        }
        let user = self.mon(source);
        let mon = self.mon(target);
        let always_hits = data.always_hit
            || user.active_ability() == "noguard"
            || mon.active_ability() == "noguard"
            || mon.glaive_drawback
            || self.locked_on_to(source, target)
            || mon.trampled_by(data);
        let stage_factor = |stage: i8| {
            if stage > 0 {
                (3 + i32::from(stage), 3)
            } else {
                (3, 3 - i32::from(stage))
            }
        };
        let (an, ad) = stage_factor(
            if source != target && self.event_ability(target) == "unaware" {
                0
            } else {
                user.boosts.get(BoostStat::Accuracy)
            },
        );
        let (en, ed) = stage_factor(
            if ignores_evasion || (source != target && self.event_ability(source) == "unaware") {
                0
            } else {
                mon.boosts.get(BoostStat::Evasion)
            },
        );
        let numerator = i32::from(base) * an * ed;
        let denominator = ad * en;
        let mut accuracy = numerator as f64 / denominator as f64;
        self.prepare_accuracy_modifiers(source, target, true, zoom);
        if always_hits {
            return true;
        }
        let (weather_evasion, _) = self.active_weather_evasion(target);
        let modifier = self.accuracy_modifier(source, target, weather_evasion, data.category, zoom);
        // The native event applies chained integer modifiers only when its
        // post-stage relay is an integer.
        if modifier != 4096 && accuracy.fract() == 0.0 {
            accuracy = f64::from(self.stat_rules.truncation.modify(accuracy as u32, modifier));
        }
        f64::from(self.rng.below(100)) < accuracy
    }
}
