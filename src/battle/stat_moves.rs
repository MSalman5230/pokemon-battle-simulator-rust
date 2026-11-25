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

    pub(super) fn swap_attack_defense(&mut self) {
        let mut stats = self.stats;
        std::mem::swap(&mut stats.atk, &mut stats.def);
        self.store_stat_change(stats);
    }
}

impl Battle {
    pub(super) fn steal_positive_stages(
        &mut self,
        source: Position,
        target: Position,
        move_log: usize,
    ) {
        let defender = self.mon(target);
        let mut stages = [(BoostStat::Atk, 0); 7];
        let mut count = 0;
        for stat in BoostStat::ALL {
            let stage = defender.boosts.get(stat);
            if stage > 0 {
                stages[count] = (stat, stage);
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        let stages = &stages[..count];
        let source_name = self.ident(source);
        let target_name = self.ident(target);
        self.move_still(move_log);
        self.add(format!(
            "|-clearpositiveboost|{target_name}|{source_name}|move: Spectral Thief"
        ));
        self.boost(source, stages, true);
        for &(stat, amount) in stages {
            self.mon_mut(target).boosts.add(stat, -amount);
        }
        self.add(format!("|-anim|{source_name}|Spectral Thief|{target_name}"));
    }

    pub(super) fn copy_critical_conditions(&mut self, source: Position, target: Position) {
        let foe = self.mon(target);
        let focus = foe.volatile_ids().contains(&"focusenergy");
        let cheer = foe.dragon_cheer;
        let laser = foe.laser_focus != 0;
        self.end_laser_focus(source);
        let mon = self.mon_mut(source);
        mon.untrack_volatile("focusenergy");
        mon.untrack_volatile("dragoncheer");
        mon.dragon_cheer = cheer;
        let name = if cheer != 0 {
            mon.track_volatile("dragoncheer");
            Some("Dragon Cheer")
        } else if focus {
            mon.track_volatile("focusenergy");
            Some("Focus Energy")
        } else {
            None
        };
        if let Some(name) = name {
            self.add(format!(
                "|-start|{}|move: {name}|[silent]",
                self.ident(source)
            ));
        }
        if laser {
            self.start_laser_focus(source, true);
        }
    }

    pub(super) fn start_laser_focus(&mut self, pos: Position, copied: bool) {
        let mon = self.mon_mut(pos);
        mon.laser_focus = 2;
        mon.track_volatile("laserfocus");
        let suffix = if copied { "|[silent]" } else { "" };
        self.add(format!(
            "|-start|{}|move: Laser Focus{suffix}",
            self.ident(pos)
        ));
    }

    pub(super) fn end_laser_focus(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.volatile_ids().contains(&"laserfocus") {
            mon.untrack_volatile("laserfocus");
            mon.laser_focus = 0;
            self.add(format!(
                "|-end|{}|move: Laser Focus|[silent]",
                self.ident(pos)
            ));
        }
    }

    pub(super) fn start_dragon_cheer(&mut self, target: Position) -> bool {
        let mon = self.mon_mut(target);
        if mon.dragon_cheer != 0 || mon.volatile_ids().contains(&"focusenergy") {
            return false;
        }
        mon.dragon_cheer = if mon.has_type("Dragon") { 2 } else { 1 };
        mon.track_volatile("dragoncheer");
        self.add(format!("|-start|{}|move: Dragon Cheer", self.ident(target)));
        true
    }

    pub(super) fn apply_stat_move(
        &mut self,
        source: Position,
        target: Position,
        name: &str,
    ) -> bool {
        use BoostStat::{Atk, Def, Spa, Spd};
        match name {
            "Power Trick" => {
                let mon = self.mon_mut(target);
                mon.swap_attack_defense();
                let event = if mon.volatile_ids().contains(&"powertrick") {
                    mon.untrack_volatile("powertrick");
                    "-end"
                } else {
                    mon.track_volatile("powertrick");
                    "-start"
                };
                self.add(format!("|{event}|{}|Power Trick", self.ident(target)));
                return true;
            }
            "Psych Up" => {
                self.mon_mut(source).boosts = self.mon(target).boosts;
                self.copy_critical_conditions(source, target);
                self.add(format!(
                    "|-copyboost|{}|{}|[from] move: Psych Up",
                    self.ident(source),
                    self.ident(target)
                ));
                return true;
            }
            "Topsy-Turvy" => {
                if self.mon_mut(target).boosts.invert() {
                    self.add(format!(
                        "|-invertboost|{}|[from] move: Topsy-Turvy",
                        self.ident(target)
                    ));
                    return true;
                }
                return false;
            }
            _ => {}
        }
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
