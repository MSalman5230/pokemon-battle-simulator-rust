//! Inherited attacks that Champions National Dex Draft can admit.

use super::{terrain::Terrain, turn::Position, *};
use crate::dex::Mod;
use crate::mechanics::BoostStat;
use std::sync::LazyLock;

pub(super) fn secret_power(dex_mod: Mod, terrain: Terrain) -> &'static Move {
    static MOVES: LazyLock<[[Move; 4]; 2]> = LazyLock::new(|| {
        [Mod::Champions, Mod::RegMb].map(|dex_mod| {
            let base = DEX
                .moves(dex_mod, "secretpower")
                .expect("Secret Power is inherited");
            [
                Terrain::Electric,
                Terrain::Grassy,
                Terrain::Misty,
                Terrain::Psychic,
            ]
            .map(|terrain| {
                let mut data = base.clone();
                let effect = &mut data.secondaries[0];
                match terrain {
                    Terrain::Electric => {}
                    Terrain::Grassy => effect.status = Some("slp".into()),
                    Terrain::Misty | Terrain::Psychic => {
                        effect.status = None;
                        let stat = if terrain == Terrain::Misty {
                            BoostStat::Spa
                        } else {
                            BoostStat::Spe
                        };
                        effect.boosts.push((stat, -1));
                    }
                }
                data
            })
        })
    });
    &MOVES[usize::from(dex_mod == Mod::RegMb)][match terrain {
        Terrain::Electric => 0,
        Terrain::Grassy => 1,
        Terrain::Misty => 2,
        Terrain::Psychic => 3,
    }]
}

pub(super) fn magnitude(roll: u32) -> (u8, u16) {
    match roll {
        0..5 => (4, 10),
        5..15 => (5, 30),
        15..35 => (6, 50),
        35..65 => (7, 70),
        65..85 => (8, 90),
        85..95 => (9, 110),
        _ => (10, 150),
    }
}

pub(super) fn fixed_damage(move_name: &str, target_hp: u32) -> Option<u32> {
    Some(match move_name {
        "Dragon Rage" => 40,
        "Sonic Boom" => 20,
        "Super Fang" | "Nature's Madness" | "Ruination" => (target_hp / 2).max(1),
        _ => return None,
    })
}

impl Battle {
    pub(super) fn legacy_base_power(
        &mut self,
        target: Position,
        name: &str,
        power: u16,
        pending: bool,
    ) -> u16 {
        let mon = self.mon(target);
        let label = match name {
            "Wake-Up Slap"
                if matches!(mon.status, Status::Sleep(_))
                    || self.event_ability(target) == "comatose" =>
            {
                "BP doubled on sleeping target"
            }
            "Smelling Salts" if mon.status == Status::Paralysis => "BP doubled on paralyzed target",
            "Bolt Beak" | "Fishious Rend" => {
                let doubled = pending || mon.active_turns == 0;
                if self.format.report_exact_hp {
                    self.add(format!(
                        "|debug|{name} {}",
                        if doubled {
                            "damage boost"
                        } else {
                            "NOT boosted"
                        }
                    ));
                }
                return if doubled { power * 2 } else { power };
            }
            "Crush Grip" | "Wring Out" => {
                let (hp, max_hp) = (mon.hp, mon.max_hp.get());
                let ratio = hp * 4096 / max_hp;
                let power = ((120 * (100 * ratio + 2047)) / 4096 / 100).max(1);
                if self.format.report_exact_hp {
                    self.add(format!("|debug|BP for {hp}/{max_hp} HP: {power}"));
                }
                return power as u16;
            }
            _ => return power,
        };
        if self.format.report_exact_hp {
            self.add(format!("|debug|{label}"));
        }
        power * 2
    }

    pub(super) fn legacy_attack_hit(&mut self, target: Position, name: &str) {
        let mon = self.mon(target);
        match (name, mon.status) {
            ("Smelling Salts", Status::Paralysis) | ("Wake-Up Slap", Status::Sleep(_))
                if mon.hp > 0 =>
            {
                self.cure_status(target, "[msg]");
            }
            ("Incinerate", _) => {
                let item = DEX.item(self.format.dex_mod, &mon.item);
                if item.is_some_and(|item| item.is_berry || mon.item == "normalgem")
                    && let Some(label) = self.can_take_item(target)
                {
                    self.lose_held_item(target);
                    self.add(format!(
                        "|-enditem|{}|{label}|[from] move: Incinerate",
                        self.ident(target)
                    ));
                }
            }
            _ => {}
        }
    }
}
