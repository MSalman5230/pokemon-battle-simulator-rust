use pokemon_battle_simulator::{
    battle::Phase,
    dex::DEX,
    input::{Player, PlayerOptions, Set, Team},
    rng::Seed,
    series::{Series, SeriesState},
};
use serde_json::Value;

const SEEDS: [Seed; 3] = [
    Seed::Gen5([1, 2, 3, 4]),
    Seed::Gen5([2, 2, 3, 4]),
    Seed::Gen5([3, 2, 3, 4]),
];

fn players() -> [PlayerOptions; 2] {
    Player::ALL.map(|p| PlayerOptions {
        name: format!("player{}", p.index()),
        team: Some(Team::Sets(
            ["Snorlax", "Clefable"]
                .into_iter()
                .map(|species| Set {
                    species: species.into(),
                    moves: vec!["Tackle".into()],
                    gender: "M".into(),
                    ..Set::default()
                })
                .collect(),
        )),
        ..PlayerOptions::default()
    })
}

#[test]
fn all_three_game_result_sequences_match_showdown_series_scoring() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/best-of-three.json")).unwrap();
    assert_eq!(fixture["revision"], DEX.revision);
    for format in [
        "gen9championsvgc2026regmcbo3",
        "gen9championsvgc2026regmbbo3",
    ] {
        for sequence in fixture["series"].as_array().unwrap() {
            let mut series = Series::new(format, players(), SEEDS).unwrap();
            for (i, row) in sequence.as_array().unwrap().iter().enumerate() {
                assert_eq!(series.state(), SeriesState::Playing);
                assert_eq!(series.game_number(), i + 1);
                assert_eq!(series.game().phase(), Phase::Preview);
                assert_eq!(series.game().seed(), SEEDS[i]);
                assert!(
                    series
                        .game()
                        .log()
                        .iter()
                        .any(|line| line.view(None).starts_with("|showteam|p1|"))
                );
                assert!(series.next_game().is_err());
                let control = row["result"]
                    .as_str()
                    .map_or_else(|| ">forcetie".into(), |p| format!(">forcewin {p}"));
                series.apply(control.parse().unwrap()).unwrap();
                assert_eq!(
                    series.score().map(Value::from).as_slice(),
                    row["wins"].as_array().unwrap()
                );
                assert_eq!(series.ties(), row["ties"].as_u64().unwrap() as u8);
                assert_eq!(
                    series.win_threshold(),
                    row["win_threshold"].as_u64().unwrap() as u8
                );
                let expected = if row["ended"] == true {
                    SeriesState::Ended {
                        winner: Player::ALL.into_iter().find(|p| row["winner"] == p.id()),
                    }
                } else {
                    SeriesState::BetweenGames
                };
                assert_eq!(series.state(), expected);
                if expected == SeriesState::BetweenGames {
                    series.next_game().unwrap();
                } else {
                    assert!(series.next_game().is_err());
                    assert!(series.apply(">forcetie".parse().unwrap()).is_err());
                }
            }
        }
    }
}

#[test]
fn a_new_series_game_resets_hp_and_pp_and_returns_to_preview() {
    let mut series = Series::new("gen9championsvgc2026regmcbo3", players(), SEEDS).unwrap();
    for line in [
        ">p1 team 12",
        ">p2 team 12",
        ">p1 move 1 1, move 1 1",
        ">p2 move 1 1, move 1 1",
    ] {
        series.apply(line.parse().unwrap()).unwrap();
    }
    for player in Player::ALL {
        let lead = &series.game().party(player).unwrap()[0];
        assert!(lead.hp < lead.max_hp.get() && lead.moves[0].pp < lead.moves[0].max_pp);
    }
    series.apply(">forcetie".parse().unwrap()).unwrap();
    series.next_game().unwrap();
    assert_eq!(series.game().phase(), Phase::Preview);
    assert_eq!(series.game().seed(), SEEDS[1]);
    for player in Player::ALL {
        let team = series.game().party(player).unwrap();
        assert!(
            team.iter()
                .all(|p| p.hp == p.max_hp.get() && p.moves[0].pp == p.moves[0].max_pp)
        );
    }
}
