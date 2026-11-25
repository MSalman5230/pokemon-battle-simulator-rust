//! Moves that change stored stats or directly exchange stat stages.

use super::{tactical::BOOSTS, turn::Position, *};
use crate::mechanics::{BoostStat, Stat};

impl Pokemon {
    pub(super) fn request_stats(&self) -> Stats {
        self.stats_before_change
            .as_deref()
            .copied()
            .unwrap_or_else(|| {
                self.transformed
                    .as_ref()
                    .map_or(self.stats, |form| form.original_stats)
            })
    }

    fn store_stat_change(&mut self, stats: Stats) {
        if self.stats == stats {
            return;
        }
        if self.transformed.is_none() && self.stats_before_change.is_none() {
            self.stats_before_change = Some(Box::new(self.stats));
        }
        self.stats = stats;
    }
}

impl Battle {
    pub(super) fn apply_stat_move(
        &mut self,
        source: Position,
        target: Position,
        name: &str,
    ) -> bool {
        use BoostStat::{Atk, Def, Spa, Spd};
        let swapped_stages = match name {
            "Power Swap" => Some((&[Atk, Spa][..], "atk, spa|")),
            "Guard Swap" => Some((&[Def, Spd][..], "def, spd|")),
            "Heart Swap" => Some((&BOOSTS[..], "")),
            _ => None,
        };
        let user = self.mon(source);
        let foe = self.mon(target);
        if let Some((stages, label)) = swapped_stages {
            let user_stages = user.boosts;
            let foe_stages = foe.boosts;
            for &stat in stages {
                let difference = foe_stages.get(stat) - user_stages.get(stat);
                self.mon_mut(source).boosts.add(stat, difference);
                self.mon_mut(target).boosts.add(stat, -difference);
            }
            self.add(format!(
                "|-swapboost|{}|{}|{label}[from] move: {name}",
                self.ident(source),
                self.ident(target)
            ));
            return true;
        }
        let (stats, speed_swap): (&[Stat], bool) = match name {
            "Power Split" => (&[Stat::Atk, Stat::Spa], false),
            "Guard Split" => (&[Stat::Def, Stat::Spd], false),
            "Speed Swap" => (&[Stat::Spe], true),
            _ => return false,
        };
        let (user, foe) = (user.stats, foe.stats);
        for (holder, other) in [(source, (user, foe)), (target, (foe, user))] {
            let (before, adversary) = other;
            let changed = Stats::map(|stat| {
                if !stats.contains(&stat) {
                    before.get(stat)
                } else if speed_swap {
                    adversary.get(stat)
                } else {
                    (before.get(stat) + adversary.get(stat)) / 2
                }
            });
            self.mon_mut(holder).store_stat_change(changed);
        }
        self.add(format!(
            "|-activate|{}|move: {name}|[of] {}",
            self.ident(source),
            self.ident(target)
        ));
        true
    }
}
