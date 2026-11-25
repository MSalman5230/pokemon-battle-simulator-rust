//! Shared ability hooks used by move effects and held-item modifiers.

use super::{
    boost_copy::PendingBoostCopy,
    turn::{BoostLabel, Position},
    *,
};

pub(super) struct PostHit {
    pub contact: bool,
    pub multihit: bool,
    pub smart_target: bool,
    pub delayed: bool,
}
use crate::{
    choice::TargetKind,
    dex::Category,
    mechanics::{BoostStat, chain},
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Attention {
    FollowMe,
    RagePowder,
}

impl Attention {
    pub(super) fn name_id(self) -> (&'static str, &'static str) {
        match self {
            Self::FollowMe => ("Follow Me", "followme"),
            Self::RagePowder => ("Rage Powder", "ragepowder"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BoostOrigin {
    Move { has_secondaries: bool },
    MagneticFlux,
    Secondary,
    Intimidate,
    SupersweetSyrup,
    MirrorArmor,
    Hazard,
    Octolock,
    SyrupBomb,
    ZPower,
}

impl BoostOrigin {
    pub(super) fn from_move(data: &Move) -> Self {
        Self::Move {
            has_secondaries: !data.secondaries.is_empty(),
        }
    }

    fn quiet(self) -> bool {
        matches!(
            self,
            Self::Secondary
                | Self::Intimidate
                | Self::SupersweetSyrup
                | Self::MirrorArmor
                | Self::MagneticFlux
        )
    }

    fn reveal_protection(self) -> bool {
        !matches!(
            self,
            Self::Secondary
                | Self::Octolock
                | Self::Move {
                    has_secondaries: true
                }
        )
    }
}

#[derive(Clone, Copy)]
enum DropHook {
    FlowerVeil,
    Block(&'static str),
    IntimidateBlock(&'static str),
    ProtectStat(BoostStat, &'static str),
    MirrorArmor,
}

impl Pokemon {
    pub(super) fn active_ability(&self) -> &str {
        if self.gastro_acid
            && !DEX
                .ability(&self.ability)
                .is_some_and(|ability| ability.cannot_suppress)
        {
            ""
        } else {
            &self.ability
        }
    }
    pub(super) fn ability_power_modifier(
        &self,
        move_data: &Move,
        move_type: &str,
        base_power: u16,
        weather: Option<Weather>,
    ) -> u32 {
        let flags = &move_data.flags;
        match self.active_ability() {
            "technician" if base_power <= 60 => 6144,
            "toughclaws" if flags.contains_key("contact") => 5325,
            "sharpness" if flags.contains_key("slicing") => 6144,
            "strongjaw" if flags.contains_key("bite") => 6144,
            "punkrock" if flags.contains_key("sound") => 5325,
            "megalauncher" if flags.contains_key("pulse") => 6144,
            "ironfist" if flags.contains_key("punch") => 4915,
            "reckless" if move_data.recoil.is_some() || crash_move(&move_data.name) => 4915,
            "sandforce"
                if weather == Some(Weather::Sand)
                    && matches!(move_type, "Rock" | "Ground" | "Steel") =>
            {
                5325
            }
            _ => 4096,
        }
    }

    pub(super) fn attack_modifier(&self, category: Category, move_type: &str) -> u32 {
        let ability = if self.active_ability() == "waterbubble" && move_type == "Water" {
            8192
        } else if self.low_hp_attack_boost(move_type).is_some()
            || (self.active_ability() == "firemane" && move_type == "Fire")
            || (category == Category::Physical
                && self.active_ability() == "guts"
                && self.status != Status::Healthy)
        {
            6144
        } else {
            4096
        };
        let item = if self.effective_item() == "lightball"
            && self.base_species.base_species == "Pikachu"
        {
            8192
        } else if matches!(
            (self.effective_item(), category),
            ("choiceband", Category::Physical) | ("choicespecs", Category::Special)
        ) {
            6144
        } else {
            4096
        };
        let native = chain(ability, item);
        if category == Category::Physical
            && matches!(self.active_ability(), "hugepower" | "purepower")
        {
            chain(native, 8192)
        } else {
            native
        }
    }

    pub(super) fn low_hp_attack_boost(&self, move_type: &str) -> Option<&'static str> {
        if self.hp * 3 > self.max_hp.get() {
            return None;
        }
        Some(match (self.active_ability(), move_type) {
            ("blaze", "Fire") => "Blaze boost",
            ("torrent", "Water") => "Torrent boost",
            ("overgrow", "Grass") => "Overgrow boost",
            ("swarm", "Bug") => "Swarm boost",
            _ => return None,
        })
    }

    pub(super) fn transferable_item(
        &self,
        dex_mod: crate::dex::Mod,
    ) -> Option<&'static crate::dex::Item> {
        let item = DEX.item(dex_mod, &self.item)?;
        self.can_transfer_item(item).then_some(item)
    }

    pub(super) fn can_transfer_item(&self, item: &crate::dex::Item) -> bool {
        !item.unremovable
            && item.protected_form_base.as_deref() != Some(self.base_species.base_species.as_str())
            && !(item.is_primal_orb
                && item
                    .item_user
                    .as_ref()
                    .is_some_and(|users| users.contains(&self.base_species.base_species)))
            && !item.mega_stone.as_ref().is_some_and(|m| {
                m.contains_key(&self.base_species.name)
                    || m.contains_key(&self.base_species.base_species)
                    || m.values().any(|forme| *forme == self.base_species.name)
            })
    }
}

impl Battle {
    pub(super) fn download_boost(&mut self, source: Position) {
        let (def_stage, spd_stage) = if self.wonder_room != 0 {
            (BoostStat::Spd, BoostStat::Def)
        } else {
            (BoostStat::Def, BoostStat::Spd)
        };
        let (def, spd) = self
            .positions()
            .filter(|&pos| pos.player != source.player && self.mon(pos).hp > 0)
            .fold((0, 0), |(def, spd), pos| {
                let mon = self.mon(pos);
                (
                    def + crate::mechanics::boosted_stat(mon.stats.def, mon.boosts.get(def_stage)),
                    spd + crate::mechanics::boosted_stat(mon.stats.spd, mon.boosts.get(spd_stage)),
                )
            });
        if def != 0 && def >= spd {
            self.ability_boost(source, "Download", BoostStat::Spa, 1, true);
        } else if spd != 0 {
            self.ability_boost(source, "Download", BoostStat::Atk, 1, true);
        }
    }

    pub(super) fn move_makes_contact(&self, source: Position, data: &Move) -> bool {
        data.flags.contains_key("contact")
            && (self.mon(source).active_ability() != "longreach"
                || self.event_ability(source) != "longreach")
    }

    pub(super) fn anticipation(&mut self, holder: Position) {
        let threatened = self
            .positions()
            .filter(|&pos| pos.player != holder.player && self.mon(pos).hp != 0)
            .flat_map(|pos| &self.mon(pos).moves)
            .any(|slot| {
                let data = slot.move_data;
                if data.category == Category::Status {
                    return false;
                }
                if data.ohko {
                    return true;
                }
                let mut stage = 0;
                for ty in self.mon(holder).types().filter(|&ty| ty != "???") {
                    let chart = &DEX.type_data(&to_id(ty)).unwrap().damage_taken;
                    match chart.get(&data.move_type) {
                        Some(3) => return false,
                        Some(1) => stage += 1,
                        Some(2) => stage -= 1,
                        _ => {}
                    }
                }
                stage > 0
            });
        if threatened {
            self.add(format!("|-ability|{}|Anticipation", self.ident(holder)));
        }
    }

    pub(super) fn forewarn(&mut self, holder: Position) {
        let mut strongest = Vec::new();
        let mut best = 1;
        for foe in self
            .positions()
            .filter(|&pos| pos.player != holder.player && self.mon(pos).hp != 0)
        {
            for slot in &self.mon(foe).moves {
                let data = slot.move_data;
                let threat = if data.ohko {
                    150
                } else if matches!(slot.id.as_str(), "counter" | "mirrorcoat" | "metalburst") {
                    120
                } else if data.base_power == 1
                    || data.base_power == 0 && data.category != Category::Status
                {
                    80
                } else {
                    data.base_power
                };
                if threat > best {
                    strongest.clear();
                    best = threat;
                }
                if threat == best {
                    strongest.push((foe, data));
                }
            }
        }
        if !strongest.is_empty() {
            let (foe, data) = strongest[self.rng.below(strongest.len() as u32) as usize];
            self.add(format!(
                "|-activate|{}|ability: Forewarn|{}|[of] {}",
                self.ident(holder),
                data.name,
                self.ident(foe)
            ));
        }
    }

    pub(super) fn target_effects_after_move(
        &mut self,
        source: Position,
        hits: &[(Position, u32)],
        damage: [u32; 4],
        hit: PostHit,
    ) {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Kind {
            Ability,
            RedCard,
            EjectButton,
        }
        let mut holders = [(source, 0, Kind::Ability); 8];
        let mut count = 0;
        for &(target, _) in hits {
            if !self.active_effects(target) {
                continue;
            }
            let mon = self.mon(target);
            if matches!(
                mon.ability.as_str(),
                "berserk" | "angershell" | "pickpocket"
            ) {
                holders[count] = (target, mon.speed, Kind::Ability);
                count += 1;
            }
            let item = match mon.item.as_str() {
                "redcard" => Some(Kind::RedCard),
                "ejectbutton" => Some(Kind::EjectButton),
                _ => None,
            };
            if let Some(item) = item {
                holders[count] = (target, mon.speed, item);
                count += 1;
            }
        }
        speed_sort_by(&mut holders[..count], &mut self.rng, |a, b| {
            (b.2 == Kind::EjectButton)
                .cmp(&(a.2 == Kind::EjectButton))
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let total: u32 = damage.iter().sum();
        for &(target, _, item) in &holders[..count] {
            match item {
                Kind::EjectButton => {
                    if !hit.delayed {
                        self.eject_button_after_move(target, source);
                    }
                    continue;
                }
                Kind::RedCard => {
                    self.red_card_after_move(target, source);
                    continue;
                }
                Kind::Ability => {}
            }
            match self.event_ability(target) {
                "pickpocket" => {
                    if hit.contact {
                        self.pickpocket_after_move(target, source);
                    }
                    continue;
                }
                "berserk" | "angershell" => {}
                _ => continue,
            }
            let mon = self.mon_mut(target);
            mon.half_hp_berry_deferred = false;
            let anger_shell = mon.active_ability() == "angershell";
            let sustained = if hit.multihit && (anger_shell || !hit.smart_target) {
                total
            } else {
                damage[target.index()]
            };
            let half = mon.max_hp.get() / 2;
            if source != target && mon.hp != 0 && mon.hp <= half && mon.hp + sustained > half {
                if anger_shell {
                    self.ability_boosts(
                        target,
                        "Anger Shell",
                        &[
                            (BoostStat::Atk, 1),
                            (BoostStat::Spa, 1),
                            (BoostStat::Spe, 1),
                            (BoostStat::Def, -1),
                            (BoostStat::Spd, -1),
                        ],
                        false,
                    );
                } else {
                    self.ability_boost(target, "Berserk", BoostStat::Spa, 1, false);
                }
            }
        }
    }

    pub(super) fn sheer_force_removes_secondaries(&self, source: Position, data: &Move) -> bool {
        !data.secondaries.is_empty()
            && !data.has_sheer_force_boost
            && self.event_ability(source) == "sheerforce"
    }

    pub(super) fn active_effects(&self, pos: Position) -> bool {
        pos.slot < self.format.game_type.active_slots() && !self.mon(pos).fainted
    }

    pub(super) fn crit_ratio(&self, source: Position, target: Position, data: &Move) -> u8 {
        let user = self.mon(source);
        if !self.active_effects(source) {
            return data.crit_ratio.unwrap_or(1);
        }
        if user.laser_focus != 0 {
            return 4;
        }
        if user.active_ability() == "merciless"
            && matches!(self.mon(target).status, Status::Poison | Status::Toxic(_))
        {
            return 4;
        }
        let item = match user.effective_item() {
            "scopelens" | "razorclaw" => 1,
            "leek"
                if matches!(
                    to_id(&user.base_species.base_species).as_str(),
                    "farfetchd" | "sirfetchd"
                ) =>
            {
                2
            }
            _ => 0,
        };
        data.crit_ratio
            .unwrap_or(1)
            .saturating_add(
                item + u8::from(user.active_ability() == "superluck")
                    + user.dragon_cheer
                    + 2 * u8::from(user.volatile_ids().contains(&"focusenergy")),
            )
            .min(4)
    }

    pub(super) fn damp_holder(&mut self, event: &str, interrupts: bool) -> Option<Position> {
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
            if mon.hp > 0 && mon.ability == "damp" {
                holders[count] = (pos, mon.speed);
                count += 1;
            }
        }
        speed_sort_by(&mut holders[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        for &(pos, _) in &holders[..count] {
            if self.ability_suppressed(pos) {
                if self.format.report_exact_hp {
                    self.log_suppressed_ability(pos, event);
                }
            } else if interrupts {
                return Some(pos);
            }
        }
        None
    }

    /// Return the first TryMove ability to block the move. The full set of
    /// listeners is sorted even when none will block this particular move.
    pub(super) fn try_move_guard(
        &mut self,
        source: Position,
        target: Position,
        data: &Move,
        priority: i8,
        explosive: bool,
    ) -> Option<(Position, &'static str)> {
        let mut guards = [(source, 0, ""); 4];
        let mut len = 0;
        for pos in self.positions() {
            let mon = self.mon(pos);
            if mon.hp == 0 {
                continue;
            }
            let name = match mon.ability.as_str() {
                "damp" => "Damp",
                "armortail" if pos.player != source.player => "Armor Tail",
                "dazzling" if pos.player != source.player => "Dazzling",
                "queenlymajesty" if pos.player != source.player => "Queenly Majesty",
                _ => continue,
            };
            guards[len] = (pos, mon.speed, name);
            len += 1;
        }
        let guards = &mut guards[..len];
        speed_sort_by(guards, &mut self.rng, |a, b| b.1.cmp(&a.1));
        let all_exception = data.target == TargetKind::All
            && matches!(
                data.name.as_str(),
                "Perish Song" | "Flower Shield" | "Rototiller"
            );
        let blockable_priority =
            priority > 0 && !matches!(data.target, TargetKind::FoeSide | TargetKind::All);
        for &(pos, _, name) in guards.iter() {
            if self.ability_suppressed(pos) {
                if self.format.report_exact_hp {
                    self.log_suppressed_ability(pos, "TryMove");
                }
            } else if if name == "Damp" {
                explosive
            } else {
                (blockable_priority && target.player == pos.player)
                    || (priority > 0 && all_exception)
            } {
                return Some((pos, name));
            }
        }
        None
    }

    pub(super) fn aftermath(&mut self, target: Position, source: Position) {
        if self.mon(target).hp == 0
            && self.mon(source).hp > 0
            && self.damp_holder("Damage", true).is_none()
        {
            self.indirect_damage(
                source,
                4,
                &format!("|[from] ability: Aftermath|[of] {}", self.ident(target)),
            );
        }
    }

    pub(super) fn switch_out_ability(&mut self, pos: Position) {
        match self.event_ability(pos) {
            "zerotohero" => self.zero_to_hero_on_switch(pos),
            "naturalcure" => {
                self.cure_status(pos, "[from] ability: Natural Cure|[silent]");
            }
            "regenerator" => {
                let mon = self.mon_mut(pos);
                let max = mon.max_hp.get();
                // Champions has no Dynamax, so base and current maximum HP
                // agree for active Pokémon, including Mega Evolutions.
                let restored = (mon.hp + max / 3).min(max);
                if mon.hp != restored {
                    mon.hp = restored;
                    self.add_health_line_with(
                        "-heal",
                        pos,
                        false,
                        "|[from] ability: Regenerator|[silent]",
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn end_ability(&mut self, pos: Position) {
        self.end_supreme_overlord(pos, false);
        if self.mon(pos).ability == "opportunist" {
            self.mon_mut(pos).opportunist = PendingBoostCopy::default();
        }
        if self.mon(pos).ability == "unburden" {
            self.mon_mut(pos).untrack_volatile("unburden");
        }
        if self.mon(pos).volatile_ids().contains(&"flashfire") {
            self.mon_mut(pos).untrack_volatile("flashfire");
            self.add(format!(
                "|-end|{}|ability: Flash Fire|[silent]",
                self.ident(pos)
            ));
        }
        self.weather_ability_changed(pos, true);
    }

    pub(super) fn start_supreme_overlord(&mut self, pos: Position) {
        let fallen = self.sides[pos.player.index()]
            .as_ref()
            .unwrap()
            .total_fainted
            .min(5);
        if fallen > 0 {
            self.mon_mut(pos).fallen_allies = fallen;
            let ident = self.ident(pos);
            self.add(format!("|-activate|{ident}|ability: Supreme Overlord"));
            self.add(format!("|-start|{ident}|fallen{fallen}|[silent]"));
        }
    }

    pub(super) fn end_supreme_overlord(&mut self, pos: Position, leaving: bool) {
        let fallen = std::mem::take(&mut self.mon_mut(pos).fallen_allies);
        if fallen != 0 && !leaving {
            self.add(format!("|-end|{}|fallen{fallen}|[silent]", self.ident(pos)));
        }
    }

    pub(super) fn receive_fainted_ability(&mut self, fainted: Position) {
        if self.format.game_type.active_slots() != 2 {
            return;
        }
        let holder = Position {
            slot: 1 - fainted.slot,
            ..fainted
        };
        let recipient = self.mon(holder);
        let label = match recipient.active_ability() {
            "receiver" => "Receiver",
            "powerofalchemy" => "Power of Alchemy",
            _ => return,
        };
        if recipient.hp == 0 {
            return;
        }
        let original = &self.mon(fainted).ability;
        if original == "noability" {
            return;
        }
        let effect = DEX.ability(original);
        if effect.is_some_and(|a| a.no_receiver || a.cannot_suppress) {
            return;
        }
        let holder_name = self.ident(holder);
        let source_name = self.ident(fainted);
        if self.mon(holder).effective_item() == "abilityshield" {
            self.add(format!("|-ability|{source_name}|{label}"));
            self.add(format!("|-block|{holder_name}|item: Ability Shield"));
            return;
        }
        let id = original.clone();
        self.end_ability(holder);
        self.possible_unaware |= id == "unaware";
        self.mon_mut(holder).ability = id;
        self.reset_ability_order(holder);
        let name = effect.map_or("", |a| a.name.as_str());
        self.add(format!(
            "|-ability|{holder_name}|{name}|{label}|[from] ability: {label}|[of] {source_name}"
        ));
        self.start_ability(holder);
    }

    pub(super) fn flash_fire(&mut self, target: Position) {
        if self.mon(target).volatile_ids().contains(&"flashfire") {
            self.add(format!(
                "|-immune|{}|[from] ability: Flash Fire",
                self.ident(target)
            ));
        } else {
            self.mon_mut(target).track_volatile("flashfire");
            self.add(format!(
                "|-start|{}|ability: Flash Fire",
                self.ident(target)
            ));
        }
    }

    pub(super) fn frisk(&mut self, pos: Position) {
        let holder = self.ident(pos);
        for target in self
            .positions()
            .filter(|target| target.player != pos.player)
        {
            let mon = self.mon(target);
            if let Some(item) = DEX
                .item(self.format.dex_mod, &mon.item)
                .filter(|_| mon.hp != 0)
            {
                self.add(format!(
                    "|-item|{}|{}|[from] ability: Frisk|[of] {holder}",
                    self.ident(target),
                    item.effect.name
                ));
            }
        }
    }

    pub(super) fn ignores_ability_of(&self, target: Position) -> bool {
        self.ability_breaker.is_some_and(|source| source != target)
            && self.mon(target).effective_item() != "abilityshield"
    }

    /// Direct ability checks account for Gastro Acid. Mold Breaker only
    /// skips breakable handlers while that attack is being resolved.
    pub(super) fn ability_suppressed(&self, holder: Position) -> bool {
        let mon = self.mon(holder);
        (mon.gastro_acid && mon.active_ability().is_empty())
            || self.ignores_ability_of(holder)
                && DEX
                    .ability(&mon.ability)
                    .is_some_and(|ability| ability.breakable)
    }

    pub(super) fn log_suppressed_ability(&mut self, holder: Position, event: &str) {
        let mon = self.mon(holder);
        let label = if mon.gastro_acid && mon.active_ability().is_empty() {
            "Gastro Acid or Neutralizing Gas"
        } else {
            "Mold Breaker"
        };
        self.add(format!("|debug|{event} handler suppressed by {label}"));
    }

    pub(super) fn debug_suppressed_hook(&mut self, holder: Position, hook: &str) {
        if self.format.report_exact_hp
            && self.ability_suppressed(holder)
            && DEX
                .ability(&self.mon(holder).ability)
                .is_some_and(|a| a.handlers.iter().any(|h| h == hook))
        {
            self.log_suppressed_ability(holder, &hook[2..]);
        }
    }

    pub(super) fn event_ability(&self, holder: Position) -> &str {
        if !self.active_effects(holder)
            || self.ability_suppressed(holder)
            || self.mon(holder).transformed.is_some()
                && DEX
                    .ability(&self.mon(holder).ability)
                    .is_some_and(|a| a.no_transform)
        {
            ""
        } else {
            self.mon(holder).active_ability()
        }
    }

    pub(super) fn airborne_ability(&self, holder: Position) -> Option<&'static str> {
        let mon = self.mon(holder);
        if mon.effective_item() == "ironball" || self.forces_grounded(holder) {
            return None;
        }
        let label = match mon.active_ability() {
            "levitate" => "Levitate",
            "eelevate" => "Eelevate",
            _ => return None,
        };
        (!self.ignores_ability_of(holder)).then_some(label)
    }

    pub(super) fn boost_after_knockouts(&mut self, source: Position, count: i8) {
        match self.event_ability(source) {
            "battlebond"
                if self.mon(source).species.name == "Greninja-Bond"
                    && self.mon(source).hp != 0
                    && self.mon(source).transformed.is_none()
                    && !self.mon(source).triggered_battle_bond
                    && self
                        .party(source.player.foe())
                        .unwrap()
                        .iter()
                        .any(|mon| !mon.fainted) =>
            {
                self.ability_boosts(
                    source,
                    "Battle Bond",
                    &[
                        (BoostStat::Atk, 1),
                        (BoostStat::Spa, 1),
                        (BoostStat::Spe, 1),
                    ],
                    false,
                );
                self.add(format!(
                    "|-activate|{}|ability: Battle Bond",
                    self.ident(source)
                ));
                self.mon_mut(source).triggered_battle_bond = true;
            }
            "moxie" => {
                self.ability_boost(source, "Moxie", BoostStat::Atk, count, false);
            }
            "eelevate" => {
                let s = self.mon(source).stats;
                let mut best = (s.atk, BoostStat::Atk);
                for pair in [
                    (s.def, BoostStat::Def),
                    (s.spa, BoostStat::Spa),
                    (s.spd, BoostStat::Spd),
                    (s.spe, BoostStat::Spe),
                ] {
                    if pair.0 > best.0 {
                        best = pair;
                    }
                }
                self.ability_boost(source, "Eelevate", best.1, count, false);
            }
            _ => {}
        }
    }

    pub(super) fn can_take_item(&self, target: Position) -> Option<&'static str> {
        let mon = self.mon(target);
        if mon.hp > 0 && self.event_ability(target) == "stickyhold" {
            return None;
        }
        mon.transferable_item(self.format.dex_mod)
            .map(|item| item.effect.name.as_str())
    }

    pub(super) fn reset_ability_order(&mut self, pos: Position) {
        self.ability_order += 1;
        let order = self.ability_order;
        let mon = self.mon_mut(pos);
        mon.ability_order = order;
        mon.type_change_used = false;
        mon.cud_chew = None;
        mon.trace_seeking = false;
    }

    pub(super) fn replacement_ability(
        &self,
        source: Position,
        target: Position,
        name: &str,
    ) -> Option<(&str, bool)> {
        let current = self.mon(target).ability.as_str();
        let (id, announce_source) = match name {
            "Worry Seed" => ("insomnia", false),
            "Simple Beam" => ("simple", false),
            "Entrainment" => (self.mon(source).ability.as_str(), true),
            _ => unreachable!(),
        };
        (self.mon(target).hp != 0
            && !(announce_source && source == target)
            && current != "truant"
            && current != id
            && !DEX.ability(current).is_some_and(|ab| ab.cannot_suppress)
            && !DEX
                .ability(id)
                .is_some_and(|ab| ab.cannot_suppress || announce_source && ab.no_entrain))
        .then_some((id, announce_source))
    }

    pub(super) fn replace_ability_from_move(
        &mut self,
        source: Position,
        target: Position,
        name: &str,
    ) -> bool {
        let (id, announce_source) = self.replacement_ability(source, target, name).unwrap();
        let id = id.to_owned();
        self.gain_move_ability(target, id, announce_source.then_some(source), name)
    }

    pub(super) fn can_role_play(&self, source: Position, target: Position) -> bool {
        let user = &self.mon(source).ability;
        let copied = &self.mon(target).ability;
        user != copied
            && !DEX.ability(user).is_some_and(|ab| ab.cannot_suppress)
            && !DEX
                .ability(copied)
                .is_some_and(|ab| ab.no_role_play || ab.cannot_suppress)
    }

    pub(super) fn role_play(&mut self, source: Position, target: Position) -> bool {
        self.gain_move_ability(
            source,
            self.mon(target).ability.clone(),
            Some(target),
            "Role Play",
        )
    }

    pub(super) fn inhibit_ability(&mut self, pos: Position) -> bool {
        if self.mon(pos).gastro_acid {
            return false;
        }
        self.add(format!("|-endability|{}", self.ident(pos)));
        if self.mon(pos).illusion.is_some() {
            self.end_illusion(pos);
        }
        self.end_ability(pos);
        let mon = self.mon_mut(pos);
        mon.gastro_acid = true;
        mon.track_volatile("gastroacid");
        true
    }

    fn gain_move_ability(
        &mut self,
        target: Position,
        id: String,
        source: Option<Position>,
        name: &str,
    ) -> bool {
        if self.mon(target).effective_item() == "abilityshield" {
            self.add(format!(
                "|-block|{}|item: Ability Shield",
                self.ident(target)
            ));
            return false;
        }
        let ability = DEX.ability(&id);
        let current = self.mon(target).ability.as_str();
        let old = DEX.ability(current);
        if current == "illusion" {
            self.end_illusion(target);
        }
        self.end_ability(target);
        self.mon_mut(target).ability = id;
        self.reset_ability_order(target);
        let mut msg = format!(
            "|-ability|{}|{}|{}|[from] move: {name}",
            self.ident(target),
            ability.map_or("", |ab| ab.name.as_str()),
            old.map_or("", |ab| ab.name.as_str()),
        );
        if let Some(source) = source {
            msg.push_str(&format!("|[of] {}", self.ident(source)));
        }
        self.add(msg);
        self.start_ability(target);
        if name == "Worry Seed"
            && old.is_some()
            && matches!(self.mon(target).status, Status::Sleep(_))
        {
            self.cure_status(target, "[msg]");
        }
        true
    }

    pub(super) fn skill_swap(&mut self, source: Position, target: Position) -> Option<bool> {
        let user = self.mon(source);
        let mon = self.mon(target);
        let user_effect = DEX.ability(&user.ability);
        let target_effect = DEX.ability(&mon.ability);
        if user.fainted
            || mon.fainted
            || user_effect.is_some_and(|a| a.no_skill_swap)
            || target_effect.is_some_and(|a| a.no_skill_swap)
        {
            return Some(false);
        }
        for pos in [target, source] {
            if self.mon(pos).effective_item() == "abilityshield" {
                self.add(format!("|-block|{}|item: Ability Shield", self.ident(pos)));
                return None;
            }
        }
        self.end_ability(source);
        self.end_ability(target);
        let a = std::mem::take(&mut self.mon_mut(source).ability);
        let b = std::mem::replace(&mut self.mon_mut(target).ability, a);
        self.mon_mut(source).ability = b;
        let target_name = self.ident(target);
        let source_name = self.ident(source);
        if target.player == source.player {
            self.add(format!(
                "|-activate|{source_name}|Skill Swap|||[of] {target_name}"
            ));
            self.add("|-hint|Skill Swap does not announce the abilities of the Pokémon when used between allies.");
        } else {
            let target_label = target_effect.map_or("", |a| a.name.as_str());
            let user_label = user_effect.map_or("", |a| a.name.as_str());
            self.add(format!(
                "|-activate|{source_name}|Skill Swap|{target_label}|{user_label}|[of] {target_name}"
            ));
        }
        self.reset_ability_order(source);
        self.reset_ability_order(target);
        self.start_ability(target);
        self.start_ability(source);
        Some(true)
    }

    pub(super) fn contact_ability(&mut self, holder: Position, source: Position) {
        let (id, label) = match self.mon(holder).ability.as_str() {
            "mummy" => ("mummy", "Mummy"),
            "lingeringaroma" => ("lingeringaroma", "Lingering Aroma"),
            _ => return,
        };
        let mon = self.mon(source);
        let old = DEX.ability(&mon.ability);
        if mon.hp == 0 || mon.ability == id || old.is_some_and(|ability| ability.cannot_suppress) {
            return;
        }
        if mon.effective_item() == "abilityshield" {
            self.add(format!("|-ability|{}|{label}", self.ident(holder)));
            self.add(format!(
                "|-block|{}|item: Ability Shield",
                self.ident(source)
            ));
            return;
        }
        let old = old.map_or("", |ability| ability.name.as_str());
        if mon.ability == "illusion" {
            self.end_illusion(source);
        }
        self.end_ability(source);
        self.mon_mut(source).ability.clear();
        self.mon_mut(source).ability.push_str(id);
        self.reset_ability_order(source);
        self.add(format!(
            "|-activate|{}|ability: {label}|{}|[ability] {old}",
            self.ident(holder),
            self.ident(source)
        ));
    }

    pub(super) fn start_trace(&mut self, pos: Position) {
        if self.event_ability(pos) != "trace" {
            return;
        }
        if self.mon(pos).effective_item() == "abilityshield" {
            self.add(format!("|-block|{}|item: Ability Shield", self.ident(pos)));
            return;
        }
        let seek = !self.positions().any(|target| {
            target.player != pos.player
                && self.mon(target).hp > 0
                && self.mon(target).ability == "noability"
        });
        self.mon_mut(pos).trace_seeking = seek;
        self.try_trace(pos);
    }

    pub(super) fn try_trace(&mut self, pos: Position) {
        if !self.mon(pos).trace_seeking
            || self.mon(pos).hp == 0
            || self.event_ability(pos) != "trace"
        {
            return;
        }
        let mut candidates = [pos; 2];
        let mut count = 0;
        for target in self
            .positions()
            .filter(|target| target.player != pos.player)
        {
            let mon = self.mon(target);
            if mon.hp > 0
                && mon.ability != "noability"
                && DEX
                    .ability(&mon.ability)
                    .is_some_and(|ability| !ability.no_trace)
            {
                candidates[count] = target;
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        let target = candidates[self.rng.below(count as u32) as usize];
        let ability = DEX.ability(&self.mon(target).ability).unwrap();
        if ability.cannot_suppress {
            return;
        }
        let id = self.mon(target).ability.clone();
        self.mon_mut(pos).ability = id;
        self.reset_ability_order(pos);
        self.add(format!(
            "|-ability|{}|{}|Trace|[from] ability: Trace|[of] {}",
            self.ident(pos),
            ability.name,
            self.ident(target)
        ));
        self.start_ability(pos);
    }

    pub(super) fn redirect_target(
        &mut self,
        source: Position,
        target: Position,
        move_data: &Move,
        move_type: &str,
    ) -> Option<Position> {
        use crate::choice::TargetKind;
        if let Some(follower) = self
            .positions()
            .filter(|&candidate| {
                let mon = self.mon(candidate);
                candidate.player != source.player
                    && mon.hp > 0
                    && (mon.under(super::field_effects::LegacyCondition::Spotlight)
                        || mon.attention.is_some_and(|kind| {
                            kind != Attention::RagePowder || !self.status_immune(source, "powder")
                        }))
                    && move_data
                        .target
                        .valid_location(2, source.slot as u8, candidate.loc(source))
            })
            .min_by_key(|&candidate| {
                let mon = self.mon(candidate);
                (
                    !mon.under(super::field_effects::LegacyCondition::Spotlight),
                    std::cmp::Reverse(mon.speed),
                    mon.ability_order,
                )
            })
        {
            if self.format.report_exact_hp {
                let mon = self.mon(follower);
                let name = if mon.under(super::field_effects::LegacyCondition::Spotlight) {
                    "Spotlight"
                } else {
                    mon.attention.unwrap().name_id().0
                };
                self.add(format!("|debug|{name} redirected target of move"));
            }
            return Some(follower);
        }
        let (ability, label) = match move_type {
            "Electric" => ("lightningrod", "Lightning Rod"),
            "Water" => ("stormdrain", "Storm Drain"),
            _ => return None,
        };
        let geometry = match move_data.target {
            TargetKind::RandomNormal | TargetKind::AdjacentFoe => TargetKind::Normal,
            other => other,
        };
        let redirect = self
            .positions()
            .filter(|&candidate| {
                let mon = self.mon(candidate);
                mon.hp > 0
                    && self.event_ability(candidate) == ability
                    && geometry.valid_location(2, source.slot as u8, candidate.loc(source))
            })
            .min_by_key(|&candidate| {
                let mon = self.mon(candidate);
                (std::cmp::Reverse(mon.speed), mon.ability_order)
            })?;
        if redirect == target {
            return None;
        }
        self.add(format!(
            "|-activate|{}|ability: {label}",
            self.ident(redirect)
        ));
        Some(redirect)
    }

    pub(super) fn absorb_elemental(
        &mut self,
        target: Position,
        source: Position,
        move_type: &str,
    ) -> bool {
        if target == source {
            return false;
        }
        let (label, boost) = match (move_type, self.event_ability(target)) {
            ("Water", "waterabsorb") => ("Water Absorb", None),
            ("Water", "dryskin") => ("Dry Skin", None),
            ("Water", "stormdrain") => ("Storm Drain", Some((BoostStat::Spa, 1))),
            ("Electric", "lightningrod") => ("Lightning Rod", Some((BoostStat::Spa, 1))),
            ("Electric", "motordrive") => ("Motor Drive", Some((BoostStat::Spe, 1))),
            ("Electric", "voltabsorb") => ("Volt Absorb", None),
            ("Ground", "eartheater") => ("Earth Eater", None),
            ("Fire", "wellbakedbody") => ("Well-Baked Body", Some((BoostStat::Def, 2))),
            ("Grass", "sapsipper") => ("Sap Sipper", Some((BoostStat::Atk, 1))),
            _ => return false,
        };
        let succeeded = if let Some((stat, stages)) = boost {
            self.ability_boost(target, label, stat, stages, false)
        } else {
            self.ability_heal(target, 4, label, Some(source))
        };
        if !succeeded {
            self.add(format!(
                "|-immune|{}|[from] ability: {label}",
                self.ident(target)
            ));
        }
        true
    }

    pub(super) fn foe_unnerve(&self, pos: Position) -> bool {
        let slots = self.format.game_type.active_slots();
        self.party(pos.player.foe()).unwrap()[..slots]
            .iter()
            .any(|p| p.hp > 0 && p.active_ability() == "unnerve")
    }

    pub(super) fn flower_veil_holder(&mut self, target: Position) -> Option<Position> {
        let (mut holders, count) = self.flower_veils(target);
        let holders = &mut holders[..count];
        speed_sort_by(holders, &mut self.rng, |a, b| b.1.cmp(&a.1));
        holders
            .iter()
            .find(|(pos, _)| !self.ability_suppressed(*pos) && self.mon(target).has_type("Grass"))
            .map(|h| h.0)
    }

    fn flower_veils(&self, target: Position) -> ([(Position, i32); 2], usize) {
        let mut holders = [(target, 0); 2];
        let mut count = 0;
        for slot in 0..self.format.game_type.active_slots() {
            let pos = Position { slot, ..target };
            let mon = self.mon(pos);
            if mon.hp > 0 && mon.ability == "flowerveil" {
                holders[count] = (pos, mon.speed);
                count += 1;
            }
        }
        (holders, count)
    }

    pub(super) fn log_flower_veil(&mut self, target: Position, holder: Position) {
        self.add(format!(
            "|-block|{}|ability: Flower Veil|[of] {}",
            self.ident(target),
            self.ident(holder)
        ));
    }

    pub(super) fn flower_veil_status(
        &mut self,
        target: Position,
        source: Position,
        id: &str,
        reveal: bool,
    ) -> bool {
        if let Some(holder) = self.flower_veil_holder(target).filter(|_| target != source) {
            if self.format.report_exact_hp {
                self.add("|debug|interrupting setStatus with Flower Veil");
            }
            if reveal {
                self.log_flower_veil(target, holder);
            }
            if self.format.report_exact_hp {
                self.add(format!("|debug|set status [{id}] interrupted"));
            }
            true
        } else {
            false
        }
    }

    pub(super) fn contact_status(&mut self, holder: Position, attacker: Position, move_log: usize) {
        let (id, label) = match self.event_ability(holder) {
            "flamebody" => ("brn", "Flame Body"),
            "static" => ("par", "Static"),
            "poisonpoint" => ("psn", "Poison Point"),
            "effectspore" => {
                if self.status_immune(attacker, "powder") {
                    if self.format.report_exact_hp && self.mon(attacker).has_type("Grass") {
                        self.add("|debug|natural status immunity");
                    }
                    return;
                }
                let id = match self.rng.below(100) {
                    0..=10 => "slp",
                    11..=20 => "par",
                    21..=29 => "psn",
                    _ => return,
                };
                self.apply_contact_status(holder, attacker, id, "Effect Spore", move_log);
                return;
            }
            _ => return,
        };
        if self.rng.chance(3, 10) {
            self.apply_contact_status(holder, attacker, id, label, move_log);
        }
    }

    pub(super) fn spicy_spray(&mut self, holder: Position, attacker: Position, move_log: usize) {
        self.apply_contact_status(holder, attacker, "brn", "Spicy Spray", move_log);
    }

    pub(super) fn poison_touch(&mut self, holder: Position, target: Position, move_log: usize) {
        if self.event_ability(target) != "shielddust"
            && self.mon(target).effective_item() != "covertcloak"
            && self.rng.chance(3, 10)
        {
            self.apply_contact_status(holder, target, "psn", "Poison Touch", move_log);
        }
    }

    pub(super) fn innards_out(&mut self, holder: Position, attacker: Position, damage: u32) {
        if damage == 0 || self.mon(attacker).hp == 0 {
            return;
        }
        if self.event_ability(attacker) == "magicguard" {
            self.add(format!(
                "|-activate|{}|ability: Innards Out",
                self.ident(holder)
            ));
            if self.format.report_exact_hp {
                self.add("|debug|damage event failed");
            }
            return;
        }
        let damage = self.focus_survival(attacker, damage, false);
        let mon = self.mon_mut(attacker);
        mon.half_hp_berry_deferred = false;
        mon.damage_hp(damage);
        self.add_health_line_with(
            "-damage",
            attacker,
            false,
            &format!("|[from] ability: Innards Out|[of] {}", self.ident(holder)),
        );
    }

    fn apply_contact_status(
        &mut self,
        holder: Position,
        attacker: Position,
        id: &str,
        label: &str,
        move_log: usize,
    ) {
        if self.mon(attacker).hp == 0 || self.mon(attacker).status != Status::Healthy {
            return;
        }
        if self.status_immune(attacker, id) {
            if self.format.report_exact_hp {
                self.add("|debug|natural status immunity");
                self.add("|debug|immune to status");
            }
            return;
        }
        let prevented = self.status_blocked_by_ability(attacker, id, false)
            || self.flower_veil_status(attacker, holder, id, false);
        if prevented
            || self.field_blocks_status(attacker, id, holder.player, false)
            || self.safeguard_blocks(attacker, holder, false, false, false)
        {
            return;
        }
        let suffix = format!("|[from] ability: {label}|[of] {}", self.ident(holder));
        self.set_status_with_suffix(attacker, id, holder.player, &suffix);
        if !self.synchronize_status(attacker, holder, id, move_log) {
            self.cure_with_berry(attacker);
        }
    }

    pub(super) fn aroma_veil_blocks(&mut self, target: Position, reveal: bool) -> bool {
        let mut holders = [(target, 0); 2];
        let mut count = 0;
        for pos in self.positions().filter(|p| p.player == target.player) {
            let mon = self.mon(pos);
            if mon.hp > 0 && mon.ability == "aromaveil" {
                holders[count] = (pos, mon.speed);
                count += 1;
            }
        }
        let holders = &mut holders[..count];
        speed_sort_by(holders, &mut self.rng, |a, b| b.1.cmp(&a.1));
        let Some(&(holder, _)) = holders.iter().find(|(p, _)| !self.ability_suppressed(*p)) else {
            return false;
        };
        if reveal {
            self.add(format!(
                "|-block|{}|ability: Aroma Veil|[of] {}",
                self.ident(target),
                self.ident(holder)
            ));
        }
        true
    }

    pub(super) fn healer_residual(&mut self, holder: Position) {
        if self.format.game_type.active_slots() != 2 {
            return;
        }
        let ally = Position {
            slot: 1 - holder.slot,
            ..holder
        };
        if self.mon(ally).hp != 0
            && self.mon(ally).status != Status::Healthy
            && self.rng.chance(1, 2)
        {
            self.add(format!("|-activate|{}|ability: Healer", self.ident(holder)));
            self.cure_status(ally, "[msg]");
        }
    }

    pub(super) fn hospitality(&mut self, holder: Position) {
        if self.format.game_type.active_slots() == 2 && self.mon(holder).hp > 0 {
            let ally = Position {
                slot: 1 - holder.slot,
                ..holder
            };
            self.ability_heal(ally, 4, "Hospitality", Some(holder));
        }
    }

    fn live_partner(&self, pos: Position) -> Option<Position> {
        if self.format.game_type.active_slots() == 2 && pos.slot < 2 {
            let ally = Position {
                slot: 1 - pos.slot,
                ..pos
            };
            (self.mon(ally).hp > 0).then_some(ally)
        } else {
            None
        }
    }

    pub(super) fn curious_medicine(&mut self, holder: Position) {
        if let Some(ally) = self.live_partner(holder) {
            self.mon_mut(ally).boosts = Boosts::default();
            self.add(format!(
                "|-clearboost|{}|[from] ability: Curious Medicine|[of] {}",
                self.ident(ally),
                self.ident(holder)
            ));
        }
    }

    pub(super) fn plus_minus_boost(&self, user: Position) -> bool {
        matches!(self.event_ability(user), "plus" | "minus")
            && self
                .live_partner(user)
                .is_some_and(|ally| matches!(self.event_ability(ally), "plus" | "minus"))
    }

    /// ModifyBoost observes all Unaware handlers. Even handlers that don't
    /// change the requested stat are shuffled when their speeds tie.
    pub(super) fn sort_unaware_handlers(&mut self) {
        if !self.possible_unaware {
            return;
        }
        let mut speeds = [0; 4];
        let mut len = 0;
        for pos in self.positions() {
            let pokemon = self.mon(pos);
            if !pokemon.fainted && pokemon.ability_order != 0 && pokemon.ability == "unaware" {
                speeds[len] = pokemon.speed;
                len += 1;
            }
        }
        speed_sort_by(&mut speeds[..len], &mut self.rng, |a, b| b.cmp(a));
    }

    pub(super) fn ability_heal(
        &mut self,
        pos: Position,
        divisor: u32,
        label: &str,
        source: Option<Position>,
    ) -> bool {
        let mon = self.mon_mut(pos);
        if mon.hp == 0 || mon.hp == mon.max_hp.get() || mon.heal_block > 0 {
            return false;
        }
        mon.hp = (mon.hp + (mon.max_hp.get() / divisor).max(1)).min(mon.max_hp.get());
        let mut cause = format!("|[from] ability: {label}");
        if let Some(source) = source.filter(|source| *source != pos) {
            cause.push_str(&format!("|[of] {}", self.ident(source)));
        }
        self.add_health_line_with("-heal", pos, false, &cause);
        true
    }

    pub(super) fn intimidate(&mut self, pos: Position) {
        let foe = pos.player.foe();
        let slots = self.format.game_type.active_slots();
        let mut started = false;
        for slot in 0..slots {
            let target = Position { player: foe, slot };
            let mon = self.mon(target);
            if mon.hp == 0 {
                continue;
            }
            if !started {
                self.add(format!("|-ability|{}|Intimidate|boost", self.ident(pos)));
                started = true;
            }
            if self.mon(target).substitute_hp > 0 {
                self.add(format!("|-immune|{}", self.ident(target)));
                continue;
            }
            if self.event_ability(target) == "guarddog" {
                self.ability_boost(target, "Guard Dog", BoostStat::Atk, 1, false);
                continue;
            }
            let lowered = self.boost_from(
                target,
                pos,
                &[(BoostStat::Atk, -1)],
                BoostOrigin::Intimidate,
            );
            if lowered && self.event_ability(target) == "rattled" {
                self.ability_boost(target, "Rattled", BoostStat::Spe, 1, false);
            }
        }
    }

    pub(super) fn supersweet_syrup(&mut self, pos: Position) {
        if self.mon(pos).syrup_triggered {
            return;
        }
        self.mon_mut(pos).syrup_triggered = true;
        self.add(format!("|-ability|{}|Supersweet Syrup", self.ident(pos)));
        for target in self
            .positions()
            .filter(|target| target.player != pos.player)
        {
            if self.mon(target).hp == 0 {
                continue;
            }
            if self.mon(target).substitute_hp > 0 {
                self.add(format!("|-immune|{}", self.ident(target)));
            } else {
                self.boost_from(
                    target,
                    pos,
                    &[(BoostStat::Evasion, -1)],
                    BoostOrigin::SupersweetSyrup,
                );
            }
        }
    }

    pub(super) fn sweet_veil_blocks(&mut self, pos: Position, yawn: bool) -> bool {
        let mut veils = [(pos, 0); 2];
        let mut count = 0;
        for holder in self
            .positions()
            .filter(|holder| holder.player == pos.player)
        {
            if self.event_ability(holder) == "sweetveil" {
                veils[count] = (holder, self.mon(holder).speed);
                count += 1;
            }
        }
        if count == 0 {
            return false;
        }
        speed_sort_by(&mut veils[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        if self.format.report_exact_hp {
            self.add(if yawn {
                "|debug|Sweet Veil blocking yawn"
            } else {
                "|debug|Sweet Veil interrupts sleep"
            });
        }
        self.add(format!(
            "|-block|{}|ability: Sweet Veil|[of] {}",
            self.ident(pos),
            self.ident(veils[0].0)
        ));
        if !yawn && self.format.report_exact_hp {
            self.add("|debug|set status [slp] interrupted");
        }
        true
    }

    pub(super) fn boost_from(
        &mut self,
        target: Position,
        source: Position,
        changes: &[(BoostStat, i8)],
        origin: BoostOrigin,
    ) -> bool {
        if self.mon(target).hp == 0 {
            return false;
        }
        let effective;
        let ability = self.event_ability(target);
        let changes = if matches!(ability, "contrary" | "simple") {
            let factor = if ability == "contrary" { -1 } else { 2 };
            effective = changes
                .iter()
                .map(|&(s, n)| (s, n.saturating_mul(factor)))
                .collect::<Vec<_>>();
            effective.as_slice()
        } else {
            changes
        };
        let mon = self.mon(target);
        let mut hooks = [(target, 0, DropHook::FlowerVeil); 3];
        let own = match mon.ability.as_str() {
            "mirrorarmor" => Some(DropHook::MirrorArmor),
            "illuminate" => Some(DropHook::ProtectStat(BoostStat::Accuracy, "Illuminate")),
            "keeneye" => Some(DropHook::ProtectStat(BoostStat::Accuracy, "Keen Eye")),
            "mindseye" => Some(DropHook::ProtectStat(BoostStat::Accuracy, "Mind's Eye")),
            "bigpecks" => Some(DropHook::ProtectStat(BoostStat::Def, "Big Pecks")),
            "hypercutter" => Some(DropHook::ProtectStat(BoostStat::Atk, "Hyper Cutter")),
            "innerfocus" => Some(DropHook::IntimidateBlock("Inner Focus")),
            "oblivious" => Some(DropHook::IntimidateBlock("Oblivious")),
            "owntempo" => Some(DropHook::IntimidateBlock("Own Tempo")),
            "scrappy" => Some(DropHook::IntimidateBlock("Scrappy")),
            _ => mon.stat_drop_protection().map(DropHook::Block),
        };
        let mut count = 0;
        if let Some(own) = own {
            hooks[0] = (target, mon.speed, own);
            count += 1;
        }
        let (veils, veils_count) = self.flower_veils(target);
        for &(holder, speed) in &veils[..veils_count] {
            hooks[count] = (holder, speed, DropHook::FlowerVeil);
            count += 1;
        }
        let hooks = &mut hooks[..count];
        speed_sort_by(hooks, &mut self.rng, |a, b| b.1.cmp(&a.1));
        let mut removed = 0_u8;
        if target != source {
            if self.mist_active(target.player)
                && !(target.player != source.player
                    && matches!(origin, BoostOrigin::Move { .. } | BoostOrigin::Secondary)
                    && self.event_ability(source) == "infiltrator")
            {
                for (i, &(_, amount)) in changes.iter().enumerate() {
                    if amount < 0 {
                        removed |= 1 << i;
                    }
                }
                if removed != 0
                    && !matches!(
                        origin,
                        BoostOrigin::Secondary
                            | BoostOrigin::Move {
                                has_secondaries: true
                            }
                    )
                {
                    self.add(format!("|-activate|{}|move: Mist", self.ident(target)));
                }
                if origin == BoostOrigin::Secondary
                    && removed.count_ones() as usize == changes.len()
                {
                    self.report_no_move_effect();
                }
            }
            for &(holder, _, hook) in hooks.iter() {
                if self.ability_suppressed(holder) {
                    continue;
                }
                if matches!(hook, DropHook::FlowerVeil) && !self.mon(target).has_type("Grass")
                    || matches!(hook, DropHook::MirrorArmor) && origin == BoostOrigin::MirrorArmor
                {
                    continue;
                }
                let mut blocked = false;
                for (i, &(stat, amount)) in changes.iter().enumerate() {
                    let current = self.mon(target).boosts.get(stat);
                    if removed & (1 << i) != 0
                        || amount >= 0
                        || current <= -6
                        || matches!(hook, DropHook::ProtectStat(protected, _) if protected != stat)
                        || matches!(hook, DropHook::IntimidateBlock(_))
                            && (stat != BoostStat::Atk || origin != BoostOrigin::Intimidate)
                    {
                        continue;
                    }
                    removed |= 1 << i;
                    blocked = true;
                    if matches!(hook, DropHook::MirrorArmor) && self.mon(source).hp != 0 {
                        self.add(format!("|-ability|{}|Mirror Armor", self.ident(target)));
                        self.boost_from(
                            source,
                            target,
                            &[(stat, amount.max(-6 - current))],
                            BoostOrigin::MirrorArmor,
                        );
                    }
                }
                if blocked && origin.reveal_protection() {
                    match hook {
                        DropHook::FlowerVeil => self.log_flower_veil(target, holder),
                        DropHook::Block(ability) => self.fail_stat_drop(target, ability),
                        DropHook::IntimidateBlock(ability) => {
                            let ident = self.ident(target);
                            self.add(format!(
                                "|-fail|{ident}|unboost|atk|[from] ability: {ability}|[of] {ident}"
                            ));
                        }
                        DropHook::ProtectStat(stat, ability) => {
                            let ident = self.ident(target);
                            self.add(format!(
                                "|-fail|{ident}|unboost|{}|[from] ability: {ability}|[of] {ident}",
                                stat.id()
                            ));
                        }
                        DropHook::MirrorArmor => {}
                    }
                }
            }
        }
        let mut success = false;
        let record = self.possible_opportunist;
        let mut observed = PendingBoostCopy::default();
        for (i, &(stat, amount)) in changes.iter().enumerate() {
            if removed & (1 << i) != 0 {
                continue;
            }
            let before = if record {
                self.mon(target).boosts.get(stat)
            } else {
                0
            };
            if self.apply_boosts_labeled(
                target,
                &[(stat, amount)],
                origin.quiet() || origin == BoostOrigin::ZPower,
                false,
                (origin == BoostOrigin::ZPower).then_some(BoostLabel::ZPower),
                false,
            ) {
                success = true;
                if record {
                    observed.accumulate(stat, self.mon(target).boosts.get(stat) - before);
                }
                if amount < 0 && target.player != source.player {
                    match self.event_ability(target) {
                        "competitive" => {
                            self.ability_boost(target, "Competitive", BoostStat::Spa, 2, true);
                        }
                        "defiant" => {
                            self.ability_boost(target, "Defiant", BoostStat::Atk, 2, true);
                        }
                        _ => {}
                    }
                }
            }
        }
        if record {
            self.note_foe_boosts(target, observed);
        }
        success
    }

    pub(super) fn ability_boost(
        &mut self,
        pos: Position,
        label: &str,
        stat: BoostStat,
        amount: i8,
        quiet: bool,
    ) -> bool {
        self.ability_boosts(pos, label, &[(stat, amount)], quiet)
    }

    pub(super) fn ability_boosts(
        &mut self,
        pos: Position,
        label: &str,
        boosts: &[(BoostStat, i8)],
        quiet: bool,
    ) -> bool {
        if self.mon(pos).hp == 0
            || !self
                .party(pos.player.foe())
                .unwrap()
                .iter()
                .any(|p| !p.fainted)
        {
            return false;
        }
        let mut success = false;
        let record = self.possible_opportunist && label != "Opportunist";
        let mut observed = PendingBoostCopy::default();
        let factor = match self.event_ability(pos) {
            "contrary" => -1,
            "simple" => 2,
            _ => 1,
        };
        for &(stat, amount) in boosts {
            let amount = amount.saturating_mul(factor);
            let change = self.mon_mut(pos).boosts.add(stat, amount);
            if record {
                observed.accumulate(stat, change);
            }
            if change > 0 {
                self.mon_mut(pos).stats_raised = true;
            } else if change < 0 {
                self.mon_mut(pos).stats_lowered = true;
            }
            if change != 0 && label == "Anger Point" {
                let stage = self.mon(pos).boosts.get(stat);
                self.add(format!(
                    "|-setboost|{}|{}|{stage}|[from] ability: Anger Point",
                    self.ident(pos),
                    stat.id()
                ));
                success = true;
                continue;
            }
            if change != 0 || quiet {
                if change != 0 && !success {
                    self.add(format!("|-ability|{}|{label}|boost", self.ident(pos)));
                    success = true;
                }
                let tag = if change < 0 { "-unboost" } else { "-boost" };
                self.add(format!(
                    "|{tag}|{}|{}|{}",
                    self.ident(pos),
                    stat.id(),
                    change.abs()
                ));
            }
        }
        if record {
            self.note_foe_boosts(pos, observed);
        }
        success
    }

    fn fail_stat_drop(&mut self, pos: Position, ability: &str) {
        let ident = self.ident(pos);
        self.add(format!(
            "|-fail|{ident}|unboost|[from] ability: {ability}|[of] {ident}"
        ));
    }

    pub(super) fn shed_skin(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp > 0 && mon.status != Status::Healthy && self.rng.chance(33, 100) {
            if self.format.report_exact_hp {
                self.add("|debug|shed skin");
            }
            self.add(format!("|-activate|{}|ability: Shed Skin", self.ident(pos)));
            self.cure_status(pos, "[msg]");
        }
    }

    pub(super) fn hydration(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if mon.hp != 0
            && mon.status != Status::Healthy
            && mon.effective_item() != "utilityumbrella"
            && self.effective_weather().map(Weather::ordinary) == Some(Weather::Rain)
        {
            if self.format.report_exact_hp {
                self.add("|debug|hydration");
            }
            self.add(format!("|-activate|{}|ability: Hydration", self.ident(pos)));
            self.cure_status(pos, "[msg]");
        }
    }

    pub(super) fn contact_speed_drop(&mut self, holder: Position, attacker: Position, label: &str) {
        self.add(format!("|-ability|{}|{label}", self.ident(holder)));
        self.boost_from(
            attacker,
            holder,
            &[(BoostStat::Spe, -1)],
            BoostOrigin::Secondary,
        );
    }

    pub(super) fn speed_boost(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if mon.active_turns == 0 {
            return;
        }
        let change = mon.boosts.add(BoostStat::Spe, 1);
        if change > 0 {
            mon.stats_raised = true;
            self.add(format!("|-ability|{}|Speed Boost|boost", self.ident(pos)));
            self.add(format!("|-boost|{}|spe|1", self.ident(pos)));
        }
        self.note_foe_boost(pos, BoostStat::Spe, change);
    }

    pub(super) fn moody(&mut self, pos: Position) {
        if self.event_ability(pos) != "moody" {
            return;
        }
        let boosts = self.mon(pos).boosts;
        let stats = &BoostStat::ALL[..5];
        let mut eligible = [BoostStat::Atk; 5];
        let mut count = 0;
        for &stat in stats {
            if boosts.get(stat) != 6 {
                eligible[count] = stat;
                count += 1;
            }
        }
        let raised = (count != 0).then(|| eligible[self.rng.below(count as u32) as usize]);
        count = 0;
        for &stat in stats {
            if Some(stat) != raised && boosts.get(stat) != -6 {
                eligible[count] = stat;
                count += 1;
            }
        }
        let lowered = (count != 0).then(|| eligible[self.rng.below(count as u32) as usize]);
        let mut changes = [(BoostStat::Atk, 0); 2];
        let mut count = 0;
        for (stat, amount) in [(raised, 2), (lowered, -1)] {
            if let Some(stat) = stat {
                changes[count] = (stat, amount);
                count += 1;
            }
        }
        self.ability_boosts(pos, "Moody", &changes[..count], false);
    }
}

pub(super) fn crash_move(name: &str) -> bool {
    matches!(
        name,
        "Supercell Slam" | "High Jump Kick" | "Jump Kick" | "Axe Kick"
    )
}

impl Pokemon {
    pub(super) fn reset_ability(&mut self) {
        if self.ability != self.base_ability {
            self.ability.clone_from(&self.base_ability);
        }
        self.type_change_used = false;
        self.cud_chew = None;
        self.trace_seeking = false;
        self.opportunist = PendingBoostCopy::default();
    }

    fn stat_drop_protection(&self) -> Option<&'static str> {
        Some(match self.active_ability() {
            "clearbody" => "Clear Body",
            "whitesmoke" => "White Smoke",
            "fullmetalbody" => "Full Metal Body",
            _ => return None,
        })
    }

    pub(super) fn converted_move_type(&self, original: &'static str) -> (&'static str, bool) {
        if original != "Normal" {
            return (original, false);
        }
        match self.active_ability() {
            "aerilate" => ("Flying", true),
            "pixilate" => ("Fairy", true),
            "galvanize" => ("Electric", true),
            "refrigerate" => ("Ice", true),
            "dragonize" => ("Dragon", true),
            _ => (original, false),
        }
    }
}

impl Battle {
    pub(super) fn protean_prepare(&mut self, pos: Position, move_type: &'static str) {
        let mon = self.mon(pos);
        let name = match self.event_ability(pos) {
            "protean" => "Protean",
            "libero" => "Libero",
            _ => return,
        };
        if move_type == "???"
            || mon.type_change_used
            || matches!(mon.species.base_species.as_str(), "Arceus" | "Silvally")
        {
            return;
        }
        let already_type = {
            let mut types = mon.types();
            types.next() == Some(move_type) && types.next().is_none()
        };
        if already_type {
            return;
        }
        let mon = self.mon_mut(pos);
        mon.set_temporary_type(Some(move_type));
        mon.type_change_used = true;
        self.add(format!(
            "|-start|{}|typechange|{move_type}|[from] ability: {name}",
            self.ident(pos)
        ));
    }
}
