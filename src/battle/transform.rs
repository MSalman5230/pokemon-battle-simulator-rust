//! Temporary Transform state is separate from the submitted species and PP.

use super::{turn::Position, *};

#[derive(Clone)]
pub(super) struct Transformed {
    pub(super) original_moves: Vec<MoveSlot>,
    pub(super) original_stats: Stats,
    pub(super) uses_base_types: bool,
}

impl Battle {
    pub(super) fn imposter_on_entry(&mut self, source: Position) {
        let target = Position {
            player: source.player.foe(),
            slot: self.format.game_type.active_slots() - 1 - source.slot,
        };
        self.transform_into(source, target, true);
    }

    pub(super) fn transform_into(
        &mut self,
        source: Position,
        target: Position,
        imposter: bool,
    ) -> bool {
        let original = self.mon(source);
        let model = self.mon(target);
        if source == target
            || model.fainted
            || model.substitute_hp != 0
            || model.illusion.is_some()
            || original.illusion.is_some()
            || model.transformed.is_some()
            || original.transformed.is_some()
            || model.species.name == "Eternatus-Eternamax"
        {
            return false;
        }
        let species = model.species;
        let ability = model.ability.clone();
        let boosts = model.boosts;
        let mut stats = model.stats;
        stats.hp = original.stats.hp;
        // setSpecies sets cached speed from the user's own points for the
        // copied species before the template's stored stats are copied.
        // The next normal speed refresh applies boosts and the copied stat.
        let speed_stat = crate::mechanics::Stat::Spe;
        let speed = self.stat_rules.stat(
            species.base_stats.spe,
            original.points.raw(speed_stat).clamp(0, 255) as u8,
            original.level,
            original.nature,
            speed_stat,
        );
        let temporary_types = model.temporary_types;
        let added_type = model.added_type;
        let spent_elements = model.spent_elements;
        let unrevealed_types = model.unrevealed_types;
        let unknown_type = source.player != target.player || model.unknown_type;
        let times_attacked = model.times_attacked;
        let weighthg = model.weighthg;
        let moves: Vec<_> = model
            .moves
            .iter()
            .map(|m| {
                let max_pp = m.move_data.pp.min(5);
                MoveSlot {
                    id: m.id.clone(),
                    move_data: m.move_data,
                    pp: max_pp,
                    max_pp,
                }
            })
            .collect();
        let new_slot = |slot: usize| {
            original
                .moves
                .get(slot)
                .and_then(|m| moves.iter().position(|copy| m.id == copy.id))
        };
        let choice_lock = original
            .choice_lock
            .map(|slot| new_slot(slot).unwrap_or(usize::MAX));
        let last_move_slot = original.last_move_slot.and_then(new_slot);
        let disabled = original
            .disabled_slot
            .map(|(slot, turns)| (new_slot(slot).unwrap_or(usize::MAX), turns));
        let encore = original
            .encore
            .map(|(slot, turns)| (new_slot(slot).unwrap_or(usize::MAX), turns));

        let mon = self.mon_mut(source);
        mon.transformed = Some(Box::new(Transformed {
            original_moves: std::mem::replace(&mut mon.moves, moves),
            original_stats: mon.stats,
            uses_base_types: false,
        }));
        mon.species = species;
        mon.weighthg = weighthg;
        mon.stats = stats;
        mon.speed = speed as i32;
        mon.boosts = boosts;
        mon.temporary_types = temporary_types;
        mon.unrevealed_types = unrevealed_types;
        mon.unknown_type = unknown_type;
        mon.added_type = added_type;
        mon.spent_elements = spent_elements;
        mon.times_attacked = times_attacked;
        mon.used_moves = 0;
        mon.choice_lock = choice_lock;
        mon.last_move_slot = last_move_slot;
        mon.disabled_slot = disabled;
        mon.encore = encore;
        self.copy_critical_conditions(source, target);

        self.add(format!(
            "|-transform|{}|{}{}",
            self.ident(source),
            self.ident(target),
            if imposter {
                "|[from] ability: Imposter"
            } else {
                ""
            }
        ));
        let changed = self.mon(source).ability != ability;
        self.end_ability(source);
        self.possible_unaware |= ability == "unaware";
        self.mon_mut(source).ability = ability;
        self.reset_ability_order(source);
        if changed && !self.event_ability(source).is_empty() {
            self.start_ability(source);
        }
        true
    }
}
