//! Side-wide damage screens and their independently ordered expiration.

use super::{turn::Position, *};
use crate::dex::Category;

#[derive(Copy, Clone, Debug)]
pub(super) enum Screen {
    Reflect,
    Light,
    AuroraVeil,
}

impl Screen {
    pub(super) fn from_move(name: &str) -> Option<Self> {
        match name {
            "Reflect" => Some(Self::Reflect),
            "Light Screen" => Some(Self::Light),
            "Aurora Veil" => Some(Self::AuroraVeil),
            _ => None,
        }
    }

    pub(super) const ALL: [Self; 3] = [Self::Reflect, Self::Light, Self::AuroraVeil];

    fn label(self) -> &'static str {
        match self {
            Self::Reflect => "Reflect",
            Self::Light => "move: Light Screen",
            Self::AuroraVeil => "move: Aurora Veil",
        }
    }

    pub(super) fn sub_order(self) -> i32 {
        match self {
            Self::Reflect => 1,
            Self::Light => 2,
            Self::AuroraVeil => 10,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl Battle {
    pub(super) fn break_screens(&mut self, player: Player) {
        for screen in Screen::ALL {
            self.remove_screen(player, screen);
        }
    }

    fn remove_screen(&mut self, player: Player, screen: Screen) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().screens[screen.index()];
        if std::mem::take(turns) != 0 {
            self.add(format!(
                "|-sideend|{}|{}",
                self.side_ident(player),
                screen.label()
            ));
        }
    }

    pub(super) fn screen_cleaner(&mut self, pos: Position) {
        let mut announced = false;
        for screen in Screen::ALL {
            for player in [pos.player, pos.player.foe()] {
                if self.has_screen(player, screen) {
                    if !announced {
                        self.add(format!(
                            "|-activate|{}|ability: Screen Cleaner",
                            self.ident(pos)
                        ));
                        announced = true;
                    }
                    self.remove_screen(player, screen);
                }
            }
        }
    }

    pub(super) fn screens_damage_modifier(
        &self,
        source: Position,
        target: Position,
        category: Category,
        critical: bool,
    ) -> (u32, &'static str) {
        if source == target || critical || self.mon(source).ability == "infiltrator" {
            return (4096, "");
        }
        let own = &self.sides[target.player.index()].as_ref().unwrap().screens;
        let debug = match category {
            Category::Physical if own[Screen::Reflect.index()] != 0 => "Reflect weaken",
            Category::Special if own[Screen::Light.index()] != 0 => "Light Screen weaken",
            _ if own[Screen::AuroraVeil.index()] != 0 => "Aurora Veil weaken",
            _ => return (4096, ""),
        };
        (
            if self.format.game_type.active_slots() > 1 {
                2732
            } else {
                2048
            },
            debug,
        )
    }

    pub(super) fn set_screen(&mut self, source: Position, screen: Screen) -> bool {
        if matches!(screen, Screen::AuroraVeil) && self.effective_weather() != Some(Weather::Snow) {
            return false;
        }
        let turns = if self.mon(source).effective_item() == "lightclay" {
            8
        } else {
            5
        };
        let status =
            &mut self.sides[source.player.index()].as_mut().unwrap().screens[screen.index()];
        if *status != 0 {
            return false;
        }
        *status = turns;
        self.add(format!(
            "|-sidestart|{}|{}",
            self.side_ident(source.player),
            screen.label()
        ));
        true
    }

    pub(super) fn has_screen(&self, player: Player, screen: Screen) -> bool {
        self.sides[player.index()].as_ref().unwrap().screens[screen.index()] > 0
    }

    pub(super) fn tick_screen(&mut self, player: Player, screen: Screen) {
        let turns = &mut self.sides[player.index()].as_mut().unwrap().screens[screen.index()];
        *turns -= 1;
        if *turns == 0 {
            self.add(format!(
                "|-sideend|{}|{}",
                self.side_ident(player),
                screen.label()
            ));
        }
    }
}
