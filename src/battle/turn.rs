use super::*;
use crate::{
    choice::{MoveChoice, SpecialChoice, TargetKind},
    dex::{Accuracy, Category},
    mechanics::{BoostStat, Damage, boosted_stat},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Position {
    pub(super) player: Player,
    pub(super) slot: usize,
}

impl Position {
    fn loc(self, source: Self) -> i8 {
        (self.slot as i8 + 1) * if self.player == source.player { -1 } else { 1 }
    }

    fn label(self) -> String {
        format!("{}{}", self.player.id(), char::from(b'a' + self.slot as u8))
    }
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
        pivot: Option<&'static str>,
    },
    RunSwitch,
    MegaEvolve,
    BeforeTurn,
    Residual,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct Action {
    actor: Option<Position>,
    kind: ActionKind,
    priority: Priority,
}

const STRUGGLE_SLOT: usize = usize::MAX;

impl Action {
    fn new(actor: Option<Position>, kind: ActionKind) -> Self {
        let order = match kind {
            ActionKind::Switch {
                immediate: true, ..
            } => 3,
            ActionKind::BeforeTurn => 4,
            ActionKind::RunSwitch => 101,
            ActionKind::Switch {
                immediate: false, ..
            } => 103,
            ActionKind::MegaEvolve => 104,
            ActionKind::Move { .. } => 200,
            ActionKind::Residual => 300,
        };
        Self {
            actor,
            kind,
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
                    let moves = if !self.any_move_in_request(pos) && mon.revealed_disables == 0 {
                        vec![json!({"move":"Struggle","id":"struggle","target":"randomNormal","disabled":false})]
                    } else {
                        mon.moves.iter().enumerate().map(|(move_slot, m)| {
                            let mut request = json!({
                                "move": m.move_data.name, "id": m.id, "pp": m.pp, "maxpp": m.max_pp,
                                "target": m.move_data.target, "disabled": self.move_disabled_in_request(player, slot, move_slot),
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
        format!("{}: {}", position.label(), self.mon(position).name)
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
        let mon = self.mon(target);
        let condition = health(mon.hp, mon.max_hp, mon.status.id(), false);
        let mut head = format!("|{event}|{}|", self.ident(target));
        if details {
            head.push_str(&mon.details);
            head.push('|');
        }
        self.log.push(LogLine {
            public: format!("{head}{}{suffix}", condition.shared),
            secret: Some((target.player, format!("{head}{}{suffix}", condition.secret))),
        });
    }

    pub(super) fn start_selected(&mut self) {
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
                self.add_health_line("switch", pos, true);
                self.insert_run_switch(pos, &mut queue);
            }
        }
        self.run_queue(queue);
    }

    fn force_switch(&self, player: Player) -> Vec<bool> {
        let party = self.party(player).unwrap();
        let bench = party[self.slots()..].iter().any(|p| !p.fainted);
        party[..self.slots()]
            .iter()
            .map(|p| bench && (p.needs_replacement || p.pivot_move.is_some()))
            .collect()
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
                    if move_slot != STRUGGLE_SLOT && self.actual_disabled(pos, move_slot) {
                        if self.mon(pos).disabled_by_choice(move_slot) {
                            return Err(BattleError::Choice("move is Choice locked"));
                        }
                        return Err(self.reveal_disabled_move(pos, move_slot));
                    }
                    self.cant_undo[idx] |=
                        self.slots() == 1 && self.foe_imprison(pos).next().is_some();
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
                    if let ActionKind::Move {
                        move_slot,
                        target: 0,
                        mega,
                    } = action.kind
                    {
                        let source = action.actor.unwrap();
                        let kind = self.move_data(source, move_slot).target;
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
            self.run_queue(queue);
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
        let needed = flags
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
                SlotChoice::Pass if replace => continue,
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
        // Explicitly enumerate the verified simple moves. Static flags alone
        // cannot prove an arbitrary Showdown move has no additional behavior.
        if !matches!(
            info.id.as_str(),
            "tackle"
                | "quickattack"
                | "swift"
                | "surf"
                | "watergun"
                | "iceshard"
                | "protect"
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
        ) {
            return Err(BattleError::Unsupported(format!("move {}", info.id)));
        }
        let kind = info.move_data.target;
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
                let data = self.move_data(source, move_slot);
                // ModifyPriority receives a resolved target even when no effects
                // listen to it. In doubles this can consume a retarget draw.
                self.resolve_target(source, data.target, target);
                action.priority.priority = f64::from(data.priority);
            }
            action.priority.speed = self.mon(source).current_speed(self.weather()) as i32;
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
        let mut action = Action::new(Some(incoming), ActionKind::RunSwitch);
        self.mon_mut(incoming).speed = self.mon(incoming).current_speed(self.weather());
        self.prioritize(&mut action);
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

    fn run_queue(&mut self, mut queue: Vec<Action>) {
        while !queue.is_empty() {
            let action = queue.remove(0);
            match action.kind {
                ActionKind::MegaEvolve => {
                    let pos = action.actor.unwrap();
                    if !self.mon(pos).fainted {
                        self.mega_evolve(pos);
                    }
                }
                ActionKind::BeforeTurn => {
                    self.ordered_actives(false);
                }
                ActionKind::RunSwitch => {
                    // Simultaneous entry effects use a fresh speed order over the field.
                    let batch_len = queue
                        .iter()
                        .take_while(|q| matches!(q.kind, ActionKind::RunSwitch))
                        .count();
                    let mut switching = [None; 4];
                    switching[0] = action.actor;
                    for (i, action) in queue.drain(..batch_len).enumerate() {
                        switching[i + 1] = action.actor;
                    }
                    let (order, count) = self.ordered_actives(true);
                    for (pos, _) in order.into_iter().take(count) {
                        if switching.contains(&Some(pos)) && self.mon(pos).hp != 0 {
                            self.on_switch_in_hazards(pos);
                            if self.check_win() {
                                return;
                            }
                            if self.mon(pos).hp == 0 {
                                continue;
                            }
                            self.start_ability(pos);
                            self.on_switch_in_item(pos);
                        }
                    }
                }
                ActionKind::Switch {
                    bench_slot, pivot, ..
                } => {
                    let outgoing = action.actor.unwrap();
                    if !self.mon(outgoing).fainted && pivot.is_none() {
                        self.each_event();
                    } // BeforeSwitchOut update
                    let party = &mut self.sides[outgoing.player.index()].as_mut().unwrap().party;
                    party[outgoing.slot].protected = false;
                    party[outgoing.slot].stall = None;
                    party[outgoing.slot].boosts = Boosts::default();
                    party[outgoing.slot].flinched = false;
                    party[outgoing.slot].volatile_order.clear();
                    party[outgoing.slot].imprisoned = false;
                    party[outgoing.slot].choice_lock = None;
                    party[outgoing.slot].active_turns = 0;
                    if !party[outgoing.slot].fainted {
                        party[outgoing.slot].times_attacked = 0;
                    }
                    party[outgoing.slot].needs_replacement = false;
                    party[outgoing.slot].pivot_move = None;
                    if party[outgoing.slot].fainted {
                        party[outgoing.slot].status = Status::Healthy;
                    }
                    party.swap(outgoing.slot, bench_slot);
                    if matches!(party[outgoing.slot].status, Status::Toxic(_)) {
                        party[outgoing.slot].status = Status::Toxic(0);
                    }
                    if let Some(move_name) = pivot {
                        self.add_health_line_with(
                            "switch",
                            outgoing,
                            true,
                            &format!("|[from] {move_name}"),
                        );
                    } else {
                        self.add_health_line("switch", outgoing, true);
                    }
                    self.insert_run_switch(outgoing, &mut queue);
                }
                ActionKind::Move {
                    move_slot, target, ..
                } => {
                    let source = action.actor.unwrap();
                    if self.mon(source).fainted {
                        continue;
                    }
                    let another_action = queue.iter().any(|q| {
                        matches!(q.kind, ActionKind::Move { .. } | ActionKind::Switch { .. })
                    });
                    self.run_move(source, move_slot, target, another_action);
                }
                ActionKind::Residual => self.residual(),
            }
            if self.phase == Phase::Ended || self.check_win() {
                return;
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
                return;
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
            if !mon.fainted {
                mon.active_turns += 1;
            }
            if !mon.holds_choice_item() {
                mon.choice_lock = None;
                mon.untrack_volatile("choicelock");
            }
        }
        self.add(format!("|turn|{}", self.turn));
        self.phase = Phase::Move;
    }

    pub(super) fn start_ability(&mut self, pos: Position) {
        if self.mon(pos).ability == "pressure" {
            self.add(format!("|-ability|{}|Pressure", self.ident(pos)));
        }
        self.switch_in_weather(pos);
        if self.mon(pos).ability == "intimidate" {
            self.intimidate(pos);
        }
    }

    fn positions(&self) -> impl Iterator<Item = Position> + use<> {
        let slots = self.slots();
        Player::ALL
            .into_iter()
            .flat_map(move |player| (0..slots).map(move |slot| Position { player, slot }))
    }

    pub(super) fn ordered_actives(
        &mut self,
        include_fainted: bool,
    ) -> ([(Position, u32); 4], usize) {
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

    fn update_speed(&mut self) {
        for pos in self.positions() {
            if !self.mon(pos).fainted {
                self.mon_mut(pos).speed = self.mon(pos).current_speed(self.weather());
            }
        }
    }

    pub(super) fn each_event(&mut self) {
        let (order, count) = self.ordered_actives(false);
        for (pos, _) in order.into_iter().take(count) {
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
            Leftovers,
            Weather,
            SpeedBoost,
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
        for pos in self.positions() {
            let mon = self.mon(pos);
            let priority = |order, sub_order| Priority {
                order,
                sub_order,
                speed: mon.speed as i32,
                ..Priority::default()
            };
            if let Some((order, _, _)) = mon.status.residual() {
                timers.push((pos, Effect::Status, priority(order, 0)));
            }
            if mon.ability == "speedboost" {
                timers.push((pos, Effect::SpeedBoost, priority(28, 2)));
            }
            if mon.item == "leftovers" {
                timers.push((pos, Effect::Leftovers, priority(5, 4)));
            }
            if mon.protected {
                timers.push((pos, Effect::Protect, priority(0, 2)));
            }
            if mon.stall.is_some() {
                timers.push((pos, Effect::Stall, priority(0, 2)));
            }
            if mon.flinched {
                timers.push((pos, Effect::Flinch, priority(0, 2)));
            }
        }
        speed_sort_by(&mut timers, &mut self.rng, |a, b| a.2.compare(b.2));
        for (pos, effect, _) in timers {
            if matches!(effect, Effect::Weather) {
                self.weather_residual();
                if self.check_win() {
                    return;
                }
                continue;
            }
            let mon = self.mon_mut(pos);
            if mon.fainted {
                continue;
            }
            match effect {
                Effect::Leftovers => {
                    let amount = mon.max_hp.get() / 16;
                    self.item_heal(pos, amount, "Leftovers");
                }
                Effect::Weather => unreachable!(),
                Effect::SpeedBoost => self.speed_boost(pos),
                Effect::Protect => {
                    mon.protected = false;
                    mon.untrack_volatile("protect");
                }
                Effect::Flinch => {
                    mon.flinched = false;
                    mon.untrack_volatile("flinch");
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
        }
        self.add("|upkeep");
    }

    fn check_win(&mut self) -> bool {
        let alive = Player::ALL.map(|p| self.party(p).unwrap().iter().any(|mon| !mon.fainted));
        if alive == [true, true] {
            return false;
        }
        self.winner = match alive {
            [true, false] => Some(Player::P1),
            [false, true] => Some(Player::P2),
            _ => self.last_faint,
        };
        self.add("|");
        if let Some(winner) = self.winner {
            self.add(format!(
                "|win|{}",
                self.sides[winner.index()].as_ref().unwrap().name
            ));
        } else {
            self.add("|tie");
        }
        self.phase = Phase::Ended;
        true
    }

    fn run_move(&mut self, source: Position, slot: usize, loc: i8, more_actions: bool) {
        let move_data = self.move_data(source, slot);
        let struggle = slot == STRUGGLE_SLOT;
        let move_type = if struggle {
            "???"
        } else {
            move_data.move_type.as_str()
        };
        let source_name = self.ident(source);
        let target = self.resolve_target(source, move_data.target, loc);
        if !self.before_status_move(source, move_data) {
            return;
        }
        if matches!(self.mon(source).status, Status::Freeze(_))
            && move_data.flags.contains_key("defrost")
        {
            self.cure_status(source, &format!("[from] move: {}", move_data.name));
        }
        if struggle {
            self.add(format!("|-activate|{source_name}|move: Struggle"));
        } else {
            let mon = self.mon_mut(source);
            mon.moves[slot].pp -= 1;
            if mon.holds_choice_item() && mon.choice_lock.is_none() {
                mon.choice_lock = Some(slot);
                mon.track_volatile("choicelock");
            }
        }
        let mut spread_targets = Vec::new();
        match move_data.target {
            TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes => {
                if move_data.target == TargetKind::AllAdjacent && self.slots() == 2 {
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
            _ => spread_targets.extend(target.filter(|pos| !self.mon(*pos).fainted)),
        }
        let actual_target = if target.is_some_and(|target| spread_targets.contains(&target)) {
            target
        } else {
            spread_targets.last().copied().or(target)
        };
        let target_name = actual_target.map_or_else(|| "null".into(), |pos| self.ident(pos));
        let base = self.log.len();
        self.add(format!(
            "|move|{source_name}|{}|{target_name}",
            move_data.name
        ));
        if spread_targets.is_empty() {
            self.log[base].public.push_str("|[notarget]");
            self.add(format!("|-fail|{source_name}"));
            return;
        }

        if !struggle
            && (move_data.target.is_choosable()
                || matches!(
                    move_data.target,
                    TargetKind::AllAdjacent | TargetKind::AllAdjacentFoes
                ))
        {
            let pressure_targets = spread_targets
                .iter()
                .filter(|pos| pos.player != source.player && self.mon(**pos).ability == "pressure")
                .count();
            self.mon_mut(source).moves[slot].pp = self.mon(source).moves[slot]
                .pp
                .saturating_sub(pressure_targets as u8);
        }
        if let Some(hazard) = Hazard::from_move(&move_data.name) {
            if move_data.flags.contains_key("mustpressure") {
                let pressure = (0..self.slots())
                    .map(|slot| Position {
                        player: source.player.foe(),
                        slot,
                    })
                    .filter(|&p| !self.mon(p).fainted && self.mon(p).ability == "pressure")
                    .count() as u8;
                self.mon_mut(source).moves[slot].pp =
                    self.mon(source).moves[slot].pp.saturating_sub(pressure);
            }
            if !self.add_hazard(source.player.foe(), hazard) {
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
            }
            return;
        }
        if let Some(weather) = Weather::from_move(&move_data.name) {
            if !self.set_weather(source, weather, None) {
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
            }
            return;
        }
        if move_data.name == "Imprison" {
            let mon = self.mon_mut(source);
            if mon.imprisoned {
                self.log[base].public = format!("|move|{source_name}|Imprison||[still]");
                self.add(format!("|-fail|{source_name}"));
            } else {
                mon.imprisoned = true;
                mon.track_volatile("imprison");
                self.add(format!("|-start|{source_name}|move: Imprison"));
                self.each_event();
                self.each_event();
            }
            return;
        }
        if move_data.name == "Protect" {
            let stall = self.mon(source).stall.map_or(1, |(counter, _)| counter);
            if !more_actions || (stall > 1 && !self.rng.chance(1, u32::from(stall))) {
                if more_actions {
                    self.mon_mut(source).stall = None;
                    self.mon_mut(source).untrack_volatile("stall");
                }
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
                return;
            }
            let mon = self.mon_mut(source);
            mon.protected = true;
            mon.track_volatile("protect");
            mon.track_volatile("stall");
            mon.stall = Some(((stall * 3).min(729), 2));
            self.add(format!("|-singleturn|{source_name}|Protect"));
            self.each_event();
            self.each_event();
            return;
        }

        let spread = spread_targets.len() > 1;
        // Preserve upstream's hit-step batching: all protection and immunity
        // checks, then all accuracy rolls, then all damage calculations.
        spread_targets.retain(|&mut_pos| {
            if self.mon(mut_pos).protected && move_data.flags.contains_key("protect") {
                self.add(format!("|-activate|{}|move: Protect", self.ident(mut_pos)));
                false
            } else if move_data.category == Category::Status
                && source != mut_pos
                && self.mon(mut_pos).ability == "goodasgold"
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
        spread_targets.retain(|&pos| {
            let ignores_type = move_data
                .ignore_immunity
                .as_bool()
                .unwrap_or(move_data.category == Category::Status);
            if !ignores_type
                && move_data.move_type == "Ground"
                && self.mon(pos).ability == "levitate"
                && !self.mon(pos).species.types.iter().any(|t| t == "Flying")
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Levitate",
                    self.ident(pos)
                ));
                false
            } else if (!ignores_type
                && (self.effectiveness(move_data, pos).is_none()
                    || (move_data.move_type == "Ground" && self.mon(pos).item == "airballoon")))
                || (move_data.flags.contains_key("powder")
                    && source != pos
                    && self.status_immune(pos, "powder"))
            {
                self.add(format!("|-immune|{}", self.ident(pos)));
                false
            } else {
                true
            }
        });
        spread_targets.retain(|&pos| {
            if let Accuracy::Percent(accuracy) = move_data.accuracy
                && !(move_data.target == TargetKind::User && move_data.category == Category::Status)
                && !(move_data.name == "Toxic"
                    && self.mon(source).species.types.iter().any(|t| t == "Poison"))
                && !self.rng.chance(self.accuracy(source, pos, accuracy), 100)
            {
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
            return;
        }

        if move_data.category == Category::Status {
            let mut succeeded = false;
            for target in spread_targets {
                if let Some(status) = &move_data.status {
                    succeeded |= self.try_status(target, status, source, move_data, true, base);
                }
                if let Some([num, den]) = move_data.heal {
                    let mon = self.mon_mut(target);
                    if mon.hp == mon.max_hp.get() {
                        self.log[base].public =
                            format!("|move|{source_name}|{}||[still]", move_data.name);
                        self.add(format!("|-fail|{}|heal", self.ident(target)));
                        continue;
                    }
                    let amount =
                        (mon.max_hp.get() * u32::from(num) + u32::from(den) / 2) / u32::from(den);
                    mon.hp = (mon.hp + amount).min(mon.max_hp.get());
                    self.add_health_line("-heal", target, false);
                    succeeded = true;
                }
                if !move_data.boosts.is_empty() {
                    succeeded |= self.boost(target, &move_data.boosts, false);
                }
            }
            if succeeded {
                self.each_event();
                self.each_event();
            }
            return;
        }

        let tr = self.format.stat_rules().truncation;
        let mut damage = Vec::with_capacity(spread_targets.len());
        for &target in &spread_targets {
            let effect = self.effectiveness(move_data, target).unwrap();
            let crit_ratio = move_data.crit_ratio.unwrap_or(1).min(4);
            let crit = crit_ratio != 0
                && self
                    .rng
                    .chance(1, [0, 24, 8, 2, 1][usize::from(crit_ratio)])
                && !matches!(
                    self.mon(target).ability.as_str(),
                    "battlearmor" | "shellarmor"
                );
            let user = self.mon(source);
            let mon = self.mon(target);
            let (atk, atk_stage, def, def_stage) = if move_data.category == Category::Physical {
                (
                    user.stats.atk,
                    user.boosts.get(BoostStat::Atk),
                    mon.stats.def,
                    mon.boosts.get(BoostStat::Def),
                )
            } else {
                (
                    user.stats.spa,
                    user.boosts.get(BoostStat::Spa),
                    mon.stats.spd,
                    mon.boosts.get(BoostStat::Spd),
                )
            };
            let atk = boosted_stat(atk, if crit { atk_stage.max(0) } else { atk_stage });
            let def = boosted_stat(def, if crit { def_stage.min(0) } else { def_stage });
            let def = if (self.weather() == Some(Weather::Sand)
                && move_data.category == Category::Special
                && mon.species.types.iter().any(|t| t == "Rock"))
                || (self.weather() == Some(Weather::Snow)
                    && move_data.category == Category::Physical
                    && mon.species.types.iter().any(|t| t == "Ice"))
            {
                tr.modify(def, 6144)
            } else {
                def
            };
            let atk = tr.modify(atk, user.attack_modifier(move_data.category, move_type));
            let stab = user.has_type(move_type);
            let mut power_modifier = crate::mechanics::chain(
                user.ability_power_modifier(move_data, move_type, self.weather()),
                user.item_power_modifier(move_type),
            );
            if move_data.name == "Knock Off" && mon.can_take_item(self.format.dex_mod).is_some() {
                power_modifier = crate::mechanics::chain(power_modifier, 6144);
            }
            let mut final_modifier = if user.item == "lifeorb" { 5324 } else { 4096 };
            if mon.ability == "multiscale" && mon.hp == mon.max_hp.get() {
                final_modifier = crate::mechanics::chain(final_modifier, 2048);
            }
            let value = Damage {
                level: user.level,
                power: tr.modify(u32::from(move_data.base_power), power_modifier),
                attack: atk,
                defense: NonZeroU32::new(def.max(1)).unwrap(),
                spread_or_bond: if spread { 3072 } else { 4096 },
                weather: self
                    .weather()
                    .map_or(4096, |w| w.damage_modifier(move_type)),
                critical: crit,
                stab: if !stab {
                    4096
                } else if user.ability == "adaptability" {
                    8192
                } else {
                    6144
                },
                effectiveness: effect,
                burned: user.status == Status::Burn
                    && user.ability != "guts"
                    && move_data.category == Category::Physical,
                final_modifier,
                bypassed_protect: false,
            }
            .roll(tr, &mut self.rng);
            if effect > 0 {
                self.add(format!(
                    "|-supereffective|{}|{}",
                    self.ident(target),
                    effect.min(2)
                ));
            } else if effect < 0 {
                self.add(format!(
                    "|-resisted|{}|{}",
                    self.ident(target),
                    (-effect).min(2)
                ));
            }
            if crit {
                self.add(format!("|-crit|{}", self.ident(target)));
            }
            damage.push((target, value));
        }
        for &(target, value) in &damage {
            let value = self.focus_sash(target, value);
            let mon = self.mon_mut(target);
            mon.hp = mon.hp.saturating_sub(value);
            self.add_health_line("-damage", target, false);
        }
        if !move_data.self_boosts.is_empty() {
            let roll = self.rng.below(100);
            if move_data
                .self_chance
                .is_none_or(|chance| roll < u32::from(chance))
            {
                self.boost(source, &move_data.self_boosts, true);
            }
        }
        for &(target, _) in &damage {
            for effect in &move_data.secondaries {
                let roll = self.rng.below(100);
                if effect.chance.is_none_or(|chance| roll < u32::from(chance)) {
                    if let Some(status) = &effect.status {
                        self.try_status(target, status, source, move_data, false, base);
                    }
                    if !effect.boosts.is_empty() {
                        self.boost(target, &effect.boosts, true);
                    }
                    if effect.volatile_status.as_deref() == Some("flinch")
                        && self.mon(target).hp > 0
                    {
                        self.mon_mut(target).flinched = true;
                        self.mon_mut(target).track_volatile("flinch");
                    }
                }
            }
        }
        for &(target, _) in &damage {
            if move_data.move_type == "Fire" && matches!(self.mon(target).status, Status::Freeze(_))
            {
                self.cure_status(target, "[msg]");
            }
        }
        if move_data.flags.contains_key("contact") {
            for &(target, _) in &damage {
                if self.mon(target).ability == "roughskin" {
                    self.indirect_damage(
                        source,
                        8,
                        &format!("|[from] ability: Rough Skin|[of] {}", self.ident(target)),
                    );
                }
            }
            for &(target, _) in &damage {
                if self.mon(target).item == "rockyhelmet" {
                    self.indirect_damage(
                        source,
                        6,
                        &format!("|[from] item: Rocky Helmet|[of] {}", self.ident(target)),
                    );
                }
            }
        }
        for &(target, _) in &damage {
            if self.mon(target).item == "airballoon" {
                self.consume(target, "Air Balloon", false);
            }
        }
        if move_data.name == "Knock Off" {
            for &(target, _) in &damage {
                if let Some(item) = self.mon(target).can_take_item(self.format.dex_mod) {
                    self.mon_mut(target).item.clear();
                    self.add(format!(
                        "|-enditem|{}|{item}|[from] move: Knock Off|[of] {source_name}",
                        self.ident(target)
                    ));
                } else if self.mon(target).ability == "stickyhold"
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
        self.each_event();
        for (target, _) in damage {
            if self.mon(target).hp == 0 {
                self.mark_fainted(target);
            }
            if target != source {
                self.mon_mut(target).times_attacked += 1;
            }
        }
        if self.mon(source).hp == 0 {
            self.mark_fainted(source);
            self.check_win();
        }
        if struggle && !self.mon(source).fainted {
            let mon = self.mon_mut(source);
            let recoil = ((mon.max_hp.get() + 2) / 4).max(1);
            mon.hp = mon.hp.saturating_sub(recoil);
            self.add_health_line_with("-damage", source, false, "|[from] recoil");
        }
        self.each_event();
        if self.mon(source).hp == 0 && !self.mon(source).fainted {
            self.mark_fainted(source);
        }
        if self.mon(source).item == "lifeorb" && !self.mon(source).fainted {
            self.indirect_damage(source, 10, "|[from] item: Life Orb");
            if self.mon(source).hp == 0 {
                self.mark_fainted(source);
            }
        }
        if !self.mon(source).fainted && self.first_bench(source.player, 0).is_some() {
            let pivot = match move_data.name.as_str() {
                "U-turn" => Some("U-turn"),
                "Volt Switch" => Some("Volt Switch"),
                "Flip Turn" => Some("Flip Turn"),
                _ => None,
            };
            self.mon_mut(source).pivot_move = pivot;
        }
        if spread {
            let labels: Vec<_> = spread_targets.into_iter().map(Position::label).collect();
            self.log[base]
                .public
                .push_str(&format!("|[spread] {}", labels.join(",")));
        }
    }

    pub(super) fn mark_fainted(&mut self, target: Position) {
        self.last_faint = Some(target.player);
        let mon = self.mon_mut(target);
        mon.fainted = true;
        mon.pivot_move = None;
        mon.stall = None;
        mon.protected = false;
        mon.flinched = false;
        mon.volatile_order.clear();
        mon.imprisoned = false;
        mon.choice_lock = None;
        mon.times_attacked = 0;
        mon.boosts = Boosts::default();
        self.add(format!("|faint|{}", self.ident(target)));
    }

    pub(super) fn boost(
        &mut self,
        pos: Position,
        boosts: &[(BoostStat, i8)],
        is_self: bool,
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
            let mon = self.mon_mut(pos);
            let changed = mon.boosts.add(stat, amount);
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

    fn accuracy(&self, source: Position, target: Position, accuracy: u8) -> u32 {
        let accuracy = i32::from(accuracy);
        let boost = (i32::from(self.mon(source).boosts.get(BoostStat::Accuracy))
            - i32::from(self.mon(target).boosts.get(BoostStat::Evasion)))
        .clamp(-6, 6);
        (accuracy * (3 + boost.max(0)) / (3 - boost.min(0))) as u32
    }

    pub(super) fn effectiveness(&self, move_data: &Move, target: Position) -> Option<i8> {
        if move_data.name == "Struggle" {
            return Some(0);
        }
        let mut modifier = 0;
        for typing in &self.mon(target).species.types {
            let case = DEX
                .type_data(&to_id(typing))
                .unwrap()
                .damage_taken
                .get(&move_data.move_type);
            match case {
                Some(1) => modifier += 1,
                Some(2) => modifier -= 1,
                Some(3) => return None,
                _ => {}
            }
        }
        Some(modifier)
    }
}

impl Pokemon {
    fn current_speed(&self, weather: Option<Weather>) -> u32 {
        let mut stat = boosted_stat(self.stats.spe, self.boosts.get(BoostStat::Spe));
        if matches!(
            (self.ability.as_str(), weather),
            ("swiftswim", Some(Weather::Rain))
                | ("chlorophyll", Some(Weather::Sun))
                | ("sandrush", Some(Weather::Sand))
                | ("slushrush", Some(Weather::Snow))
        ) {
            stat *= 2;
        }
        if self.item == "choicescarf" {
            stat = crate::mechanics::Truncation::Cartridge.modify(stat, 6144);
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
                | "speedboost"
                | "multiscale"
                | "guts"
                | "adaptability"
                | "technician"
                | "ironfist"
                | "sharpness"
                | "toughclaws"
                | "stickyhold"
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
                | "softsand"
                | "magnet"
        ) && DEX
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
