use pokemon_battle_simulator::{
    battle::{BattleError, BattleStream, Phase},
    input::Player,
    mechanics::BoostStat,
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
            let result = stream.apply(line.parse().unwrap());
            if checkpoint["unavailable"]
                .as_array()
                .is_some_and(|entries| entries.iter().any(|entry| entry == line))
            {
                assert!(
                    matches!(result, Err(BattleError::Unavailable(_))),
                    "{context}: {result:?}"
                );
            } else {
                result.unwrap_or_else(|err| panic!("{context}, {line}: {err}"));
            }
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
                json!(battle.waiting(player)),
                checkpoint["waiting"][index],
                "{context}, waiting for {}",
                player.id()
            );
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
                    "name": mon.name, "species": mon.species.name, "details": mon.details, "hp": mon.hp, "status": mon.status.id(),
                    "maxhp": mon.max_hp, "stats": stats, "ability": mon.ability, "item": mon.item,
                    "boosts": ([BoostStat::Atk, BoostStat::Def, BoostStat::Spa, BoostStat::Spd, BoostStat::Spe,
                        BoostStat::Accuracy, BoostStat::Evasion].into_iter()
                        .map(|s| (s.id().to_owned(), json!(mon.boosts.get(s)))).collect::<serde_json::Map<_,_>>()),
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

#[test]
fn bss_replay_with_legal_rosters_and_stat_changes() {
    replay(include_str!("fixtures/bss-competitive.json"));
}

#[test]
fn bss_replay_with_paralysis_burn_and_toxic_reset() {
    replay(include_str!("fixtures/bss-status.json"));
}

#[test]
fn bss_replay_with_champions_sleep_and_freeze_timers() {
    replay(include_str!("fixtures/bss-sleep-freeze.json"));
}

#[test]
fn bss_replay_with_held_items_and_ability_interactions() {
    replay(include_str!("fixtures/bss-items.json"));
}

#[test]
fn vgc_replay_with_weather_setters_and_dynamic_speed() {
    replay(include_str!("fixtures/vgc-weather.json"));
}

#[test]
fn bss_replay_with_ordered_entry_hazards_and_poison_absorption() {
    replay(include_str!("fixtures/bss-hazards.json"));
}

#[test]
fn vgc_replay_resumes_turn_after_two_pivots() {
    replay(include_str!("fixtures/vgc-pivots.json"));
}

#[test]
fn vgc_replay_with_simultaneous_megas_and_permanent_formes() {
    replay(include_str!("fixtures/vgc-megas.json"));
}

#[test]
fn bss_replay_with_champions_mega_z_and_levitate() {
    replay(include_str!("fixtures/bss-mega-z.json"));
}

#[test]
fn bss_packed_teams_decode_ability_slots() {
    replay(include_str!("fixtures/bss-packed.json"));
}

#[test]
fn bss_reg_m_b_preview_and_pp_overrides() {
    replay(include_str!("fixtures/bss-reg-mb.json"));
}

#[test]
fn bss_hidden_trap_is_discovered_without_revealing_the_other_choice() {
    replay(include_str!("fixtures/bss-hidden-trap.json"));
}

#[test]
fn bss_imprison_discovery_only_updates_the_choosing_owner() {
    replay(include_str!("fixtures/bss-imprison.json"));
}

#[test]
fn bss_no_pp_forces_typeless_struggle_and_recoil() {
    replay(include_str!("fixtures/bss-struggle.json"));
}

#[test]
fn bss_choice_lock_intimidate_speed_boost_and_knock_off() {
    replay(include_str!("fixtures/bss-choice-items.json"));
}

#[test]
fn bss_roost_body_press_mirror_armor_and_recoil() {
    replay(include_str!("fixtures/bss-bulky-effects.json"));
}
