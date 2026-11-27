//! Champions weather lifecycle and its stat/damage effects.

use super::turn::Position;
use super::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Weather {
    Rain,
    Sun,
    Sand,
    Snow,
    Hail,
    StrongWinds,
    HarshSunlight,
    HeavyRain,
}

impl Weather {
    fn label(self) -> &'static str {
        match self {
            Self::Rain => "RainDance",
            Self::Sun => "SunnyDay",
            Self::Sand => "Sandstorm",
            Self::Snow => "Snowscape",
            Self::Hail => "Hail",
            Self::StrongWinds => "DeltaStream",
            Self::HarshSunlight => "DesolateLand",
            Self::HeavyRain => "PrimordialSea",
        }
    }

    fn rock(self) -> &'static str {
        match self {
            Self::Rain => "damprock",
            Self::Sun => "heatrock",
            Self::Sand => "smoothrock",
            Self::Snow | Self::Hail => "icyrock",
            Self::StrongWinds | Self::HarshSunlight | Self::HeavyRain => "",
        }
    }

    pub(super) fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Rain Dance" => Self::Rain,
            "Sunny Day" => Self::Sun,
            "Sandstorm" => Self::Sand,
            "Snowscape" => Self::Snow,
            "Hail" => Self::Hail,
            _ => return None,
        })
    }

    pub(super) fn damage_modifier(self, move_type: &str) -> u32 {
        match (self, move_type) {
            (Self::Rain | Self::HeavyRain, "Water") | (Self::Sun | Self::HarshSunlight, "Fire") => {
                6144
            }
            (Self::Rain, "Fire") | (Self::Sun, "Water") => 2048,
            _ => 4096,
        }
    }

    pub(super) fn ball_type(self) -> &'static str {
        match self {
            Self::Rain | Self::HeavyRain => "Water",
            Self::Sun | Self::HarshSunlight => "Fire",
            Self::Sand => "Rock",
            Self::Snow | Self::Hail => "Ice",
            Self::StrongWinds => "Normal",
        }
    }

    fn permanent_ability(self) -> Option<&'static str> {
        match self {
            Self::StrongWinds => Some("deltastream"),
            Self::HarshSunlight => Some("desolateland"),
            Self::HeavyRain => Some("primordialsea"),
            _ => None,
        }
    }

    pub(super) fn ordinary(self) -> Self {
        match self {
            Self::HarshSunlight => Self::Sun,
            Self::HeavyRain => Self::Rain,
            weather => weather,
        }
    }

    pub(super) fn extinguishes(self, typing: &str) -> Option<(&'static str, &'static str)> {
        match (self, typing) {
            (Self::HarshSunlight, "Water") => Some(("Desolate Land", "water")),
            (Self::HeavyRain, "Fire") => Some(("Primordial Sea", "fire")),
            _ => None,
        }
    }
}

impl Pokemon {
    pub(super) fn personal_weather(&self, weather: Option<Weather>) -> Option<Weather> {
        weather.filter(|weather| {
            !matches!(weather.ordinary(), Weather::Sun | Weather::Rain)
                || self.effective_item() != "utilityumbrella"
        })
    }
}

impl Battle {
    pub fn weather(&self) -> Option<Weather> {
        self.weather.map(|(weather, _)| weather)
    }

    pub(super) fn effective_weather(&self) -> Option<Weather> {
        self.weather().filter(|_| !self.weather_suppressed())
    }

    /// Numeric ModifyAccuracy multiplier and whether its cause is Snow Cloak.
    pub(super) fn weather_evasion(&self, pos: Position, weather: Option<Weather>) -> (bool, bool) {
        match (weather, self.event_ability(pos)) {
            (Some(Weather::Sand), "sandveil") => (true, false),
            (Some(Weather::Snow | Weather::Hail), "snowcloak") => (true, true),
            _ => (false, false),
        }
    }

    pub(super) fn active_weather_evasion(&self, pos: Position) -> (bool, bool) {
        match (self.weather(), self.event_ability(pos)) {
            (Some(Weather::Sand), "sandveil") => (!self.weather_suppressed(), false),
            (Some(Weather::Snow | Weather::Hail), "snowcloak") if !self.weather_suppressed() => {
                (true, true)
            }
            _ => (false, false),
        }
    }

    fn weather_suppressed(&self) -> bool {
        self.positions()
            .any(|pos| !self.mon(pos).fainted && self.weather_suppressor(pos).is_some())
    }

    pub(super) fn weather_suppressor(&self, pos: Position) -> Option<&'static str> {
        match self.event_ability(pos) {
            "cloudnine" => Some("Cloud Nine"),
            "airlock" => Some("Air Lock"),
            _ => None,
        }
    }

    pub(super) fn weather_ability_changed(&mut self, pos: Position, ending: bool) {
        if ending
            && self.strong_weather_source == Some((pos.player, self.mon(pos).roster_index))
            && let Some(ability) = self.weather().and_then(Weather::permanent_ability)
            && self.mon(pos).ability == ability
        {
            self.strong_weather_source = self
                .positions()
                .filter(|&other| other != pos)
                .find(|&other| self.mon(other).hp > 0 && self.event_ability(other) == ability)
                .map(|other| (other.player, self.mon(other).roster_index));
            if self.strong_weather_source.is_none() {
                self.weather = None;
                self.add("|-weather|none");
                self.weather_change_event(None);
            }
        }
        if self.weather_suppressor(pos).is_some() {
            self.weather_change_event(ending.then_some(pos));
        }
    }

    fn weather_change_event(&mut self, ending_suppressor: Option<Position>) {
        // WeatherChange sorts all actives even with no listeners.
        let (ordered, count) = self.ordered_actives(false);
        let weather = self.weather().filter(|_| {
            !self.positions().any(|pos| {
                Some(pos) != ending_suppressor
                    && !self.mon(pos).fainted
                    && self.weather_suppressor(pos).is_some()
            })
        });
        for (pos, _) in ordered.into_iter().take(count) {
            self.forecast(pos, weather);
            if self.mon(pos).ability == "flowergift" {
                self.sync_flower_gift(pos, weather);
            }
            if ending_suppressor.is_none() && self.mon(pos).ability == "iceface" {
                self.restore_ice_face(pos, weather);
            }
            if self.mon(pos).ability == "protosynthesis" {
                self.sync_paradox(pos, weather, None);
            }
        }
    }

    pub(super) fn switch_in_weather(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let (weather, label) = match mon.active_ability() {
            "drizzle" if mon.species.name == "Kyogre" && mon.item == "blueorb" => return,
            "drought" if mon.species.name == "Groudon" && mon.item == "redorb" => return,
            "drizzle" => (Weather::Rain, "Drizzle"),
            "drought" => (Weather::Sun, "Drought"),
            "sandstream" => (Weather::Sand, "Sand Stream"),
            "snowwarning" => (Weather::Snow, "Snow Warning"),
            "deltastream" => (Weather::StrongWinds, "Delta Stream"),
            "desolateland" => (Weather::HarshSunlight, "Desolate Land"),
            "primordialsea" => (Weather::HeavyRain, "Primordial Sea"),
            _ => return,
        };
        self.set_weather(pos, weather, Some(label));
    }

    pub(super) fn set_weather(
        &mut self,
        pos: Position,
        weather: Weather,
        ability: Option<&str>,
    ) -> bool {
        if self.weather() == Some(weather) {
            return false;
        }
        let strong = weather.permanent_ability().is_some();
        if let Some(lock) = self.weather_lock(weather) {
            if let Some(label) = ability {
                self.add(format!(
                    "|-ability|{}|{label}|[from] {lock}|[fail]",
                    self.ident(pos)
                ));
            }
            return false;
        }
        let duration = if strong {
            0
        } else if self.mon(pos).effective_item() == weather.rock() {
            8
        } else {
            5
        };
        self.weather = Some((weather, duration));
        self.strong_weather_source = strong.then(|| (pos.player, self.mon(pos).roster_index));
        let reason = ability.map_or_else(String::new, |label| {
            format!("|[from] ability: {label}|[of] {}", self.ident(pos))
        });
        self.add(format!("|-weather|{}{reason}", weather.label()));
        self.weather_change_event(None);
        true
    }

    pub(super) fn weather_lock(&self, proposed: Weather) -> Option<&'static str> {
        if self.strong_weather_source.is_some() && proposed.permanent_ability().is_none() {
            self.weather().and_then(Weather::permanent_ability)
        } else {
            None
        }
    }

    pub(super) fn weather_residual(&mut self) {
        let Some((weather, turns)) = self.weather else {
            return;
        };
        if turns <= 1 && weather.permanent_ability().is_none() {
            self.weather = None;
            self.add("|-weather|none");
            self.weather_change_event(None);
            return;
        }
        self.weather = Some((weather, turns.saturating_sub(1)));
        self.add(format!("|-weather|{}|[upkeep]", weather.label()));
        if self.weather_suppressed() {
            return;
        }
        let (ordered, count) = self.ordered_actives(false);
        let ordinary = weather.ordinary();
        for (pos, _) in ordered.into_iter().take(count) {
            if weather == Weather::Sand {
                if !self.status_immune(pos, "sandstorm")
                    && !matches!(
                        self.mon(pos).active_ability(),
                        "sandrush" | "sandforce" | "sandveil"
                    )
                {
                    self.indirect_damage(pos, 16, "|[from] Sandstorm");
                } else {
                    self.log_weather_immunity(pos, "sandstorm");
                }
            } else if weather == Weather::Hail {
                let ability = self.event_ability(pos);
                let ice_body = ability == "icebody";
                if ice_body || ability == "snowcloak" || self.status_immune(pos, "hail") {
                    self.log_weather_immunity(pos, "hail");
                    if ice_body {
                        self.ability_heal(pos, 16, "Ice Body", None);
                    }
                } else {
                    self.indirect_damage(pos, 16, "|[from] Hail");
                }
            } else if weather == Weather::Snow && self.mon(pos).active_ability() == "icebody" {
                self.ability_heal(pos, 16, "Ice Body", None);
            } else if ordinary == Weather::Rain
                && self.event_ability(pos) == "raindish"
                && self.mon(pos).effective_item() != "utilityumbrella"
            {
                self.ability_heal(pos, 16, "Rain Dish", None);
            } else if ordinary == Weather::Sun
                && self.event_ability(pos) == "solarpower"
                && self.mon(pos).effective_item() != "utilityumbrella"
            {
                self.indirect_damage(
                    pos,
                    8,
                    &format!("|[from] ability: Solar Power|[of] {}", self.ident(pos)),
                );
            } else if self.mon(pos).active_ability() == "dryskin"
                && self.mon(pos).effective_item() != "utilityumbrella"
            {
                match ordinary {
                    Weather::Rain => {
                        self.ability_heal(pos, 8, "Dry Skin", None);
                    }
                    Weather::Sun => self.indirect_damage(
                        pos,
                        8,
                        &format!("|[from] ability: Dry Skin|[of] {}", self.ident(pos)),
                    ),
                    _ => {}
                }
            }
        }
        self.each_event();
        for (pos, _) in ordered.into_iter().take(count) {
            if self.mon(pos).hp == 0 && !self.mon(pos).fainted {
                self.mark_fainted(pos);
            }
        }
    }

    fn log_weather_immunity(&mut self, pos: Position, kind: &str) {
        if self.format.report_exact_hp {
            if self
                .mon(pos)
                .types()
                .filter(|ty| *ty != "???")
                .any(|ty| DEX.type_data_named(ty).unwrap().damage_taken.get(kind) == Some(&3))
            {
                self.add("|debug|natural status immunity");
            } else {
                self.add("|debug|artificial status immunity");
            }
            self.add("|debug|weather immunity");
        }
    }
}
