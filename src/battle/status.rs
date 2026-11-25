//! Major status and flinch behavior, including the Champions timer overrides.

use super::turn::Position;
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Yawn {
    pub(super) turns: u8,
    pub(super) source: Player,
}

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

    pub(super) fn from_id(id: &str) -> Self {
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
        if let Some((slot, _)) = self.mon(pos).disabled_slot
            && self.mon(pos).moves[slot].move_data.name == data.name
        {
            self.add(format!("|cant|{}|Disable|{}", self.ident(pos), data.name));
            return false;
        }
        if self.mon(pos).taunt > 0
            && data.category == crate::dex::Category::Status
            && data.name != "Me First"
        {
            self.add(format!(
                "|cant|{}|move: Taunt|{}",
                self.ident(pos),
                data.name
            ));
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
        if !self.before_confusion_move(pos) {
            return false;
        }
        if let Some(source) = self.attract_source(pos) {
            self.add(format!(
                "|-activate|{}|move: Attract|[of] {}",
                self.ident(pos),
                self.ident(source)
            ));
            if self.rng.chance(1, 2) {
                self.add(format!("|cant|{}|Attract", self.ident(pos)));
                return false;
            }
        }
        if self.mon(pos).status == Status::Paralysis && self.rng.chance(1, 8) {
            self.add(format!("|cant|{}|par", self.ident(pos)));
            return false;
        }
        true
    }

    pub(super) fn apply_confusion(&mut self, pos: Position, fatigue: bool) -> bool {
        if self.mon(pos).hp == 0
            || self.event_ability(pos) == "owntempo"
            || self.mon(pos).confusion > 0
            || (self.terrain() == Some(Terrain::Misty)
                && self.grounded(pos)
                && !self.mon(pos).semi_invulnerable())
        {
            return false;
        }
        let reason = if fatigue { "|[fatigue]" } else { "" };
        self.add(format!("|-start|{}|confusion{reason}", self.ident(pos)));
        let turns = 2 + self.rng.below(4) as u8;
        let mon = self.mon_mut(pos);
        mon.confusion = turns;
        mon.track_volatile("confusion");
        true
    }

    pub(super) fn cure_confusion(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        mon.confusion = 0;
        mon.untrack_volatile("confusion");
        self.add(format!("|-end|{}|confusion", self.ident(pos)));
    }

    fn before_confusion_move(&mut self, pos: Position) -> bool {
        use crate::mechanics::{BoostStat, boosted_stat, confusion_damage};
        let mon = self.mon_mut(pos);
        if mon.confusion == 0 {
            return true;
        }
        mon.confusion -= 1;
        if mon.confusion == 0 {
            self.cure_confusion(pos);
            return true;
        }
        self.add(format!("|-activate|{}|confusion", self.ident(pos)));
        if !self.rng.chance(33, 100) {
            return true;
        }
        let mon = self.mon(pos);
        let damage = confusion_damage(
            mon.level,
            boosted_stat(mon.stats.atk, mon.boosts.get(BoostStat::Atk)),
            boosted_stat(mon.stats.def, mon.boosts.get(BoostStat::Def)),
            self.stat_rules.truncation,
            &mut self.rng,
        );
        let mon = self.mon_mut(pos);
        mon.hp = mon.hp.saturating_sub(damage);
        self.add_health_line_with("-damage", pos, false, "|[from] confusion");
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
        false
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
        let mon = self.mon(pos);
        if mon.semi_invulnerable() && matches!(key, "sandstorm" | "hail") {
            return true;
        }
        (matches!(key, "powder" | "sandstorm")
            && (mon.item == "safetygoggles" || self.event_ability(pos) == "overcoat"))
            || mon.types().any(|typing| {
                DEX.type_data(&to_id(typing)).unwrap().damage_taken.get(key) == Some(&3)
            })
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
                    self.move_still(move_log);
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

        let prevented = self.status_blocked_by_ability(pos, id, primary)
            || self.flower_veil_status(pos, source, id, primary && data.secondaries.is_empty());
        if prevented || self.field_blocks_status(pos, id, source.player, primary) {
            return false;
        }
        self.apply_status(pos, id, source.player, Some(&data.name));
        true
    }

    pub(super) fn status_blocked_by_ability(
        &mut self,
        pos: Position,
        id: &str,
        reveal: bool,
    ) -> bool {
        let label = match (self.event_ability(pos), id) {
            ("immunity", "psn" | "tox") => "Immunity",
            ("thermalexchange", "brn") => "Thermal Exchange",
            ("purifyingsalt", _) => "Purifying Salt",
            _ => return false,
        };
        if reveal {
            self.add(format!(
                "|-immune|{}|[from] ability: {label}",
                self.ident(pos)
            ));
        }
        if self.format.report_exact_hp {
            self.add(format!("|debug|interrupting setStatus with {label}"));
            self.add(format!("|debug|set status [{id}] interrupted"));
        }
        true
    }

    pub(super) fn field_blocks_status(
        &mut self,
        pos: Position,
        id: &str,
        source: Player,
        reveal_terrain: bool,
    ) -> bool {
        // Terrain effects have sub-order 0, before the format rule at 5.
        if let Some(terrain) = self.status_blocked_by_terrain(pos, id) {
            if reveal_terrain {
                self.terrain_activate(pos, terrain);
            }
            return true;
        }
        self.format.has_rule("sleepclausemod") && self.sleep_clause_blocks(pos, id, source)
    }

    fn sleep_clause_blocks(&mut self, pos: Position, id: &str, source: Player) -> bool {
        if id != "slp"
            || pos.player == source
            || !self.party(pos.player).unwrap().iter().any(|pokemon| {
                pokemon.hp != 0
                    && pokemon.slept_by_foe
                    && matches!(pokemon.status, Status::Sleep(_))
            })
        {
            return false;
        }
        self.add("|-message|Sleep Clause Mod activated.");
        self.add("|-hint|Sleep Clause Mod prevents players from putting more than one of their opponent's Pokemon to sleep at a time");
        true
    }

    pub(super) fn apply_status(
        &mut self,
        pos: Position,
        id: &str,
        source: Player,
        move_name: Option<&str>,
    ) {
        self.set_status(pos, id, source, move_name);
        self.cure_with_berry(pos);
    }

    pub(super) fn set_status(
        &mut self,
        pos: Position,
        id: &str,
        source: Player,
        move_name: Option<&str>,
    ) {
        let mut status = Status::from_id(id);
        let reason = if let Status::Sleep(_) = status {
            status = Status::Sleep(if self.rng.below(3) == 0 { 2 } else { 3 });
            move_name.map_or_else(String::new, |name| format!("|[from] move: {name}"))
        } else {
            String::new()
        };
        let mon = self.mon_mut(pos);
        mon.status = status;
        mon.slept_by_foe = id == "slp" && source != pos.player;
        self.add(format!("|-status|{}|{id}{reason}", self.ident(pos)));
    }

    pub(super) fn tick_yawn(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        let Some(yawn) = &mut mon.yawn else {
            return;
        };
        yawn.turns -= 1;
        if yawn.turns != 0 {
            return;
        }
        let source = yawn.source;
        mon.yawn = None;
        mon.untrack_volatile("yawn");
        self.add(format!("|-end|{}|move: Yawn|[silent]", self.ident(pos)));
        if self.mon(pos).status == Status::Healthy
            && !self.status_immune(pos, "slp")
            && !self.status_blocked_by_ability(pos, "slp", false)
            && !self.field_blocks_status(pos, "slp", source, true)
        {
            self.apply_status(pos, "slp", source, None);
        }
    }

    pub(super) fn status_residual(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        let Some((_, divisor, label)) = mon.status.residual() else {
            // Earlier residual hooks can cure a status before its queued turn.
            return;
        };
        let mut amount = (mon.max_hp.get() / divisor).max(1);
        if let Status::Toxic(stage) = mon.status {
            let stage = (stage + 1).min(15);
            mon.status = Status::Toxic(stage);
            amount *= u32::from(stage);
        }
        if mon.ability == "magicguard" {
            return;
        }
        if mon.status == Status::Burn && mon.ability == "heatproof" {
            amount = (amount / 2).max(1);
        }
        mon.hp = mon.hp.saturating_sub(amount);
        self.add_health_line_with("-damage", pos, false, &format!("|[from] {label}"));
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
    }
}
