use pokemon_battle_simulator::{dex::DEX, generation::generate_team, input::Set, rng::Rng};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    revision: String,
    format: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    seed: String,
    after: String,
    sets: Vec<Set>,
}

fn verify(raw: &str) {
    let data: Fixture = serde_json::from_str(raw).unwrap();
    assert_eq!(data.revision, DEX.revision);
    let mut failures = Vec::new();
    for Case { seed, after, sets } in data.cases {
        let mut rng = Rng::new(seed.parse().unwrap());
        let generated = generate_team(&data.format, &mut rng).unwrap();
        assert_eq!(generated.len(), sets.len(), "{seed}");
        for (i, (actual, expected)) in generated.iter().zip(&sets).enumerate() {
            if actual != expected {
                failures.push(format!(
                    "{seed} slot {}\nexpected {expected:?}\ngot      {actual:?}",
                    i + 1
                ));
                break;
            }
        }
        if rng.seed() != after.parse().unwrap() {
            failures.push(format!(
                "{seed} final seed expected {after}, got {}",
                rng.seed()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn ranch_generator_matches_team_slots_and_rng_across_seed_families() {
    verify(include_str!("fixtures/ranch-generated.json"));
}

#[test]
#[ignore = "generate a larger corpus with node scripts/capture-generate-teams.mjs .tools/ranch-stress.json 1000"]
fn ranch_generator_matches_the_large_corpus() {
    verify(&std::fs::read_to_string(".tools/ranch-stress.json").unwrap());
}
