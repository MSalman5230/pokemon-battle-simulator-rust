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
    let data: Value = serde_json::from_str(include_str!("fixtures/tactics.json")).unwrap();
    assert_eq!(data["revision"], DEX.revision);
    let mut problems = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        match tactics::calculate_many(&json!([case["input"]])) {
            Ok(row) if equal(&json!(row[0]), &case["expected"]) => {}
            Ok(row) => problems.push(format!(
                "{}\nexpected {}\ngot      {}",
                case["name"],
                case["expected"],
                json!(row[0])
            )),
            Err(err) => problems.push(format!("{}: {err}", case["name"])),
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
