//! Ordered side conditions that apply when a Pokémon enters the field.

use super::turn::Position;
use super::*;
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

    fn side_ident(&self, player: Player) -> String {
        format!(
            "{}: {}",
            player.id(),
            self.sides[player.index()].as_ref().unwrap().name
        )
    }

    pub(super) fn on_switch_in_hazards(&mut self, pos: Position) {
        let player = pos.player;
        // Side conditions have equal speed, order and priority. Showdown's
        // SwitchIn comparator breaks the tie by creation order.
        let hazards = self.sides[player.index()].as_ref().unwrap().hazards.clone();
        for (hazard, layers) in hazards {
            let mon = self.mon(pos);
            if mon.hp == 0 {
                break;
            }
            let grounded = mon.grounded();
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
                        let status = if layers >= 2 {
                            Status::Toxic(0)
                        } else {
                            Status::Poison
                        };
                        self.mon_mut(pos).status = status;
                        self.add(format!("|-status|{}|{}", self.ident(pos), status.id()));
                        self.cure_with_berry(pos);
                    }
                }
                Hazard::StickyWeb if grounded && !boots => {
                    self.add(format!("|-activate|{}|move: Sticky Web", self.ident(pos)));
                    self.boost_from(
                        pos,
                        Position {
                            player: player.foe(),
                            slot: 0,
                        },
                        &[(BoostStat::Spe, -1)],
                        false,
                        false,
                    );
                }
                _ => {}
            }
        }
    }

    fn hazard_damage(&mut self, pos: Position, damage: u32, reason: &str) {
        let mon = self.mon_mut(pos);
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
        let add_normal = self.roosted && self.species.types.as_slice() == ["Flying"];
        self.species
            .types
            .iter()
            .map(String::as_str)
            .filter(|t| !self.roosted || *t != "Flying")
            .chain(add_normal.then_some("Normal"))
    }

    pub(super) fn grounded(&self) -> bool {
        !self.has_type("Flying") && self.item != "airballoon" && self.ability != "levitate"
    }
}
