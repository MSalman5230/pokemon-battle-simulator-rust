//! Major status and flinch behavior, including the Champions timer overrides.

use super::turn::Position;
use super::*;
use crate::mechanics::BoostStat;

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
    pub(super) fn before_status_move(
        &mut self,
        pos: Position,
        data: &Move,
        z_powered: bool,
    ) -> bool {
        let status = self.mon(pos).status;
        match status {
            Status::Sleep(turns) => {
                let decrement = 1 + u8::from(self.mon(pos).active_ability() == "earlybird");
                if turns <= decrement {
                    self.cure_status(pos, "[msg]");
                } else {
                    self.mon_mut(pos).status = Status::Sleep(turns - decrement);
                    self.add(format!("|cant|{}|slp", self.ident(pos)));
                    if !matches!(data.name.as_str(), "Sleep Talk" | "Snore") {
                        return false;
                    }
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
            if self.event_ability(pos) == "steadfast" {
                self.ability_boost(pos, "Steadfast", BoostStat::Spe, 1, false);
            }
            return false;
        }
        if self.gravity > 0 && data.flags.contains_key("gravity") {
            self.add(format!(
                "|cant|{}|move: Gravity|{}",
                self.ident(pos),
                data.name
            ));
            return false;
        }
        if !z_powered
            && let Some((slot, _)) = self.mon(pos).disabled_slot
            && self
                .mon(pos)
                .moves
                .get(slot)
                .is_some_and(|m| m.move_data.name == data.name)
        {
            self.add(format!("|cant|{}|Disable|{}", self.ident(pos), data.name));
            return false;
        }
        if !z_powered && self.mon(pos).throat_chop > 0 && data.flags.contains_key("sound") {
            self.add(format!("|cant|{}|move: Throat Chop", self.ident(pos)));
            return false;
        }
        if !z_powered && self.mon(pos).heal_block > 0 && data.flags.contains_key("heal") {
            self.add(format!(
                "|cant|{}|move: Heal Block|{}",
                self.ident(pos),
                data.name
            ));
            return false;
        }
        if !z_powered
            && self.mon(pos).taunt > 0
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
        if !z_powered && self.imprisoned_move(pos, &data.name) {
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
            boosted_stat(
                if self.wonder_room > 0 {
                    mon.stats.spd
                } else {
                    mon.stats.def
                },
                mon.boosts.get(BoostStat::Def),
            ),
            self.stat_rules.truncation,
            &mut self.rng,
        );
        let damage = self.focus_survival(pos, damage, true);
        let mon = self.mon_mut(pos);
        // Showdown represents a confusion self-hit as single-hit Move damage.
        mon.half_hp_berry_deferred = matches!(mon.active_ability(), "berserk" | "angershell");
        mon.damage_hp(damage);
        self.add_health_line_with("-damage", pos, false, "|[from] confusion");
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
        false
    }

    pub(super) fn cure_status(&mut self, pos: Position, reason: &str) {
        let status = self.mon(pos).status;
        if self.mon(pos).hp != 0 && status != Status::Healthy {
            let ident = if self.active_effects(pos) {
                self.ident(pos)
            } else {
                format!("{}: {}", pos.player.id(), self.mon(pos).name)
            };
            self.add(format!("|-curestatus|{ident}|{}|{reason}", status.id()));
            if matches!(status, Status::Sleep(_)) {
                self.clear_nightmare(pos);
            }
            self.mon_mut(pos).status = Status::Healthy;
        }
    }

    pub(super) fn start_nightmare(&mut self, pos: Position) -> bool {
        let mon = self.mon(pos);
        if mon.volatile_ids().contains(&"nightmare")
            || (!matches!(mon.status, Status::Sleep(_)) && self.event_ability(pos) != "comatose")
        {
            return false;
        }
        self.mon_mut(pos).track_volatile("nightmare");
        self.add(format!("|-start|{}|Nightmare", self.ident(pos)));
        true
    }

    fn clear_nightmare(&mut self, pos: Position) {
        if self.mon(pos).volatile_ids().contains(&"nightmare") {
            self.mon_mut(pos).untrack_volatile("nightmare");
            self.add(format!("|-end|{}|Nightmare|[silent]", self.ident(pos)));
        }
    }

    pub(super) fn cure_team(&mut self, source: Position, data: &Move) -> bool {
        let aroma = data.name == "Aromatherapy";
        let mut allies = [(source, 0); 2];
        let mut count = 0;
        for ally in self.positions().filter(|pos| pos.player == source.player) {
            let ability = self.event_ability(ally);
            if (aroma && ability == "sapsipper") || (!aroma && ability == "soundproof") {
                allies[count] = (ally, self.mon(ally).speed);
                count += 1;
            }
        }
        speed_sort_by(&mut allies[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        for &(ally, _) in &allies[..count] {
            if !aroma {
                self.add(format!(
                    "|-immune|{}|[from] ability: Soundproof",
                    self.ident(ally)
                ));
            } else if ally != source {
                self.ability_boost(
                    ally,
                    "Sap Sipper",
                    crate::mechanics::BoostStat::Atk,
                    1,
                    false,
                );
            }
        }
        self.add(format!(
            "|-activate|{}|move: {}",
            self.ident(source),
            data.name
        ));
        let mut cured = false;
        for slot in 0..self.party(source.player).unwrap().len() {
            let ally = Position { slot, ..source };
            if ally != source {
                let blocked = match self.event_ability(ally) {
                    "soundproof" if !aroma => Some("Soundproof"),
                    "sapsipper" if aroma => Some("Sap Sipper"),
                    "goodasgold" => Some("Good as Gold"),
                    _ => None,
                };
                if let Some(ability) = blocked {
                    self.add(format!(
                        "|-immune|{}|[from] ability: {ability}",
                        self.ident(ally)
                    ));
                    continue;
                }
                if aroma && self.mon(ally).substitute_hp > 0 {
                    continue;
                }
            }
            if self.mon(ally).hp != 0 && self.mon(ally).status != Status::Healthy {
                self.cure_status(ally, "[msg]");
                cured = true;
            }
        }
        cured
    }

    pub(super) fn status_immune(&self, pos: Position, key: &str) -> bool {
        let mon = self.mon(pos);
        if mon.semi_invulnerable() && matches!(key, "sandstorm" | "hail") {
            return true;
        }
        (matches!(key, "powder" | "sandstorm" | "hail")
            && (mon.effective_item() == "safetygoggles" || self.event_ability(pos) == "overcoat"))
            || mon.types().filter(|ty| *ty != "???").any(|typing| {
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
        let corrosion =
            matches!(id, "tox" | "psn") && self.mon(source).active_ability() == "corrosion";
        if !corrosion && self.status_immune(pos, if id == "tox" { "psn" } else { id }) {
            if self.format.report_exact_hp {
                self.add("|debug|natural status immunity");
                self.add("|debug|immune to status");
            }
            if primary {
                self.add(format!("|-immune|{}", self.ident(pos)));
            }
            return false;
        }
        if id == "frz"
            && ((self.effective_weather().map(Weather::ordinary) == Some(Weather::Sun)
                && self.mon(pos).effective_item() != "utilityumbrella")
                || self.event_ability(pos) == "magmaarmor")
        {
            if self.format.report_exact_hp {
                self.add("|debug|artificial status immunity");
                self.add("|debug|immune to status");
            }
            if primary {
                self.add(format!("|-immune|{}", self.ident(pos)));
            }
            return false;
        }

        let prevented = self.status_blocked_by_ability(pos, id, primary)
            || self.flower_veil_status(pos, source, id, primary && data.secondaries.is_empty());
        if prevented
            || self.field_blocks_status(pos, id, source.player, primary)
            || self.safeguard_blocks(
                pos,
                source,
                true,
                primary && data.secondaries.is_empty(),
                false,
            )
        {
            return false;
        }
        // Declarative sleep secondaries pass the active move as their cause.
        // Callback-based inflictions, such as Dire Claw, supply their own effect.
        let from_move = primary
            || id == "slp"
                && data
                    .secondaries
                    .iter()
                    .any(|effect| effect.status.as_deref() == Some(id));
        self.set_status(pos, id, source.player, from_move.then_some(&data.name));
        if !self.synchronize_status(pos, source, id, move_log) {
            self.cure_with_berry(pos);
        }
        true
    }

    pub(super) fn synchronize_status(
        &mut self,
        target: Position,
        source: Position,
        id: &str,
        move_log: usize,
    ) -> bool {
        if target == source
            || matches!(id, "slp" | "frz")
            || self.event_ability(target) != "synchronize"
        {
            return false;
        }
        self.add(format!(
            "|-activate|{}|ability: Synchronize",
            self.ident(target)
        ));
        let source_contents = self.mon(source);
        if source_contents.hp == 0 || source_contents.fainted {
            return true;
        }
        if source_contents.status.id() == id {
            self.add(format!("|-fail|{}|{id}", self.ident(source)));
            return true;
        }
        if source_contents.status != Status::Healthy {
            self.add(format!("|-fail|{}", self.ident(target)));
            self.move_still(move_log);
            return true;
        }
        if self.status_immune(source, if id == "tox" { "psn" } else { id }) {
            if self.format.report_exact_hp {
                self.add("|debug|natural status immunity");
                self.add("|debug|immune to status");
            }
            self.add(format!("|-immune|{}", self.ident(source)));
            return true;
        }
        if !self.status_blocked_by_ability(source, id, true)
            && !self.flower_veil_status(source, target, id, true)
            && !self.field_blocks_status(source, id, target.player, true)
            && !self.safeguard_blocks(source, target, false, true, false)
        {
            self.set_status(source, id, target.player, None);
            // Two Synchronize holders both activate. The original recipient
            // already has the reflected status, which ends the chain.
            self.synchronize_status(source, target, id, move_log);
        }
        true
    }

    pub(super) fn status_blocked_by_ability(
        &mut self,
        pos: Position,
        id: &str,
        reveal: bool,
    ) -> bool {
        if id == "slp" && self.sweet_veil_blocks(pos, false) {
            return true;
        }
        let label = match (self.event_ability(pos), id) {
            ("immunity", "psn" | "tox") => "Immunity",
            ("thermalexchange", "brn") => "Thermal Exchange",
            ("limber", "par") => "Limber",
            ("waterbubble", "brn") => "Water Bubble",
            ("purifyingsalt", _) => "Purifying Salt",
            ("comatose", _) => "Comatose",
            ("insomnia", "slp") => "Insomnia",
            ("vitalspirit", "slp") => "Vital Spirit",
            ("leafguard", _)
                if self
                    .mon(pos)
                    .personal_weather(self.effective_weather())
                    .map(Weather::ordinary)
                    == Some(Weather::Sun) =>
            {
                "Leaf Guard"
            }
            _ => return false,
        };
        if reveal {
            self.add(format!(
                "|-immune|{}|[from] ability: {label}",
                self.ident(pos)
            ));
        }
        if self.format.report_exact_hp {
            if !matches!(
                label,
                "Limber" | "Water Bubble" | "Insomnia" | "Vital Spirit" | "Leaf Guard" | "Comatose"
            ) {
                self.add(format!("|debug|interrupting setStatus with {label}"));
            }
            self.add(format!("|debug|set status [{id}] interrupted"));
        }
        true
    }

    pub(super) fn yawn_blocked_by_ability(&mut self, pos: Position) -> bool {
        if self.sweet_veil_blocks(pos, true) {
            return true;
        }
        let label = match self.event_ability(pos) {
            "purifyingsalt" => "Purifying Salt",
            "insomnia" => "Insomnia",
            "vitalspirit" => "Vital Spirit",
            "leafguard"
                if self
                    .mon(pos)
                    .personal_weather(self.effective_weather())
                    .map(Weather::ordinary)
                    == Some(Weather::Sun) =>
            {
                "Leaf Guard"
            }
            _ => return false,
        };
        self.add(format!(
            "|-immune|{}|[from] ability: {label}",
            self.ident(pos)
        ));
        if label == "Purifying Salt" && self.format.report_exact_hp {
            self.add("|debug|interrupting tryAddVolatile with Purifying Salt");
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
        (id == "slp" && self.uproar_blocks_sleep(pos))
            || self.format.has_rule("sleepclausemod") && self.sleep_clause_blocks(pos, id, source)
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
        let reason = if id == "slp" {
            move_name.map_or_else(String::new, |name| format!("|[from] move: {name}"))
        } else {
            String::new()
        };
        self.set_status_with_suffix(pos, id, source, &reason);
    }

    pub(super) fn set_status_with_suffix(
        &mut self,
        pos: Position,
        id: &str,
        source: Player,
        reason: &str,
    ) {
        let mut status = Status::from_id(id);
        if let Status::Sleep(_) = status {
            status = Status::Sleep(if self.rng.below(3) == 0 { 2 } else { 3 });
        }
        let mon = self.mon_mut(pos);
        mon.status = status;
        mon.slept_by_foe = id == "slp" && source != pos.player;
        self.add(format!("|-status|{}|{id}{reason}", self.ident(pos)));
        if id == "slp" {
            self.clear_nightmare(pos);
        }
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
        if mon.active_ability() == "poisonheal"
            && matches!(mon.status, Status::Poison | Status::Toxic(_))
        {
            self.ability_heal(pos, 8, "Poison Heal", None);
            if self.format.report_exact_hp {
                self.add("|debug|damage event failed");
            }
            return;
        }
        if mon.active_ability() == "magicguard" {
            return;
        }
        if mon.status == Status::Burn && mon.active_ability() == "heatproof" {
            amount = (amount / 2).max(1);
        }
        let amount = self.focus_survival(pos, amount, false);
        let mon = self.mon_mut(pos);
        mon.half_hp_berry_deferred = false;
        mon.damage_hp(amount);
        self.add_health_line_with("-damage", pos, false, &format!("|[from] {label}"));
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
    }

    pub(super) fn activate_status_orb(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let (status, id, immunity, item) = match mon.effective_item() {
            "toxicorb" => (Status::Toxic(0), "tox", "psn", "Toxic Orb"),
            "flameorb" => (Status::Burn, "brn", "brn", "Flame Orb"),
            _ => return,
        };
        if mon.hp == 0
            || mon.status != Status::Healthy
            || self.status_immune(pos, immunity)
            || self.status_blocked_by_ability(pos, id, false)
            || self.field_blocks_status(pos, id, pos.player, false)
        {
            return;
        }
        let mon = self.mon_mut(pos);
        mon.status = status;
        mon.slept_by_foe = false;
        self.add(format!(
            "|-status|{}|{id}|[from] item: {item}",
            self.ident(pos)
        ));
    }
}
