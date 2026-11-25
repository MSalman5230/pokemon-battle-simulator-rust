//! Move-selection history and move slots changed during a battle.

use super::{turn::Position, *};

#[derive(Clone, Default)]
pub(super) struct MimickedMoves(Vec<(usize, MoveSlot)>);

impl Pokemon {
    pub(super) fn reset_move_history(&mut self) {
        self.last_declared_move = None;
        self.last_used_type = None;
        self.last_used_type_known = true;
        if let Some(original) = self.mimicked_moves.take() {
            for (index, slot) in original.0 {
                self.moves[index] = slot;
            }
        }
    }
}

impl Battle {
    pub(super) fn copy_move_slot(
        &mut self,
        source: Position,
        target: Position,
        sketch: bool,
    ) -> bool {
        let mon = self.mon(source);
        let opponent = self.mon(target);
        let Some(move_data) = opponent.last_declared_move else {
            if !opponent.last_used_type_known {
                self.unsupported_called_move = Some((
                    if sketch { "Sketch" } else { "Mimic" },
                    "the target's previous selected move was not observed",
                ));
            }
            return false;
        };
        let id = to_id(&move_data.name);
        if mon.transformed.is_some()
            || move_data.is_z_move
            || move_data
                .flags
                .contains_key(if sketch { "nosketch" } else { "failmimic" })
            || mon.moves.iter().any(|slot| slot.id == id)
        {
            return false;
        }
        let Some(index) = mon
            .moves
            .iter()
            .position(|slot| slot.id == if sketch { "sketch" } else { "mimic" })
        else {
            return false;
        };
        let mon = self.mon_mut(source);
        let previous = std::mem::replace(
            &mut mon.moves[index],
            MoveSlot {
                id,
                move_data,
                pp: move_data.pp,
                max_pp: move_data.pp,
            },
        );
        if !sketch {
            mon.mimicked_moves
                .get_or_insert_with(Default::default)
                .0
                .push((index, previous));
        }
        mon.used_moves &= !(1 << index);
        if mon.last_move_slot == Some(index) {
            mon.last_move_slot = None;
        }
        if sketch {
            self.add(format!(
                "|-activate|{}|move: Sketch|{}",
                self.ident(source),
                move_data.name
            ));
        } else {
            self.add(format!(
                "|-start|{}|Mimic|{}",
                self.ident(source),
                move_data.name
            ));
        }
        true
    }

    pub(super) fn conversion_two(&mut self, source: Position, target: Position) -> bool {
        // Dex.types.names(), in the pinned TypeChart's key order. An
        // ordinary sorted Rust table of display types isn't this API.
        const TYPE_ORDER: [(&str, &str); 18] = [
            ("bug", "Bug"),
            ("dark", "Dark"),
            ("dragon", "Dragon"),
            ("electric", "Electric"),
            ("fairy", "Fairy"),
            ("fighting", "Fighting"),
            ("fire", "Fire"),
            ("flying", "Flying"),
            ("ghost", "Ghost"),
            ("grass", "Grass"),
            ("ground", "Ground"),
            ("ice", "Ice"),
            ("normal", "Normal"),
            ("poison", "Poison"),
            ("psychic", "Psychic"),
            ("rock", "Rock"),
            ("steel", "Steel"),
            ("water", "Water"),
        ];
        let target = self.mon(target);
        let Some(ty) = target.last_used_type else {
            if !target.last_used_type_known {
                self.unsupported_called_move = Some((
                    "Conversion 2",
                    "the target's previous executed move type was not observed",
                ));
            }
            return false;
        };
        let mut candidates = [""; 18];
        let mut count = 0;
        for (key, name) in TYPE_ORDER {
            if !self.mon(source).has_type(name)
                && DEX
                    .type_data(key)
                    .is_some_and(|data| matches!(data.damage_taken.get(ty), Some(2 | 3)))
            {
                candidates[count] = name;
                count += 1;
            }
        }
        if count == 0 {
            return false;
        }
        let typing = candidates[self.rng.below(count as u32) as usize];
        if matches!(
            self.mon(source).species.base_species.as_str(),
            "Arceus" | "Silvally"
        ) {
            return false;
        }
        self.mon_mut(source).set_temporary_type(Some(typing));
        self.add(format!(
            "|-start|{}|typechange|{typing}",
            self.ident(source)
        ));
        true
    }
}
