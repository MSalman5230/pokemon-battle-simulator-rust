//! Grounding-sensitive terrain conditions and their lifetimes.

use super::{turn::Position, *};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Terrain {
    Electric,
    Grassy,
    Misty,
    Psychic,
}

impl Terrain {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Electric => "Electric Terrain",
            Self::Grassy => "Grassy Terrain",
            Self::Misty => "Misty Terrain",
            Self::Psychic => "Psychic Terrain",
        }
    }

    pub(super) fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Electric Terrain" => Self::Electric,
            "Grassy Terrain" => Self::Grassy,
            "Misty Terrain" => Self::Misty,
            "Psychic Terrain" => Self::Psychic,
            _ => return None,
        })
    }
}

impl Battle {
    pub fn terrain(&self) -> Option<Terrain> {
        self.terrain.map(|(terrain, _)| terrain)
    }

    pub(super) fn switch_in_terrain(&mut self, pos: Position) {
        let (terrain, ability) = match self.mon(pos).ability.as_str() {
            "electricsurge" => (Terrain::Electric, "Electric Surge"),
            "grassysurge" => (Terrain::Grassy, "Grassy Surge"),
            "mistysurge" => (Terrain::Misty, "Misty Surge"),
            "psychicsurge" => (Terrain::Psychic, "Psychic Surge"),
            _ => return,
        };
        self.set_terrain(pos, terrain, Some(ability));
    }

    pub(super) fn set_terrain(
        &mut self,
        pos: Position,
        terrain: Terrain,
        ability: Option<&str>,
    ) -> bool {
        if self.terrain() == Some(terrain) {
            return false;
        }
        self.terrain = Some((
            terrain,
            if self.mon(pos).item == "terrainextender" {
                8
            } else {
                5
            },
        ));
        let source = ability.map_or_else(String::new, |name| {
            format!("|[from] ability: {name}|[of] {}", self.ident(pos))
        });
        self.add(format!("|-fieldstart|move: {}{source}", terrain.name()));
        self.ordered_actives(false);
        true
    }

    pub(super) fn clear_terrain(&mut self) {
        if let Some((terrain, _)) = self.terrain.take() {
            let prefix = if terrain == Terrain::Misty {
                ""
            } else {
                "move: "
            };
            self.add(format!("|-fieldend|{prefix}{}", terrain.name()));
            self.ordered_actives(false);
        }
    }

    pub(super) fn tick_terrain(&mut self) {
        if let Some((_, turns)) = &mut self.terrain {
            *turns -= 1;
            if *turns == 0 {
                self.clear_terrain();
            }
        }
    }

    pub(super) fn grassy_heal(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if !mon.grounded() {
            if self.format.report_exact_hp {
                self.add("|debug|Pokemon semi-invuln or not grounded; Grassy Terrain skipped");
            }
            return;
        }
        if mon.hp > 0 && mon.hp < mon.max_hp.get() {
            mon.hp = (mon.hp + (mon.max_hp.get() / 16).max(1)).min(mon.max_hp.get());
            self.add_health_line_with("-heal", pos, false, "|[from] Grassy Terrain");
        }
    }

    pub(super) fn status_blocked_by_terrain(&self, pos: Position, status: &str) -> Option<Terrain> {
        if !self.mon(pos).grounded() {
            return None;
        }
        match self.terrain() {
            Some(Terrain::Misty) => Some(Terrain::Misty),
            Some(Terrain::Electric) if status == "slp" => Some(Terrain::Electric),
            _ => None,
        }
    }

    pub(super) fn terrain_activate(&mut self, pos: Position, terrain: Terrain) {
        self.add(format!(
            "|-activate|{}|move: {}",
            self.ident(pos),
            terrain.name()
        ));
    }

    pub(super) fn terrain_power_modifier(
        &mut self,
        attacker: Position,
        defender: Position,
        data: &Move,
        move_type: &str,
    ) -> u32 {
        let Some(terrain) = self.terrain() else {
            return 4096;
        };
        let (factor, debug) = match (terrain, move_type) {
            (Terrain::Grassy, _)
                if self.mon(defender).grounded()
                    && matches!(data.name.as_str(), "Earthquake" | "Bulldoze" | "Magnitude") =>
            {
                (2048, "move weakened by grassy terrain")
            }
            (Terrain::Misty, "Dragon") if self.mon(defender).grounded() => {
                (2048, "misty terrain weaken")
            }
            (Terrain::Electric, "Electric") if self.mon(attacker).grounded() => {
                (5325, "electric terrain boost")
            }
            (Terrain::Grassy, "Grass") if self.mon(attacker).grounded() => {
                (5325, "grassy terrain boost")
            }
            (Terrain::Psychic, "Psychic") if self.mon(attacker).grounded() => {
                (5325, "psychic terrain boost")
            }
            _ => return 4096,
        };
        if self.format.report_exact_hp {
            self.add(format!("|debug|{debug}"));
        }
        factor
    }
}
