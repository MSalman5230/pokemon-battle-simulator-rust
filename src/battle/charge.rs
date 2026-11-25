//! Two-turn moves, including charge turns that grant semi-invulnerability.

use super::*;
use crate::mechanics::BoostStat;

#[derive(Clone, Copy)]
pub(super) enum SemiInvulnerable {
    Underwater,
    Underground,
}

impl SemiInvulnerable {
    pub(super) fn exposed_to(self, move_name: &str) -> bool {
        match self {
            Self::Underwater => matches!(move_name, "Surf" | "Whirlpool"),
            Self::Underground => matches!(move_name, "Earthquake" | "Magnitude"),
        }
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
        match self.charge.filter(|state| !state.released)?.id {
            "dive" => Some(SemiInvulnerable::Underwater),
            "dig" => Some(SemiInvulnerable::Underground),
            _ => None,
        }
    }

    pub(super) fn semi_invulnerable(&self) -> bool {
        self.semi_invulnerability().is_some()
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
