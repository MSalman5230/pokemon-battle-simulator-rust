//! Native battle state. Engine inputs and protocol messages are independent of Poke-RL.

use crate::{
    choice::{Choice, SlotChoice, to_id},
    dex::{DEX, Format, Move, Species},
    generation::{GenerationError, generate_for_format},
    input::{Input, Player, PlayerOptions, Set, SetStats, Start, Team},
    mechanics::{Boosts, Nature, StatRules, Stats, health, max_pp},
    order::{Priority, speed_sort_by},
    rng::{Rng, Seed},
};
use serde::Serialize;

mod abilities;
use abilities::Attention;
mod attract;
mod charge;
mod control;
mod damage;
mod disable;
mod forme;
mod guards;
mod hazards;
pub(crate) mod hydration;
mod illusion;
mod items;
mod leech;
mod mega;
mod multihit;
mod partial_trap;
mod perish;
mod rampage;
mod screens;
use charge::Charge;
use guards::SideGuard;
use hazards::Hazard;
use rampage::Rampage;
use screens::Screen;
mod status;
mod substitute;
mod support;
mod switch_copy;
mod tactical;
mod terrain;
mod transform;
mod trap;
mod turn;
mod types;
mod weather;
mod weight;
pub use status::Status;
use std::{borrow::Cow, error::Error, fmt, num::NonZeroU32};
pub use tactical::{DamagePreview, DamageProbe};
pub use terrain::Terrain;
use turn::{Action, FoeHit, Position};
pub use weather::Weather;

#[derive(Debug, PartialEq, Eq)]
pub enum BattleError {
    Input(&'static str),
    Unknown(String),
    Unsupported(String),
    Choice(&'static str),
    Unavailable(&'static str),
    Generation(GenerationError),
}

impl fmt::Display for BattleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(s) | Self::Choice(s) | Self::Unavailable(s) => f.write_str(s),
            Self::Unknown(s) => write!(f, "unknown {s}"),
            Self::Unsupported(s) => write!(f, "not ported: {s}"),
            Self::Generation(err) => err.fmt(f),
        }
    }
}

impl Error for BattleError {}

#[derive(Clone, Default)]
pub struct BattleStream {
    battle: Option<Battle>,
}

impl BattleStream {
    pub fn battle(&self) -> Option<&Battle> {
        self.battle.as_ref()
    }

    pub fn apply(&mut self, input: Input) -> Result<(), BattleError> {
        if let Input::Start(options) = input {
            self.battle = Some(Battle::new(options)?);
            return Ok(());
        }
        let battle = self
            .battle
            .as_mut()
            .ok_or(BattleError::Input("missing >start command"))?;
        match input {
            Input::SetPlayer(player, options) => battle.set_player(player, options),
            Input::Choose(player, choice) => battle.choose(player, choice),
            Input::Reseed(seed) => {
                battle.rng = Rng::new(seed);
                battle.add("|message|The battle's RNG was reset.");
                Ok(())
            }
            Input::ForceWin(winner) => {
                battle.finish(winner);
                Ok(())
            }
            Input::ForceLose(loser) => {
                if battle.sides[loser.index()].is_some() {
                    battle.finish(Some(loser.foe()));
                }
                Ok(())
            }
            Input::TieBreak => battle.tiebreak(),
            Input::ShowOpenTeamSheets => battle.show_open_team_sheets(),
            Input::Start(_) => unreachable!(),
        }
    }

    pub(crate) fn reseed_rollout(&mut self, seed: Seed) {
        self.battle
            .as_mut()
            .expect("hydrated rollout has a battle")
            .rng = Rng::new(seed);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    public: String,
    secret: Option<(Player, String)>,
}

impl LogLine {
    pub fn view(&self, viewer: Option<Player>) -> &str {
        self.secret
            .as_ref()
            .filter(|(owner, _)| Some(*owner) == viewer)
            .map_or(&self.public, |(_, line)| line)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Setup,
    Preview,
    Move,
    Replace,
    Ended,
}

#[derive(Clone)]
pub struct Battle {
    format: &'static Format,
    stat_rules: StatRules,
    twisted_dimension: bool,
    rng: Rng,
    sides: [Option<Side>; 2],
    pending_preview: [Option<Vec<usize>>; 2],
    log: Vec<LogLine>,
    phase: Phase,
    turn: u32,
    winner: Option<Player>,
    last_faint: Option<Player>,
    weather: Option<(Weather, u8)>,
    terrain: Option<(Terrain, u8)>,
    trick_room: u8,
    gravity: u8,
    ability_order: u32,
    ability_breaker: Option<Position>,
    possible_unaware: bool,
    unsupported_called_move: Option<&'static str>,
    turn_queue_empty: bool,
    pending_actions: [Option<Vec<Action>>; 2],
    drag_flags: u8,
    suspended: Vec<Action>,
    cant_undo: [bool; 2],
}

#[derive(Clone)]
struct Side {
    name: String,
    party: Vec<Pokemon>,
    open_sheet: String,
    hazards: Vec<(Hazard, u8)>,
    mega_used: bool,
    screens: [u8; 3],
    guards: [Option<SideGuard>; 2],
    safeguard: u8,
    wishes: [Option<support::Wish>; 2],
    future_moves: [Option<support::FutureMove>; 2],
    replacement_heals: [Vec<support::ReplacementHeal>; 2],
    tailwind: u8,
    total_fainted: u8,
}

#[derive(Clone)]
pub struct Pokemon {
    pub name: String,
    pub species: &'static Species,
    base_species: &'static Species,
    submitted_species: &'static Species,
    transformed: Option<Box<transform::Transformed>>,
    pub details: String,
    pub level: u16,
    points: SetStats,
    nature: Nature,
    pub stats: Stats,
    pub boosts: Boosts,
    stats_raised: bool,
    stats_lowered: bool,
    speed: i32,
    pub hp: u32,
    substitute_hp: u32,
    pub status: Status,
    slept_by_foe: bool,
    pub times_attacked: u32,
    last_foe_hit: Option<FoeHit>,
    attackers_this_turn: [u32; 2],
    heal_block: u8,
    octolock: Option<(Player, usize)>,
    counter: Option<turn::PreparedCounter>,
    volatile_order: Vec<&'static str>,
    flinched: bool,
    lost_focus: bool,
    recharge: u8,
    charge: Option<Charge>,
    confusion: u8,
    rampage: Option<Rampage>,
    uproar: Option<rampage::Uproar>,
    attention: Option<Attention>,
    glaive_drawback: bool,
    destiny_bond: bool,
    helping_hand: u8,
    pub max_hp: NonZeroU32,
    pub item: String,
    last_item: String,
    used_item_this_turn: bool,
    ate_berry: bool,
    ripen_pending: bool,
    cud_chew: Option<items::CudChew>,
    pub ability: String,
    base_ability: String,
    ability_order: u32,
    half_hp_berry_deferred: bool,
    pub pokeball: String,
    happiness: u8,
    hidden_power_index: u8,
    fallen_allies: u8,
    pub moves: Vec<MoveSlot>,
    pub roster_index: usize,
    illusion: Option<usize>,
    disguise_pending: bool,
    announced_hero: bool,
    triggered_battle_bond: bool,
    protection: Option<turn::Protection>,
    stall: Option<(u16, u8)>,
    fainted: bool,
    needs_replacement: bool,
    reviving: bool,
    revealed_trap: bool,
    imprisoned: bool,
    choice_lock: Option<usize>,
    active_turns: u32,
    active_move_actions: u32,
    hurt_this_turn: bool,
    roosted: bool,
    temporary_types: types::TypeOverride,
    type_change_used: bool,
    added_type: Option<&'static str>,
    spent_elements: u8,
    attract: Option<(Player, usize)>,
    disabled_slot: Option<(usize, u8)>,
    observed_disables: u32,
    used_moves: u32,
    last_move_slot: Option<usize>,
    last_move_target: i8,
    move_failed_this_turn: bool,
    move_failed_last_turn: bool,
    acted: bool,
    gender: u8,
    yawn: Option<status::Yawn>,
    seeded_from: Option<Position>,
    partial_trap: Option<partial_trap::PartialTrap>,
    perish_count: u8,
    throat_chop: u8,
    taunt: u8,
    encore: Option<(usize, u8)>,
    revealed_disables: u32,
    revealed_all_disables: bool,
    pivot_move: Option<turn::Pivot>,
}

#[derive(Clone)]
pub struct MoveSlot {
    pub id: String,
    pub move_data: &'static Move,
    pub pp: u8,
    pub max_pp: u8,
}

impl Pokemon {
    fn request_move_id<'a>(&self, slot: &'a MoveSlot) -> Cow<'a, str> {
        match slot.id.as_str() {
            "hiddenpower" => format!("hiddenpower{}60", to_id(self.hidden_power_type())).into(),
            "return" | "frustration" => format!(
                "{}{}",
                slot.id,
                self.friendship_power(slot.id == "frustration")
            )
            .into(),
            _ => slot.id.as_str().into(),
        }
    }

    fn request_move_name<'a>(&self, slot: &'a MoveSlot) -> Cow<'a, str> {
        match slot.id.as_str() {
            "hiddenpower" => format!("Hidden Power {}", self.hidden_power_type()).into(),
            "return" | "frustration" => format!(
                "{} {}",
                slot.move_data.name,
                self.friendship_power(slot.id == "frustration")
            )
            .into(),
            _ => slot.move_data.name.as_str().into(),
        }
    }

    fn friendship_power(&self, frustration: bool) -> u16 {
        let value = if frustration {
            255 - self.happiness
        } else {
            self.happiness
        };
        (u16::from(value) * 2 / 5).max(1)
    }

    fn damage_hp(&mut self, damage: u32) -> u32 {
        let landed = damage.min(self.hp);
        self.hp -= landed;
        self.hurt_this_turn |= landed > 0;
        landed
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest<'a> {
    team_preview: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chosen_team_size: Option<usize>,
    side: SideRequest<'a>,
}

#[derive(Serialize)]
struct SideRequest<'a> {
    name: &'a str,
    id: &'static str,
    pokemon: Vec<SwitchRequest<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SwitchRequest<'a> {
    ident: String,
    details: &'a str,
    condition: String,
    active: bool,
    stats: OtherStats,
    moves: Vec<Cow<'a, str>>,
    base_ability: &'a str,
    item: &'a str,
    pokeball: &'a str,
    ability: &'a str,
    commanding: bool,
    reviving: bool,
}

#[derive(Serialize)]
struct OtherStats {
    atk: u32,
    def: u32,
    spa: u32,
    spd: u32,
    spe: u32,
}

impl From<Stats> for OtherStats {
    fn from(
        Stats {
            atk,
            def,
            spa,
            spd,
            spe,
            ..
        }: Stats,
    ) -> Self {
        Self {
            atk,
            def,
            spa,
            spd,
            spe,
        }
    }
}

impl Battle {
    pub fn new(start: Start) -> Result<Self, BattleError> {
        let format = DEX
            .format(&start.formatid)
            .ok_or_else(|| BattleError::Unknown(format!("format {}", start.formatid)))?;
        // These formats use the common preview/start path.
        if !matches!(
            start.formatid.as_str(),
            "gen9championsbssregmc"
                | "gen9championsvgc2026regmc"
                | "gen9championsbssregmb"
                | "gen9championsvgc2026regmb"
                | "gen9championsvgc2026regmcbo3"
                | "gen9championsvgc2026regmbbo3"
                | "gen9championscustomgame"
                | "gen9championsdoublescustomgame"
                | "gen9championsou"
                | "gen9championsuu"
                | "gen9championsdoubles"
                | "gen9champions4v4doublesuu"
                | "gen9championsdraft"
                | "gen9champions4v4doublesdraft"
                | "gen9championsrosterranchrumble"
                | "gen9championsnatdexdraft"
                | "gen9championsrandombattle"
                | "gen9championsrandomdoublesbattle"
        ) {
            return Err(BattleError::Unsupported(format!(
                "startup rules for {}",
                format.name
            )));
        }
        if start.debug || start.force_random_chance.is_some() {
            return Err(BattleError::Unsupported("debug battle options".into()));
        }
        let seed = resolve_seed(start.seed)?;
        let mut battle = Self {
            format,
            stat_rules: format.stat_rules(),
            twisted_dimension: format.has_rule("twisteddimensionmod"),
            rng: Rng::new(seed),
            sides: [None, None],
            pending_preview: [None, None],
            log: Vec::new(),
            phase: Phase::Setup,
            turn: 0,
            winner: None,
            last_faint: None,
            weather: None,
            terrain: None,
            trick_room: 0,
            gravity: 0,
            ability_order: 0,
            ability_breaker: None,
            possible_unaware: false,
            unsupported_called_move: None,
            turn_queue_empty: true,
            pending_actions: [None, None],
            drag_flags: 0,
            suspended: Vec::new(),
            cant_undo: [false; 2],
        };
        battle.add(format!(
            "|gametype|{}",
            if format.game_type.active_slots() == 1 {
                "singles"
            } else {
                "doubles"
            }
        ));
        Ok(battle)
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn seed(&self) -> Seed {
        self.rng.seed()
    }

    pub fn log(&self) -> &[LogLine] {
        &self.log
    }

    pub fn party(&self, player: Player) -> Option<&[Pokemon]> {
        self.sides[player.index()]
            .as_ref()
            .map(|s| s.party.as_slice())
    }

    pub fn preview_request(&self, player: Player) -> Option<PreviewRequest<'_>> {
        if self.phase != Phase::Preview {
            return None;
        }
        Some(PreviewRequest {
            team_preview: true,
            max_chosen_team_size: self.format.picked_team_size,
            side: self.side_request(player)?,
        })
    }

    fn side_request(&self, player: Player) -> Option<SideRequest<'_>> {
        let side = self.sides[player.index()].as_ref()?;
        let id = player.id();
        Some(SideRequest {
            id,
            name: &side.name,
            pokemon: side
                .party
                .iter()
                .enumerate()
                .map(|(index, mon)| SwitchRequest {
                    ident: format!("{id}: {}", mon.name),
                    details: &mon.details,
                    condition: health(mon.hp, mon.max_hp, mon.status.id(), true).secret,
                    active: index < self.format.game_type.active_slots(),
                    stats: mon
                        .transformed
                        .as_ref()
                        .map_or(mon.stats, |form| form.original_stats)
                        .into(),
                    moves: mon.moves.iter().map(|m| mon.request_move_id(m)).collect(),
                    base_ability: &mon.base_ability,
                    item: &mon.item,
                    pokeball: &mon.pokeball,
                    ability: &mon.ability,
                    commanding: false,
                    reviving: mon.reviving,
                })
                .collect(),
        })
    }

    fn add(&mut self, text: impl Into<String>) {
        self.log.push(LogLine {
            public: text.into(),
            secret: None,
        });
    }

    fn set_player(&mut self, player: Player, options: PlayerOptions) -> Result<(), BattleError> {
        let index = player.index();
        if self.sides[index].is_some() {
            return Err(BattleError::Unsupported(
                "editing an existing player".into(),
            ));
        }
        let sets = match options.team {
            Some(Team::Sets(sets)) => sets,
            Some(Team::Packed(packed)) => {
                crate::input::unpack(&packed).map_err(BattleError::Input)?
            }
            None => {
                // A missing team uses the player's independent seed, never a
                // draw from the ordering or damage PRNG of the battle.
                let seed = resolve_seed(options.seed)?;
                generate_for_format(self.format, &mut Rng::new(seed))
                    .map_err(BattleError::Generation)?
            }
        };
        if sets.len() < self.format.game_type.active_slots() || sets.len() > 24 {
            return Err(BattleError::Input(
                "team length cannot fill this battle's active slots or exceeds 24",
            ));
        }
        // Showdown constructs Pokémon when each player arrives. Preserve that
        // order, but do not consume RNG on a rejected input.
        let mut rng = self.rng.clone();
        let party = sets
            .iter()
            .enumerate()
            .map(|(i, set)| Pokemon::new(self.format, i, set, &mut rng))
            .collect::<Result<Vec<_>, _>>()?;
        if !self.format.has_rule("teampreview")
            && let Some(other) = self.party(player.foe())
        {
            let active = self.format.game_type.active_slots();
            if let Some(err) = [&party[..active], &other[..active]]
                .into_iter()
                .flatten()
                .find_map(Pokemon::entry_error)
            {
                return Err(err);
            }
        }
        self.possible_unaware |= party.iter().any(|p| p.ability == "unaware");
        let open_sheet = crate::input::pack_open_sheet(
            sets.iter()
                .zip(&party)
                .map(|(set, mon)| (set, mon.level, mon.gender)),
            !self.format.has_rule("terastalclause"),
        );
        let name = if options.name.is_empty() {
            format!("Player {}", index + 1)
        } else {
            options.name
        };
        let avatar = match options.avatar {
            None | Some(serde_json::Value::Null | serde_json::Value::Bool(false)) => String::new(),
            Some(serde_json::Value::String(s)) => s,
            Some(serde_json::Value::Number(n)) if n.as_i64() == Some(0) => String::new(),
            Some(value) => value.to_string(),
        };
        let rating = options
            .rating
            .filter(|r| *r != 0)
            .map_or_else(String::new, |r| r.to_string());
        self.add(format!("|player|{}|{name}|{avatar}|{rating}", player.id()));
        self.sides[index] = Some(Side {
            name,
            party,
            open_sheet,
            hazards: Vec::new(),
            mega_used: false,
            screens: [0; 3],
            guards: [None; 2],
            safeguard: 0,
            wishes: [None; 2],
            future_moves: [None; 2],
            replacement_heals: [Vec::new(), Vec::new()],
            tailwind: 0,
            total_fainted: 0,
        });
        self.rng = rng;
        if self.sides.iter().all(Option::is_some) {
            self.begin_battle()?;
        }
        Ok(())
    }

    fn begin_battle(&mut self) -> Result<(), BattleError> {
        self.add("|gen|9");
        self.add(format!("|tier|{}", self.format.name));
        for rule in &self.format.rule_log {
            self.add(rule.as_str());
        }
        if !self.format.has_rule("teampreview") {
            return self.start_selected();
        }
        self.add("|clearpoke");
        for player in Player::ALL {
            for i in 0..self.party(player).unwrap().len() {
                let mon = &self.party(player).unwrap()[i];
                let details = preview_details(&mon.details);
                self.add(format!("|poke|{}|{}|", player.id(), details));
            }
        }
        if let Some(size) = self.format.picked_team_size {
            self.add(format!("|teampreview|{size}"));
        } else {
            self.add("|teampreview");
        }
        self.phase = Phase::Preview;
        if self.format.has_rule("forceopenteamsheets") {
            self.show_open_team_sheets()
                .expect("both teams present before preview");
        }
        Ok(())
    }

    fn show_open_team_sheets(&mut self) -> Result<(), BattleError> {
        if self.turn != 0 {
            return Ok(());
        }
        if self.sides.iter().any(Option::is_none) {
            return Err(BattleError::Input(
                "both players are needed for open team sheets",
            ));
        }
        for player in Player::ALL {
            let sheet = &self.sides[player.index()].as_ref().unwrap().open_sheet;
            self.add(format!("|showteam|{}|{sheet}", player.id()));
        }
        Ok(())
    }

    fn choose(&mut self, player: Player, choice: Choice) -> Result<(), BattleError> {
        match self.phase {
            Phase::Setup => {
                return Err(BattleError::Choice(
                    "both sides must be set before choosing",
                ));
            }
            Phase::Move | Phase::Replace => return self.choose_action(player, choice),
            Phase::Ended => return Err(BattleError::Choice("battle is over")),
            Phase::Preview => {}
        }
        let index = player.index();
        let choice = match choice {
            Choice::Undo => {
                self.pending_preview[index] = None;
                return Ok(());
            }
            Choice::Team(team) => team,
            Choice::Actions(actions) if actions == [SlotChoice::Auto] => "team"
                .parse()
                .map(|c| match c {
                    Choice::Team(team) => team,
                    _ => unreachable!(),
                })
                .unwrap(),
            _ => return Err(BattleError::Choice("expected a team preview choice")),
        };
        let size = self.format.picked_team_size.unwrap_or(usize::MAX);
        let party = self.party(player).unwrap();
        self.pending_preview[index] = Some(
            choice
                .resolve(party.len(), size)
                .map_err(|err| BattleError::Choice(err.0))?,
        );
        if self.pending_preview.iter().all(Option::is_some) {
            if let Some(error) = self.unsupported_leads() {
                self.pending_preview[index] = None;
                return Err(error);
            }
            self.select_leads();
            self.start_selected()?;
        }
        Ok(())
    }

    fn unsupported_leads(&self) -> Option<BattleError> {
        Player::ALL.into_iter().find_map(|player| {
            let party = self.party(player).unwrap();
            self.pending_preview[player.index()]
                .as_ref()
                .unwrap()
                .iter()
                .take(self.format.game_type.active_slots())
                .find_map(|&i| party[i].entry_error())
        })
    }

    fn select_leads(&mut self) {
        // Upstream sorts team actions globally, even across the two sides.
        // Resulting tie draws affect the seed without changing either roster order.
        let mut queue = Vec::new();
        for player in Player::ALL {
            let party = self.party(player).unwrap();
            for (index, &pick) in self.pending_preview[player.index()]
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
            {
                queue.push(Priority {
                    order: 1,
                    priority: -(index as f64),
                    speed: party[pick].stats.spe as i32,
                    ..Priority::default()
                });
            }
        }
        speed_sort_by(&mut queue, &mut self.rng, |a, b| a.compare(*b));
        for player in Player::ALL {
            let picks = self.pending_preview[player.index()].take().unwrap();
            let party = &mut self.sides[player.index()].as_mut().unwrap().party;
            let mut original: Vec<_> = std::mem::take(party).into_iter().map(Some).collect();
            *party = picks
                .into_iter()
                .map(|i| original[i].take().unwrap())
                .collect();
        }
    }
}

fn resolve_seed(seed: Option<Seed>) -> Result<Seed, BattleError> {
    seed.map_or_else(Seed::from_entropy, Ok)
        .map_err(|_| BattleError::Input("could not read system randomness for a Showdown seed"))
}

fn preview_details(details: &str) -> String {
    // Champions uses its own Team Preview handler. It leaves shiny detail
    // intact and masks only these three families.
    let (species, suffix) = details
        .split_once(',')
        .map_or((details, None), |(a, b)| (a, Some(b)));
    let base = species.split('-').next().unwrap();
    if !matches!(base, "Xerneas" | "Zacian" | "Zamazenta") {
        return details.to_owned();
    }
    suffix.map_or_else(|| format!("{base}-*"), |s| format!("{base}-*,{s}"))
}

impl Pokemon {
    pub fn volatile_ids(&self) -> &[&'static str] {
        &self.volatile_order
    }

    fn track_volatile(&mut self, id: &'static str) {
        if !self.volatile_order.contains(&id) {
            self.volatile_order.push(id);
        }
    }

    fn untrack_volatile(&mut self, id: &str) {
        self.volatile_order.retain(|v| *v != id);
    }

    fn new(format: &Format, index: usize, set: &Set, rng: &mut Rng) -> Result<Self, BattleError> {
        let species_name = if set.species.is_empty() {
            &set.name
        } else {
            &set.species
        };
        let species = DEX
            .species(format.dex_mod, &to_id(species_name))
            .ok_or_else(|| BattleError::Unknown(format!("species {species_name}")))?;
        let level = set
            .adjust_level
            .filter(|l| *l != 0)
            .or(set.level.filter(|l| *l != 0))
            .unwrap_or(100)
            .clamp(1, 9999) as u16;
        let gender = if matches!(set.gender.as_str(), "M" | "F" | "N") {
            set.gender.as_str()
        } else if let Some(gender) = species.gender.as_deref().filter(|g| !g.is_empty()) {
            gender
        } else if rng.below(2) == 0 {
            "M"
        } else {
            "F"
        };
        let rules = format.stat_rules();
        let nature = DEX.nature(&set.nature);
        let stats = Stats::map(|s| {
            rules.stat(
                species.base_stats.get(s),
                set.evs.raw(s).clamp(0, 255) as u8,
                level,
                nature,
                s,
            )
        });
        let max_hp = NonZeroU32::new(species.max_hp.unwrap_or(stats.hp))
            .ok_or(BattleError::Input("zero maximum HP"))?;
        let mut moves = Vec::with_capacity(set.moves.len());
        if set.moves.is_empty() {
            return Err(BattleError::Input("set has no moves"));
        }
        if set.moves.len() > u32::BITS as usize {
            return Err(BattleError::Unsupported("more than 32 move slots".into()));
        }
        let mut hp_type = set.hp_type.as_deref().filter(|entry| !entry.is_empty());
        for name in &set.moves {
            let mut data = DEX
                .moves(format.dex_mod, &to_id(name))
                .ok_or_else(|| BattleError::Unknown(format!("move {name}")))?;
            if data.name.starts_with("Hidden Power ") {
                hp_type.get_or_insert(&data.move_type);
                data = DEX.moves(format.dex_mod, "hiddenpower").unwrap();
            }
            let id = to_id(&data.name);
            let pp = max_pp(data.pp, data.no_pp_boosts);
            moves.push(MoveSlot {
                id,
                move_data: data,
                pp,
                max_pp: pp,
            });
        }
        let hidden_power_index = types::hidden_power_index(hp_type, set.ivs)?;
        let name = if set.name.is_empty() || set.name == *species_name {
            species.base_species.clone()
        } else {
            set.name.chars().take(20).collect()
        };
        let mut details = if matches!(species.name.as_str(), "Greninja-Bond" | "Rockruff-Dusk") {
            species.base_species.clone()
        } else {
            species.name.clone()
        };
        if level != 100 {
            details.push_str(&format!(", L{level}"));
        }
        if gender != "N" {
            details.push_str(&format!(", {gender}"));
        }
        if set.shiny {
            details.push_str(", shiny");
        }
        let item = to_id(&set.item);
        let ability = to_id(&set.ability);
        let pokeball = to_id(&set.pokeball);
        Ok(Self {
            name,
            species,
            base_species: species,
            submitted_species: species,
            transformed: None,
            details,
            level,
            points: set.evs,
            nature,
            stats,
            boosts: Boosts::default(),
            stats_raised: false,
            stats_lowered: false,
            speed: stats.spe as i32,
            hp: max_hp.get(),
            substitute_hp: 0,
            status: Status::Healthy,
            slept_by_foe: false,
            times_attacked: 0,
            last_foe_hit: None,
            attackers_this_turn: [0; 2],
            heal_block: 0,
            octolock: None,
            counter: None,
            volatile_order: Vec::new(),
            flinched: false,
            lost_focus: false,
            recharge: 0,
            charge: None,
            confusion: 0,
            rampage: None,
            uproar: None,
            attention: None,
            glaive_drawback: false,
            destiny_bond: false,
            helping_hand: 0,
            max_hp,
            item,
            last_item: String::new(),
            used_item_this_turn: false,
            ate_berry: false,
            ripen_pending: false,
            cud_chew: None,
            base_ability: ability.clone(),
            ability,
            ability_order: 0,
            half_hp_berry_deferred: false,
            fallen_allies: 0,
            moves,
            pokeball: if pokeball.is_empty() {
                "pokeball".into()
            } else {
                pokeball
            },
            happiness: set.happiness.unwrap_or(255),
            hidden_power_index,
            roster_index: index,
            illusion: None,
            disguise_pending: false,
            announced_hero: false,
            triggered_battle_bond: false,
            protection: None,
            stall: None,
            fainted: false,
            needs_replacement: false,
            reviving: false,
            revealed_trap: false,
            imprisoned: false,
            choice_lock: None,
            active_turns: 0,
            active_move_actions: 0,
            hurt_this_turn: false,
            roosted: false,
            temporary_types: types::TypeOverride::default(),
            type_change_used: false,
            added_type: None,
            spent_elements: 0,
            attract: None,
            disabled_slot: None,
            observed_disables: 0,
            used_moves: 0,
            last_move_slot: None,
            last_move_target: 0,
            move_failed_this_turn: false,
            move_failed_last_turn: false,
            acted: false,
            gender: gender.as_bytes()[0],
            yawn: None,
            seeded_from: None,
            partial_trap: None,
            perish_count: 0,
            throat_chop: 0,
            taunt: 0,
            encore: None,
            revealed_disables: 0,
            revealed_all_disables: false,
            pivot_move: None,
        })
    }
}
