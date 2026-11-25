use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

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
    let requests = [
        json!({"id":1,"op":"batch","requests":[
            {"op":"validate","sets":[valid,invalid_team[0]]},
            {"op":"validate","teams":[teams[0],invalid_team]},
        ]}),
        json!({"id":2,"op":"new","game":"bad","teams":[invalid_team,teams[1]],"seed":[1,2,3,4]}),
        json!({"id":3,"op":"new","game":"good","teams":teams,"seed":[1,2,3,4]}),
        json!({"id":4,"op":"step","game":"good","actions":[14,14]}),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_pokemon-battle-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut input = child.stdin.take().unwrap();
        for request in requests {
            writeln!(input, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let responses: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 4);
    assert_eq!(
        responses[0],
        json!({"id":1,"result":[[null,[error]],[null,[error]]]})
    );
    assert_eq!(responses[1], json!({"id":2,"error":error}));
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["views"][0]["phase"], "preview");
    assert_eq!(responses[3]["id"], 4);
    assert_eq!(responses[3]["result"]["views"][0]["turn"], 1);
}
