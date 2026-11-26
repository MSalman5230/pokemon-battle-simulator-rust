use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn replay<const N: usize>(args: &[&str], requests: [Value; N]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pokemon-battle-worker"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        for request in requests {
            writeln!(input, "{request}").unwrap();
        }
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn worker_validates_sets_and_admits_rosters_without_an_external_manifest() {
    let corpus: Value =
        serde_json::from_str(include_str!("../benchmarks/verified-bss-corpus.json")).unwrap();
    let mut teams = [0, 1].map(|i| corpus["teams"][i]["team"].clone());
    // A legal nickname makes this different from every exact roster in the corpus.
    teams[0][0]["name"] = json!("Lefty's buddy");
    let valid = &teams[0][0];
    let mut invalid_team = teams[0].clone();
    invalid_team[0]["moves"][0] = json!("no such move");
    let error = "\"nosuchmove\" is an invalid move.";
    let tactic: Value = serde_json::from_str(include_str!("fixtures/tactics.json")).unwrap();
    let requests = [
        json!({"id":1,"op":"batch","requests":[
            {"op":"validate","sets":[valid,invalid_team[0]]},
            {"op":"validate","teams":[teams[0],invalid_team]},
        ]}),
        json!({"id":2,"op":"new","game":"bad","teams":[invalid_team,teams[1]],"seed":[1,2,3,4]}),
        json!({"id":3,"op":"new","game":"good","teams":teams,"seed":[1,2,3,4]}),
        json!({"id":4,"op":"step","game":"good","actions":[14,14]}),
        json!({"id":5,"op":"tactics_metadata"}),
        json!({"id":6,"op":"calculate","queries":[tactic["cases"][0]["input"],tactic["cases"][2]["input"]]}),
    ];
    let responses = replay(&[], requests);
    assert_eq!(responses.len(), 6);
    assert_eq!(
        responses[0],
        json!({"id":1,"result":[[null,[error]],[null,[error]]]})
    );
    assert_eq!(responses[1], json!({"id":2,"error":error}));
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["views"][0]["phase"], "preview");
    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["result"]["views"][0]["turn"], 1);
    let data: Value = serde_json::from_str(include_str!("../data/tactics-metadata.json")).unwrap();
    assert_eq!(responses[4], json!({"id":5,"result":data["metadata"]}));
    assert_eq!(responses[5]["id"], 6);
    assert_eq!(
        responses[5]["result"][0]["rows"],
        tactic["cases"][0]["expected"]["rows"]
    );
    assert_eq!(responses[5]["result"][1]["rows"], json!([[0, 1.0]]));
}

#[test]
fn delta_worker_resends_full_observations_after_partial_batch_failure() {
    let corpus: Value =
        serde_json::from_str(include_str!("../benchmarks/verified-bss-corpus.json")).unwrap();
    let teams = [0, 1].map(|i| corpus["teams"][i]["team"].clone());
    let new_game = |id, training| {
        json!({"id":id,"op":"new","game":"slot","teams":teams,"seed":[1,2,3,4],
               "training":training,"max_turns":0})
    };
    let requests = [
        new_game(0, true),
        json!({"id":1,"op":"batch","requests":[
            {"op":"step","game":"slot","actions":[14,14]},
            {"op":"step","game":"missing","actions":[null,null]},
        ]}),
        json!({"id":2,"op":"step","game":"slot","actions":[null,null]}),
        json!({"id":3,"op":"batch","requests":[
            {"op":"step","game":"slot","actions":[null,null]},
            {"op":"step","game":"slot","actions":[null,null]},
            {"op":"close","game":"slot"},
        ]}),
        new_game(4, true),
        new_game(5, false),
    ];
    let responses = replay(&["--delta-observations"], requests);
    let marker = "$battle_delta_v1";
    assert_eq!(responses[0]["result"][marker][1], 0);
    assert_eq!(responses[1]["error"], "game does not exist");
    let recovered = &responses[2]["result"][marker];
    assert_eq!(recovered[1], 0);
    assert_eq!(recovered[2][0][0]["truncated"], true);
    assert_eq!(recovered[2][0][1].as_array().unwrap().len(), 12);
    assert!(recovered[2][0][1][1].get("set").is_none());
    assert_eq!(recovered[4][0][0]["species"], teams[0][0]["species"]);
    assert_eq!(responses[3]["result"][0][marker][1], 1);
    assert_eq!(
        responses[3]["result"][0][marker][2][0],
        json!([{}, [], [], null])
    );
    assert_eq!(responses[3]["result"][1][marker][1], 2);
    assert_eq!(responses[3]["result"][2]["$battle_closed"], "slot");
    assert_eq!(responses[4]["result"][marker][1], 0);
    assert!(responses[5]["result"]["views"].is_array());
    assert!(responses[5]["result"][marker].is_null());
}
