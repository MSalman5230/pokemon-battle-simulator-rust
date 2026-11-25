//! Ordered side conditions that apply when a Pokémon enters the field.

use super::*;
use super::{abilities::BoostOrigin, turn::Position};
use crate::mechanics::BoostStat;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(super) enum Hazard {
    StealthRock,
    Spikes,
    ToxicSpikes,
    StickyWeb,
}

impl Hazard {
    pub(super) fn from_attack(name: &str) -> Option<Self> {
        match name {
            "Stone Axe" => Some(Self::StealthRock),
            "Ceaseless Edge" => Some(Self::Spikes),
            _ => None,
        }
    }

    pub(super) fn from_move(name: &str) -> Option<Self> {
        Some(match name {
            "Stealth Rock" => Self::StealthRock,
            "Spikes" => Self::Spikes,
            "Toxic Spikes" => Self::ToxicSpikes,
            "Sticky Web" => Self::StickyWeb,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::StealthRock => "move: Stealth Rock",
            Self::Spikes => "Spikes",
            Self::ToxicSpikes => "move: Toxic Spikes",
            Self::StickyWeb => "move: Sticky Web",
        }
    }

    fn cap(self) -> u8 {
        match self {
            Self::Spikes => 3,
            Self::ToxicSpikes => 2,
            _ => 1,
        }
    }
}

impl Battle {
    pub(super) fn defog(&mut self, source: Position, target: Position, data: &Move) -> bool {
        let mut changed = (self.mon(target).substitute_hp == 0
            || self.event_ability(source) == "infiltrator")
            && self.boost_from(
                target,
                source,
                &[(BoostStat::Evasion, -1)],
                BoostOrigin::from_move(data),
            );
        self.break_screens(target.player);
        let ident = self.ident(source);
        for player in [target.player, source.player] {
            for hazard in [
                Hazard::Spikes,
                Hazard::ToxicSpikes,
                Hazard::StealthRock,
                Hazard::StickyWeb,
            ] {
                let hazards = &mut self.sides[player.index()].as_mut().unwrap().hazards;
                let len = hazards.len();
                hazards.retain(|&(h, _)| h != hazard);
                if hazards.len() != len {
                    let label = hazard.label().trim_start_matches("move: ");
                    self.add(format!(
                        "|-sideend|{}|{label}|[from] move: Defog|[of] {ident}",
                        self.side_ident(player)
                    ));
                    changed = true;
                }
            }
        }
        self.clear_terrain();
        changed
    }

    pub(super) fn clear_spinning_hazards(&mut self, pos: Position, move_name: &str) {
        if self.mon(pos).hp == 0 {
            return;
        }
        let source = self.ident(pos);
        let suffix = format!("|[from] move: {move_name}|[of] {source}");
        if self.mon_mut(pos).seeded_from.take().is_some() {
            self.mon_mut(pos).untrack_volatile("leechseed");
            self.add(format!("|-end|{source}|Leech Seed{suffix}"));
        }
        for hazard in [
            Hazard::Spikes,
            Hazard::ToxicSpikes,
            Hazard::StealthRock,
            Hazard::StickyWeb,
        ] {
            let hazards = &mut self.sides[pos.player.index()].as_mut().unwrap().hazards;
            let len = hazards.len();
            hazards.retain(|&(h, _)| h != hazard);
            if hazards.len() != len {
                let label = hazard
                    .label()
                    .strip_prefix("move: ")
                    .unwrap_or(hazard.label());
                self.add(format!(
                    "|-sideend|{}|{label}{suffix}",
                    self.side_ident(pos.player)
                ));
            }
        }
        self.end_partial_trap(pos, false);
    }

    pub(super) fn grounded(&self, pos: Position) -> bool {
        let mon = self.mon(pos);
        match mon.effective_item() {
            "ironball" => true,
            "airballoon" => false,
            _ => !mon.has_type("Flying") && self.airborne_ability(pos).is_none(),
        }
    }

    pub(super) fn tidy_hazards(&mut self, pos: Position) -> bool {
        let mut removed = false;
        for active in self.positions() {
            if self.mon(active).hp > 0 {
                removed |= self.end_substitute(active);
            }
        }
        for player in [pos.player, pos.player.foe()] {
            for hazard in [
                Hazard::Spikes,
                Hazard::ToxicSpikes,
                Hazard::StealthRock,
                Hazard::StickyWeb,
            ] {
                let hazards = &mut self.sides[player.index()].as_mut().unwrap().hazards;
                let before = hazards.len();
                hazards.retain(|&(h, _)| h != hazard);
                if hazards.len() != before {
                    let label = hazard
                        .label()
                        .strip_prefix("move: ")
                        .unwrap_or(hazard.label());
                    self.add(format!("|-sideend|{}|{label}", self.side_ident(player)));
                    removed = true;
                }
            }
        }
        if removed {
            self.add(format!("|-activate|{}|move: Tidy Up", self.ident(pos)));
        }
        removed
    }

    pub(super) fn court_change(&mut self, source: Position) -> bool {
        let [p1, p2] = &mut self.sides;
        let p1 = p1.as_mut().unwrap();
        let p2 = p2.as_mut().unwrap();
        if p1
            .screens
            .iter()
            .chain(&p2.screens)
            .all(|&duration| duration == 0)
            && p1.hazards.is_empty()
            && p2.hazards.is_empty()
            && p1.tailwind == 0
            && p2.tailwind == 0
        {
            return false;
        }
        std::mem::swap(&mut p1.screens, &mut p2.screens);
        std::mem::swap(&mut p1.hazards, &mut p2.hazards);
        std::mem::swap(&mut p1.tailwind, &mut p2.tailwind);
        self.add("|-swapsideconditions");
        self.add(format!(
            "|-activate|{}|move: Court Change",
            self.ident(source)
        ));
        true
    }

    pub(super) fn add_hazard(&mut self, player: Player, hazard: Hazard) -> bool {
        let side = self.sides[player.index()].as_mut().unwrap();
        if let Some((_, layers)) = side.hazards.iter_mut().find(|(h, _)| *h == hazard) {
            if *layers >= hazard.cap() {
                return false;
            }
            *layers += 1;
        } else {
            side.hazards.push((hazard, 1));
        }
        self.add(format!(
            "|-sidestart|{}|{}",
            self.side_ident(player),
            hazard.label()
        ));
        true
    }

    pub(super) fn scatter_toxic_debris(&mut self, target: Position, source: Position) {
        let player = if target.player == source.player {
            source.player.foe()
        } else {
            source.player
        };
        let hazard = Hazard::ToxicSpikes;
        if self.sides[player.index()]
            .as_ref()
            .unwrap()
            .hazards
            .iter()
            .any(|&(id, layers)| id == hazard && layers >= hazard.cap())
        {
            return;
        }
        self.add(format!(
            "|-activate|{}|ability: Toxic Debris",
            self.ident(target)
        ));
        self.add_hazard(player, hazard);
    }

    pub(super) fn side_ident(&self, player: Player) -> String {
        format!(
            "{}: {}",
            player.id(),
            self.sides[player.index()].as_ref().unwrap().name
        )
    }

    pub(super) fn on_switch_in_hazards(&mut self, pos: Position) {
        let player = pos.player;
        let foe = Position {
            player: player.foe(),
            slot: 0,
        };
        // Side conditions have equal speed, order and priority. Showdown's
        // SwitchIn comparator breaks the tie by creation order.
        let hazards = self.sides[player.index()].as_ref().unwrap().hazards.clone();
        for (hazard, layers) in hazards {
            let mon = self.mon(pos);
            if mon.hp == 0 {
                break;
            }
            let grounded = self.grounded(pos);
            let boots = mon.effective_item() == "heavydutyboots";
            let max_hp = mon.max_hp.get();
            match hazard {
                Hazard::StealthRock if !boots => {
                    let move_data = DEX.moves(self.format.dex_mod, "stealthrock").unwrap();
                    let stages = self.effectiveness(move_data, pos).unwrap_or(0);
                    let damage = if stages >= 0 {
                        (max_hp << stages) / 8
                    } else {
                        (max_hp >> -stages) / 8
                    };
                    self.hazard_damage(pos, damage, "|[from] Stealth Rock");
                }
                Hazard::Spikes if grounded && !boots => {
                    let damage = [0, 3, 4, 6][usize::from(layers)] * max_hp / 24;
                    self.hazard_damage(pos, damage, "|[from] Spikes");
                }
                Hazard::ToxicSpikes if grounded => {
                    if mon.has_type("Poison") {
                        self.sides[player.index()]
                            .as_mut()
                            .unwrap()
                            .hazards
                            .retain(|(h, _)| *h != hazard);
                        self.add(format!(
                            "|-sideend|{}|{}|[of] {}",
                            self.side_ident(player),
                            hazard.label(),
                            self.ident(pos)
                        ));
                    } else if !boots
                        && mon.status == Status::Healthy
                        && !self.status_immune(pos, "psn")
                    {
                        let id = if layers >= 2 { "tox" } else { "psn" };
                        let prevented = self.status_blocked_by_ability(pos, id, false)
                            || self.flower_veil_status(pos, foe, id, false);
                        if !prevented && !self.field_blocks_status(pos, id, foe.player, false) {
                            self.apply_status(pos, id, foe.player, None);
                        }
                    }
                }
                Hazard::StickyWeb if grounded && !boots => {
                    self.add(format!("|-activate|{}|move: Sticky Web", self.ident(pos)));
                    self.boost_from(pos, foe, &[(BoostStat::Spe, -1)], BoostOrigin::Hazard);
                }
                _ => {}
            }
        }
    }

    fn hazard_damage(&mut self, pos: Position, damage: u32, reason: &str) {
        let mon = self.mon_mut(pos);
        if mon.ability == "magicguard" {
            return;
        }
        mon.half_hp_berry_deferred = false;
        mon.hp = mon.hp.saturating_sub(damage.max(1));
        self.add_health_line_with("-damage", pos, false, reason);
        if self.mon(pos).hp == 0 {
            self.mark_fainted(pos);
        }
    }
}

impl Pokemon {
    pub(super) fn element_bit(ty: &str) -> u8 {
        match ty {
            "Fire" => 1,
            "Electric" => 2,
            _ => 0,
        }
    }

    pub(super) fn has_type(&self, name: &str) -> bool {
        self.types().any(|t| t == name)
    }

    pub fn types(&self) -> impl Iterator<Item = &str> {
        let original_types = if self
            .transformed
            .as_ref()
            .is_some_and(|tr| tr.uses_base_types)
        {
            &self.base_species.types
        } else {
            &self.species.types
        };
        let lone = self.temporary_type.or_else(|| {
            (self.roosted && original_types.as_slice() == ["Flying"]).then_some("Normal")
        });
        let lone = lone.map(|ty| {
            if self.roosted && ty == "Flying" {
                "Normal"
            } else {
                ty
            }
        });
        original_types
            .iter()
            .map(String::as_str)
            .filter(move |t| lone.is_none() && (!self.roosted || *t != "Flying"))
            .chain(lone)
            .map(|ty| {
                if self.spent_elements & Self::element_bit(ty) != 0 {
                    "???"
                } else {
                    ty
                }
            })
            .chain(self.added_type)
    }
}
