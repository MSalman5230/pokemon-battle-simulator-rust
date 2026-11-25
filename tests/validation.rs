use pokemon_battle_simulator::{
    dex::{DEX, Mod},
    validation::{LEGALITY, MoveSources, TeamValidator},
};
use serde_json::{Value, json};

#[test]
fn champions_legality_uses_the_pinned_learnsets_and_regulation_table() {
    assert_eq!(LEGALITY.revision, DEX.revision);
    let bss = LEGALITY.format("gen9championsbssregmc").unwrap();
    assert_eq!(bss.ev_limit, Some(66));
    assert_eq!(bss.item_limit, 1);
    assert_eq!(bss.rule_sources.get("-tag:mythical").unwrap(), "Flat Rules");
    assert_eq!(
        LEGALITY.species(Mod::Champions, "venusaur").unwrap().tier,
        "RU"
    );
    assert_eq!(LEGALITY.species(Mod::RegMb, "venusaur").unwrap().tier, "UU");
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur", "Energy Ball", 50),
        MoveSources::Unrestricted
    );
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur-Mega", "Energy Ball", 50),
        MoveSources::Unrestricted
    );
    assert_eq!(
        LEGALITY.move_sources_by_name(Mod::Champions, "Venusaur", "Dragon Ascent", 50),
        MoveSources::Absent
    );
}

#[test]
fn ordinary_competitive_sets_and_teams_match_pinned_validation() {
    let data: Value = serde_json::from_str(include_str!("fixtures/validation.json")).unwrap();
    assert_eq!(data["revision"], DEX.revision);
    let mut differences = Vec::new();
    for case in data["fixtures"].as_array().unwrap() {
        let validator = TeamValidator::new(case["format"].as_str().unwrap()).unwrap();
        let actual = if let Some(team) = case["team"].as_array() {
            validator.validate_team(team)
        } else {
            validator.validate_set(&case["set"])
        };
        match actual {
            Ok(value) if json!(value) == case["expected"] => {}
            Ok(value) => differences.push(format!(
                "{} {}: expected {}, got {}",
                case["format"],
                case["name"],
                case["expected"],
                json!(value)
            )),
            Err(err) => differences.push(format!("{} {}: {err}", case["format"], case["name"])),
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
#[ignore = "requires the locally generated reference-verified training manifest"]
fn bss_admits_the_reference_validated_training_rosters() {
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(".tools/verified-training-corpus.json").unwrap())
            .unwrap();
    assert_eq!(manifest["revision"], DEX.revision);
    let validator = TeamValidator::new(manifest["format"].as_str().unwrap()).unwrap();
    let mut errors = Vec::new();
    for (i, row) in manifest["teams"].as_array().unwrap().iter().enumerate() {
        match validator.validate_team(row["team"].as_array().unwrap()) {
            Ok(None) => {}
            Ok(Some(problems)) => errors.push(format!("team {i}: {}", problems.join("; "))),
            Err(e) => errors.push(format!("team {i}: {e}")),
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
