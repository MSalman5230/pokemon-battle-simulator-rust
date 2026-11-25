//! Commander ties two active roster members together without removing either slot.

use super::{turn::Position, *};
use crate::mechanics::BoostStat;

impl Pokemon {
    pub(super) fn is_command_locked(&self) -> bool {
        self.commanding || self.commanded_by.is_some()
    }
}

impl Battle {
    pub(super) fn update_commander(&mut self, pos: Position) {
        if self.slots() != 2 || self.event_ability(pos) != "commander" || self.mon(pos).hp == 0 {
            return;
        }
        let ally = Position {
            slot: 1 - pos.slot,
            ..pos
        };
        let user = self.mon(pos);
        let partner = self.mon(ally);
        if user.pivot_move.is_some() || partner.pivot_move.is_some() {
            return;
        }
        if user.base_species.base_species != "Tatsugiri"
            || partner.base_species.base_species != "Dondozo"
            || partner.hp == 0
        {
            self.release_commander(pos);
            return;
        }
        if user.commanding || partner.commanded_by.is_some() {
            return;
        }
        let roster = user.roster_index as u8;
        self.add(format!(
            "|-activate|{}|ability: Commander|[of] {}",
            self.ident(pos),
            self.ident(ally)
        ));
        self.mon_mut(pos).commanding = true;
        self.mon_mut(pos).track_volatile("commanding");
        self.mon_mut(ally).commanded_by = Some(roster);
        self.mon_mut(ally).track_volatile("commanded");
        self.boost(
            ally,
            &[
                (BoostStat::Atk, 2),
                (BoostStat::Spa, 2),
                (BoostStat::Spe, 2),
                (BoostStat::Def, 2),
                (BoostStat::Spd, 2),
            ],
            true,
        );
    }

    pub(super) fn release_commander(&mut self, pos: Position) {
        let mon = self.mon_mut(pos);
        if std::mem::take(&mut mon.commanding) {
            mon.untrack_volatile("commanding");
        }
    }

    pub(super) fn order_up_boost(&mut self, pos: Position) {
        let Some(roster) = self.mon(pos).commanded_by else {
            return;
        };
        let Some(commander) = self.party(pos.player).unwrap().iter().find(|mon| {
            mon.roster_index == usize::from(roster) && mon.base_species.base_species == "Tatsugiri"
        }) else {
            return;
        };
        let stat = match commander.base_species.name.as_str() {
            "Tatsugiri-Droopy" => BoostStat::Def,
            "Tatsugiri-Stretchy" => BoostStat::Spe,
            _ => BoostStat::Atk,
        };
        self.boost(pos, &[(stat, 1)], true);
    }
}
