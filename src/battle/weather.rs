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
}

impl Battle {
    pub fn weather(&self) -> Option<Weather> {
        self.weather.map(|(weather, _)| weather)
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
