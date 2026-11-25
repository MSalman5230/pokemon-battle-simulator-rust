use super::*;
use super::{abilities::BoostOrigin, charge::ChargeSpec, damage::RegularHit, leech::DrainKind};
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
    Ability,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) enum Protection {
    Protect,
    KingsShield,
}

#[derive(Clone, Copy)]
enum MoveInvocation {
    Choice {
        slot: usize,
        loc: i8,
        prankster: bool,
    },
    Reflected {
        data: &'static Move,
        target: Position,
    },
    Future {
        data: &'static Move,
        target: Position,
    },
}

#[derive(Default)]
struct MoveResult {
    spends_charge: bool,
    failed: bool,
}

impl Protection {
    fn volatile(self) -> &'static str {
        match self {
            Self::Protect => "protect",
            Self::KingsShield => "kingsshield",
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
    priority: Priority,
    fractional_priority: f64,
    prankster: bool,
}

const STRUGGLE_SLOT: usize = usize::MAX;
const RECHARGE_SLOT: usize = usize::MAX - 1;

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
            fractional_priority: 0.0,
            prankster: false,
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
                                "move": m.move_data.name, "id": m.id, "pp": m.pp, "maxpp": m.max_pp,
                                "target": mon.requested_target(m.move_data), "disabled": self.move_disabled_in_request(player, slot, move_slot),
                            });
                            if let Some(source) = self.disable_source(pos, move_slot) {
                                request["disabledSource"] = json!(source);
                            }
                            request
                        }).collect()
                    };
                    let mut request = json!({"moves":moves});
                    if self.last_active(pos) && self.foe_imprison(pos).next().is_some() && !mon.revealed_all_disables {
                        request["maybeDisabled"] = json!(true);
                        if mon.revealed_disables == 0 { request["maybeLocked"] = json!(true); }
                    }
                    if self.trapped_in_request(player, slot) {
                        request["trapped"] = json!(true);
                    } else if self.last_active(pos) && self.first_bench(player, 0).is_some() && self.trap_info(pos).possible {
                        request["maybeTrapped"] = json!(true);
                    }
                    if self.can_mega_in_slot(player, slot) { request["canMegaEvo"] = json!(true); }
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

    fn slots(&self) -> usize {
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
            self.mon_mut(pos).pivot_move = Some(Pivot::Ability);
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
                        None => false,
                        Some(SpecialChoice::Mega) if !mega => {
                            self.validate_mega(actor)?;
                            mega = true;
                            true
                        }
                        Some(SpecialChoice::Mega) => {
                            return Err(BattleError::Choice("only one Mega Evolution per side"));
                        }
                        _ => return Err(BattleError::Unsupported("special move modifier".into())),
                    };
                    self.select_move(actor, selection, *target, wants_mega)?
                }
                SlotChoice::Auto => self.select_move(actor, &MoveChoice::Auto, None, false)?,
                _ => return Err(BattleError::Choice("invalid action for current request")),
            };
            if let ActionKind::Switch { bench_slot, .. } = action {
                if let Some(error) = party[bench_slot].entry_error() {
                    return Err(error);
                }
                switched |= 1 << bench_slot;
            }
            actions.push(Action::new(Some(actor), action));
        }
        if choices.next().is_some() || (replace && switched.count_ones() as usize != needed) {
            return Err(BattleError::Choice(
                "wrong number of replacement or slot choices",
            ));
        }
        Ok(actions)
    }

    fn first_bench(&self, player: Player, exclude: u32) -> Option<usize> {
        self.party(player)
            .unwrap()
            .iter()
            .enumerate()
            .skip(self.slots())
            .find(|(slot, p)| !p.fainted && exclude & (1 << slot) == 0)
            .map(|(i, _)| i)
    }

    fn select_move(
        &self,
        actor: Position,
        selection: &MoveChoice,
        target: Option<i8>,
        mega: bool,
    ) -> Result<ActionKind, BattleError> {
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
            if !valid || mega {
                return Err(BattleError::Choice("only the forced move is available"));
            }
            return Ok(ActionKind::Move {
                move_slot,
                target: self.mon(actor).charge.map_or(0, |state| state.target),
                mega: false,
            });
        }
        let moves = &self.mon(actor).moves;
        let visibly_available = self.any_move_in_request(actor);
        let really_available = moves
            .iter()
            .enumerate()
            .any(|(i, _)| !self.actual_disabled(actor, i));
        if !really_available {
            if mega
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
            return Ok(ActionKind::Move {
                move_slot: STRUGGLE_SLOT,
                target: 0,
                mega: false,
            });
        }
        let move_slot = match selection {
            MoveChoice::Slot(n) => Some(n.get() - 1),
            MoveChoice::Name(name) => moves.iter().position(|m| &m.id == name),
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
        // A projected generic move has only implemented data effects.
        // The remaining moves must have explicit, audited special handling.
        if !info.move_data.generic
            && !matches!(
                info.id.as_str(),
                "tackle"
                    | "quickattack"
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
        {
            return Err(BattleError::Unsupported(format!("move {}", info.id)));
        }
        let kind = self.mon(actor).requested_target(info.move_data);
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
        Ok(ActionKind::Move {
            move_slot,
            target,
            mega,
        })
    }

    fn move_data(&self, source: Position, slot: usize) -> &'static Move {
        if slot == STRUGGLE_SLOT {
            DEX.moves(self.format.dex_mod, "struggle").unwrap()
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
                    action.prankster = self.prankster_boosts(source, data);
                    action.priority.priority = f64::from(
                        self.base_move_priority(source, data) + i8::from(action.prankster),
                    ) + action.fractional_priority;
                }
            }
            action.priority.speed = self.field_speed(source);
        }
    }

    fn assign_fractional_priority(&mut self, action: &mut Action) {
        let actor = action.actor.unwrap();
        if self.event_ability(actor) == "stall" {
            action.fractional_priority = -0.1;
        }
        if self.mon(actor).effective_item() == "quickclaw" && self.rng.chance(1, 5) {
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
        self.prioritize(&mut action);
        self.insert_action(queue, action);
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
                && self.mon(pos).ability == "unnerve"
            {
                self.add(format!("|-ability|{}|Unnerve", self.ident(pos)));
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
                match self.mon(pos).ability.as_str() {
                    "hospitality" => deferred_hospitality |= pos.mask(),
                    "imposter" => self.imposter_on_entry(pos),
                    _ => self.start_ability(pos),
                }
                self.on_switch_in_item(pos);
            }
        }
        for (pos, _) in order.into_iter().take(count) {
            if deferred_hospitality & pos.mask() != 0 {
                self.hospitality(pos);
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
        let party = &mut self.sides[outgoing.player.index()].as_mut().unwrap().party;
        if let Some(Pivot::Move(pass @ ("Baton Pass" | "Shed Tail"))) = pivot {
            let (active, incoming) = party.split_at_mut(bench_slot);
            incoming[0].receive_pass(&active[outgoing.slot], pass == "Shed Tail");
        }
        party[outgoing.slot].reset_forme(rules);
        party[outgoing.slot].protection = None;
        party[outgoing.slot].stall = None;
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
        party[outgoing.slot].active_turns = 0;
        party[outgoing.slot].active_move_actions = 0;
        party[outgoing.slot].roosted = false;
        party[outgoing.slot].temporary_type = None;
        party[outgoing.slot].added_type = None;
        party[outgoing.slot].spent_elements = 0;
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
        party[outgoing.slot].move_failed_this_turn = false;
        party[outgoing.slot].move_failed_last_turn = false;
        party[outgoing.slot].last_foe_hit = None;
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
            let mut original_hp = [0; 4];
            match action.kind {
                ActionKind::MegaEvolve => {
                    let pos = action.actor.unwrap();
                    if !self.mon(pos).fainted {
                        self.mega_evolve(pos);
                        self.white_herbs_after_action();
                    }
                }
                ActionKind::BeforeTurn => {
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
                    if self.run_move(
                        source,
                        MoveInvocation::Choice {
                            slot: move_slot,
                            loc: target,
                            prankster: action.prankster,
                        },
                        &mut queue,
                    ) {
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
            if !queue.first().is_some_and(|q| {
                matches!(
                    q.kind,
                    ActionKind::Switch {
                        immediate: true,
                        ..
                    }
                )
            }) {
                self.each_event();
            }
            if matches!(action.kind, ActionKind::Residual | ActionKind::RunSwitch) {
                for pos in self.positions() {
                    self.emergency_exit(pos, original_hp[pos.index()]);
                }
            }
            let mut waiting = false;
            for player in Player::ALL {
                for (slot, required) in self.force_switch(player).into_iter().enumerate() {
                    if !required {
                        continue;
                    }
                    waiting = true;
                    if self.mon(Position { player, slot }).pivot_move.is_some() {
                        // BeforeSwitchOut marks this move-driven switch as having
                        // already run its update, so it happens only once.
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
        for pos in self.positions() {
            let mon = self.mon_mut(pos);
            mon.revealed_trap = false;
            mon.revealed_disables = 0;
            mon.revealed_all_disables = false;
            mon.acted = false;
            mon.stats_raised = false;
            mon.stats_lowered = false;
            mon.move_failed_last_turn = std::mem::take(&mut mon.move_failed_this_turn);
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

    fn schedule_drag(&mut self, source: Position, target: Position) -> bool {
        if self.mon(target).hp == 0
            || self.mon(source).hp == 0
            || self.first_bench(target.player, 0).is_none()
        {
            return false;
        }
        if self.event_ability(target) == "suctioncups" {
            self.add(format!(
                "|-activate|{}|ability: Suction Cups",
                self.ident(target)
            ));
        } else {
            self.drag_flags |= target.mask();
        }
        true
    }

    pub(super) fn start_ability(&mut self, pos: Position) {
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
            _ => None,
        } {
            self.add(format!("|-ability|{}|{label}", self.ident(pos)));
        }
        self.switch_in_weather(pos);
        self.weather_ability_changed(pos);
        self.switch_in_terrain(pos);
        if self.mon(pos).ability == "intimidate" {
            self.intimidate(pos);
        }
        if self.mon(pos).ability == "frisk" {
            self.frisk(pos);
        }
        if self.mon(pos).ability == "screencleaner" {
            self.screen_cleaner(pos);
        }
        if self.mon(pos).ability == "mimicry" {
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
        self.try_trace(pos);
    }

    pub(super) fn positions(&self) -> impl Iterator<Item = Position> + use<> {
        let slots = self.slots();
        Player::ALL
            .into_iter()
            .flat_map(move |player| (0..slots).map(move |slot| Position { player, slot }))
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
        self.sort_unaware_handlers();
        self.stat_rules.truncation.action_speed(
            self.mon(pos).current_speed(
                self.effective_weather(),
                self.terrain(),
                self.sides[pos.player.index()].as_ref().unwrap().tailwind != 0,
            ),
            (self.trick_room > 0) ^ self.twisted_dimension,
        )
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
            self.attract_update(pos);
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
            Stall,
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
            StatusOrb,
            WhiteHerb,
            ShedSkin,
            Hydration,
            Healer,
            Weather,
            SpeedBoost,
            Harvest,
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
            Taunt,
            Encore,
            Disable,
            Screen(Screen),
            Tailwind,
            TrickRoom,
            Terrain,
            GrassyHeal,
        }
        self.add("|");
        self.update_speed();
        let mut timers = Vec::new();
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
        for player in Player::ALL {
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
            if self.terrain() == Some(Terrain::Grassy) {
                timers.push((pos, Effect::GrassyHeal, priority(5, 2)));
            }
            if mon.ability == "speedboost" {
                timers.push((pos, Effect::SpeedBoost, priority(28, 2)));
            }
            if !mon.fainted {
                match mon.ability.as_str() {
                    "harvest" => timers.push((pos, Effect::Harvest, priority(28, 2))),
                    "pickup" => timers.push((pos, Effect::Pickup, priority(28, 2))),
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
            if mon.encore.is_some() {
                timers.push((pos, Effect::Encore, priority(16, 2)));
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
            if mon.ability == "healer" {
                timers.push((pos, Effect::Healer, priority(5, 3)));
            }
            if mon.protection.is_some() {
                timers.push((pos, Effect::Protect, priority(0, 2)));
            }
            if mon.stall.is_some() {
                timers.push((pos, Effect::Stall, priority(0, 2)));
            }
            if mon.flinched {
                timers.push((pos, Effect::Flinch, priority(0, 2)));
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
        }
        speed_sort_by(&mut timers, &mut self.rng, |a, b| a.2.compare(b.2));
        let mut perish_faints = Vec::new();
        for (pos, effect, _) in timers {
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
                Effect::Leftovers | Effect::StatusOrb | Effect::WhiteHerb
                    if mon.ignores_held_item() =>
                {
                    self.debug_ignored_item(pos, "Residual");
                }
                Effect::Leftovers => {
                    let amount = mon.max_hp.get() / 16;
                    self.item_heal(pos, amount, "Leftovers");
                }
                Effect::StatusOrb => self.activate_status_orb(pos),
                Effect::WhiteHerb => self.white_herb(pos),
                Effect::Weather
                | Effect::Screen(_)
                | Effect::TrickRoom
                | Effect::Terrain
                | Effect::Wish
                | Effect::FutureMove
                | Effect::Tailwind => {
                    unreachable!()
                }
                Effect::GrassyHeal => self.grassy_heal(pos),
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
                Effect::Healer => self.healer_residual(pos),
                Effect::SpeedBoost => self.speed_boost(pos),
                Effect::Harvest => self.harvest_item(pos),
                Effect::Pickup => self.pick_up_item(pos),
                Effect::Roost => {
                    mon.roosted = false;
                    mon.untrack_volatile("roost");
                }
                Effect::Protect => {
                    if let Some(protect) = mon.protection.take() {
                        mon.untrack_volatile(protect.volatile());
                    }
                }
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
                    | Effect::Flinch
                    | Effect::Attention
                    | Effect::Roost
                    | Effect::HelpingHand
                    | Effect::FocusPunch
                    | Effect::BeakBlast
                    | Effect::ChillyReception
                    | Effect::Counter => true,
                    Effect::Stall => mon.stall.is_none(),
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
            if !mon.fainted && mon.ability == "magicbounce" {
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
    ) -> bool {
        let mut result = MoveResult::default();
        let attempted = self.run_move_inner(source, invocation, queue, &mut result);
        if result.spends_charge {
            self.use_electric_charge(source);
        }
        if !matches!(invocation, MoveInvocation::Future { .. }) && !self.mon(source).fainted {
            self.mon_mut(source).move_failed_this_turn = result.failed || !attempted;
        }
        attempted
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
            } => (Some(slot), loc, None, false, prankster),
            MoveInvocation::Reflected { target, .. } => (None, 0, Some(target), true, false),
            MoveInvocation::Future { target, .. } => (None, 0, Some(target), false, false),
        };
        let delayed = matches!(invocation, MoveInvocation::Future { .. });
        let own_action = !reflected && !delayed;
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
        let move_data = match invocation {
            MoveInvocation::Choice { slot, .. } => self.move_data(source, slot),
            MoveInvocation::Reflected { data, .. } | MoveInvocation::Future { data, .. } => data,
        };
        result.spends_charge =
            own_action && move_data.name != "Charge" && move_data.move_type == "Electric";
        let struggle = slot == Some(STRUGGLE_SLOT);
        let charge_spec = ChargeSpec::for_move(&move_data.name);
        let releasing_charge = own_action && self.mon(source).charge.is_some();
        let weather = self.effective_weather();
        let solar_weak = charge_spec.is_some_and(|spec| spec.weak_in_bad_weather)
            && matches!(weather, Some(Weather::Rain | Weather::Sand | Weather::Snow));
        let counter = CounterMove::for_move(move_data);
        let retaliation =
            counter.is_some() || matches!(move_data.name.as_str(), "Metal Burst" | "Comeuppance");
        let sheer_cold = move_data.name == "Sheer Cold";
        let ohko = sheer_cold
            || matches!(
                move_data.name.as_str(),
                "Fissure" | "Guillotine" | "Horn Drill"
            );
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
                || releasing_charge);
        let called_from = if reflected {
            "|[from] ability: Magic Bounce"
        } else if continuing {
            "|[from] lockedmove"
        } else {
            ""
        };
        let weather_ball = move_data.name == "Weather Ball";
        let stacked_power = matches!(move_data.name.as_str(), "Stored Power" | "Power Trip");
        let power = match move_data.name.as_str() {
            "Weather Ball" if weather.is_some() => move_data.base_power * 2,
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
            _ => move_data.base_power,
        };
        let (move_type, converted) = if struggle {
            ("???", false)
        } else if weather_ball {
            (weather.map_or("Normal", Weather::ball_type), false)
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
            if mon.ability == "liquidvoice" && move_data.flags.contains_key("sound") {
                ("Water", false)
            } else {
                mon.converted_move_type(base)
            }
        };
        let source_name = self.ident(source);
        let darts = move_data.name == "Dragon Darts";
        let curse = move_data.name == "Curse";
        let base_target = move_base_target(move_data, source, loc);
        let original_target =
            forced_target.or_else(|| self.resolve_target(source, base_target, loc));
        let mut target = retaliation_hit.map(|hit| hit.source).or(original_target);
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
        if own_action && !self.before_status_move(source, move_data) {
            self.mon_mut(source).clear_charge();
            self.clear_destiny_bond(source, false);
            return false;
        }
        if reflected && self.mon(source).throat_chop > 0 && move_data.flags.contains_key("sound") {
            self.add(format!("|cant|{source_name}|move: Throat Chop"));
            return false;
        }
        result.spends_charge = own_action && move_data.name != "Charge" && move_type == "Electric";
        if move_data.name == "Focus Punch" && self.mon(source).lost_focus {
            self.add(format!("|cant|{source_name}|Focus Punch|Focus Punch"));
            return false;
        }
        if !struggle && slot.is_some_and(|slot| self.mon(source).disabled_by_choice(slot)) {
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
        let mut accuracy = match (move_data.name.as_str(), weather) {
            ("Blizzard", Some(Weather::Snow)) => Accuracy::Bypass(true),
            ("Hurricane" | "Thunder", Some(Weather::Rain)) if target.is_some() => {
                Accuracy::Bypass(true)
            }
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
        } else if let Some(slot) = slot {
            let mon = self.mon_mut(source);
            if !continuing {
                mon.moves[slot].pp -= 1;
                mon.used_moves |= 1 << slot;
            }
            mon.last_move_slot = Some(slot);
            if mon.holds_choice_item() && mon.choice_lock.is_none() {
                mon.choice_lock = Some(slot);
                mon.track_volatile("choicelock");
            }
        }
        if !delayed
            && matches!(
                self.mon(source).ability.as_str(),
                "moldbreaker" | "teravolt" | "turboblaze"
            )
        {
            self.ability_breaker = Some(source);
        }
        if !delayed {
            self.stance_change(source, move_data);
        }
        let expanded = move_data.name == "Expanding Force"
            && self.terrain() == Some(Terrain::Psychic)
            && self.grounded(source);
        let kind = if expanded {
            // Both ModifyMove passes retarget a changed target kind.
            self.random_target(source, TargetKind::AllAdjacentFoes);
            target = self.random_target(source, TargetKind::AllAdjacentFoes);
            TargetKind::AllAdjacentFoes
        } else if curse {
            if self.format.report_exact_hp {
                self.add("|debug|Curse onModifyMove triggered");
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
        let base = if delayed {
            self.log
                .iter()
                .rposition(|line| line.public.starts_with("|move|"))
                .unwrap_or(self.log.len() - 1)
        } else {
            let base = self.log.len();
            self.add(format!(
                "|move|{source_name}|{}|{target_name}{called_from}",
                move_data.name
            ));
            base
        };
        let mut redirected = false;
        if !delayed
            && self.slots() > 1
            && !move_data.tracks_target
            && let Some(target) = actual_target.filter(|_| {
                kind.is_choosable()
                    || matches!(kind, TargetKind::RandomNormal | TargetKind::Scripted)
            })
            && let Some(redirect) = self.redirect_target(source, target, move_data, move_type)
        {
            redirected = true;
            spread_targets.clear();
            spread_targets.push(redirect);
            self.log[base].public = format!(
                "|move|{source_name}|{}|{}{called_from}",
                move_data.name,
                self.ident(redirect)
            );
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

        if let Some(slot) = slot
            && !struggle
            && !continuing
        {
            let pressure_targets =
                if kind == TargetKind::All || move_data.flags.contains_key("mustpressure") {
                    self.party(source.player.foe()).unwrap()[..self.slots()]
                        .iter()
                        .filter(|mon| mon.hp != 0 && mon.ability == "pressure")
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
                            pos.player != source.player && self.mon(**pos).ability == "pressure"
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
        let priority = self.base_move_priority(source, move_data) + i8::from(prankster);
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
                let weather_skip = spec.instant.is_some() && spec.instant == weather;
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
                        loc,
                    );
                    self.protean_prepare(source, move_type);
                    return true;
                }
            }
        }
        if let Some(screen) = Screen::from_move(&move_data.name) {
            if !matches!(screen, Screen::AuroraVeil)
                || self.effective_weather() == Some(Weather::Snow)
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
        if move_data.name == "Haze" {
            self.protean_prepare(source, move_type);
            self.add("|-clearallboost");
            for pos in self.positions() {
                self.mon_mut(pos).boosts = Boosts::default();
            }
            self.each_event();
            self.each_event();
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
        if let Some(weather) = Weather::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if !self.set_weather(source, weather, None) {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
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
                result.failed = true;
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
            if mon.hp == mon.max_hp.get() {
                self.add(format!("|-fail|{source_name}|heal"));
                result.failed = true;
            } else if matches!(mon.ability.as_str(), "insomnia" | "vitalspirit") {
                let ability = if mon.ability == "insomnia" {
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
        if matches!(
            move_data.name.as_str(),
            "Protect" | "Detect" | "King's Shield"
        ) {
            let more_actions = queue
                .iter()
                .any(|q| matches!(q.kind, ActionKind::Move { .. } | ActionKind::Switch { .. }));
            let stall = self.mon(source).stall.map_or(1, |(counter, _)| counter);
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
            let protect = if move_data.name == "King's Shield" {
                Protection::KingsShield
            } else {
                Protection::Protect
            };
            mon.protection = Some(protect);
            mon.track_volatile(protect.volatile());
            mon.track_volatile("stall");
            mon.stall = Some(((stall * 3).min(729), 2));
            self.add(format!("|-singleturn|{source_name}|Protect"));
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
        if move_data.name == "Poltergeist" && self.mon(spread_targets[0]).item.is_empty() {
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
        if matches!(move_data.name.as_str(), "Sucker Punch" | "Thunderclap") {
            let target = spread_targets[0];
            let pending_attack = self.mon(target).recharge == 0
                && Self::queued_move(queue, target).is_some_and(|slot| {
                    slot != RECHARGE_SLOT
                        && self.move_data(target, slot).category != Category::Status
                });
            if !pending_attack {
                self.move_still(base);
                self.add(format!("|-fail|{source_name}"));
                result.failed = true;
                return true;
            }
        }
        if !delayed {
            self.protean_prepare(source, move_type);
        }
        let spread = !darts && spread_targets.len() > 1;
        let potential = spread_targets.len();
        spread_targets.retain(|&target| {
            if self
                .mon(target)
                .semi_invulnerability()
                .is_none_or(|state| state.exposed_to(&move_data.name))
                || self.mon(source).ability == "noguard"
                || self.mon(target).ability == "noguard"
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
        spread_targets.retain(|&mut_pos| {
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
                && (protect == Protection::Protect || move_data.category != Category::Status)
                && move_data.flags.contains_key("protect")
            {
                // Protect and King's Shield return NOT_FAIL, unlike a type
                // immunity or a stopped move from Psychic Terrain.
                protected += 1;
                if smart_target {
                    smart_target = false;
                } else {
                    self.add(format!("|-activate|{}|move: Protect", self.ident(mut_pos)));
                }
                self.stop_protected_rampage(source);
                if protect == Protection::KingsShield && move_data.flags.contains_key("contact") {
                    self.boost_from(
                        source,
                        mut_pos,
                        &[(BoostStat::Atk, -1)],
                        BoostOrigin::from_move(
                            DEX.moves(self.format.dex_mod, "kingsshield").unwrap(),
                        ),
                    );
                }
                false
            } else if !reflected
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
            } else if self.event_ability(mut_pos) == "oblivious" && move_data.name == "Taunt" {
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
            if !ignores_type
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
                    || (move_data.move_type == "Ground"
                        && self.mon(pos).effective_item() == "airballoon")))
                || (move_data.flags.contains_key("powder")
                    && source != pos
                    && self.status_immune(pos, "powder"))
                || (move_data.name == "Leech Seed" && self.mon(pos).has_type("Grass"))
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
        if move_data.name == "Disable"
            && spread_targets
                .iter()
                .any(|&p| self.mon(p).last_move_slot.is_none())
        {
            self.move_still(base);
            self.add(format!("|-fail|{source_name}"));
            result.failed = true;
            return true;
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
                && self.mon(source).ability != "noguard"
                && self.mon(pos).ability != "noguard"
                && !self.mon(pos).glaive_drawback
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
            result.failed = failed_hit || potential != 0;
            if spread {
                self.log[base].public.push_str("|[spread] ");
            }
            if matches!(
                move_data.name.as_str(),
                "Supercell Slam" | "High Jump Kick" | "Jump Kick"
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
            } else if let Some(recoil) = half_recoil {
                self.half_max_hp_recoil(source, recoil);
                if self.mon(source).hp == 0 {
                    self.mark_fainted(source);
                }
            }
            self.note_consecutive_move(consecutive, move_data);
            return true;
        }
        if move_data.name == "Feint" {
            for &target in &spread_targets {
                let mon = self.mon_mut(target);
                if let Some(protection) = mon.protection.take() {
                    mon.untrack_volatile(protection.volatile());
                    mon.stall = None;
                    mon.untrack_volatile("stall");
                    self.add(format!("|-activate|{}|move: Feint", self.ident(target)));
                }
            }
        }

        if move_data.category == Category::Status {
            spread_targets.retain(|&target| {
                if !self.behind_substitute(source, target, move_data) {
                    return true;
                }
                self.add(format!("|-fail|{source_name}"));
                self.move_still(base);
                false
            });
            let mut any_success = false;
            let mut hit_labels = Vec::new();
            let life_dew = move_data.name == "Life Dew";
            for target in spread_targets {
                let mut succeeded = false;
                if spread && life_dew {
                    hit_labels.push(target.label());
                }
                if matches!(move_data.name.as_str(), "Taunt" | "Encore" | "Disable")
                    && self.aroma_veil_blocks(target, true)
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
                    if self.mon(target).volatile_ids().contains(&"focusenergy") {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        self.mon_mut(target).track_volatile("focusenergy");
                        self.add(format!("|-start|{}|move: Focus Energy", self.ident(target)));
                        succeeded = true;
                    }
                }
                if matches!(
                    move_data.name.as_str(),
                    "Confuse Ray" | "Sweet Kiss" | "Supersonic"
                ) {
                    if self.mon(target).confusion > 0 {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    } else if self.event_ability(target) == "owntempo" {
                        self.add(format!(
                            "|-immune|{}|confusion|[from] ability: Own Tempo",
                            self.ident(target)
                        ));
                    } else if self.status_blocked_by_terrain(target, "confusion")
                        == Some(Terrain::Misty)
                    {
                        self.terrain_activate(target, Terrain::Misty);
                    } else {
                        succeeded = self.apply_confusion(target, false);
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
                if move_data.name == "Soak" {
                    let mon = self.mon(target);
                    if mon.types().eq(["Water"])
                        || matches!(mon.species.base_species.as_str(), "Arceus" | "Silvally")
                    {
                        self.add(format!("|-fail|{}", self.ident(target)));
                    } else {
                        let mon = self.mon_mut(target);
                        mon.temporary_type = Some("Water");
                        mon.added_type = None;
                        mon.spent_elements = 0;
                        self.add(format!("|-start|{}|typechange|Water", self.ident(target)));
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
                        self.add("|debug|Curse onTryHit triggered");
                    }
                    if !ghost || !self.mon(target).volatile_ids().contains(&"curse") {
                        if self.format.report_exact_hp {
                            self.add("|debug|Curse onHit triggered");
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
                if move_data.name == "Strength Sap" {
                    let mon = self.mon(target);
                    let stage = mon.boosts.get(BoostStat::Atk);
                    let mut success = false;
                    if stage > -6 {
                        let restored = boosted_stat(mon.stats.atk, stage).max(1);
                        success = self.boost_from(
                            target,
                            source,
                            &[(BoostStat::Atk, -1)],
                            BoostOrigin::from_move(move_data),
                        );
                        let mon = self.mon_mut(source);
                        if mon.hp > 0 && mon.hp < mon.max_hp.get() {
                            mon.hp = (mon.hp + restored).min(mon.max_hp.get());
                            self.add_health_line("-heal", source, false);
                            success = true;
                        }
                    }
                    if !success {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
                    }
                    succeeded |= success;
                }
                if move_data.name == "Parting Shot" {
                    let lowered = self.boost_from(
                        target,
                        source,
                        &[(BoostStat::Atk, -1), (BoostStat::Spa, -1)],
                        BoostOrigin::from_move(move_data),
                    );
                    if (lowered || self.mon(target).ability == "mirrorarmor")
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
                if move_data.name == "Yawn" {
                    let mon = self.mon(target);
                    if mon.yawn.is_some()
                        || mon.status != Status::Healthy
                        || self.status_immune(target, "slp")
                    {
                        self.move_still(base);
                        self.add(format!("|-fail|{source_name}"));
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
                    "Moonlight" | "Morning Sun" | "Synthesis"
                ) {
                    let fraction = match self.effective_weather() {
                        Some(Weather::Sun) => 2732,
                        Some(_) => 1024,
                        None => 2048,
                    };
                    let tr = self.stat_rules.truncation;
                    let mon = self.mon_mut(target);
                    if mon.hp == mon.max_hp.get() {
                        self.add(format!("|-fail|{}|heal", self.ident(target)));
                    } else {
                        mon.hp = (mon.hp + tr.modify(mon.max_hp.get(), fraction).max(1))
                            .min(mon.max_hp.get());
                        self.add_health_line("-heal", target, false);
                        succeeded = true;
                    }
                }
                if let Some(status) = &move_data.status {
                    succeeded |= self.try_status(target, status, source, move_data, true, base);
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
                let boosts = if move_data.name == "Growth" && weather == Some(Weather::Sun) {
                    &[(BoostStat::Atk, 2), (BoostStat::Spa, 2)]
                } else {
                    move_data.boosts.as_slice()
                };
                if !boosts.is_empty() {
                    succeeded |=
                        self.boost_from(target, source, boosts, BoostOrigin::from_move(move_data));
                }
                if move_data.name == "No Retreat" {
                    let mon = self.mon_mut(target);
                    mon.track_volatile("noretreat");
                    if !mon.volatile_ids().contains(&"trapped") {
                        mon.track_volatile("trapped");
                        self.add(format!("|-activate|{}|trapped", self.ident(target)));
                        self.add(format!("|-start|{}|move: No Retreat", self.ident(target)));
                    }
                    succeeded = true;
                }
                if move_data.name == "Charge" {
                    self.charge_electric(target, None);
                    succeeded = true;
                }
                if succeeded {
                    any_success = true;
                    if spread && !life_dew {
                        hit_labels.push(target.label());
                    }
                }
            }
            if spread {
                if life_dew && !any_success {
                    hit_labels.clear();
                }
                self.log[base]
                    .public
                    .push_str(&format!("|[spread] {}", hit_labels.join(",")));
            }
            if any_success {
                self.each_event();
                if curse && self.mon(source).hp == 0 {
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
            let mon = self.mon_mut(target);
            if mon.hp < mon.max_hp.get() {
                mon.hp = (mon.hp + (mon.max_hp.get() / 2).max(1)).min(mon.max_hp.get());
                self.add_health_line("-heal", target, false);
            }
            self.each_event();
            self.each_event();
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
        let multihit = move_data.multihit.is_some() || beat_up.is_some();
        let (max_hits, per_strike_accuracy) = if let Some(allies) = &beat_up {
            (allies.len() as u8, false)
        } else if ash_shuriken {
            (3, false)
        } else {
            self.hits(source, move_data)
        };
        let mut total = 0;
        let mut landed_by_target = [0u32; 4];
        let mut hp_after_damage = [0u32; 4];
        let mut hits_landed = 0;
        let mut split_darts = [None; 2];
        let sheer_force_removed =
            !delayed && self.sheer_force_removes_secondaries(source, move_data);
        let attack_hazard = if sheer_force_removed {
            None
        } else {
            Hazard::from_attack(&move_data.name)
        };
        let mut hit_substitute = false;
        let binding = super::partial_trap::binding_move(&move_data.name);
        for hit in 1..=max_hits {
            if (self.mon(source).hp == 0 && !sacrifice && !smart_target)
                || spread_targets
                    .iter()
                    .all(|&target| self.mon(target).hp == 0)
            {
                break;
            }
            if hit > 1
                && (matches!(self.mon(source).status, Status::Sleep(_))
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
                let fixed_damage = match move_data.name.as_str() {
                    "Metal Burst" | "Comeuppance" => {
                        retaliation_hit.map(|hit| (hit.damage * 3 / 2).max(1))
                    }
                    "Counter" | "Mirror Coat" => retaliation_hit.map(|hit| (hit.damage * 2).max(1)),
                    "Seismic Toss" | "Night Shade" => Some(u32::from(battle.mon(source).level)),
                    "Super Fang" => Some((battle.mon(target).hp / 2).max(1)),
                    "Endeavor" => Some(battle.mon(target).hp - battle.mon(source).hp),
                    _ => None,
                };
                if let Some(value) = fixed_damage {
                    return value;
                }
                if stacked_power && battle.format.report_exact_hp {
                    battle.add(format!("|debug|BP: {power}"));
                }
                let power = if let Some(allies) = &beat_up {
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
                let mut power = battle.situational_base_power(source, target, move_data, power);
                if move_data.name == "Hex" && battle.mon(target).status != Status::Healthy {
                    if battle.format.report_exact_hp {
                        battle.add("|debug|BP doubled from status condition");
                    }
                    power *= 2;
                }
                if move_data.name == "Stomping Tantrum" && battle.mon(source).move_failed_last_turn
                {
                    if battle.format.report_exact_hp {
                        battle.add(
                            "|debug|doubling Stomping Tantrum BP due to previous move failure",
                        );
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
                let value = if self.event_ability(target) == "sturdy"
                    && mon.hp == mon.max_hp.get()
                    && value >= mon.hp
                {
                    let value = mon.hp - 1;
                    self.add(format!("|-ability|{}|Sturdy", self.ident(target)));
                    value
                } else {
                    value
                };
                let value = self.focus_sash(target, value);
                let half_hp_berry_deferred =
                    matches!(self.event_ability(target), "berserk" | "angershell") && !multihit;
                let turn = self.turn;
                let mon = self.mon_mut(target);
                mon.half_hp_berry_deferred = half_hp_berry_deferred;
                let landed = value.min(mon.hp);
                mon.hp -= landed;
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
                if matches!(move_data.name.as_str(), "Bug Bite" | "Pluck") {
                    self.eat_stolen_berry(source, target, &move_data.name);
                }
                self.charge_after_damage(target, move_data);
            }
            for &(target, _) in &damage {
                if move_data.flags.contains_key("contact")
                    && self.mon(target).volatile_ids().contains(&"beakblast")
                {
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
            if !sheer_force_removed && !move_data.self_boosts.is_empty() {
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
                if sheer_force_removed || move_data.secondaries.is_empty() {
                    break;
                }
                let dust = self.event_ability(target) == "shielddust";
                let cloak = self.mon(target).effective_item() == "covertcloak";
                if self.format.report_exact_hp {
                    if dust {
                        self.add("|debug|Shield Dust prevent secondary");
                    } else if self.mon(target).ability == "shielddust" {
                        self.add("|debug|ModifySecondaries handler suppressed by Mold Breaker");
                    }
                    if cloak {
                        self.add("|debug|Covert Cloak prevent secondary");
                    }
                }
                for effect in &move_data.secondaries {
                    if (dust || cloak) && effect.self_boosts.is_empty() {
                        continue;
                    }
                    let roll = self.rng.below(100);
                    if effect.chance.is_none_or(|chance| roll < u32::from(chance)) {
                        if shielded & target.mask() != 0 {
                            if !effect.self_boosts.is_empty() {
                                self.boost(source, &effect.self_boosts, true);
                            }
                            continue;
                        }
                        if move_data.name == "Dire Claw" {
                            let status = ["psn", "par", "slp"][self.rng.below(3) as usize];
                            self.try_status(target, status, source, move_data, false, base);
                        }
                        if move_data.name == "Alluring Voice" && self.mon(target).stats_raised {
                            self.apply_confusion(target, false);
                        }
                        if let Some(status) = &effect.status {
                            self.try_status(target, status, source, move_data, false, base);
                        }
                        if !effect.boosts.is_empty() {
                            self.boost_from(target, source, &effect.boosts, BoostOrigin::Secondary);
                        }
                        if !effect.self_boosts.is_empty() {
                            self.boost(source, &effect.self_boosts, true);
                        }
                        if effect.volatile_status.as_deref() == Some("flinch")
                            && self.mon(target).hp > 0
                            && self.event_ability(target) != "innerfocus"
                            && !self.mon(target).volatile_ids().contains(&"focuspunch")
                        {
                            self.mon_mut(target).flinched = true;
                            self.mon_mut(target).track_volatile("flinch");
                        }
                        if effect.volatile_status.as_deref() == Some("confusion") {
                            self.apply_confusion(target, false);
                        }
                        if effect.volatile_status.as_deref() == Some("saltcure") {
                            self.start_salt_cure(target);
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
            if move_data.flags.contains_key("contact") {
                for &(target, _) in &damage {
                    if self.mon(target).ability == "aftermath" {
                        self.aftermath(target, source);
                    }
                }
                for &(target, _) in &damage {
                    if self.mon(target).ability == "roughskin" {
                        if self.mon(source).hp > 0 && self.mon(source).ability == "magicguard" {
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
                if move_data.flags.contains_key("contact") {
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
                if self.mon(target).ability == "cursedbody"
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
                if self.event_ability(target) == "stamina" {
                    self.ability_boost(target, "Stamina", BoostStat::Def, 1, false);
                }
                if move_type == "Dark" && self.event_ability(target) == "justified" {
                    self.ability_boost(target, "Justified", BoostStat::Atk, 1, false);
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
        let hint_before_faint = consecutive
            && (self.mon(source).hp == 0
                || damage.iter().any(|&(target, _)| self.mon(target).hp == 0));
        if hint_before_faint {
            self.note_consecutive_move(true, move_data);
        }
        if sacrifice || final_gambit && self.mon(source).hp == 0 {
            self.mark_fainted(source);
        }
        let mut fainted = 0;
        for (target, direct_damage) in damage {
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
        if multihit && hits_landed > 0 && !darts {
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
            && !matches!(self.mon(source).ability.as_str(), "rockhead" | "magicguard")
        {
            ((total * u32::from(num) + u32::from(den) / 2) / u32::from(den)).max(1)
        } else {
            0
        };
        if recoil > 0 && !self.mon(source).fainted {
            let mon = self.mon_mut(source);
            let original_hp = mon.hp;
            if !struggle {
                mon.half_hp_berry_deferred = false;
            }
            mon.hp = mon.hp.saturating_sub(recoil);
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
            && total > 0
        {
            self.half_max_hp_recoil(source, recoil);
        }
        self.each_event();
        // Champions runs the target-side AfterMoveSecondary and EmergencyExit
        // hooks even when Sheer Force removes the move's secondary effects.
        self.target_abilities_after_move(
            source,
            &spread_targets,
            landed_by_target,
            move_data,
            multihit,
            smart_target,
        );
        for &target in &spread_targets {
            let index = target.index();
            self.emergency_exit(target, hp_after_damage[index] + landed_by_target[index]);
        }
        let suppress_after_move = sheer_force_removed && self.event_ability(source) == "sheerforce";
        if hits_landed > 0 && !move_data.after_hit_boosts.is_empty() {
            self.boost(source, &move_data.after_hit_boosts, true);
        }
        if self.mon(source).hp == 0 && !self.mon(source).fainted {
            self.mark_fainted(source);
        }
        let original_source_hp = self.mon(source).hp;
        if !suppress_after_move
            && self.mon(source).effective_item() == "lifeorb"
            && self.active_effects(source)
        {
            self.indirect_damage(source, 10, "|[from] item: Life Orb");
            if self.mon(source).hp == 0 {
                self.mark_fainted(source);
            }
        }
        if !suppress_after_move && hits_landed > 0 {
            self.magician_after_hit(source, &spread_targets);
        }
        self.emergency_exit(source, original_source_hp);
        if !suppress_after_move && move_data.name == "Sparkling Aria" && self.mon(source).hp > 0 {
            for &target in &spread_targets {
                if target != source && self.mon(target).status == Status::Burn {
                    self.cure_status(target, "[msg]");
                }
            }
        }
        if consecutive && !hint_before_faint {
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
        let mon = self.mon_mut(pos);
        if mon.hp != 0 && mon.ability != "magicguard" {
            let original_hp = mon.hp;
            mon.half_hp_berry_deferred = false;
            mon.hp = mon.hp.saturating_sub(mon.max_hp.get().div_ceil(2));
            self.add_health_line_with("-damage", pos, false, &format!("|[from] {effect}"));
            self.emergency_exit(pos, original_hp);
        }
    }

    pub(super) fn mark_fainted(&mut self, target: Position) {
        self.last_faint = Some(target.player);
        let side = self.sides[target.player.index()].as_mut().unwrap();
        side.total_fainted = (side.total_fainted + 1).min(100);
        self.add(format!("|faint|{}", self.ident(target)));
        self.receive_fainted_ability(target);
        self.end_supreme_overlord(target, false);
        self.weather_ability_changed(target);
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
        mon.roosted = false;
        mon.temporary_type = None;
        mon.added_type = None;
        mon.spent_elements = 0;
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
        mon.move_failed_this_turn = false;
        mon.move_failed_last_turn = false;
        mon.last_foe_hit = None;
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
        for &(stat, amount) in boosts {
            let amount = if invert {
                amount.saturating_neg()
            } else {
                amount
            };
            let mon = self.mon_mut(pos);
            let changed = mon.boosts.add(stat, amount);
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
                self.add(format!(
                    "|{event}|{}|{}|{}",
                    self.ident(pos),
                    stat.id(),
                    changed.abs()
                ));
            }
            succeeded |= changed != 0;
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
            self.mon(target).boosts.get(BoostStat::Evasion)
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
        if move_type == "Ground" && mon.effective_item() == "ironball" && mon.has_type("Flying") {
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
                Some(1) => modifier += 1,
                Some(2) => modifier -= 1,
                Some(3) if !ignores_immunity => return None,
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
        Some(modifier)
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
                .or_else(|| self.uproar.map(|state| state.slot))?;
            let m = &self.moves[slot];
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
        if tailwind {
            stat *= 2;
        }
        if self.fainted {
            // A queued replacement still uses the outgoing Pokemon's speed,
            // but Showdown ignores its inactive ability and held item.
            return stat;
        }
        if matches!(
            (self.ability.as_str(), weather),
            ("swiftswim", Some(Weather::Rain))
                | ("chlorophyll", Some(Weather::Sun))
                | ("sandrush", Some(Weather::Sand))
                | ("slushrush", Some(Weather::Snow))
        ) || self.ability == "surgesurfer" && terrain == Some(Terrain::Electric)
            || self.ability == "unburden"
                && self.item.is_empty()
                && self.volatile_ids().contains(&"unburden")
        {
            stat *= 2;
        }
        match self.effective_item() {
            "choicescarf" => stat = crate::mechanics::Truncation::Cartridge.modify(stat, 6144),
            "ironball" => stat /= 2,
            _ => {}
        }
        if self.status == Status::Paralysis {
            stat / 2
        } else {
            stat
        }
    }

    pub(super) fn attack_boosts_type(&self, move_type: &str) -> bool {
        self.hp * 3 <= self.max_hp.get()
            && matches!(
                (self.ability.as_str(), move_type),
                ("blaze", "Fire") | ("torrent", "Water") | ("overgrow", "Grass") | ("swarm", "Bug")
            )
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
                | "airballoon"
                | "lifeorb"
                | "rockyhelmet"
                | "lumberry"
                | "chestoberry"
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
                | "whiteherb"
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
                .is_none_or(|item| item.mega_stone.is_none())
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
