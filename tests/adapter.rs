use pokemon_battle_simulator::{adapter::Game, rng::Seed};
use serde_json::{Value, json, to_value};

fn game(training: bool, max_turns: u32) -> Game {
    let corpus: Value =
        serde_json::from_str(include_str!("../benchmarks/verified-bss-corpus.json")).unwrap();
    let teams = [0, 1].map(|i| corpus["teams"][i]["team"].as_array().unwrap().clone());
    Game::new_prevalidated(teams, Seed::Gen5([1, 2, 3, 4]), max_turns, training).unwrap()
}

#[test]
fn preview_permutations_and_private_critic_contract() {
    let public_game = game(false, 5);
    let normal = to_value(public_game.result()).unwrap();
    assert!(normal.get("training").is_none());
    for view in normal["views"].as_array().unwrap() {
        assert_eq!(view["turn"], 0);
        assert_eq!(view["phase"], "preview");
        let legal = view["legal"].as_array().unwrap();
        assert_eq!(legal.len(), 120);
        assert_eq!(
            legal[0],
            json!({
                "choice": "team 1,2,3", "id": 14, "picks": [0, 1, 2], "kind": "preview",
            })
        );
        assert_eq!(
            legal[119],
            json!({
                "choice": "team 6,5,4", "id": 133, "picks": [5, 4, 3], "kind": "preview",
            })
        );
        assert!(
            view["opponent"].as_array().unwrap().iter().all(|m| {
                m["item"].is_null() && m["ability"].is_null() && m.get("set").is_none()
            })
        );
    }
    let training_game = game(true, 5);
    let training = to_value(training_game.result()).unwrap();
    assert_eq!(normal["views"], training["views"]);
    for i in 0..2 {
        assert_eq!(
            training["training"]["critic_views"][i]["opponent"],
            training["views"][1 - i]["own"]
        );
    }
}

#[test]
fn invalid_joint_choice_does_not_advance_and_turn_cap_keeps_native_phase() {
    let mut game = game(false, 0);
    let before = to_value(game.result()).unwrap();
    assert!(game.step([Some(14), Some(134)]).is_err());
    assert_eq!(to_value(game.result()).unwrap(), before);
    game.step([Some(14), Some(14)]).unwrap();
    let capped = to_value(game.result()).unwrap();
    assert_eq!(capped["truncated"], true);
    assert_eq!(capped["ended"], false);
    assert_eq!(capped["views"][0]["turn"], 1);
    assert_eq!(capped["views"][0]["phase"], "move");
    assert_eq!(capped["views"][0]["legal"], json!([]));
    assert!(capped["views"][0]["opponent"][0].get("nickname").is_none());
    assert!(
        capped["views"][0]["opponent"][0]
            .get("current_hp")
            .is_none()
    );
}

#[test]
fn revival_legal_actions_use_original_roster_slot_and_exclude_living_bench() {
    let mut game = Game::new_prevalidated(
        [
            vec![
                json!({"species":"Electrode","moves":["Explosion"]}),
                json!({"species":"Pawmot","moves":["Revival Blessing","Tackle"]}),
                json!({"species":"Vaporeon","moves":["Recover"]}),
                json!({"species":"Umbreon","moves":["Recover"]}),
                json!({"species":"Glaceon","moves":["Recover"]}),
                json!({"species":"Espeon","moves":["Recover"]}),
            ],
            [
                "Gengar", "Banette", "Sableye", "Vaporeon", "Scizor", "Umbreon",
            ]
            .into_iter()
            .map(|species| json!({"species":species,"moves":["Recover"]}))
            .collect(),
        ],
        Seed::Gen5([1, 2, 3, 4]),
        8,
        true,
    )
    .unwrap();
    for choices in [
        [Some(14), Some(14)],
        [Some(0), Some(0)],
        [Some(9), None],
        [Some(0), Some(0)],
    ] {
        game.step(choices).unwrap();
    }
    let snapshot = to_value(game.result()).unwrap();
    assert_eq!(snapshot["views"][0]["phase"], "switch");
    assert_eq!(
        snapshot["views"][0]["legal"],
        json!([
            {"id":8,"kind":"switch","slot":0,"party_slot":1,"revive":true,"species":"Electrode","choice":"switch 2"}
        ])
    );
    game.step([Some(8), None]).unwrap();
    let snapshot = to_value(game.result()).unwrap();
    assert_eq!(snapshot["views"][0]["phase"], "move");
    assert!(
        snapshot["views"][0]["legal"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == 8 && row["revive"] == false)
    );
}
