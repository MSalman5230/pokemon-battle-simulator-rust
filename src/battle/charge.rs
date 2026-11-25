//! Two-turn moves, including charge turns that grant semi-invulnerability.

use super::turn::Position;
use super::*;
use crate::mechanics::BoostStat;

#[derive(Clone, Copy)]
pub(super) enum SemiInvulnerable {
    Vanished,
    Underwater,
    Underground,
    Airborne { bounce: bool },
    SkyDrop,
}

impl SemiInvulnerable {
    pub(super) fn exposed_to(self, move_name: &str) -> bool {
        match self {
            Self::Vanished => false,
            Self::Underwater => matches!(move_name, "Surf" | "Whirlpool"),
            Self::Underground => matches!(move_name, "Earthquake" | "Magnitude"),
            Self::Airborne { .. } | Self::SkyDrop => matches!(
                move_name,
                "Gust"
                    | "Twister"
                    | "Sky Uppercut"
                    | "Thunder"
                    | "Hurricane"
                    | "Smack Down"
                    | "Thousand Arrows"
            ),
        }
    }

    pub(super) fn damage_modifier(self, move_name: &str) -> Option<u32> {
        let doubled = match self {
            Self::Vanished => return None,
            Self::Airborne { bounce: true } | Self::SkyDrop => return None,
            Self::Airborne { bounce: false } => matches!(move_name, "Gust" | "Twister"),
            _ => self.exposed_to(move_name),
        };
        Some(if doubled { 8192 } else { 4096 })
    }

    pub(super) fn doubles_power(self, move_name: &str) -> bool {
        matches!(self, Self::Airborne { bounce: true }) && matches!(move_name, "Gust" | "Twister")
    }
}

#[derive(Clone, Copy)]
pub(super) struct Charge {
    pub(super) slot: usize,
    pub(super) target: i8,
    id: &'static str,
    turns: u8,
    released: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ChargeSpec {
    pub(super) id: &'static str,
    pub(super) boost: Option<BoostStat>,
    pub(super) instant: Option<Weather>,
    pub(super) weak_in_bad_weather: bool,
}

impl ChargeSpec {
    pub(super) fn for_move(name: &str) -> Option<Self> {
        let (id, boost, instant) = match name {
            "Electro Shot" => ("electroshot", Some(BoostStat::Spa), Some(Weather::Rain)),
            "Solar Beam" => ("solarbeam", None, Some(Weather::Sun)),
            "Solar Blade" => ("solarblade", None, Some(Weather::Sun)),
            "Meteor Beam" => ("meteorbeam", Some(BoostStat::Spa), None),
            "Skull Bash" => ("skullbash", Some(BoostStat::Def), None),
            "Geomancy" => ("geomancy", None, None),
            "Dive" => ("dive", None, None),
            "Dig" => ("dig", None, None),
            "Bounce" => ("bounce", None, None),
            "Fly" => ("fly", None, None),
            "Phantom Force" => ("phantomforce", None, None),
            "Shadow Force" => ("shadowforce", None, None),
            "Sky Attack" => ("skyattack", None, None),
            "Freeze Shock" => ("freezeshock", None, None),
            "Ice Burn" => ("iceburn", None, None),
            "Razor Wind" => ("razorwind", None, None),
            _ => return None,
        };
        Some(Self {
            id,
            boost,
            instant,
            weak_in_bad_weather: instant == Some(Weather::Sun),
        })
    }
}

impl Pokemon {
    pub(super) fn begin_charge(&mut self, spec: ChargeSpec, slot: usize, target: i8) {
        self.charge = Some(Charge {
            slot,
            target,
            id: spec.id,
            turns: 2,
            released: false,
        });
        self.track_volatile("twoturnmove");
        self.track_volatile(spec.id);
    }

    pub(super) fn clear_charge(&mut self) {
        if let Some(state) = self.charge.take() {
            self.untrack_volatile("twoturnmove");
            self.untrack_volatile(state.id);
        }
    }

    pub(super) fn release_charge(&mut self) {
        if let Some(state) = &mut self.charge {
            state.released = true;
            let id = state.id;
            self.untrack_volatile(id);
        }
    }

    pub(super) fn semi_invulnerability(&self) -> Option<SemiInvulnerable> {
        if self.sky_drop_rider {
            return Some(SemiInvulnerable::SkyDrop);
        }
        match self.charge.filter(|state| !state.released)?.id {
            "dive" => Some(SemiInvulnerable::Underwater),
            "dig" => Some(SemiInvulnerable::Underground),
            "bounce" => Some(SemiInvulnerable::Airborne { bounce: true }),
            "fly" => Some(SemiInvulnerable::Airborne { bounce: false }),
            "skydrop" => Some(SemiInvulnerable::SkyDrop),
            "phantomforce" | "shadowforce" => Some(SemiInvulnerable::Vanished),
            _ => None,
        }
    }

    pub(super) fn semi_invulnerable(&self) -> bool {
        self.semi_invulnerability().is_some()
    }

    pub(super) fn is_sky_dropped(&self) -> bool {
        self.sky_drop_rider || self.sky_drop_target.is_some()
    }

    pub(super) fn tick_charge(&mut self) {
        if let Some(state) = &mut self.charge {
            state.turns -= 1;
            if state.turns == 0 {
                self.clear_charge();
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum DropEnd {
    Land,
    Cancel,
    Interrupt,
}

impl Battle {
    pub(super) fn sky_drop_rider(&self, carrier: Position) -> Option<Position> {
        let (player, roster) = self.mon(carrier).sky_drop_target?;
        (0..self.slots())
            .map(|slot| Position { player, slot })
            .find(|&pos| self.mon(pos).roster_index == usize::from(roster))
    }

    pub(super) fn start_sky_drop(
        &mut self,
        carrier: Position,
        rider: Position,
        slot: usize,
        loc: i8,
    ) {
        let roster = self.mon(rider).roster_index as u8;
        let mon = self.mon_mut(carrier);
        mon.sky_drop_target = Some((rider.player, roster));
        mon.begin_charge(
            ChargeSpec {
                id: "skydrop",
                boost: None,
                instant: None,
                weak_in_bad_weather: false,
            },
            slot,
            loc,
        );
        self.mon_mut(rider).sky_drop_rider = true;
        self.add(format!(
            "|-prepare|{}|Sky Drop|{}",
            self.ident(carrier),
            self.ident(rider)
        ));
    }

    pub(super) fn finish_sky_drop(&mut self, carrier: Position, how: DropEnd) {
        if let Some(rider) = self.sky_drop_rider(carrier) {
            self.mon_mut(rider).sky_drop_rider = false;
            if matches!(how, DropEnd::Interrupt) {
                self.add(format!("|-end|{}|Sky Drop|[interrupt]", self.ident(rider)));
            }
        }
        let mon = self.mon_mut(carrier);
        mon.sky_drop_target = None;
        if matches!(how, DropEnd::Land) {
            mon.release_charge();
        } else {
            mon.clear_charge();
        }
    }

    pub(super) fn charge_electric(&mut self, pos: Position, triggered_by: Option<(&str, &str)>) {
        self.mon_mut(pos).track_volatile("charge");
        let ident = self.ident(pos);
        if let Some((move_name, ability)) = triggered_by {
            self.add(format!(
                "|-start|{ident}|Charge|{move_name}|[from] ability: {ability}"
            ));
        } else {
            self.add(format!("|-start|{ident}|Charge"));
        }
    }

    pub(super) fn use_electric_charge(&mut self, pos: Position) {
        if self.mon(pos).volatile_ids().contains(&"charge") {
            self.mon_mut(pos).untrack_volatile("charge");
            self.add(format!("|-end|{}|Charge|[silent]", self.ident(pos)));
        }
    }

    pub(super) fn charge_after_damage(&mut self, target: Position, data: &Move) {
        if self.mon(target).hp == 0 {
            return;
        }
        let cause = match self.event_ability(target) {
            "electromorphosis" => "Electromorphosis",
            "windpower" if data.flags.contains_key("wind") => "Wind Power",
            _ => return,
        };
        self.charge_electric(target, Some((&data.name, cause)));
    }

    pub(super) fn charge_from_tailwind(&mut self, player: Player) {
        let mut holders = [(Position { player, slot: 0 }, 0); 2];
        let mut len = 0;
        for pos in self.positions().filter(|pos| pos.player == player) {
            if self.event_ability(pos) == "windpower" {
                holders[len] = (pos, self.mon(pos).speed);
                len += 1;
            }
        }
        speed_sort_by(&mut holders[..len], &mut self.rng, |a, b| b.1.cmp(&a.1));
        for &(pos, _) in &holders[..len] {
            self.charge_electric(pos, Some(("Tailwind", "Wind Power")));
        }
    }
}
