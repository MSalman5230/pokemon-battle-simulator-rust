use pokemon_battle_simulator::{adapter::tactics, dex::DEX};
use serde_json::{Value, json};

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, a)| b.get(k).is_some_and(|b| equal(a, b)))
        }
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => a == b,
    }
}

#[test]
fn hypothetical_calculator_matches_native_damage_rows_and_public_state() {
    check_fixture(include_str!("fixtures/tactics.json"));
}

#[test]
#[ignore = "run node scripts/capture-tactics.mjs .tools/tactics-training.json after setup"]
fn all_training_moves_match_native_tactical_calculator() {
    check_fixture(&std::fs::read_to_string(".tools/tactics-training.json").unwrap());
}

fn check_fixture(source: &str) {
    let data: Value = serde_json::from_str(source).unwrap();
    assert_eq!(data["revision"], DEX.revision);
    let cases = data["cases"].as_array().unwrap();
    let queries = Value::Array(cases.iter().map(|case| case["input"].clone()).collect());
    let rows = tactics::calculate_many(&queries).unwrap();
    let mut problems = Vec::new();
    assert_eq!(rows.len(), cases.len());
    for (case, row) in cases.iter().zip(rows) {
        let row = json!(row);
        if !equal(&row, &case["expected"]) {
            problems.push(format!(
                "{}\nexpected {}\ngot      {}",
                case["name"], case["expected"], row,
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn hypothetical_queries_fail_explicitly_when_a_callback_is_not_ported() {
    let set = json!({"species":"Vaporeon","moves":["Protect"]});
    let query = json!({"move":"Beat Up","actor":{"set":set},"target":{"set":set}});
    assert!(
        tactics::calculate_many(&json!([query]))
            .unwrap_err()
            .contains("not ported")
    );
}
