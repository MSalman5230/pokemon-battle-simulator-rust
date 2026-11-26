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
}

#[derive(Default)]
struct ViewDelta {
    fields: Vec<Vec<u8>>,
    own: [Vec<u8>; 6],
    opponent: [Vec<u8>; 6],
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
        let game = result.game;
        debug_assert!(game.training);
        out.extend_from_slice(b"{\"$battle_delta_v1\":[");
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
    fn write(&mut self, out: &mut Vec<u8>, scratch: &mut Vec<u8>, view: WireView<'_>) -> Result {
        out.extend_from_slice(b"[{");
        let mut fields = DeltaFields {
            out,
            scratch,
            previous: &mut self.fields,
            index: 0,
            comma: false,
        };
        view.header(&mut fields)?;
        view.details(&mut fields)?;
        view.tail(&mut fields)?;
        out.extend_from_slice(b"},[");

        let roster = OwnRoster {
            game: view.game,
            player: view.player,
            json: true,
        };
        let mut comma = false;
        for mut mon in roster.slots() {
            let previous = &mut self.own[mon.slot];
            mon.include_set = false;
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
                serde_json::to_writer(&mut *out, &HistoryLine(event, index))?;
            }
            out.push(b']');
            self.history_count = public.history_count;
            self.history_len = public.history.len();
        }
        out.push(b']');
        Ok(())
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

    fn field(&mut self, name: &'static str, value: &(impl Serialize + ?Sized)) -> Result {
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
