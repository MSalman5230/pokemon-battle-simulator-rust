//! Illusion changes only the protocol identity, not the combatant or request.

use super::{turn::Position, *};
use std::borrow::Cow;

impl Battle {
    pub(super) fn prepare_illusion(&mut self, pos: Position) {
        if self.event_ability(pos) != "illusion" {
            return;
        }
        let disguise = self.party(pos.player).unwrap()[pos.slot + 1..]
            .iter()
            .rfind(|mon| !mon.fainted)
            .map(|mon| mon.roster_index);
        self.mon_mut(pos).illusion = disguise;
    }

    pub(super) fn disguise(&self, pos: Position) -> Option<&Pokemon> {
        let roster_index = self.mon(pos).illusion?;
        self.party(pos.player)
            .unwrap()
            .iter()
            .find(|mon| mon.roster_index == roster_index)
    }

    pub(super) fn displayed_details(&self, pos: Position) -> Cow<'_, str> {
        let mon = self.mon(pos);
        let Some(disguise) = self.disguise(pos) else {
            return Cow::Borrowed(&mon.details);
        };
        if disguise.level == mon.level || self.format.has_rule("illusionlevelmod") {
            return Cow::Borrowed(&disguise.details);
        }
        let (species, mut suffix) = disguise
            .details
            .split_once(',')
            .map_or((disguise.details.as_str(), ""), |(a, _)| {
                (a, &disguise.details[a.len()..])
            });
        if suffix.starts_with(", L") {
            suffix = suffix[2..].find(',').map_or("", |i| &suffix[i + 2..]);
        }
        if mon.level == 100 {
            Cow::Owned(format!("{species}{suffix}"))
        } else {
            Cow::Owned(format!("{species}, L{}{suffix}", mon.level))
        }
    }

    pub(super) fn end_illusion(&mut self, pos: Position) {
        if self.mon_mut(pos).illusion.take().is_none() {
            return;
        }
        if self.format.report_exact_hp {
            self.add("|debug|illusion cleared");
        }
        let ident = self.ident(pos);
        self.add(format!("|replace|{ident}|{}", self.mon(pos).details));
        self.add(format!("|-end|{ident}|Illusion"));
        const HINT: &str =
            "|-hint|Illusion Level Mod is active, so this Pokémon's true level was hidden.";
        if self.format.has_rule("illusionlevelmod")
            && !self.log.iter().any(|line| line.public == HINT)
        {
            self.add(HINT);
        }
    }
}
