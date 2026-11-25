use pokemon_battle_simulator::{
    battle::{BattleStream, Phase},
    input::Player,
};
use serde_json::{Value, json};

fn replay(raw: &str) {
    let fixture: Value = serde_json::from_str(raw).unwrap();
    let format = fixture["formatid"].as_str().unwrap();
    let mut stream = BattleStream::default();
    let mut log_cursor = 0;
    for (index, checkpoint) in fixture["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let context = format!("{format} checkpoint {index}");
        for line in checkpoint["lines"].as_array().unwrap() {
            let line = line.as_str().unwrap();
            stream
                .apply(line.parse().unwrap())
                .unwrap_or_else(|err| panic!("{context}, {line}: {err}"));
        }
        let battle = stream.battle().unwrap();
        assert_eq!(
            battle.turn(),
            checkpoint["turn"].as_u64().unwrap() as u32,
            "{context}"
        );
        let phase = match battle.phase() {
            Phase::Preview => "teampreview",
            Phase::Move => "move",
            Phase::Replace => "switch",
            Phase::Ended | Phase::Setup => "",
        };
        assert_eq!(json!(phase), checkpoint["requestState"], "{context}");
        for (channel, player) in [None, Some(Player::P1), Some(Player::P2)]
            .into_iter()
            .enumerate()
        {
            let log: Vec<_> = battle.log()[log_cursor..]
                .iter()
                .map(|line| line.view(player))
                .collect();
            assert_eq!(
                json!(log),
                checkpoint["channels"][channel],
                "{context}, channel {channel}"
            );
        }
        assert_eq!(
            battle.seed().to_string(),
            checkpoint["seed"].as_str().unwrap(),
            "{context}"
        );
        log_cursor = battle.log().len();
        for player in Player::ALL {
            let index = player.index();
            assert_eq!(
                battle.request(player).unwrap_or(Value::Null),
                checkpoint["requests"][index],
                "{context}, request for {}",
                player.id()
            );
            let party: Vec<_> = battle.party(player).unwrap().iter().enumerate().map(|(i, mon)| {
                let mut stats = serde_json::to_value(mon.stats).unwrap();
                stats.as_object_mut().unwrap().remove("hp");
                json!({
                    "name": mon.name, "species": mon.species.name, "details": mon.details, "hp": mon.hp,
                    "maxhp": mon.max_hp, "stats": stats, "ability": mon.ability, "item": mon.item,
                    "active":battle.is_active(player, i),
                    "moves": mon.moves.iter().map(|m| json!({"id":m.id,"pp":m.pp,"maxpp":m.max_pp})).collect::<Vec<_>>(),
                })
            }).collect();
            assert_eq!(
                json!(party),
                checkpoint["teams"][index],
                "{context}, party for {}",
                player.id()
            );
        }
    }
}

#[test]
fn bss_singles_native_replay() {
    replay(include_str!("fixtures/bssregmc.json"));
}

#[test]
fn vgc_doubles_native_replay() {
    replay(include_str!("fixtures/vgc2026regmc.json"));
}

#[test]
fn vgc_doubles_replay_with_two_faints_and_one_replacement() {
    replay(include_str!("fixtures/vgc-replacements.json"));
}
