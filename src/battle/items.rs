//! Held-item hooks for the currently supported damage and status pipeline.

use super::terrain::Terrain;
use super::turn::{BoostLabel, Pivot, Position};
use super::*;
use crate::{
    dex::{FlingEffect, Item},
    mechanics::BoostStat,
};

#[derive(Clone, Copy)]
pub(super) struct CudChew {
    item: &'static crate::dex::Item,
    ticks: u8,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Metronome {
    last_move: Option<&'static Move>,
    consecutive: u8,
    pub last_turn_succeeded: bool,
    pub this_turn_succeeded: bool,
}

impl Metronome {
    pub(super) fn damage_factor(self) -> u32 {
        [4096, 4915, 5734, 6553, 7372, 8192][usize::from(self.consecutive)]
    }
}

pub(super) fn resistance_berry(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "chilanberry" => ("Normal", "Chilan Berry"),
        "occaberry" => ("Fire", "Occa Berry"),
        "passhoberry" => ("Water", "Passho Berry"),
        "wacanberry" => ("Electric", "Wacan Berry"),
        "rindoberry" => ("Grass", "Rindo Berry"),
        "yacheberry" => ("Ice", "Yache Berry"),
        "chopleberry" => ("Fighting", "Chople Berry"),
        "kebiaberry" => ("Poison", "Kebia Berry"),
        "shucaberry" => ("Ground", "Shuca Berry"),
        "cobaberry" => ("Flying", "Coba Berry"),
        "payapaberry" => ("Psychic", "Payapa Berry"),
        "tangaberry" => ("Bug", "Tanga Berry"),
        "chartiberry" => ("Rock", "Charti Berry"),
        "kasibberry" => ("Ghost", "Kasib Berry"),
        "habanberry" => ("Dragon", "Haban Berry"),
        "colburberry" => ("Dark", "Colbur Berry"),
        "babiriberry" => ("Steel", "Babiri Berry"),
        "roseliberry" => ("Fairy", "Roseli Berry"),
        _ => return None,
    })
}

fn stat_berry(id: &str) -> Option<(&'static str, BoostStat)> {
    Some(match id {
        "liechiberry" => ("Liechi Berry", BoostStat::Atk),
        "ganlonberry" => ("Ganlon Berry", BoostStat::Def),
        "salacberry" => ("Salac Berry", BoostStat::Spe),
        "petayaberry" => ("Petaya Berry", BoostStat::Spa),
        "apicotberry" => ("Apicot Berry", BoostStat::Spd),
        _ => return None,
    })
}

fn status_berry_matches(id: &str, status: Status) -> bool {
    matches!(
        (id, status),
        ("aspearberry", Status::Freeze(_))
            | ("cheriberry", Status::Paralysis)
            | ("pechaberry", Status::Poison | Status::Toxic(_))
            | ("rawstberry", Status::Burn)
    )
}

impl Battle {
    pub(super) fn prepare_accuracy_modifiers(
        &mut self,
        source: Position,
        target: Position,
        numeric: bool,
        zoom: bool,
    ) {
        let ability = self.event_ability(source);
        let compound = ability == "compoundeyes";
        let active_handler = compound || ability == "hustle";
        if !active_handler && !self.format.report_exact_hp {
            return;
        }
        let evasion_handler = matches!(self.event_ability(target), "snowcloak" | "sandveil");
        let mut evasion_first = true;
        if active_handler && evasion_handler {
            let target_speed = self.mon(target).speed;
            let source_speed = self.mon(source).speed;
            evasion_first = if target_speed == source_speed {
                self.rng.below(2) == 0
            } else {
                target_speed > source_speed
            };
        }
        if self.format.report_exact_hp && numeric {
            let (weather, snow) = self.active_weather_evasion(target);
            let evasion_debug = if snow {
                "|debug|Snow Cloak - decreasing accuracy"
            } else {
                "|debug|Sand Veil - decreasing accuracy"
            };
            if weather && evasion_first {
                self.add(evasion_debug);
            }
            if compound {
                self.add("|debug|compoundeyes - enhancing accuracy");
            }
            if weather && !evasion_first {
                self.add(evasion_debug);
            }
            if self.tangled_feet(target) {
                self.add("|debug|Tangled Feet - decreasing accuracy");
            }
            if zoom {
                self.add("|debug|Zoom Lens boosting accuracy");
            }
            if self.active_effects(target) {
                match self.mon(target).effective_item() {
                    "brightpowder" => self.add("|debug|brightpowder - decreasing accuracy"),
                    "laxincense" => self.add("|debug|lax incense - decreasing accuracy"),
                    _ => {}
                }
            }
        }
        if matches!(self.mon(source).item.as_str(), "widelens" | "zoomlens") {
            self.debug_ignored_item(source, "ModifyAccuracy");
        }
        if matches!(
            self.mon(target).item.as_str(),
            "brightpowder" | "laxincense"
        ) {
            self.debug_ignored_item(target, "ModifyAccuracy");
        }
    }

    pub(super) fn accuracy_modifier(
        &self,
        source: Position,
        target: Position,
        weather_evasion: bool,
        category: crate::dex::Category,
        zoom: bool,
    ) -> u32 {
        let mut modifier = if weather_evasion { 3277 } else { 4096 };
        match self.event_ability(source) {
            "compoundeyes" => modifier = crate::mechanics::chain(modifier, 5325),
            "hustle" if category == crate::dex::Category::Physical => {
                modifier = crate::mechanics::chain(modifier, 3277);
            }
            _ => {}
        }
        if self.gravity > 0 {
            modifier = crate::mechanics::chain(modifier, 6840);
        }
        if self.tangled_feet(target) {
            modifier = crate::mechanics::chain(modifier, 2048);
        }
        if zoom {
            modifier = crate::mechanics::chain(modifier, 4915);
        } else if self.mon(source).effective_item() == "widelens" && self.active_effects(source) {
            modifier = crate::mechanics::chain(modifier, 4505);
        }
        if matches!(
            self.mon(target).effective_item(),
            "brightpowder" | "laxincense"
        ) && self.active_effects(target)
        {
            modifier = crate::mechanics::chain(modifier, 3686);
        }
        modifier
    }

    fn tangled_feet(&self, target: Position) -> bool {
        self.mon(target).confusion != 0 && self.event_ability(target) == "tangledfeet"
    }

    pub(super) fn on_switch_in_item(&mut self, pos: Position) {
        if self.mon(pos).item == "metronome" && self.mon(pos).metronome.is_none() {
            let mon = self.mon_mut(pos);
            mon.metronome = Some(Metronome::default());
            mon.track_volatile("metronome");
        }
        if self.mon(pos).item == "airballoon" && !self.debug_ignored_item(pos, "SwitchIn") {
            self.add(format!("|-item|{}|Air Balloon", self.ident(pos)));
        }
    }

    pub(super) fn metronome_try_move(&mut self, pos: Position, data: &'static Move) {
        let mon = self.mon_mut(pos);
        if mon.metronome.is_none() {
            return;
        }
        if mon.effective_item() != "metronome" {
            mon.metronome = None;
            mon.untrack_volatile("metronome");
            return;
        }
        if data.calls_move {
            return;
        }
        let state = mon.metronome.as_mut().unwrap();
        let same = state
            .last_move
            .is_some_and(|previous| previous.name == data.name);
        state.consecutive = if same && (state.last_turn_succeeded || mon.charge.is_some()) {
            state.consecutive.saturating_add(1).min(5)
        } else {
            u8::from(mon.charge.is_some())
        };
        state.last_move = Some(data);
    }

    pub(super) fn debug_ignored_item(&mut self, pos: Position, event: &str) -> bool {
        if !self.mon(pos).ignores_held_item() {
            return false;
        }
        if self.format.report_exact_hp {
            self.add(format!(
                "|debug|{event} handler suppressed by Embargo, Klutz or Magic Room"
            ));
        }
        true
    }

    fn on_gained_item(&mut self, pos: Position) {
        if self.mon(pos).holds_choice_item() && self.mon(pos).choice_lock.is_some() {
            if self.format.report_exact_hp {
                self.add("|debug|removing choicelock");
            }
            let mon = self.mon_mut(pos);
            mon.choice_lock = None;
            mon.untrack_volatile("choicelock");
        }
        self.on_switch_in_item(pos);
        self.use_matching_terrain_seed(pos);
        self.white_herb(pos);
    }

    pub(super) fn use_matching_terrain_seed(&mut self, pos: Position) {
        let (terrain, name, stat) = match self.mon(pos).effective_item() {
            "electricseed" => (Terrain::Electric, "Electric Seed", BoostStat::Def),
            "grassyseed" => (Terrain::Grassy, "Grassy Seed", BoostStat::Def),
            "mistyseed" => (Terrain::Misty, "Misty Seed", BoostStat::Spd),
            "psychicseed" => (Terrain::Psychic, "Psychic Seed", BoostStat::Spd),
            _ => return,
        };
        if self.terrain() != Some(terrain) || self.mon(pos).hp == 0 {
            return;
        }
        self.consume(pos, name, false);
        let (amount, invert) = match self.event_ability(pos) {
            "simple" => (2, false),
            "contrary" => (1, true),
            _ => (1, false),
        };
        self.apply_boosts_labeled(
            pos,
            &[(stat, amount)],
            false,
            invert,
            Some(BoostLabel::Item(name)),
            true,
        );
        self.symbiosis_after_item(pos);
    }

    pub(super) fn white_herb(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.effective_item() != "whiteherb" || !mon.boosts.clear_negative() {
            return;
        }
        self.consume(pos, "White Herb", false);
        self.add(format!("|-clearnegativeboost|{}|[silent]", self.ident(pos)));
        self.symbiosis_after_item(pos);
    }

    pub(super) fn white_herbs_after_action(&mut self) {
        let mut holders = [(
            Position {
                player: Player::P1,
                slot: 0,
            },
            0,
        ); 4];
        let mut count = 0;
        for pos in self.positions() {
            let mon = self.mon(pos);
            if !mon.fainted && mon.item == "whiteherb" {
                holders[count] = (pos, mon.speed);
                count += 1;
            }
        }
        let holders = &mut holders[..count];
        speed_sort_by(holders, &mut self.rng, |a, b| b.1.cmp(&a.1));
        for &(pos, _) in holders.iter() {
            self.white_herb(pos);
        }
    }

    pub(super) fn focus_survival(&mut self, pos: Position, damage: u32, move_damage: bool) -> u32 {
        if !matches!(self.mon(pos).item.as_str(), "focussash" | "focusband")
            || self.debug_ignored_item(pos, "Damage")
        {
            return damage;
        }
        if self.mon(pos).item == "focusband" {
            // Native Focus Band checks its roll even on nonfatal and indirect damage.
            if self.rng.chance(1, 10) && move_damage && damage >= self.mon(pos).hp {
                self.add(format!("|-activate|{}|item: Focus Band", self.ident(pos)));
                return self.mon(pos).hp - 1;
            }
            return damage;
        }
        let mon = self.mon(pos);
        if move_damage && mon.hp == mon.max_hp.get() && damage >= mon.hp {
            let remaining = mon.hp - 1;
            self.consume(pos, "Focus Sash", false);
            self.symbiosis_after_item(pos);
            remaining
        } else {
            damage
        }
    }

    pub(super) fn on_update_item(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp > 0 && mon.effective_item() == "mentalherb" {
            self.mental_herb(pos);
            return;
        }
        if mon.hp > 0
            && mon.hp * 2 <= mon.max_hp.get()
            && mon.heal_block == 0
            && mon.effective_item() == "berryjuice"
        {
            if self.event_ability(pos) == "ripen" {
                self.add(format!("|-activate|{}|ability: Ripen", self.ident(pos)));
            }
            self.consume(pos, "Berry Juice", false);
            self.item_heal(pos, 20, "Berry Juice");
            self.symbiosis_after_item(pos);
        }
        let mon = self.mon(pos);
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        if mon.effective_item() == "sitrusberry"
            && mon.hp * 2 <= mon.max_hp.get()
            && mon.heal_block == 0
            && (!matches!(self.event_ability(pos), "berserk" | "angershell")
                || !mon.half_hp_berry_deferred)
        {
            let amount = mon.max_hp.get() / 4;
            self.consume(pos, "Sitrus Berry", true);
            self.item_heal(pos, amount, "Sitrus Berry");
            self.after_eating_berry(pos, "Sitrus Berry", false);
            self.symbiosis_after_item(pos);
        }
        self.pinch_stat_berry(pos);
        self.cure_with_berry(pos);
        self.check_other_berries(pos);
    }

    fn check_other_berries(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let id = mon.effective_item();
        let ready = match id {
            "oranberry" => {
                mon.hp * 2 <= mon.max_hp.get()
                    && mon.heal_block == 0
                    && (!matches!(self.event_ability(pos), "berserk" | "angershell")
                        || !mon.half_hp_berry_deferred)
            }
            "persimberry" => mon.confusion > 0,
            "leppaberry" => mon.moves.iter().any(|slot| slot.pp == 0),
            _ => status_berry_matches(id, mon.status),
        };
        if !ready || mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        let item = DEX.item(self.format.dex_mod, id).unwrap();
        let id = id.to_owned();
        let name = &item.effect.name;
        self.consume(pos, name, true);
        self.berry_effect(pos, &id, item, None);
        self.after_eating_berry(pos, name, false);
        self.symbiosis_after_item(pos);
    }

    pub(super) fn has_held_berry(&self, pos: Position) -> bool {
        DEX.item(self.format.dex_mod, &self.mon(pos).item)
            .is_some_and(|item| item.is_berry)
    }

    pub(super) fn force_eat_berry(&mut self, target: Position, source: Position) {
        let id = &self.mon(target).item;
        let Some(item) = DEX
            .item(self.format.dex_mod, id)
            .filter(|item| item.is_berry)
        else {
            return;
        };
        if self.mon(target).hp == 0 {
            return;
        }
        let resisted = resistance_berry(id).is_some();
        let id = id.clone();
        let name = &item.effect.name;
        // A forced meal skips TryEatItem, including the Ripen announcement
        // and Unnerve, but still runs the berry's Eat and the EatItem hooks.
        self.consume_with_reason(target, name, true, "|[eat]");
        if !self.debug_ignored_item(target, "Eat") {
            self.berry_effect(target, &id, item, Some(source));
        }
        self.after_eating_berry_from(target, name, false, Some(source));
        if self.event_ability(target) == "ripen" {
            self.mon_mut(target).ripen_pending = resisted;
        }
        self.symbiosis_after_item(target);
    }

    pub(super) fn corrode_item(&mut self, source: Position, target: Position) {
        if let Some(item) = self.can_take_item(target) {
            self.lose_held_item(target);
            self.add(format!(
                "|-enditem|{}|{item}|[from] move: Corrosive Gas|[of] {}",
                self.ident(target),
                self.ident(source)
            ));
        } else {
            if !self.mon(target).item.is_empty()
                && self.mon(target).hp != 0
                && self.event_ability(target) == "stickyhold"
            {
                self.add(format!(
                    "|-activate|{}|ability: Sticky Hold",
                    self.ident(target)
                ));
            }
            self.add(format!("|-fail|{}|move: Corrosive Gas", self.ident(target)));
        }
    }

    pub(super) fn consume_normal_gem(&mut self, source: Position, move_name: &str) {
        if self.mon(source).effective_item() == "normalgem" {
            self.consume_with_reason(
                source,
                "Normal Gem",
                false,
                &format!("|[from] gem|[move] {move_name}"),
            );
            self.symbiosis_after_item(source);
            let mon = self.mon_mut(source);
            mon.gem_boost = true;
            mon.track_volatile("gem");
        }
    }

    pub(super) fn pay_life_orb_recoil(&mut self, source: Position) {
        if self.drag_flags & source.mask() == 0
            && self.mon(source).effective_item() == "lifeorb"
            && self.active_effects(source)
        {
            self.indirect_damage(source, 10, "|[from] item: Life Orb");
            if self.mon(source).hp == 0 {
                self.mark_fainted(source);
            }
        }
    }

    pub(super) fn shell_bell_after_move(
        &mut self,
        source: Position,
        target: Option<Position>,
        total: u32,
    ) {
        if self.mon(source).item == "shellbell"
            && self.active_effects(source)
            && !self.debug_ignored_item(source, "AfterMoveSecondarySelf")
            && self.drag_flags & source.mask() == 0
            && total > 0
        {
            self.item_heal_from(source, total / 8, "Shell Bell", target);
        }
    }

    pub(super) fn cure_with_berry(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp == 0 || self.foe_unnerve(pos) {
            return;
        }
        let berry = match (mon.effective_item(), mon.status) {
            ("lumberry", _) if mon.confusion > 0 => "Lum Berry",
            (_, Status::Healthy | Status::Fainted) => return,
            ("lumberry", _) => "Lum Berry",
            ("chestoberry", Status::Sleep(_)) => "Chesto Berry",
            _ => return,
        };
        self.consume(pos, berry, true);
        self.cure_status(pos, "[msg]");
        if berry == "Lum Berry" && self.mon(pos).confusion > 0 {
            self.cure_confusion(pos);
        }
        self.after_eating_berry(pos, berry, false);
        self.symbiosis_after_item(pos);
    }

    fn pinch_stat_berry(&mut self, pos: Position) {
        let mon = self.mon(pos);
        let Some((name, stat)) = stat_berry(mon.effective_item()) else {
            return;
        };
        if mon.hp
            * if self.event_ability(pos) == "gluttony" {
                2
            } else {
                4
            }
            > mon.max_hp.get()
        {
            return;
        }
        self.consume(pos, name, true);
        self.apply_stat_berry(pos, name, stat);
        self.after_eating_berry(pos, name, false);
        self.symbiosis_after_item(pos);
    }

    fn apply_stat_berry(&mut self, pos: Position, name: &str, stat: BoostStat) {
        let amount = match self.event_ability(pos) {
            "contrary" => -1,
            "ripen" | "simple" => 2,
            _ => 1,
        };
        let mon = self.mon_mut(pos);
        let changed = mon.boosts.add(stat, amount);
        let reason = if changed != 0 {
            mon.stats_raised |= changed > 0;
            mon.stats_lowered |= changed < 0;
            format!("|[from] item: {name}")
        } else {
            String::new()
        };
        self.add(format!(
            "|{}|{}|{}|{}{reason}",
            if amount < 0 { "-unboost" } else { "-boost" },
            self.ident(pos),
            stat.id(),
            changed.abs(),
        ));
        self.note_foe_boost(pos, stat, changed);
    }

    pub(super) fn eat_stolen_berry(&mut self, source: Position, target: Position, move_name: &str) {
        if self.mon(source).hp == 0 {
            return;
        }
        let Some(item) = DEX
            .item(self.format.dex_mod, &self.mon(target).item)
            .filter(|item| item.is_berry)
        else {
            return;
        };
        if self.can_take_item(target).is_none() {
            if self.event_ability(target) == "stickyhold" && self.mon(target).hp > 0 {
                self.add(format!(
                    "|-activate|{}|ability: Sticky Hold",
                    self.ident(target)
                ));
            }
            return;
        }
        let id = self.lose_held_item(target);
        let name = &item.effect.name;
        self.add(format!(
            "|-enditem|{}|{name}|[from] stealeat|[move] {move_name}|[of] {}",
            self.ident(target),
            self.ident(source)
        ));
        // Pluck and Bug Bite invoke Eat directly, without replacing the
        // user's held item or recording a consumed item for Recycle.
        if !self.debug_ignored_item(source, "Eat") {
            self.berry_effect(source, &id, item, None);
            self.after_eating_berry(source, name, true);
        }
        if self.event_ability(source) == "ripen" {
            self.mon_mut(source).ripen_pending = resistance_berry(&id).is_some();
        }
        if item.effect.handlers.iter().any(|hook| hook == "onEat") {
            self.mon_mut(source).ate_berry = true;
        }
    }

    pub(super) fn prepare_fling(&mut self, pos: Position) -> Option<&'static Item> {
        let mon = self.mon(pos);
        if mon.items_suppressed
            || mon.under(super::field_effects::LegacyCondition::Embargo)
            || self.event_ability(pos) == "klutz"
        {
            return None;
        }
        let item = mon.transferable_item(self.format.dex_mod)?;
        let power = item.fling.as_ref()?.base_power;
        if self.format.report_exact_hp {
            self.add(format!("|debug|BP: {power}"));
        }
        if item.is_berry && self.event_ability(pos) == "cudchew" {
            self.after_eating_berry(pos, &item.effect.name, false);
        }
        self.mon_mut(pos).fling_pending = true;
        Some(item)
    }

    pub(super) fn fling_hit(&mut self, source: Position, target: Position, item: &'static Item) {
        if item.is_berry {
            let name = &item.effect.name;
            if !self.debug_ignored_item(target, "Eat") {
                self.berry_effect(target, &to_id(name), item, Some(source));
                self.after_eating_berry_from(target, name, false, Some(source));
            }
            if item.effect.handlers.iter().any(|hook| hook == "onEat") {
                self.mon_mut(target).ate_berry = true;
            }
        } else {
            match item.fling.as_ref().and_then(|data| data.effect) {
                Some(FlingEffect::WhiteHerb) => {
                    if self.mon_mut(target).boosts.clear_negative() {
                        self.add(format!(
                            "|-clearnegativeboost|{}|[silent]",
                            self.ident(target)
                        ));
                    }
                }
                Some(FlingEffect::MentalHerb) if self.mon(target).hp > 0 => {
                    self.mental_herb_effect(target, false);
                }
                _ => {}
            }
        }
    }

    pub(super) fn finish_fling(&mut self, pos: Position) {
        self.mon_mut(pos).fling_pending = false;
        let Some(item) = DEX.item(self.format.dex_mod, &self.mon(pos).item) else {
            return;
        };
        let id = if self.mon(pos).hp == 0 {
            self.mon(pos).item.clone()
        } else {
            self.lose_held_item(pos)
        };
        let mon = self.mon_mut(pos);
        mon.last_item = id;
        mon.used_item_this_turn = true;
        self.add(format!(
            "|-enditem|{}|{}|[from] move: Fling",
            self.ident(pos),
            item.effect.name
        ));
        self.symbiosis_after_item(pos);
    }

    pub(super) fn symbiosis_after_item(&mut self, recipient: Position) {
        if self.format.game_type.active_slots() == 1
            || self.mon(recipient).hp == 0
            || self.mon(recipient).pivot_move.is_some()
        {
            return;
        }
        let ally = Position {
            slot: 1 - recipient.slot,
            ..recipient
        };
        let mon = self.mon(ally);
        if mon.hp == 0 || self.event_ability(ally) != "symbiosis" {
            return;
        }
        let Some(item) = mon.transferable_item(self.format.dex_mod) else {
            return;
        };
        if !self.mon(recipient).can_transfer_item(item) {
            return;
        }
        let id = self.lose_held_item(ally);
        self.mon_mut(recipient).item = id;
        self.on_gained_item(recipient);
        self.add(format!(
            "|-activate|{}|ability: Symbiosis|{}|[of] {}",
            self.ident(ally),
            item.effect.name,
            self.ident(recipient)
        ));
    }

    pub(super) fn magician_after_hit(&mut self, user: Position, targets: &[Position]) {
        let mon = self.mon(user);
        if mon.active_ability() != "magician" || mon.hp == 0 || !mon.item.is_empty() {
            return;
        }
        let mut ordered: Vec<_> = targets
            .iter()
            .map(|&target| (target, self.mon(target).speed))
            .collect();
        speed_sort_by(&mut ordered, &mut self.rng, |a, b| b.1.cmp(&a.1));
        for (target, _) in ordered {
            if target == user {
                continue;
            }
            let victim = self.mon(target);
            let Some(name) = self.can_take_item(target) else {
                if victim.hp > 0
                    && self.event_ability(target) == "stickyhold"
                    && !victim.item.is_empty()
                {
                    self.add(format!(
                        "|-activate|{}|ability: Sticky Hold",
                        self.ident(target)
                    ));
                }
                continue;
            };
            let target_name = if victim.fainted {
                format!("{}: {}", target.player.id(), victim.name)
            } else {
                self.ident(target)
            };
            let id = self.lose_held_item(target);
            self.mon_mut(user).item = id;
            self.on_gained_item(user);
            self.add(format!(
                "|-item|{}|{name}|[from] ability: Magician|[of] {target_name}",
                self.ident(user)
            ));
            break;
        }
    }

    pub(super) fn thief_after_hit(&mut self, user: Position, target: Position, move_name: &str) {
        if self.mon(user).hp == 0 || !self.mon(user).item.is_empty() {
            return;
        }
        self.transfer_stolen_item(user, target, move_name == "Thief", move_name, false);
    }

    pub(super) fn pickpocket_after_move(&mut self, holder: Position, attacker: Position) {
        let mon = self.mon(holder);
        if holder == attacker
            || mon.hp == 0
            || !mon.item.is_empty()
            || mon.pivot_move.is_some()
            || self.drag_flags & holder.mask() != 0
            || self.mon(attacker).pivot_move == Some(Pivot::Reactive)
        {
            return;
        }
        self.transfer_stolen_item(holder, attacker, true, "Pickpocket", true);
    }

    pub(super) fn red_card_after_move(&mut self, holder: Position, attacker: Position) {
        if holder == attacker
            || self.mon(holder).hp == 0
            || self.mon(attacker).hp == 0
            || !self.active_effects(attacker)
            || self.mon(holder).effective_item() != "redcard"
            || self.drag_flags & (holder.mask() | attacker.mask()) != 0
            || self.first_bench(attacker.player, 0).is_none()
        {
            return;
        }
        self.consume_with_reason(
            holder,
            "Red Card",
            false,
            &format!("|[of] {}", self.ident(attacker)),
        );
        self.symbiosis_after_item(holder);
        self.schedule_drag(holder, attacker);
    }

    pub(super) fn eject_button_after_move(&mut self, holder: Position, attacker: Position) {
        if holder == attacker
            || self.mon(holder).hp == 0
            || self.debug_ignored_item(holder, "AfterMoveSecondary")
            || self.mon(holder).effective_item() != "ejectbutton"
            || self.drag_flags & holder.mask() != 0
            || self.first_bench(holder.player, 0).is_none()
            || self
                .positions()
                .any(|pos| self.mon(pos).pivot_move == Some(Pivot::Reactive))
        {
            return;
        }
        self.mon_mut(holder).pivot_move = Some(Pivot::Reactive);
        self.consume(holder, "Eject Button", false);
        self.symbiosis_after_item(holder);
        // Champions keeps the attacker's own U-turn-style switch pending.
    }

    fn transfer_stolen_item(
        &mut self,
        recipient: Position,
        donor: Position,
        show_loss: bool,
        effect: &str,
        ability: bool,
    ) {
        let Some(item) = DEX.item(self.format.dex_mod, &self.mon(donor).item) else {
            return;
        };
        if self.mon(donor).hp > 0 && self.event_ability(donor) == "stickyhold" {
            self.add(format!(
                "|-activate|{}|ability: Sticky Hold",
                self.ident(donor)
            ));
            return;
        }
        if !self.mon(donor).can_transfer_item(item)
            || !ability && !self.mon(recipient).can_transfer_item(item)
        {
            return;
        }
        self.mon_mut(recipient).item = self.lose_held_item(donor);
        self.on_gained_item(recipient);
        let recipient = self.ident(recipient);
        let donor = self.ident(donor);
        let kind = if ability { "ability" } else { "move" };
        let name = &item.effect.name;
        if show_loss {
            let loss_of = if ability { &donor } else { &recipient };
            self.add(format!(
                "|-enditem|{donor}|{name}|[silent]|[from] {kind}: {effect}|[of] {loss_of}"
            ));
        }
        self.add(format!(
            "|-item|{recipient}|{name}|[from] {kind}: {effect}|[of] {donor}"
        ));
    }

    pub(super) fn swap_items(&mut self, user: Position, target: Position, move_name: &str) -> bool {
        let user_item = DEX.item(self.format.dex_mod, &self.mon(user).item);
        let foe_item = DEX.item(self.format.dex_mod, &self.mon(target).item);
        if (user_item.is_none() && foe_item.is_none())
            || user_item.is_some_and(|item| {
                !self.mon(user).can_transfer_item(item) || !self.mon(target).can_transfer_item(item)
            })
            || foe_item.is_some_and(|item| {
                !self.mon(target).can_transfer_item(item) || !self.mon(user).can_transfer_item(item)
            })
        {
            return false;
        }
        let from_user = self.lose_held_item(user);
        let from_target = self.lose_held_item(target);
        self.add(format!(
            "|-activate|{}|move: Trick|[of] {}",
            self.ident(user),
            self.ident(target)
        ));
        for (pos, id, new, previous) in [
            (target, from_user, user_item, foe_item),
            (user, from_target, foe_item, user_item),
        ] {
            if let Some(item) = new {
                self.mon_mut(pos).item = id;
                self.on_gained_item(pos);
                self.add(format!(
                    "|-item|{}|{}|[from] move: {move_name}",
                    self.ident(pos),
                    item.effect.name
                ));
            } else if let Some(item) = previous {
                self.add(format!(
                    "|-enditem|{}|{}|[silent]|[from] move: {move_name}",
                    self.ident(pos),
                    item.effect.name
                ));
            }
        }
        true
    }

    pub(super) fn bestow_item(&mut self, user: Position, target: Position) -> bool {
        let recipient = self.mon(target);
        if recipient.hp == 0 || !recipient.item.is_empty() {
            return false;
        }
        let Some(item) = self.mon(user).transferable_item(self.format.dex_mod) else {
            return false;
        };
        if !recipient.can_transfer_item(item) {
            return false;
        }
        let id = self.lose_held_item(user);
        self.mon_mut(target).item = id;
        self.on_gained_item(target);
        self.add(format!(
            "|-item|{}|{}|[from] move: Bestow|[of] {}",
            self.ident(target),
            item.effect.name,
            self.ident(user)
        ));
        true
    }

    pub(super) fn item_heal(&mut self, pos: Position, amount: u32, item: &str) {
        self.item_heal_from(pos, amount, item, None);
    }

    fn item_heal_from(&mut self, pos: Position, amount: u32, item: &str, source: Option<Position>) {
        if self.mon(pos).heal_block > 0 {
            return;
        }
        let ripen = self.event_ability(pos) == "ripen";
        if ripen && matches!(item, "Leftovers" | "Berry Juice") {
            self.add(format!("|-activate|{}|ability: Ripen", self.ident(pos)));
        }
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.hp == mon.max_hp.get() {
            return;
        }
        let amount = amount.max(1)
            * if ripen && item.ends_with(" Berry") {
                2
            } else {
                1
            };
        mon.hp = (mon.hp + amount).min(mon.max_hp.get());
        let mut cause = format!("|[from] item: {item}");
        if let Some(source) = source.filter(|&source| source != pos) {
            cause.push_str(&format!("|[of] {}", self.ident(source)));
        }
        self.add_health_line_with("-heal", pos, false, &cause);
    }

    pub(super) fn after_eating_berry(&mut self, pos: Position, name: &str, stolen: bool) {
        self.after_eating_berry_from(pos, name, stolen, None);
    }

    fn after_eating_berry_from(
        &mut self,
        pos: Position,
        name: &str,
        stolen: bool,
        source: Option<Position>,
    ) {
        match self.event_ability(pos) {
            "cheekpouch" => {
                self.ability_heal(pos, 3, "Cheek Pouch", source);
            }
            "cudchew" if !stolen => {
                if let Some(item) = DEX.item(self.format.dex_mod, &to_id(name)) {
                    let ticks = if self.turn_queue_empty { 1 } else { 2 };
                    self.mon_mut(pos).cud_chew = Some(CudChew { item, ticks });
                }
            }
            _ => {}
        }
    }

    pub(super) fn chew_berry(&mut self, pos: Position) {
        if self.event_ability(pos) != "cudchew" || self.mon(pos).hp == 0 {
            return;
        }
        let mon = self.mon_mut(pos);
        let Some(state) = mon.cud_chew.as_mut() else {
            return;
        };
        state.ticks -= 1;
        if state.ticks != 0 {
            return;
        }
        let item = state.item;
        mon.cud_chew = None;
        let name = &item.effect.name;
        self.add(format!("|-activate|{}|ability: Cud Chew", self.ident(pos)));
        self.add(format!("|-enditem|{}|{name}|[eat]", self.ident(pos)));
        if !self.debug_ignored_item(pos, "Eat") {
            self.berry_effect(pos, &to_id(name), item, None);
        }
        if item.effect.handlers.iter().any(|hook| hook == "onEat") {
            self.mon_mut(pos).ate_berry = true;
        }
    }

    fn berry_effect(
        &mut self,
        pos: Position,
        id: &str,
        item: &crate::dex::Item,
        source: Option<Position>,
    ) {
        match id {
            "sitrusberry" => {
                let amount = self.mon(pos).max_hp.get() / 4;
                self.item_heal_from(pos, amount, &item.effect.name, source);
            }
            "oranberry" => self.item_heal_from(pos, 10, &item.effect.name, source),
            "persimberry" if self.mon(pos).confusion > 0 => self.cure_confusion(pos),
            "leppaberry" => self.replenish_pp_with_leppa(pos),
            "lumberry" => {
                self.cure_status(pos, "[msg]");
                if self.mon(pos).confusion > 0 {
                    self.cure_confusion(pos);
                }
            }
            "chestoberry" if matches!(self.mon(pos).status, Status::Sleep(_)) => {
                self.cure_status(pos, "[msg]")
            }
            id if status_berry_matches(id, self.mon(pos).status) => self.cure_status(pos, "[msg]"),
            _ => {
                if let Some((name, stat)) = stat_berry(id) {
                    self.apply_stat_berry(pos, name, stat);
                }
            }
        }
    }

    fn replenish_pp_with_leppa(&mut self, pos: Position) {
        let amount = if self.event_ability(pos) == "ripen" {
            20
        } else {
            10
        };
        let slots = &mut self.mon_mut(pos).moves;
        let Some(slot) = slots
            .iter()
            .position(|m| m.pp == 0)
            .or_else(|| slots.iter().position(|m| m.pp < m.max_pp))
        else {
            return;
        };
        let slot = &mut slots[slot];
        slot.pp = (slot.pp + amount).min(slot.max_pp);
        let name = &slot.move_data.name;
        self.add(format!(
            "|-activate|{}|item: Leppa Berry|{name}|[consumed]",
            self.ident(pos)
        ));
    }

    pub(super) fn consume(&mut self, pos: Position, item: &str, eaten: bool) {
        if eaten && self.event_ability(pos) == "ripen" {
            self.add(format!("|-activate|{}|ability: Ripen", self.ident(pos)));
        }
        self.consume_with_reason(pos, item, eaten, if eaten { "|[eat]" } else { "" });
    }

    fn consume_with_reason(&mut self, pos: Position, item: &str, eaten: bool, reason: &str) {
        let id = self.lose_held_item(pos);
        // Air Balloon runs AfterUseItem directly without recording lastItem.
        if !id.is_empty() && id != "airballoon" {
            let mon = self.mon_mut(pos);
            mon.last_item = id;
            mon.used_item_this_turn = true;
            mon.ate_berry |= eaten;
            if eaten {
                mon.ripen_pending = false;
            }
        }
        self.add(format!("|-enditem|{}|{item}{reason}", self.ident(pos)));
    }

    pub(super) fn lose_held_item(&mut self, pos: Position) -> String {
        let mon = self.mon_mut(pos);
        let item = std::mem::take(&mut mon.item);
        if !item.is_empty() && mon.active_ability() == "unburden" {
            mon.track_volatile("unburden");
        }
        item
    }

    pub(super) fn use_natural_gift_item(&mut self, pos: Position) {
        let item = self.lose_held_item(pos);
        let mon = self.mon_mut(pos);
        mon.last_item = item;
        mon.used_item_this_turn = true;
        self.symbiosis_after_item(pos);
    }

    pub(super) fn recycle_item(&mut self, pos: Position) -> bool {
        let mon = self.mon(pos);
        if !mon.item.is_empty() {
            return false;
        }
        let Some(item) = DEX.item(self.format.dex_mod, &mon.last_item) else {
            return false;
        };
        let mon = self.mon_mut(pos);
        mon.item = std::mem::take(&mut mon.last_item);
        self.add(format!(
            "|-item|{}|{}|[from] move: Recycle",
            self.ident(pos),
            item.effect.name
        ));
        self.on_gained_item(pos);
        true
    }

    pub(super) fn harvest_item(&mut self, pos: Position) {
        // Harvest flips its coin before checking the holder's item.
        if self.effective_weather().map(Weather::ordinary) != Some(Weather::Sun)
            && !self.rng.chance(1, 2)
        {
            return;
        }
        let mon = self.mon(pos);
        if mon.hp == 0 || !mon.item.is_empty() {
            return;
        }
        let Some(item) = DEX
            .item(self.format.dex_mod, &mon.last_item)
            .filter(|item| item.is_berry)
        else {
            return;
        };
        let mon = self.mon_mut(pos);
        mon.item = std::mem::take(&mut mon.last_item);
        self.on_gained_item(pos);
        self.add(format!(
            "|-item|{}|{}|[from] ability: Harvest",
            self.ident(pos),
            item.effect.name
        ));
    }

    pub(super) fn pick_up_item(&mut self, pos: Position) {
        if !self.mon(pos).item.is_empty() {
            return;
        }
        let mut candidates = [pos; 3];
        let mut count = 0;
        for donor in self.positions() {
            let mon = self.mon(donor);
            if donor != pos && !mon.fainted && mon.used_item_this_turn && !mon.last_item.is_empty()
            {
                candidates[count] = donor;
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        let donor = candidates[self.rng.below(count as u32) as usize];
        let id = std::mem::take(&mut self.mon_mut(donor).last_item);
        let item = DEX.item(self.format.dex_mod, &id).unwrap();
        self.add(format!(
            "|-item|{}|{}|[from] ability: Pickup",
            self.ident(pos),
            item.effect.name
        ));
        self.mon_mut(pos).item = id;
        self.on_gained_item(pos);
    }

    pub(super) fn indirect_damage(&mut self, pos: Position, divisor: u32, cause: &str) {
        let amount = (self.mon(pos).max_hp.get() / divisor).max(1);
        self.indirect_damage_amount(pos, amount, cause);
    }

    pub(super) fn indirect_damage_amount(&mut self, pos: Position, amount: u32, cause: &str) {
        let mon = self.mon(pos);
        if mon.hp == 0 || mon.active_ability() == "magicguard" {
            return;
        }
        let amount = self.focus_survival(pos, amount, false);
        let mon = self.mon_mut(pos);
        mon.half_hp_berry_deferred = false;
        mon.damage_hp(amount);
        self.add_health_line_with("-damage", pos, false, cause);
    }
}

impl Pokemon {
    pub(super) fn ignores_held_item(&self) -> bool {
        (self.items_suppressed
            || self.under(super::field_effects::LegacyCondition::Embargo)
            || self.active_ability() == "klutz"
                && !DEX
                    .item(crate::dex::Mod::Champions, &self.item)
                    .is_some_and(|item| item.ignore_klutz))
            && !matches!(self.item.as_str(), "redorb" | "blueorb")
    }

    /// The held item remains in inventories even when its effects are ignored.
    pub(super) fn effective_item(&self) -> &str {
        if self.ignores_held_item() {
            ""
        } else {
            &self.item
        }
    }

    pub(super) fn holds_choice_item(&self) -> bool {
        matches!(
            self.effective_item(),
            "choicescarf" | "choiceband" | "choicespecs"
        )
    }

    pub(super) fn has_choice_item(&self) -> bool {
        matches!(
            self.item.as_str(),
            "choicescarf" | "choiceband" | "choicespecs"
        )
    }

    pub(super) fn disabled_by_choice(&self, slot: usize) -> bool {
        self.holds_choice_item()
            && self.choice_lock.is_some_and(|i| {
                self.moves.get(i).is_none_or(|locked| {
                    self.moves
                        .get(slot)
                        .or(self.extra_move.as_deref())
                        .is_none_or(|selected| locked.id != selected.id)
                })
            })
    }

    pub(super) fn item_power_modifier(
        &self,
        move_type: &str,
        category: crate::dex::Category,
    ) -> u32 {
        use crate::dex::Category;
        let held = self.effective_item();
        if matches!(
            (held, category),
            ("wiseglasses", Category::Special) | ("muscleband", Category::Physical)
        ) {
            4505
        } else if matches!(
            (held, move_type),
            ("mysticwater", "Water")
                | ("miracleseed", "Grass")
                | ("charcoal", "Fire")
                | ("fairyfeather", "Fairy")
                | ("spelltag", "Ghost")
                | ("softsand", "Ground")
                | ("magnet", "Electric")
                | ("blackbelt", "Fighting")
                | ("blackglasses", "Dark")
                | ("dragonfang", "Dragon")
                | ("hardstone", "Rock")
                | ("metalcoat", "Steel")
                | ("nevermeltice", "Ice")
                | ("poisonbarb", "Poison")
                | ("sharpbeak", "Flying")
                | ("silkscarf", "Normal")
                | ("silverpowder", "Bug")
                | ("twistedspoon", "Psychic")
        ) || (held.ends_with("plate") || held.ends_with("mask"))
            && DEX
                .item(crate::dex::Mod::Champions, held)
                .is_some_and(|item| {
                    item.plate_boost && item.plate_type.as_deref() == Some(move_type)
                        || item
                            .mask_form
                            .as_ref()
                            .is_some_and(|form| self.base_species.name.starts_with(form))
                })
        {
            4915
        } else {
            4096
        }
    }
}
