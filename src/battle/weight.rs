//! Weight uses the effective form and runs the target's weight modifiers.

use super::{turn::Position, *};

impl Battle {
    fn weight(&mut self, pos: Position) -> u32 {
        let mon = self.mon(pos);
        let mut weight = u32::from(mon.weighthg);
        let float = mon.effective_item() == "floatstone";
        match self.event_ability(pos) {
            "heavymetal" => weight *= 2,
            "lightmetal" => weight /= 2,
            _ if self.format.report_exact_hp
                && matches!(mon.active_ability(), "heavymetal" | "lightmetal") =>
            {
                self.add("|debug|ModifyWeight handler suppressed by Mold Breaker");
            }
            _ => {}
        }
        if float {
            weight /= 2;
        }
        weight.max(1)
    }

    pub(super) fn weight_move_power(
        &mut self,
        move_name: &str,
        source: Position,
        target: Position,
    ) -> Option<u16> {
        let power = match move_name {
            "Grass Knot" | "Low Kick" => match self.weight(target) {
                2000.. => 120,
                1000.. => 100,
                500.. => 80,
                250.. => 60,
                100.. => 40,
                _ => 20,
            },
            "Heavy Slam" | "Heat Crash" => {
                let defender = self.weight(target);
                match self.weight(source) / defender {
                    5.. => 120,
                    4 => 100,
                    3 => 80,
                    2 => 60,
                    _ => 40,
                }
            }
            _ => return None,
        };
        if self.format.report_exact_hp {
            self.add(format!("|debug|BP: {power}"));
        }
        Some(power)
    }
}
