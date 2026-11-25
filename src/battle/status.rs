//! Major status and flinch behavior, including the Champions timer overrides.

use super::turn::Position;
use super::*;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum Status {
    #[default]
    Healthy,
    Burn,
    Poison,
    Toxic(u8),
    Paralysis,
    Sleep(u8),
    Freeze(u8),
    Fainted,
}

impl Status {
    pub fn id(self) -> &'static str {
        match self {
            Self::Healthy => "",
            Self::Burn => "brn",
            Self::Poison => "psn",
            Self::Toxic(_) => "tox",
            Self::Paralysis => "par",
            Self::Sleep(_) => "slp",
            Self::Freeze(_) => "frz",
            Self::Fainted => "fnt",
        }
    }

    fn from_id(id: &str) -> Self {
        match id {
            "brn" => Self::Burn,
            "psn" => Self::Poison,
            "tox" => Self::Toxic(0),
            "par" => Self::Paralysis,
            "slp" => Self::Sleep(0),
            "frz" => Self::Freeze(3),
            _ => unreachable!("move must only use supported major status effects"),
        }
    }

    // Order, HP divisor, and protocol effect label.
    pub(super) fn residual(self) -> Option<(u32, u32, &'static str)> {
        match self {
            Self::Burn => Some((10, 16, "brn")),
            Self::Poison => Some((9, 8, "psn")),
            Self::Toxic(_) => Some((9, 16, "psn")),
            _ => None,
        }
    }
}

impl Battle {
    pub(super) fn before_status_move(&mut self, pos: Position, data: &Move) -> bool {
        let status = self.mon(pos).status;
        match status {
            Status::Sleep(turns) => {
                if turns <= 1 {
                    self.cure_status(pos, "[msg]");
                } else {
                    self.mon_mut(pos).status = Status::Sleep(turns - 1);
                    self.add(format!("|cant|{}|slp", self.ident(pos)));
                    return false;
                }
            }
            Status::Freeze(turns) if !data.flags.contains_key("defrost") => {
                if turns <= 1 || self.rng.chance(1, 4) {
                    self.cure_status(pos, "[msg]");
                } else {
                    self.mon_mut(pos).status = Status::Freeze(turns - 1);
                    self.add(format!("|cant|{}|frz", self.ident(pos)));
                    return false;
                }
            }
            _ => {}
        }
        if self.mon(pos).flinched {
            self.add(format!("|cant|{}|flinch", self.ident(pos)));
            return false;
        }
        if self.imprisoned_move(pos, &data.name) {
            self.add(format!(
                "|cant|{}|move: Imprison|{}",
                self.ident(pos),
                data.name
            ));
            return false;
        }
        if self.mon(pos).status == Status::Paralysis && self.rng.chance(1, 8) {
            self.add(format!("|cant|{}|par", self.ident(pos)));
            return false;
        }
        true
    }

    pub(super) fn cure_status(&mut self, pos: Position, reason: &str) {
        let status = self.mon(pos).status;
        if self.mon(pos).hp != 0 && status != Status::Healthy {
            self.add(format!(
                "|-curestatus|{}|{}|{reason}",
                self.ident(pos),
                status.id()
            ));
            self.mon_mut(pos).status = Status::Healthy;
        }
    }

    pub(super) fn status_immune(&self, pos: Position, key: &str) -> bool {
        self.mon(pos)
            .species
            .types
            .iter()
            .any(|typing| DEX.type_data(&to_id(typing)).unwrap().damage_taken.get(key) == Some(&3))
    }

    pub(super) fn try_status(
        &mut self,
        pos: Position,
        id: &str,
        source: Position,
        data: &Move,
        primary: bool,
        move_log: usize,
    ) -> bool {
        let mon = self.mon(pos);
        if mon.hp == 0 {
            return false;
        }
        if mon.status != Status::Healthy {
            if primary {
                if mon.status.id() == id {
                    self.add(format!("|-fail|{}|{id}", self.ident(pos)));
                } else {
                    self.add(format!("|-fail|{}", self.ident(source)));
                    self.log[move_log].public =
                        format!("|move|{}|{}||[still]", self.ident(source), data.name);
                }
            }
            return false;
        }
        if self.status_immune(pos, if id == "tox" { "psn" } else { id }) {
            if primary {
                self.add(format!("|-immune|{}", self.ident(pos)));
            }
            return false;
        }

        let mut status = Status::from_id(id);
        let reason = if let Status::Sleep(_) = status {
            status = Status::Sleep(if self.rng.below(3) == 0 { 2 } else { 3 });
            format!("|[from] move: {}", data.name)
        } else {
            String::new()
        };
        self.mon_mut(pos).status = status;
        self.add(format!("|-status|{}|{id}{reason}", self.ident(pos)));
        self.cure_with_berry(pos);
        true
    }

    pub(super) fn status_residual(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        let (_, divisor, label) = mon.status.residual().unwrap();
        let mut amount = (mon.max_hp.get() / divisor).max(1);
        if let Status::Toxic(stage) = mon.status {
            let stage = (stage + 1).min(15);
            mon.status = Status::Toxic(stage);
            amount *= u32::from(stage);
        }
        mon.hp = mon.hp.saturating_sub(amount);
        self.add_health_line_with("-damage", pos, false, &format!("|[from] {label}"));
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
    }
}
