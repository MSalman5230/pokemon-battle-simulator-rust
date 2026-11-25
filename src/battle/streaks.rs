//! Inherited move powers whose histories belong to different owners.

use super::{turn::Position, *};

#[derive(Clone, Copy)]
pub(super) struct Streak {
    multiplier: u8,
    turns: u8,
}

impl Streak {
    fn first() -> Self {
        Self {
            multiplier: 1,
            turns: 2,
        }
    }

    pub(super) fn tick(state: &mut Option<Self>) {
        if let Some(streak) = state.as_mut() {
            streak.turns -= 1;
            if streak.turns == 0 {
                *state = None;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fusion {
    Bolt,
    Flare,
}

#[derive(Clone, Copy)]
pub(super) struct RollingAttack {
    pub(super) slot: usize,
    pub(super) target: i8,
    id: &'static str,
    hits: u8,
    turns: u8,
}

impl Pokemon {
    pub(super) fn begin_rolling_attack(&mut self, name: &str, slot: usize, target: i8) {
        if self.rolling_attack.is_some() || matches!(self.status, Status::Sleep(_)) {
            return;
        }
        let id = match name {
            "Rollout" => "rollout",
            "Ice Ball" => "iceball",
            _ => return,
        };
        self.rolling_attack = Some(RollingAttack {
            slot,
            target,
            id,
            hits: 0,
            turns: 1,
        });
        self.track_volatile(id);
    }

    pub(super) fn tick_rolling_attack(&mut self) {
        if let Some(state) = &mut self.rolling_attack {
            state.turns -= 1;
            if state.turns == 0 {
                let id = state.id;
                self.rolling_attack = None;
                self.untrack_volatile(id);
            }
        }
    }
}

impl Fusion {
    pub(super) fn for_move(name: &str) -> Option<Self> {
        match name {
            "Fusion Bolt" => Some(Self::Bolt),
            "Fusion Flare" => Some(Self::Flare),
            _ => None,
        }
    }
}

impl Battle {
    pub(super) fn continue_echoed_voice(&mut self) {
        let Some(streak) = &mut self.echoed_voice else {
            self.echoed_voice = Some(Streak::first());
            return;
        };
        if streak.turns != 2 {
            streak.turns = 2;
            streak.multiplier = (streak.multiplier + 1).min(5);
        }
    }

    pub(super) fn inherited_streak_power(
        &mut self,
        pos: Position,
        name: &str,
        hit: u8,
        base: u16,
    ) -> u16 {
        match name {
            "Echoed Voice" => {
                if self.format.report_exact_hp {
                    // The native debug text displays the move's printed power.
                    self.add(format!("|debug|BP: {base}"));
                }
                base * u16::from(self.echoed_voice.map_or(1, |s| s.multiplier))
            }
            "Fury Cutter" => {
                let mon = self.mon_mut(pos);
                if let Some(streak) = &mut mon.fury_cutter {
                    if hit == 1 {
                        streak.turns = 2;
                        streak.multiplier = (streak.multiplier * 2).min(4);
                    }
                } else {
                    mon.fury_cutter = Some(Streak::first());
                    mon.track_volatile("furycutter");
                }
                let power = (base * u16::from(mon.fury_cutter.unwrap().multiplier)).clamp(1, 160);
                if self.format.report_exact_hp {
                    self.add(format!("|debug|BP: {power}"));
                }
                power
            }
            "Rollout" | "Ice Ball" => {
                let mon = self.mon_mut(pos);
                let mut power = base;
                if let Some(state) = &mut mon.rolling_attack {
                    power <<= state.hits;
                    if !matches!(mon.status, Status::Sleep(_)) {
                        state.hits += 1;
                        if state.hits < 5 {
                            state.turns = 2;
                        }
                    }
                }
                if mon.volatile_ids().contains(&"defensecurl") {
                    power *= 2;
                }
                if self.format.report_exact_hp {
                    self.add(format!("|debug|BP: {power}"));
                }
                power
            }
            _ => base,
        }
    }

    pub(super) fn fusion_power(&self, name: &str) -> bool {
        self.last_fusion
            .is_some_and(|last| Fusion::for_move(name).is_some_and(|selected| selected != last))
    }
}
