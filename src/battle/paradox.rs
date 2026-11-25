//! Field- and item-driven Paradox stat boosts.

use super::{turn::Position, *};
use crate::mechanics::{BoostStat, Stat, boosted_stat};

#[derive(Clone, Copy)]
pub(super) enum ParadoxKind {
    Protosynthesis,
    QuarkDrive,
}

impl ParadoxKind {
    pub(super) fn from_ability(id: &str) -> Option<Self> {
        match id {
            "protosynthesis" => Some(Self::Protosynthesis),
            "quarkdrive" => Some(Self::QuarkDrive),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Protosynthesis => "protosynthesis",
            Self::QuarkDrive => "quarkdrive",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Protosynthesis => "Protosynthesis",
            Self::QuarkDrive => "Quark Drive",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ParadoxBoost {
    pub kind: ParadoxKind,
    pub stat: BoostStat,
    pub from_item: bool,
}

impl Pokemon {
    pub(super) fn best_battle_stat(&self, unboosted: bool, wonder_room: bool) -> BoostStat {
        let mut winner = BoostStat::Atk;
        let mut highest = 0;
        for (stat, mut stage) in [
            (Stat::Atk, BoostStat::Atk),
            (Stat::Def, BoostStat::Def),
            (Stat::Spa, BoostStat::Spa),
            (Stat::Spd, BoostStat::Spd),
            (Stat::Spe, BoostStat::Spe),
        ] {
            if wonder_room {
                stage = match stage {
                    BoostStat::Def => BoostStat::Spd,
                    BoostStat::Spd => BoostStat::Def,
                    _ => stage,
                };
            }
            let raw = self.stats.get(stat);
            let value = if unboosted {
                raw
            } else {
                boosted_stat(raw, self.boosts.get(stage))
            };
            if value > highest {
                highest = value;
                winner = match stat {
                    Stat::Atk => BoostStat::Atk,
                    Stat::Def => BoostStat::Def,
                    Stat::Spa => BoostStat::Spa,
                    Stat::Spd => BoostStat::Spd,
                    Stat::Spe => BoostStat::Spe,
                    Stat::Hp => unreachable!(),
                };
            }
        }
        winner
    }
}

impl Battle {
    pub(super) fn sync_paradox(
        &mut self,
        pos: Position,
        weather: Option<Weather>,
        terrain: Option<Terrain>,
    ) {
        let Some(kind) = ParadoxKind::from_ability(&self.mon(pos).ability) else {
            return;
        };
        if self.mon(pos).hp == 0 || self.ability_suppressed(pos) {
            return;
        }
        let from_field = match kind {
            ParadoxKind::Protosynthesis => weather == Some(Weather::Sun),
            ParadoxKind::QuarkDrive => terrain == Some(Terrain::Electric),
        };
        if from_field {
            self.start_paradox(pos, kind, false);
        } else if self
            .mon(pos)
            .paradox_boost
            .is_some_and(|boost| !boost.from_item)
        {
            self.end_paradox(pos, kind, false);
        }
    }

    pub(super) fn start_booster_energy(&mut self, pos: Position) {
        if self.mon(pos).item == "boosterenergy" && !self.debug_ignored_item(pos, "Start") {
            self.mon_mut(pos).booster_energy_started = true;
            self.update_booster_energy(pos);
        }
    }

    pub(super) fn update_booster_energy(&mut self, pos: Position) {
        let mon = self.mon(pos);
        if !mon.booster_energy_started || mon.hp == 0 || mon.transformed.is_some() {
            return;
        }
        let Some(kind) = ParadoxKind::from_ability(self.event_ability(pos)) else {
            return;
        };
        if self.debug_ignored_item(pos, "Update") {
            return;
        }
        let field_powers = match kind {
            ParadoxKind::Protosynthesis => self.effective_weather() == Some(Weather::Sun),
            ParadoxKind::QuarkDrive => self.terrain() == Some(Terrain::Electric),
        };
        if !field_powers {
            self.consume(pos, "Booster Energy", false);
            self.symbiosis_after_item(pos);
            self.start_paradox(pos, kind, true);
        }
    }

    fn start_paradox(&mut self, pos: Position, kind: ParadoxKind, from_item: bool) {
        if self.mon(pos).paradox_boost.is_some() {
            return;
        }
        let stat = self.mon(pos).best_battle_stat(false, self.wonder_room != 0);
        let mon = self.mon_mut(pos);
        mon.paradox_boost = Some(ParadoxBoost {
            kind,
            stat,
            from_item,
        });
        mon.track_volatile(kind.id());
        let ident = self.ident(pos);
        let label = kind.label();
        let reason = if from_item { "|[fromitem]" } else { "" };
        self.add(format!("|-activate|{ident}|ability: {label}{reason}"));
        self.add(format!("|-start|{ident}|{}{}", kind.id(), stat.id()));
    }

    pub(super) fn end_paradox_ability(&mut self, pos: Position) {
        if let Some(kind) = ParadoxKind::from_ability(&self.mon(pos).ability) {
            self.end_paradox(pos, kind, true);
        }
    }

    fn end_paradox(&mut self, pos: Position, kind: ParadoxKind, silent: bool) {
        let mon = self.mon_mut(pos);
        mon.paradox_boost = None;
        mon.untrack_volatile(kind.id());
        let reason = if silent { "|[silent]" } else { "" };
        self.add(format!(
            "|-end|{}|{}{reason}",
            self.ident(pos),
            kind.label()
        ));
    }

    pub(super) fn paradox_modifier(&mut self, pos: Position, stat: BoostStat) -> u32 {
        let Some(boost) = self.mon(pos).paradox_boost else {
            return 4096;
        };
        if boost.stat != stat || self.ability_suppressed(pos) {
            return 4096;
        }
        if self.format.report_exact_hp {
            self.add(format!("|debug|{} {} boost", boost.kind.label(), stat.id()));
        }
        if stat == BoostStat::Spe { 6144 } else { 5325 }
    }
}
