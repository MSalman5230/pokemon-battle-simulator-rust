//! Ordered side conditions that apply when a Pokémon enters the field.

use super::*;
use super::{abilities::BoostOrigin, turn::Position};
use crate::mechanics::BoostStat;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) enum Hazard {
    StealthRock,
    Spikes,
    ToxicSpikes,
    StickyWeb,
}

impl Hazard {
    pub(super) fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Stealth Rock" => Self::StealthRock,
            "Spikes" => Self::Spikes,
            "Toxic Spikes" => Self::ToxicSpikes,
            "Sticky Web" => Self::StickyWeb,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::StealthRock => "move: Stealth Rock",
            Self::Spikes => "Spikes",
            Self::ToxicSpikes => "move: Toxic Spikes",
            Self::StickyWeb => "move: Sticky Web",
        }
    }

    fn cap(self) -> u8 {
        match self {
            Self::Spikes => 3,
            Self::ToxicSpikes => 2,
            _ => 1,
        }
    }
}

impl Battle {
    pub(super) fn grounded(&self, pos: Position) -> bool {
        let mon = self.mon(pos);
        !mon.has_type("Flying")
            && mon.item != "airballoon"
            && (mon.ability != "levitate" || self.ignores_ability_of(pos))
    }

    pub(super) fn tidy_hazards(&mut self, pos: Position) -> bool {
        let mut removed = false;
        for player in [pos.player, pos.player.foe()] {
            for hazard in [
                Hazard::Spikes,
                Hazard::ToxicSpikes,
                Hazard::StealthRock,
                Hazard::StickyWeb,
            ] {
                let hazards = &mut self.sides[player.index()].as_mut().unwrap().hazards;
                let before = hazards.len();
                hazards.retain(|&(h, _)| h != hazard);
                if hazards.len() != before {
                    let label = hazard
                        .label()
                        .strip_prefix("move: ")
                        .unwrap_or(hazard.label());
                    self.add(format!("|-sideend|{}|{label}", self.side_ident(player)));
                    removed = true;
                }
            }
        }
        if removed {
            self.add(format!("|-activate|{}|move: Tidy Up", self.ident(pos)));
        }
        removed
    }

    pub(super) fn court_change(&mut self, source: Position) -> bool {
        let [p1, p2] = &mut self.sides;
        let p1 = p1.as_mut().unwrap();
        let p2 = p2.as_mut().unwrap();
        if p1
            .screens
            .iter()
            .chain(&p2.screens)
            .all(|&duration| duration == 0)
            && p1.hazards.is_empty()
            && p2.hazards.is_empty()
            && p1.tailwind == 0
            && p2.tailwind == 0
        {
            return false;
        }
        std::mem::swap(&mut p1.screens, &mut p2.screens);
        std::mem::swap(&mut p1.hazards, &mut p2.hazards);
        std::mem::swap(&mut p1.tailwind, &mut p2.tailwind);
        self.add("|-swapsideconditions");
        self.add(format!(
            "|-activate|{}|move: Court Change",
            self.ident(source)
        ));
        true
    }

    pub(super) fn add_hazard(&mut self, player: Player, hazard: Hazard) -> bool {
        let side = self.sides[player.index()].as_mut().unwrap();
        if let Some((_, layers)) = side.hazards.iter_mut().find(|(h, _)| *h == hazard) {
            if *layers >= hazard.cap() {
                return false;
            }
            *layers += 1;
        } else {
            side.hazards.push((hazard, 1));
        }
        self.add(format!(
            "|-sidestart|{}|{}",
            self.side_ident(player),
            hazard.label()
        ));
        true
    }

    pub(super) fn side_ident(&self, player: Player) -> String {
        format!(
            "{}: {}",
            player.id(),
            self.sides[player.index()].as_ref().unwrap().name
        )
    }

    pub(super) fn on_switch_in_hazards(&mut self, pos: Position) {
        let player = pos.player;
        let foe = Position {
            player: player.foe(),
            slot: 0,
        };
        // Side conditions have equal speed, order and priority. Showdown's
        // SwitchIn comparator breaks the tie by creation order.
        let hazards = self.sides[player.index()].as_ref().unwrap().hazards.clone();
        for (hazard, layers) in hazards {
            let mon = self.mon(pos);
            if mon.hp == 0 {
                break;
            }
            let grounded = self.grounded(pos);
            let boots = mon.item == "heavydutyboots";
            let max_hp = mon.max_hp.get();
            match hazard {
                Hazard::StealthRock if !boots => {
                    let move_data = DEX.moves(self.format.dex_mod, "stealthrock").unwrap();
                    let stages = self.effectiveness(move_data, pos).unwrap_or(0);
                    let damage = if stages >= 0 {
                        (max_hp << stages) / 8
                    } else {
                        (max_hp >> -stages) / 8
                    };
                    self.hazard_damage(pos, damage, "|[from] Stealth Rock");
                }
                Hazard::Spikes if grounded && !boots => {
                    let damage = [0, 3, 4, 6][usize::from(layers)] * max_hp / 24;
                    self.hazard_damage(pos, damage, "|[from] Spikes");
                }
                Hazard::ToxicSpikes if grounded => {
                    if mon.has_type("Poison") {
                        self.sides[player.index()]
                            .as_mut()
                            .unwrap()
                            .hazards
                            .retain(|(h, _)| *h != hazard);
                        self.add(format!(
                            "|-sideend|{}|{}|[of] {}",
                            self.side_ident(player),
                            hazard.label(),
                            self.ident(pos)
                        ));
                    } else if !boots
                        && mon.status == Status::Healthy
                        && !self.status_immune(pos, "psn")
                    {
                        let id = if layers >= 2 { "tox" } else { "psn" };
                        let prevented = self.status_blocked_by_ability(pos, id, false)
                            || self.flower_veil_status(pos, foe, id, false);
                        if !prevented && !self.field_blocks_status(pos, id, foe.player, false) {
                            self.apply_status(pos, id, foe.player, None);
                        }
                    }
                }
                Hazard::StickyWeb if grounded && !boots => {
                    self.add(format!("|-activate|{}|move: Sticky Web", self.ident(pos)));
                    self.boost_from(pos, foe, &[(BoostStat::Spe, -1)], BoostOrigin::Hazard);
                }
                _ => {}
            }
        }
    }

    fn hazard_damage(&mut self, pos: Position, damage: u32, reason: &str) {
        let mon = self.mon_mut(pos);
        if mon.ability == "magicguard" {
            return;
        }
        mon.hp = mon.hp.saturating_sub(damage.max(1));
        self.add_health_line_with("-damage", pos, false, reason);
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
    }
}

impl Pokemon {
    pub(super) fn has_type(&self, name: &str) -> bool {
        self.types().any(|t| t == name)
    }

    pub fn types(&self) -> impl Iterator<Item = &str> {
        let lone = self.temporary_type.or_else(|| {
            (self.roosted && self.species.types.as_slice() == ["Flying"]).then_some("Normal")
        });
        let lone = lone.map(|ty| {
            if self.roosted && ty == "Flying" {
                "Normal"
            } else {
                ty
            }
        });
        self.species
            .types
            .iter()
            .map(String::as_str)
            .filter(move |t| lone.is_none() && (!self.roosted || *t != "Flying"))
            .chain(lone)
    }
}
