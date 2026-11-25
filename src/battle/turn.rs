use super::boost_copy::PendingBoostCopy;
use super::*;
use super::{
    abilities::{BoostOrigin, PostHit},
    charge::ChargeSpec,
    damage::RegularHit,
    field_effects::{LegacyCondition, LegacyField},
    leech::DrainKind,
};
use crate::{
    choice::{MoveChoice, SpecialChoice, TargetKind},
    dex::{Accuracy, Category},
    mechanics::{BoostStat, boosted_stat},
};
use serde_json::Value;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Position {
    pub(super) player: Player,
    pub(super) slot: usize,
}

impl Position {
    fn other_slot(self) -> Self {
        Self {
            slot: 1 - self.slot,
            ..self
        }
    }

    pub(super) fn index(self) -> usize {
        self.player.index() * 2 + self.slot
    }

    pub(super) fn mask(self) -> u8 {
        1 << self.index()
    }

    pub(super) fn loc(self, source: Self) -> i8 {
        (self.slot as i8 + 1) * if self.player == source.player { -1 } else { 1 }
    }

    fn label(self) -> String {
        format!("{}{}", self.player.id(), char::from(b'a' + self.slot as u8))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pivot {
    Move(&'static str),
    Reactive,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) enum Protection {
    Protect,
    KingsShield,
    BanefulBunker,
    SpikyShield,
    BurningBulwark,
    Obstruct,
    SilkTrap,
    Endure,
}

#[derive(Clone, Copy)]
enum MoveInvocation {
    Choice {
        slot: usize,
        loc: i8,
        original_target: Option<(Player, u8)>,
        prankster: bool,
        round_followup: bool,
        z_priority: Option<i8>,
    },
    Reflected {
        data: &'static Move,
        target: Position,
    },
    Future {
        data: &'static Move,
        target: Position,
    },
    SleepTalk {
        slot: usize,
        caller: usize,
        priority: i8,
        prankster: bool,
    },
    Copycat {
        slot: usize,
        caller: usize,
        priority: i8,
        prankster: bool,
    },
}

#[derive(Default)]
struct MoveResult {
    spends_charge: bool,
    spends_stockpile: bool,
    ran_after_move: bool,
    failed: bool,
    counts_as_success: bool,
}

pub(super) enum BoostLabel<'a> {
    Item(&'a str),
    ZPower,
}

impl Protection {
    fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Protect" | "Detect" => Self::Protect,
            "King's Shield" => Self::KingsShield,
            "Baneful Bunker" => Self::BanefulBunker,
            "Spiky Shield" => Self::SpikyShield,
            "Burning Bulwark" => Self::BurningBulwark,
            "Obstruct" => Self::Obstruct,
            "Silk Trap" => Self::SilkTrap,
            "Endure" => Self::Endure,
            _ => return None,
        })
    }

    fn volatile(self) -> &'static str {
        match self {
            Self::Protect => "protect",
            Self::KingsShield => "kingsshield",
            Self::BanefulBunker => "banefulbunker",
            Self::SpikyShield => "spikyshield",
            Self::BurningBulwark => "burningbulwark",
            Self::Obstruct => "obstruct",
            Self::SilkTrap => "silktrap",
            Self::Endure => "endure",
        }
    }

    fn blocks_status(self) -> bool {
        matches!(
            self,
            Self::Protect | Self::BanefulBunker | Self::SpikyShield
        )
    }

    fn start_label(self) -> &'static str {
        match self {
            Self::BanefulBunker | Self::SpikyShield | Self::BurningBulwark => "move: Protect",
            Self::Endure => "move: Endure",
            _ => "Protect",
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(super) struct FoeHit {
    source: Position,
    damage: u32,
    turn: u32,
}

#[derive(Debug, Copy, Clone)]
enum CounterMove {
    Counter,
    MirrorCoat,
}

impl CounterMove {
    fn for_move(data: &Move) -> Option<Self> {
        match data.name.as_str() {
            "Counter" => Some(Self::Counter),
            "Mirror Coat" => Some(Self::MirrorCoat),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::MirrorCoat => "mirrorcoat",
        }
    }

    fn category(self) -> Category {
        match self {
            Self::Counter => Category::Physical,
            Self::MirrorCoat => Category::Special,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(super) struct PreparedCounter {
    move_kind: CounterMove,
    hit: Option<FoeHit>,
}

#[derive(Debug, Copy, Clone)]
enum ActionKind {
    Move {
        move_slot: usize,
        target: i8,
        mega: bool,
    },
    Switch {
        bench_slot: usize,
        immediate: bool,
        pivot: Option<Pivot>,
    },
    Revive {
        party_slot: usize,
    },
    PrepareCounter {
        move_kind: CounterMove,
        target: i8,
    },
    RunSwitch,
    MegaEvolve,
    PriorityCharge(&'static str),
    BeforeTurn,
    Residual,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct Action {
    actor: Option<Position>,
    kind: ActionKind,
    // Use the originally selected move when re-sorting after a faint can
    // restore the actor's original moveset (Transform/Imposter).
    move_data: Option<&'static Move>,
    original_target: Option<(Player, u8)>,
    priority: Priority,
    fractional_priority: f64,
    prankster: bool,
    round_followup: bool,
    z_move: bool,
}

const STRUGGLE_SLOT: usize = usize::MAX;
const RECHARGE_SLOT: usize = usize::MAX - 1;
const EXTRA_MOVE_SLOT: usize = usize::MAX - 2;

impl Action {
    fn new(actor: Option<Position>, kind: ActionKind) -> Self {
        let order = match kind {
            ActionKind::Switch {
                immediate: true, ..
            } => 3,
            ActionKind::BeforeTurn => 4,
            ActionKind::PrepareCounter { .. } => 5,
            ActionKind::Revive { .. } => 6,
            ActionKind::RunSwitch => 101,
            ActionKind::Switch {
                immediate: false, ..
            } => 103,
            ActionKind::MegaEvolve => 104,
            ActionKind::PriorityCharge(_) => 107,
            ActionKind::Move { .. } => 200,
            ActionKind::Residual => 300,
        };
        Self {
            actor,
            kind,
            move_data: None,
            original_target: None,
            fractional_priority: 0.0,
            prankster: false,
            round_followup: false,
            z_move: false,
            priority: Priority {
                order,
                ..Priority::default()
            },
        }
    }
}

impl Battle {
    pub fn turn(&self) -> u32 {
        self.turn
    }

    pub fn forced_move_id(&self, player: Player, slot: usize) -> Option<&str> {
        self.party(player)?
            .get(slot)?
            .forced_move()
            .map(|(_, _, id)| id)
    }

    pub fn can_mega_in_slot(&self, player: Player, slot: usize) -> bool {
        self.party(player)
            .and_then(|p| p.get(slot))
            .is_some_and(|p| !p.fainted)
            && self.any_move_in_request(Position { player, slot })
            && self.can_mega(Position { player, slot })
    }

    pub fn waiting(&self, player: Player) -> bool {
        matches!(self.phase, Phase::Setup | Phase::Ended)
            || (self.phase == Phase::Replace && !self.force_switch(player).contains(&true))
            || self.submitted(player)
    }

    pub fn submitted(&self, player: Player) -> bool {
        self.pending_actions[player.index()].is_some()
            || self.pending_preview[player.index()].is_some()
    }

    pub fn winner(&self) -> Option<Player> {
        self.winner
    }

    pub fn is_active(&self, player: Player, party_slot: usize) -> bool {
        self.turn > 0
            && party_slot < self.slots()
            && self
                .party(player)
                .and_then(|p| p.get(party_slot))
                .is_some_and(|mon| !mon.fainted)
    }

    /// Serialize the current Showdown request for its owner. This is not part
    /// of the move hot path or the Poke-RL observation schema.
    pub fn request(&self, player: Player) -> Option<serde_json::Value> {
        use serde_json::{json, to_value};
        match self.phase {
            Phase::Setup | Phase::Ended => None,
            Phase::Preview => self.preview_request(player).map(|r| to_value(r).unwrap()),
            Phase::Move => {
                let mut request = json!({
                "active": (0..self.slots()).map(|slot| {
                    let mon = self.mon(Position { player, slot });
                    let pos = Position { player, slot };
                    if let Some((_, name, id)) = mon.forced_move() {
                        return json!({"moves":[{"move":name,"id":id}],"trapped":true});
                    }
                    let moves = if !self.any_move_in_request(pos) && mon.revealed_disables == 0 {
                        vec![json!({"move":"Struggle","id":"struggle","target":"randomNormal","disabled":false})]
                    } else {
                        mon.moves.iter().enumerate().map(|(move_slot, m)| {
                            let mut request = json!({
                                "move": mon.request_move_name(m), "id": m.id, "pp": m.pp, "maxpp": m.max_pp,
                                "target": mon.requested_target(m.move_data), "disabled": self.move_disabled_in_request(player, slot, move_slot),
                            });
                            if let Some(source) = self.disable_source(pos, move_slot) {
                                request["disabledSource"] = json!(source);
                            }
                            request
                        }).collect()
                    };
                    let mut request = json!({"moves":moves});
                    if !mon.fainted && self.last_active(pos) && self.foe_imprison(pos).next().is_some() && !mon.revealed_all_disables {
                        request["maybeDisabled"] = json!(true);
                        if mon.revealed_disables == 0 { request["maybeLocked"] = json!(true); }
                    }
                    if self.trapped_in_request(player, slot) {
                        request["trapped"] = json!(true);
                    } else if self.last_active(pos) && self.first_bench(player, 0).is_some() && self.trap_info(pos).possible {
                        request["maybeTrapped"] = json!(true);
                    }
                    if self.can_mega(pos) && self.any_move_in_request(pos) {
                        // A fainted active still has a Showdown request entry.
                        // It can advertise the form even though it cannot act.
                        request["canMegaEvo"] = json!(true);
                    }
                    if self.can_ultra_burst(pos) {
                        request["canUltraBurst"] = json!(true);
                    }
                    if let Some(z_moves) = self.z_move_requests(pos) {
                        request["canZMove"] = json!(z_moves);
                    }
                    request
                }).collect::<Vec<_>>(),
                "side": self.side_request(player).unwrap(),
                });
                if self.party(player).unwrap()[..self.slots()]
                    .iter()
                    .any(|p| p.revealed_trap || p.revealed_disables != 0)
                {
                    request["update"] = json!(true);
                }
                Some(request)
            }
            Phase::Replace => {
                let flags = self.force_switch(player);
                if flags.iter().any(|x| *x) {
                    let mut request =
                        json!({"forceSwitch": flags, "side": self.side_request(player).unwrap()});
                    if !self.force_switch(player.foe()).contains(&true) {
                        request["noCancel"] = json!(true);
                    }
                    Some(request)
                } else {
                    Some(json!({"wait": true, "side": self.side_request(player).unwrap()}))
                }
            }
        }
    }

    pub(super) fn slots(&self) -> usize {
        self.format.game_type.active_slots()
    }

    pub(super) fn mon(&self, position: Position) -> &Pokemon {
        &self.party(position.player).unwrap()[position.slot]
    }

    pub(super) fn mon_mut(&mut self, position: Position) -> &mut Pokemon {
        &mut self.sides[position.player.index()].as_mut().unwrap().party[position.slot]
    }

    pub(super) fn ident(&self, position: Position) -> String {
        let display = self
            .disguise(position)
            .unwrap_or_else(|| self.mon(position));
        format!("{}: {}", position.label(), display.name)
    }

    pub(super) fn move_still(&mut self, index: usize) {
        let line = &mut self.log[index].public;
        let mut delimiters = line.match_indices('|');
        let start = delimiters.nth(3).unwrap().0 + 1;
        let end = delimiters.next().map_or(line.len(), |(offset, _)| offset);
        line.replace_range(start..end, "");
        line.push_str("|[still]");
    }

    pub(super) fn report_no_move_effect(&mut self) {
        if self.format.report_exact_hp {
            self.add("|debug|move failed because it did nothing");
        }
    }

    fn protection_contact(
        &mut self,
        protect: Protection,
        source: Position,
        holder: Position,
        move_log: usize,
    ) {
        let data = || DEX.moves(self.format.dex_mod, protect.volatile()).unwrap();
        let drop = match protect {
            Protection::Protect | Protection::Endure => return,
            Protection::KingsShield => (BoostStat::Atk, -1),
            Protection::Obstruct => (BoostStat::Def, -2),
            Protection::SilkTrap => (BoostStat::Spe, -1),
            Protection::BanefulBunker | Protection::BurningBulwark => {
                let id = if protect == Protection::BanefulBunker {
                    "psn"
                } else {
                    "brn"
                };
                self.try_status(source, id, holder, data(), false, move_log);
                return;
            }
            Protection::SpikyShield => {
                self.indirect_damage(
                    source,
                    8,
                    &format!("|[from] Spiky Shield|[of] {}", self.ident(holder)),
                );
                return;
            }
        };
        self.boost_from(source, holder, &[drop], BoostOrigin::from_move(data()));
    }

    fn add_health_line(&mut self, event: &str, target: Position, details: bool) {
        self.add_health_line_with(event, target, details, "");
    }

    pub(super) fn add_health_line_with(
        &mut self,
        event: &str,
        target: Position,
        details: bool,
        suffix: &str,
    ) {
        let head = self.ident(target);
        self.add_health_with_ident(event, target, &head, details, suffix);
    }

    fn add_health_with_ident(
        &mut self,
        event: &str,
        target: Position,
        ident: &str,
        details: bool,
        suffix: &str,
    ) {
        let mon = self.mon(target);
        let condition = health(
            mon.hp,
            mon.max_hp,
            mon.status.id(),
            self.format.report_exact_hp,
        );
        let mut head = format!("|{event}|{ident}|");
        if details {
            head.push_str(&self.displayed_details(target));
            head.push('|');
        }
        self.log.push(LogLine {
            public: format!("{head}{}{suffix}", condition.shared),
            secret: Some((target.player, format!("{head}{}{suffix}", condition.secret))),
        });
    }

    pub(super) fn start_selected(&mut self) -> Result<(), BattleError> {
        self.add("|");
        for player in Player::ALL {
            self.add(format!(
                "|teamsize|{}|{}",
                player.id(),
                self.party(player).unwrap().len()
            ));
        }
        self.add("|start");
        let mut queue = Vec::new();
        for player in Player::ALL {
            for slot in 0..self.slots() {
                let pos = Position { player, slot };
                self.prepare_illusion(pos);
                self.add_health_line("switch", pos, true);
                self.insert_run_switch(pos, &mut queue);
            }
        }
        self.run_queue(queue)
    }

    fn force_switch(&self, player: Player) -> Vec<bool> {
        let party = self.party(player).unwrap();
        let bench = party[self.slots()..].iter().any(|p| !p.fainted);
        party[..self.slots()]
            .iter()
            .map(|p| p.reviving || bench && (p.needs_replacement || p.pivot_move.is_some()))
            .collect()
    }

    pub(super) fn emergency_exit(&mut self, pos: Position, original_hp: u32) {
        let mon = self.mon(pos);
        let half = mon.max_hp.get() / 2;
        if original_hp <= half
            || mon.hp == 0
            || mon.hp > half
            || mon.pivot_move.is_some()
            || self.drag_flags & pos.mask() != 0
            || mon.reviving
        {
            return;
        }
        let ability = match self.event_ability(pos) {
            "emergencyexit" => "Emergency Exit",
            "wimpout" => "Wimp Out",
            _ => return,
        };
        if self.first_bench(pos.player, 0).is_some() {
            self.mon_mut(pos).pivot_move = Some(Pivot::Reactive);
            self.add(format!("|-activate|{}|ability: {ability}", self.ident(pos)));
        }
    }

    pub fn is_reviving(&self, player: Player, slot: usize) -> bool {
        self.phase == Phase::Replace
            && self
                .party(player)
                .and_then(|p| p.get(slot))
                .is_some_and(|p| p.reviving)
    }

    pub(super) fn choose_action(
        &mut self,
        player: Player,
        choice: Choice,
    ) -> Result<(), BattleError> {
        let idx = player.index();
        let required = self.phase == Phase::Move || self.force_switch(player).contains(&true);
        if !required {
            return Err(BattleError::Choice(
                "this player is waiting for the other side",
            ));
        }
        if self.cant_undo[idx] {
            return Err(BattleError::Choice(
                "cannot undo without revealing a hidden effect",
            ));
        }
        let choices = match choice {
            Choice::Actions(choices) => choices,
            Choice::Undo => {
                if self.phase == Phase::Replace && !self.force_switch(player.foe()).contains(&true)
                {
                    return Err(BattleError::Choice("the replacement cannot be canceled"));
                }
                self.pending_actions[idx] = None;
                return Ok(());
            }
            Choice::Team(_) => return Err(BattleError::Choice("team preview has finished")),
        };

        self.pending_actions[idx] = None;
        let actions = self.resolve_choices(player, &choices)?;
        if self.phase == Phase::Move {
            for action in &actions {
                if matches!(action.kind, ActionKind::Switch { .. }) {
                    let pos = action.actor.unwrap();
                    let trap = self.trap_info(pos);
                    if trap.actual {
                        if self.trapped_in_request(player, pos.slot) {
                            return Err(BattleError::Choice("active Pokémon is trapped"));
                        }
                        self.mon_mut(pos).revealed_trap = true;
                        return Err(BattleError::Unavailable("active Pokémon is trapped"));
                    }
                    self.cant_undo[idx] |= self.last_active(pos) && trap.possible;
                } else if let ActionKind::Move { move_slot, .. } = action.kind {
                    let pos = action.actor.unwrap();
                    if move_slot != STRUGGLE_SLOT
                        && !action.z_move
                        && !self.mon(pos).hard_locked()
                        && self.actual_disabled(pos, move_slot)
                    {
                        if self.visible_disable(pos, move_slot) {
                            return Err(BattleError::Choice("move is disabled"));
                        }
                        return Err(self.reveal_disabled_move(pos, move_slot));
                    }
                    self.cant_undo[idx] |= !self.mon(pos).hard_locked()
                        && self.slots() == 1
                        && self.foe_imprison(pos).next().is_some();
                }
            }
        }
        self.pending_actions[idx] = Some(actions);
        if Player::ALL.into_iter().all(|p| {
            self.pending_actions[p.index()].is_some()
                || (self.phase == Phase::Replace && !self.force_switch(p).contains(&true))
        }) {
            self.cant_undo = [false; 2];
            self.update_speed();
            let mut queue = Vec::new();
            for p in Player::ALL {
                for mut action in self.pending_actions[p.index()].take().unwrap_or_default() {
                    if matches!(action.kind, ActionKind::Revive { .. }) {
                        self.mon_mut(action.actor.unwrap()).reviving = false;
                    }
                    if let ActionKind::Switch {
                        immediate: true, ..
                    } = action.kind
                    {
                        let mon = self.mon_mut(action.actor.unwrap());
                        mon.needs_replacement = false;
                        mon.pivot_move = None;
                    }
                    if matches!(action.kind, ActionKind::Move { mega: true, .. }) {
                        let mut mega = Action::new(action.actor, ActionKind::MegaEvolve);
                        self.prioritize(&mut mega);
                        queue.push(mega);
                    }
                    if self.phase == Phase::Move
                        && !action.z_move
                        && let ActionKind::Move {
                            move_slot, target, ..
                        } = action.kind
                        && move_slot != RECHARGE_SLOT
                        && let Some(move_kind) =
                            CounterMove::for_move(self.move_data(action.actor.unwrap(), move_slot))
                    {
                        let actor = action.actor.unwrap();
                        let target = if target != 0 {
                            target
                        } else {
                            self.random_target(actor, TargetKind::Scripted)
                                .map_or(0, |p| p.loc(actor))
                        };
                        let mut prep = Action::new(
                            Some(actor),
                            ActionKind::PrepareCounter { move_kind, target },
                        );
                        self.prioritize(&mut prep);
                        queue.push(prep);
                    }
                    if self.phase == Phase::Move
                        && !action.z_move
                        && let ActionKind::Move { move_slot, .. } = action.kind
                        && move_slot != RECHARGE_SLOT
                        && let Some(id) = match self
                            .move_data(action.actor.unwrap(), move_slot)
                            .name
                            .as_str()
                        {
                            "Focus Punch" => Some("focuspunch"),
                            "Chilly Reception" => Some("chillyreception"),
                            "Beak Blast" => Some("beakblast"),
                            _ => None,
                        }
                    {
                        let mut charge = Action::new(action.actor, ActionKind::PriorityCharge(id));
                        // The separate charge action has no explicit target even
                        // when the actual move does. Showdown resolves both.
                        let actor = action.actor.unwrap();
                        self.random_target(actor, self.move_data(actor, move_slot).target);
                        self.prioritize(&mut charge);
                        queue.push(charge);
                    }
                    if matches!(action.kind, ActionKind::Move { .. }) {
                        self.assign_fractional_priority(&mut action);
                    }
                    if let ActionKind::Move {
                        move_slot,
                        target: 0,
                        mega,
                    } = action.kind
                        && !self.mon(action.actor.unwrap()).hard_locked()
                    {
                        let source = action.actor.unwrap();
                        let kind = self
                            .mon(source)
                            .requested_target(self.move_data(source, move_slot));
                        let target = self
                            .random_target(source, kind)
                            .map_or(0, |p| p.loc(source));
                        action.kind = ActionKind::Move {
                            move_slot,
                            target,
                            mega,
                        };
                    }
                    self.bind_original_target(&mut action);
                    self.prioritize(&mut action);
                    queue.push(action);
                }
            }
            speed_sort_by(&mut queue, &mut self.rng, |a, b| {
                a.priority.compare(b.priority)
            });
            self.add("|");
            if self.phase == Phase::Move {
                queue.insert(0, Action::new(None, ActionKind::BeforeTurn));
                queue.push(Action::new(None, ActionKind::Residual));
            } else {
                queue.append(&mut self.suspended);
            }
            self.run_queue(queue)?;
        }
        Ok(())
    }

    fn resolve_choices(
        &self,
        player: Player,
        choices: &[SlotChoice],
    ) -> Result<Vec<Action>, BattleError> {
        let count = self.slots();
        if choices.len() > count {
            return Err(BattleError::Choice("too many slot choices"));
        }
        let party = self.party(player).unwrap();
        let replace = self.phase == Phase::Replace;
        let flags = self.force_switch(player);
        let mut needed = flags
            .iter()
            .filter(|&&x| x)
            .count()
            .min(party[count..].iter().filter(|p| !p.fainted).count());
        let mut choices = choices.iter().peekable();
        let mut actions = Vec::new();
        let mut switched = 0_u32;
        let mut auto = false;
        let mut mega = false;
        let mut ultra = false;
        let mut z_move = false;
        for (slot, force_switch) in flags.into_iter().enumerate() {
            let actor = Position { player, slot };
            let mon = self.mon(actor);
            let needs_action = if replace { force_switch } else { !mon.fainted };
            if !needs_action {
                if choices.peek() == Some(&&SlotChoice::Pass) {
                    choices.next();
                }
                continue;
            }
            let choice = if auto {
                &SlotChoice::Auto
            } else {
                choices
                    .next()
                    .ok_or(BattleError::Choice("incomplete choices"))?
            };
            auto |= matches!(choice, SlotChoice::Auto);
            if replace && mon.reviving && matches!(choice, SlotChoice::Auto | SlotChoice::Switch(_))
            {
                let party_slot = match choice {
                    SlotChoice::Switch(Some(i)) => i.get() - 1,
                    _ => party
                        .iter()
                        .position(|p| p.fainted)
                        .ok_or(BattleError::Choice("no fainted Pokémon to revive"))?,
                };
                let target =
                    party
                        .get(party_slot)
                        .filter(|p| p.fainted)
                        .ok_or(BattleError::Choice(
                            "Revival Blessing requires a fainted Pokémon",
                        ))?;
                if party_slot < count
                    && let Some(error) = target.entry_error()
                {
                    return Err(error);
                }
                needed = needed.saturating_sub(1);
                actions.push(Action::new(Some(actor), ActionKind::Revive { party_slot }));
                continue;
            }
            let mut z_powered = false;
            let action = match choice {
                SlotChoice::Auto if replace => {
                    if switched.count_ones() as usize == needed {
                        continue;
                    }
                    let bench_slot = self
                        .first_bench(player, switched)
                        .ok_or(BattleError::Choice("no legal replacement"))?;
                    ActionKind::Switch {
                        bench_slot,
                        immediate: true,
                        pivot: mon.pivot_move,
                    }
                }
                SlotChoice::Switch(bench_slot) => {
                    let bench_slot = match bench_slot {
                        Some(i) => i.get() - 1,
                        None => self
                            .first_bench(player, switched)
                            .ok_or(BattleError::Choice("no legal switch"))?,
                    };
                    if !(count..party.len()).contains(&bench_slot)
                        || party[bench_slot].fainted
                        || switched & (1 << bench_slot) != 0
                    {
                        return Err(BattleError::Choice("invalid or duplicate switch slot"));
                    }
                    ActionKind::Switch {
                        bench_slot,
                        immediate: replace,
                        pivot: mon.pivot_move,
                    }
                }
                SlotChoice::Pass if replace && !mon.reviving => continue,
                SlotChoice::Move {
                    selection,
                    target,
                    special,
                } if !replace => {
                    let wants_mega = match special {
                        None | Some(SpecialChoice::ZMove) => false,
                        Some(SpecialChoice::Mega) if !mega => {
                            self.validate_mega(actor)?;
                            mega = true;
                            true
                        }
                        Some(SpecialChoice::Mega) => {
                            return Err(BattleError::Choice("only one Mega Evolution per side"));
                        }
                        Some(SpecialChoice::Ultra) if !ultra => {
                            self.validate_ultra_burst(actor)?;
                            ultra = true;
                            true
                        }
                        Some(SpecialChoice::Ultra) => {
                            return Err(BattleError::Choice("only one Ultra Burst per side"));
                        }
                        _ => return Err(BattleError::Unsupported("special move modifier".into())),
                    };
                    let (action, selected_z) = self.select_move(
                        actor,
                        selection,
                        *target,
                        wants_mega,
                        *special == Some(SpecialChoice::ZMove),
                    )?;
                    if selected_z && z_move {
                        return Err(BattleError::Choice("only one Z-Move per side"));
                    }
                    z_move |= selected_z;
                    z_powered = selected_z;
                    action
                }
                SlotChoice::Auto => {
                    self.select_move(actor, &MoveChoice::Auto, None, false, false)?
                        .0
                }
                _ => return Err(BattleError::Choice("invalid action for current request")),
            };
            if let ActionKind::Switch { bench_slot, .. } = action {
                if let Some(error) = party[bench_slot].entry_error() {
                    return Err(error);
                }
                switched |= 1 << bench_slot;
            }
            let mut action = Action::new(Some(actor), action);
            action.z_move = z_powered;
            actions.push(action);
        }
        if choices.next().is_some() || (replace && switched.count_ones() as usize != needed) {
            return Err(BattleError::Choice(
                "wrong number of replacement or slot choices",
            ));
        }
        Ok(actions)
    }

    pub(super) fn first_bench(&self, player: Player, exclude: u32) -> Option<usize> {
        self.party(player)
            .unwrap()
            .iter()
            .enumerate()
            .skip(self.slots())
            .find(|(slot, p)| !p.fainted && exclude & (1 << slot) == 0)
            .map(|(i, _)| i)
    }

    fn move_supported(info: &MoveSlot) -> bool {
        info.move_data.generic
            || matches!(
                info.id.as_str(),
                "tackle"
                    | "quickattack"
                    | "afteryou"
                    | "allyswitch"
                    | "electrify"
                    | "roleplay"
                    | "reflecttype"
                    | "gastroacid"
                    | "magicroom"
                    | "copycat"
                    | "hail"
                    | "celebrate"
                    | "splash"
                    | "happyhour"
                    | "refresh"
                    | "terablast"
                    | "falseswipe"
                    | "holdback"
                    | "brine"
                    | "retaliate"
                    | "incinerate"
                    | "dreameater"
                    | "dragonrage"
                    | "sonicboom"
                    | "wringout"
                    | "crushgrip"
                    | "naturesmadness"
                    | "ruination"
                    | "wakeupslap"
                    | "smellingsalts"
                    | "synchronoise"
                    | "fishiousrend"
                    | "boltbeak"
                    | "revelationdance"
                    | "photongeyser"
                    | "bleakwindstorm"
                    | "sandsearstorm"
                    | "wildboltstorm"
                    | "chloroblast"
                    | "collisioncourse"
                    | "electrodrift"
                    | "psyblade"
                    | "hyperspacehole"
                    | "hyperspacefury"
                    | "shoreup"
                    | "floralhealing"
                    | "junglehealing"
                    | "lunarblessing"
                    | "takeheart"
                    | "purify"
                    | "psychoshift"
                    | "autotomize"
                    | "camouflage"
                    | "conversion"
                    | "tarshot"
                    | "foresight"
                    | "odorsleuth"
                    | "miracleeye"
                    | "laserfocus"
                    | "luckychant"
                    | "healblock"
                    | "freezeshock"
                    | "iceburn"
                    | "razorwind"
                    | "furycutter"
                    | "echoedvoice"
                    | "fusionbolt"
                    | "fusionflare"
                    | "rollout"
                    | "iceball"
                    | "defensecurl"
                    | "coreenforcer"
                    | "flameburst"
                    | "spectralthief"
                    | "thousandarrows"
                    | "thousandwaves"
                    | "captivate"
                    | "filletaway"
                    | "flowershield"
                    | "gearup"
                    | "holdhands"
                    | "powershift"
                    | "rototiller"
                    | "venomdrench"
                    | "magnitude"
                    | "present"
                    | "psywave"
                    | "punishment"
                    | "trumpcard"
                    | "secretpower"
                    | "judgment"
                    | "multiattack"
                    | "technoblast"
                    | "ivycudgel"
                    | "naturalgift"
                    | "iondeluge"
                    | "plasmafists"
                    | "watersport"
                    | "mudsport"
                    | "round"
                    | "swift"
                    | "surf"
                    | "watergun"
                    | "iceshard"
                    | "protect"
                    | "kingsshield"
                    | "aquajet"
                    | "bulletpunch"
                    | "earthquake"
                    | "leafblade"
                    | "closecombat"
                    | "makeitrain"
                    | "dracometeor"
                    | "leafstorm"
                    | "swordsdance"
                    | "nastyplot"
                    | "dragondance"
                    | "calmmind"
                    | "bulkup"
                    | "irondefense"
                    | "amnesia"
                    | "recover"
                    | "slackoff"
                    | "softboiled"
                    | "milkdrink"
                    | "flamethrower"
                    | "heatwave"
                    | "ember"
                    | "willowisp"
                    | "thunderwave"
                    | "toxic"
                    | "hypnosis"
                    | "sleeppowder"
                    | "poisonjab"
                    | "sludgebomb"
                    | "icebeam"
                    | "thunderbolt"
                    | "nuzzle"
                    | "rockslide"
                    | "shadowball"
                    | "moonblast"
                    | "raindance"
                    | "sunnyday"
                    | "sandstorm"
                    | "snowscape"
                    | "stealthrock"
                    | "spikes"
                    | "toxicspikes"
                    | "stickyweb"
                    | "uturn"
                    | "voltswitch"
                    | "flipturn"
                    | "partingshot"
                    | "imprison"
                    | "knockoff"
                    | "dragonclaw"
                    | "dragonpulse"
                    | "aurasphere"
                    | "darkpulse"
                    | "psychic"
                    | "crunch"
                    | "ironhead"
                    | "playrough"
                    | "rocktomb"
                    | "airslash"
                    | "icywind"
                    | "discharge"
                    | "disarmingvoice"
                    | "hypervoice"
                    | "bodypress"
                    | "roost"
                    | "freezedry"
                    | "scald"
                    | "acidarmor"
                    | "bravebird"
                    | "doubleedge"
                    | "gigadrain"
                    | "drainpunch"
                    | "headsmash"
                    | "lightofruin"
                    | "flareblitz"
                    | "wavecrash"
                    | "woodhammer"
                    | "energyball"
                    | "flashcannon"
                    | "firepunch"
                    | "icepunch"
                    | "thunderpunch"
                    | "pyroball"
                    | "extremespeed"
                    | "megakick"
                    | "hydropump"
                    | "lavaplume"
                    | "overheat"
                    | "shadowsneak"
                    | "bittermalice"
                    | "bugbuzz"
                    | "quiverdance"
                    | "shellsmash"
                    | "stoneedge"
                    | "bite"
                    | "mysticalfire"
                    | "liquidation"
                    | "earthpower"
                    | "glare"
                    | "rest"
                    | "yawn"
                    | "moonlight"
                    | "synthesis"
                    | "morningsun"
                    | "auroraveil"
                    | "reflect"
                    | "lightscreen"
                    | "disable"
                    | "lastresort"
                    | "weatherball"
                    | "hurricane"
                    | "thunder"
                    | "blizzard"
                    | "taunt"
                    | "fakeout"
                    | "firstimpression"
                    | "glaiverush"
                    | "poltergeist"
                    | "courtchange"
                    | "growth"
                    | "strengthsap"
                    | "painsplit"
                    | "tripleaxel"
                    | "triplekick"
                    | "trickroom"
                    | "electricterrain"
                    | "grassyterrain"
                    | "mistyterrain"
                    | "psychicterrain"
                    | "icespinner"
                    | "stoneaxe"
                    | "ceaselessedge"
                    | "defog"
                    | "pollenpuff"
                    | "curse"
                    | "futuresight"
                    | "doomdesire"
                    | "belch"
                    | "transform"
                    | "dragondarts"
                    | "grassyglide"
                    | "encore"
                    | "tidyup"
                    | "storedpower"
                    | "powertrip"
                    | "supercellslam"
                    | "highjumpkick"
                    | "jumpkick"
                    | "dragontail"
                    | "circlethrow"
                    | "metalburst"
                    | "comeuppance"
                    | "seismictoss"
                    | "nightshade"
                    | "superfang"
                    | "followme"
                    | "ragepowder"
                    | "alluringvoice"
                    | "direclaw"
                    | "leechseed"
                    | "fissure"
                    | "guillotine"
                    | "horndrill"
                    | "sheercold"
                    | "destinybond"
                    | "foulplay"
                    | "trick"
                    | "switcheroo"
                    | "skillswap"
                    | "worryseed"
                    | "simplebeam"
                    | "entrainment"
                    | "hiddenpower"
                    | "return"
                    | "frustration"
                    | "assurance"
                    | "infernalparade"
                    | "clearsmog"
                    | "healbell"
                    | "aromatherapy"
                    | "swagger"
                    | "flatter"
                    | "magicpowder"
                    | "ingrain"
                    | "gravity"
                    | "telekinesis"
                    | "embargo"
                    | "powder"
                    | "mist"
                    | "craftyshield"
                    | "matblock"
                    | "spotlight"
                    | "gravapple"
                    | "steelroller"
                    | "fellstinger"
                    | "powersplit"
                    | "guardsplit"
                    | "powerswap"
                    | "guardswap"
                    | "heartswap"
                    | "speedswap"
                    | "wonderroom"
                    | "electroshot"
                    | "solarbeam"
                    | "solarblade"
                    | "meteorbeam"
                    | "skullbash"
                    | "geomancy"
                    | "expandingforce"
                    | "dive"
                    | "dig"
                    | "bounce"
                    | "fly"
                    | "flyingpress"
                    | "charge"
                    | "throatchop"
                    | "lashout"
                    | "stompingtantrum"
                    | "reversal"
                    | "flail"
                    | "payback"
                    | "acrobatics"
                    | "hardpress"
                    | "risingvoltage"
                    | "confuseray"
                    | "sweetkiss"
                    | "supersonic"
                    | "bellydrum"
                    | "quash"
                    | "thief"
                    | "covet"
                    | "healpulse"
                    | "lifedew"
                    | "finalgambit"
                    | "aurawheel"
                    | "phantomforce"
                    | "shadowforce"
                    | "skyattack"
                    | "banefulbunker"
                    | "spikyshield"
                    | "burningbulwark"
                    | "obstruct"
                    | "silktrap"
                    | "wideguard"
                    | "quickguard"
                    | "endure"
                    | "avalanche"
                    | "revenge"
                    | "temperflare"
                    | "coaching"
                    | "burningjealousy"
                    | "psychicnoise"
                    | "gyroball"
                    | "electroball"
                    | "terrainpulse"
                    | "upperhand"
                    | "aquaring"
                    | "instruct"
                    | "shellsidearm"
                    | "sleeptalk"
                    | "torment"
                    | "octolock"
                    | "anchorshot"
                    | "spiritshackle"
                    | "jawlock"
                    | "fling"
                    | "stockpile"
                    | "spitup"
                    | "swallow"
                    | "smackdown"
                    | "magnetrise"
                    | "block"
                    | "meanlook"
                    | "spiderweb"
                    | "howl"
                    | "teeterdance"
                    | "memento"
                    | "minimize"
                    | "aromaticmist"
                    | "dragoncheer"
                    | "powertrick"
                    | "psychup"
                    | "topsyturvy"
                    | "attract"
                    | "axekick"
                    | "eeriespell"
                    | "snore"
                    | "spite"
                    | "triattack"
                    | "corrosivegas"
                    | "stuffcheeks"
                    | "teatime"
                    | "fairylock"
                    | "lockon"
                    | "mindreader"
                    | "syrupbomb"
                    | "safeguard"
                    | "acupressure"
                    | "magneticflux"
                    | "bugbite"
                    | "pluck"
                    | "beatup"
                    | "watershuriken"
                    | "trickortreat"
                    | "forestscurse"
                    | "uproar"
                    | "suckerpunch"
                    | "thunderclap"
                    | "grassknot"
                    | "lowkick"
                    | "heavyslam"
                    | "heatcrash"
                    | "endeavor"
                    | "eruption"
                    | "waterspout"
                    | "dragonenergy"
                    | "wish"
                    | "recycle"
                    | "healingwish"
                    | "lunardance"
                    | "tailwind"
                    | "lastrespects"
                    | "ragefist"
                    | "saltcure"
                    | "detect"
                    | "rapidspin"
                    | "whirlwind"
                    | "roar"
                    | "explosion"
                    | "selfdestruct"
                    | "mistyexplosion"
                    | "mindblown"
                    | "soak"
                    | "ragingbull"
                    | "brickbreak"
                    | "psychicfangs"
                    | "feint"
                    | "facade"
                    | "focusenergy"
                    | "helpinghand"
                    | "noretreat"
                    | "perishsong"
                    | "focuspunch"
                    | "beakblast"
                    | "revivalblessing"
                    | "chillyreception"
                    | "ficklebeam"
                    | "barbbarrage"
                    | "venoshock"
                    | "hex"
                    | "haze"
                    | "mortalspin"
                    | "substitute"
                    | "shedtail"
                    | "doubleshock"
                    | "burnup"
                    | "sparklingaria"
                    | "counter"
                    | "mirrorcoat"
                    | "clangoroussoul"
                    | "steelbeam"
                    | "bind"
                    | "clamp"
                    | "firespin"
                    | "infestation"
                    | "magmastorm"
                    | "sandtomb"
                    | "snaptrap"
                    | "thundercage"
                    | "whirlpool"
                    | "wrap"
                    | "batonpass"
            )
    }

    fn select_move(
        &self,
        actor: Position,
        selection: &MoveChoice,
        target: Option<i8>,
        mega: bool,
        mut z_powered: bool,
    ) -> Result<(ActionKind, bool), BattleError> {
        if let Some((move_slot, _, id)) = self.mon(actor).forced_move() {
            let valid = match selection {
                MoveChoice::Auto => true,
                MoveChoice::Slot(slot) => slot.get() == 1 && target.is_none(),
                MoveChoice::Name(name) => {
                    name == id
                        && (self.slots() == 1 || target.is_some())
                        && target.is_none_or(|loc| {
                            TargetKind::Normal.valid_location(
                                self.slots() as u8,
                                actor.slot as u8,
                                loc,
                            )
                        })
                }
            };
            if !valid || mega || z_powered {
                return Err(BattleError::Choice("only the forced move is available"));
            }
            return Ok((
                ActionKind::Move {
                    move_slot,
                    target: self
                        .mon(actor)
                        .charge
                        .map(|state| state.target)
                        .or_else(|| self.mon(actor).rolling_attack.map(|state| state.target))
                        .unwrap_or(0),
                    mega: false,
                },
                false,
            ));
        }
        let moves = &self.mon(actor).moves;
        let visibly_available = self.any_move_in_request(actor);
        let really_available = moves
            .iter()
            .enumerate()
            .any(|(i, _)| !self.actual_disabled(actor, i));
        if !really_available {
            if mega
                || z_powered
                || target.is_some()
                || !matches!(selection, MoveChoice::Auto | MoveChoice::Slot(_))
                    && !matches!(selection, MoveChoice::Name(name) if name == "struggle")
            {
                return Err(BattleError::Choice("only Struggle is available"));
            }
            if let MoveChoice::Slot(n) = selection
                && ((!visibly_available && n.get() != 1)
                    || (visibly_available && n.get() > moves.len()))
            {
                return Err(BattleError::Choice("only Struggle is available"));
            }
            return Ok((
                ActionKind::Move {
                    move_slot: STRUGGLE_SLOT,
                    target: 0,
                    mega: false,
                },
                false,
            ));
        }
        let move_slot = match selection {
            MoveChoice::Slot(n) => Some(n.get() - 1),
            MoveChoice::Name(name) => {
                let id = if name.starts_with("hiddenpower") {
                    "hiddenpower"
                } else {
                    name
                };
                moves.iter().position(|m| m.id == id).or_else(|| {
                    let slot = (!mega)
                        .then(|| self.z_named_base_slot(actor, name))
                        .flatten();
                    z_powered |= slot.is_some();
                    slot
                })
            }
            MoveChoice::Auto => moves
                .iter()
                .enumerate()
                .position(|(i, _)| !self.actual_disabled(actor, i)),
        }
        .ok_or(BattleError::Choice("unknown move slot"))?;
        let info = moves
            .get(move_slot)
            .filter(|m| m.pp > 0)
            .ok_or(BattleError::Choice("invalid move slot or no PP remaining"))?;
        let z_data = if z_powered {
            let data = self
                .z_crystal(actor)
                .and_then(|crystal| self.z_move_data(crystal, info.move_data))
                .ok_or(BattleError::Choice("this move cannot be used as a Z-Move"))?;
            if data.is_z_move && !super::z_move::signature_supported(data) {
                return Err(BattleError::Unsupported(format!("Z-Move {}", data.name)));
            }
            Some(data)
        } else {
            None
        };
        // Signature and damaging Z-Moves replace the base move and its callbacks.
        if z_data.is_none_or(|data| !data.is_z_move) && !Self::move_supported(info) {
            return Err(BattleError::Unsupported(format!("move {}", info.id)));
        }
        let kind = z_data.map_or_else(
            || self.mon(actor).requested_target(info.move_data),
            |data| data.target,
        );
        let target = target.unwrap_or(0);
        if kind.is_choosable() {
            if (target == 0 && self.slots() > 1 && !matches!(selection, MoveChoice::Auto))
                || !kind.valid_location(self.slots() as u8, actor.slot as u8, target)
            {
                return Err(BattleError::Choice("move needs a valid target"));
            }
        } else if target != 0 {
            return Err(BattleError::Choice("cannot specify a target for this move"));
        }
        Ok((
            ActionKind::Move {
                move_slot,
                target,
                mega,
            },
            z_powered,
        ))
    }

    fn move_data(&self, source: Position, slot: usize) -> &'static Move {
        if slot == STRUGGLE_SLOT {
            DEX.moves(self.format.dex_mod, "struggle").unwrap()
        } else if slot == EXTRA_MOVE_SLOT {
            self.mon(source).extra_move.as_ref().unwrap().move_data
        } else {
            self.mon(source).moves[slot].move_data
        }
    }

    fn prioritize(&mut self, action: &mut Action) {
        if let Some(source) = action.actor {
            if let ActionKind::Move {
                move_slot, target, ..
            } = action.kind
            {
                // ModifyPriority receives a resolved target even when no effects
                // listen to it. In doubles this can consume a retarget draw.
                if move_slot == RECHARGE_SLOT {
                    self.random_target(source, TargetKind::Normal);
                    action.priority.priority = action.fractional_priority;
                } else {
                    let data = *action
                        .move_data
                        .get_or_insert_with(|| self.move_data(source, move_slot));
                    self.resolve_target(source, move_base_target(data, source, target), target);
                    let data = if action.z_move {
                        self.z_crystal(source)
                            .and_then(|crystal| self.z_move_data(crystal, data))
                            .unwrap_or(data)
                    } else {
                        data
                    };
                    action.prankster = self.prankster_boosts(source, data);
                    action.priority.priority = f64::from(
                        self.base_move_priority(source, data) + i8::from(action.prankster),
                    ) + action.fractional_priority;
                }
            }
            action.priority.speed = self.field_speed(source);
        }
    }

    fn bind_original_target(&self, action: &mut Action) {
        let ActionKind::Move { target, .. } = action.kind else {
            return;
        };
        if self.slots() != 2 || !(1..=2).contains(&target.unsigned_abs()) {
            return;
        }
        let source = action.actor.unwrap();
        let player = if target > 0 {
            source.player.foe()
        } else {
            source.player
        };
        let position = Position {
            player,
            slot: usize::from(target.unsigned_abs()) - 1,
        };
        action.original_target = Some((player, self.mon(position).roster_index as u8));
    }

    fn assign_fractional_priority(&mut self, action: &mut Action) {
        let actor = action.actor.unwrap();
        if self.event_ability(actor) == "stall" {
            action.fractional_priority = -0.1;
        }
        if self.event_ability(actor) == "quickdraw"
            && let ActionKind::Move { move_slot, .. } = action.kind
            && move_slot != RECHARGE_SLOT
            && self.move_data(actor, move_slot).category != Category::Status
            && self.rng.chance(3, 10)
        {
            action.fractional_priority = 0.1;
            self.add(format!(
                "|-activate|{}|ability: Quick Draw",
                self.ident(actor)
            ));
        }
        if action.fractional_priority <= 0.0
            && self.mon(actor).effective_item() == "quickclaw"
            && self.rng.chance(1, 5)
        {
            action.fractional_priority = 0.1;
            self.add(format!("|-activate|{}|item: Quick Claw", self.ident(actor)));
        }
    }

    fn random_target(&mut self, source: Position, kind: TargetKind) -> Option<Position> {
        match kind {
            TargetKind::User
            | TargetKind::All
            | TargetKind::AllySide
            | TargetKind::AllyTeam
            | TargetKind::AdjacentAllyOrSelf => return Some(source),
            TargetKind::AdjacentAlly => {
                if self.slots() == 1 {
                    return None;
                }
                let mate = Position {
                    slot: 1 - source.slot,
                    ..source
                };
                return (!self.mon(mate).fainted && self.rng.below(1) == 0).then_some(mate);
            }
            _ => {}
        }
        let first = Position {
            player: source.player.foe(),
            slot: 0,
        };
        if self.slots() == 1 {
            return Some(first);
        }
        let second = Position { slot: 1, ..first };
        let living = [first, second].map(|p| (!self.mon(p).fainted).then_some(p));
        match living {
            [Some(a), Some(b)] => Some(if self.rng.below(2) == 0 { a } else { b }),
            [Some(p), None] | [None, Some(p)] => {
                self.rng.below(1);
                Some(p)
            }
            _ => Some(first),
        }
    }

    fn resolve_target(
        &mut self,
        source: Position,
        kind: TargetKind,
        target: i8,
    ) -> Option<Position> {
        if target != 0
            && kind != TargetKind::RandomNormal
            && kind.valid_location(self.slots() as u8, source.slot as u8, target)
        {
            let position = Position {
                slot: usize::from(target.unsigned_abs()) - 1,
                player: if target > 0 {
                    source.player.foe()
                } else {
                    source.player
                },
            };
            if !self.mon(position).fainted || position.player == source.player {
                if kind == TargetKind::AdjacentAllyOrSelf && self.mon(position).fainted {
                    return Some(source);
                }
                return Some(position);
            }
        }
        self.random_target(source, kind)
    }

    fn insert_run_switch(&mut self, incoming: Position, queue: &mut Vec<Action>) {
        self.reset_ability_order(incoming);
        let mut action = Action::new(Some(incoming), ActionKind::RunSwitch);
        self.mon_mut(incoming).speed = self.field_speed(incoming);
        self.prioritize(&mut action);
        self.insert_action(queue, action);
    }

    fn insert_action(&mut self, queue: &mut Vec<Action>, action: Action) {
        let first = queue.partition_point(|q| q.priority.compare(action.priority).is_lt());
        let last =
            first + queue[first..].partition_point(|q| q.priority.compare(action.priority).is_eq());
        let index = if first == last {
            first
        } else {
            first + self.rng.below((last - first + 1) as u32) as usize
        };
        queue.insert(index, action);
    }

    fn queued_move(queue: &[Action], target: Position) -> Option<usize> {
        queue.iter().find_map(|q| {
            if q.actor == Some(target)
                && let ActionKind::Move { move_slot, .. } = q.kind
            {
                Some(move_slot)
            } else {
                None
            }
        })
    }

    fn promote_move(queue: &mut Vec<Action>, index: usize, round_followup: bool) {
        let mut action = queue.remove(index);
        action.priority.order = 3;
        action.round_followup = round_followup;
        queue.insert(0, action);
    }

    fn zoom_lens_active(&self, source: Position, target: Position, queue: &[Action]) -> bool {
        self.mon(source).effective_item() == "zoomlens"
            && self.active_effects(source)
            && Self::queued_move(queue, target).is_none()
    }

    fn replace_queued_move(&mut self, queue: &mut Vec<Action>, actor: Position, move_slot: usize) {
        queue.retain(|q| q.actor != Some(actor) || matches!(q.kind, ActionKind::RunSwitch));
        self.mon_mut(actor).speed = self.field_speed(actor);
        let kind = self
            .mon(actor)
            .requested_target(self.move_data(actor, move_slot));
        let target = self
            .random_target(actor, kind)
            .map_or(0, |pos| pos.loc(actor));
        let mut action = Action::new(
            Some(actor),
            ActionKind::Move {
                move_slot,
                target,
                mega: false,
            },
        );
        self.assign_fractional_priority(&mut action);
        self.bind_original_target(&mut action);
        self.prioritize(&mut action);
        self.insert_action(queue, action);
    }

    fn instruct(&mut self, source: Position, actor: Position, queue: &mut Vec<Action>) -> bool {
        let mon = self.mon(actor);
        let Some((slot, data)) = mon
            .last_move_slot
            .and_then(|slot| mon.moves.get(slot).map(|data| (slot, data)))
        else {
            return false;
        };
        if data.pp == 0
            || ["failinstruct", "charge", "recharge"]
                .iter()
                .any(|&flag| data.move_data.flags.contains_key(flag))
            || ["beakblast", "focuspunch", "shelltrap"]
                .iter()
                .any(|id| mon.volatile_ids().contains(id))
        {
            return false;
        }
        let prior_target = mon.last_move_target;
        let kind = mon.requested_target(data.move_data);
        self.add(format!(
            "|-singleturn|{}|move: Instruct|[of] {}",
            self.ident(actor),
            self.ident(source)
        ));
        let mut action = Action::new(
            Some(actor),
            ActionKind::Move {
                move_slot: slot,
                target: prior_target,
                mega: false,
            },
        );
        self.assign_fractional_priority(&mut action);
        if prior_target == 0 {
            action.kind = ActionKind::Move {
                move_slot: slot,
                target: self.random_target(actor, kind).map_or(0, |p| p.loc(actor)),
                mega: false,
            };
        }
        self.bind_original_target(&mut action);
        self.prioritize(&mut action);
        action.priority.order = 3;
        queue.insert(0, action);
        true
    }

    fn run_switch_batch(&mut self, first: Position, queue: &mut Vec<Action>) -> [u32; 4] {
        // Simultaneous entry effects use a fresh speed order over the field.
        let batch_len = queue
            .iter()
            .take_while(|q| matches!(q.kind, ActionKind::RunSwitch))
            .count();
        let mut switching = [None; 4];
        switching[0] = Some(first);
        for (i, action) in queue.drain(..batch_len).enumerate() {
            switching[i + 1] = action.actor;
        }
        let mut original_hp = [0; 4];
        for pos in switching.iter().flatten() {
            original_hp[pos.index()] = self.mon(*pos).hp;
        }
        let (order, count) = self.ordered_actives(true);
        for (pos, _) in order.into_iter().take(count) {
            if switching.contains(&Some(pos))
                && self.mon(pos).hp > 0
                && self.mon(pos).active_ability() == "unnerve"
            {
                self.start_ability(pos);
            }
        }
        let mut deferred_hospitality = 0;
        for (pos, _) in order.into_iter().take(count) {
            if switching.contains(&Some(pos)) && self.mon(pos).hp != 0 {
                self.apply_replacement_heals(pos);
                self.on_switch_in_hazards(pos);
                if self.check_win() {
                    return original_hp;
                }
                if self.mon(pos).hp == 0 {
                    continue;
                }
                if let Some(label) = self.weather_suppressor(pos) {
                    self.add(format!("|-ability|{}|{label}", self.ident(pos)));
                }
                match self.mon(pos).active_ability() {
                    "hospitality" => deferred_hospitality |= pos.mask(),
                    "imposter" => self.imposter_on_entry(pos),
                    "unnerve" | "mimicry" => {}
                    _ => self.start_ability(pos),
                }
                self.on_switch_in_item(pos);
            }
        }
        for (pos, _) in order.into_iter().take(count) {
            if switching.contains(&Some(pos)) && self.mon(pos).hp != 0 {
                self.primal_reversion(pos);
                if self.mon(pos).active_ability() == "mimicry" {
                    self.apply_mimicry(pos);
                }
                self.use_matching_terrain_seed(pos);
            }
        }
        for (pos, _) in order.into_iter().take(count) {
            if deferred_hospitality & pos.mask() != 0 {
                self.hospitality(pos);
            }
        }
        if self.possible_opportunist {
            for (pos, _) in order.into_iter().take(count) {
                self.apply_pending_boost_copy(pos);
            }
        }
        for (pos, _) in order.into_iter().take(count) {
            self.white_herb(pos);
        }
        original_hp
    }

    fn switch_party_member(
        &mut self,
        outgoing: Position,
        bench_slot: usize,
        pivot: Option<Pivot>,
        drag: bool,
    ) {
        let returning = outgoing.slot == bench_slot;
        if !returning && !self.mon(outgoing).fainted && pivot.is_none() && !drag {
            self.each_event();
        }
        if !returning && !self.mon(outgoing).fainted {
            self.switch_out_ability(outgoing);
            self.end_supreme_overlord(outgoing, true);
            self.end_ability(outgoing);
        }
        let rules = self.stat_rules;
        self.clear_linked_traps(outgoing);
        let party = &mut self.sides[outgoing.player.index()].as_mut().unwrap().party;
        if let Some(Pivot::Move(pass @ ("Baton Pass" | "Shed Tail"))) = pivot {
            let (active, incoming) = party.split_at_mut(bench_slot);
            incoming[0].receive_pass(&active[outgoing.slot], pass == "Shed Tail");
        }
        party[outgoing.slot].reset_forme(rules);
        party[outgoing.slot].protection = None;
        party[outgoing.slot].stall = None;
        party[outgoing.slot].ally_switch_stall = None;
        party[outgoing.slot].boosts = Boosts::default();
        party[outgoing.slot].stats_raised = false;
        party[outgoing.slot].flinched = false;
        party[outgoing.slot].lost_focus = false;
        party[outgoing.slot].substitute_hp = 0;
        party[outgoing.slot].recharge = 0;
        party[outgoing.slot].charge = None;
        party[outgoing.slot].confusion = 0;
        party[outgoing.slot].rampage = None;
        party[outgoing.slot].uproar = None;
        party[outgoing.slot].attention = None;
        party[outgoing.slot].glaive_drawback = false;
        party[outgoing.slot].destiny_bond = false;
        party[outgoing.slot].helping_hand = 0;
        party[outgoing.slot].volatile_order.clear();
        party[outgoing.slot].imprisoned = false;
        party[outgoing.slot].choice_lock = None;
        party[outgoing.slot].metronome = None;
        party[outgoing.slot].active_turns = 0;
        party[outgoing.slot].active_move_actions = 0;
        party[outgoing.slot].hurt_this_turn = false;
        party[outgoing.slot].roosted = false;
        party[outgoing.slot].set_temporary_type(None);
        party[outgoing.slot].yawn = None;
        party[outgoing.slot].seeded_from = None;
        party[outgoing.slot].partial_trap = None;
        party[outgoing.slot].perish_count = 0;
        party[outgoing.slot].throat_chop = 0;
        party[outgoing.slot].taunt = 0;
        party[outgoing.slot].encore = None;
        party[outgoing.slot].attract = None;
        party[outgoing.slot].disabled_slot = None;
        party[outgoing.slot].last_move_slot = None;
        party[outgoing.slot].extra_move = None;
        party[outgoing.slot].move_failed_this_turn = false;
        party[outgoing.slot].move_failed_last_turn = false;
        party[outgoing.slot].last_foe_hit = None;
        party[outgoing.slot].attackers_this_turn = [0; 2];
        party[outgoing.slot].heal_block = 0;
        party[outgoing.slot].legacy_conditions = Default::default();
        party[outgoing.slot].octolock = None;
        party[outgoing.slot].fling_pending = false;
        party[outgoing.slot].stockpile = Default::default();
        party[outgoing.slot].magnet_rise = 0;
        party[outgoing.slot].dragon_cheer = 0;
        party[outgoing.slot].laser_focus = 0;
        party[outgoing.slot].fury_cutter = None;
        party[outgoing.slot].rolling_attack = None;
        party[outgoing.slot].gem_boost = false;
        party[outgoing.slot].electrified = false;
        party[outgoing.slot].gastro_acid = false;
        party[outgoing.slot].lock_on = None;
        party[outgoing.slot].syrup_bomb = None;
        party[outgoing.slot].counter = None;
        party[outgoing.slot].acted = false;
        if !party[outgoing.slot].fainted {
            party[outgoing.slot].times_attacked = 0;
        }
        party[outgoing.slot].needs_replacement = false;
        party[outgoing.slot].reviving = false;
        party[outgoing.slot].speed = party[outgoing.slot].stats.spe as i32;
        party[outgoing.slot].pivot_move = None;
        party[outgoing.slot].illusion = None;
        party[outgoing.slot].reset_ability();
        if party[outgoing.slot].fainted {
            party[outgoing.slot].status = Status::Healthy;
        }
        party.swap(outgoing.slot, bench_slot);
        party[outgoing.slot].used_moves = 0;
        if matches!(party[outgoing.slot].status, Status::Toxic(_)) {
            party[outgoing.slot].status = Status::Toxic(0);
        }
        self.prepare_illusion(outgoing);
        if drag {
            self.add_health_line("drag", outgoing, true);
        } else if let Some(Pivot::Move(move_name)) = pivot {
            self.add_health_line_with("switch", outgoing, true, &format!("|[from] {move_name}"));
        } else {
            self.add_health_line("switch", outgoing, true);
        }
    }

    fn run_queue(&mut self, mut queue: Vec<Action>) -> Result<(), BattleError> {
        while !queue.is_empty() {
            let action = queue.remove(0);
            self.turn_queue_empty = queue.is_empty();
            let mut original_hp = [0; 4];
            match action.kind {
                ActionKind::MegaEvolve => {
                    let pos = action.actor.unwrap();
                    if !self.mon(pos).fainted {
                        self.mega_evolve(pos);
                        self.apply_pending_boost_copies();
                        self.white_herbs_after_action();
                    }
                }
                ActionKind::BeforeTurn => {
                    self.last_fusion = None;
                    self.ordered_actives(false);
                }
                ActionKind::PrepareCounter { move_kind, target } => {
                    let pos = action.actor.unwrap();
                    let id = move_kind.id();
                    if !self.mon(pos).fainted {
                        if self.format.report_exact_hp {
                            self.add(format!("|debug|before turn callback: {id}"));
                        }
                        if self
                            .resolve_target(pos, TargetKind::Scripted, target)
                            .is_some()
                        {
                            let mon = self.mon_mut(pos);
                            mon.counter = Some(PreparedCounter {
                                move_kind,
                                hit: None,
                            });
                            mon.track_volatile(id);
                        }
                    }
                }
                ActionKind::PriorityCharge(id) => {
                    let pos = action.actor.unwrap();
                    if self.mon(pos).hp > 0 {
                        if self.format.report_exact_hp {
                            self.add(format!("|debug|priority charge callback: {id}"));
                        }
                        self.mon_mut(pos).track_volatile(id);
                        if id == "focuspunch" {
                            self.mon_mut(pos).lost_focus = false;
                            self.add(format!(
                                "|-singleturn|{}|move: Focus Punch",
                                self.ident(pos)
                            ));
                        } else if id == "beakblast" {
                            self.add(format!("|-singleturn|{}|move: Beak Blast", self.ident(pos)));
                        }
                    }
                }
                ActionKind::RunSwitch => {
                    original_hp = self.run_switch_batch(action.actor.unwrap(), &mut queue);
                }
                ActionKind::Switch {
                    bench_slot, pivot, ..
                } => {
                    let outgoing = action.actor.unwrap();
                    if pivot.is_some() || outgoing.slot == bench_slot {
                        queue.retain(|q| {
                            q.actor != Some(outgoing) || matches!(q.kind, ActionKind::RunSwitch)
                        });
                    }
                    self.switch_party_member(outgoing, bench_slot, pivot, false);
                    self.insert_run_switch(outgoing, &mut queue);
                }
                ActionKind::Revive { party_slot } => {
                    let target = Position {
                        slot: party_slot,
                        player: action.actor.unwrap().player,
                    };
                    let returns_to_field = party_slot < self.slots();
                    if returns_to_field {
                        let mut reentry = Action::new(
                            Some(target),
                            ActionKind::Switch {
                                bench_slot: party_slot,
                                immediate: true,
                                pivot: None,
                            },
                        );
                        self.prioritize(&mut reentry);
                        queue.push(reentry);
                    }
                    let mon = self.mon_mut(target);
                    mon.fainted = false;
                    mon.needs_replacement = false;
                    mon.slept_by_foe = false;
                    mon.status = Status::Healthy;
                    mon.hp = (mon.max_hp.get() / 2).max(1);
                    let ident = format!("{}: {}", target.player.id(), mon.name);
                    self.add_health_with_ident(
                        "-heal",
                        target,
                        &ident,
                        false,
                        "|[from] move: Revival Blessing",
                    );
                }
                ActionKind::Move {
                    move_slot, target, ..
                } => {
                    let source = action.actor.unwrap();
                    if self.mon(source).fainted {
                        continue;
                    }
                    self.mon_mut(source).acted = true;
                    self.mon_mut(source).active_move_actions += 1;
                    let recharging = move_slot == RECHARGE_SLOT || self.mon(source).recharge > 0;
                    let (attempted, source) = self.run_move(
                        source,
                        MoveInvocation::Choice {
                            slot: move_slot,
                            loc: target,
                            original_target: action.original_target,
                            prankster: action.prankster,
                            round_followup: action.round_followup,
                            z_priority: action.z_move.then(|| {
                                (action.priority.priority - action.fractional_priority).round()
                                    as i8
                            }),
                        },
                        &mut queue,
                    );
                    if attempted {
                        if self.mon(source).volatile_ids().contains(&"beakblast")
                            && self.move_data(source, move_slot).name == "Beak Blast"
                        {
                            self.mon_mut(source).untrack_volatile("beakblast");
                        }
                        self.end_rampage_attempt(source);
                        if !recharging {
                            self.white_herbs_after_action();
                        }
                    }
                }
                ActionKind::Residual => {
                    for pos in self.positions() {
                        original_hp[pos.index()] = self.mon(pos).hp;
                    }
                    self.residual();
                }
            }
            if let Some((caller, name)) = self.unsupported_called_move.take() {
                return Err(BattleError::Unsupported(format!("{caller}: {name}")));
            }
            let drags = if self.drag_flags != 0 {
                self.apply_drags(&mut queue)
            } else {
                Ok(())
            };
            self.ability_breaker = None;
            drags?;
            if self.phase == Phase::Ended || self.check_win() {
                return Ok(());
            }
            if queue.is_empty() {
                for player in Player::ALL {
                    if self.first_bench(player, 0).is_none() {
                        continue;
                    }
                    for slot in 0..self.slots() {
                        let mon = self.mon_mut(Position { player, slot });
                        if mon.fainted {
                            mon.status = Status::Fainted;
                            mon.needs_replacement = true;
                        }
                    }
                }
            }
            let replacements_pending = queue.first().is_some_and(|q| {
                matches!(
                    q.kind,
                    ActionKind::Switch {
                        immediate: true,
                        ..
                    }
                )
            });
            if !replacements_pending {
                self.turn_queue_empty = queue.is_empty();
                self.each_event();
            }
            if matches!(action.kind, ActionKind::Residual | ActionKind::RunSwitch) {
                for pos in self.positions() {
                    self.emergency_exit(pos, original_hp[pos.index()]);
                }
            }
            let mut waiting = false;
            for player in Player::ALL.into_iter().filter(|_| !replacements_pending) {
                for (slot, required) in self.force_switch(player).into_iter().enumerate() {
                    if !required {
                        continue;
                    }
                    waiting = true;
                    if self
                        .mon(Position { player, slot })
                        .pivot_move
                        .is_some_and(|pivot| {
                            !matches!(pivot, Pivot::Move("Shed Tail" | "Baton Pass"))
                        })
                    {
                        // BeforeSwitchOut marks this move-driven switch as having
                        // already run its update. Volatile-passing moves skip it.
                        self.each_event();
                    }
                }
            }
            if waiting {
                self.suspended = queue;
                self.phase = Phase::Replace;
                return Ok(());
            }
            if queue
                .first()
                .is_some_and(|q| matches!(q.kind, ActionKind::Move { .. }))
            {
                self.update_speed();
                for candidate in &mut queue {
                    self.prioritize(candidate);
                }
                speed_sort_by(&mut queue, &mut self.rng, |a, b| {
                    a.priority.compare(b.priority)
                });
            }
        }
        self.turn += 1;
        for side in self.sides.iter_mut().flatten() {
            side.fainted_last_turn = std::mem::take(&mut side.fainted_this_turn);
        }
        for pos in self.positions() {
            self.announce_reflected_type(pos);
            let mon = self.mon_mut(pos);
            mon.revealed_trap = false;
            mon.revealed_disables = 0;
            mon.revealed_all_disables = false;
            mon.acted = false;
            mon.stats_raised = false;
            mon.stats_lowered = false;
            if mon.attackers_this_turn != [0; 2] {
                mon.unknown_type = false;
            }
            mon.attackers_this_turn = [0; 2];
            mon.hurt_this_turn = false;
            mon.move_failed_last_turn = std::mem::take(&mut mon.move_failed_this_turn);
            if let Some(metronome) = mon.metronome.as_mut() {
                metronome.last_turn_succeeded = std::mem::take(&mut metronome.this_turn_succeeded);
            }
            mon.used_item_this_turn = false;
            if !mon.fainted {
                mon.active_turns += 1;
            }
            self.refresh_observed_disables(pos);
        }
        self.add(format!("|turn|{}", self.turn));
        self.phase = Phase::Move;
        Ok(())
    }

    fn apply_drags(&mut self, queue: &mut Vec<Action>) -> Result<(), BattleError> {
        let flags = std::mem::take(&mut self.drag_flags);
        for target in self.positions() {
            if flags & target.mask() == 0 || self.mon(target).hp == 0 {
                continue;
            }
            let slots = self.slots();
            let candidates: Vec<_> = self
                .party(target.player)
                .unwrap()
                .iter()
                .enumerate()
                .skip(slots)
                .filter(|&(_, mon)| !mon.fainted)
                .map(|(slot, _)| slot)
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let bench_slot = candidates[self.rng.below(candidates.len() as u32) as usize];
            if let Some(error) = self.party(target.player).unwrap()[bench_slot].entry_error() {
                return Err(error);
            }
            queue.retain(|q| q.actor != Some(target) || matches!(q.kind, ActionKind::RunSwitch));
            self.switch_party_member(target, bench_slot, None, true);
            self.reset_ability_order(target);
            self.run_switch_batch(target, queue);
        }
        Ok(())
    }

    pub(super) fn schedule_drag(&mut self, source: Position, target: Position) -> bool {
        if self.mon(target).hp == 0
            || self.mon(source).hp == 0
            || self.first_bench(target.player, 0).is_none()
        {
            return false;
        }
        if let Some(label) = match self.event_ability(target) {
            "suctioncups" => Some("Suction Cups"),
            "guarddog" => Some("Guard Dog"),
            _ => None,
        } {
            self.add(format!(
                "|-activate|{}|ability: {label}",
                self.ident(target)
            ));
        } else if self.mon(target).volatile_ids().contains(&"ingrain") {
            self.add(format!("|-activate|{}|move: Ingrain", self.ident(target)));
        } else {
            self.drag_flags |= target.mask();
        }
        true
    }

    pub(super) fn start_ability(&mut self, pos: Position) {
        if self.mon(pos).gastro_acid && self.mon(pos).active_ability().is_empty() {
            if self.format.report_exact_hp
                && DEX
                    .ability(&self.mon(pos).ability)
                    .is_some_and(|a| a.handlers.iter().any(|h| h == "onStart"))
            {
                self.add("|debug|Start handler suppressed by Gastro Acid or Neutralizing Gas");
            }
            return;
        }
        self.possible_opportunist |= self.mon(pos).ability == "opportunist";
        if matches!(self.mon(pos).ability.as_str(), "berserk" | "angershell") {
            self.mon_mut(pos).half_hp_berry_deferred = false;
        }
        if self.mon(pos).ability == "zerotohero" {
            self.announce_hero(pos);
        }
        if self.mon(pos).ability == "ripen" {
            self.mon_mut(pos).ripen_pending = false;
        }
        if let Some(label) = match self.mon(pos).ability.as_str() {
            "pressure" => Some("Pressure"),
            "moldbreaker" => Some("Mold Breaker"),
            "teravolt" => Some("Teravolt"),
            "turboblaze" => Some("Turboblaze"),
            "fairyaura" => Some("Fairy Aura"),
            "darkaura" => Some("Dark Aura"),
            "aurabreak" => Some("Aura Break"),
            "unnerve" => Some("Unnerve"),
            _ => None,
        } {
            self.add(format!("|-ability|{}|{label}", self.ident(pos)));
        }
        self.switch_in_weather(pos);
        self.weather_ability_changed(pos, false);
        if self.mon(pos).ability == "forecast" {
            self.forecast(pos, self.effective_weather());
        }
        self.switch_in_terrain(pos);
        if self.mon(pos).ability == "intimidate" {
            self.intimidate(pos);
        }
        if self.event_ability(pos) == "supersweetsyrup" {
            self.supersweet_syrup(pos);
        }
        if self.mon(pos).ability == "frisk" {
            self.frisk(pos);
        }
        match self.event_ability(pos) {
            "download" => self.download_boost(pos),
            "anticipation" => self.anticipation(pos),
            "forewarn" => self.forewarn(pos),
            _ => {}
        }
        if self.mon(pos).ability == "screencleaner" {
            self.screen_cleaner(pos);
        }
        if self.mon(pos).active_ability() == "mimicry" {
            self.apply_mimicry(pos);
        }
        if self.mon(pos).ability == "hospitality" {
            self.hospitality(pos);
        }
        if self.mon(pos).ability == "supremeoverlord" {
            self.start_supreme_overlord(pos);
        }
        if self.mon(pos).ability == "curiousmedicine" {
            self.curious_medicine(pos);
        }
        self.start_trace(pos);
    }

    pub(super) fn positions(&self) -> impl Iterator<Item = Position> + use<> {
        let slots = self.slots();
        Player::ALL
            .into_iter()
            .flat_map(move |player| (0..slots).map(move |slot| Position { player, slot }))
    }

    fn position_after_move(&self, source: Position, roster_index: usize) -> Position {
        if self.mon(source).roster_index == roster_index {
            source
        } else {
            source.other_slot()
        }
    }

    fn ally_switch(&mut self, source: Position, queue: &mut [Action]) {
        let partner = source.other_slot();
        self.add(format!(
            "|swap|{}|{}|[from] move: Ally Switch",
            self.ident(source),
            partner.slot
        ));
        self.sides[source.player.index()]
            .as_mut()
            .unwrap()
            .party
            .swap(source.slot, partner.slot);
        if let Some(holder) = &mut self.ability_breaker
            && holder.player == source.player
        {
            *holder = holder.other_slot();
        }
        // Actions belong to Pokémon, but their explicit target locations and
        // slot conditions do not (including Leech Seed and delayed attacks).
        for action in queue {
            if let Some(actor) = &mut action.actor
                && actor.player == source.player
            {
                *actor = actor.other_slot();
                if let ActionKind::Revive { party_slot } = &mut action.kind
                    && *party_slot < 2
                {
                    *party_slot = 1 - *party_slot;
                }
            }
        }
        self.apply_ally_switch_heals(source);
        self.apply_ally_switch_heals(partner);
    }

    pub(super) fn ordered_actives(
        &mut self,
        include_fainted: bool,
    ) -> ([(Position, i32); 4], usize) {
        // Even a field-wide event with no matching handler performs this
        // selection and consumes every speed-tie draw.
        let mut actives = [(
            Position {
                player: Player::P1,
                slot: 0,
            },
            0,
        ); 4];
        let mut count = 0;
        for pos in self.positions() {
            let pokemon = self.mon(pos);
            if include_fainted || !pokemon.fainted {
                actives[count] = (pos, pokemon.speed);
                count += 1;
            }
        }
        speed_sort_by(&mut actives[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        (actives, count)
    }

    pub(super) fn update_speed(&mut self) {
        for pos in self.positions() {
            if !self.mon(pos).fainted {
                self.mon_mut(pos).speed = self.field_speed(pos);
            }
        }
    }

    pub(super) fn field_speed(&mut self, pos: Position) -> i32 {
        let stat = self.speed_stat(pos);
        self.stat_rules
            .truncation
            .action_speed(stat, (self.trick_room > 0) ^ self.twisted_dimension)
    }

    pub(super) fn speed_stat(&mut self, pos: Position) -> u32 {
        self.sort_unaware_handlers();
        let stat = self.mon(pos).current_speed(
            self.effective_weather(),
            self.terrain(),
            self.sides[pos.player.index()].as_ref().unwrap().tailwind != 0,
        );
        match self.stat_rules.truncation {
            crate::mechanics::Truncation::Cartridge => stat.min(10_000),
            crate::mechanics::Truncation::Unbounded => stat,
        }
    }

    pub(super) fn move_priority(&self, pos: Position, data: &Move) -> i8 {
        self.base_move_priority(pos, data) + i8::from(self.prankster_boosts(pos, data))
    }

    fn prankster_boosts(&self, pos: Position, data: &Move) -> bool {
        data.category == Category::Status && self.event_ability(pos) == "prankster"
    }

    fn base_move_priority(&self, pos: Position, data: &Move) -> i8 {
        data.priority
            + i8::from(
                data.name == "Grassy Glide"
                    && self.terrain() == Some(Terrain::Grassy)
                    && self.grounded(pos),
            )
            + i8::from(
                data.move_type == "Flying"
                    && self.mon(pos).hp == self.mon(pos).max_hp.get()
                    && self.event_ability(pos) == "galewings",
            )
    }

    pub(super) fn each_event(&mut self) {
        let (order, count) = self.ordered_actives(false);
        for (pos, _) in order.into_iter().take(count) {
            self.debug_suppressed_hook(pos, "onUpdate");
            self.attract_update(pos);
            self.update_syrup_bomb(pos);
            if self.mon(pos).fling_pending {
                self.finish_fling(pos);
            }
            self.try_trace(pos);
            self.disguise_update(pos);
            if self.event_ability(pos) == "owntempo" && self.mon(pos).confusion > 0 {
                self.add(format!("|-activate|{}|ability: Own Tempo", self.ident(pos)));
                self.cure_confusion(pos);
            }
            if self.event_ability(pos) == "limber" && self.mon(pos).status == Status::Paralysis {
                self.add(format!("|-activate|{}|ability: Limber", self.ident(pos)));
                self.cure_status(pos, "[msg]");
            }
            if self.event_ability(pos) == "waterbubble" && self.mon(pos).status == Status::Burn {
                self.add(format!(
                    "|-activate|{}|ability: Water Bubble",
                    self.ident(pos)
                ));
                self.cure_status(pos, "[msg]");
            }
            if matches!(self.mon(pos).status, Status::Freeze(_))
                && self.event_ability(pos) == "magmaarmor"
            {
                self.add(format!(
                    "|-activate|{}|ability: Magma Armor",
                    self.ident(pos)
                ));
                self.cure_status(pos, "[msg]");
            }
            if let Status::Sleep(_) = self.mon(pos).status {
                let label = match self.event_ability(pos) {
                    "insomnia" => Some("Insomnia"),
                    "vitalspirit" => Some("Vital Spirit"),
                    _ => None,
                };
                if let Some(label) = label {
                    self.add(format!("|-activate|{}|ability: {label}", self.ident(pos)));
                    self.cure_status(pos, "[msg]");
                }
            }
            self.on_update_item(pos);
        }
    }

    fn residual(&mut self) {
        #[derive(Copy, Clone)]
        enum Effect {
            Status,
            Protect,
            Endure,
            Stall,
            AllySwitchStall,
            Electrify,
            Flinch,
            FocusPunch,
            ChillyReception,
            BeakBlast,
            HelpingHand,
            Counter,
            Recharge,
            Charge,
            SemiInvulnerableTimer,
            Rampage,
            Uproar,
            Attention,
            Leftovers,
            BlackSludge,
            StatusOrb,
            WhiteHerb,
            ShedSkin,
            Hydration,
            HungerSwitch,
            Healer,
            Weather,
            SpeedBoost,
            Harvest,
            CudChew,
            Pickup,
            Roost,
            Yawn,
            Wish,
            FutureMove,
            LeechSeed,
            Curse,
            SaltCure,
            PartialTrap,
            PerishSong,
            ThroatChop,
            HealBlock,
            LaserFocus,
            FuryCutter,
            RollingAttack,
            EchoedVoice,
            LegacyField(LegacyField),
            LegacyCondition(LegacyCondition),
            Mist,
            Taunt,
            Encore,
            Disable,
            Screen(Screen),
            SideGuard(SideGuard),
            Safeguard,
            LuckyChant,
            Tailwind,
            TrickRoom,
            Gravity,
            WonderRoom,
            MagicRoom,
            Terrain,
            GrassyHeal,
            AquaRing,
            Ingrain,
            Octolock,
            Opportunist,
            MagnetRise,
            Moody,
            Gem,
            FairyLock,
            LockOn,
            SyrupBomb,
        }
        self.add("|");
        self.update_speed();
        let mut timers = Vec::new();
        for effect in LegacyField::ALL {
            if self.legacy_field_active(effect) {
                let (order, sub_order) = effect.residual_priority();
                timers.push((
                    Position {
                        player: Player::P1,
                        slot: 0,
                    },
                    Effect::LegacyField(effect),
                    Priority {
                        order,
                        sub_order,
                        ..Priority::default()
                    },
                ));
            }
        }
        if self.echoed_voice.is_some() {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::EchoedVoice,
                Priority {
                    sub_order: 5,
                    ..Priority::default()
                },
            ));
        }
        if self.weather().is_some() {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::Weather,
                Priority {
                    order: 1,
                    sub_order: 5,
                    ..Priority::default()
                },
            ));
        }
        if self.trick_room > 0 {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::TrickRoom,
                Priority {
                    order: 27,
                    sub_order: 1,
                    ..Priority::default()
                },
            ));
        }
        if self.gravity > 0 {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::Gravity,
                Priority {
                    order: 27,
                    sub_order: 2,
                    ..Priority::default()
                },
            ));
        }
        if self.wonder_room > 0 {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::WonderRoom,
                Priority {
                    order: 27,
                    sub_order: 5,
                    ..Priority::default()
                },
            ));
        }
        if self.magic_room.is_some() {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::MagicRoom,
                Priority {
                    order: 27,
                    sub_order: 6,
                    ..Priority::default()
                },
            ));
        }
        if self.terrain().is_some() {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::Terrain,
                Priority {
                    order: 27,
                    sub_order: 7,
                    ..Priority::default()
                },
            ));
        }
        if self.fairy_lock != 0 {
            timers.push((
                Position {
                    player: Player::P1,
                    slot: 0,
                },
                Effect::FairyLock,
                Priority {
                    sub_order: 5,
                    ..Priority::default()
                },
            ));
        }
        for player in Player::ALL {
            if self.mist_active(player) {
                timers.push((
                    Position { player, slot: 0 },
                    Effect::Mist,
                    Priority {
                        order: 26,
                        sub_order: 4,
                        ..Priority::default()
                    },
                ));
            }
            if self.lucky_chant_active(player) {
                timers.push((
                    Position { player, slot: 0 },
                    Effect::LuckyChant,
                    Priority {
                        order: 26,
                        sub_order: 6,
                        ..Priority::default()
                    },
                ));
            }
            if self.sides[player.index()].as_ref().unwrap().safeguard > 0 {
                timers.push((
                    Position { player, slot: 0 },
                    Effect::Safeguard,
                    Priority {
                        order: 26,
                        sub_order: 3,
                        ..Priority::default()
                    },
                ));
            }
            for &guard in self.sides[player.index()]
                .as_ref()
                .unwrap()
                .guards
                .iter()
                .flatten()
            {
                timers.push((
                    Position { player, slot: 0 },
                    Effect::SideGuard(guard),
                    Priority {
                        sub_order: 4,
                        ..Priority::default()
                    },
                ));
            }
            if self.sides[player.index()].as_ref().unwrap().tailwind > 0 {
                timers.push((
                    Position { player, slot: 0 },
                    Effect::Tailwind,
                    Priority {
                        order: 26,
                        sub_order: 5,
                        ..Priority::default()
                    },
                ));
            }
            for screen in Screen::ALL {
                if self.has_screen(player, screen) {
                    timers.push((
                        Position { player, slot: 0 },
                        Effect::Screen(screen),
                        Priority {
                            order: 26,
                            sub_order: screen.sub_order(),
                            ..Priority::default()
                        },
                    ));
                }
            }
        }
        for pos in self.positions() {
            let mon = self.mon(pos);
            let priority = |order, sub_order| Priority {
                order,
                sub_order,
                speed: mon.speed,
                ..Priority::default()
            };
            if let Some((order, _, _)) = mon.status.residual() {
                timers.push((pos, Effect::Status, priority(order, 0)));
            }
            if mon.volatile_ids().contains(&"aquaring") {
                timers.push((pos, Effect::AquaRing, priority(6, 2)));
            }
            if mon.volatile_ids().contains(&"ingrain") {
                timers.push((pos, Effect::Ingrain, priority(7, 2)));
            }
            for id in mon.volatile_ids() {
                let effect = match *id {
                    "octolock" => Effect::Octolock,
                    "syrupbomb" => Effect::SyrupBomb,
                    _ => continue,
                };
                timers.push((pos, effect, priority(14, 2)));
            }
            if mon.ability == "speedboost" {
                timers.push((pos, Effect::SpeedBoost, priority(28, 2)));
            }
            if !mon.fainted {
                match mon.ability.as_str() {
                    "harvest" => timers.push((pos, Effect::Harvest, priority(28, 2))),
                    "cudchew" => timers.push((pos, Effect::CudChew, priority(28, 2))),
                    "pickup" => timers.push((pos, Effect::Pickup, priority(28, 2))),
                    "opportunist" => timers.push((pos, Effect::Opportunist, priority(29, 7))),
                    "moody" => timers.push((pos, Effect::Moody, priority(28, 2))),
                    _ => {}
                }
            }
            if mon.disabled_slot.is_some() {
                timers.push((pos, Effect::Disable, priority(17, 2)));
            }
            if mon.taunt > 0 {
                timers.push((pos, Effect::Taunt, priority(15, 2)));
            }
            if mon.perish_count != 0 {
                timers.push((pos, Effect::PerishSong, priority(24, 2)));
            }
            if mon.throat_chop != 0 {
                timers.push((pos, Effect::ThroatChop, priority(22, 2)));
            }
            if mon.heal_block > 0 {
                timers.push((pos, Effect::HealBlock, priority(20, 2)));
            }
            for condition in LegacyCondition::ALL {
                if mon.under(condition) {
                    timers.push((
                        pos,
                        Effect::LegacyCondition(condition),
                        priority(condition.residual_order(), 2),
                    ));
                }
            }
            if mon.encore.is_some() {
                timers.push((pos, Effect::Encore, priority(16, 2)));
            }
            if mon.magnet_rise != 0 {
                timers.push((pos, Effect::MagnetRise, priority(18, 2)));
            }
            if mon.yawn.is_some() {
                timers.push((pos, Effect::Yawn, priority(23, 2)));
            }
            if self.sides[pos.player.index()].as_ref().unwrap().wishes[pos.slot].is_some() {
                timers.push((pos, Effect::Wish, priority(4, 3)));
            }
            if self.sides[pos.player.index()]
                .as_ref()
                .unwrap()
                .future_moves[pos.slot]
                .is_some()
            {
                timers.push((pos, Effect::FutureMove, priority(3, 3)));
            }
            if mon.seeded_from.is_some() {
                timers.push((pos, Effect::LeechSeed, priority(8, 2)));
            }
            if mon.volatile_ids().contains(&"curse") {
                timers.push((pos, Effect::Curse, priority(12, 2)));
            }
            for &id in mon.volatile_ids() {
                let effect = match id {
                    "saltcure" => Effect::SaltCure,
                    "partiallytrapped" => Effect::PartialTrap,
                    _ => continue,
                };
                timers.push((pos, effect, priority(13, 2)));
            }
            if mon.volatile_ids().contains(&"focuspunch") {
                timers.push((pos, Effect::FocusPunch, priority(0, 2)));
            }
            if mon.volatile_ids().contains(&"chillyreception") {
                timers.push((pos, Effect::ChillyReception, priority(0, 2)));
            }
            if mon.volatile_ids().contains(&"beakblast") {
                timers.push((pos, Effect::BeakBlast, priority(0, 2)));
            }
            if mon.helping_hand != 0 {
                timers.push((pos, Effect::HelpingHand, priority(0, 2)));
            }
            if mon.counter.is_some() {
                timers.push((pos, Effect::Counter, priority(0, 2)));
            }
            if mon.roosted {
                timers.push((pos, Effect::Roost, priority(25, 2)));
            }
            if mon.item == "leftovers" {
                timers.push((pos, Effect::Leftovers, priority(5, 4)));
            }
            if mon.item == "blacksludge" {
                timers.push((pos, Effect::BlackSludge, priority(5, 4)));
            }
            if matches!(mon.item.as_str(), "toxicorb" | "flameorb") {
                timers.push((pos, Effect::StatusOrb, priority(28, 3)));
            }
            if mon.item == "whiteherb" {
                timers.push((pos, Effect::WhiteHerb, priority(29, 8)));
            }
            if mon.ability == "shedskin" {
                timers.push((pos, Effect::ShedSkin, priority(5, 3)));
            }
            if mon.ability == "hydration" {
                timers.push((pos, Effect::Hydration, priority(5, 3)));
            }
            if mon.ability == "hungerswitch" {
                timers.push((pos, Effect::HungerSwitch, priority(29, 7)));
            }
            if mon.ability == "healer" {
                timers.push((pos, Effect::Healer, priority(5, 3)));
            }
            if mon.protection.is_some() {
                timers.push((pos, Effect::Protect, priority(0, 2)));
            }
            if mon.volatile_ids().contains(&"endure") {
                timers.push((pos, Effect::Endure, priority(0, 2)));
            }
            if mon.stall.is_some() {
                timers.push((pos, Effect::Stall, priority(0, 2)));
            }
            if mon.ally_switch_stall.is_some() {
                timers.push((pos, Effect::AllySwitchStall, priority(0, 2)));
            }
            if mon.electrified {
                timers.push((pos, Effect::Electrify, priority(0, 2)));
            }
            if mon.flinched {
                timers.push((pos, Effect::Flinch, priority(0, 2)));
            }
            if mon.gem_boost {
                timers.push((pos, Effect::Gem, priority(0, 2)));
            }
            if mon.lock_on.is_some() {
                timers.push((pos, Effect::LockOn, priority(0, 2)));
            }
            if mon.laser_focus != 0 {
                timers.push((pos, Effect::LaserFocus, priority(0, 2)));
            }
            if mon.fury_cutter.is_some() {
                timers.push((pos, Effect::FuryCutter, priority(0, 2)));
            }
            if mon.rolling_attack.is_some() {
                timers.push((pos, Effect::RollingAttack, priority(0, 2)));
            }
            if mon.recharge != 0 {
                timers.push((pos, Effect::Recharge, priority(0, 2)));
            }
            if mon.charge.is_some() {
                timers.push((pos, Effect::Charge, priority(0, 2)));
            }
            if mon.semi_invulnerable() {
                timers.push((pos, Effect::SemiInvulnerableTimer, priority(0, 2)));
            }
            if mon.rampage.is_some() {
                timers.push((pos, Effect::Rampage, priority(0, 2)));
            }
            if mon.uproar.is_some() {
                timers.push((pos, Effect::Uproar, priority(28, 1)));
            }
            if mon.attention.is_some() {
                timers.push((pos, Effect::Attention, priority(0, 2)));
            }
            // Field handlers are discovered after this slot's own effects.
            // Selection-sort swaps use that order when residual speeds tie.
            if self.terrain() == Some(Terrain::Grassy) {
                timers.push((pos, Effect::GrassyHeal, priority(5, 2)));
            }
        }
        speed_sort_by(&mut timers, &mut self.rng, |a, b| a.2.compare(b.2));
        let mut perish_faints = Vec::new();
        for (pos, effect, _) in timers {
            if let Effect::LegacyField(effect) = effect {
                self.tick_legacy_field(effect);
                continue;
            }
            if matches!(effect, Effect::Mist) {
                self.tick_mist(pos.player);
                continue;
            }
            if matches!(effect, Effect::EchoedVoice) {
                super::streaks::Streak::tick(&mut self.echoed_voice);
                continue;
            }
            if let Effect::SideGuard(guard) = effect {
                self.clear_side_guard(pos.player, guard);
                continue;
            }
            if matches!(effect, Effect::Safeguard) {
                self.tick_safeguard(pos.player);
                continue;
            }
            if matches!(effect, Effect::LuckyChant) {
                self.tick_lucky_chant(pos.player);
                continue;
            }
            if matches!(effect, Effect::Weather) {
                self.weather_residual();
                if self.check_win() {
                    return;
                }
                continue;
            }
            if let Effect::Screen(screen) = effect {
                self.tick_screen(pos.player, screen);
                if !perish_faints.is_empty()
                    && self.has_screen(pos.player, screen)
                    && self.resolve_perish_faints(&mut perish_faints)
                {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::Tailwind) {
                self.tick_tailwind(pos.player);
                if !perish_faints.is_empty()
                    && self.sides[pos.player.index()].as_ref().unwrap().tailwind != 0
                    && self.resolve_perish_faints(&mut perish_faints)
                {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::Wish) {
                self.tick_wish(pos);
                continue;
            }
            if matches!(effect, Effect::FutureMove) {
                self.tick_future_move(pos);
                if self.check_win() {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::TrickRoom) {
                self.trick_room -= 1;
                if self.trick_room == 0 {
                    self.add("|-fieldend|move: Trick Room");
                } else if self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::Gravity) {
                self.gravity -= 1;
                if self.gravity == 0 {
                    self.add("|-fieldend|move: Gravity");
                } else if self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::FairyLock) {
                self.fairy_lock -= 1;
                continue;
            }
            if matches!(effect, Effect::WonderRoom) {
                self.wonder_room -= 1;
                if self.wonder_room == 0 {
                    self.add("|-fieldend|move: Wonder Room");
                } else if self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::MagicRoom) {
                self.tick_magic_room();
                if self.magic_room.is_some() && self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
                continue;
            }
            if matches!(effect, Effect::Terrain) {
                self.tick_terrain();
                if self.terrain.is_some() && self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
                continue;
            }
            let mon = self.mon_mut(pos);
            if mon.fainted {
                continue;
            }
            if mon.gastro_acid
                && mon.active_ability().is_empty()
                && matches!(
                    effect,
                    Effect::ShedSkin
                        | Effect::Hydration
                        | Effect::HungerSwitch
                        | Effect::Healer
                        | Effect::SpeedBoost
                        | Effect::Harvest
                        | Effect::CudChew
                        | Effect::Pickup
                        | Effect::Opportunist
                        | Effect::Moody
                )
            {
                if self.format.report_exact_hp {
                    self.add(
                        "|debug|Residual handler suppressed by Gastro Acid or Neutralizing Gas",
                    );
                }
                continue;
            }
            let uproar_expired = matches!(effect, Effect::Uproar)
                && mon.uproar.is_some_and(|state| state.remaining == 1);
            match effect {
                Effect::PerishSong => {
                    if self.tick_perish_song(pos) {
                        // An expiring condition queues fainting but doesn't
                        // resolve it until the next non-expiring callback.
                        perish_faints.push(pos);
                        continue;
                    }
                }
                Effect::ThroatChop => {
                    mon.throat_chop -= 1;
                    if mon.throat_chop == 0 {
                        mon.untrack_volatile("throatchop");
                        self.add(format!("|-end|{}|Throat Chop|[silent]", self.ident(pos)));
                    }
                }
                Effect::HealBlock => {
                    mon.heal_block = mon.heal_block.saturating_sub(1);
                    if mon.heal_block == 0 {
                        self.end_heal_block(pos);
                    }
                }
                Effect::LegacyCondition(condition) => self.tick_legacy_condition(pos, condition),
                Effect::LaserFocus => {
                    mon.laser_focus -= 1;
                    if mon.laser_focus == 0 {
                        self.end_laser_focus(pos);
                    }
                }
                Effect::FuryCutter => {
                    super::streaks::Streak::tick(&mut mon.fury_cutter);
                    if mon.fury_cutter.is_none() {
                        mon.untrack_volatile("furycutter");
                    }
                }
                Effect::RollingAttack => mon.tick_rolling_attack(),
                Effect::FocusPunch => {
                    mon.lost_focus = false;
                    mon.untrack_volatile("focuspunch");
                }
                Effect::ChillyReception => mon.untrack_volatile("chillyreception"),
                Effect::BeakBlast => mon.untrack_volatile("beakblast"),
                Effect::HelpingHand => {
                    mon.helping_hand = 0;
                    mon.untrack_volatile("helpinghand");
                }
                Effect::Counter => {
                    if let Some(counter) = mon.counter.take() {
                        mon.untrack_volatile(counter.move_kind.id());
                    }
                }
                Effect::Leftovers | Effect::BlackSludge | Effect::StatusOrb | Effect::WhiteHerb
                    if mon.ignores_held_item() =>
                {
                    self.debug_ignored_item(pos, "Residual");
                }
                Effect::Leftovers => {
                    let amount = mon.max_hp.get() / 16;
                    self.item_heal(pos, amount, "Leftovers");
                }
                Effect::BlackSludge => {
                    if mon.has_type("Poison") {
                        let amount = mon.max_hp.get() / 16;
                        self.item_heal(pos, amount, "Black Sludge");
                    } else {
                        self.indirect_damage(pos, 8, "|[from] item: Black Sludge");
                        if self.mon(pos).hp == 0 {
                            self.mark_fainted(pos);
                            if self.check_win() {
                                return;
                            }
                        }
                    }
                }
                Effect::StatusOrb => self.activate_status_orb(pos),
                Effect::WhiteHerb => self.white_herb(pos),
                Effect::Weather
                | Effect::Screen(_)
                | Effect::SideGuard(_)
                | Effect::Safeguard
                | Effect::LuckyChant
                | Effect::EchoedVoice
                | Effect::LegacyField(_)
                | Effect::Mist
                | Effect::TrickRoom
                | Effect::Gravity
                | Effect::WonderRoom
                | Effect::MagicRoom
                | Effect::Terrain
                | Effect::Wish
                | Effect::FutureMove
                | Effect::Tailwind
                | Effect::FairyLock => {
                    unreachable!()
                }
                Effect::GrassyHeal => self.grassy_heal(pos),
                Effect::AquaRing => self.rooting_heal(pos, "Aqua Ring"),
                Effect::Ingrain => self.rooting_heal(pos, "Ingrain"),
                Effect::Octolock => self.tick_octolock(pos),
                Effect::SyrupBomb => self.tick_syrup_bomb(pos),
                Effect::LockOn => {
                    if super::target_effects::TargetTimer::tick(&mut mon.lock_on) {
                        mon.untrack_volatile("lockon");
                    }
                }
                Effect::Yawn => self.tick_yawn(pos),
                Effect::LeechSeed => {
                    self.tick_leech_seed(pos);
                    if self.check_win() {
                        return;
                    }
                }
                Effect::Curse => {
                    self.indirect_damage(pos, 4, "|[from] Curse");
                    if self.mon(pos).hp == 0 {
                        self.mark_fainted(pos);
                        if self.check_win() {
                            return;
                        }
                    }
                }
                Effect::SaltCure => {
                    self.tick_salt_cure(pos);
                    if self.check_win() {
                        return;
                    }
                }
                Effect::PartialTrap => {
                    self.tick_partial_trap(pos);
                    if self.check_win() {
                        return;
                    }
                }
                Effect::Disable => self.tick_disable(pos),
                Effect::Taunt => self.tick_taunt(pos),
                Effect::Encore => self.tick_encore(pos),
                Effect::ShedSkin => self.shed_skin(pos),
                Effect::Hydration => self.hydration(pos),
                Effect::HungerSwitch => self.hunger_switch(pos),
                Effect::Healer => self.healer_residual(pos),
                Effect::SpeedBoost => self.speed_boost(pos),
                Effect::Harvest => self.harvest_item(pos),
                Effect::CudChew => self.chew_berry(pos),
                Effect::Pickup => self.pick_up_item(pos),
                Effect::Opportunist => self.apply_pending_boost_copy(pos),
                Effect::Moody => self.moody(pos),
                Effect::Gem => {
                    mon.gem_boost = false;
                    mon.untrack_volatile("gem");
                }
                Effect::MagnetRise => {
                    mon.magnet_rise = mon.magnet_rise.saturating_sub(1);
                    if mon.magnet_rise == 0 {
                        mon.untrack_volatile("magnetrise");
                        self.add(format!("|-end|{}|Magnet Rise", self.ident(pos)));
                    }
                }
                Effect::Roost => {
                    mon.roosted = false;
                    mon.untrack_volatile("roost");
                }
                Effect::Protect => {
                    if let Some(protect) = mon.protection.take() {
                        mon.untrack_volatile(protect.volatile());
                    }
                }
                Effect::Endure => mon.untrack_volatile("endure"),
                Effect::Flinch => {
                    mon.flinched = false;
                    mon.untrack_volatile("flinch");
                }
                Effect::Recharge => {
                    mon.recharge -= 1;
                    if mon.recharge == 0 {
                        mon.untrack_volatile("mustrecharge");
                    }
                }
                Effect::Rampage => self.tick_rampage(pos),
                Effect::Uproar => self.tick_uproar(pos),
                Effect::Charge => mon.tick_charge(),
                // The move's condition has its own duration and participates
                // in ordering. The parent charge expires at the same time.
                Effect::SemiInvulnerableTimer => {}
                Effect::Attention => {
                    if let Some(kind) = mon.attention.take() {
                        mon.untrack_volatile(kind.name_id().1);
                    }
                }
                Effect::Stall => {
                    mon.stall = mon.stall.and_then(|(counter, duration)| {
                        (duration > 1).then_some((counter, duration - 1))
                    });
                    if mon.stall.is_none() {
                        mon.untrack_volatile("stall");
                    }
                }
                Effect::AllySwitchStall => {
                    mon.ally_switch_stall =
                        mon.ally_switch_stall.and_then(|(counter, duration)| {
                            (duration > 1).then_some((counter, duration - 1))
                        });
                    if mon.ally_switch_stall.is_none() {
                        mon.untrack_volatile("allyswitch");
                    }
                }
                Effect::Electrify => {
                    mon.electrified = false;
                    mon.untrack_volatile("electrify");
                }
                Effect::Status => {
                    self.status_residual(pos);
                    if self.check_win() {
                        return;
                    }
                }
            }
            if !perish_faints.is_empty() {
                let mon = self.mon(pos);
                let expiry = match effect {
                    Effect::Protect
                    | Effect::Endure
                    | Effect::Flinch
                    | Effect::Attention
                    | Effect::Roost
                    | Effect::HelpingHand
                    | Effect::FocusPunch
                    | Effect::BeakBlast
                    | Effect::ChillyReception
                    | Effect::Counter
                    | Effect::Electrify => true,
                    Effect::Stall => mon.stall.is_none(),
                    Effect::AllySwitchStall => mon.ally_switch_stall.is_none(),
                    Effect::Charge | Effect::SemiInvulnerableTimer => mon.charge.is_none(),
                    Effect::Rampage => mon.rampage.is_none(),
                    Effect::Uproar => uproar_expired,
                    Effect::Recharge => mon.recharge == 0,
                    _ => false,
                };
                if !expiry && self.resolve_perish_faints(&mut perish_faints) {
                    return;
                }
            }
        }
        self.add("|upkeep");
        self.resolve_perish_faints(&mut perish_faints);
    }

    fn resolve_perish_faints(&mut self, queued: &mut Vec<Position>) -> bool {
        if queued.is_empty() {
            return false;
        }
        for pos in queued.drain(..) {
            self.mark_fainted(pos);
        }
        self.check_win()
    }

    fn check_win(&mut self) -> bool {
        let alive = Player::ALL.map(|p| self.party(p).unwrap().iter().any(|mon| !mon.fainted));
        if alive == [true, true] {
            return false;
        }
        let winner = match alive {
            [true, false] => Some(Player::P1),
            [false, true] => Some(Player::P2),
            _ => self.last_faint,
        };
        self.finish(winner);
        true
    }

    fn side_magic_bounce(&mut self, player: Player) -> Option<Position> {
        let mut candidates = [(Position { player, slot: 0 }, 0_u32); 2];
        let mut count = 0;
        for slot in 0..self.slots() {
            let pos = Position { player, slot };
            let mon = self.mon(pos);
            if !mon.fainted && mon.active_ability() == "magicbounce" {
                candidates[count] = (pos, mon.stats.spe);
                count += 1;
            }
        }
        let candidates = &mut candidates[..count];
        // Unlike most event handlers, side Magic Bounce uses raw Speed.
        speed_sort_by(candidates, &mut self.rng, |a, b| b.1.cmp(&a.1));
        candidates
            .iter()
            .map(|&(pos, _)| pos)
            .find(|&pos| !self.ability_suppressed(pos))
    }

    fn bounce_move(
        &mut self,
        holder: Position,
        source: Position,
        data: &'static Move,
        queue: &mut Vec<Action>,
    ) {
        let prior_breaker = self.ability_breaker.take();
        self.run_move(
            holder,
            MoveInvocation::Reflected {
                data,
                target: source,
            },
            queue,
        );
        self.ability_breaker = prior_breaker;
    }

    // Whether this action reached the move attempt, for AfterMove hooks.
    fn run_move(
        &mut self,
        source: Position,
        invocation: MoveInvocation,
        queue: &mut Vec<Action>,
    ) -> (bool, Position) {
        let roster_index = self.mon(source).roster_index;
        let mut result = MoveResult::default();
        let attempted = self.run_move_inner(source, invocation, queue, &mut result);
        if matches!(invocation, MoveInvocation::Choice { .. }) {
            self.last_fusion = if result.counts_as_success && !result.failed {
                self.last_battle_move
                    .and_then(|data| super::streaks::Fusion::for_move(&data.name))
            } else {
                None
            };
        }
        let source = self.position_after_move(source, roster_index);
        if result.spends_charge {
            self.use_electric_charge(source);
        }
        if result.spends_stockpile {
            self.end_stockpile(source);
        }
        if result.ran_after_move {
            self.apply_pending_boost_copies();
        }
        if !matches!(invocation, MoveInvocation::Future { .. }) && !self.mon(source).fainted {
            let mon = self.mon_mut(source);
            mon.move_failed_this_turn = result.failed || !attempted;
            if let Some(metronome) = mon.metronome.as_mut() {
                metronome.this_turn_succeeded =
                    result.counts_as_success && !mon.move_failed_this_turn;
            }
        }
        (attempted, source)
    }

    fn run_move_inner(
        &mut self,
        source: Position,
        invocation: MoveInvocation,
        queue: &mut Vec<Action>,
        result: &mut MoveResult,
    ) -> bool {
        let (slot, loc, forced_target, reflected, prankster) = match invocation {
            MoveInvocation::Choice {
                slot,
                loc,
                prankster,
                ..
            } => (Some(slot), loc, None, false, prankster),
            MoveInvocation::Reflected { target, .. } => (None, 0, Some(target), true, false),
            MoveInvocation::Future { target, .. } => (None, 0, Some(target), false, false),
            MoveInvocation::SleepTalk {
                slot, prankster, ..
            }
            | MoveInvocation::Copycat {
                slot, prankster, ..
            } => (Some(slot), 0, None, false, prankster),
        };
        let sleep_talk = match invocation {
            MoveInvocation::SleepTalk {
                caller, priority, ..
            } => Some((caller, priority)),
            _ => None,
        };
        let caller = match invocation {
            MoveInvocation::Copycat {
                caller, priority, ..
            } => Some((caller, priority)),
            _ => sleep_talk,
        };
        let delayed = matches!(invocation, MoveInvocation::Future { .. });
        let own_action = !reflected && !delayed && caller.is_none();
        if own_action && self.mon(source).glaive_drawback {
            let mon = self.mon_mut(source);
            mon.glaive_drawback = false;
            mon.untrack_volatile("glaiverush");
            if self.format.report_exact_hp {
                self.add("|debug|removing Glaive Rush drawback before attack");
            }
        }
        if let Some(slot) = slot
            && (slot == RECHARGE_SLOT || self.mon(source).recharge > 0)
        {
            if slot == RECHARGE_SLOT {
                self.random_target(source, TargetKind::Normal);
            } else {
                self.resolve_target(source, self.move_data(source, slot).target, loc);
            }
            self.add(format!("|cant|{}|recharge", self.ident(source)));
            let mon = self.mon_mut(source);
            mon.recharge = 0;
            mon.untrack_volatile("mustrecharge");
            return true;
        }
        let base_data = match invocation {
            MoveInvocation::Choice { slot, .. }
            | MoveInvocation::SleepTalk { slot, .. }
            | MoveInvocation::Copycat { slot, .. } => self.move_data(source, slot),
            MoveInvocation::Reflected { data, .. } | MoveInvocation::Future { data, .. } => data,
        };
        let z_priority = match invocation {
            MoveInvocation::Choice { z_priority, .. } => z_priority,
            _ => None,
        };
        let z_powered = z_priority.is_some();
        let weather = self.effective_weather();
        let mega_sun = !delayed && self.event_ability(source) == "megasol";
        let move_weather = if mega_sun {
            Some(Weather::Sun)
        } else {
            self.mon(source).personal_weather(weather)
        };
        let ball_weather = if base_data.name == "Weather Ball" {
            move_weather.filter(|w| *w != Weather::StrongWinds)
        } else {
            None
        };
        let z_weather = if z_powered {
            ball_weather.map(Weather::ball_type)
        } else {
            None
        };
        let mut move_data = if z_powered {
            self.z_active_move(source, base_data, z_weather)
        } else {
            base_data
        };
        if move_data.ignore_ability && !delayed {
            self.ability_breaker = Some(source);
        }
        if matches!(
            move_data.name.as_str(),
            "Photon Geyser" | "Light That Burns the Sky"
        ) {
            let mon = self.mon(source);
            if boosted_stat(mon.stats.atk, mon.boosts.get(BoostStat::Atk))
                > boosted_stat(mon.stats.spa, mon.boosts.get(BoostStat::Spa))
            {
                move_data =
                    crate::dex::physical_psychic_power(self.format.dex_mod, &move_data.name);
            }
        }
        let z_status = z_powered && move_data.category == Category::Status;
        let z_animated = z_status && !move_data.is_z_move;
        let damaging_z = z_powered && !z_status;
        result.spends_charge =
            own_action && move_data.name != "Charge" && move_data.move_type == "Electric";
        let struggle = slot == Some(STRUGGLE_SLOT);
        let charge_spec = ChargeSpec::for_move(&move_data.name);
        let releasing_charge = own_action && self.mon(source).charge.is_some();
        let solar_weak = charge_spec.is_some_and(|spec| spec.weak_in_bad_weather)
            && matches!(
                move_weather.map(Weather::ordinary),
                Some(Weather::Rain | Weather::Sand | Weather::Snow | Weather::Hail)
            );
        let counter = CounterMove::for_move(move_data);
        let trapping_status = matches!(
            move_data.name.as_str(),
            "Block" | "Mean Look" | "Spider Web"
        );
        let retaliation =
            counter.is_some() || matches!(move_data.name.as_str(), "Metal Burst" | "Comeuppance");
        let sheer_cold = move_data.name == "Sheer Cold";
        let ohko = move_data.ohko;
        let retaliation_hit = if counter.is_some() {
            self.mon(source).counter.and_then(|state| state.hit)
        } else if retaliation {
            self.mon(source).last_foe_hit
        } else {
            None
        };
        let continuing = own_action
            && (self.mon(source).rampage.is_some()
                || self.mon(source).uproar.is_some()
                || self.mon(source).rolling_attack.is_some()
                || releasing_charge);
        let round_followup = matches!(
            invocation,
            MoveInvocation::Choice {
                round_followup: true,
                ..
            }
        );
        let called_from = if z_weather.is_some() {
            "|[from] move: Weather Ball"
        } else if reflected {
            "|[from] ability: Magic Bounce"
        } else if sleep_talk.is_some() {
            "|[from] move: Sleep Talk"
        } else if caller.is_some() {
            "|[from] move: Copycat"
        } else if round_followup {
            "|[from] move: Round"
        } else if continuing {
            "|[from] lockedmove"
        } else {
            ""
        };
        let weather_ball = move_data.name == "Weather Ball";
        let terrain_pulse = if move_data.name == "Terrain Pulse" && self.grounded(source) {
            self.terrain()
        } else {
            None
        };
        let stacked_power = matches!(move_data.name.as_str(), "Stored Power" | "Power Trip");
        let mut power = match move_data.name.as_str() {
            "Weather Ball" if ball_weather.is_some() => move_data.base_power * 2,
            "Terrain Pulse" if terrain_pulse.is_some() => move_data.base_power * 2,
            "Stored Power" | "Power Trip" => {
                move_data.base_power + self.mon(source).boosts.positive() * 20
            }
            "Last Respects" => {
                50 + 50
                    * u16::from(
                        self.sides[source.player.index()]
                            .as_ref()
                            .unwrap()
                            .total_fainted,
                    )
            }
            "Rage Fist" => 50 + 50 * self.mon(source).times_attacked.min(6) as u16,
            "Return" => self.mon(source).friendship_power(false),
            "Frustration" => self.mon(source).friendship_power(true),
            "Spit Up" => u16::from(self.mon(source).stockpile.layers) * 100,
            _ => move_data.base_power,
        };
        let typed_item = matches!(
            move_data.name.as_str(),
            "Judgment" | "Multi-Attack" | "Techno Blast" | "Natural Gift"
        )
        .then(|| DEX.item(self.format.dex_mod, self.mon(source).effective_item()))
        .flatten();
        let gift = typed_item
            .and_then(|item| item.natural_gift.as_ref())
            .filter(|_| move_data.name == "Natural Gift");
        let (mut move_type, converted) = if damaging_z {
            (move_data.move_type.as_str(), false)
        } else if struggle {
            ("???", false)
        } else if weather_ball {
            (move_weather.map_or("Normal", Weather::ball_type), false)
        } else if let Some(terrain) = terrain_pulse {
            (terrain.element(), false)
        } else if move_data.name == "Hidden Power" {
            (self.mon(source).hidden_power_type(), false)
        } else if move_data.name == "Revelation Dance" {
            (self.mon(source).types().next().unwrap_or("Normal"), false)
        } else if move_data.name == "Judgment" {
            (
                typed_item
                    .and_then(|item| item.plate_type.as_deref())
                    .unwrap_or("Normal"),
                false,
            )
        } else if move_data.name == "Multi-Attack" {
            (
                typed_item
                    .and_then(|item| item.memory_type.as_deref())
                    .unwrap_or("Normal"),
                false,
            )
        } else if move_data.name == "Techno Blast" {
            (
                typed_item
                    .and_then(|item| item.drive_type.as_deref())
                    .unwrap_or("Normal"),
                false,
            )
        } else if move_data.name == "Natural Gift" {
            (gift.map_or("Normal", |gift| gift.typing.as_str()), false)
        } else if move_data.name == "Ivy Cudgel" {
            (
                match self.mon(source).species.name.as_str() {
                    "Ogerpon-Wellspring" | "Ogerpon-Wellspring-Tera" => "Water",
                    "Ogerpon-Hearthflame" | "Ogerpon-Hearthflame-Tera" => "Fire",
                    "Ogerpon-Cornerstone" | "Ogerpon-Cornerstone-Tera" => "Rock",
                    _ => "Grass",
                },
                false,
            )
        } else if move_data.name == "Aura Wheel"
            && self.mon(source).species.name == "Morpeko-Hangry"
        {
            ("Dark", false)
        } else {
            let mon = self.mon(source);
            let base = if move_data.name == "Raging Bull" {
                match mon.species.name.as_str() {
                    "Tauros-Paldea-Combat" => "Fighting",
                    "Tauros-Paldea-Blaze" => "Fire",
                    "Tauros-Paldea-Aqua" => "Water",
                    _ => &move_data.move_type,
                }
            } else {
                &move_data.move_type
            };
            if mon.active_ability() == "liquidvoice" && move_data.flags.contains_key("sound") {
                ("Water", false)
            } else {
                mon.converted_move_type(base)
            }
        };
        let source_name = self.ident(source);
        let darts = move_data.name == "Dragon Darts";
        let curse = move_data.name == "Curse";
        let base_target = move_base_target(move_data, source, loc);
        let tracked_target = match invocation {
            MoveInvocation::Choice {
                original_target: Some((player, roster_index)),
                ..
            } if move_data.tracks_target
                || (matches!(
                    self.mon(source).active_ability(),
                    "stalwart" | "propellertail"
                ) && matches!(self.event_ability(source), "stalwart" | "propellertail")) =>
            {
                (0..self.slots())
                    .map(|slot| Position { player, slot })
                    .find(|&pos| {
                        let mon = self.mon(pos);
                        !mon.fainted && mon.roster_index == usize::from(roster_index)
                    })
            }
            _ => None,
        };
        let original_target = forced_target
            .or(tracked_target)
            .or_else(|| self.resolve_target(source, base_target, loc));
        let lost_counter_target =
            counter.is_some() && retaliation_hit.is_some_and(|hit| self.mon(hit.source).fainted);
        let mut target = retaliation_hit
            .filter(|_| !lost_counter_target)
            .map(|hit| hit.source)
            .or(original_target);
        if darts && target.is_some_and(|pos| self.mon(pos).fainted && pos.player == source.player) {
            target = self.random_target(source, TargetKind::Normal);
        }
        if move_data.name == "Chilly Reception"
            && self.mon(source).volatile_ids().contains(&"chillyreception")
        {
            self.add(format!(
                "|-prepare|{source_name}|Chilly Reception|[premajor]"
            ));
        }
        if own_action && !self.before_status_move(source, move_data, z_powered) {
            self.mon_mut(source).clear_charge();
            self.clear_destiny_bond(source, false);
            return false;
        }
        if !own_action && self.gravity > 0 && move_data.flags.contains_key("gravity") {
            self.add(format!(
                "|cant|{source_name}|move: Gravity|{}",
                move_data.name
            ));
            return false;
        }
        if (reflected || caller.is_some())
            && self.mon(source).throat_chop > 0
            && move_data.flags.contains_key("sound")
        {
            self.add(format!("|cant|{source_name}|move: Throat Chop"));
            return false;
        }
        if caller.is_some()
            && self.mon(source).heal_block > 0
            && move_data.flags.contains_key("heal")
        {
            self.add(format!(
                "|cant|{source_name}|move: Heal Block|{}",
                move_data.name
            ));
            return false;
        }
        let electrified = !delayed && !struggle && self.mon(source).electrified;
        if electrified {
            move_type = "Electric";
        }
        result.spends_charge =
            !reflected && !delayed && move_data.name != "Charge" && move_type == "Electric";
        if move_data.name == "Focus Punch" && self.mon(source).lost_focus {
            self.add(format!("|cant|{source_name}|Focus Punch|Focus Punch"));
            return false;
        }
        if own_action
            && !z_powered
            && !struggle
            && slot.is_some_and(|slot| self.mon(source).disabled_by_choice(slot))
        {
            self.add(format!("|move|{source_name}|{}||[still]", move_data.name));
            if self.format.report_exact_hp {
                self.add("|debug|Disabled by Choice item lock");
            }
            self.add(format!("|-fail|{source_name}"));
            self.mon_mut(source).clear_charge();
            self.clear_destiny_bond(source, false);
            return false;
        }
        let consecutive = move_data.cant_use_twice
            && self
                .mon(source)
                .last_move_slot
                .is_some_and(|last| self.mon(source).moves[last].move_data.name == move_data.name);
        if own_action && move_data.name != "Destiny Bond" {
            self.clear_destiny_bond(source, true);
        }
        let weather_at_target = if move_data.name == "Blizzard" {
            weather
        } else if mega_sun {
            Some(Weather::Sun)
        } else {
            target.and_then(|target| self.mon(target).personal_weather(weather))
        };
        let mut accuracy = match (
            move_data.name.as_str(),
            weather_at_target.map(Weather::ordinary),
        ) {
            ("Blizzard", Some(Weather::Snow | Weather::Hail)) => Accuracy::Bypass(true),
            (
                "Hurricane" | "Thunder" | "Bleakwind Storm" | "Sandsear Storm" | "Wildbolt Storm",
                Some(Weather::Rain),
            ) if target.is_some() => Accuracy::Bypass(true),
            ("Hurricane" | "Thunder", Some(Weather::Sun)) if target.is_some() => {
                Accuracy::Percent(50)
            }
            _ => move_data.accuracy,
        };
        if own_action
            && matches!(self.mon(source).status, Status::Freeze(_))
            && move_data.flags.contains_key("defrost")
        {
            self.cure_status(source, &format!("[from] move: {}", move_data.name));
        }
        if struggle {
            self.mon_mut(source).last_move_slot = None;
            self.add(format!("|-activate|{source_name}|move: Struggle"));
        } else if own_action && let Some(slot) = slot {
            let mon = self.mon_mut(source);
            if !continuing {
                if mon.moves[slot].pp == 0 {
                    self.add(format!("|cant|{source_name}|nopp|{}", move_data.name));
                    return false;
                }
                mon.moves[slot].pp -= 1;
                mon.used_moves |= 1 << slot;
            }
            mon.last_move_slot = (!z_powered && slot != EXTRA_MOVE_SLOT).then_some(slot);
            mon.last_move_target = loc;
            if slot != EXTRA_MOVE_SLOT && mon.holds_choice_item() && mon.choice_lock.is_none() {
                mon.choice_lock = Some(slot);
                mon.track_volatile("choicelock");
            }
        }
        let source_name = if z_powered {
            self.spend_z_move(source);
            self.ident(source)
        } else {
            source_name
        };
        let magnitude = if move_data.name == "Magnitude" {
            let (magnitude, value) = super::legacy_attacks::magnitude(self.rng.below(100));
            power = value;
            Some(magnitude)
        } else {
            None
        };
        let present_heal = if move_data.name == "Present" {
            power = match self.rng.below(10) {
                0..2 => 0,
                2..6 => 40,
                6..9 => 80,
                _ => 120,
            };
            power == 0
        } else {
            false
        };
        if move_data.name == "Secret Power"
            && let Some(terrain) = self.terrain()
        {
            move_data = super::legacy_attacks::secret_power(self.format.dex_mod, terrain);
        }
        if !delayed
            && matches!(
                self.mon(source).active_ability(),
                "moldbreaker" | "teravolt" | "turboblaze"
            )
        {
            self.ability_breaker = Some(source);
        }
        if !delayed {
            self.stance_change(source, move_data);
        }
        if electrified && self.format.report_exact_hp {
            self.add("|debug|Electrify making move type electric");
        }
        if !delayed && move_type == "Normal" && self.legacy_field_active(LegacyField::IonDeluge) {
            move_type = "Electric";
            result.spends_charge = !reflected;
            if self.format.report_exact_hp {
                self.add(format!(
                    "|debug|{}'s type changed to Electric",
                    move_data.name
                ));
            }
        }
        result.spends_stockpile = !delayed && move_data.name == "Spit Up";
        result.ran_after_move = own_action;
        let shell_side_arm = move_data.name == "Shell Side Arm";
        if shell_side_arm && let Some(target) = target {
            move_data = self.shell_side_arm(source, target, move_data);
        }
        let shell_category = if move_data.category == Category::Physical {
            "Physical"
        } else {
            "Special"
        };
        let contact = self.move_makes_contact(source, move_data);
        let expanded = move_data.name == "Expanding Force"
            && self.terrain() == Some(Terrain::Psychic)
            && self.grounded(source);
        let sheer_force_removed =
            !delayed && self.sheer_force_removes_secondaries(source, move_data);
        let added_flinch = if !delayed && move_data.category != Category::Status {
            let user = self.mon(source);
            let stench =
                user.active_ability() == "stench" && self.event_ability(source) == "stench";
            if stench && self.format.report_exact_hp {
                self.add("|debug|Adding Stench flinch");
            }
            (stench || matches!(self.mon(source).effective_item(), "kingsrock" | "razorfang"))
                && (sheer_force_removed
                    || !move_data
                        .secondaries
                        .iter()
                        .any(|effect| effect.volatile_status.as_deref() == Some("flinch")))
        } else {
            false
        };
        let kind = if expanded {
            // Both ModifyMove passes retarget a changed target kind.
            self.random_target(source, TargetKind::AllAdjacentFoes);
            target = self.random_target(source, TargetKind::AllAdjacentFoes);
            TargetKind::AllAdjacentFoes
        } else if curse {
            if self.format.report_exact_hp {
                let recipient = target.map_or_else(|| "null".into(), |pos| self.ident(pos));
                self.add(format!(
                    "|debug|Curse onModifyMove triggered|{source_name}|{recipient}"
                ));
            }
            let kind = if !self.mon(source).has_type("Ghost") {
                TargetKind::User
            } else if target.is_none_or(|pos| pos.player == source.player && pos != source) {
                TargetKind::RandomNormal
            } else {
                base_target
            };
            if kind != base_target {
                self.random_target(source, kind);
                target = self.random_target(source, kind);
            }
            kind
        } else {
            move_data.target
        };
        let mut spread_targets = Vec::new();
        match kind {
            TargetKind::Allies => {
                target = Some(source);
                spread_targets.extend(
                    (0..self.slots())
                        .map(|slot| Position { slot, ..source })
                        .filter(|&pos| !self.mon(pos).fainted),
                );
            }
            TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes => {
                if kind == TargetKind::AllAdjacent && self.slots() == 2 {
                    let mate = Position {
                        slot: 1 - source.slot,
                        ..source
                    };
                    if !self.mon(mate).fainted {
                        spread_targets.push(mate);
                    }
                }
                spread_targets.extend(
                    (0..self.slots())
                        .map(|slot| Position {
                            player: source.player.foe(),
                            slot,
                        })
                        .filter(|pos| !self.mon(*pos).fainted),
                );
            }
            TargetKind::FoeSide | TargetKind::AllySide | TargetKind::AllyTeam | TargetKind::All => {
                spread_targets.extend(target);
            }
            _ => spread_targets.extend(target.filter(|pos| !self.mon(*pos).fainted)),
        }
        if lost_counter_target {
            spread_targets.clear();
        }
        let actual_target = if target.is_some_and(|target| spread_targets.contains(&target)) {
            target
        } else {
            spread_targets.last().copied().or(target)
        };
        let target_name = actual_target.map_or_else(
            || "null".into(),
            |pos| {
                if self.mon(pos).fainted {
                    format!("{}: {}", pos.player.id(), self.mon(pos).name)
                } else {
                    self.ident(pos)
                }
            },
        );
        if weather_ball && self.format.report_exact_hp {
            self.add(format!("|debug|BP: {power}"));
        }
        let previous = self.last_battle_move;
        let previous_z = self.last_battle_move_z;
        let history_known = self.history_known;
        if !delayed {
            self.last_battle_move = Some(move_data);
            self.last_battle_move_z = z_powered || move_data.is_z_move;
            self.history_known = true;
            result.counts_as_success = true;
        }
        if move_data.name == "Body Press" && self.wonder_room > 0 {
            self.add("|-hint|Body Press uses Def boosts when Wonder Room is active.");
        }
        let base = if delayed {
            self.log
                .iter()
                .rposition(|line| line.public.starts_with("|move|"))
                .unwrap_or(self.log.len() - 1)
        } else {
            if terrain_pulse.is_some() && self.format.report_exact_hp {
                self.add("|debug|BP doubled in Terrain");
            }
            let base = self.log.len();
            let prefix = if z_animated { "Z-" } else { "" };
            let suffix = if z_animated {
                format!("|[anim]{}{called_from}", move_data.name)
            } else {
                String::new()
            };
            let attributes = if z_animated {
                suffix.as_str()
            } else {
                called_from
            };
            self.add(format!(
                "|move|{source_name}|{prefix}{}|{target_name}{attributes}",
                move_data.name
            ));
            base
        };
        if damaging_z {
            self.log[base].public.push_str("|[zeffect]");
        } else if z_status {
            self.apply_z_power(source, move_data);
        }
        let mut redirected = false;
        if !delayed
            && !lost_counter_target
            && self.slots() > 1
            && !move_data.tracks_target
            && (kind == TargetKind::Scripted
                || !matches!(self.event_ability(source), "stalwart" | "propellertail"))
            && let Some(target) = actual_target.filter(|_| {
                kind.is_choosable()
                    || matches!(kind, TargetKind::RandomNormal | TargetKind::Scripted)
            })
            && let Some(redirect) = self.redirect_target(source, target, move_data, move_type)
        {
            redirected = true;
            spread_targets.clear();
            spread_targets.push(redirect);
            let redirected_ident = self.ident(redirect);
            let line = &mut self.log[base].public;
            let target_start = line.match_indices('|').nth(3).unwrap().0 + 1;
            let target_end = line[target_start..]
                .find('|')
                .map_or(line.len(), |end| target_start + end);
            line.replace_range(target_start..target_end, &redirected_ident);
        }
        let mut smart_target = false;
        if darts
            && !redirected
            && self.slots() == 2
            && let Some(&first) = spread_targets.first()
        {
            let mate = Position {
                slot: 1 - first.slot,
                ..first
            };
            if mate != source && self.mon(mate).hp > 0 {
                spread_targets.push(mate);
                smart_target = true;
            }
        }
        let sacrifice = matches!(
            move_data.name.as_str(),
            "Explosion" | "Self-Destruct" | "Misty Explosion"
        );
        let final_gambit = move_data.name == "Final Gambit";
        let mind_blown = move_data.name == "Mind Blown";
        let half_recoil = match move_data.name.as_str() {
            "Mind Blown" => Some("mindblown"),
            "Steel Beam" => Some("steelbeam"),
            "Chloroblast" => Some("Recoil"),
            _ => None,
        };
        let spent_element = match move_data.name.as_str() {
            "Burn Up" => Some("Fire"),
            "Double Shock" => Some("Electric"),
            _ => None,
        };
        if spread_targets.is_empty() && !sacrifice && !mind_blown {
            self.log[base].public.push_str("|[notarget]");
            self.add(format!("|-fail|{source_name}"));
            self.note_consecutive_move(consecutive, move_data);
            result.failed = true;
            return true;
        }
        let after_move_target = spread_targets.last().copied().or(actual_target);
        if matches!(move_data.name.as_str(), "Rollout" | "Ice Ball")
            && let (Some(slot), Some(target)) = (slot, target)
        {
            self.mon_mut(source).begin_rolling_attack(
                &move_data.name,
                slot,
                if caller.is_some() {
                    target.loc(source)
                } else {
                    loc
                },
            );
        }

        if let Some(slot) = caller.map(|(caller, _)| caller).or(slot)
            && !struggle
            && !continuing
        {
            let pressure_targets =
                if kind == TargetKind::All || move_data.flags.contains_key("mustpressure") {
                    self.party(source.player.foe()).unwrap()[..self.slots()]
                        .iter()
                        .filter(|mon| mon.hp != 0 && mon.active_ability() == "pressure")
                        .count()
                } else if kind.is_choosable()
                    || matches!(
                        kind,
                        TargetKind::AllAdjacent
                            | TargetKind::AllAdjacentFoes
                            | TargetKind::RandomNormal
                            | TargetKind::Scripted
                    )
                {
                    spread_targets
                        .iter()
                        .filter(|pos| {
                            pos.player != source.player
                                && self.mon(**pos).active_ability() == "pressure"
                        })
                        .count()
                } else {
                    0
                };
            self.mon_mut(source).moves[slot].pp = self.mon(source).moves[slot]
                .pp
                .saturating_sub(pressure_targets as u8);
        }
        if let Some(ty) = spent_element
            && !self.mon(source).has_type(ty)
        {
            self.add(format!("|-fail|{source_name}|move: {}", move_data.name));
            self.move_still(base);
            // Its TryMove hook deliberately returns null, not false.
            return true;
        }
        let priority = caller.map_or_else(
            || {
                z_priority.unwrap_or_else(|| {
                    self.base_move_priority(source, move_data) + i8::from(prankster)
                })
            },
            |(_, priority)| priority,
        );
        if !delayed
            && move_data.category != Category::Status
            && let Some((label, element)) = weather.and_then(|w| w.extinguishes(move_type))
        {
            if self.format.report_exact_hp {
                self.add(format!("|debug|{label} {element} suppress"));
            }
            self.add(format!(
                "|-fail|{source_name}|{}|[from] {label}",
                move_data.name
            ));
            self.move_still(base);
            return true;
        }
        if !delayed
            && let Some(target) = spread_targets.last().copied().or(actual_target)
            && let Some((holder, name)) =
                self.try_move_guard(source, target, move_data, priority, sacrifice || mind_blown)
        {
            self.move_still(base);
            self.add(format!(
                "|cant|{}|ability: {name}|{}|[of] {source_name}",
                self.ident(holder),
                move_data.name
            ));
            result.failed = true;
            return true;
        }
        if !delayed {
            if move_type == "Fire" && self.mon(source).under(LegacyCondition::Powder) {
                self.stop_fire_with_powder(source, &move_data.name);
                self.move_still(base);
                if self.mon(source).hp == 0 {
                    self.mark_fainted(source);
                }
                result.failed = true;
                return true;
            }
            self.metronome_try_move(source, move_data);
        }
        if let Some(magnitude) = magnitude {
            self.add(format!(
                "|-activate|{source_name}|move: Magnitude|{magnitude}"
            ));
        }
        if move_data.name == "Echoed Voice" {
            self.continue_echoed_voice();
        }
        if move_data.name == "Aura Wheel" && self.mon(source).species.base_species != "Morpeko" {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}|move: Aura Wheel"));
            self.add(
                "|-hint|Only a Pokemon whose form is Morpeko or Morpeko-Hangry can use this move.",
            );
            return true;
        }
        if move_data.name == "Hyperspace Fury" && self.mon(source).species.name != "Hoopa-Unbound" {
            self.add("|-hint|Only a Pokemon whose form is Hoopa Unbound can use this move.");
            self.move_still(base);
            let suffix = if self.mon(source).species.name == "Hoopa" {
                "|[forme]"
            } else {
                ""
            };
            self.add(format!(
                "|-fail|{source_name}|move: Hyperspace Fury{suffix}"
            ));
            return true;
        }
        if move_data.name == "Sleep Talk" {
            let may_call = matches!(self.mon(source).status, Status::Sleep(_))
                || self.event_ability(source) == "comatose";
            let eligible = |data: &Move| {
                !["nosleeptalk", "charge"]
                    .iter()
                    .any(|&flag| data.flags.contains_key(flag))
            };
            let choices: Vec<_> = if may_call {
                self.mon(source)
                    .moves
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, m)| eligible(m.move_data).then_some(slot))
                    .collect()
            } else {
                Vec::new()
            };
            if choices.is_empty() {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
            let called_slot = choices[self.rng.below(choices.len() as u32) as usize];
            let called = &self.mon(source).moves[called_slot];
            if !Self::move_supported(called) {
                self.unsupported_called_move = Some(("Sleep Talk called", &called.move_data.name));
                return true;
            }
            let (_, source) = self.run_move(
                source,
                MoveInvocation::SleepTalk {
                    slot: called_slot,
                    caller: slot.expect("Sleep Talk is never reflected or recursively called"),
                    priority,
                    prankster,
                },
                queue,
            );
            result.failed = self.mon(source).move_failed_this_turn;
            self.each_event();
            self.each_event();
            return true;
        }
        if !delayed && matches!(move_data.name.as_str(), "Future Sight" | "Doom Desire") {
            if !self.schedule_future_move(source, spread_targets[0], move_data) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if let Some(effect) = super::support::ReplacementHeal::from_move(&move_data.name) {
            if self.first_bench(source.player, 0).is_none() {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.protean_prepare(source, move_type);
                self.prepare_replacement_heal(source, effect);
                self.mon_mut(source).hp = 0;
                self.each_event();
                self.mark_fainted(source);
                self.each_event();
            }
            return true;
        }
        if move_data.name == "Belch" && !self.mon(source).ate_berry {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "Snore"
            && !matches!(self.mon(source).status, Status::Sleep(_))
            && self.event_ability(source) != "comatose"
        {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "Round"
            && let Some(index) = queue.iter().position(|action| {
                action.move_data.is_some_and(|data| data.name == "Round")
                    && matches!(action.kind, ActionKind::Move { .. })
            })
        {
            Self::promote_move(queue, index, true);
        }
        if move_data.name == "Ally Switch" {
            let stall = self
                .mon(source)
                .ally_switch_stall
                .map_or(1, |(counter, _)| counter);
            if stall > 1 {
                if self.format.report_exact_hp {
                    self.add(format!(
                        "|debug|Ally Switch success chance: {}%",
                        (100 + stall / 2) / stall
                    ));
                }
                if !self.rng.chance(1, u32::from(stall)) {
                    let mon = self.mon_mut(source);
                    mon.ally_switch_stall = None;
                    mon.untrack_volatile("allyswitch");
                    self.move_still(base);
                    self.add(format!("|-fail|{source_name}"));
                    result.failed = true;
                    return true;
                }
            }
            let mon = self.mon_mut(source);
            mon.ally_switch_stall = Some(((stall * 3).min(729), 2));
            mon.track_volatile("allyswitch");
            self.protean_prepare(source, move_type);
            if self.slots() == 1 || self.mon(source.other_slot()).fainted {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}|move: Ally Switch"));
            } else {
                self.ally_switch(source, queue);
                self.each_event();
                self.each_event();
            }
            return true;
        }
        if move_data.name == "Stuff Cheeks" && !self.has_held_berry(source) {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if matches!(
            (move_data.name.as_str(), self.mon(source).stockpile.layers),
            ("Stockpile", 3) | ("Spit Up" | "Swallow", 0)
        ) {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "No Retreat" && self.mon(source).volatile_ids().contains(&"noretreat")
        {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if sacrifice || mind_blown {
            if sacrifice {
                // The user is queued to faint before any target, but its
                // faint log and cleanup happen after damage is applied.
                self.mon_mut(source).hp = 0;
            }
            if spread_targets.is_empty() {
                self.log[base].public.push_str("|[notarget]");
                self.add(format!("|-fail|{source_name}"));
                if sacrifice {
                    self.mark_fainted(source);
                }
                result.failed = true;
                return true;
            }
        }
        if let Some(spec) = charge_spec {
            if releasing_charge {
                self.mon_mut(source).release_charge();
            } else {
                self.add(format!("|-prepare|{source_name}|{}", move_data.name));
                if let Some(stat) = spec.boost {
                    self.boost(source, &[(stat, 1)], false);
                }
                if mega_sun && weather != Some(Weather::Sun) && spec.instant == Some(Weather::Sun) {
                    self.add(format!("|-activate|{source_name}|ability: Mega Sol"));
                }
                let weather_skip =
                    spec.instant.is_some() && spec.instant == move_weather.map(Weather::ordinary);
                let herb_skip = !weather_skip && self.mon(source).effective_item() == "powerherb";
                self.move_still(base);
                if weather_skip || herb_skip {
                    if herb_skip {
                        self.consume(source, "Power Herb", false);
                        self.symbiosis_after_item(source);
                        if self.format.report_exact_hp {
                            self.add(format!(
                                "|debug|power herb - remove charge turn for {}",
                                spec.id
                            ));
                        }
                    }
                    self.add(format!(
                        "|-anim|{source_name}|{}|{}",
                        move_data.name,
                        self.ident(spread_targets[0])
                    ));
                } else {
                    self.mon_mut(source).begin_charge(
                        spec,
                        slot.expect("charge moves are not reflectable"),
                        if caller.is_some() {
                            spread_targets[0].loc(source)
                        } else {
                            loc
                        },
                    );
                    self.protean_prepare(source, move_type);
                    return true;
                }
            }
        }
        if let Some(screen) = Screen::from_move(&move_data.name) {
            if !matches!(screen, Screen::AuroraVeil)
                || matches!(
                    self.effective_weather(),
                    Some(Weather::Snow | Weather::Hail)
                )
            {
                self.protean_prepare(source, move_type);
            }
            if !self.set_screen(source, screen) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Tailwind" {
            self.protean_prepare(source, move_type);
            if !self.start_tailwind(source.player) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.charge_from_tailwind(source.player);
            }
            return true;
        }
        if move_data.name == "Fairy Lock" {
            self.protean_prepare(source, move_type);
            if self.fairy_lock == 0 {
                self.fairy_lock = 2;
                self.add("|-fieldactivate|move: Fairy Lock");
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Safeguard" {
            self.protean_prepare(source, move_type);
            if !self.start_safeguard(source.player) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Mist" {
            self.protean_prepare(source, move_type);
            if self.start_mist(source.player) {
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }
        if matches!(move_data.name.as_str(), "Heal Bell" | "Aromatherapy") {
            self.protean_prepare(source, move_type);
            if self.cure_team(source, move_data) {
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Magnetic Flux" {
            self.protean_prepare(source, move_type);
            let mut raised = false;
            for slot in 0..self.slots() {
                let target = Position { slot, ..source };
                if matches!(self.event_ability(target), "plus" | "minus") {
                    raised |= self.boost_from(
                        target,
                        source,
                        &[(BoostStat::Def, 1), (BoostStat::Spd, 1)],
                        BoostOrigin::MagneticFlux,
                    );
                }
            }
            if !raised {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                if self.format.report_exact_hp {
                    self.add("|debug|move failed because it did nothing");
                }
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Teatime" {
            self.protean_prepare(source, move_type);
            let mut eaters = 0;
            for target in self.positions() {
                if self.mon(target).hp == 0 {
                    continue;
                }
                if self.mon(target).semi_invulnerable()
                    && !matches!(self.event_ability(target), "noguard")
                    && self.event_ability(source) != "noguard"
                {
                    self.add(format!("|-miss|{source_name}|{}", self.ident(target)));
                } else if self.event_ability(target) == "goodasgold" && target != source {
                    self.add(format!(
                        "|-immune|{}|[from] ability: Good as Gold",
                        self.ident(target)
                    ));
                } else if self.has_held_berry(target) {
                    eaters |= target.mask();
                }
            }
            self.add("|-fieldactivate|move: Teatime");
            if eaters == 0 {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}|move: Teatime"));
            } else {
                for target in self.positions() {
                    if eaters & target.mask() != 0 {
                        self.force_eat_berry(target, source);
                    }
                }
            }
            return true;
        }
        if let Some(hazard) = Hazard::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if !reflected
                && move_data.flags.contains_key("reflectable")
                && let Some(holder) = self.side_magic_bounce(source.player.foe())
                && !self.mon(spread_targets[0]).semi_invulnerable()
            {
                self.bounce_move(holder, source, move_data, queue);
                result.failed = true;
                return true;
            }
            if !self.add_hazard(source.player.foe(), hazard) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Court Change" {
            self.protean_prepare(source, move_type);
            if !self.court_change(source) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if let Some(guard) = SideGuard::for_move(&move_data.name) {
            let more_actions = queue.iter().any(|action| {
                matches!(
                    action.kind,
                    ActionKind::Move { .. } | ActionKind::Switch { .. }
                )
            });
            let too_late = guard == SideGuard::MatBlock && self.mon(source).active_move_actions > 1;
            if too_late {
                self.add("|-hint|Mat Block only works on your first turn out.");
            }
            if !too_late && more_actions && self.start_side_guard(source, guard) {
                if guard.grows_stall() {
                    let mon = self.mon_mut(source);
                    let stall = mon.stall.map_or(1, |(counter, _)| counter);
                    mon.track_volatile("stall");
                    mon.stall = Some(((stall * 3).min(729), 2));
                }
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Haze" {
            self.protean_prepare(source, move_type);
            self.add("|-clearallboost");
            for pos in self.positions() {
                self.mon_mut(pos).boosts = Boosts::default();
            }
            return true;
        }
        if move_data.name == "Perish Song" {
            self.protean_prepare(source, move_type);
            if !self.perish_song(source, priority) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Trick Room" {
            self.protean_prepare(source, move_type);
            if self.trick_room > 0 {
                self.trick_room = 0;
                self.add("|-fieldend|move: Trick Room");
            } else {
                self.trick_room = 5;
                self.add(format!("|-fieldstart|move: Trick Room|[of] {source_name}"));
            }
            return true;
        }
        if move_data.name == "Gravity" {
            self.protean_prepare(source, move_type);
            if self.gravity > 0 {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.gravity = 5;
                self.add("|-fieldstart|move: Gravity");
                for target in self.positions() {
                    let canceled = self.cancel_airborne_charge(target, queue);
                    let grounded = self.clear_magnet_rise(target);
                    let was_lifted = self
                        .mon_mut(target)
                        .clear_legacy_condition(LegacyCondition::Telekinesis);
                    if canceled || grounded || was_lifted {
                        self.add(format!("|-activate|{}|move: Gravity", self.ident(target)));
                    }
                }
            }
            return true;
        }
        if move_data.name == "Wonder Room" {
            self.protean_prepare(source, move_type);
            if self.wonder_room > 0 {
                self.wonder_room = 0;
                self.add("|-fieldend|move: Wonder Room");
            } else {
                self.wonder_room = 5;
                self.add(format!("|-fieldstart|move: Wonder Room|[of] {source_name}"));
            }
            return true;
        }
        if move_data.name == "Magic Room" {
            self.protean_prepare(source, move_type);
            self.toggle_magic_room(source);
            return true;
        }
        if move_data.name == "Happy Hour" {
            self.protean_prepare(source, move_type);
            return true;
        }
        if let Some(weather) = Weather::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if !self.set_weather(source, weather, None) {
                if let Some(lock) = self.weather_lock(weather) {
                    self.add(format!(
                        "|-fail|{source_name}|{}|[from] {lock}",
                        move_data.name
                    ));
                } else {
                    self.move_still(base);
                    self.add(format!("|-fail|{source_name}"));
                    result.failed = true;
                }
            }
            return true;
        }
        if move_data.name == "Chilly Reception" {
            self.protean_prepare(source, move_type);
            let snow_started = self.set_weather(source, Weather::Snow, None);
            if self.first_bench(source.player, 0).is_some() {
                self.mon_mut(source).pivot_move = Some(Pivot::Move("Chilly Reception"));
                self.each_event();
                self.each_event();
            } else if !snow_started {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }
        if let Some(effect) = LegacyField::for_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if self.start_legacy_field(source, effect, &move_data.name) {
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }
        if matches!(
            move_data.name.as_str(),
            "Flower Shield" | "Rototiller" | "Gear Up"
        ) {
            self.protean_prepare(source, move_type);
            if self.apply_field_stage_move(source, move_data) {
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Lucky Chant" {
            self.protean_prepare(source, move_type);
            if !self.start_lucky_chant(source.player) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.each_event();
                self.each_event();
            }
            return true;
        }
        if move_data.name == "Imprison" {
            self.protean_prepare(source, move_type);
            let mon = self.mon_mut(source);
            if mon.imprisoned {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                mon.imprisoned = true;
                mon.track_volatile("imprison");
                self.add(format!("|-start|{source_name}|move: Imprison"));
                self.each_event();
                self.each_event();
            }
            return true;
        }
        if let Some(terrain) = Terrain::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if !self.set_terrain(source, terrain, None) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Rest" {
            let mon = self.mon(source);
            let ability = self.event_ability(source);
            if matches!(mon.status, Status::Sleep(_)) || ability == "comatose" {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else if mon.hp == mon.max_hp.get() {
                self.add(format!("|-fail|{source_name}|heal"));
                result.failed = true;
            } else if matches!(ability, "insomnia" | "vitalspirit") {
                let ability = if ability == "insomnia" {
                    "Insomnia"
                } else {
                    "Vital Spirit"
                };
                self.add(format!(
                    "|-fail|{source_name}|[from] ability: {ability}|[of] {source_name}"
                ));
                result.failed = true;
            } else {
                self.protean_prepare(source, move_type);
                if self.status_blocked_by_ability(source, "slp", false)
                    || self.field_blocks_status(
                        source,
                        "slp",
                        source.player,
                        self.terrain() == Some(Terrain::Electric),
                    )
                {
                    self.move_still(base);
                    self.add(format!("|-fail|{source_name}"));
                    result.failed = true;
                    return true;
                }
                self.set_status(source, "slp", source.player, Some("Rest"));
                let mon = self.mon_mut(source);
                if matches!(mon.status, Status::Sleep(_)) {
                    mon.status = Status::Sleep(3);
                }
                mon.hp = mon.max_hp.get();
                self.add_health_line_with("-heal", source, false, "|[silent]");
                self.each_event();
                self.each_event();
            }
            return true;
        }
        if move_data.name == "Revival Blessing" {
            self.protean_prepare(source, move_type);
            if self.party(source.player).unwrap().iter().any(|p| p.fainted) {
                self.mon_mut(source).reviving = true;
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Clangorous Soul" {
            let mon = self.mon(source);
            let cost = mon.max_hp.get() * 33 / 100;
            if mon.hp <= cost || mon.max_hp.get() == 1 {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
            self.protean_prepare(source, move_type);
            if self.boost(source, &move_data.boosts, true) {
                self.mon_mut(source).hp -= cost.max(1);
                self.add_health_line("-damage", source, false);
                self.each_event();
                self.each_event();
            } else {
                result.failed = true;
            }
            return true;
        }
        if matches!(move_data.name.as_str(), "Substitute" | "Shed Tail") {
            let shed_tail = move_data.name == "Shed Tail";
            if shed_tail && self.first_bench(source.player, 0).is_none() {
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
            self.protean_prepare(source, move_type);
            if self.make_substitute(source, shed_tail) {
                if shed_tail {
                    self.mon_mut(source).pivot_move = Some(Pivot::Move("Shed Tail"));
                }
                self.each_event();
                self.each_event();
            } else {
                result.failed = true;
            }
            return true;
        }
        if move_data.name == "Baton Pass" {
            self.protean_prepare(source, move_type);
            if self.first_bench(source.player, 0).is_some() {
                self.mon_mut(source).pivot_move = Some(Pivot::Move("Baton Pass"));
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            }
            return true;
        }
        if let Some(protect) = Protection::from_move(&move_data.name) {
            let more_actions = queue
                .iter()
                .any(|q| matches!(q.kind, ActionKind::Move { .. } | ActionKind::Switch { .. }));
            let stall = self.mon(source).stall.map_or(1, |(counter, _)| counter);
            if more_actions && stall > 1 && self.format.report_exact_hp {
                self.add(format!(
                    "|debug|Success chance: {}%",
                    (100 + stall / 2) / stall
                ));
            }
            if !more_actions || (stall > 1 && !self.rng.chance(1, u32::from(stall))) {
                if more_actions {
                    self.mon_mut(source).stall = None;
                    self.mon_mut(source).untrack_volatile("stall");
                }
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
            self.protean_prepare(source, move_type);
            let mon = self.mon_mut(source);
            if protect != Protection::Endure {
                mon.protection = Some(protect);
            }
            mon.track_volatile(protect.volatile());
            mon.track_volatile("stall");
            mon.stall = Some(((stall * 3).min(729), 2));
            self.add(format!(
                "|-singleturn|{source_name}|{}",
                protect.start_label()
            ));
            self.each_event();
            self.each_event();
            return true;
        }
        if move_data.name == "Destiny Bond" {
            if self.clear_destiny_bond(source, false) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.protean_prepare(source, move_type);
                let mon = self.mon_mut(source);
                mon.destiny_bond = true;
                mon.track_volatile("destinybond");
                self.add(format!("|-singlemove|{source_name}|Destiny Bond"));
                self.each_event();
                self.each_event();
            }
            return true;
        }

        if let Some(attention) = match move_data.name.as_str() {
            "Follow Me" => Some(Attention::FollowMe),
            "Rage Powder" => Some(Attention::RagePowder),
            _ => None,
        } {
            if self.slots() == 1 {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
            } else {
                self.protean_prepare(source, move_type);
                let (name, id) = attention.name_id();
                let mon = self.mon_mut(source);
                mon.attention = Some(attention);
                mon.track_volatile(id);
                self.add(format!("|-singleturn|{source_name}|move: {name}"));
                self.each_event();
                self.each_event();
            }
            return true;
        }

        if self.mon(source).active_move_actions > 1
            && matches!(move_data.name.as_str(), "Fake Out" | "First Impression")
        {
            self.add(format!(
                "|-hint|{} only works on your first turn out.",
                move_data.name
            ));
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "Steel Roller" && self.terrain.is_none() {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "Poltergeist" && self.mon(spread_targets[0]).item.is_empty() {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if move_data.name == "Magnet Rise" && self.forces_grounded(source) {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if retaliation && !retaliation_hit.is_some_and(|hit| hit.turn == self.turn) {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
        }
        if matches!(
            move_data.name.as_str(),
            "Sucker Punch" | "Thunderclap" | "Upper Hand"
        ) {
            let target = spread_targets[0];
            let needs_priority = move_data.name == "Upper Hand";
            let pending_attack = self.mon(target).recharge == 0
                && Self::queued_move(queue, target).is_some_and(|slot| {
                    slot != RECHARGE_SLOT
                        && self.move_data(target, slot).category != Category::Status
                        && (!needs_priority
                            || self.move_priority(target, self.move_data(target, slot)) > 0)
                });
            if !pending_attack {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
        }
        let thrown_item = if move_data.name == "Fling" {
            let Some(item) = self.prepare_fling(source) else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            };
            Some(item)
        } else {
            None
        };
        let mut power = thrown_item
            .and_then(|item| item.fling.as_ref())
            .map_or(power, |fling| fling.base_power);
        let fling_secondary = thrown_item
            .filter(|item| !item.is_berry)
            .and_then(|item| item.fling.as_ref())
            .filter(|effect| effect.status.is_some() || effect.volatile_status.is_some());
        if !delayed {
            self.protean_prepare(source, move_type);
        }
        if move_data.name == "Natural Gift" {
            let Some(gift) = gift else {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            };
            power = gift.base_power;
            if self.format.report_exact_hp {
                self.add(format!("|debug|BP: {power}"));
            }
            self.use_natural_gift_item(source);
        }
        if move_data.name == "Ivy Cudgel" && move_type != "Grass" {
            self.log[base]
                .public
                .push_str(&format!("|[anim] Ivy Cudgel {move_type}"));
        }
        if shell_side_arm && spread_targets[0].player != source.player {
            self.log[base]
                .public
                .push_str(&format!("|[anim] Shell Side Arm {shell_category}"));
        }
        let spread = !darts && spread_targets.len() > 1;
        let potential = spread_targets.len();
        spread_targets.retain(|&target| {
            if self
                .mon(target)
                .semi_invulnerability()
                .is_none_or(|state| state.exposed_to(&move_data.name))
                || self.locked_on_to(source, target)
                || self.mon(source).active_ability() == "noguard"
                || self.mon(target).active_ability() == "noguard"
                || (move_data.name == "Toxic" && self.mon(source).has_type("Poison"))
            {
                return true;
            }
            if smart_target {
                smart_target = false;
                return false;
            }
            if !spread {
                self.log[base].public.push_str("|[miss]");
            }
            self.add(format!("|-miss|{source_name}|{}", self.ident(target)));
            false
        });
        let mut failed_hit = spread_targets.len() != potential;
        // Preserve upstream's hit-step batching: all protection and immunity
        // checks, then all accuracy rolls, then all damage calculations.
        let potential = spread_targets.len();
        let mut protected = 0;
        let mut bypassed = 0;
        spread_targets.retain(|&mut_pos| {
            if let Some(guard) =
                self.blocking_side_guard(mut_pos.player, move_data, kind, priority, damaging_z)
            {
                if guard.checks_protect()
                    && (damaging_z && !move_data.flags.contains_key("protect")
                        || matches!(self.event_ability(source), "unseenfist" | "piercingdrill")
                            && contact)
                {
                    bypassed |= mut_pos.mask();
                } else {
                    protected += 1;
                    let move_label = if guard == SideGuard::MatBlock {
                        format!("|{}", move_data.name)
                    } else {
                        String::new()
                    };
                    self.add(format!(
                        "|-activate|{}|move: {}{move_label}",
                        self.ident(mut_pos),
                        guard.name()
                    ));
                    self.stop_protected_rampage(source);
                    return false;
                }
            }
            if self.terrain() == Some(Terrain::Psychic)
                && priority > 0
                && mut_pos.player != source.player
                && !self.mon(mut_pos).semi_invulnerable()
            {
                if self.grounded(mut_pos) {
                    self.terrain_activate(mut_pos, Terrain::Psychic);
                    return false;
                } else if move_data.priority > 0 {
                    self.add("|-hint|Psychic Terrain doesn't affect airborne Pokémon.");
                }
            }
            if let Some(protect) = self.mon(mut_pos).protection
                && protect != Protection::Endure
                && (protect.blocks_status() || move_data.category != Category::Status)
                && (damaging_z || move_data.flags.contains_key("protect"))
            {
                if damaging_z && !move_data.flags.contains_key("protect")
                    || matches!(self.event_ability(source), "unseenfist" | "piercingdrill")
                        && contact
                {
                    bypassed |= mut_pos.mask();
                } else {
                    // Personal guards return NOT_FAIL.
                    protected += 1;
                    if smart_target {
                        smart_target = false;
                    } else {
                        self.add(format!("|-activate|{}|move: Protect", self.ident(mut_pos)));
                    }
                    self.stop_protected_rampage(source);
                    if contact {
                        self.protection_contact(protect, source, mut_pos, base);
                    }
                    return false;
                }
            }
            if !reflected
                && source != mut_pos
                && move_data.flags.contains_key("reflectable")
                && !self.mon(mut_pos).semi_invulnerable()
                && self.event_ability(mut_pos) == "magicbounce"
            {
                self.bounce_move(mut_pos, source, move_data, queue);
                false
            } else if ohko && self.event_ability(mut_pos) == "sturdy" {
                self.add(format!(
                    "|-immune|{}|[from] ability: Sturdy",
                    self.ident(mut_pos)
                ));
                false
            } else if source != mut_pos
                && move_data.flags.contains_key("powder")
                && !self.mon(mut_pos).has_type("Grass")
                && self.event_ability(mut_pos) == "overcoat"
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Overcoat",
                    self.ident(mut_pos)
                ));
                false
            } else if source != mut_pos
                && move_data.flags.contains_key("powder")
                && !self.mon(mut_pos).has_type("Grass")
                && self.mon(mut_pos).effective_item() == "safetygoggles"
            {
                self.add(format!(
                    "|-activate|{}|item: Safety Goggles|{}",
                    self.ident(mut_pos),
                    move_data.name
                ));
                false
            } else if source != mut_pos
                && move_type == "Fire"
                && self.event_ability(mut_pos) == "flashfire"
            {
                accuracy = Accuracy::Bypass(true);
                self.flash_fire(mut_pos);
                false
            } else if self.absorb_elemental(mut_pos, source, move_type) {
                false
            } else if self.event_ability(mut_pos) == "oblivious"
                && matches!(move_data.name.as_str(), "Taunt" | "Attract")
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Oblivious",
                    self.ident(mut_pos)
                ));
                false
            } else if source != mut_pos
                && move_data.flags.contains_key("sound")
                && self.event_ability(mut_pos) == "soundproof"
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Soundproof",
                    self.ident(mut_pos)
                ));
                false
            } else if move_data.flags.contains_key("bullet")
                && self.event_ability(mut_pos) == "bulletproof"
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Bulletproof",
                    self.ident(mut_pos)
                ));
                false
            } else if source != mut_pos
                && source.player == mut_pos.player
                && move_data.category != Category::Status
                && self.event_ability(mut_pos) == "telepathy"
            {
                self.add(format!(
                    "|-activate|{}|ability: Telepathy",
                    self.ident(mut_pos)
                ));
                false
            } else if move_data.category == Category::Status
                && source != mut_pos
                && self.event_ability(mut_pos) == "goodasgold"
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Good as Gold",
                    self.ident(mut_pos)
                ));
                false
            } else {
                true
            }
        });
        failed_hit |= spread_targets.len() + protected != potential;
        smart_target &= spread_targets.len() > 1;
        let ignores_type = !delayed && self.move_ignores_immunity(source, move_data, move_type);
        let potential = spread_targets.len();
        spread_targets.retain(|&pos| {
            if move_data.name == "Captivate"
                && !matches!(
                    (self.mon(source).gender, self.mon(pos).gender),
                    (b'F', b'M') | (b'M', b'F')
                )
            {
                self.add(format!("|-immune|{}", self.ident(pos)));
                false
            } else if !ignores_type
                && move_type == "Ground"
                && !self.mon(pos).has_type("Flying")
                && let Some(label) = self.airborne_ability(pos)
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: {label}",
                    self.ident(pos)
                ));
                false
            } else if (!ignores_type
                && (self
                    .effectiveness_of(move_data, move_type, pos, false)
                    .is_none()
                    || (move_type == "Ground" && !self.grounded(pos))))
                || (move_data.flags.contains_key("powder")
                    && source != pos
                    && self.status_immune(pos, "powder"))
                || (move_data.name == "Leech Seed" && self.mon(pos).has_type("Grass"))
                || (move_data.name == "Dream Eater"
                    && !matches!(self.mon(pos).status, Status::Sleep(_))
                    && self.event_ability(pos) != "comatose")
                || (move_data.name == "Synchronoise"
                    && !self
                        .mon(source)
                        .types()
                        .filter(|&t| t != "???")
                        .any(|t| self.mon(pos).has_type(t)))
                || (move_data.name == "Endeavor" && self.mon(source).hp >= self.mon(pos).hp)
                || (matches!(move_data.name.as_str(), "Trick" | "Switcheroo")
                    && self.event_ability(pos) == "stickyhold")
            {
                if !smart_target {
                    self.add(format!("|-immune|{}", self.ident(pos)));
                }
                false
            } else {
                true
            }
        });
        smart_target &= spread_targets.len() > 1;
        if trapping_status
            || matches!(
                move_data.name.as_str(),
                "Octolock" | "Worry Seed" | "Attract"
            )
        {
            spread_targets.retain(|&target| {
                if if trapping_status {
                    self.mon(target).has_type("Ghost")
                } else if move_data.name == "Octolock" {
                    self.status_immune(target, "trapped")
                } else if move_data.name == "Attract" {
                    !self.compatible_attraction(source, target)
                } else {
                    matches!(self.mon(target).active_ability(), "insomnia" | "truant")
                } {
                    self.add(format!("|-immune|{}", self.ident(target)));
                    false
                } else {
                    true
                }
            });
        }
        if prankster && self.event_ability(source) == "prankster" {
            spread_targets.retain(|&pos| {
                if pos.player == source.player || !self.status_immune(pos, "prankster") {
                    return true;
                }
                if self.format.report_exact_hp {
                    self.add("|debug|natural prankster immunity");
                }
                if self.mon(pos).illusion.is_some()
                    || move_data.status.as_deref().is_none_or(|status| {
                        !self.status_immune(pos, if status == "tox" { "psn" } else { status })
                    })
                {
                    self.add("|-hint|Since gen 7, Dark is immune to Prankster moves.");
                }
                self.add(format!("|-immune|{}", self.ident(pos)));
                false
            });
        }
        spread_targets.retain(|&pos| {
            if ohko
                && !self.mon(pos).semi_invulnerable()
                && (self.mon(source).level < self.mon(pos).level
                    || (sheer_cold && self.mon(pos).has_type("Ice")))
            {
                self.add(format!("|-immune|{}|[ohko]", self.ident(pos)));
                return false;
            }
            let numeric_accuracy = matches!(accuracy, Accuracy::Percent(_));
            let zoom = self.zoom_lens_active(source, pos, queue);
            if !ohko {
                self.prepare_accuracy_modifiers(source, pos, numeric_accuracy, zoom);
            }
            if numeric_accuracy && !ohko {
                self.sort_unaware_handlers();
                if !self.move_ignores_evasion(source, move_data) {
                    self.sort_unaware_handlers();
                }
            }
            if let Accuracy::Percent(accuracy) = accuracy
                && !move_data.always_hit
                && !(move_data.target == TargetKind::User && move_data.category == Category::Status)
                && !(move_data.name == "Toxic" && self.mon(source).has_type("Poison"))
                && self.mon(source).active_ability() != "noguard"
                && self.mon(pos).active_ability() != "noguard"
                && !self.mon(pos).glaive_drawback
                && !self.mon(pos).trampled_by(move_data)
                && !self.locked_on_to(source, pos)
                && (ohko || !self.mon(pos).under(LegacyCondition::Telekinesis))
                && !self.rng.chance(
                    if ohko && !self.mon(pos).semi_invulnerable() {
                        let base = if sheer_cold && !self.mon(source).has_type("Ice") {
                            20
                        } else {
                            30
                        };
                        base + u32::from(self.mon(source).level - self.mon(pos).level)
                    } else {
                        self.accuracy(source, pos, move_data, accuracy, zoom)
                    },
                    100,
                )
            {
                if smart_target {
                    smart_target = false;
                    return false;
                }
                if !spread {
                    self.log[base].public.push_str("|[miss]");
                }
                self.add(format!("|-miss|{source_name}|{}", self.ident(pos)));
                false
            } else {
                true
            }
        });
        if spread_targets.is_empty() {
            result.counts_as_success = false;
            result.failed = failed_hit || potential != 0;
            if spread {
                self.log[base].public.push_str("|[spread] ");
            }
            if matches!(
                move_data.name.as_str(),
                "Supercell Slam" | "High Jump Kick" | "Jump Kick" | "Axe Kick"
            ) {
                let original_hp = self.mon(source).hp;
                self.indirect_damage(source, 2, &format!("|[from] {}", to_id(&move_data.name)));
                self.emergency_exit(source, original_hp);
                if self.mon(source).hp == 0 {
                    self.mark_fainted(source);
                }
            }
            if sacrifice {
                self.mark_fainted(source);
            } else if let Some(recoil) = half_recoil
                && move_data.name != "Chloroblast"
            {
                self.half_max_hp_recoil(source, recoil);
                if self.mon(source).hp == 0 {
                    self.mark_fainted(source);
                }
            }
            if self.mon(source).hp == 0 && !self.mon(source).fainted {
                self.mark_fainted(source);
            }
            self.note_consecutive_move(consecutive, move_data);
            return true;
        }
        if matches!(
            move_data.name.as_str(),
            "Feint" | "Phantom Force" | "Shadow Force" | "Hyperspace Hole" | "Hyperspace Fury"
        ) {
            for &target in &spread_targets {
                let lost_side_guard = self.clear_side_guards(target.player);
                let mon = self.mon_mut(target);
                let lost_protection = mon.protection.take();
                if let Some(protection) = lost_protection {
                    mon.untrack_volatile(protection.volatile());
                }
                if lost_protection.is_some() || lost_side_guard {
                    mon.stall = None;
                    mon.untrack_volatile("stall");
                    let attr = if move_data.name == "Feint" {
                        ""
                    } else {
                        "|[broken]"
                    };
                    self.add(format!(
                        "|-activate|{}|move: {}{attr}",
                        self.ident(target),
                        move_data.name
                    ));
                }
            }
        }
        if move_data.name == "Spectral Thief" {
            self.steal_positive_stages(source, spread_targets[0], base);
        }

        if move_data.category == Category::Status {
            let ability_replacement = matches!(
                move_data.name.as_str(),
                "Worry Seed" | "Simple Beam" | "Entrainment"
            );
            spread_targets.retain(|&target| {
                if move_data.name == "Gastro Acid"
                    && !DEX
                        .ability(&self.mon(target).ability)
                        .is_some_and(|a| a.cannot_suppress)
                    && self.mon(target).effective_item() == "abilityshield"
                {
                    self.add(format!(
                        "|-block|{}|item: Ability Shield",
                        self.ident(target)
                    ));
                    return false;
                }
                if (!ability_replacement
                    || self
                        .replacement_ability(source, target, &move_data.name)
                        .is_some())
                    && (move_data.name != "Role Play" || self.can_role_play(source, target))
                    && (move_data.name != "Gastro Acid"
                        || !DEX
                            .ability(&self.mon(target).ability)
                            .is_some_and(|a| a.cannot_suppress))
                    && (move_data.name != "Electrify"
                        || self.mon(target).active_turns == 0
                        || Self::queued_move(queue, target).is_some())
                    && !self.behind_substitute(source, target, move_data)
                {
                    return true;
                }
                self.add(format!("|-fail|{source_name}"));
                self.move_still(base);
                false
            });
            let mut any_success = false;
            let mut hit_labels = Vec::new();
            let life_dew = move_data.name == "Life Dew";
            let venom_drench = move_data.name == "Venom Drench";
            let fillet_away = move_data.name == "Fillet Away";
            let group_recovery =
                matches!(move_data.name.as_str(), "Jungle Healing" | "Lunar Blessing");
            let teeter_dance = move_data.name == "Teeter Dance";
            let memento = move_data.name == "Memento";
            let list_all_targets = life_dew || teeter_dance || group_recovery || venom_drench;
            let mut confusion_failed = false;
            for target in spread_targets {
                if move_data.name == "Autotomize"
                    && self.mon(target).boosts.get(BoostStat::Spe)
                        == if self.event_ability(target) == "contrary" {
                            -6
                        } else {
                            6
                        }
                {
                    self.move_still(base);
                    self.add(format!("|-fail|{source_name}"));
                    continue;
                }
                if move_data.name == "Copycat" {
                    if !history_known {
                        self.unsupported_called_move =
                            Some(("Copycat", "previous battle move is not observed"));
                        return true;
                    }
                    let Some(called) = previous else {
                        any_success = true;
                        continue;
                    };
                    if previous_z || called.flags.contains_key("failcopycat") {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        result.failed = true;
                        return true;
                    }
                    let id = to_id(&called.name);
                    let extra = MoveSlot {
                        id,
                        move_data: called,
                        pp: 0,
                        max_pp: 0,
                    };
                    if !Self::move_supported(&extra) {
                        self.unsupported_called_move = Some(("Copycat called", &called.name));
                        return true;
                    }
                    let user = self.mon_mut(source);
                    let called_slot = user
                        .moves
                        .iter()
                        .position(|m| m.id == extra.id)
                        .unwrap_or_else(|| {
                            user.extra_move = Some(Box::new(extra));
                            EXTRA_MOVE_SLOT
                        });
                    let (_, source) = self.run_move(
                        source,
                        MoveInvocation::Copycat {
                            slot: called_slot,
                            caller: slot.unwrap(),
                            priority,
                            prankster,
                        },
                        queue,
                    );
                    result.failed = self.mon(source).move_failed_this_turn;
                    self.each_event();
                    self.each_event();
                    return true;
                }
                if ability_replacement {
                    any_success |= self.replace_ability_from_move(source, target, &move_data.name);
                    continue;
                }
                if move_data.name == "Role Play" {
                    any_success |= self.role_play(source, target);
                    continue;
                }
                let mut succeeded = false;
                match move_data.name.as_str() {
                    "Celebrate" => {
                        self.add(format!("|-activate|{source_name}|move: Celebrate"));
                        succeeded = true;
                    }
                    "Splash" => {
                        self.add("|-nothing");
                        succeeded = true;
                    }
                    "Hold Hands" => succeeded = true,
                    "Fillet Away" => {
                        let user = self.mon(source);
                        if user.hp * 2 <= user.max_hp.get() || user.max_hp.get() == 1 {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                        } else if self.boost(source, &move_data.boosts, true) {
                            let user = self.mon_mut(source);
                            user.hp -= user.max_hp.get() / 2;
                            self.add_health_line("-damage", source, false);
                            succeeded = true;
                        }
                    }
                    "Venom Drench"
                        if matches!(self.mon(target).status, Status::Poison | Status::Toxic(_)) =>
                    {
                        succeeded = self.boost_from(
                            target,
                            source,
                            &[
                                (BoostStat::Atk, -1),
                                (BoostStat::Spa, -1),
                                (BoostStat::Spe, -1),
                            ],
                            BoostOrigin::from_move(move_data),
                        );
                    }
                    "Refresh" => {
                        if matches!(
                            self.mon(target).status,
                            Status::Burn | Status::Paralysis | Status::Poison | Status::Toxic(_)
                        ) {
                            self.cure_status(target, "[msg]");
                            succeeded = true;
                        } else {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                            if self.format.report_exact_hp {
                                self.add("|debug|move failed because it did nothing");
                            }
                        }
                    }
                    "Take Heart" => {
                        succeeded =
                            self.boost(target, &[(BoostStat::Spa, 1), (BoostStat::Spd, 1)], true);
                        if self.mon(target).status != Status::Healthy {
                            self.cure_status(target, "[msg]");
                            succeeded = true;
                        }
                        if !succeeded {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                            self.report_no_move_effect();
                        }
                    }
                    "Laser Focus" => {
                        self.start_laser_focus(target, false);
                        succeeded = true;
                    }
                    "Heal Block" => {
                        if self.mon(target).heal_block > 0 {
                            self.add(format!("|-fail|{}|move: Heal Block", self.ident(target)));
                        } else {
                            succeeded = self.start_heal_block(target, 5);
                        }
                    }
                    "Spotlight" if self.slots() == 1 => {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                    "Telekinesis" | "Embargo" | "Powder" | "Spotlight" => {
                        let condition = LegacyCondition::for_move(&move_data.name);
                        match self.start_legacy_condition(target, condition) {
                            Some(true) => succeeded = true,
                            Some(false) => {
                                self.move_still(base);
                                self.add(format!("|-fail|{source_name}"));
                                self.report_no_move_effect();
                            }
                            None => {}
                        }
                    }
                    "Psycho Shift" => {
                        let status = self.mon(source).status.id();
                        if status.is_empty() {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                            self.report_no_move_effect();
                        } else if self.try_status(target, status, source, move_data, true, base) {
                            self.cure_status(source, "[msg]");
                            succeeded = true;
                        } else {
                            self.report_no_move_effect();
                        }
                    }
                    "Purify" => {
                        if self.mon(target).status == Status::Healthy {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                        } else {
                            self.cure_status(target, "[msg]");
                            let amount = self.mon(source).max_hp.get().div_ceil(2);
                            self.restore_from_move(source, amount, z_powered);
                        }
                        succeeded = true;
                    }
                    "Camouflage" | "Conversion" => {
                        succeeded =
                            self.set_camouflage_type(target, move_data.name == "Conversion");
                        if !succeeded {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                            self.report_no_move_effect();
                        }
                    }
                    "Foresight" | "Odor Sleuth" | "Miracle Eye" => {
                        succeeded = self.mark_identified(target, move_data.name == "Miracle Eye");
                        if !succeeded {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                        }
                    }
                    _ => {}
                }
                if move_data.name == "Gastro Acid" {
                    if self.inhibit_ability(target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                if move_data.name == "Reflect Type" {
                    if self.reflect_type(source, target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                if move_data.name == "Electrify" {
                    let mon = self.mon_mut(target);
                    if mon.electrified {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        mon.electrified = true;
                        mon.track_volatile("electrify");
                        self.add(format!(
                            "|-singleturn|{}|move: Electrify",
                            self.ident(target)
                        ));
                        succeeded = true;
                    }
                }
                succeeded |= self.apply_stat_move(source, target, &move_data.name);
                if move_data.name == "Topsy-Turvy" && !succeeded {
                    self.move_still(base);
                    self.add(format!("|-fail|{source_name}"));
                    if self.format.report_exact_hp {
                        self.add("|debug|move failed because it did nothing");
                    }
                }
                if move_data.name == "Acupressure" {
                    let boosts = self.mon(target).boosts;
                    let mut eligible = BoostStat::ALL;
                    let mut count = 0;
                    for stat in BoostStat::ALL {
                        if boosts.get(stat) < 6 {
                            eligible[count] = stat;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    } else {
                        let stat = eligible[self.rng.below(count as u32) as usize];
                        self.boost_from(
                            target,
                            source,
                            &[(stat, 2)],
                            BoostOrigin::from_move(move_data),
                        );
                        succeeded = true;
                    }
                }
                if spread && list_all_targets {
                    hit_labels.push(target.label());
                }
                if matches!(
                    move_data.name.as_str(),
                    "Taunt" | "Encore" | "Disable" | "Torment" | "Attract"
                ) && self.aroma_veil_blocks(target, true)
                {
                    continue;
                }
                if move_data.name == "Helping Hand" {
                    if self.mon(target).active_turns != 0
                        && Self::queued_move(queue, target).is_none()
                    {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        let mon = self.mon_mut(target);
                        mon.helping_hand += 1;
                        mon.track_volatile("helpinghand");
                        self.add(format!(
                            "|-singleturn|{}|Helping Hand|[of] {source_name}",
                            self.ident(target)
                        ));
                        succeeded = true;
                    }
                } else if move_data.name == "Focus Energy" {
                    if self.mon(target).volatile_ids().contains(&"focusenergy")
                        || self.mon(target).dragon_cheer != 0
                    {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        self.mon_mut(target).track_volatile("focusenergy");
                        self.add(format!("|-start|{}|move: Focus Energy", self.ident(target)));
                        succeeded = true;
                    }
                } else if move_data.name == "Dragon Cheer" {
                    if self.start_dragon_cheer(target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                let confusing_boost = matches!(move_data.name.as_str(), "Swagger" | "Flatter");
                if confusing_boost {
                    succeeded |= self.boost_from(
                        target,
                        source,
                        &move_data.boosts,
                        BoostOrigin::from_move(move_data),
                    );
                }
                if confusing_boost
                    || matches!(
                        move_data.name.as_str(),
                        "Confuse Ray" | "Sweet Kiss" | "Supersonic" | "Teeter Dance"
                    )
                {
                    if self.mon(target).confusion > 0 {
                        if teeter_dance {
                            confusion_failed = true;
                        } else if !succeeded {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                        }
                    } else if self.event_ability(target) == "owntempo" {
                        self.add(format!(
                            "|-immune|{}|confusion|[from] ability: Own Tempo",
                            self.ident(target)
                        ));
                    } else if self.status_blocked_by_terrain(target, "confusion")
                        == Some(Terrain::Misty)
                    {
                        self.terrain_activate(target, Terrain::Misty);
                    } else if !self.safeguard_blocks(target, source, true, true, true) {
                        succeeded |= self.apply_confusion(target, false);
                    }
                }
                if move_data.name == "Aqua Ring" {
                    if self.mon(target).volatile_ids().contains(&"aquaring") {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        self.mon_mut(target).track_volatile("aquaring");
                        self.add(format!("|-start|{}|Aqua Ring", self.ident(target)));
                        succeeded = true;
                    }
                }
                if move_data.name == "Ingrain" {
                    if self.mon(target).volatile_ids().contains(&"ingrain") {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        self.mon_mut(target).track_volatile("ingrain");
                        self.add(format!("|-start|{}|move: Ingrain", self.ident(target)));
                        succeeded = true;
                    }
                }
                if move_data.name == "Magnet Rise" {
                    if self.mon(target).magnet_rise != 0 {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    } else {
                        let mon = self.mon_mut(target);
                        mon.magnet_rise = 5;
                        mon.track_volatile("magnetrise");
                        self.add(format!("|-start|{}|Magnet Rise", self.ident(target)));
                        succeeded = true;
                    }
                }
                if trapping_status {
                    if self.start_linked_trap(source, target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                if move_data.name == "Strength Sap" {
                    let mon = self.mon(target);
                    let stage = mon.boosts.get(BoostStat::Atk);
                    if stage != -6 {
                        let amount = boosted_stat(mon.stats.atk, stage);
                        succeeded = self.boost_from(
                            target,
                            source,
                            &[(BoostStat::Atk, -1)],
                            BoostOrigin::from_move(move_data),
                        );
                        succeeded |=
                            self.transfer_health(source, target, amount, DrainKind::StrengthSap);
                    }
                    if !succeeded {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Torment" || move_data.name == "Octolock" {
                    let started = if move_data.name == "Torment" {
                        self.start_torment(target)
                    } else {
                        self.start_octolock(source, target)
                    };
                    if started {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Instruct" {
                    if self.instruct(source, target, queue) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Belly Drum" {
                    let mon = self.mon(target);
                    let cost = mon.max_hp.get() / 2;
                    if cost == 0 || mon.hp <= cost || mon.boosts.get(BoostStat::Atk) >= 6 {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        // Direct HP loss skips Damage events. Update can
                        // still notice a pinch berry afterwards.
                        let delta = if self.event_ability(target) == "contrary" {
                            -12
                        } else {
                            12
                        };
                        self.mon_mut(target).hp -= cost;
                        self.add_health_line("-damage", target, false);
                        let mon = self.mon_mut(target);
                        let changed = mon.boosts.add(BoostStat::Atk, delta);
                        mon.stats_raised |= changed > 0;
                        mon.stats_lowered |= changed < 0;
                        let stage = mon.boosts.get(BoostStat::Atk);
                        if changed != 0 {
                            self.add(format!(
                                "|-setboost|{}|atk|{stage}|[from] move: Belly Drum",
                                self.ident(target)
                            ));
                        }
                        self.note_foe_boost(target, BoostStat::Atk, changed);
                        succeeded = true;
                    }
                }
                if move_data.name == "Quash" {
                    if self.slots() > 1
                        && let Some(action) = queue.iter_mut().find(|action| {
                            action.actor == Some(target)
                                && matches!(action.kind, ActionKind::Move { .. })
                        })
                    {
                        action.priority.order = 201;
                        self.add(format!("|-activate|{}|move: Quash", self.ident(target)));
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if matches!(move_data.name.as_str(), "Soak" | "Magic Powder") {
                    let ty = if move_data.name == "Soak" {
                        "Water"
                    } else {
                        "Psychic"
                    };
                    let mon = self.mon(target);
                    if mon.types().eq([ty])
                        || matches!(mon.species.base_species.as_str(), "Arceus" | "Silvally")
                    {
                        if move_data.name == "Magic Powder" {
                            self.move_still(base);
                            self.add(format!("|-fail|{source_name}"));
                        } else {
                            self.add(format!("|-fail|{}", self.ident(target)));
                        }
                        self.report_no_move_effect();
                    } else {
                        let mon = self.mon_mut(target);
                        mon.set_temporary_type(Some(ty));
                        self.add(format!("|-start|{}|typechange|{ty}", self.ident(target)));
                        succeeded = true;
                    }
                }
                if matches!(move_data.name.as_str(), "Trick-or-Treat" | "Forest's Curse") {
                    let ty = if move_data.name == "Trick-or-Treat" {
                        "Ghost"
                    } else {
                        "Grass"
                    };
                    if self.mon(target).has_type(ty) {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        self.mon_mut(target).added_type = Some(ty);
                        self.add(format!(
                            "|-start|{}|typeadd|{ty}|[from] move: {}",
                            self.ident(target),
                            move_data.name
                        ));
                        if ty == "Ghost" && self.slots() == 2 && target.slot == 1 {
                            // When the second allied slot gains Ghost before
                            // Curse resolves, upstream rewrites its target.
                            for action in queue.iter_mut() {
                                if action.actor == Some(target)
                                    && let ActionKind::Move {
                                        move_slot,
                                        target: loc,
                                        ..
                                    } = &mut action.kind
                                    && *move_slot != RECHARGE_SLOT
                                    && self.move_data(target, *move_slot).name == "Curse"
                                {
                                    // The queue's resolved self target remains
                                    // self even when upstream changes its loc.
                                    if *loc != target.loc(target) {
                                        *loc = -1;
                                    }
                                    break;
                                }
                            }
                        }
                        succeeded = true;
                    }
                }
                if matches!(move_data.name.as_str(), "Whirlwind" | "Roar") {
                    if self.schedule_drag(source, target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Wish" {
                    if self.start_wish(source) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        self.report_no_move_effect();
                    }
                }
                if move_data.name == "Recycle" {
                    if self.recycle_item(target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Transform" {
                    if self.transform_into(source, target, false) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if curse {
                    let ghost = self.mon(source).has_type("Ghost");
                    if self.format.report_exact_hp {
                        self.add(format!(
                            "|debug|Curse onTryHit triggered|{}|{source_name}",
                            self.ident(target)
                        ));
                    }
                    if !ghost || !self.mon(target).volatile_ids().contains(&"curse") {
                        if self.format.report_exact_hp {
                            self.add(format!(
                                "|debug|Curse onHit triggered|{source_name}|{}",
                                self.ident(target)
                            ));
                        }
                        succeeded = if ghost {
                            self.apply_curse(source, target)
                        } else {
                            self.boost(
                                source,
                                &[
                                    (BoostStat::Spe, -1),
                                    (BoostStat::Atk, 1),
                                    (BoostStat::Def, 1),
                                ],
                                false,
                            )
                        };
                    }
                    if !succeeded {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Defog" {
                    if self.defog(source, target, move_data) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        self.report_no_move_effect();
                    }
                }
                if matches!(
                    move_data.name.as_str(),
                    "Skill Swap" | "Trick" | "Switcheroo"
                ) {
                    let swapped = if move_data.name == "Skill Swap" {
                        self.skill_swap(source, target)
                    } else {
                        Some(self.swap_items(source, target, &move_data.name))
                    };
                    if swapped == Some(true) {
                        succeeded = true;
                    } else if swapped == Some(false) {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Leech Seed" {
                    if self.leech_seed(source, target) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        self.report_no_move_effect();
                    }
                }
                if move_data.name == "Tidy Up" {
                    let removed = self.tidy_hazards(source);
                    let raised =
                        self.boost(source, &[(BoostStat::Atk, 1), (BoostStat::Spe, 1)], true);
                    if removed || raised {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Encore" {
                    let queued = Self::queued_move(queue, target);
                    if let Some(slot) = self.start_encore(target, queued.is_some()) {
                        if let Some(before) = queued
                            && before != slot
                            && self.mon(target).effective_item() != "mentalherb"
                        {
                            self.replace_queued_move(queue, target, slot);
                        }
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        self.report_no_move_effect();
                    }
                }
                if move_data.name == "Pain Split" {
                    let average = ((self.mon(source).hp + self.mon(target).hp) / 2).max(1);
                    let mon = self.mon_mut(target);
                    mon.hp = average.min(mon.max_hp.get());
                    self.add_health_line_with(
                        "-sethp",
                        target,
                        false,
                        "|[from] move: Pain Split|[silent]",
                    );
                    let mon = self.mon_mut(source);
                    mon.hp = average.min(mon.max_hp.get());
                    self.add_health_line_with("-sethp", source, false, "|[from] move: Pain Split");
                    succeeded = true;
                }
                if move_data.name == "Parting Shot" {
                    let lowered = self.boost_from(
                        target,
                        source,
                        &[(BoostStat::Atk, -1), (BoostStat::Spa, -1)],
                        BoostOrigin::from_move(move_data),
                    );
                    if (lowered || self.mon(target).active_ability() == "mirrorarmor")
                        && self.first_bench(source.player, 0).is_some()
                    {
                        self.mon_mut(source).pivot_move = Some(Pivot::Move("Parting Shot"));
                    }
                    succeeded = true;
                }
                if move_data.name == "Taunt" {
                    if self.taunt(target, Self::queued_move(queue, target).is_some()) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        self.report_no_move_effect();
                    }
                }
                if move_data.name == "Disable" {
                    if !self.disable_last_move(target, source, false) {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        succeeded = true;
                    }
                }
                if matches!(move_data.name.as_str(), "Attract" | "Spite") {
                    let applied = if move_data.name == "Attract" {
                        self.start_attraction(source, target, false)
                    } else {
                        self.reduce_last_move_pp(target, "Spite", 4)
                    };
                    if applied {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                if move_data.name == "Corrosive Gas" {
                    self.corrode_item(source, target);
                    succeeded = true;
                }
                if matches!(move_data.name.as_str(), "Lock-On" | "Mind Reader") {
                    if self.start_lock_on(source, target, &move_data.name) {
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "After You" {
                    let action = (self.slots() > 1)
                        .then(|| {
                            queue.iter().position(|action| {
                                action.actor == Some(target)
                                    && matches!(action.kind, ActionKind::Move { .. })
                            })
                        })
                        .flatten();
                    if let Some(index) = action {
                        Self::promote_move(queue, index, false);
                        self.add(format!("|-activate|{}|move: After You", self.ident(target)));
                        succeeded = true;
                    } else {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|move failed because it did nothing");
                        }
                    }
                }
                if move_data.name == "Stuff Cheeks"
                    && self.boost_from(
                        target,
                        source,
                        &[(BoostStat::Def, 2)],
                        BoostOrigin::from_move(move_data),
                    )
                {
                    self.force_eat_berry(target, source);
                    succeeded = true;
                }
                if move_data.name == "Yawn" {
                    let mon = self.mon(target);
                    let already_yawning = mon.yawn.is_some() && mon.status == Status::Healthy;
                    if mon.yawn.is_some()
                        || mon.status != Status::Healthy
                        || self.status_immune(target, "slp")
                    {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if already_yawning {
                            self.report_no_move_effect();
                        }
                        continue;
                    }
                    if self.yawn_blocked_by_ability(target) {
                        continue;
                    }
                    if let Some(holder) = self.flower_veil_holder(target) {
                        if self.format.report_exact_hp {
                            self.add("|debug|Flower Veil blocking yawn");
                        }
                        self.log_flower_veil(target, holder);
                        continue;
                    }
                    if self.status_blocked_by_terrain(target, "slp") == Some(Terrain::Electric) {
                        self.terrain_activate(target, Terrain::Electric);
                        continue;
                    }
                    if self.safeguard_blocks(target, source, true, true, true) {
                        continue;
                    }
                    let mon = self.mon_mut(target);
                    mon.yawn = Some(super::status::Yawn {
                        turns: 2,
                        source: source.player,
                    });
                    mon.track_volatile("yawn");
                    self.add(format!(
                        "|-start|{}|move: Yawn|[of] {source_name}",
                        self.ident(target)
                    ));
                    succeeded = true;
                }
                if matches!(
                    move_data.name.as_str(),
                    "Moonlight" | "Morning Sun" | "Synthesis" | "Shore Up" | "Floral Healing"
                ) {
                    let weather_healing =
                        !matches!(move_data.name.as_str(), "Shore Up" | "Floral Healing");
                    if weather_healing && mega_sun && weather != Some(Weather::Sun) {
                        self.add(format!(
                            "|-activate|{}|ability: Mega Sol",
                            self.ident(target)
                        ));
                    }
                    let tr = self.stat_rules.truncation;
                    let max_hp = self.mon(target).max_hp.get();
                    let amount = match move_data.name.as_str() {
                        "Shore Up" => tr.modify(
                            max_hp,
                            if weather == Some(Weather::Sand) {
                                2732
                            } else {
                                2048
                            },
                        ),
                        "Floral Healing" if self.terrain() == Some(Terrain::Grassy) => {
                            tr.modify(max_hp, 2732)
                        }
                        "Floral Healing" => max_hp.div_ceil(2),
                        _ => tr.modify(
                            max_hp,
                            match move_weather.map(Weather::ordinary) {
                                Some(Weather::Sun) => 2732,
                                None | Some(Weather::StrongWinds) => 2048,
                                Some(_) => 1024,
                            },
                        ),
                    };
                    if !self.restore_from_move(
                        target,
                        amount.max(u32::from(weather_healing)),
                        z_powered,
                    ) {
                        self.add(format!("|-fail|{}|heal", self.ident(target)));
                    }
                    // Weather-based recovery uses NOT_FAIL at full health.
                    // Update events still run and can have RNG-consuming ties.
                    succeeded = true;
                }
                if group_recovery {
                    let amount = self
                        .stat_rules
                        .truncation
                        .modify(self.mon(target).max_hp.get(), 1024);
                    succeeded = self.restore_from_move(target, amount, z_powered);
                    if self.mon(target).status != Status::Healthy {
                        self.cure_status(target, "[msg]");
                        succeeded = true;
                    }
                }
                if let Some(status) = &move_data.status {
                    if self.try_status(target, status, source, move_data, true, base) {
                        succeeded = true;
                    } else {
                        self.report_no_move_effect();
                    }
                }
                if move_data.name == "Heal Pulse" {
                    self.heal_pulse(source, target);
                    // Heal Pulse returns NOT_FAIL when the target is full.
                    succeeded = true;
                }
                if let Some([num, den]) = move_data.heal {
                    let mon = self.mon_mut(target);
                    if mon.hp == mon.max_hp.get() {
                        self.move_still(base);
                        self.add(format!("|-fail|{}|heal", self.ident(target)));
                        if !move_data.self_effect && !life_dew {
                            self.report_no_move_effect();
                        }
                        continue;
                    }
                    if mon.heal_block > 0 && !z_powered {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                        if self.format.report_exact_hp {
                            self.add("|debug|heal interrupted");
                        }
                        continue;
                    }
                    let amount =
                        (mon.max_hp.get() * u32::from(num) + u32::from(den) / 2) / u32::from(den);
                    mon.hp = (mon.hp + amount).min(mon.max_hp.get());
                    self.add_health_line("-heal", target, false);
                    if move_data.name == "Roost" {
                        let mon = self.mon_mut(target);
                        mon.roosted = true;
                        mon.track_volatile("roost");
                        self.add(format!("|-singleturn|{}|move: Roost", self.ident(target)));
                    }
                    succeeded = true;
                }
                let boosts = if move_data.name == "Growth"
                    && move_weather.map(Weather::ordinary) == Some(Weather::Sun)
                {
                    &[(BoostStat::Atk, 2), (BoostStat::Spa, 2)]
                } else {
                    move_data.boosts.as_slice()
                };
                if !boosts.is_empty() && !confusing_boost && !fillet_away {
                    if self.boost_from(target, source, boosts, BoostOrigin::from_move(move_data)) {
                        succeeded = true;
                    } else if !succeeded && !move_data.self_effect {
                        self.report_no_move_effect();
                    }
                }
                if memento {
                    self.mon_mut(source).hp = 0;
                    succeeded = true;
                }
                if move_data.name == "Tar Shot"
                    && !self.mon(target).volatile_ids().contains(&"tarshot")
                {
                    self.mon_mut(target).track_volatile("tarshot");
                    self.add(format!("|-start|{}|Tar Shot", self.ident(target)));
                    succeeded = true;
                }
                if move_data.name == "Defense Curl"
                    && !self.mon(target).volatile_ids().contains(&"defensecurl")
                {
                    self.mon_mut(target).track_volatile("defensecurl");
                    succeeded = true;
                }
                if move_data.name == "Autotomize" && self.mon(target).weighthg > 1 {
                    let mon = self.mon_mut(target);
                    mon.weighthg = mon.weighthg.saturating_sub(1000).max(1);
                    self.add(format!("|-start|{}|Autotomize", self.ident(target)));
                    succeeded = true;
                }
                if move_data.name == "Minimize"
                    && !self.mon(target).volatile_ids().contains(&"minimize")
                {
                    self.mon_mut(target).track_volatile("minimize");
                    succeeded = true;
                }
                if move_data.name == "No Retreat" {
                    let mon = self.mon_mut(target);
                    if !mon.volatile_ids().contains(&"trapped") {
                        mon.track_volatile("noretreat");
                        mon.track_volatile("trapped");
                        self.add(format!("|-activate|{}|trapped", self.ident(target)));
                        self.add(format!("|-start|{}|move: No Retreat", self.ident(target)));
                        succeeded = true;
                    } else if !succeeded {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Stockpile" {
                    self.build_stockpile(target);
                    succeeded = true;
                }
                if move_data.name == "Swallow" {
                    self.swallow_stockpile(target);
                    succeeded = true;
                }
                if move_data.name == "Charge" {
                    self.charge_electric(target, None);
                    succeeded = true;
                }
                self.debug_suppressed_hook(target, "onHit");
                if succeeded {
                    any_success = true;
                    if spread && !list_all_targets {
                        hit_labels.push(target.label());
                    }
                }
            }
            if teeter_dance && !any_success && confusion_failed {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                if self.format.report_exact_hp {
                    self.add("|debug|move failed because it did nothing");
                }
            }
            if (group_recovery || venom_drench) && !any_success {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                self.report_no_move_effect();
            }
            if life_dew && !any_success {
                self.report_no_move_effect();
            }
            if spread {
                if list_all_targets && !any_success {
                    hit_labels.clear();
                }
                self.log[base]
                    .public
                    .push_str(&format!("|[spread] {}", hit_labels.join(",")));
            }
            if any_success {
                self.each_event();
                if (curse || memento) && self.mon(source).hp == 0 {
                    self.mark_fainted(source);
                }
                self.each_event();
            } else {
                result.failed = true;
            }
            return true;
        }

        if move_data.name == "Pollen Puff" && spread_targets[0].player == source.player {
            let target = spread_targets[0];
            if self.mon(source).heal_block > 0 {
                self.move_still(base);
                self.add(format!("|cant|{source_name}|move: Heal Block|Pollen Puff"));
                result.failed = true;
                return true;
            }
            let mon = self.mon_mut(target);
            if mon.hp < mon.max_hp.get() {
                if mon.heal_block > 0 {
                    self.move_still(base);
                    self.add(format!("|cant|{source_name}|move: Heal Block|Pollen Puff"));
                } else {
                    mon.hp = (mon.hp + (mon.max_hp.get() / 2).max(1)).min(mon.max_hp.get());
                    self.add_health_line("-heal", target, false);
                }
            }
            self.each_event();
            self.each_event();
            let original_hp = self.mon(source).hp;
            self.pay_life_orb_recoil(source);
            self.emergency_exit(source, original_hp);
            return true;
        }

        if present_heal {
            let target = spread_targets[0];
            let amount = self.mon(target).max_hp.get().div_ceil(4);
            if self.restore_from_move(target, amount, false) {
                self.each_event();
                self.each_event();
            } else {
                self.move_still(base);
                self.add(format!("|-fail|{}|heal", self.ident(target)));
                self.report_no_move_effect();
                result.failed = true;
            }
            return true;
        }

        if matches!(
            move_data.name.as_str(),
            "Raging Bull" | "Brick Break" | "Psychic Fangs"
        ) {
            self.break_screens(spread_targets[0].player);
        }
        if move_data.name == "Uproar" {
            self.wake_from_uproar(spread_targets[0]);
        }
        let screen_handlers = self
            .sides
            .iter()
            .flatten()
            .flat_map(|s| s.screens)
            .filter(|turns| *turns != 0)
            .count() as u32;
        let mut damage = Vec::with_capacity(spread_targets.len());
        let beat_up = (move_data.name == "Beat Up").then(|| {
            self.party(source.player)
                .unwrap()
                .iter()
                .enumerate()
                .filter(|&(slot, ally)| {
                    slot == source.slot || !ally.fainted && ally.status == Status::Healthy
                })
                .map(|(_, ally)| ally.submitted_species)
                .collect::<Vec<_>>()
        });
        let ash_shuriken = move_data.name == "Water Shuriken"
            && self.mon(source).species.name == "Greninja-Ash"
            && self.mon(source).transformed.is_none()
            && self.event_ability(source) == "battlebond";
        let parental = !delayed
            && !z_powered
            && !spread
            && move_data.multihit.is_none()
            && beat_up.is_none()
            && !move_data.flags.contains_key("noparentalbond")
            && !move_data.flags.contains_key("charge")
            && !move_data.flags.contains_key("futuremove")
            && self.event_ability(source) == "parentalbond";
        let multihit = move_data.multihit.is_some() || beat_up.is_some() || parental;
        let (max_hits, per_strike_accuracy) = if let Some(allies) = &beat_up {
            (allies.len() as u8, false)
        } else if ash_shuriken {
            (3, false)
        } else if parental {
            (2, false)
        } else {
            self.hits(source, move_data)
        };
        let mut total = 0;
        let mut landed_by_target = [0u32; 4];
        let mut hp_after_damage = [0u32; 4];
        let mut hits_landed = 0;
        let mut split_darts = [None; 2];
        let attack_hazard = if sheer_force_removed {
            None
        } else {
            Hazard::from_attack(&move_data.name)
        };
        let mut hit_substitute = false;
        let mut splash_faints = 0;
        let mut sparkling_aria_targets = 0;
        let binding = super::partial_trap::binding_move(&move_data.name);
        for hit in 1..=max_hits {
            if (self.mon(source).hp == 0 && !sacrifice && !smart_target && !delayed)
                || spread_targets
                    .iter()
                    .all(|&target| self.mon(target).hp == 0)
            {
                break;
            }
            if hit > 1
                && ((sleep_talk.is_none() && matches!(self.mon(source).status, Status::Sleep(_)))
                    || (per_strike_accuracy
                        && !self.next_strike_hits(
                            source,
                            spread_targets[0],
                            move_data,
                            accuracy,
                            self.zoom_lens_active(source, spread_targets[0], queue),
                        )))
            {
                break;
            }
            let strike_targets = if smart_target {
                &spread_targets[usize::from(hit - 1)..usize::from(hit)]
            } else {
                spread_targets.as_slice()
            };
            if !delayed
                && move_type == "Normal"
                && self.mon(source).item == "normalgem"
                && !move_data.flags.contains_key("pledgecombo")
                && strike_targets.iter().any(|&target| target != source)
            {
                self.consume_normal_gem(source, &move_data.name);
            }
            if darts {
                let target = self.ident(strike_targets[0]);
                if hit == 1 {
                    self.log[base].public =
                        format!("|move|{source_name}|Dragon Darts|{target}{called_from}");
                } else {
                    self.add(format!("|-anim|{source_name}|Dragon Darts|{target}"));
                }
            }
            let power = if matches!(move_data.name.as_str(), "Triple Axel" | "Triple Kick") {
                power * u16::from(hit)
            } else {
                power
            };
            damage.clear();
            let previous_strike_damage = total;
            let mut critical_hits = 0;
            let mut compute = |battle: &mut Self, target: Position, shield: bool| {
                if ohko {
                    return battle.mon(target).max_hp.get();
                }
                if final_gambit {
                    // The damage callback queues the user's faint before
                    // applying the target's damage, including to a Substitute.
                    return std::mem::take(&mut battle.mon_mut(source).hp);
                }
                if move_data.name == "Guardian of Alola" {
                    let mon = battle.mon(target);
                    let power = mon.hp * 3 / 4;
                    if matches!(
                        mon.protection,
                        Some(
                            Protection::Protect
                                | Protection::BanefulBunker
                                | Protection::KingsShield
                                | Protection::SpikyShield
                        )
                    ) {
                        battle.add(format!("|-zbroken|{}", battle.ident(target)));
                        return ((power + 1) / 4).max(1);
                    }
                    return power.max(1);
                }
                let fixed_damage = match move_data.name.as_str() {
                    "Metal Burst" | "Comeuppance" => {
                        retaliation_hit.map(|hit| (hit.damage * 3 / 2).max(1))
                    }
                    "Counter" | "Mirror Coat" => retaliation_hit.map(|hit| (hit.damage * 2).max(1)),
                    "Seismic Toss" | "Night Shade" => Some(u32::from(battle.mon(source).level)),
                    "Endeavor" => Some(battle.mon(target).hp - battle.mon(source).hp),
                    "Psywave" => Some(
                        ((50 + battle.rng.below(101)) * u32::from(battle.mon(source).level) / 100)
                            .max(1),
                    ),
                    name => super::legacy_attacks::fixed_damage(name, battle.mon(target).hp),
                };
                if let Some(value) = fixed_damage {
                    return value;
                }
                if stacked_power && battle.format.report_exact_hp {
                    battle.add(format!("|debug|BP: {power}"));
                }
                let mut power = if round_followup && move_data.name == "Round" {
                    if battle.format.report_exact_hp {
                        battle.add("|debug|BP doubled");
                    }
                    power * 2
                } else if let Some(allies) = &beat_up {
                    let ally = allies[usize::from(hit - 1)];
                    let result = 5 + ally.base_stats.atk as u16 / 10;
                    if battle.format.report_exact_hp {
                        battle.add(format!("|debug|BP for {} hit: {result}", ally.name));
                    }
                    result
                } else if ash_shuriken {
                    power + 5
                } else if power == 0 {
                    battle
                        .weight_move_power(&move_data.name, source, target)
                        .unwrap_or(power)
                } else if matches!(
                    move_data.name.as_str(),
                    "Eruption" | "Water Spout" | "Dragon Energy"
                ) {
                    let user = battle.mon(source);
                    let numerator = u32::from(power) * user.hp;
                    let denominator = user.max_hp.get();
                    if battle.format.report_exact_hp {
                        battle.add(format!(
                            "|debug|BP: {}",
                            f64::from(numerator) / f64::from(denominator)
                        ));
                    }
                    (numerator / denominator).max(1) as u16
                } else {
                    power
                };
                if matches!(
                    move_data.name.as_str(),
                    "Wake-Up Slap"
                        | "Smelling Salts"
                        | "Bolt Beak"
                        | "Fishious Rend"
                        | "Crush Grip"
                        | "Wring Out"
                ) {
                    power = battle.legacy_base_power(
                        target,
                        &move_data.name,
                        power,
                        Self::queued_move(queue, target).is_some(),
                    );
                }
                if move_data.name == "Poltergeist" {
                    let item = DEX
                        .item(battle.format.dex_mod, &battle.mon(target).item)
                        .unwrap();
                    battle.add(format!(
                        "|-activate|{}|move: Poltergeist|{}",
                        battle.ident(target),
                        item.effect.name
                    ));
                }
                let disguised = !shield && battle.disguise_form(target).is_some();
                let effect = if disguised {
                    0
                } else {
                    battle
                        .effectiveness_of(move_data, move_type, target, ignores_type)
                        .unwrap()
                };
                if !disguised && battle.delta_stream_weakens(target, move_type) {
                    battle.add("|-fieldactivate|Delta Stream");
                }
                let mut power = battle.situational_base_power(source, target, move_data, power);
                power = battle.inherited_streak_power(source, &move_data.name, hit, power);
                if matches!(move_data.name.as_str(), "Punishment" | "Trump Card") {
                    power = if move_data.name == "Punishment" {
                        (60 + battle.mon(target).boosts.positive() * 20).min(200)
                    } else {
                        let pp = caller
                            .map(|(caller, _)| caller)
                            .or(slot)
                            .and_then(|slot| battle.mon(source).moves.get(slot))
                            .map(|slot| slot.pp);
                        match pp {
                            Some(0) => 200,
                            Some(1) => 80,
                            Some(2) => 60,
                            Some(3) => 50,
                            _ => 40,
                        }
                    };
                    if battle.format.report_exact_hp {
                        battle.add(format!("|debug|BP: {power}"));
                    }
                }
                if matches!(move_data.name.as_str(), "Hex" | "Infernal Parade")
                    && (battle.mon(target).status != Status::Healthy
                        || battle.event_ability(target) == "comatose")
                {
                    if move_data.name == "Hex" && battle.format.report_exact_hp {
                        battle.add("|debug|BP doubled from status condition");
                    }
                    power *= 2;
                }
                if move_data.name == "Assurance" && battle.mon(target).hurt_this_turn {
                    if battle.format.report_exact_hp {
                        battle.add("|debug|BP doubled on damaged target");
                    }
                    power *= 2;
                }
                if matches!(move_data.name.as_str(), "Stomping Tantrum" | "Temper Flare")
                    && battle.mon(source).move_failed_last_turn
                {
                    if battle.format.report_exact_hp {
                        battle.add(format!(
                            "|debug|doubling {} BP due to previous move failure",
                            move_data.name
                        ));
                    }
                    power *= 2;
                }
                if matches!(move_data.name.as_str(), "Avalanche" | "Revenge")
                    && battle.mon(source).attackers_this_turn[target.player.index()]
                        & (1 << battle.mon(target).roster_index)
                        != 0
                {
                    if battle.format.report_exact_hp {
                        battle.add(format!(
                            "|debug|BP doubled for getting hit by {}",
                            battle.ident(target)
                        ));
                    }
                    power *= 2;
                }
                if matches!(move_data.name.as_str(), "Reversal" | "Flail") {
                    power = battle.mon(source).reversal_power();
                    if battle.format.report_exact_hp {
                        battle.add(format!("|debug|BP: {power}"));
                    }
                }
                if move_data.name == "Payback" {
                    let enhanced = battle.mon(target).active_turns != 0
                        && Self::queued_move(queue, target).is_none();
                    if enhanced {
                        power *= 2;
                    }
                    if battle.format.report_exact_hp {
                        battle.add(if enhanced {
                            "|debug|Payback damage boost"
                        } else {
                            "|debug|Payback NOT boosted"
                        });
                    }
                }
                let crit_ratio = battle.crit_ratio(source, target, move_data);
                let crit_roll = move_data.will_crit.unwrap_or_else(|| {
                    crit_ratio != 0
                        && battle
                            .rng
                            .chance(1, [0, 24, 8, 2, 1][usize::from(crit_ratio)])
                });
                let crit = crit_roll
                    && !disguised
                    && !battle.lucky_chant_active(target.player)
                    && !matches!(battle.event_ability(target), "battlearmor" | "shellarmor");
                if crit && !shield {
                    critical_hits |= target.mask();
                }
                // BasePower handlers run after the critical-hit draw, unlike
                // the earlier basePowerCallback used by Hex.
                if move_data.name == "Fickle Beam" && battle.rng.chance(3, 10) {
                    power *= 2;
                    battle.log[base]
                        .public
                        .push_str("|[anim] Fickle Beam All Out");
                    battle.add(format!("|-activate|{source_name}|move: Fickle Beam"));
                } else if matches!(move_data.name.as_str(), "Barb Barrage" | "Venoshock")
                    && matches!(battle.mon(target).status, Status::Poison | Status::Toxic(_))
                {
                    power *= 2;
                }
                battle.regular_hit(
                    source,
                    target,
                    move_data,
                    RegularHit {
                        power,
                        move_type,
                        weather,
                        solar_weak,
                        expanded,
                        converted,
                        hit,
                        spread,
                        effect,
                        crit,
                        screen_handlers,
                        sheer_force_removed,
                        analytic_last: battle.event_ability(source) == "analytic"
                            && !queue.iter().any(|action| {
                                matches!(action.kind, ActionKind::Move { .. })
                                    && action.actor.is_some_and(|actor| {
                                        actor != source && !battle.mon(actor).fainted
                                    })
                            }),
                        bypassed_protect: bypassed & target.mask() != 0,
                        parental_second_hit: parental && hit > 1,
                        contact,
                        roll: None,
                    },
                )
            };
            // TryPrimaryHit handles each shield first, before the ordinary
            // spread calculations consume any critical or damage draws.
            let mut shielded = 0;
            for &target in strike_targets {
                if self.behind_substitute(source, target, move_data) {
                    let value = compute(self, target, true);
                    self.strike_substitute(source, target, move_data, value, ohko);
                    splash_faints |= self.inherited_damage_hook(source, target, move_data, queue);
                    if matches!(
                        move_data.name.as_str(),
                        "Steel Roller" | "Splintered Stormshards"
                    ) {
                        self.clear_terrain();
                    }
                    if shell_side_arm && target.player != source.player {
                        self.add(format!("|-hint|{shell_category} Shell Side Arm"));
                    }
                    shielded |= target.mask();
                    hit_substitute = true;
                }
            }
            for &target in strike_targets {
                if shielded & target.mask() == 0 {
                    let value = compute(self, target, false);
                    damage.push((target, value));
                }
            }
            for (target, hit_damage) in &mut damage {
                let target = *target;
                let value = if self.disguise_form(target).is_some() {
                    self.mon_mut(target).disguise_pending = true;
                    self.add(format!(
                        "|-activate|{}|ability: Disguise",
                        self.ident(target)
                    ));
                    0
                } else {
                    *hit_damage
                };
                let mon = self.mon(target);
                let value = if mon.volatile_ids().contains(&"endure") && value >= mon.hp {
                    let value = mon.hp - 1;
                    self.add(format!("|-activate|{}|move: Endure", self.ident(target)));
                    value
                } else if self.event_ability(target) == "sturdy"
                    && mon.hp == mon.max_hp.get()
                    && value >= mon.hp
                {
                    let value = mon.hp - 1;
                    self.add(format!("|-ability|{}|Sturdy", self.ident(target)));
                    value
                } else {
                    value
                };
                let value = if matches!(move_data.name.as_str(), "False Swipe" | "Hold Back") {
                    value.min(self.mon(target).hp.saturating_sub(1))
                } else {
                    value
                };
                let value = self.focus_survival(target, value, true);
                let half_hp_berry_deferred =
                    matches!(self.event_ability(target), "berserk" | "angershell") && !multihit;
                let turn = self.turn;
                let mon = self.mon_mut(target);
                mon.half_hp_berry_deferred = half_hp_berry_deferred;
                let landed = mon.damage_hp(value);
                if landed > 0 {
                    landed_by_target[target.index()] += landed;
                    hp_after_damage[target.index()] = mon.hp;
                }
                if mon.volatile_ids().contains(&"focuspunch") {
                    mon.lost_focus = true;
                }
                if target.player != source.player
                    && let Some(state) = &mut mon.counter
                    && state.move_kind.category() == move_data.category
                {
                    state.hit = Some(FoeHit {
                        source,
                        damage: landed,
                        turn,
                    });
                }
                *hit_damage = landed;
                total += landed;
                self.add_health_line("-damage", target, false);
                if let Some([a, b]) = move_data.drain
                    && landed > 0
                {
                    let restored = (landed * u32::from(a) + u32::from(b) / 2) / u32::from(b);
                    self.transfer_health(source, target, restored, DrainKind::Move);
                }
            }
            for &(target, _) in &damage {
                if matches!(
                    move_data.name.as_str(),
                    "Wake-Up Slap" | "Smelling Salts" | "Incinerate"
                ) {
                    self.legacy_attack_hit(target, &move_data.name);
                }
                if shell_side_arm && target.player != source.player {
                    self.add(format!("|-hint|{shell_category} Shell Side Arm"));
                }
                if matches!(move_data.name.as_str(), "Bug Bite" | "Pluck") {
                    self.eat_stolen_berry(source, target, &move_data.name);
                }
                if move_data.name == "Clear Smog" {
                    self.mon_mut(target).boosts = Boosts::default();
                    self.add(format!("|-clearboost|{}", self.ident(target)));
                }
                if move_data.name == "Jaw Lock" {
                    self.start_mutual_trap(source, target);
                }
                if matches!(move_data.name.as_str(), "Smack Down" | "Thousand Arrows") {
                    self.smack_down(target, queue);
                }
                splash_faints |= self.inherited_damage_hook(source, target, move_data, queue);
                if move_data.name == "Thousand Waves" {
                    self.start_linked_trap(source, target);
                }
                if move_data.name == "Plasma Fists" {
                    self.start_legacy_field(source, LegacyField::IonDeluge, &move_data.name);
                }
                if let Some(item) = thrown_item {
                    self.fling_hit(source, target, item);
                }
                if matches!(
                    move_data.name.as_str(),
                    "Steel Roller" | "Splintered Stormshards"
                ) {
                    self.clear_terrain();
                }
                self.debug_suppressed_hook(target, "onHit");
            }
            for &(target, _) in &damage {
                if damaging_z
                    && contact
                    && let Some(protection) = self.mon(target).protection
                {
                    self.protection_contact(protection, source, target, base);
                }
                if contact && self.mon(target).volatile_ids().contains(&"beakblast") {
                    self.try_status(source, "brn", target, move_data, false, base);
                }
                if critical_hits & target.mask() != 0 && self.event_ability(target) == "angerpoint"
                {
                    self.ability_boost(target, "Anger Point", BoostStat::Atk, 12, false);
                }
            }
            if move_data.name == "Ice Spinner" && !damage.is_empty() {
                self.clear_terrain();
            }
            if let Some(binding) = binding {
                for &(target, _) in &damage {
                    self.start_partial_trap(target, source, binding);
                }
            }
            let pivot = match move_data.name.as_str() {
                "U-turn" => Some(Pivot::Move("U-turn")),
                "Volt Switch" => Some(Pivot::Move("Volt Switch")),
                "Flip Turn" => Some(Pivot::Move("Flip Turn")),
                _ => None,
            };
            if pivot.is_some()
                && self.mon(source).hp != 0
                && self.first_bench(source.player, 0).is_some()
            {
                self.mon_mut(source).pivot_move = pivot;
            }
            if !move_data.boosts.is_empty() {
                for &(target, _) in &damage {
                    self.boost_from(
                        target,
                        source,
                        &move_data.boosts,
                        BoostOrigin::from_move(move_data),
                    );
                }
            }
            if !sheer_force_removed && !move_data.self_boosts.is_empty() && (!parental || hit == 1)
            {
                let roll = self.rng.below(100);
                if move_data
                    .self_chance
                    .is_none_or(|chance| roll < u32::from(chance))
                {
                    self.boost(source, &move_data.self_boosts, true);
                }
            }
            if let Some(ty) = spent_element
                && self.mon(source).hp != 0
            {
                let mon = self.mon_mut(source);
                if !matches!(mon.species.base_species.as_str(), "Arceus" | "Silvally") {
                    mon.spent_elements |= Pokemon::element_bit(ty);
                    mon.added_type = None;
                    mon.unrevealed_types = Default::default();
                    mon.unknown_type = false;
                }
                let types = mon.types().collect::<Vec<_>>().join("/");
                self.add(format!(
                    "|-start|{source_name}|typechange|{types}|[from] move: {}",
                    move_data.name
                ));
            }
            if !sheer_force_removed && move_data.self_recharge && self.mon(source).hp > 0 {
                let mon = self.mon_mut(source);
                mon.recharge = 2;
                mon.track_volatile("mustrecharge");
                self.add(format!("|-mustrecharge|{source_name}"));
            }
            if !sheer_force_removed && move_data.self_rampage {
                self.rampage_hit(source, slot.expect("rampage moves are not reflectable"));
            }
            if move_data.name == "Uproar" {
                self.begin_uproar(source, slot.expect("Uproar is not reflectable"));
            }
            if move_data.name == "Glaive Rush" && self.mon(source).hp > 0 {
                let mon = self.mon_mut(source);
                mon.glaive_drawback = true;
                mon.track_volatile("glaiverush");
                self.add(format!("|-singlemove|{source_name}|Glaive Rush|[silent]"));
            }
            for &target in strike_targets {
                if (sheer_force_removed || move_data.secondaries.is_empty())
                    && !added_flinch
                    && fling_secondary.is_none()
                {
                    break;
                }
                let hit_pokemon = shielded & target.mask() == 0;
                let dust = hit_pokemon && self.event_ability(target) == "shielddust";
                let cloak = hit_pokemon && self.mon(target).effective_item() == "covertcloak";
                if hit_pokemon && self.format.report_exact_hp {
                    if dust {
                        self.add("|debug|Shield Dust prevent secondary");
                    } else if self.mon(target).active_ability() == "shielddust" {
                        self.add("|debug|ModifySecondaries handler suppressed by Mold Breaker");
                    }
                    if cloak {
                        self.add("|debug|Covert Cloak prevent secondary");
                    }
                }
                for effect in move_data
                    .secondaries
                    .iter()
                    .filter(|_| !sheer_force_removed)
                {
                    let genesis = damaging_z && move_data.name == "Genesis Supernova";
                    if (dust || cloak) && effect.self_boosts.is_empty() && !genesis {
                        continue;
                    }
                    let roll = self.rng.below(100);
                    if effect.chance.is_none_or(|chance| roll < u32::from(chance)) {
                        if genesis {
                            self.set_terrain(source, Terrain::Psychic, None);
                        }
                        if !hit_pokemon {
                            if !effect.self_boosts.is_empty() {
                                self.boost(source, &effect.self_boosts, true);
                            }
                            continue;
                        }
                        if matches!(move_data.name.as_str(), "Dire Claw" | "Tri Attack") {
                            let choices = if move_data.name == "Dire Claw" {
                                ["psn", "par", "slp"]
                            } else {
                                ["brn", "par", "frz"]
                            };
                            let status = choices[self.rng.below(3) as usize];
                            self.try_status(target, status, source, move_data, false, base);
                        }
                        if move_data.name == "Eerie Spell" {
                            self.reduce_last_move_pp(target, "Eerie Spell", 3);
                        }
                        if move_data.name == "Syrup Bomb" {
                            self.start_syrup_bomb(source, target);
                        }
                        if matches!(move_data.name.as_str(), "Anchor Shot" | "Spirit Shackle") {
                            self.start_linked_trap(source, target);
                        }
                        if move_data.name == "Alluring Voice"
                            && self.mon(target).stats_raised
                            && !self.safeguard_blocks(target, source, true, false, true)
                        {
                            self.apply_confusion(target, false);
                        }
                        if move_data.name == "Burning Jealousy" && self.mon(target).stats_raised {
                            self.try_status(target, "brn", source, move_data, false, base);
                        }
                        if let Some(status) = &effect.status
                            && !self.try_status(target, status, source, move_data, false, base)
                        {
                            self.report_no_move_effect();
                        }
                        if !effect.boosts.is_empty() {
                            self.boost_from(target, source, &effect.boosts, BoostOrigin::Secondary);
                        }
                        if !effect.self_boosts.is_empty() {
                            self.boost(source, &effect.self_boosts, true);
                        }
                        if effect.volatile_status.as_deref() == Some("flinch") {
                            self.apply_flinch(target);
                        }
                        if effect.volatile_status.as_deref() == Some("confusion")
                            && !self.safeguard_blocks(target, source, true, false, true)
                            && !self.apply_confusion(target, false)
                        {
                            self.report_no_move_effect();
                        }
                        if effect.volatile_status.as_deref() == Some("healblock") {
                            self.start_heal_block(target, 2);
                        }
                        if effect.volatile_status.as_deref() == Some("saltcure")
                            && !self.start_salt_cure(target)
                        {
                            self.report_no_move_effect();
                        }
                        if effect.volatile_status.as_deref() == Some("sparklingaria") {
                            if self.mon(target).hp == 0 {
                                self.report_no_move_effect();
                            } else {
                                sparkling_aria_targets |= target.mask();
                            }
                        }
                        if move_data.name == "Throat Chop"
                            && self.mon(target).hp > 0
                            && self.mon(target).throat_chop == 0
                        {
                            let mon = self.mon_mut(target);
                            mon.throat_chop = 2;
                            mon.track_volatile("throatchop");
                            self.add(format!(
                                "|-start|{}|Throat Chop|[silent]",
                                self.ident(target)
                            ));
                        }
                    }
                }
                if added_flinch && !dust && !cloak && self.rng.chance(1, 10) && hit_pokemon {
                    self.apply_flinch(target);
                }
                if let Some(effect) = fling_secondary
                    && !dust
                    && !cloak
                {
                    // An unspecified chance is guaranteed but still draws.
                    self.rng.below(100);
                    if hit_pokemon {
                        if let Some(status) = &effect.status {
                            self.try_status(target, status, source, move_data, false, base);
                        } else if effect.volatile_status.as_deref() == Some("flinch") {
                            self.apply_flinch(target);
                        }
                    }
                }
            }
            for &(target, _) in &damage {
                if matches!(move_data.name.as_str(), "Dragon Tail" | "Circle Throw") {
                    self.schedule_drag(source, target);
                }
                if (move_type == "Fire" || move_data.thaws_target)
                    && matches!(self.mon(target).status, Status::Freeze(_))
                {
                    self.cure_status(target, "[msg]");
                }
            }
            let original_source_hp = self.mon(source).hp;
            for &(target, landed) in &damage {
                if self.mon(target).hp == 0 && self.event_ability(target) == "innardsout" {
                    let previous = if smart_target {
                        0
                    } else {
                        previous_strike_damage
                    };
                    self.innards_out(target, source, landed + previous);
                }
            }
            if contact {
                for &(target, _) in &damage {
                    if self.mon(target).active_ability() == "aftermath" {
                        self.aftermath(target, source);
                    }
                }
                for &(target, _) in &damage {
                    if self.mon(target).active_ability() == "roughskin" {
                        if self.mon(source).hp > 0
                            && self.mon(source).active_ability() == "magicguard"
                        {
                            self.add(format!(
                                "|-activate|{}|ability: Rough Skin",
                                self.ident(target)
                            ));
                        } else {
                            self.indirect_damage(
                                source,
                                8,
                                &format!("|[from] ability: Rough Skin|[of] {}", self.ident(target)),
                            );
                        }
                    }
                }
                for &(target, _) in &damage {
                    if self.mon(target).effective_item() == "rockyhelmet" {
                        self.indirect_damage(
                            source,
                            6,
                            &format!("|[from] item: Rocky Helmet|[of] {}", self.ident(target)),
                        );
                    }
                }
            }
            for &(target, landed) in &damage {
                if self.event_ability(target) == "illusion" {
                    self.end_illusion(target);
                }
                if landed > 0 && self.event_ability(target) == "spicyspray" {
                    self.spicy_spray(target, source, base);
                }
                if landed > 0 && self.event_ability(target) == "seedsower" {
                    self.set_terrain(target, Terrain::Grassy, Some("Seed Sower"));
                }
                if landed > 0 && self.event_ability(target) == "sandspit" {
                    self.set_weather(target, Weather::Sand, Some("Sand Spit"));
                }
                if contact {
                    match self.event_ability(target) {
                        "flamebody" | "static" | "poisonpoint" | "effectspore" => {
                            self.contact_status(target, source, base)
                        }
                        "cutecharm" => self.cute_charm(target, source),
                        "gooey" => self.contact_speed_drop(target, source, "Gooey"),
                        "tanglinghair" => self.contact_speed_drop(target, source, "Tangling Hair"),
                        "mummy" | "lingeringaroma" => self.contact_ability(target, source),
                        "wanderingspirit" => {
                            self.skill_swap(source, target);
                        }
                        _ => {}
                    }
                }
                if self.mon(target).active_ability() == "cursedbody"
                    && !struggle
                    && !delayed
                    && self.mon(source).disabled_slot.is_none()
                    && self.rng.chance(3, 10)
                    && !self.aroma_veil_blocks(source, false)
                {
                    self.disable_last_move(source, target, true);
                }
                if self.event_ability(target) == "thermalexchange" && move_type == "Fire" {
                    self.ability_boost(target, "Thermal Exchange", BoostStat::Atk, 1, false);
                }
                self.charge_after_damage(target, move_data);
                if self.event_ability(target) == "stamina" {
                    self.ability_boost(target, "Stamina", BoostStat::Def, 1, false);
                }
                if move_type == "Dark" && self.event_ability(target) == "justified" {
                    self.ability_boost(target, "Justified", BoostStat::Atk, 1, false);
                }
                if matches!(move_type, "Dark" | "Bug" | "Ghost")
                    && self.event_ability(target) == "rattled"
                {
                    self.ability_boost(target, "Rattled", BoostStat::Spe, 1, false);
                }
                if move_data.category == Category::Physical
                    && self.event_ability(target) == "toxicdebris"
                {
                    self.scatter_toxic_debris(target, source);
                }
                if self.event_ability(target) == "weakarmor"
                    && move_data.category == Category::Physical
                {
                    self.ability_boosts(
                        target,
                        "Weak Armor",
                        &[(BoostStat::Def, -1), (BoostStat::Spe, 2)],
                        false,
                    );
                }
                if self.mon(target).effective_item() == "airballoon" {
                    self.consume(target, "Air Balloon", false);
                }
                if contact && self.event_ability(source) == "poisontouch" {
                    self.poison_touch(source, target, base);
                }
            }
            if !sheer_force_removed
                && matches!(move_data.name.as_str(), "Rapid Spin" | "Mortal Spin")
                && !damage.is_empty()
            {
                self.clear_spinning_hazards(source, &move_data.name);
            }
            if matches!(move_data.name.as_str(), "Thief" | "Covet") {
                for &(target, _) in &damage {
                    self.thief_after_hit(source, target, &move_data.name);
                }
            } else if move_data.name == "Knock Off" {
                for &(target, _) in &damage {
                    if let Some(item) = self.can_take_item(target) {
                        self.lose_held_item(target);
                        self.add(format!(
                            "|-enditem|{}|{item}|[from] move: Knock Off|[of] {source_name}",
                            self.ident(target)
                        ));
                    } else if self.event_ability(target) == "stickyhold"
                        && self.mon(target).hp > 0
                        && !self.mon(target).item.is_empty()
                    {
                        self.add(format!(
                            "|-activate|{}|ability: Sticky Hold",
                            self.ident(target)
                        ));
                    }
                }
            }
            if self.mon(source).hp > 0
                && let Some(hazard) = attack_hazard
                && damage.iter().any(|&(_, amount)| amount > 0)
            {
                self.add_hazard(source.player.foe(), hazard);
            }
            self.emergency_exit(source, original_source_hp);
            self.each_event();
            if smart_target {
                split_darts[usize::from(hit - 1)] = damage.first().copied();
            }
            hits_landed += 1;
        }
        if smart_target {
            damage.clear();
            damage.extend(split_darts.into_iter().flatten());
        }
        if sacrifice || final_gambit && self.mon(source).hp == 0 {
            self.mark_fainted(source);
        }
        let mut fainted = 0;
        for &(target, direct_damage) in &damage {
            if self.mon(target).hp == 0 {
                let linked = self.mon(target).destiny_bond
                    && target.player != source.player
                    && self.mon(source).hp > 0;
                self.mark_fainted(target);
                if linked {
                    self.add(format!(
                        "|-activate|{}|move: Destiny Bond",
                        self.ident(target)
                    ));
                    self.mon_mut(source).hp = 0;
                }
                fainted += 1;
            }
            if target != source {
                if direct_damage != 0 {
                    let attacker = self.mon(source).roster_index;
                    self.mon_mut(target).attackers_this_turn[source.player.index()] |=
                        1 << attacker;
                }
                self.mon_mut(target).times_attacked += if smart_target { 1 } else { hits_landed };
            }
            if target.player != source.player && !self.mon(target).fainted {
                self.mon_mut(target).last_foe_hit = Some(FoeHit {
                    source,
                    damage: direct_damage,
                    turn: self.turn,
                });
            }
        }
        if splash_faints != 0 {
            for pos in self.positions() {
                if splash_faints & pos.mask() != 0
                    && self.mon(pos).hp == 0
                    && !self.mon(pos).fainted
                {
                    self.mark_fainted(pos);
                }
            }
        }
        if self.mon(source).hp == 0 {
            if !self.mon(source).fainted {
                self.mark_fainted(source);
            }
            self.check_win();
        }
        if fainted > 0 && self.active_effects(source) {
            self.boost_after_knockouts(source, fainted);
        }
        if ohko && self.mon(spread_targets[0]).hp == 0 {
            self.add("|-ohko");
        }
        if multihit && hits_landed > u32::from(parental) && !darts {
            let target = spread_targets[0];
            let ident = if self.mon(target).fainted {
                format!("{}: {}", target.player.id(), self.mon(target).name)
            } else {
                self.ident(target)
            };
            self.add(format!("|-hitcount|{ident}|{hits_landed}"));
        }
        let recoil = if struggle && !hit_substitute {
            (self.mon(source).max_hp.get() + 2) / 4
        } else if let Some([num, den]) = move_data.recoil
            && total > 0
            && !matches!(self.mon(source).active_ability(), "rockhead" | "magicguard")
        {
            ((total * u32::from(num) + u32::from(den) / 2) / u32::from(den)).max(1)
        } else {
            0
        };
        if recoil > 0 && !self.mon(source).fainted {
            let recoil = if struggle {
                recoil
            } else {
                self.focus_survival(source, recoil, false)
            };
            let mon = self.mon_mut(source);
            let original_hp = mon.hp;
            if !struggle {
                mon.half_hp_berry_deferred = false;
            }
            if struggle {
                mon.hp = mon.hp.saturating_sub(recoil);
            } else {
                mon.damage_hp(recoil);
            }
            self.add_health_line_with(
                "-damage",
                source,
                false,
                if struggle {
                    "|[from] recoil"
                } else {
                    "|[from] Recoil"
                },
            );
            self.emergency_exit(source, original_hp);
        }
        if let Some(recoil) = half_recoil
            && hits_landed > 0
        {
            self.half_max_hp_recoil(source, recoil);
        }
        self.each_event();
        // Champions runs the target-side AfterMoveSecondary and EmergencyExit
        // hooks even when Sheer Force removes the move's secondary effects.
        self.target_effects_after_move(
            source,
            &damage,
            landed_by_target,
            PostHit {
                contact,
                multihit,
                smart_target,
                delayed,
            },
        );
        for &target in &spread_targets {
            let index = target.index();
            self.emergency_exit(target, hp_after_damage[index] + landed_by_target[index]);
        }
        let suppress_after_move = sheer_force_removed && self.event_ability(source) == "sheerforce";
        if move_data.name == "Fell Stinger" && fainted > 0 && !suppress_after_move {
            self.boost(source, &[(BoostStat::Atk, 3)], true);
        }
        if hits_landed > 0
            && !move_data.after_hit_boosts.is_empty()
            && !(sheer_force_removed && move_data.name == "Clangorous Soulblaze")
        {
            self.boost(source, &move_data.after_hit_boosts, true);
        }
        if self.mon(source).hp == 0 && !self.mon(source).fainted {
            self.mark_fainted(source);
        }
        let original_source_hp = self.mon(source).hp;
        if !suppress_after_move && !delayed {
            self.pay_life_orb_recoil(source);
            self.shell_bell_after_move(source, after_move_target, total);
        }
        if !suppress_after_move && !delayed && hits_landed > 0 {
            self.magician_after_hit(source, &spread_targets);
        }
        self.emergency_exit(source, original_source_hp);
        if !suppress_after_move && move_data.name == "Sparkling Aria" && self.mon(source).hp > 0 {
            for &target in &spread_targets {
                if target != source
                    && self.mon(target).hp != 0
                    && self.mon(target).status == Status::Burn
                    && (spread_targets.len() > 1 || sparkling_aria_targets & target.mask() != 0)
                {
                    self.cure_status(target, "[msg]");
                }
            }
        }
        if consecutive {
            self.note_consecutive_move(true, move_data);
        }
        if spread {
            let labels: Vec<_> = spread_targets.into_iter().map(Position::label).collect();
            self.log[base]
                .public
                .push_str(&format!("|[spread] {}", labels.join(",")));
        }
        true
    }

    fn tick_future_move(&mut self, target: Position) {
        let turn = self.overflowed_turn();
        let side = self.sides[target.player.index()].as_mut().unwrap();
        let Some(future) = side.future_moves[target.slot].filter(|m| turn >= m.ending_turn) else {
            return;
        };
        side.future_moves[target.slot] = None;
        let party = self.party(future.source_side).unwrap();
        let source = Position {
            player: future.source_side,
            slot: party
                .iter()
                .position(|p| p.roster_index == future.roster_index)
                .unwrap(),
        };
        if self.mon(target).fainted || target == source {
            let cause = if self.mon(target).fainted {
                "fainted"
            } else {
                "the user"
            };
            self.add(format!(
                "|-hint|{} did not hit because the target is {cause}.",
                future.data.name
            ));
            return;
        }
        self.add(format!(
            "|-end|{}|move: {}",
            self.ident(target),
            future.data.name
        ));
        let prior_breaker = self.ability_breaker.take();
        self.run_move(
            source,
            MoveInvocation::Future {
                data: future.data,
                target,
            },
            &mut Vec::new(),
        );
        self.ability_breaker = prior_breaker;
    }

    fn apply_flinch(&mut self, target: Position) {
        if self.mon(target).hp > 0
            && self.event_ability(target) != "innerfocus"
            && !self.mon(target).volatile_ids().contains(&"focuspunch")
        {
            let mon = self.mon_mut(target);
            mon.flinched = true;
            mon.track_volatile("flinch");
        }
    }

    fn cancel_airborne_charge(&mut self, target: Position, queue: &mut Vec<Action>) -> bool {
        if !matches!(
            self.mon(target).semi_invulnerability(),
            Some(super::charge::SemiInvulnerable::Airborne { .. })
        ) {
            return false;
        }
        self.mon_mut(target).clear_charge();
        queue.retain(|action| {
            action.actor != Some(target) || !matches!(action.kind, ActionKind::Move { .. })
        });
        true
    }

    fn smack_down(&mut self, target: Position, queue: &mut Vec<Action>) {
        if self.mon(target).hp == 0 {
            return;
        }
        let canceled = self.cancel_airborne_charge(target, queue);
        if self.mon(target).volatile_ids().contains(&"smackdown") {
            if canceled {
                self.add(format!("|-start|{}|Smack Down", self.ident(target)));
            }
            return;
        }
        let mon = self.mon(target);
        let airborne = (mon.has_type("Flying")
            || matches!(self.event_ability(target), "levitate" | "eelevate"))
            && mon.effective_item() != "ironball"
            && !self.forces_grounded(target);
        let was_magnetized = self.clear_magnet_rise(target);
        let was_lifted = self
            .mon_mut(target)
            .clear_legacy_condition(LegacyCondition::Telekinesis);
        if airborne || canceled || was_magnetized || was_lifted {
            self.mon_mut(target).track_volatile("smackdown");
            self.add(format!("|-start|{}|Smack Down", self.ident(target)));
        }
    }

    fn inherited_damage_hook(
        &mut self,
        source: Position,
        target: Position,
        move_data: &Move,
        queue: &[Action],
    ) -> u8 {
        match move_data.name.as_str() {
            "Core Enforcer"
                if self.mon(target).hp > 0
                    && self.mon(target).active_turns > 0
                    && Self::queued_move(queue, target).is_none()
                    && self.mon(target).effective_item() != "abilityshield"
                    && !DEX
                        .ability(&self.mon(target).ability)
                        .is_some_and(|ability| ability.cannot_suppress) =>
            {
                self.inhibit_ability(target);
            }
            "Flame Burst" if self.slots() > 1 => {
                let ally = target.other_slot();
                self.indirect_damage(
                    ally,
                    16,
                    &format!("|[from] flameburst|[of] {}", self.ident(source)),
                );
                if self.mon(ally).hp == 0 && !self.mon(ally).fainted {
                    return ally.mask();
                }
            }
            _ => {}
        }
        0
    }

    fn note_consecutive_move(&mut self, forced: bool, data: &Move) {
        if forced {
            self.add(format!(
                "|-hint|Some effects can force a Pokemon to use {} again in a row.",
                data.name
            ));
        }
    }

    fn apply_curse(&mut self, source: Position, mut target: Position) -> bool {
        let mon = self.mon_mut(source);
        let cost = (mon.max_hp.get() / 2).max(1);
        mon.hp = mon.hp.saturating_sub(cost);
        self.add_health_line("-damage", source, false);
        if source.player == target.player {
            let Some(foe) = self.random_target(source, TargetKind::Normal) else {
                return false;
            };
            target = foe;
        }
        let mon = self.mon_mut(target);
        mon.untrack_volatile("curse");
        mon.track_volatile("curse");
        self.add(format!(
            "|-start|{}|Curse|[of] {}",
            self.ident(target),
            self.ident(source)
        ));
        true
    }

    pub(super) fn half_max_hp_recoil(&mut self, pos: Position, effect: &str) {
        let mon = self.mon(pos);
        if mon.hp != 0 && mon.active_ability() != "magicguard" {
            let original_hp = mon.hp;
            let damage = mon.max_hp.get().div_ceil(2);
            let damage = self.focus_survival(pos, damage, false);
            let mon = self.mon_mut(pos);
            mon.half_hp_berry_deferred = false;
            mon.damage_hp(damage);
            self.add_health_line_with("-damage", pos, false, &format!("|[from] {effect}"));
            self.emergency_exit(pos, original_hp);
        }
    }

    pub(super) fn mark_fainted(&mut self, target: Position) {
        self.last_faint = Some(target.player);
        let side = self.sides[target.player.index()].as_mut().unwrap();
        side.total_fainted = (side.total_fainted + 1).min(100);
        side.fainted_this_turn = true;
        self.add(format!("|faint|{}", self.ident(target)));
        self.receive_fainted_ability(target);
        self.end_supreme_overlord(target, false);
        self.weather_ability_changed(target, true);
        self.clear_linked_traps(target);
        let rules = self.stat_rules;
        let mon = self.mon_mut(target);
        mon.reset_forme(rules);
        mon.fainted = true;
        mon.illusion = None;
        mon.disguise_pending = false;
        mon.reset_ability();
        mon.pivot_move = None;
        mon.reviving = false;
        mon.stall = None;
        mon.ally_switch_stall = None;
        mon.protection = None;
        mon.flinched = false;
        mon.lost_focus = false;
        mon.substitute_hp = 0;
        mon.recharge = 0;
        mon.charge = None;
        mon.confusion = 0;
        mon.rampage = None;
        mon.uproar = None;
        mon.attention = None;
        mon.glaive_drawback = false;
        mon.destiny_bond = false;
        mon.helping_hand = 0;
        mon.volatile_order.clear();
        mon.imprisoned = false;
        mon.choice_lock = None;
        mon.metronome = None;
        mon.roosted = false;
        mon.set_temporary_type(None);
        mon.yawn = None;
        mon.seeded_from = None;
        mon.partial_trap = None;
        mon.perish_count = 0;
        mon.throat_chop = 0;
        mon.taunt = 0;
        mon.encore = None;
        mon.attract = None;
        mon.disabled_slot = None;
        mon.last_move_slot = None;
        mon.extra_move = None;
        mon.move_failed_this_turn = false;
        mon.move_failed_last_turn = false;
        mon.last_foe_hit = None;
        mon.attackers_this_turn = [0; 2];
        mon.heal_block = 0;
        mon.legacy_conditions = Default::default();
        mon.octolock = None;
        mon.fling_pending = false;
        mon.stockpile = Default::default();
        mon.magnet_rise = 0;
        mon.dragon_cheer = 0;
        mon.laser_focus = 0;
        mon.fury_cutter = None;
        mon.rolling_attack = None;
        mon.gem_boost = false;
        mon.electrified = false;
        mon.gastro_acid = false;
        mon.lock_on = None;
        mon.syrup_bomb = None;
        mon.counter = None;
        mon.times_attacked = 0;
        mon.boosts = Boosts::default();
        // Clearing volatiles restores the raw species speed, even though
        // normal field-speed updates skip a fainted Pokémon.
        mon.speed = mon.stats.spe as i32;
    }

    fn clear_destiny_bond(&mut self, pos: Position, notify_debug: bool) -> bool {
        let mon = self.mon_mut(pos);
        if !std::mem::take(&mut mon.destiny_bond) {
            return false;
        }
        mon.untrack_volatile("destinybond");
        if notify_debug && self.format.report_exact_hp {
            self.add("|debug|removing Destiny Bond before attack");
        }
        true
    }

    pub(super) fn boost(
        &mut self,
        pos: Position,
        boosts: &[(BoostStat, i8)],
        is_self: bool,
    ) -> bool {
        if self.event_ability(pos) == "simple" {
            let changes: Vec<_> = boosts
                .iter()
                .map(|&(stat, n)| (stat, n.saturating_mul(2)))
                .collect();
            return self.apply_boosts(pos, &changes, is_self, false);
        }
        let invert = self.event_ability(pos) == "contrary";
        self.apply_boosts(pos, boosts, is_self, invert)
    }

    pub(super) fn apply_boosts(
        &mut self,
        pos: Position,
        boosts: &[(BoostStat, i8)],
        is_self: bool,
        invert: bool,
    ) -> bool {
        self.apply_boosts_labeled(pos, boosts, is_self, invert, None, true)
    }

    pub(super) fn apply_boosts_labeled(
        &mut self,
        pos: Position,
        boosts: &[(BoostStat, i8)],
        is_self: bool,
        invert: bool,
        label: Option<BoostLabel<'_>>,
        notify_copy: bool,
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
        let mut succeeded = false;
        let record = notify_copy && self.possible_opportunist;
        let mut observed = PendingBoostCopy::default();
        for &(stat, amount) in boosts {
            let amount = if invert {
                amount.saturating_neg()
            } else {
                amount
            };
            let mon = self.mon_mut(pos);
            let changed = mon.boosts.add(stat, amount);
            if record {
                observed.accumulate(stat, changed);
            }
            if changed > 0 {
                mon.stats_raised = true;
            } else if changed < 0 {
                mon.stats_lowered = true;
            }
            let event = if amount < 0 || mon.boosts.get(stat) == -6 {
                "-unboost"
            } else {
                "-boost"
            };
            if changed != 0 || !is_self {
                let mut line = format!(
                    "|{event}|{}|{}|{}",
                    self.ident(pos),
                    stat.id(),
                    changed.abs()
                );
                if changed != 0 {
                    match label {
                        Some(BoostLabel::Item(item)) => {
                            line.push_str("|[from] item: ");
                            line.push_str(item);
                        }
                        Some(BoostLabel::ZPower) => line.push_str("|[zeffect]"),
                        None => {}
                    }
                }
                self.add(line);
            }
            succeeded |= changed != 0;
        }
        if record {
            self.note_foe_boosts(pos, observed);
        }
        succeeded
    }

    fn accuracy(
        &self,
        source: Position,
        target: Position,
        data: &Move,
        accuracy: u8,
        zoom: bool,
    ) -> u32 {
        let modifier = self.accuracy_modifier(
            source,
            target,
            self.active_weather_evasion(target).0,
            data.category,
            zoom,
        );
        let accuracy = if modifier != 4096 {
            self.stat_rules
                .truncation
                .modify(u32::from(accuracy), modifier) as i32
        } else {
            i32::from(accuracy)
        };
        let evasion = if self.move_ignores_evasion(source, data)
            || source != target && self.event_ability(source) == "unaware"
        {
            0
        } else {
            let mon = self.mon(target);
            let evasion = mon.boosts.get(BoostStat::Evasion);
            if evasion > 0 && mon.is_identified() {
                0
            } else {
                evasion
            }
        };
        let aim = if source != target && self.event_ability(target) == "unaware" {
            0
        } else {
            self.mon(source).boosts.get(BoostStat::Accuracy)
        };
        let boost = (i32::from(aim) - i32::from(evasion)).clamp(-6, 6);
        (accuracy * (3 + boost.max(0)) / (3 - boost.min(0))) as u32
    }

    pub(super) fn effectiveness(&self, move_data: &Move, target: Position) -> Option<i8> {
        if move_data.name == "Struggle" {
            return Some(0);
        }
        self.effectiveness_of(move_data, &move_data.move_type, target, false)
    }

    pub(super) fn move_ignores_immunity(
        &self,
        source: Position,
        move_data: &Move,
        move_type: &str,
    ) -> bool {
        matches!(self.event_ability(source), "scrappy" | "mindseye")
            && matches!(move_type, "Normal" | "Fighting")
            || match &move_data.ignore_immunity {
                Value::Bool(value) => *value,
                Value::Object(types) => types
                    .get(move_type)
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                _ => move_data.category == Category::Status,
            }
    }

    pub(super) fn move_ignores_evasion(&self, source: Position, data: &Move) -> bool {
        data.ignore_evasion
            || matches!(
                self.event_ability(source),
                "illuminate" | "keeneye" | "mindseye"
            )
    }

    pub(super) fn effectiveness_of(
        &self,
        move_data: &Move,
        move_type: &str,
        target: Position,
        ignores_immunity: bool,
    ) -> Option<i8> {
        let mon = self.mon(target);
        if move_data.name == "Thousand Arrows"
            && move_type == "Ground"
            && mon.has_type("Flying")
            && !self.grounded(target)
        {
            return Some(0);
        }
        if move_type == "Ground"
            && mon.effective_item() == "ironball"
            && mon.has_type("Flying")
            && !self.forces_grounded(target)
        {
            // The held item neutralizes the entire Ground matchup on a
            // Flying holder, including the contribution of its other type.
            return Some(0);
        }
        let mut modifier = 0;
        for typing in mon.types() {
            if typing == "???" {
                continue;
            }
            if move_data.name == "Freeze-Dry" && typing == "Water" {
                modifier += 1;
                continue;
            }
            let types = &DEX.type_data(&to_id(typing)).unwrap().damage_taken;
            match types.get(move_type) {
                Some(1)
                    if typing == "Flying"
                        && move_data.category != Category::Status
                        && self.weather() == Some(Weather::StrongWinds)
                        && self.effective_weather() == Some(Weather::StrongWinds) => {}
                Some(1) => modifier += 1,
                Some(2) => modifier -= 1,
                Some(3)
                    if !(ignores_immunity
                        || mon.exposed_immunity(typing, move_type)
                        || typing == "Flying"
                            && move_type == "Ground"
                            && self.forces_grounded(target)) =>
                {
                    return None;
                }
                _ => {}
            }
            if move_data.name == "Flying Press" {
                match types.get("Flying") {
                    Some(1) => modifier += 1,
                    Some(2) => modifier -= 1,
                    _ => {}
                }
            }
        }
        if move_type == "Fire" && mon.volatile_ids().contains(&"tarshot") {
            modifier += 1;
        }
        Some(modifier)
    }

    fn delta_stream_weakens(&self, target: Position, move_type: &str) -> bool {
        self.weather() == Some(Weather::StrongWinds)
            && self.mon(target).has_type("Flying")
            && self.effective_weather() == Some(Weather::StrongWinds)
            && DEX.type_data("flying").unwrap().damage_taken.get(move_type) == Some(&1)
    }
}

fn move_base_target(data: &Move, source: Position, loc: i8) -> TargetKind {
    if data.name == "Curse" && loc == source.loc(source) {
        // Champions locks in this self target while resolving the action.
        // The hit callback still checks the user's type at its own step.
        TargetKind::User
    } else {
        data.target
    }
}

impl Pokemon {
    pub(super) fn trampled_by(&self, data: &Move) -> bool {
        self.volatile_ids().contains(&"minimize") && data.flags.contains_key("minimize")
    }

    fn requested_target(&self, data: &Move) -> TargetKind {
        if data.name == "Curse" && !self.has_type("Ghost") {
            TargetKind::User
        } else {
            data.target
        }
    }

    fn forced_move(&self) -> Option<(usize, &str, &str)> {
        if self.recharge > 0 {
            Some((RECHARGE_SLOT, "Recharge", "recharge"))
        } else {
            let slot = self
                .rampage
                .map(|state| state.slot)
                .or_else(|| self.charge.map(|state| state.slot))
                .or_else(|| self.uproar.map(|state| state.slot))
                .or_else(|| self.rolling_attack.map(|state| state.slot))?;
            let m = if slot == EXTRA_MOVE_SLOT {
                self.extra_move.as_ref().unwrap()
            } else {
                &self.moves[slot]
            };
            Some((slot, &m.move_data.name, &m.id))
        }
    }

    pub(super) fn current_speed(
        &self,
        weather: Option<Weather>,
        terrain: Option<Terrain>,
        tailwind: bool,
    ) -> u32 {
        let mut stat = boosted_stat(self.stats.spe, self.boosts.get(BoostStat::Spe));
        if self.fainted {
            // A queued replacement still uses the outgoing Pokemon's speed,
            // but Showdown ignores its inactive item, ability, and side.
            return stat;
        }
        if tailwind {
            stat *= 2;
        }
        if matches!(
            (
                self.active_ability(),
                self.personal_weather(weather).map(Weather::ordinary)
            ),
            ("swiftswim", Some(Weather::Rain))
                | ("chlorophyll", Some(Weather::Sun))
                | ("sandrush", Some(Weather::Sand))
                | ("slushrush", Some(Weather::Snow | Weather::Hail))
        ) || self.active_ability() == "surgesurfer" && terrain == Some(Terrain::Electric)
            || self.active_ability() == "unburden"
                && self.item.is_empty()
                && self.volatile_ids().contains(&"unburden")
        {
            stat *= 2;
        }
        let quick_feet = self.active_ability() == "quickfeet" && self.status != Status::Healthy;
        let multiplier = match (self.effective_item(), quick_feet) {
            ("choicescarf", true) => 9216,
            ("ironball", true) => 3072,
            (_, true) | ("choicescarf", false) => 6144,
            ("ironball", false) => 2048,
            _ => 4096,
        };
        if multiplier != 4096 {
            stat = crate::mechanics::Truncation::Cartridge.modify(stat, multiplier);
        }
        if self.status == Status::Paralysis && !quick_feet {
            stat / 2
        } else {
            stat
        }
    }

    pub(super) fn supported_ability(ability: &str) -> bool {
        matches!(
            ability,
            "" | "blaze"
                | "torrent"
                | "overgrow"
                | "swarm"
                | "pressure"
                | "levitate"
                | "shellarmor"
                | "battlearmor"
                | "goodasgold"
                | "roughskin"
                | "runaway"
                | "drizzle"
                | "drought"
                | "sandstream"
                | "snowwarning"
                | "swiftswim"
                | "chlorophyll"
                | "sandrush"
                | "slushrush"
                | "sandforce"
                | "arenatrap"
                | "magnetpull"
                | "shadowtag"
                | "intimidate"
                | "innerfocus"
                | "oblivious"
                | "owntempo"
                | "magician"
                | "symbiosis"
                | "gluttony"
                | "stakeout"
                | "overcoat"
                | "speedboost"
                | "multiscale"
                | "guts"
                | "adaptability"
                | "technician"
                | "ironfist"
                | "sharpness"
                | "toughclaws"
                | "stickyhold"
                | "rockhead"
                | "mirrorarmor"
                | "competitive"
                | "stamina"
                | "icebody"
                | "waterabsorb"
                | "lightningrod"
                | "stormdrain"
                | "motordrive"
                | "voltabsorb"
                | "eartheater"
                | "wellbakedbody"
                | "magicguard"
                | "weakarmor"
                | "mummy"
                | "wanderingspirit"
                | "lingeringaroma"
                | "trace"
                | "illuminate"
                | "skilllink"
                | "suctioncups"
                | "electricsurge"
                | "grassysurge"
                | "mistysurge"
                | "psychicsurge"
                | "shedskin"
                | "clearbody"
                | "whitesmoke"
                | "fullmetalbody"
                | "soundproof"
                | "auraguard"
                | "sturdy"
                | "thermalexchange"
                | "infiltrator"
                | "vitalspirit"
                | "insomnia"
                | "simple"
                | "leafguard"
                | "earlybird"
                | "scrappy"
                | "angerpoint"
                | "eelevate"
                | "mimicry"
                | "unburden"
                | "hospitality"
                | "supremeoverlord"
                | "receiver"
                | "powerofalchemy"
                | "ripen"
                | "plus"
                | "minus"
                | "curiousmedicine"
                | "imposter"
                | "filter"
                | "solidrock"
                | "prismarmor"
                | "neuroforce"
                | "shadowshield"
                | "bigpecks"
                | "justified"
                | "poisonheal"
                | "prankster"
                | "liquidooze"
                | "toxicdebris"
                | "compoundeyes"
                | "friendguard"
                | "armortail"
                | "dazzling"
                | "queenlymajesty"
                | "sheerforce"
                | "harvest"
                | "pickup"
                | "berserk"
                | "angershell"
                | "hypercutter"
                | "gooey"
                | "tanglinghair"
                | "hydration"
                | "stall"
                | "tangledfeet"
                | "fluffy"
                | "punkrock"
                | "spicyspray"
                | "pickpocket"
                | "innardsout"
                | "seedsower"
                | "anticipation"
                | "forewarn"
                | "forecast"
                | "hungerswitch"
                | "unseenfist"
                | "piercingdrill"
                | "quickdraw"
                | "poisontouch"
                | "cheekpouch"
                | "cudchew"
                | "solarpower"
                | "megasol"
                | "parentalbond"
                | "sweetveil"
                | "supersweetsyrup"
                | "guarddog"
                | "stench"
                | "longreach"
                | "opportunist"
                | "moody"
                | "quickfeet"
                | "grasspelt"
                | "marvelscale"
                | "magmaarmor"
                | "sandspit"
                | "fairyaura"
                | "darkaura"
                | "aurabreak"
                | "stalwart"
                | "propellertail"
                | "rattled"
                | "steelyspirit"
                | "analytic"
                | "galewings"
                | "rivalry"
                | "hustle"
                | "firemane"
                | "effectspore"
                | "electromorphosis"
                | "windpower"
                | "zerotohero"
                | "klutz"
                | "battlebond"
                | "mindseye"
                | "shielddust"
                | "telepathy"
                | "immunity"
                | "cutecharm"
                | "flamebody"
                | "static"
                | "poisonpoint"
                | "strongjaw"
                | "megalauncher"
                | "sandveil"
                | "snowcloak"
                | "raindish"
                | "sniper"
                | "hugepower"
                | "purepower"
                | "moxie"
                | "heatproof"
                | "thickfat"
                | "unnerve"
                | "cursedbody"
                | "protean"
                | "libero"
                | "aerilate"
                | "pixilate"
                | "galvanize"
                | "refrigerate"
                | "dragonize"
                | "noguard"
                | "flowerveil"
                | "moldbreaker"
                | "teravolt"
                | "turboblaze"
                | "illusion"
                | "heavymetal"
                | "lightmetal"
                | "cloudnine"
                | "airlock"
                | "deltastream"
                | "multitype"
                | "rkssystem"
                | "download"
                | "desolateland"
                | "primordialsea"
                | "stancechange"
                | "magicbounce"
                | "naturalcure"
                | "regenerator"
                | "flashfire"
                | "dryskin"
                | "keeneye"
                | "frisk"
                | "reckless"
                | "purifyingsalt"
                | "disguise"
                | "damp"
                | "aftermath"
                | "sapsipper"
                | "corrosion"
                | "furcoat"
                | "defiant"
                | "contrary"
                | "merciless"
                | "superluck"
                | "synchronize"
                | "limber"
                | "emergencyexit"
                | "wimpout"
                | "waterbubble"
                | "liquidvoice"
                | "bulletproof"
                | "steadfast"
                | "surgesurfer"
                | "aromaveil"
                | "healer"
                | "screencleaner"
                | "unaware"
        )
    }

    pub(super) fn entry_error(&self) -> Option<BattleError> {
        if !Self::supported_ability(&self.ability) {
            return Some(BattleError::Unsupported(format!(
                "ability {}",
                self.ability
            )));
        }
        if !matches!(
            self.item.as_str(),
            "" | "leftovers"
                | "sitrusberry"
                | "focussash"
                | "focusband"
                | "ejectbutton"
                | "shellbell"
                | "metronome"
                | "assaultvest"
                | "eviolite"
                | "berryjuice"
                | "blacksludge"
                | "airballoon"
                | "lifeorb"
                | "rockyhelmet"
                | "lumberry"
                | "chestoberry"
                | "aspearberry"
                | "cheriberry"
                | "oranberry"
                | "pechaberry"
                | "persimberry"
                | "rawstberry"
                | "leppaberry"
                | "mysticwater"
                | "miracleseed"
                | "charcoal"
                | "fairyfeather"
                | "spelltag"
                | "damprock"
                | "heatrock"
                | "smoothrock"
                | "icyrock"
                | "heavydutyboots"
                | "shedshell"
                | "choicescarf"
                | "choiceband"
                | "choicespecs"
                | "quickclaw"
                | "kingsrock"
                | "razorfang"
                | "normalgem"
                | "lightball"
                | "bindingband"
                | "covertcloak"
                | "widelens"
                | "zoomlens"
                | "ironball"
                | "toxicorb"
                | "flameorb"
                | "gripclaw"
                | "softsand"
                | "magnet"
                | "lightclay"
                | "expertbelt"
                | "wiseglasses"
                | "muscleband"
                | "loadeddice"
                | "terrainextender"
                | "mentalherb"
                | "safetygoggles"
                | "liechiberry"
                | "ganlonberry"
                | "salacberry"
                | "petayaberry"
                | "apicotberry"
                | "powerherb"
                | "abilityshield"
                | "utilityumbrella"
                | "bigroot"
                | "brightpowder"
                | "laxincense"
                | "whiteherb"
                | "redcard"
                | "electricseed"
                | "grassyseed"
                | "mistyseed"
                | "psychicseed"
                | "floatstone"
                | "blackbelt"
                | "blackglasses"
                | "dragonfang"
                | "hardstone"
                | "metalcoat"
                | "nevermeltice"
                | "poisonbarb"
                | "sharpbeak"
                | "silkscarf"
                | "silverpowder"
                | "twistedspoon"
                | "scopelens"
                | "razorclaw"
                | "leek"
        ) && super::items::resistance_berry(&self.item).is_none()
            && DEX
                .item(crate::dex::Mod::Champions, &self.item)
                .is_none_or(|item| {
                    item.mega_stone.is_none()
                        && item.z_move.is_none()
                        && item.protected_form_base.is_none()
                        && !item.is_primal_orb
                        && !item.inert_held_item
                })
        {
            return Some(BattleError::Unsupported(format!("item {}", self.item)));
        }
        if self.species.has_condition {
            return Some(BattleError::Unsupported(format!(
                "entry effects for {}",
                self.species.name
            )));
        }
        None
    }
}
