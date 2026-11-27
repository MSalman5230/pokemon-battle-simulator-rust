//! Lossless, per-game wire deltas. Legacy observations and deltas serialize
//! the same field projections. Only the transmission of unchanged data differs.

use super::*;

type Error = serde_json::Error;
type Result<T = ()> = std::result::Result<T, Error>;

/// The last observations transmitted for a single game on one connection.
/// Replace with the default after a discarded reply or a game reset.
#[derive(Default)]
pub struct ObservationDelta {
    sequence: usize,
    views: [ViewDelta; 2],
    scratch: Vec<u8>,
    mon_scratch: MonFields,
}

#[derive(Default)]
struct ViewDelta {
    fields: Vec<Vec<u8>>,
    own: [Vec<u8>; 6],
    opponent: [Vec<u8>; 6],
    packed_own: [MonFields; 6],
    packed_opp: [MonFields; 6],
    opp_revisions: [Option<usize>; 6],
    bench: [Option<BenchState>; 6],
    history_count: usize,
    history_len: usize,
}

impl ObservationDelta {
    pub fn write_json(
        &mut self,
        out: &mut Vec<u8>,
        game_key: &str,
        result: &GameResult<'_>,
    ) -> Result {
        self.write(out, game_key, result, false)
    }

    pub fn write_packed_json(
        &mut self,
        out: &mut Vec<u8>,
        game_key: &str,
        result: &GameResult<'_>,
    ) -> Result {
        self.write(out, game_key, result, true)
    }

    fn write(
        &mut self,
        out: &mut Vec<u8>,
        game_key: &str,
        result: &GameResult<'_>,
        packed: bool,
    ) -> Result {
        let game = result.game;
        debug_assert!(game.training);
        out.extend_from_slice(if packed {
            b"{\"$battle_delta_v2\":["
        } else {
            b"{\"$battle_delta_v1\":["
        });
        serde_json::to_writer(&mut *out, game_key)?;
        out.push(b',');
        serde_json::to_writer(&mut *out, &self.sequence)?;
        out.extend_from_slice(b",[");
        for (i, player) in Player::ALL.into_iter().enumerate() {
            if i != 0 {
                out.push(b',');
            }
            self.views[i].write(
                out,
                &mut self.scratch,
                packed.then_some(&mut self.mon_scratch),
                WireView {
                    game,
                    player,
                    critic: false,
                    legal: &result.legal[i],
                    encoding: Encoding::Json,
                },
            )?;
        }
        out.extend_from_slice(b"],");
        serde_json::to_writer(
            &mut *out,
            &(
                game.ended(),
                game.truncated,
                game.battle().winner().map(Player::index),
            ),
        )?;
        if self.sequence == 0 {
            // Original sets never change while a game is live. Send them once
            // with the snapshot and reuse them across subsequent own-slot updates.
            out.push(b',');
            serde_json::to_writer(&mut *out, game.teams.as_ref())?;
        }
        out.extend_from_slice(b"]}");
        self.sequence += 1;
        Ok(())
    }
}

impl ViewDelta {
    fn write(
        &mut self,
        out: &mut Vec<u8>,
        scratch: &mut Vec<u8>,
        mut packed: Option<&mut MonFields>,
        view: WireView<'_>,
    ) -> Result {
        out.extend_from_slice(b"[{");
        let mut fields = DeltaFields {
            out,
            scratch,
            previous: &mut self.fields,
            index: 0,
            comma: false,
        };
        view.header(&mut fields)?;
        view.details(&mut fields, packed.is_some())?;
        view.tail(&mut fields)?;
        out.extend_from_slice(b"},[");

        let roster = OwnRoster {
            game: view.game,
            player: view.player,
            json: true,
        };
        let mut comma = false;
        for mut mon in roster.slots() {
            mon.include_set = false;
            if let Some(temp) = packed.as_deref_mut() {
                let previous = &mut self.bench[mon.slot];
                if previous.as_ref().is_some_and(|base| base.matches(&mon)) {
                    continue;
                }
                *previous = BenchState::capture(&mon);
                temp.clear();
                mon.fields(temp, true)?;
                temp.emit(out, &mut comma, mon.slot, &mut self.packed_own[mon.slot])?;
                continue;
            }
            let previous = &mut self.own[mon.slot];
            if let Some(raw) = mon.cached_unselected() {
                write_slot(out, &mut comma, mon.slot, previous, raw.get().as_bytes())?;
            } else {
                scratch.clear();
                serde_json::to_writer(&mut *scratch, &mon)?;
                write_slot(out, &mut comma, mon.slot, previous, scratch)?;
            }
        }
        out.extend_from_slice(b"],[");
        let index = view.player.index();
        let public = &view.game.public[index];
        let mut comma = false;
        for (slot, (mon, previous)) in public.teams[view.player.foe().index()]
            .iter()
            .zip(&mut self.opponent)
            .enumerate()
        {
            if let Some(temp) = packed.as_deref_mut() {
                if self.opp_revisions[slot] == Some(mon.revision) {
                    continue;
                }
                temp.clear();
                for (key, value) in mon.iter() {
                    if key != "nickname" {
                        temp.field(key, value)?;
                    }
                }
                temp.emit(out, &mut comma, slot, &mut self.packed_opp[slot])?;
                self.opp_revisions[slot] = Some(mon.revision);
                continue;
            }
            let raw = mon.encoded(|mon| to_raw_value(&OppMon(mon)).unwrap());
            write_slot(out, &mut comma, slot, previous, raw.get().as_bytes())?;
        }
        out.extend_from_slice(b"],");
        if public.history_count == self.history_count {
            out.extend_from_slice(b"null");
        } else {
            let first = public.history_count - public.history.len();
            let drop = first
                .saturating_sub(self.history_count - self.history_len)
                .min(self.history_len);
            let skip = self.history_count.saturating_sub(first);
            out.push(b'[');
            serde_json::to_writer(&mut *out, &drop)?;
            for event in public.history.iter().skip(skip) {
                out.push(b',');
                if packed.is_some()
                    && let HistoryEntry::Move {
                        turn,
                        side,
                        move_id,
                        actor,
                        species,
                        order,
                    } = event
                {
                    serde_json::to_writer(
                        &mut *out,
                        &(
                            turn,
                            usize::from(*side != index),
                            move_id,
                            actor,
                            species,
                            order,
                        ),
                    )?;
                } else {
                    serde_json::to_writer(&mut *out, &HistoryLine(event, index))?;
                }
            }
            out.push(b']');
            self.history_count = public.history_count;
            self.history_len = public.history.len();
        }
        out.push(b']');
        Ok(())
    }
}

// Compare the fields of the native projection, not guessed turn activity.
// A bench member can still have HP, status, moves and other data changed.
struct BenchState {
    public_revision: usize,
    selected: Option<bool>,
    native: Option<BenchNative>,
}

struct BenchNative {
    species: &'static str,
    hp: u32,
    max_hp: u32,
    stats: crate::mechanics::Stats,
    status: &'static str,
    boosts: [i8; STATS.len()],
    types: Vec<&'static str>,
    hits: u32,
    volatiles: Vec<&'static str>,
    moves: Vec<(String, u8, u8, bool)>,
    item: String,
    ability: String,
}

impl BenchState {
    fn capture(mon: &OwnMon<'_>) -> Option<Self> {
        (!mon.active).then(|| Self {
            public_revision: mon.visible.revision,
            selected: mon.selected,
            native: mon.actual.map(|p| BenchNative {
                species: p.species.name.as_str(),
                hp: p.hp,
                max_hp: p.max_hp.get(),
                stats: p.stats,
                status: p.status.id(),
                boosts: STATS.map(|stat| p.boosts.get(stat)),
                types: p.types().collect(),
                hits: p.times_attacked,
                volatiles: p.volatile_ids().to_vec(),
                moves: p
                    .moves
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        (
                            m.id.clone(),
                            m.pp,
                            m.max_pp,
                            mon.battle
                                .actual_move_disabled(mon.player, mon.party_slot.unwrap(), i),
                        )
                    })
                    .collect(),
                item: p.item.clone(),
                ability: p.ability.clone(),
            }),
        })
    }

    fn matches(&self, mon: &OwnMon<'_>) -> bool {
        if mon.active
            || mon.visible.revision != self.public_revision
            || mon.selected != self.selected
        {
            return false;
        }
        match (&self.native, mon.actual) {
            (None, None) => true,
            (Some(base), Some(p)) => {
                base.species == p.species.name
                    && base.hp == p.hp
                    && base.max_hp == p.max_hp.get()
                    && base.stats == p.stats
                    && base.status == p.status.id()
                    && base.boosts == STATS.map(|stat| p.boosts.get(stat))
                    && p.types().eq(base.types.iter().copied())
                    && base.hits == p.times_attacked
                    && base.volatiles == p.volatile_ids()
                    && base.item == p.item
                    && base.ability == p.ability
                    && base.moves.len() == p.moves.len()
                    && base.moves.iter().zip(&p.moves).enumerate().all(
                        |(i, ((id, pp, maxpp, disabled), m))| {
                            id == &m.id
                                && *pp == m.pp
                                && *maxpp == m.max_pp
                                && *disabled
                                    == mon.battle.actual_move_disabled(
                                        mon.player,
                                        mon.party_slot.unwrap(),
                                        i,
                                    )
                        },
                    )
            }
            _ => false,
        }
    }
}

struct DeltaFields<'a> {
    out: &'a mut Vec<u8>,
    scratch: &'a mut Vec<u8>,
    previous: &'a mut Vec<Vec<u8>>,
    index: usize,
    comma: bool,
}

impl ViewFields for DeltaFields<'_> {
    type Error = Error;

    fn field(&mut self, name: &str, value: &(impl Serialize + ?Sized)) -> Result {
        self.scratch.clear();
        serde_json::to_writer(&mut *self.scratch, value)?;
        if self.index == self.previous.len() {
            self.previous.push(Vec::new());
        }
        let previous = &mut self.previous[self.index];
        self.index += 1;
        if *previous != *self.scratch {
            comma(self.out, &mut self.comma);
            serde_json::to_writer(&mut *self.out, name)?;
            self.out.push(b':');
            self.out.extend_from_slice(self.scratch);
            std::mem::swap(previous, self.scratch);
        }
        Ok(())
    }
}

const EXTRA_FIELDS: [&str; 9] = [
    "current_hp",
    "max_hp",
    "stats",
    "volatile_ids",
    "index",
    "selected",
    "move2",
    "move3",
    "move4",
];
const MON_FIELD_COUNT: usize = public::MON_FIELDS.len() + EXTRA_FIELDS.len();

#[derive(Default)]
struct MonFields {
    bytes: Vec<u8>,
    offsets: [(usize, usize); MON_FIELD_COUNT],
    mask: u32,
}

impl ViewFields for MonFields {
    type Error = Error;

    fn field(&mut self, name: &str, value: &(impl Serialize + ?Sized)) -> Result {
        let column = public::field_index(name)
            .or_else(|| {
                EXTRA_FIELDS
                    .iter()
                    .position(|key| *key == name)
                    .map(|i| i + public::MON_FIELDS.len())
            })
            .ok_or_else(|| {
                <Error as serde::ser::Error>::custom(format_args!(
                    "Unknown packed observation column {name}"
                ))
            })?;
        let start = self.bytes.len();
        serde_json::to_writer(&mut self.bytes, value)?;
        self.offsets[column] = (start, self.bytes.len());
        self.mask |= 1 << column;
        Ok(())
    }
}

impl MonFields {
    fn clear(&mut self) {
        self.bytes.clear();
        self.mask = 0;
    }

    fn value(&self, column: usize) -> &[u8] {
        let (start, end) = self.offsets[column];
        &self.bytes[start..end]
    }

    fn emit(
        &mut self,
        out: &mut Vec<u8>,
        pending: &mut bool,
        slot: usize,
        previous: &mut Self,
    ) -> Result {
        let mut changed = self.mask & !previous.mask;
        let mut old_columns = self.mask & previous.mask;
        while old_columns != 0 {
            let column = old_columns.trailing_zeros() as usize;
            let bit = 1 << column;
            old_columns ^= bit;
            if self.value(column) != previous.value(column) {
                changed |= bit;
            }
        }
        if changed == 0 && self.mask == previous.mask {
            return Ok(());
        }
        comma(out, pending);
        serde_json::to_writer(&mut *out, &slot)?;
        out.extend_from_slice(b",[");
        serde_json::to_writer(&mut *out, &self.mask)?;
        out.push(b',');
        serde_json::to_writer(&mut *out, &changed)?;
        while changed != 0 {
            let column = changed.trailing_zeros() as usize;
            changed ^= 1 << column;
            out.push(b',');
            out.extend_from_slice(self.value(column));
        }
        out.push(b']');
        std::mem::swap(self, previous);
        Ok(())
    }
}

fn write_slot(
    out: &mut Vec<u8>,
    needs_comma: &mut bool,
    slot: usize,
    previous: &mut Vec<u8>,
    value: &[u8],
) -> Result {
    if value != previous {
        comma(out, needs_comma);
        serde_json::to_writer(&mut *out, &slot)?;
        out.push(b',');
        out.extend_from_slice(value);
        previous.clear();
        previous.extend_from_slice(value);
    }
    Ok(())
}

fn comma(out: &mut Vec<u8>, needs_comma: &mut bool) {
    if *needs_comma {
        out.push(b',');
    }
    *needs_comma = true;
}
