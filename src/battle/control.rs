//! Explicit headless battle controls. Clocks do not run inside the engine.

use super::*;

impl Battle {
    pub(super) fn finish(&mut self, winner: Option<Player>) {
        if self.phase == Phase::Ended {
            return;
        }
        self.add("|");
        self.winner = winner.filter(|&p| self.sides[p.index()].is_some());
        if let Some(player) = self.winner {
            self.add(format!(
                "|win|{}",
                self.sides[player.index()].as_ref().unwrap().name
            ));
        } else {
            self.add("|tie");
        }
        self.phase = Phase::Ended;
        self.pending_actions = [None, None];
        self.pending_preview = [None, None];
        self.suspended.clear();
    }

    pub(super) fn tiebreak(&mut self) -> Result<(), BattleError> {
        if self.phase == Phase::Ended {
            return Ok(());
        }
        let [Some(p1), Some(p2)] = &self.sides else {
            return Err(BattleError::Input("tiebreak requires two players"));
        };
        let names = [p1.name.clone(), p2.name.clone()];
        let party = [&p1.party, &p2.party];
        let count = party.map(|p| p.iter().filter(|m| !m.fainted).count());
        let percent = party.map(|p| {
            p.iter()
                .map(|m| f64::from(m.hp) / f64::from(m.max_hp.get()))
                .sum::<f64>()
                * 100.0
                / 6.0
        });
        let total = party.map(|p| p.iter().map(|m| m.hp).sum::<u32>());
        let [a, b] = names;
        self.add("|message|Time's up! Going to tiebreaker...");
        self.add(format!(
            "|-message|{a}: {} Pokemon left; {b}: {} Pokemon left",
            count[0], count[1]
        ));
        if count[0] == count[1] {
            self.add(format!(
                "|-message|{a}: {}% total HP left; {b}: {}% total HP left",
                percent[0].round(),
                percent[1].round()
            ));
            if percent[0] == percent[1] {
                self.add(format!(
                    "|-message|{a}: {} total HP left; {b}: {} total HP left",
                    total[0], total[1]
                ));
            }
        }
        let result = count[0]
            .cmp(&count[1])
            .then_with(|| percent[0].total_cmp(&percent[1]))
            .then_with(|| total[0].cmp(&total[1]));
        self.finish(match result {
            std::cmp::Ordering::Greater => Some(Player::P1),
            std::cmp::Ordering::Less => Some(Player::P2),
            std::cmp::Ordering::Equal => None,
        });
        Ok(())
    }
}
