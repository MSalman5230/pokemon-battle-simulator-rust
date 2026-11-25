//! Champions weather lifecycle and its stat/damage effects.

use super::turn::Position;
use super::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Weather {
    Rain,
    Sun,
    Sand,
    Snow,
}

impl Weather {
    fn label(self) -> &'static str {
        match self {
            Self::Rain => "RainDance",
            Self::Sun => "SunnyDay",
            Self::Sand => "Sandstorm",
            Self::Snow => "Snowscape",
        }
    }

    fn rock(self) -> &'static str {
        match self {
            Self::Rain => "damprock",
            Self::Sun => "heatrock",
            Self::Sand => "smoothrock",
            Self::Snow => "icyrock",
        }
    }

    pub(super) fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Rain Dance" => Self::Rain,
            "Sunny Day" => Self::Sun,
            "Sandstorm" => Self::Sand,
            "Snowscape" => Self::Snow,
            _ => return None,
        })
    }

    pub(super) fn damage_modifier(self, move_type: &str) -> u32 {
        match (self, move_type) {
            (Self::Rain, "Water") | (Self::Sun, "Fire") => 6144,
            (Self::Rain, "Fire") | (Self::Sun, "Water") => 2048,
            _ => 4096,
        }
    }

    pub(super) fn ball_type(self) -> &'static str {
        match self {
            Self::Rain => "Water",
            Self::Sun => "Fire",
            Self::Sand => "Rock",
            Self::Snow => "Ice",
        }
    }
}

impl Battle {
    pub fn weather(&self) -> Option<Weather> {
        self.weather.map(|(weather, _)| weather)
    }

    pub(super) fn effective_weather(&self) -> Option<Weather> {
        self.weather().filter(|_| !self.weather_suppressed())
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

    pub(super) fn weather_ability_changed(&mut self, pos: Position) {
        if self.weather_suppressor(pos).is_some() {
            // WeatherChange sorts all actives even with no listeners.
            self.ordered_actives(false);
        }
    }

    pub(super) fn switch_in_weather(&mut self, pos: Position) {
        let (weather, label) = match self.mon(pos).ability.as_str() {
            "drizzle" => (Weather::Rain, "Drizzle"),
            "drought" => (Weather::Sun, "Drought"),
            "sandstream" => (Weather::Sand, "Sand Stream"),
            "snowwarning" => (Weather::Snow, "Snow Warning"),
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
        let duration = if self.mon(pos).item == weather.rock() {
            8
        } else {
            5
        };
        self.weather = Some((weather, duration));
        let reason = ability.map_or_else(String::new, |label| {
            format!("|[from] ability: {label}|[of] {}", self.ident(pos))
        });
        self.add(format!("|-weather|{}{reason}", weather.label()));
        self.ordered_actives(false);
        true
    }

    pub(super) fn weather_residual(&mut self) {
        let Some((weather, turns)) = self.weather else {
            return;
        };
        if turns <= 1 {
            self.weather = None;
            self.add("|-weather|none");
            self.ordered_actives(false);
            return;
        }
        self.weather = Some((weather, turns - 1));
        self.add(format!("|-weather|{}|[upkeep]", weather.label()));
        if self.weather_suppressed() {
            return;
        }
        let (ordered, count) = self.ordered_actives(false);
        for (pos, _) in ordered.into_iter().take(count) {
            if weather == Weather::Sand {
                if !self.status_immune(pos, "sandstorm")
                    && !matches!(
                        self.mon(pos).ability.as_str(),
                        "sandrush" | "sandforce" | "sandveil"
                    )
                {
                    self.indirect_damage(pos, 16, "|[from] Sandstorm");
                }
            } else if weather == Weather::Snow && self.mon(pos).ability == "icebody" {
                self.ability_heal(pos, 16, "Ice Body", None);
            } else if self.mon(pos).ability == "dryskin" {
                match weather {
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
}
