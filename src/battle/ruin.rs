//! Ruin abilities never stack. The native move remembers the contributing
//! holder separately for each stat, including across spread and repeated hits.

use super::{turn::Position, *};
use crate::mechanics::BoostStat;

#[derive(Clone, Copy, Default)]
pub(super) struct RuinCache([Option<(Player, usize)>; 4]);

pub(super) fn ruin_label(ability: &str) -> Option<&'static str> {
    match ability {
        "tabletsofruin" => Some("Tablets of Ruin"),
        "swordofruin" => Some("Sword of Ruin"),
        "vesselofruin" => Some("Vessel of Ruin"),
        "beadsofruin" => Some("Beads of Ruin"),
        _ => None,
    }
}

impl Battle {
    pub(super) fn ruin_modifier(
        &mut self,
        pos: Position,
        stat: BoostStat,
        cache: &mut RuinCache,
    ) -> u32 {
        if !self.possible_ruin {
            return 4096;
        }
        let (ability, debug, replace_gone) = match stat {
            BoostStat::Atk => ("tabletsofruin", "Tablets of Ruin Atk drop", false),
            BoostStat::Def => ("swordofruin", "Sword of Ruin Def drop", true),
            BoostStat::Spa => ("vesselofruin", "Vessel of Ruin SpA drop", false),
            BoostStat::Spd => ("beadsofruin", "Beads of Ruin SpD drop", true),
            _ => return 4096,
        };
        let mut handlers = [(pos, 0); 4];
        let mut count = 0;
        for holder in self.positions() {
            let mon = self.mon(holder);
            if mon.ability == ability && !mon.fainted {
                handlers[count] = (holder, mon.speed);
                count += 1;
            }
        }
        speed_sort_by(&mut handlers[..count], &mut self.rng, |a, b| b.1.cmp(&a.1));
        if self.event_ability(pos) == ability {
            return 4096;
        }
        let cached = &mut cache.0[stat as usize];
        for &(holder, _) in &handlers[..count] {
            if self.ability_suppressed(holder) {
                if self.format.report_exact_hp {
                    let event = match stat {
                        BoostStat::Atk => "ModifyAtk",
                        BoostStat::Def => "ModifyDef",
                        BoostStat::Spa => "ModifySpA",
                        _ => "ModifySpD",
                    };
                    self.log_suppressed_ability(holder, event);
                }
                continue;
            }
            let identity = (holder.player, self.mon(holder).roster_index);
            if cached.is_none()
                || replace_gone
                    && !handlers[..count].iter().any(|&(other, _)| {
                        *cached == Some((other.player, self.mon(other).roster_index))
                            && self.event_ability(other) == ability
                    })
            {
                *cached = Some(identity);
            }
            if *cached == Some(identity) {
                if self.format.report_exact_hp {
                    self.add(format!("|debug|{debug}"));
                }
                return 3072;
            }
        }
        4096
    }

    pub(super) fn field_power_ability(&mut self, pos: Position, physical: bool) -> u32 {
        let label = match self.event_ability(pos) {
            "hadronengine" if !physical && self.terrain() == Some(Terrain::Electric) => {
                "Hadron Engine boost"
            }
            "orichalcumpulse"
                if physical
                    && self
                        .mon(pos)
                        .personal_weather(self.effective_weather())
                        .map(Weather::ordinary)
                        == Some(Weather::Sun) =>
            {
                "Orichalcum boost"
            }
            _ => return 4096,
        };
        if self.format.report_exact_hp {
            self.add(format!("|debug|{label}"));
        }
        5461
    }
}
