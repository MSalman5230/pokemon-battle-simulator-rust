//! Voluntary switching and private discovery of trapping abilities.

use super::{turn::Position, *};

#[derive(Default, Copy, Clone)]
pub(super) struct Trap {
    pub actual: bool,
    pub possible: bool,
}

impl Battle {
    pub(super) fn trap_info(&self, pos: Position) -> Trap {
        let mon = self.mon(pos);
        if mon.recharge > 0 {
            return Trap {
                actual: true,
                possible: true,
            };
        }
        if mon.fainted || mon.has_type("Ghost") {
            return Trap::default();
        }
        let affected = |ability: &str| match ability {
            "arenatrap" => mon.grounded(),
            "magnetpull" => mon.has_type("Steel"),
            "shadowtag" => mon.ability != "shadowtag",
            _ => false,
        };
        let foe = self.party(pos.player.foe()).unwrap();
        let active = &foe[..self.format.game_type.active_slots()];
        let live = active.iter().filter(|mon| !mon.fainted);
        let actual = mon.item != "shedshell" && live.clone().any(|foe| affected(&foe.ability));
        let possible = actual
            || live
                .flat_map(|foe| {
                    foe.species
                        .abilities
                        .iter()
                        .filter(|(slot, _)| **slot != "H" || !foe.species.unreleased_hidden)
                })
                .any(|(_, name)| {
                    let id = to_id(name);
                    affected(&id) && !self.format.has_rule(&format!("-ability:{id}"))
                });
        Trap { actual, possible }
    }

    pub(super) fn last_active(&self, pos: Position) -> bool {
        let slots = self.format.game_type.active_slots();
        self.party(pos.player).unwrap()[pos.slot + 1..slots]
            .iter()
            .all(|p| p.fainted)
    }

    /// Whether the owner currently has an affirmative ("trapped") request.
    /// This intentionally does not expose the true value of a hidden trap.
    pub fn trapped_in_request(&self, player: Player, slot: usize) -> bool {
        let pos = Position { player, slot };
        let mon = self.mon(pos);
        mon.recharge > 0
            || mon.revealed_trap
            || (!self.last_active(pos)
                && self.trap_info(pos).actual
                && self.party(player).unwrap()[self.format.game_type.active_slots()..]
                    .iter()
                    .any(|p| !p.fainted))
    }
}
