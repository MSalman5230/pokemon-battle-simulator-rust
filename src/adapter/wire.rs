//! Serialize borrowed viewpoints. In particular the critic does not need a
//! deep copy of either player's JSON tree.

mod delta;
pub use delta::ObservationDelta;

use super::*;
use crate::{
    adapter::public::{HistoryEntry, PublicMon},
    battle::Pokemon,
};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::value::{RawValue, to_raw_value};

pub struct GameResult<'a> {
    pub(super) game: &'a Game,
    pub(super) legal: [Cow<'a, [Legal<'a>]>; 2],
}

#[derive(Serialize)]
struct Training<'a> {
    critic_views: [WireView<'a>; 2],
}

const SHARED_CRITIC_VIEWS: &str = "shared-rosters-v1";

struct JsonResult<'a, 'game> {
    result: &'a GameResult<'game>,
    encoding: Encoding<'a>,
}

impl Serialize for JsonResult<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.result.serialize_result(serializer, self.encoding)
    }
}

#[derive(Clone, Copy)]
enum Encoding<'a> {
    Generic,
    Json,
    SharedCritic,
    CachedOwn(&'a [Box<RawValue>; 2]),
}

impl Encoding<'_> {
    fn json(self) -> bool {
        !matches!(self, Self::Generic)
    }
}

impl GameResult<'_> {
    pub fn action_plan(&self) -> ActionPlan {
        let lists = self.legal.each_ref().map(|rows| match rows.first() {
            None => RequestActions::Wait,
            Some(Legal::Preview { .. }) => RequestActions::Preview,
            _ => RequestActions::Normal(
                rows.iter()
                    .map(|action| (action.id(), action.token().unwrap()))
                    .collect(),
            ),
        });
        let available = self.game.ended()
            || self.game.truncated
            || lists
                .iter()
                .any(|list| !matches!(list, RequestActions::Wait));
        ActionPlan(lists, available)
    }

    /// Serialize worker replies directly to JSON, reusing unchanged public
    /// projections and encoding each private roster at most once per reply.
    /// Compact training replies let an opt-in decoder reconstruct the critic.
    pub fn write_json(&self, out: &mut Vec<u8>, compact: bool) -> serde_json::Result<()> {
        let roster = |player| {
            to_raw_value(&OwnRoster {
                game: self.game,
                player,
                json: true,
            })
        };
        let own = if self.game.training && !compact {
            Some([roster(Player::P1)?, roster(Player::P2)?])
        } else {
            None
        };
        serde_json::to_writer(
            out,
            &JsonResult {
                result: self,
                encoding: if self.game.training && compact {
                    Encoding::SharedCritic
                } else if let Some(own) = &own {
                    Encoding::CachedOwn(own)
                } else {
                    Encoding::Json
                },
            },
        )
    }
}

impl Serialize for GameResult<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize_result(serializer, Encoding::Generic)
    }
}

impl GameResult<'_> {
    fn serialize_result<S: Serializer>(
        &self,
        serializer: S,
        encoding: Encoding<'_>,
    ) -> Result<S::Ok, S::Error> {
        let view = |player: Player, critic| WireView {
            game: self.game,
            player,
            critic,
            legal: &self.legal[player.index()],
            encoding,
        };
        let game = self.game;
        let mut map = serializer.serialize_map(Some(if game.training { 5 } else { 4 }))?;
        map.serialize_entry("views", &Player::ALL.map(|p| view(p, false)))?;
        map.serialize_entry("ended", &game.ended())?;
        map.serialize_entry("truncated", &game.truncated)?;
        map.serialize_entry("winner", &game.battle().winner().map(Player::index))?;
        if matches!(encoding, Encoding::SharedCritic) {
            #[derive(Serialize)]
            struct SharedCritic {
                critic_views: &'static str,
            }
            map.serialize_entry(
                "training",
                &SharedCritic {
                    critic_views: SHARED_CRITIC_VIEWS,
                },
            )?;
        } else if game.training {
            map.serialize_entry(
                "training",
                &Training {
                    critic_views: Player::ALL.map(|p| view(p, true)),
                },
            )?;
        }
        map.end()
    }
}

struct WireView<'a> {
    game: &'a Game,
    player: Player,
    critic: bool,
    legal: &'a [Legal<'a>],
    encoding: Encoding<'a>,
}

pub(super) fn search_views(game: &Game) -> serde_json::Result<[Value; 2]> {
    let legal = game.legal(Player::P1);
    let view = |critic| {
        serde_json::to_value(WireView {
            game,
            player: Player::P1,
            critic,
            legal: &legal,
            encoding: Encoding::Generic,
        })
    };
    Ok([view(false)?, view(true)?])
}

impl Serialize for WireView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (game, player) = (self.game, self.player);
        let index = player.index();
        let foe = player.foe().index();
        let public = game.public.view(player);
        let mut map = serializer.serialize_map(Some(19))?;
        self.header(&mut MapFields(&mut map))?;
        let roster = |player| OwnRoster {
            game,
            player,
            json: self.encoding.json(),
        };
        if let Encoding::CachedOwn(own) = self.encoding {
            map.serialize_entry("own", &own[index])?;
        } else {
            map.serialize_entry("own", &roster(player))?;
        }
        if self.critic {
            if let Encoding::CachedOwn(own) = self.encoding {
                map.serialize_entry("opponent", &own[foe])?;
            } else {
                map.serialize_entry("opponent", &roster(player.foe()))?;
            }
        } else {
            map.serialize_entry("opponent", &OppRoster(&public.teams[foe], self.encoding))?;
        }
        self.details(&mut MapFields(&mut map), false)?;
        map.serialize_entry(
            "history",
            &History {
                public,
                viewer: index,
            },
        )?;
        self.tail(&mut MapFields(&mut map))?;
        map.end()
    }
}

trait ViewFields {
    type Error;
    fn field(&mut self, name: &str, value: &(impl Serialize + ?Sized)) -> Result<(), Self::Error>;
    fn visible_field(
        &mut self,
        name: &str,
        value: &Value,
        revision: Option<(usize, usize)>,
    ) -> Result<(), Self::Error> {
        let _ = revision;
        self.field(name, value)
    }
}

struct MapFields<'a, M>(&'a mut M);

impl<M: SerializeMap> ViewFields for MapFields<'_, M> {
    type Error = M::Error;
    fn field(&mut self, name: &str, value: &(impl Serialize + ?Sized)) -> Result<(), Self::Error> {
        self.0.serialize_entry(name, value)
    }
}

impl WireView<'_> {
    fn header<F: ViewFields>(&self, fields: &mut F) -> Result<(), F::Error> {
        let game = self.game;
        let player = self.player;
        let index = player.index();
        let public = game.public.view(player);
        fields.field("format", FORMAT)?;
        fields.field("turn", &public.turn)?;
        fields.field("side", &index)?;
        let battle = game.battle();
        let phase = if battle.waiting(player) {
            "wait"
        } else {
            match battle.phase() {
                Phase::Preview => "preview",
                Phase::Replace => "switch",
                _ => "move",
            }
        };
        fields.field("phase", phase)?;
        Ok(())
    }

    fn details<F: ViewFields>(&self, fields: &mut F, packed: bool) -> Result<(), F::Error> {
        let game = self.game;
        let player = self.player;
        let index = player.index();
        let foe = player.foe().index();
        let public = game.public.view(player);
        if packed {
            fields.field("legal", &PackedLegalList(self.legal))?;
        } else {
            fields.field("legal", &LegalList(self.legal))?;
        }
        fields.field("weather", &public.weather)?;
        fields.field("terrain", &public.terrain)?;
        fields.field("pseudo", &public.pseudo)?;
        fields.field(
            "weather_since",
            &public.weather_since.unwrap_or(public.turn),
        )?;
        fields.field(
            "terrain_since",
            &public.terrain_since.unwrap_or(public.turn),
        )?;
        fields.field(
            "conditions",
            &[&public.conditions[index], &public.conditions[foe]],
        )?;
        fields.field("mega", &[public.mega[index], public.mega[foe]])?;
        Ok(())
    }

    fn tail<F: ViewFields>(&self, fields: &mut F) -> Result<(), F::Error> {
        let game = self.game;
        let player = self.player;
        let index = player.index();
        let foe = player.foe().index();
        let public = game.public.view(player);
        fields.field("faints", &[&public.faints[index], &public.faints[foe]])?;
        fields.field("ended", &game.ended())?;
        fields.field("truncated", &game.truncated)?;
        Ok(())
    }
}

struct LegalList<'a>(&'a [Legal<'a>]);
struct PackedLegalList<'a>(&'a [Legal<'a>]);

impl Serialize for PackedLegalList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if matches!(self.0.first(), Some(Legal::Preview { .. })) {
            return serializer.serialize_u8(1);
        }
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for action in self.0 {
            match action {
                Legal::Move {
                    id,
                    slot,
                    name,
                    mega,
                    ..
                } => seq.serialize_element(&(id, slot, name, mega))?,
                Legal::Switch {
                    id,
                    slot,
                    species,
                    party_slot,
                    revive,
                    ..
                } => seq.serialize_element(&(id, slot, species, party_slot, revive))?,
                Legal::Preview { .. } => unreachable!("preview list has no other kinds"),
            }
        }
        seq.end()
    }
}

impl Serialize for LegalList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for action in self.0 {
            seq.serialize_element(action)?;
        }
        seq.end()
    }
}

struct OwnRoster<'a> {
    game: &'a Game,
    player: Player,
    json: bool,
}

impl<'a> OwnRoster<'a> {
    fn slots(&self) -> impl Iterator<Item = OwnMon<'a>> + '_ {
        let index = self.player.index();
        let battle = self.game.battle();
        let sets = &self.game.teams[index];
        let party = battle.party(self.player).unwrap();
        let public = &self.game.public.view(self.player).teams[index];
        sets.iter().enumerate().map(move |(slot, set)| {
            let original = &self.game.originals[index][slot];
            let species = set["species"].as_str().unwrap();
            let public_slot = public.iter().position(|m| m["base_species"] == species);
            let visible = public_slot.map(|i| &public[i]);
            let actual = party
                .iter()
                .enumerate()
                .find(|(_, p)| p.roster_index == slot);
            OwnMon {
                battle,
                player: self.player,
                party_slot: actual.map(|(i, _)| i),
                visible: visible.unwrap_or(&original.fresh),
                public_slot,
                actual: actual.map(|(_, p)| p),
                slot,
                set,
                original,
                active: battle.turn() > 0 && actual.is_some_and(|(i, _)| i == 0),
                selected: (battle.phase() != Phase::Preview).then_some(actual.is_some()),
                json: self.json,
                fresh: visible.is_none_or(PublicMon::is_fresh),
                include_set: true,
            }
        })
    }
}

impl Serialize for OwnRoster<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.game.teams[self.player.index()].len()))?;
        for mon in self.slots() {
            if let Some(raw) = mon.cached_unselected() {
                seq.serialize_element(raw)?;
            } else {
                seq.serialize_element(&mon)?;
            }
        }
        seq.end()
    }
}

struct OwnMon<'a> {
    battle: &'a Battle,
    player: Player,
    party_slot: Option<usize>,
    visible: &'a PublicMon,
    public_slot: Option<usize>,
    actual: Option<&'a Pokemon>,
    slot: usize,
    set: &'a Value,
    original: &'a OriginalSet,
    active: bool,
    selected: Option<bool>,
    json: bool,
    fresh: bool,
    include_set: bool,
}

impl OwnMon<'_> {
    fn cached_unselected(&self) -> Option<&RawValue> {
        // Never cache a decoy or hydrated observation as a fresh slot.
        (self.json && self.selected == Some(false) && self.fresh).then(|| {
            self.original.wire_unselected[usize::from(self.include_set)]
                .get_or_init(|| to_raw_value(self).unwrap())
                .as_ref()
        })
    }
}

#[derive(Serialize)]
struct BattleStats {
    atk: u32,
    def: u32,
    spa: u32,
    spd: u32,
    spe: u32,
}

struct BoostTable<'a>(&'a Pokemon);

struct Types<'a>(&'a Pokemon);

impl Serialize for Types<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for name in self.0.types() {
            seq.serialize_element(name)?;
        }
        seq.end()
    }
}

impl Serialize for BoostTable<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(STATS.len()))?;
        for stat in STATS {
            map.serialize_entry(stat.id(), &self.0.boosts.get(stat))?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct SlotView<'a> {
    id: &'a str,
    pp: u8,
    maxpp: u8,
    disabled: bool,
}

struct Moves<'a> {
    battle: &'a Battle,
    player: Player,
    party_slot: usize,
    slots: &'a [crate::battle::MoveSlot],
}

const PACKED_MOVE_COLUMNS: [&str; 4] = ["moves", "move2", "move3", "move4"];

impl Moves<'_> {
    fn slot(&self, slot: usize) -> SlotView<'_> {
        let m = &self.slots[slot];
        SlotView {
            id: &m.id,
            pp: m.pp,
            maxpp: m.max_pp,
            disabled: self
                .battle
                .actual_move_disabled(self.player, self.party_slot, slot),
        }
    }
}

impl Serialize for Moves<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.slots.len()))?;
        for slot in 0..self.slots.len() {
            seq.serialize_element(&self.slot(slot))?;
        }
        seq.end()
    }
}

impl Serialize for OwnMon<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.fields(&mut MapFields(&mut map), false, false)?;
        map.end()
    }
}

impl OwnMon<'_> {
    fn fields<F: ViewFields>(
        &self,
        fields: &mut F,
        packed: bool,
        reuse_public: bool,
    ) -> Result<(), F::Error> {
        for (key, value, revision) in self.visible.revisions() {
            if reuse_public && self.public_slot.is_some() && !matches!(key, "hp" | "nickname") {
                // The opponent projection already has every other
                // visible field, including fields replaced below.
                continue;
            }
            if matches!(key, "moves" | "item" | "ability")
                || self.actual.is_some()
                    && matches!(
                        key,
                        "species"
                            | "hp"
                            | "status"
                            | "boosts"
                            | "types"
                            | "active"
                            | "fainted"
                            | "identity_uncertain"
                            | "times_attacked"
                    )
            {
                continue;
            }
            fields.visible_field(
                key,
                if key == "hp" {
                    self.visible.own_hp()
                } else {
                    value
                },
                // The owner's HP can change even when the displayed
                // fraction is ignored at a public color boundary.
                if key == "hp" { None } else { revision },
            )?;
        }
        if let Some(p) = self.actual {
            fields.field("species", &p.species.name)?;
            fields.field("hp", &(f64::from(p.hp) / f64::from(p.max_hp.get())))?;
            fields.field("current_hp", &p.hp)?;
            fields.field("max_hp", &p.max_hp.get())?;
            fields.field(
                "stats",
                &BattleStats {
                    atk: p.stats.atk,
                    def: p.stats.def,
                    spa: p.stats.spa,
                    spd: p.stats.spd,
                    spe: p.stats.spe,
                },
            )?;
            fields.field("status", p.status.id())?;
            fields.field("boosts", &BoostTable(p))?;
            if packed {
                fields.field("types", &Types(p))?;
            } else {
                fields.field("types", &p.types().collect::<Vec<_>>())?;
            }
            fields.field("active", &self.active)?;
            fields.field("fainted", &(p.hp == 0))?;
            fields.field("identity_uncertain", &false)?;
            fields.field("times_attacked", &p.times_attacked)?;
            fields.field("volatile_ids", p.volatile_ids())?;
            let moves = Moves {
                battle: self.battle,
                player: self.player,
                party_slot: self.party_slot.unwrap(),
                slots: &p.moves,
            };
            if packed && (1..=PACKED_MOVE_COLUMNS.len()).contains(&p.moves.len()) {
                for (i, column) in PACKED_MOVE_COLUMNS[..p.moves.len()].iter().enumerate() {
                    let row = moves.slot(i);
                    fields.field(column, &(row.id, row.pp, row.maxpp, row.disabled))?;
                }
            } else {
                fields.field("moves", &moves)?;
            }
            fields.field("item", &p.item)?;
            fields.field("ability", &p.ability)?;
        } else {
            fields.field("item", &self.original.item)?;
            fields.field("ability", &self.original.ability)?;
            fields.field("moves", &self.original.moves)?;
        }
        fields.field("index", &self.slot)?;
        if self.include_set {
            if self.json {
                fields.field(
                    "set",
                    self.original
                        .wire_set
                        .get_or_init(|| to_raw_value(self.set).unwrap()),
                )?;
            } else {
                fields.field("set", self.set)?;
            }
        }
        fields.field("selected", &self.selected)?;
        Ok(())
    }
}

struct OppRoster<'a>(&'a [PublicMon], Encoding<'a>);
struct OppMon<'a>(&'a PublicMon);

impl Serialize for OppRoster<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for v in self.0 {
            if matches!(self.1, Encoding::SharedCritic) && v.is_fresh() {
                // This template uses only data already exposed by the preview log.
                seq.serialize_element(&(&v["species"], &v["types"]))?;
            } else if self.1.json() {
                seq.serialize_element(v.encoded(|mon| to_raw_value(&OppMon(mon)).unwrap()))?;
            } else {
                seq.serialize_element(&OppMon(v))?;
            }
        }
        seq.end()
    }
}

impl Serialize for OppMon<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in self.0.iter() {
            if key != "nickname" {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

struct History<'a> {
    public: &'a PublicState,
    viewer: usize,
}

struct HistoryLine<'a>(&'a HistoryEntry, usize);

impl Serialize for History<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let events = &self.public.history;
        let mut seq = serializer.serialize_seq(Some(events.len()))?;
        for h in events {
            seq.serialize_element(&HistoryLine(h, self.viewer))?;
        }
        seq.end()
    }
}

impl Serialize for HistoryLine<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let HistoryEntry::Move {
            turn,
            side,
            move_id,
            actor,
            species,
            order,
        } = self.0
        {
            let mut map = serializer.serialize_map(Some(6))?;
            map.serialize_entry("turn", turn)?;
            map.serialize_entry("side", &usize::from(*side != self.1))?;
            map.serialize_entry("move", move_id)?;
            map.serialize_entry("actor", actor)?;
            map.serialize_entry("species", species)?;
            map.serialize_entry("order", order)?;
            return map.end();
        }
        let HistoryEntry::Hydrated(h) = self.0 else {
            unreachable!()
        };
        let h = h.as_object().unwrap();
        let mut map = serializer.serialize_map(Some(h.len()))?;
        for (key, value) in h {
            if key == "side" {
                map.serialize_entry(key, &usize::from(*value != self.1))?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}
