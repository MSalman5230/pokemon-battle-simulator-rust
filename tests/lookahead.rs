use pokemon_battle_simulator::{adapter::lookahead, dex::DEX};
use serde_json::{Value, json};

fn difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(i, (a, b))| difference(a, b, &format!("{at}/{i}"))),
        (Value::Object(a), Value::Object(b)) => a
            .iter()
            .find_map(|(key, a)| {
                b.get(key).map_or_else(
                    || Some(format!("{at}/{key}: extra field {a}")),
                    |b| difference(a, b, &format!("{at}/{key}")),
                )
            })
            .or_else(|| {
                b.keys()
                    .find(|key| !a.contains_key(*key))
                    .map(|key| format!("{at}/{key}: missing field"))
            }),
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => None,
        _ if a == b => None,
        _ => Some(format!("{at}: expected {b}, got {a}")),
    }
}

#[test]
fn paired_public_worlds_match_pinned_lookahead() {
    let data: Value = serde_json::from_str(include_str!("fixtures/lookahead.json")).unwrap();
    assert_eq!(data["revision"], DEX.revision);
    for case in data["cases"].as_array().unwrap() {
        let result = json!(lookahead::evaluate(&case["input"]).unwrap());
        let diff = difference(&result, &case["expected"], "");
        assert!(
            diff.is_none(),
            "{}: {}",
            case["name"],
            diff.unwrap_or_default()
        );
    }
}

#[test]
fn search_rejects_privileged_opponent_roster_fields() {
    let q = json!({"observation":{"opponent":[{"selected":false}]},"worlds":[],"candidates":[],"replies":0,"reply_probabilities":{}});
    assert!(
        matches!(lookahead::evaluate(&q), Err(err) if err == "Search requires a public opponent view")
    );
}
