//! Serialize borrowed viewpoints. In particular the critic does not need a
//! deep copy of either player's JSON tree.

use super::*;
use crate::{adapter::public::PublicMon, battle::Pokemon};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::value::{RawValue, to_raw_value};

pub struct GameResult<'a> {
    pub(super) game: &'a Game,
    pub(super) legal: [Cow<'static, [Legal]>; 2],
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
    legal: &'a [Legal],
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
        let public = &game.public[index];
        let mut map = serializer.serialize_map(Some(19))?;
        map.serialize_entry("format", FORMAT)?;
        map.serialize_entry("turn", &public.turn)?;
        map.serialize_entry("side", &index)?;
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
        map.serialize_entry("phase", phase)?;
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
        map.serialize_entry("legal", &LegalList(self.legal))?;
        map.serialize_entry("weather", &public.weather)?;
        map.serialize_entry("terrain", &public.terrain)?;
        map.serialize_entry("pseudo", &public.pseudo)?;
        map.serialize_entry(
            "weather_since",
            &public.weather_since.unwrap_or(public.turn),
        )?;
        map.serialize_entry(
            "terrain_since",
            &public.terrain_since.unwrap_or(public.turn),
        )?;
        map.serialize_entry(
            "conditions",
            &[&public.conditions[index], &public.conditions[foe]],
        )?;
        map.serialize_entry("mega", &[public.mega[index], public.mega[foe]])?;
        map.serialize_entry(
            "history",
            &History {
                public,
                viewer: index,
            },
        )?;
        map.serialize_entry("faints", &[&public.faints[index], &public.faints[foe]])?;
        map.serialize_entry("ended", &game.ended())?;
        map.serialize_entry("truncated", &game.truncated)?;
        map.end()
    }
}

struct LegalList<'a>(&'a [Legal]);

impl Serialize for LegalList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for action in self.0 {
            seq.serialize_element(&action.public)?;
        }
        seq.end()
    }
}

struct OwnRoster<'a> {
    game: &'a Game,
    player: Player,
    json: bool,
}

impl Serialize for OwnRoster<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let index = self.player.index();
        let battle = self.game.battle();
        let sets = &self.game.teams[index];
        let party = battle.party(self.player).unwrap();
        let public = &self.game.public[index].teams[index];
        let mut seq = serializer.serialize_seq(Some(sets.len()))?;
        for (slot, set) in sets.iter().enumerate() {
            let original = &self.game.originals[index][slot];
            let species = set["species"].as_str().unwrap();
            let visible = public.iter().find(|m| m["base_species"] == species);
            let actual = party
                .iter()
                .enumerate()
                .find(|(_, p)| p.roster_index == slot);
            let mon = OwnMon {
                battle,
                player: self.player,
                party_slot: actual.map(|(i, _)| i),
                visible: visible.map_or(&original.fresh, |mon| &**mon),
                actual: actual.map(|(_, p)| p),
                slot,
                set,
                original,
                active: battle.turn() > 0 && actual.is_some_and(|(i, _)| i == 0),
                selected: (battle.phase() != Phase::Preview).then_some(actual.is_some()),
                json: self.json,
            };
            // Never cache a decoy or hydrated observation as a fresh slot.
            if self.json && mon.selected == Some(false) && visible.is_none_or(PublicMon::is_fresh) {
                seq.serialize_element(
                    original
                        .wire_unselected
                        .get_or_init(|| to_raw_value(&mon).unwrap()),
                )?;
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
    visible: &'a Value,
    actual: Option<&'a Pokemon>,
    slot: usize,
    set: &'a Value,
    original: &'a OriginalSet,
    active: bool,
    selected: Option<bool>,
    json: bool,
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

impl Serialize for Moves<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.slots.len()))?;
        for (slot, m) in self.slots.iter().enumerate() {
            seq.serialize_element(&SlotView {
                id: &m.id,
                pp: m.pp,
                maxpp: m.max_pp,
                disabled: self
                    .battle
                    .actual_move_disabled(self.player, self.party_slot, slot),
            })?;
        }
        seq.end()
    }
}

impl Serialize for OwnMon<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (key, value) in self.visible.as_object().unwrap() {
            if matches!(key.as_str(), "moves" | "item" | "ability")
                || self.actual.is_some()
                    && matches!(
                        key.as_str(),
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
            map.serialize_entry(key, value)?;
        }
        if let Some(p) = self.actual {
            map.serialize_entry("species", &p.species.name)?;
            map.serialize_entry("hp", &(f64::from(p.hp) / f64::from(p.max_hp.get())))?;
            map.serialize_entry("current_hp", &p.hp)?;
            map.serialize_entry("max_hp", &p.max_hp.get())?;
            map.serialize_entry(
                "stats",
                &BattleStats {
                    atk: p.stats.atk,
                    def: p.stats.def,
                    spa: p.stats.spa,
                    spd: p.stats.spd,
                    spe: p.stats.spe,
                },
            )?;
            map.serialize_entry("status", p.status.id())?;
            map.serialize_entry("boosts", &BoostTable(p))?;
            map.serialize_entry("types", &p.types().collect::<Vec<_>>())?;
            map.serialize_entry("active", &self.active)?;
            map.serialize_entry("fainted", &(p.hp == 0))?;
            map.serialize_entry("identity_uncertain", &false)?;
            map.serialize_entry("times_attacked", &p.times_attacked)?;
            map.serialize_entry("volatile_ids", p.volatile_ids())?;
            map.serialize_entry(
                "moves",
                &Moves {
                    battle: self.battle,
                    player: self.player,
                    party_slot: self.party_slot.unwrap(),
                    slots: &p.moves,
                },
            )?;
            map.serialize_entry("item", &p.item)?;
            map.serialize_entry("ability", &p.ability)?;
        } else {
            map.serialize_entry("item", &self.original.item)?;
            map.serialize_entry("ability", &self.original.ability)?;
            map.serialize_entry("moves", &self.original.moves)?;
        }
        map.serialize_entry("index", &self.slot)?;
        if self.json {
            map.serialize_entry(
                "set",
                self.original
                    .wire_set
                    .get_or_init(|| to_raw_value(self.set).unwrap()),
            )?;
        } else {
            map.serialize_entry("set", self.set)?;
        }
        map.serialize_entry("selected", &self.selected)?;
        map.end()
    }
}

struct OppRoster<'a>(&'a [PublicMon], Encoding<'a>);
struct OppMon<'a>(&'a Value);

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
        let mon = self.0.as_object().unwrap();
        let mut map = serializer
            .serialize_map(Some(mon.len() - usize::from(mon.contains_key("nickname"))))?;
        for (key, value) in mon {
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

struct HistoryLine<'a>(&'a Value, usize);

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
        let h = self.0.as_object().unwrap();
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
