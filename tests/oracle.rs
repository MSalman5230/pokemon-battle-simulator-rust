use pokemon_battle_simulator::{
    choice::{Choice, MoveChoice, SlotChoice, SpecialChoice, TargetKind},
    dex::{DEX, GameType, Mod},
    mechanics::{Stat, health},
    order::{Priority, speed_sort_by},
    rng::Rng,
};
use serde::Deserialize;
use std::num::NonZeroU32;

#[derive(Deserialize)]
struct Oracle {
    revision: String,
    rng: Vec<RngCase>,
    sorting: Vec<SortCase>,
    stat: Vec<StatCase>,
    health: Vec<HealthCase>,
}

#[derive(Deserialize)]
struct RngCase {
    seed: String,
    words: Vec<u32>,
    bounded: Vec<(u32, u32)>,
    after: String,
}

#[derive(Deserialize)]
struct SortCase {
    seed: String,
    input: Vec<Node>,
    output: Vec<usize>,
    after: String,
}

#[derive(Deserialize)]
struct Node {
    i: usize,
    speed: i32,
    priority: f64,
}

impl Node {
    fn priority(&self) -> Priority {
        Priority {
            speed: self.speed,
            priority: self.priority,
            ..Priority::default()
        }
    }
}

#[derive(Deserialize)]
struct StatCase {
    mode: usize,
    name: Stat,
    points: u8,
    base: u32,
    level: u16,
    nature: String,
    result: u32,
}

#[derive(Deserialize)]
struct HealthCase {
    hp: u32,
    maxhp: NonZeroU32,
    status: String,
    exact: bool,
    secret: String,
    shared: String,
}

fn oracle() -> Oracle {
    let data: Oracle = serde_json::from_str(include_str!("fixtures/mechanics.json")).unwrap();
    assert_eq!(data.revision, DEX.revision);
    data
}

#[test]
fn seeded_draws_match_showdown() {
    for case in oracle().rng {
        let mut rng = Rng::new(case.seed.parse().unwrap());
        for expected in case.words {
            assert_eq!(rng.next_word(), expected, "initial seed {}", case.seed);
        }
        for (upper, expected) in case.bounded {
            assert_eq!(rng.below(upper), expected);
        }
        assert_eq!(rng.seed().to_string(), case.after);
    }
}

#[test]
fn action_order_and_end_seed_match_showdown() {
    for case in oracle().sorting {
        let mut rng = Rng::new(case.seed.parse().unwrap());
        let mut nodes = case.input;
        speed_sort_by(&mut nodes, &mut rng, |a, b| {
            a.priority().compare(b.priority())
        });
        assert_eq!(
            nodes.into_iter().map(|x| x.i).collect::<Vec<_>>(),
            case.output
        );
        assert_eq!(rng.seed().to_string(), case.after);
    }
}

#[test]
fn champions_stats_match_showdown() {
    let formats = ["bssregmc", "bssregmb", "randombattle", "customgame"];
    for case in oracle().stat {
        let f = DEX
            .format(&format!("gen9champions{}", formats[case.mode]))
            .unwrap();
        let rules = f.stat_rules();
        let nature = DEX.nature(&case.nature);
        assert_eq!(
            rules.stat(case.base, case.points, case.level, nature, case.name),
            case.result,
            "{} {:?} {} {}",
            f.name,
            case.name,
            case.points,
            case.nature,
        );
    }
}

#[test]
fn health_channels_match_showdown() {
    for h in oracle().health {
        let got = health(h.hp, h.maxhp, &h.status, h.exact);
        assert_eq!(got.secret, h.secret);
        assert_eq!(got.shared, h.shared);
    }
}

#[test]
fn dex_uses_the_effective_mod_and_covers_the_full_config() {
    assert_eq!(DEX.formats().count(), 18);
    let doubles = DEX.format("gen9championsvgc2026regmc").unwrap();
    assert_eq!(doubles.game_type, GameType::Doubles);
    assert_eq!(doubles.picked_team_size, Some(4));
    assert_eq!(doubles.adjust_level, Some(50));
    let singles = DEX.format("gen9championsbssregmb").unwrap();
    assert_eq!(singles.dex_mod, Mod::RegMb);
    assert_eq!(singles.picked_team_size, Some(3));
    assert_ne!(
        DEX.moves(Mod::RegMb, "wish").unwrap().pp,
        DEX.moves(Mod::Champions, "wish").unwrap().pp,
    );
}

#[test]
fn native_choice_syntax_keeps_joint_choices_and_preview_slots_separate() {
    let Choice::Team(team) = "team 631245".parse().unwrap() else {
        panic!()
    };
    assert_eq!(team.resolve(6, 3).unwrap(), [5, 2, 0]);
    let Choice::Team(team) = "team 10".parse().unwrap() else {
        panic!()
    };
    assert_eq!(team.resolve(10, 4).unwrap(), [9, 0, 1, 2]);
    let Choice::Team(team) = "team [6,3,1,2]".parse().unwrap() else {
        panic!()
    };
    assert!(team.resolve(6, 3).is_err());
    let Choice::Actions(slots) = "move Conversion 2, move 2 mega -1".parse().unwrap() else {
        panic!()
    };
    assert_eq!(
        slots,
        [
            SlotChoice::Move {
                selection: MoveChoice::Name("conversion2".into()),
                target: None,
                special: None,
            },
            SlotChoice::Move {
                selection: MoveChoice::Slot(2.try_into().unwrap()),
                target: Some(-1),
                special: Some(SpecialChoice::Mega),
            },
        ]
    );
    assert!("move 2 1 -1".parse::<Choice>().is_err());
    assert!(TargetKind::Normal.valid_location(2, 0, -2));
    assert!(!TargetKind::Normal.valid_location(2, 0, -1));
    assert!(TargetKind::AdjacentAllyOrSelf.valid_location(2, 0, -1));
    assert!(!TargetKind::AdjacentAllyOrSelf.valid_location(2, 0, 2));
}
