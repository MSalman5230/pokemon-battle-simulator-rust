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
    pub(super) fn loc(self, source: Self) -> i8 {
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
const RECHARGE_SLOT: usize = usize::MAX - 1;

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
        let condition = health(
            mon.hp,
            mon.max_hp,
            mon.status.id(),
            self.format.report_exact_hp,
        );
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
                        && !self.mon(action.actor.unwrap()).hard_locked()
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
                target: 0,
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
            )
        {
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
                // ModifyPriority receives a resolved target even when no effects
                // listen to it. In doubles this can consume a retarget draw.
                if move_slot == RECHARGE_SLOT {
                    self.random_target(source, TargetKind::Normal);
                    action.priority.priority = 0.0;
                } else {
                    let data = self.move_data(source, move_slot);
                    self.resolve_target(source, data.target, target);
                    action.priority.priority = f64::from(data.priority);
                }
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
        self.reset_ability_order(incoming);
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
                        if switching.contains(&Some(pos))
                            && self.mon(pos).hp > 0
                            && self.mon(pos).ability == "unnerve"
                        {
                            self.add(format!("|-ability|{}|Unnerve", self.ident(pos)));
                        }
                    }
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
                    party[outgoing.slot].recharge = 0;
                    party[outgoing.slot].confusion = 0;
                    party[outgoing.slot].rampage = None;
                    party[outgoing.slot].glaive_drawback = false;
                    party[outgoing.slot].volatile_order.clear();
                    party[outgoing.slot].imprisoned = false;
                    party[outgoing.slot].choice_lock = None;
                    party[outgoing.slot].active_turns = 0;
                    party[outgoing.slot].active_move_actions = 0;
                    party[outgoing.slot].roosted = false;
                    party[outgoing.slot].temporary_type = None;
                    party[outgoing.slot].yawn = 0;
                    party[outgoing.slot].taunt = 0;
                    party[outgoing.slot].attract = None;
                    party[outgoing.slot].disabled_slot = None;
                    party[outgoing.slot].last_move_slot = None;
                    party[outgoing.slot].acted = false;
                    if !party[outgoing.slot].fainted {
                        party[outgoing.slot].times_attacked = 0;
                    }
                    party[outgoing.slot].needs_replacement = false;
                    party[outgoing.slot].pivot_move = None;
                    party[outgoing.slot].reset_ability();
                    if party[outgoing.slot].fainted {
                        party[outgoing.slot].status = Status::Healthy;
                    }
                    party.swap(outgoing.slot, bench_slot);
                    party[outgoing.slot].used_moves = 0;
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
                    self.mon_mut(source).acted = true;
                    self.mon_mut(source).active_move_actions += 1;
                    if self.run_move(source, move_slot, target, another_action) {
                        self.end_rampage_attempt(source);
                    }
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
            mon.acted = false;
            if !mon.fainted {
                mon.active_turns += 1;
            }
            if !mon.holds_choice_item() {
                mon.choice_lock = None;
                mon.untrack_volatile("choicelock");
            }
            self.refresh_observed_disables(pos);
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
            self.attract_update(pos);
            self.try_trace(pos);
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
            Recharge,
            Rampage,
            Leftovers,
            ShedSkin,
            Weather,
            SpeedBoost,
            Roost,
            Yawn,
            Taunt,
            Disable,
            Screen(Screen),
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
        for player in Player::ALL {
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
                speed: mon.speed as i32,
                ..Priority::default()
            };
            if let Some((order, _, _)) = mon.status.residual() {
                timers.push((pos, Effect::Status, priority(order, 0)));
            }
            if mon.ability == "speedboost" {
                timers.push((pos, Effect::SpeedBoost, priority(28, 2)));
            }
            if mon.disabled_slot.is_some() {
                timers.push((pos, Effect::Disable, priority(17, 2)));
            }
            if mon.taunt > 0 {
                timers.push((pos, Effect::Taunt, priority(15, 2)));
            }
            if mon.yawn > 0 {
                timers.push((pos, Effect::Yawn, priority(23, 2)));
            }
            if mon.roosted {
                timers.push((pos, Effect::Roost, priority(25, 2)));
            }
            if mon.item == "leftovers" {
                timers.push((pos, Effect::Leftovers, priority(5, 4)));
            }
            if mon.ability == "shedskin" {
                timers.push((pos, Effect::ShedSkin, priority(5, 3)));
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
            if mon.recharge != 0 {
                timers.push((pos, Effect::Recharge, priority(0, 2)));
            }
            if mon.rampage.is_some() {
                timers.push((pos, Effect::Rampage, priority(0, 2)));
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
            if let Effect::Screen(screen) = effect {
                self.tick_screen(pos.player, screen);
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
                Effect::Weather | Effect::Screen(_) => unreachable!(),
                Effect::Yawn => self.tick_yawn(pos),
                Effect::Disable => self.tick_disable(pos),
                Effect::Taunt => self.tick_taunt(pos),
                Effect::ShedSkin => self.shed_skin(pos),
                Effect::SpeedBoost => self.speed_boost(pos),
                Effect::Roost => {
                    mon.roosted = false;
                    mon.untrack_volatile("roost");
                }
                Effect::Protect => {
                    mon.protected = false;
                    mon.untrack_volatile("protect");
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

    // Whether this action reached the move attempt, for AfterMove hooks.
    fn run_move(&mut self, source: Position, slot: usize, loc: i8, more_actions: bool) -> bool {
        let mon = self.mon_mut(source);
        if mon.glaive_drawback {
            mon.glaive_drawback = false;
            mon.untrack_volatile("glaiverush");
            if self.format.report_exact_hp {
                self.add("|debug|removing Glaive Rush drawback before attack");
            }
        }
        if slot == RECHARGE_SLOT {
            self.random_target(source, TargetKind::Normal);
            self.add(format!("|cant|{}|recharge", self.ident(source)));
            let mon = self.mon_mut(source);
            mon.recharge = 0;
            mon.untrack_volatile("mustrecharge");
            return true;
        }
        let move_data = self.move_data(source, slot);
        let struggle = slot == STRUGGLE_SLOT;
        let rampage = self.mon(source).rampage.is_some();
        let called_from = if rampage { "|[from] lockedmove" } else { "" };
        let weather = self.weather();
        let weather_ball = move_data.name == "Weather Ball";
        let power = if weather_ball && weather.is_some() {
            move_data.base_power * 2
        } else {
            move_data.base_power
        };
        let (move_type, converted) = if struggle {
            ("???", false)
        } else if weather_ball {
            (weather.map_or("Normal", Weather::ball_type), false)
        } else {
            self.mon(source).converted_move_type(&move_data.move_type)
        };
        let source_name = self.ident(source);
        let target = self.resolve_target(source, move_data.target, loc);
        if !self.before_status_move(source, move_data) {
            return false;
        }
        let accuracy = match (move_data.name.as_str(), weather) {
            ("Blizzard", Some(Weather::Snow)) => Accuracy::Bypass(true),
            ("Hurricane" | "Thunder", Some(Weather::Rain)) if target.is_some() => {
                Accuracy::Bypass(true)
            }
            ("Hurricane" | "Thunder", Some(Weather::Sun)) if target.is_some() => {
                Accuracy::Percent(50)
            }
            _ => move_data.accuracy,
        };
        if matches!(self.mon(source).status, Status::Freeze(_))
            && move_data.flags.contains_key("defrost")
        {
            self.cure_status(source, &format!("[from] move: {}", move_data.name));
        }
        if struggle {
            self.mon_mut(source).last_move_slot = None;
            self.add(format!("|-activate|{source_name}|move: Struggle"));
        } else {
            let mon = self.mon_mut(source);
            if !rampage {
                mon.moves[slot].pp -= 1;
                mon.used_moves |= 1 << slot;
            }
            mon.last_move_slot = Some(slot);
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
        if weather_ball && self.format.report_exact_hp {
            self.add(format!("|debug|BP: {power}"));
        }
        let base = self.log.len();
        self.add(format!(
            "|move|{source_name}|{}|{target_name}{called_from}",
            move_data.name
        ));
        if self.slots() > 1
            && let Some(target) = actual_target.filter(|_| {
                move_data.target.is_choosable() || move_data.target == TargetKind::RandomNormal
            })
            && let Some(redirect) = self.redirect_elemental(source, target, move_data, move_type)
        {
            spread_targets.clear();
            spread_targets.push(redirect);
            self.log[base].public = format!(
                "|move|{source_name}|{}|{}{called_from}",
                move_data.name,
                self.ident(redirect)
            );
        }
        if spread_targets.is_empty() {
            self.log[base].public.push_str("|[notarget]");
            self.add(format!("|-fail|{source_name}"));
            return true;
        }

        if !struggle
            && !rampage
            && (move_data.target.is_choosable()
                || matches!(
                    move_data.target,
                    TargetKind::AllAdjacent
                        | TargetKind::AllAdjacentFoes
                        | TargetKind::RandomNormal
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
        if let Some(screen) = Screen::from_move(&move_data.name) {
            if !matches!(screen, Screen::AuroraVeil) || self.weather() == Some(Weather::Snow) {
                self.protean_prepare(source, move_type);
            }
            if !self.set_screen(source, screen) {
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
            }
            return true;
        }
        if let Some(hazard) = Hazard::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
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
            return true;
        }
        if move_data.name == "Court Change" {
            self.protean_prepare(source, move_type);
            if !self.court_change(source) {
                self.log[base].public = format!("|move|{source_name}|Court Change||[still]");
                self.add(format!("|-fail|{source_name}"));
            }
            return true;
        }
        if let Some(weather) = Weather::from_move(&move_data.name) {
            self.protean_prepare(source, move_type);
            if !self.set_weather(source, weather, None) {
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
            }
            return true;
        }
        if move_data.name == "Imprison" {
            self.protean_prepare(source, move_type);
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
            return true;
        }
        if move_data.name == "Rest" {
            let mon = self.mon(source);
            if mon.hp == mon.max_hp.get() {
                self.add(format!("|-fail|{source_name}|heal"));
            } else if self.status_immune(source, "slp") {
                let ability = if mon.ability == "insomnia" {
                    "Insomnia"
                } else {
                    "Vital Spirit"
                };
                self.add(format!(
                    "|-fail|{source_name}|[from] ability: {ability}|[of] {source_name}"
                ));
            } else {
                self.protean_prepare(source, move_type);
                self.apply_status(source, "slp", Some("Rest"));
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
        if move_data.name == "Protect" {
            let stall = self.mon(source).stall.map_or(1, |(counter, _)| counter);
            if !more_actions || (stall > 1 && !self.rng.chance(1, u32::from(stall))) {
                if more_actions {
                    self.mon_mut(source).stall = None;
                    self.mon_mut(source).untrack_volatile("stall");
                }
                self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
                self.add(format!("|-fail|{source_name}"));
                return true;
            }
            self.protean_prepare(source, move_type);
            let mon = self.mon_mut(source);
            mon.protected = true;
            mon.track_volatile("protect");
            mon.track_volatile("stall");
            mon.stall = Some(((stall * 3).min(729), 2));
            self.add(format!("|-singleturn|{source_name}|Protect"));
            self.each_event();
            self.each_event();
            return true;
        }

        if self.mon(source).active_move_actions > 1
            && matches!(move_data.name.as_str(), "Fake Out" | "First Impression")
        {
            self.add(format!(
                "|-hint|{} only works on your first turn out.",
                move_data.name
            ));
            self.log[base].public = format!("|move|{source_name}|{}||[still]", move_data.name);
            self.add(format!("|-fail|{source_name}"));
            return true;
        }
        if move_data.name == "Poltergeist" && self.mon(spread_targets[0]).item.is_empty() {
            self.log[base].public = format!("|move|{source_name}|Poltergeist||[still]");
            self.add(format!("|-fail|{source_name}"));
            return true;
        }
        self.protean_prepare(source, move_type);
        let spread = spread_targets.len() > 1;
        // Preserve upstream's hit-step batching: all protection and immunity
        // checks, then all accuracy rolls, then all damage calculations.
        spread_targets.retain(|&mut_pos| {
            if self.mon(mut_pos).protected && move_data.flags.contains_key("protect") {
                self.add(format!("|-activate|{}|move: Protect", self.ident(mut_pos)));
                false
            } else if self.absorb_elemental(mut_pos, source, move_type) {
                false
            } else if self.mon(mut_pos).ability == "oblivious" && move_data.name == "Taunt" {
                self.add(format!(
                    "|-immune|{}|[from] ability: Oblivious",
                    self.ident(mut_pos)
                ));
                false
            } else if source != mut_pos
                && move_data.flags.contains_key("sound")
                && self.mon(mut_pos).ability == "soundproof"
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Soundproof",
                    self.ident(mut_pos)
                ));
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
                && move_type == "Ground"
                && self.mon(pos).ability == "levitate"
                && !self.mon(pos).has_type("Flying")
            {
                self.add(format!(
                    "|-immune|{}|[from] ability: Levitate",
                    self.ident(pos)
                ));
                false
            } else if (!ignores_type
                && (self.effectiveness_of(move_data, move_type, pos).is_none()
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
        if move_data.name == "Disable"
            && spread_targets
                .iter()
                .any(|&p| self.mon(p).last_move_slot.is_none())
        {
            self.log[base].public = format!("|move|{source_name}|Disable||[still]");
            self.add(format!("|-fail|{source_name}"));
            return true;
        }
        spread_targets.retain(|&pos| {
            if let Accuracy::Percent(accuracy) = accuracy
                && !move_data.always_hit
                && !(move_data.target == TargetKind::User && move_data.category == Category::Status)
                && !(move_data.name == "Toxic" && self.mon(source).has_type("Poison"))
                && self.mon(source).ability != "noguard"
                && self.mon(pos).ability != "noguard"
                && !self.mon(pos).glaive_drawback
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
            return true;
        }

        if move_data.category == Category::Status {
            let mut succeeded = false;
            for target in spread_targets {
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
                        success =
                            self.boost_from(target, source, &[(BoostStat::Atk, -1)], false, false);
                        let mon = self.mon_mut(source);
                        if mon.hp > 0 && mon.hp < mon.max_hp.get() {
                            mon.hp = (mon.hp + restored).min(mon.max_hp.get());
                            self.add_health_line("-heal", source, false);
                            success = true;
                        }
                    }
                    if !success {
                        self.log[base].public =
                            format!("|move|{source_name}|Strength Sap||[still]");
                        self.add(format!("|-fail|{source_name}"));
                    }
                    succeeded |= success;
                }
                if move_data.name == "Taunt" {
                    if self.taunt(target) {
                        succeeded = true;
                    } else {
                        self.log[base].public = format!("|move|{source_name}|Taunt||[still]");
                        self.add(format!("|-fail|{source_name}"));
                    }
                }
                if move_data.name == "Disable" {
                    if !self.disable_last_move(target, source, false) {
                        self.log[base].public = format!("|move|{source_name}|Disable||[still]");
                        self.add(format!("|-fail|{source_name}"));
                    } else {
                        succeeded = true;
                    }
                }
                if move_data.name == "Yawn" {
                    let mon = self.mon(target);
                    if mon.yawn != 0
                        || mon.status != Status::Healthy
                        || self.status_immune(target, "slp")
                    {
                        self.log[base].public = format!("|move|{source_name}|Yawn||[still]");
                        self.add(format!("|-fail|{source_name}"));
                        continue;
                    }
                    let mon = self.mon_mut(target);
                    mon.yawn = 2;
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
                    let fraction = match self.weather() {
                        Some(Weather::Sun) => 2732,
                        Some(_) => 1024,
                        None => 2048,
                    };
                    let tr = self.format.stat_rules().truncation;
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
                    succeeded |= self.boost_from(target, source, boosts, false, false);
                }
            }
            if succeeded {
                self.each_event();
                self.each_event();
            }
            return true;
        }

        let tr = self.format.stat_rules().truncation;
        let screen_handlers = self
            .sides
            .iter()
            .flatten()
            .flat_map(|s| s.screens)
            .filter(|turns| *turns != 0)
            .count() as u32;
        let mut damage = Vec::with_capacity(spread_targets.len());
        for &target in &spread_targets {
            if move_data.name == "Poltergeist" {
                let item = DEX
                    .item(self.format.dex_mod, &self.mon(target).item)
                    .unwrap();
                self.add(format!(
                    "|-activate|{}|move: Poltergeist|{}",
                    self.ident(target),
                    item.effect.name
                ));
            }
            let effect = self.effectiveness_of(move_data, move_type, target).unwrap();
            let crit_ratio = move_data.crit_ratio.unwrap_or(1).min(4);
            let crit_roll = move_data.will_crit.unwrap_or_else(|| {
                crit_ratio != 0
                    && self
                        .rng
                        .chance(1, [0, 24, 8, 2, 1][usize::from(crit_ratio)])
            });
            let crit = crit_roll
                && !matches!(
                    self.mon(target).ability.as_str(),
                    "battlearmor" | "shellarmor"
                );
            let user = self.mon(source);
            let mon = self.mon(target);
            let (atk, atk_stage, def, def_stage) = if move_data.category == Category::Physical {
                (
                    if move_data.name == "Body Press" {
                        user.stats.def
                    } else {
                        user.stats.atk
                    },
                    user.boosts.get(if move_data.name == "Body Press" {
                        BoostStat::Def
                    } else {
                        BoostStat::Atk
                    }),
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
                && mon.has_type("Rock"))
                || (self.weather() == Some(Weather::Snow)
                    && move_data.category == Category::Physical
                    && mon.has_type("Ice"))
            {
                tr.modify(def, 6144)
            } else {
                def
            };
            let mut attack_modifier = user.attack_modifier(move_data.category, move_type);
            if (mon.ability == "heatproof" && move_type == "Fire")
                || (mon.ability == "thickfat" && matches!(move_type, "Fire" | "Ice"))
            {
                attack_modifier = crate::mechanics::chain(attack_modifier, 2048);
            }
            let atk = tr.modify(atk, attack_modifier);
            let stab = user.has_type(move_type);
            let mut power_modifier = crate::mechanics::chain(
                user.ability_power_modifier(move_data, move_type, power, weather),
                user.item_power_modifier(move_type, move_data.category),
            );
            if converted {
                power_modifier = crate::mechanics::chain(4915, power_modifier);
            }
            if move_data.name == "Knock Off" && mon.can_take_item(self.format.dex_mod).is_some() {
                power_modifier = crate::mechanics::chain(power_modifier, 6144);
            }
            let mut final_modifier = match user.item.as_str() {
                "lifeorb" => 5324,
                "expertbelt" if effect > 0 => 4915,
                _ => 4096,
            };
            if mon.glaive_drawback {
                final_modifier = crate::mechanics::chain(final_modifier, 8192);
            }
            if (mon.ability == "multiscale" && mon.hp == mon.max_hp.get())
                || (mon.ability == "auraguard" && move_data.flags.contains_key("contact"))
            {
                if user.ability == "sniper" && crit {
                    final_modifier = crate::mechanics::chain(final_modifier, 6144);
                }
                final_modifier = crate::mechanics::chain(final_modifier, 2048);
            }
            final_modifier = crate::mechanics::chain(
                final_modifier,
                self.screens_damage_modifier(source, target, move_data.category, crit),
            );
            let value = Damage {
                level: user.level,
                power: tr.modify(u32::from(power), power_modifier),
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
            .roll_with_final_event(tr, &mut self.rng, |rng| {
                // All screens register onAnyModifyDamage. Their handlers tie
                // even when their own side/category then makes them no-ops.
                for choices in (2..=screen_handlers).rev() {
                    rng.below(choices);
                }
            });
            if spread && self.format.report_exact_hp {
                self.add("|debug|Spread modifier: 0.75");
            }
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
        let mut total = 0;
        for &(target, value) in &damage {
            let mon = self.mon(target);
            let value = if mon.ability == "sturdy" && mon.hp == mon.max_hp.get() && value >= mon.hp
            {
                let value = mon.hp - 1;
                self.add(format!("|-ability|{}|Sturdy", self.ident(target)));
                value
            } else {
                value
            };
            let value = self.focus_sash(target, value);
            let mon = self.mon_mut(target);
            let landed = value.min(mon.hp);
            mon.hp -= landed;
            total += landed;
            self.add_health_line("-damage", target, false);
            if let Some([a, b]) = move_data.drain
                && landed > 0
            {
                let restored = (landed * u32::from(a) + u32::from(b) / 2) / u32::from(b);
                let user = self.mon_mut(source);
                if user.hp > 0 && user.hp < user.max_hp.get() {
                    user.hp = (user.hp + restored.max(1)).min(user.max_hp.get());
                    self.add_health_line_with(
                        "-heal",
                        source,
                        false,
                        &format!("|[from] drain|[of] {}", self.ident(target)),
                    );
                }
            }
        }
        if !move_data.boosts.is_empty() {
            for &(target, _) in &damage {
                self.boost_from(target, source, &move_data.boosts, false, false);
            }
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
        if move_data.self_recharge && self.mon(source).hp > 0 {
            let mon = self.mon_mut(source);
            mon.recharge = 2;
            mon.track_volatile("mustrecharge");
            self.add(format!("|-mustrecharge|{source_name}"));
        }
        if move_data.self_rampage {
            self.rampage_hit(source, slot);
        }
        if move_data.name == "Glaive Rush" && self.mon(source).hp > 0 {
            let mon = self.mon_mut(source);
            mon.glaive_drawback = true;
            mon.track_volatile("glaiverush");
            self.add(format!("|-singlemove|{source_name}|Glaive Rush|[silent]"));
        }
        for &(target, _) in &damage {
            for effect in &move_data.secondaries {
                let roll = self.rng.below(100);
                if effect.chance.is_none_or(|chance| roll < u32::from(chance)) {
                    if let Some(status) = &effect.status {
                        self.try_status(target, status, source, move_data, false, base);
                    }
                    if !effect.boosts.is_empty() {
                        self.boost_from(target, source, &effect.boosts, true, false);
                    }
                    if !effect.self_boosts.is_empty() {
                        self.boost(source, &effect.self_boosts, true);
                    }
                    if effect.volatile_status.as_deref() == Some("flinch")
                        && self.mon(target).hp > 0
                        && self.mon(target).ability != "innerfocus"
                    {
                        self.mon_mut(target).flinched = true;
                        self.mon_mut(target).track_volatile("flinch");
                    }
                    if effect.volatile_status.as_deref() == Some("confusion") {
                        self.apply_confusion(target, false);
                    }
                }
            }
        }
        for &(target, _) in &damage {
            if (move_type == "Fire" || move_data.thaws_target)
                && matches!(self.mon(target).status, Status::Freeze(_))
            {
                self.cure_status(target, "[msg]");
            }
        }
        if move_data.flags.contains_key("contact") {
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
            if move_data.flags.contains_key("contact") {
                match self.mon(target).ability.as_str() {
                    "flamebody" => self.flame_body(target, source),
                    "cutecharm" => self.cute_charm(target, source),
                    "mummy" | "lingeringaroma" => self.contact_ability(target, source),
                    _ => {}
                }
            }
            if self.mon(target).ability == "cursedbody"
                && !struggle
                && self.mon(source).disabled_slot.is_none()
                && self.rng.chance(3, 10)
            {
                self.disable_last_move(source, target, true);
            }
            if self.mon(target).ability == "thermalexchange" && move_type == "Fire" {
                self.ability_boost(target, "Thermal Exchange", BoostStat::Atk, 1, false);
            }
            if self.mon(target).ability == "stamina" {
                self.ability_boost(target, "Stamina", BoostStat::Def, 1, false);
            }
            if self.mon(target).ability == "weakarmor" && move_data.category == Category::Physical {
                self.ability_boosts(
                    target,
                    "Weak Armor",
                    &[(BoostStat::Def, -1), (BoostStat::Spe, 2)],
                    false,
                );
            }
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
        let mut fainted = 0;
        for (target, _) in damage {
            if self.mon(target).hp == 0 {
                self.mark_fainted(target);
                fainted += 1;
            }
            if target != source {
                self.mon_mut(target).times_attacked += 1;
            }
        }
        if self.mon(source).hp == 0 {
            self.mark_fainted(source);
            self.check_win();
        }
        if fainted > 0 && !self.mon(source).fainted && self.mon(source).ability == "moxie" {
            self.ability_boost(source, "Moxie", BoostStat::Atk, fainted, false);
        }
        let recoil = if struggle {
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
        true
    }

    pub(super) fn mark_fainted(&mut self, target: Position) {
        self.last_faint = Some(target.player);
        let mon = self.mon_mut(target);
        mon.fainted = true;
        mon.reset_ability();
        mon.pivot_move = None;
        mon.stall = None;
        mon.protected = false;
        mon.flinched = false;
        mon.recharge = 0;
        mon.confusion = 0;
        mon.rampage = None;
        mon.glaive_drawback = false;
        mon.volatile_order.clear();
        mon.imprisoned = false;
        mon.choice_lock = None;
        mon.roosted = false;
        mon.temporary_type = None;
        mon.yawn = 0;
        mon.taunt = 0;
        mon.attract = None;
        mon.disabled_slot = None;
        mon.last_move_slot = None;
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
        let accuracy =
            if self.mon(target).ability == "sandveil" && self.weather() == Some(Weather::Sand) {
                self.format
                    .stat_rules()
                    .truncation
                    .modify(u32::from(accuracy), 3277) as i32
            } else {
                i32::from(accuracy)
            };
        let evasion = if self.mon(source).ability == "illuminate" {
            0
        } else {
            self.mon(target).boosts.get(BoostStat::Evasion)
        };
        let boost = (i32::from(self.mon(source).boosts.get(BoostStat::Accuracy))
            - i32::from(evasion))
        .clamp(-6, 6);
        (accuracy * (3 + boost.max(0)) / (3 - boost.min(0))) as u32
    }

    pub(super) fn effectiveness(&self, move_data: &Move, target: Position) -> Option<i8> {
        if move_data.name == "Struggle" {
            return Some(0);
        }
        self.effectiveness_of(move_data, &move_data.move_type, target)
    }

    fn effectiveness_of(&self, move_data: &Move, move_type: &str, target: Position) -> Option<i8> {
        let mut modifier = 0;
        for typing in self.mon(target).types() {
            if move_data.name == "Freeze-Dry" && typing == "Water" {
                modifier += 1;
                continue;
            }
            let case = DEX
                .type_data(&to_id(typing))
                .unwrap()
                .damage_taken
                .get(move_type);
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
    fn forced_move(&self) -> Option<(usize, &str, &str)> {
        if self.recharge > 0 {
            Some((RECHARGE_SLOT, "Recharge", "recharge"))
        } else {
            let slot = self.rampage?.slot;
            let m = &self.moves[slot];
            Some((slot, &m.move_data.name, &m.id))
        }
    }

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
                | "oblivious"
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
                | "lingeringaroma"
                | "trace"
                | "illuminate"
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
                | "immunity"
                | "cutecharm"
                | "flamebody"
                | "strongjaw"
                | "megalauncher"
                | "sandveil"
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
                | "lightclay"
                | "expertbelt"
                | "wiseglasses"
                | "muscleband"
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
