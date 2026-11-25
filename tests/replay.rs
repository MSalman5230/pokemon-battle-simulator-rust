use pokemon_battle_simulator::{
    battle::{BattleError, BattleStream, Phase},
    input::Player,
    mechanics::BoostStat,
};
use serde_json::{Value, json};

fn replay(raw: &str) {
    let fixture: Value = serde_json::from_str(raw).unwrap();
    replay_fixture(&fixture);
}

fn replay_fixture(fixture: &Value) {
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
            if let Some(expected) = checkpoint.get("observedDisables") {
                let disables = battle
                    .party(player)
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        (0..p.moves.len())
                            .map(|m| battle.actual_move_disabled(player, i, m))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    json!(disables),
                    expected[index],
                    "{context}, observed disables for {}",
                    player.id()
                );
            }
        }
    }
}

#[test]
#[ignore = "generate a corpus with node scripts/capture-wide-native.mjs .tools/wide-native.jsonl"]
fn wide_native_battles_match_native_checkpoints() {
    use std::{
        collections::BTreeMap,
        fs::File,
        io::{BufRead, BufReader},
        panic,
    };
    let path = std::env::var("CHAMPIONS_NATIVE_CORPUS")
        .unwrap_or_else(|_| ".tools/wide-native.jsonl".into());
    let cases = BufReader::new(File::open(path).unwrap()).lines();
    let mut totals = BTreeMap::<String, (usize, usize)>::new();
    for raw in cases {
        let raw = raw.unwrap();
        let header: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            header["revision"],
            pokemon_battle_simulator::dex::DEX.revision
        );
        let format = header["formatid"].as_str().unwrap();
        let counts = totals.entry(format.to_owned()).or_default();
        counts.1 += 1;
        if panic::catch_unwind(|| replay_fixture(&header)).is_ok() {
            counts.0 += 1;
        } else {
            eprintln!("first mismatch for {format}, seed {}", header["seed"]);
        }
    }
    assert!(!totals.is_empty(), "empty replay corpus");
    eprintln!("Exact native replays by format (passed, total) {totals:?}");
    assert!(totals.values().all(|(passed, total)| passed == total));
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
fn active_sunlight_blocks_the_freeze_secondary_without_changing_the_roll() {
    replay(include_str!("fixtures/bss-sun-blocks-freeze.json"));
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

#[test]
fn bss_weather_screens_yawn_rest_and_water_absorb() {
    replay(include_str!("fixtures/bss-screen-rest.json"));
}

#[test]
fn bss_cute_charm_probability_and_end_on_source_switch() {
    replay(include_str!("fixtures/bss-cute-charm.json"));
}

#[test]
fn bss_cursed_body_disables_and_expires() {
    replay(include_str!("fixtures/bss-cursed-body.json"));
}

#[test]
fn bss_type_conversion_crit_guarantees_and_no_guard() {
    replay(include_str!("fixtures/bss-converted-types.json"));
}

#[test]
fn champions_custom_game_reports_exact_hp_in_every_view() {
    replay(include_str!("fixtures/doublescustomgame.json"));
}

#[test]
fn champions_ou_preview_selects_the_full_team() {
    replay(include_str!("fixtures/ou.json"));
}

#[test]
fn vgc_elemental_redirection_prefers_faster_holder_and_never_redirects_spread() {
    replay(include_str!("fixtures/vgc-elemental-redirection.json"));
}

#[test]
fn bss_magic_guard_blocks_indirect_damage_without_preventing_status() {
    replay(include_str!("fixtures/bss-magic-guard.json"));
}

#[test]
fn bss_last_resort_unlocks_after_using_other_moves_and_resets_on_switch_in() {
    replay(include_str!("fixtures/bss-last-resort.json"));
}

#[test]
fn bss_armor_items_secondary_self_boost_and_shed_skin() {
    replay(include_str!("fixtures/bss-damage-hooks.json"));
}

#[test]
fn bss_recharge_locks_the_request_only_when_the_hit_succeeds() {
    replay(include_str!("fixtures/bss-recharge.json"));
}

#[test]
fn vgc_recharge_keeps_the_ally_choice_and_rng_order() {
    replay(include_str!("fixtures/vgc-recharge.json"));
}

#[test]
fn bss_weather_attacks_modify_type_power_and_accuracy_and_replay_confusion() {
    replay(include_str!("fixtures/bss-weather-attacks.json"));
}

#[test]
fn bss_rampage_requests_spend_pp_only_on_start_and_end_with_fatigue() {
    replay(include_str!("fixtures/bss-rampage.json"));
}

#[test]
fn vgc_rampage_replays_random_targeting_across_forced_moves() {
    replay(include_str!("fixtures/vgc-rampage.json"));
}

#[test]
fn bss_taunt_interrupts_selected_status_moves_and_updates_requests() {
    replay(include_str!("fixtures/bss-taunt.json"));
}

#[test]
fn bss_court_change_glaive_rush_and_poltergeist() {
    replay(include_str!("fixtures/bss-court-change.json"));
}

#[test]
fn bss_temporary_ability_changes_preserve_base_ability_and_reset_on_switch() {
    replay(include_str!("fixtures/bss-ability-transfer.json"));
}

#[test]
fn bss_hp_sharing_attack_based_healing_and_sunny_growth() {
    replay(include_str!("fixtures/bss-hp-effects.json"));
}

#[test]
fn bss_multihit_rechecks_damage_and_abilities_between_strikes() {
    replay(include_str!("fixtures/bss-multihit.json"));
}

#[test]
fn vgc_multihit_counts_only_the_strikes_that_execute_after_focus_sash() {
    replay(include_str!("fixtures/vgc-multihit-sash.json"));
}

#[test]
fn vgc_trick_room_reverses_speed_without_reversing_priority_and_can_end_early() {
    replay(include_str!("fixtures/vgc-trick-room.json"));
}

#[test]
fn vgc_grassy_terrain_modifies_grounded_power_priority_and_healing() {
    replay(include_str!("fixtures/vgc-grassy-terrain.json"));
}

#[test]
fn bss_terrain_protects_grounded_pokemon_from_status_and_priority() {
    replay(include_str!("fixtures/bss-terrain-protection.json"));
}

#[test]
fn vgc_encore_replaces_and_retargets_a_queued_action() {
    replay(include_str!("fixtures/vgc-encore.json"));
}

#[test]
fn bss_mental_herb_cures_encore_before_the_selected_move_executes() {
    replay(include_str!("fixtures/bss-mental-herb.json"));
}

#[test]
fn bss_side_target_moves_still_execute_after_the_last_active_foe_faints() {
    replay(include_str!("fixtures/bss-hazard-after-faint.json"));
}

#[test]
fn bss_tied_disable_handlers_consume_the_same_rng_as_showdown() {
    replay(include_str!("fixtures/bss-tied-disables.json"));
}

#[test]
fn bss_tidy_up_stored_power_and_crash_damage() {
    replay(include_str!("fixtures/bss-stacking-moves.json"));
}

#[test]
fn bss_own_tempo_prevents_confusion_and_intimidate() {
    replay(include_str!("fixtures/bss-own-tempo.json"));
}

#[test]
fn bss_phazing_samples_a_bench_member_without_a_replacement_request() {
    replay(include_str!("fixtures/bss-phazing.json"));
}

#[test]
fn vgc_phazing_cancels_the_removed_pokemons_queued_move() {
    replay(include_str!("fixtures/vgc-phazing.json"));
}

#[test]
fn bss_retaliation_uses_damage_from_this_turn_and_follows_the_attackers_slot() {
    replay(include_str!("fixtures/bss-retaliation.json"));
}

#[test]
fn vgc_follow_me_preempts_ability_redirection_and_powder_respects_typing() {
    replay(include_str!("fixtures/vgc-attention.json"));
}

#[test]
fn bss_conditional_secondaries_follow_same_turn_boosts_and_sample_status() {
    replay(include_str!("fixtures/bss-conditional-secondary.json"));
}

#[test]
fn bss_leech_seed_heals_the_replacement_in_the_seeders_slot() {
    replay(include_str!("fixtures/bss-leech-seed.json"));
}

#[test]
fn bss_magician_steals_the_held_item_before_residuals() {
    replay(include_str!("fixtures/bss-magician.json"));
}

#[test]
fn vgc_symbiosis_transfers_after_the_consumed_item_resolves() {
    replay(include_str!("fixtures/vgc-symbiosis.json"));
}

#[test]
fn bss_gluttony_activates_a_pinch_berry_at_half_hp() {
    replay(include_str!("fixtures/bss-gluttony.json"));
}

#[test]
fn bss_ohko_bypasses_regular_damage_and_respects_sturdy_and_type_immunity() {
    replay(include_str!("fixtures/bss-ohko.json"));
}

#[test]
fn bss_destiny_bond_disallows_repetition_and_resolves_the_faint_chain() {
    replay(include_str!("fixtures/bss-destiny-bond.json"));
}

#[test]
fn bss_trick_swaps_held_items_and_foul_play_uses_the_targets_attack_stat() {
    replay(include_str!("fixtures/bss-trick-foul-play.json"));
    replay(include_str!("fixtures/bss-trick-same-choice-item.json"));
}

#[test]
fn bss_wandering_spirit_and_skill_swap_preserve_base_abilities() {
    replay(include_str!("fixtures/bss-wandering-spirit.json"));
}

#[test]
fn bss_charge_moves_handle_weather_power_herb_and_pp_exactly() {
    replay(include_str!("fixtures/bss-charge.json"));
}

#[test]
fn vgc_charge_preserves_the_selected_target_slot() {
    replay(include_str!("fixtures/vgc-charge.json"));
}

#[test]
fn vgc_expanding_force_becomes_a_spread_move_on_psychic_terrain() {
    replay(include_str!("fixtures/vgc-expanding-force.json"));
}

#[test]
fn bss_stakeout_applies_only_on_the_targets_switch_in_turn() {
    replay(include_str!("fixtures/bss-stakeout.json"));
}

#[test]
fn bss_dive_blocks_ordinary_attacks_but_allows_surf_and_no_guard() {
    replay(include_str!("fixtures/bss-dive.json"));
}

#[test]
fn bss_resist_berries_check_type_effectiveness_and_unnerve() {
    replay(include_str!("fixtures/bss-resist-berries.json"));
}

#[test]
fn vgc_flower_veil_protects_grass_allies_from_status_and_stat_drops() {
    replay(include_str!("fixtures/vgc-flower-veil.json"));
}

#[test]
fn bss_parting_shot_switches_after_a_stat_drop_or_mirror_armor() {
    replay(include_str!("fixtures/bss-parting-shot.json"));
}

#[test]
fn bss_mold_breaker_ignores_levitate_but_not_rough_skin_or_ability_shield() {
    replay(include_str!("fixtures/bss-mold-breaker.json"));
}

#[test]
fn vgc_mold_breaker_only_suppresses_defenses_during_the_holders_move() {
    replay(include_str!("fixtures/vgc-mold-breaker.json"));
}

#[test]
fn bss_illusion_hides_public_identity_until_direct_damage() {
    replay(include_str!("fixtures/bss-illusion.json"));
}

#[test]
fn vgc_illusion_can_use_the_active_ally_to_its_right() {
    replay(include_str!("fixtures/vgc-illusion.json"));
}

#[test]
fn bss_white_herb_clears_negative_stages_after_move_and_entry_effects() {
    replay(include_str!("fixtures/bss-white-herb.json"));
}

#[test]
fn bss_sucker_punch_and_thunderclap_require_a_pending_damaging_move() {
    replay(include_str!("fixtures/bss-conditional-priority.json"));
}

#[test]
fn bss_weight_moves_use_effective_weight_and_attacker_to_defender_ratio() {
    replay(include_str!("fixtures/bss-weight-moves.json"));
}

#[test]
fn bss_scale_shot_boosts_once_after_a_successful_hit_sequence() {
    replay(include_str!("fixtures/bss-scale-shot.json"));
}

#[test]
fn bss_endeavor_checks_current_hp_and_natural_type_immunity() {
    replay(include_str!("fixtures/bss-endeavor.json"));
}

#[test]
fn bss_cloud_nine_suppresses_effects_without_removing_the_weather() {
    replay(include_str!("fixtures/bss-cloud-nine.json"));
}

#[test]
fn vgc_eruption_and_water_spout_use_current_hp_after_priority_damage() {
    replay(include_str!("fixtures/vgc-hp-power.json"));
}

#[test]
fn bss_stance_change_switches_form_before_attack_and_kings_shield() {
    replay(include_str!("fixtures/bss-stance-change.json"));
}

#[test]
fn bss_magic_bounce_reflects_status_and_hazards_only_once_without_pp() {
    replay(include_str!("fixtures/bss-magic-bounce.json"));
}

#[test]
fn vgc_magic_bounce_reflects_hazards_aimed_at_the_holders_side() {
    replay(include_str!("fixtures/vgc-magic-bounce.json"));
}

#[test]
fn bss_magic_bounce_pivot_cancels_the_reflectors_original_action() {
    replay(include_str!("fixtures/bss-magic-bounce-pivot.json"));
}

#[test]
fn bss_dig_is_semi_invulnerable_but_takes_double_earthquake_damage() {
    replay(include_str!("fixtures/bss-dig.json"));
}

#[test]
fn draft_sleep_clause_counts_benched_foe_inflicted_sleep_but_not_rest() {
    replay(include_str!("fixtures/draft-sleep-clause.json"));
}

#[test]
fn draft_misty_terrain_blocks_sleep_before_the_sleep_clause() {
    replay(include_str!("fixtures/draft-sleep-terrain.json"));
}

#[test]
fn vgc_open_sheets_show_no_private_stats_and_only_before_turn_one() {
    replay(include_str!("fixtures/vgc-open-sheets.json"));
}

#[test]
fn vgc_bo3_battle_emits_forced_open_sheets_in_preview() {
    replay(include_str!("fixtures/vgc-forced-open-sheets.json"));
}

#[test]
fn bss_wish_heals_the_incoming_slot_from_the_original_wishers_hp() {
    replay(include_str!("fixtures/bss-wish.json"));
}

#[test]
fn vgc_tailwind_reorders_pending_moves_and_court_change_transfers_it() {
    replay(include_str!("fixtures/vgc-tailwind.json"));
}

#[test]
fn bss_last_respects_counts_faints_and_rage_fist_counts_hits_this_stint() {
    replay(include_str!("fixtures/bss-hit-count-power.json"));
}

#[test]
fn bss_regenerator_and_natural_cure_work_on_pivots_drags_and_voluntary_switches() {
    replay(include_str!("fixtures/bss-switch-abilities.json"));
}

#[test]
fn vgc_static_and_poison_point_roll_only_on_contact_and_reveal_the_holder() {
    replay(include_str!("fixtures/vgc-contact-status.json"));
}

#[test]
fn ranch_generates_independent_rosters_then_plays_native_pick_four_doubles() {
    replay(include_str!("fixtures/ranch-start.json"));
}

#[test]
fn vgc_flash_fire_and_dry_skin_handle_elemental_hits_and_frisk_reveals_both_items() {
    replay(include_str!("fixtures/vgc-flash-dry-frisk.json"));
}

#[test]
fn bss_keen_eye_protects_accuracy_and_reckless_boosts_damage_before_recoil() {
    replay(include_str!("fixtures/bss-keen-reckless.json"));
}

#[test]
fn bss_purifying_salt_blocks_status_and_salt_cure_uses_champions_residual_damage() {
    replay(include_str!("fixtures/bss-purifying-salt.json"));
}

#[test]
fn bss_disguise_survives_mold_breaker_then_busts_permanently_on_an_ordinary_hit() {
    replay(include_str!("fixtures/bss-disguise.json"));
}

#[test]
fn bss_multihit_breaks_disguise_before_resolving_the_remaining_strikes() {
    replay(include_str!("fixtures/bss-disguise-multihit.json"));
}

#[test]
fn bss_rapid_spin_requires_a_hit_and_clears_leech_seed_and_entry_hazards() {
    replay(include_str!("fixtures/bss-rapid-spin.json"));
}

#[test]
fn bss_status_phazing_respects_suction_cups_and_roar_respects_soundproof() {
    replay(include_str!("fixtures/bss-status-drag.json"));
}

#[test]
fn vgc_damp_blocks_explosion_without_sacrificing_the_user() {
    replay(include_str!("fixtures/vgc-damp.json"));
}

#[test]
fn bss_aftermath_damages_the_contact_attacker_before_faint_resolution() {
    replay(include_str!("fixtures/bss-aftermath.json"));
}

#[test]
fn vgc_damp_on_the_attackers_partner_prevents_aftermath() {
    replay(include_str!("fixtures/vgc-damp-aftermath.json"));
}

#[test]
fn bss_rest_completes_healing_before_chesto_berry_is_consumed() {
    replay(include_str!("fixtures/bss-chesto-rest.json"));
}

#[test]
fn champions_random_formats_with_supplied_teams_skip_preview_and_mask_illusion_level() {
    replay(include_str!("fixtures/random-native-supplied.json"));
    replay(include_str!("fixtures/random-doubles-native-supplied.json"));
}

#[test]
fn champions_random_formats_generate_and_play_from_native_seeded_inputs() {
    replay(include_str!("fixtures/random-native-generated.json"));
    replay(include_str!(
        "fixtures/random-doubles-native-generated.json"
    ));
}

#[test]
fn natdex_draft_uses_champions_battle_rules_and_sleep_clause() {
    replay(include_str!("fixtures/natdex-draft-native.json"));
}

#[test]
fn bss_soak_changes_typing_until_switch_and_can_be_absorbed() {
    replay(include_str!("fixtures/bss-soak-type.json"));
}

#[test]
fn bss_raging_bull_uses_tauros_forme_and_shatters_screens_before_damage() {
    replay(include_str!("fixtures/bss-raging-bull-furcoat.json"));
}

#[test]
fn bss_corrosion_poisons_steel_and_defiant_reacts_to_each_foe_stat_drop() {
    replay(include_str!("fixtures/bss-corrosion-defiant.json"));
}

#[test]
fn bss_contrary_inverts_self_and_foe_stat_changes_before_caps() {
    replay(include_str!("fixtures/bss-contrary.json"));
}

#[test]
fn bss_crit_ratio_sources_stack_and_respect_shell_armor() {
    replay(include_str!("fixtures/bss-crit-ratio.json"));
}

#[test]
fn bss_merciless_guarantees_criticals_against_poisoned_targets() {
    replay(include_str!("fixtures/bss-merciless.json"));
}

#[test]
fn focus_punch_prepares_in_speed_order_and_loses_no_pp_when_interrupted() {
    replay(include_str!("fixtures/bss-focus-punch.json"));
    replay(include_str!("fixtures/vgc-focus-punch.json"));
}

#[test]
fn revival_blessing_selects_a_fainted_teammate_without_switching_the_user() {
    replay(include_str!("fixtures/bss-revive-bench.json"));
    replay(include_str!("fixtures/bss-revive-fails.json"));
    replay(include_str!("fixtures/vgc-revive-active.json"));
}

#[test]
fn chilly_reception_announces_before_flinch_and_works_with_existing_snow() {
    replay(include_str!("fixtures/bss-chilly-reception.json"));
    replay(include_str!("fixtures/vgc-chilly-reception.json"));
}

#[test]
fn conditional_base_power_and_field_wide_boost_reset_match_showdown() {
    replay(include_str!("fixtures/bss-fickle-beam.json"));
    replay(include_str!("fixtures/bss-status-power-haze.json"));
}

#[test]
fn synchronize_reflects_before_berries_and_obeys_type_and_ability_immunity() {
    replay(include_str!("fixtures/bss-synchronize.json"));
    replay(include_str!("fixtures/bss-sync-limber.json"));
}

#[test]
fn mortal_spin_clears_the_users_side_only_after_a_successful_hit() {
    replay(include_str!("fixtures/vgc-mortal-spin.json"));
}

#[test]
fn substitute_separates_shield_damage_and_shed_tail_preserves_remaining_shield_hp() {
    replay(include_str!("fixtures/bss-substitute.json"));
    replay(include_str!("fixtures/bss-shed-tail.json"));
    replay(include_str!("fixtures/vgc-substitute-spread.json"));
    replay(include_str!("fixtures/bss-substitute-focus.json"));
    replay(include_str!("fixtures/bss-substitute-multihit.json"));
    replay(include_str!("fixtures/bss-substitute-secondaries.json"));
}

#[test]
fn double_shock_and_burn_up_spend_only_their_element_until_switching() {
    replay(include_str!("fixtures/bss-double-shock.json"));
    replay(include_str!("fixtures/bss-burn-up.json"));
}

#[test]
fn champions_emergency_switching_preserves_other_pivots_and_checks_berries_first() {
    replay(include_str!("fixtures/bss-wimp-out-double-pivot.json"));
    replay(include_str!("fixtures/bss-emergency-exit-berry.json"));
}

#[test]
fn voice_type_changes_bubble_modifiers_and_bulletproof_follow_native_hit_steps() {
    replay(include_str!(
        "fixtures/vgc-sparkling-aria-liquid-voice.json"
    ));
    replay(include_str!("fixtures/bss-water-bubble-bulletproof.json"));
}

#[test]
fn headless_controls_match_reseeding_tiebreaks_and_forced_results() {
    replay(include_str!("fixtures/bss-reseed-tiebreak-fraction.json"));
    replay(include_str!("fixtures/bss-tiebreak-raw-hp.json"));
    replay(include_str!("fixtures/bss-tiebreak-tie.json"));
    replay(include_str!("fixtures/bss-forcewin.json"));
    replay(include_str!("fixtures/bss-forcelose.json"));
    replay(include_str!("fixtures/bss-forcetie.json"));
}

#[test]
fn counter_and_mirror_coat_track_separate_damage_categories_and_retaliate_at_the_right_slot() {
    replay(include_str!("fixtures/bss-typed-counter.json"));
    replay(include_str!("fixtures/vgc-typed-counter.json"));
    replay(include_str!("fixtures/vgc-counter-fainted-attacker.json"));
}

#[test]
fn health_costs_match_clangorous_soul_thresholds_and_steel_beam_recoil() {
    replay(include_str!("fixtures/bss-clangorous-soul.json"));
    replay(include_str!("fixtures/bss-steel-beam.json"));
}

#[test]
fn held_stat_modifiers_and_fractional_turn_order_match_showdown() {
    replay(include_str!("fixtures/bss-quick-claw.json"));
    replay(include_str!("fixtures/bss-light-ball.json"));
    replay(include_str!("fixtures/bss-steadfast-surge-surfer.json"));
}

#[test]
fn binding_moves_pin_the_live_source_until_expiry_switch_or_removal() {
    replay(include_str!("fixtures/bss-binding-move-removal.json"));
    replay(include_str!("fixtures/bss-binding-move-expiry.json"));
}

#[test]
fn side_and_ally_abilities_match_native_activation_order() {
    replay(include_str!("fixtures/bss-screen-cleaner.json"));
    replay(include_str!("fixtures/vgc-aroma-veil.json"));
    replay(include_str!("fixtures/vgc-champions-healer.json"));
}

#[test]
fn unaware_ignores_relevant_boosts_and_respects_mold_breaker() {
    replay(include_str!("fixtures/bss-unaware.json"));
    replay(include_str!("fixtures/vgc-unaware-speed-tie.json"));
}

#[test]
fn weather_abilities_heal_and_change_accuracy_only_when_effective() {
    replay(include_str!("fixtures/bss-rain-dish.json"));
    replay(include_str!("fixtures/bss-snow-cloak.json"));
}

#[test]
fn move_properties_ignore_stages_or_redirects_in_the_matching_events() {
    replay(include_str!("fixtures/bss-sacred-sword.json"));
    replay(include_str!("fixtures/vgc-snipe-shot.json"));
}

#[test]
fn sleep_prevention_and_early_bird_follow_native_status_events() {
    replay(include_str!("fixtures/bss-yawn-abilities.json"));
    replay(include_str!("fixtures/bss-early-bird.json"));
}

#[test]
fn ability_hit_hooks_preserve_secondary_rng_and_ignore_only_matching_immunities() {
    replay(include_str!("fixtures/bss-scrappy.json"));
    replay(include_str!("fixtures/bss-secondary-protection.json"));
    replay(include_str!("fixtures/vgc-telepathy.json"));
}

#[test]
fn attacks_place_hazards_only_on_hits_even_through_substitute() {
    replay(include_str!("fixtures/bss-attack-hazards.json"));
}

#[test]
fn moves_that_cannot_be_selected_twice_still_work_when_forced() {
    replay(include_str!("fixtures/bss-gigaton-hammer.json"));
    replay(include_str!("fixtures/bss-blood-moon.json"));
}

#[test]
fn ability_events_change_typing_or_boosts_at_the_native_step() {
    replay(include_str!("fixtures/bss-mimicry.json"));
    replay(include_str!("fixtures/bss-anger-point.json"));
    replay(include_str!("fixtures/bss-eelevate.json"));
}

#[test]
fn unburden_can_resume_after_item_loss_and_recycle_only_restores_used_items() {
    replay(include_str!("fixtures/bss-recycle-unburden.json"));
    replay(include_str!("fixtures/bss-recycle-popped-balloon.json"));
}

#[test]
fn defog_and_ally_heals_keep_native_order_and_targeting() {
    replay(include_str!("fixtures/bss-defog.json"));
    replay(include_str!("fixtures/vgc-hospitality-pollen-puff.json"));
}

#[test]
fn supreme_overlord_snapshots_faints_and_receiver_starts_inherited_ability() {
    replay(include_str!("fixtures/bss-supreme-overlord.json"));
    replay(include_str!("fixtures/vgc-receiver.json"));
    replay(include_str!("fixtures/vgc-receiver-unnerve.json"));
}

#[test]
fn sacrifice_heals_wait_for_an_eligible_slot_occupant() {
    replay(include_str!("fixtures/bss-healing-wish.json"));
    replay(include_str!("fixtures/bss-lunar-dance.json"));
}

#[test]
fn ripen_doubles_berry_effects_but_only_announces_leftovers() {
    replay(include_str!("fixtures/bss-ripen.json"));
}

#[test]
fn beak_blast_burns_contact_only_while_preparing() {
    replay(include_str!("fixtures/bss-beak-blast.json"));
}

#[test]
fn curse_retargets_by_type_and_persists_until_the_victim_leaves() {
    replay(include_str!("fixtures/bss-ghost-curse.json"));
    replay(include_str!("fixtures/vgc-curse-targets.json"));
}

#[test]
fn plus_minus_and_curious_medicine_observe_the_live_partner() {
    replay(include_str!(
        "fixtures/vgc-plus-minus-curious-medicine.json"
    ));
}

#[test]
fn acupressure_samples_a_stat_and_magnetic_flux_checks_each_ally() {
    replay(include_str!("fixtures/vgc-acupressure.json"));
    replay(include_str!("fixtures/vgc-magnetic-flux.json"));
}

#[test]
fn delayed_attacks_use_original_source_and_current_slot_occupant() {
    replay(include_str!("fixtures/bss-future-sight.json"));
    replay(include_str!("fixtures/vgc-delayed-attacks.json"));
    replay(include_str!("fixtures/bss-future-fainted-user.json"));
}

#[test]
fn champions_belch_is_selectable_but_fails_until_the_user_eats_a_berry() {
    replay(include_str!("fixtures/bss-belch.json"));
}

#[test]
fn transform_preserves_the_original_roster_while_copying_live_combat_state() {
    replay(include_str!("fixtures/bss-imposter.json"));
    replay(include_str!("fixtures/bss-transform.json"));
    replay(include_str!("fixtures/vgc-imposter.json"));
}

#[test]
fn fainted_imposter_keeps_queued_move_metadata_after_restoring_base_move_slots() {
    replay(include_str!(
        "fixtures/vgc-imposter-faints-before-moving.json"
    ));
}

#[test]
fn dragon_darts_splits_when_possible_and_retargets_to_the_eligible_survivor() {
    replay(include_str!("fixtures/bss-dragon-darts.json"));
    replay(include_str!("fixtures/vgc-dragon-darts.json"));
}

#[test]
fn defensive_abilities_modify_only_matching_moves_and_stat_drops() {
    replay(include_str!("fixtures/bss-super-effective-guards.json"));
    replay(include_str!("fixtures/bss-defense-hit-hooks.json"));
}

#[test]
fn replacements_ignore_speed_modifiers_on_fainted_holders() {
    replay(include_str!("fixtures/vgc-fainted-choice-scarf.json"));
}

#[test]
fn held_status_orbs_and_poison_heal_follow_residual_order() {
    replay(include_str!("fixtures/bss-orbs-and-poison-heal.json"));
}

#[test]
fn wide_lens_chains_with_weather_evasion_before_accuracy_rounding() {
    replay(include_str!("fixtures/bss-wide-lens.json"));
}

#[test]
fn helping_hand_boosts_a_pending_partner_and_expires() {
    replay(include_str!("fixtures/vgc-helping-hand.json"));
}

#[test]
fn prankster_priority_dark_immunity_and_psychic_terrain() {
    replay(include_str!("fixtures/bss-prankster.json"));
}

#[test]
fn psychic_fangs_breaks_screens_through_substitute() {
    replay(include_str!("fixtures/bss-psychic-fangs.json"));
}

#[test]
fn toxic_debris_lays_at_most_two_layers_on_physical_hits() {
    replay(include_str!("fixtures/bss-toxic-debris.json"));
}

#[test]
fn liquid_ooze_reverses_drain_and_leech_seed_even_at_full_hp() {
    replay(include_str!("fixtures/bss-liquid-ooze.json"));
}

#[test]
fn compound_eyes_orders_equal_speed_accuracy_handlers() {
    replay(include_str!("fixtures/bss-compound-eyes.json"));
}

#[test]
fn fainted_cached_speed_is_reset_before_an_opposing_pivot_switch() {
    replay(include_str!("fixtures/vgc-faint-before-pivot.json"));
}

#[test]
fn friend_guard_weakens_hits_against_its_ally_and_not_itself() {
    replay(include_str!("fixtures/vgc-friend-guard.json"));
}

#[test]
fn priority_guard_abilities_protect_foes_but_not_allied_targets() {
    replay(include_str!("fixtures/vgc-priority-guards.json"));
}

#[test]
fn feint_breaks_protect_and_exposes_the_target_to_later_attacks() {
    replay(include_str!("fixtures/vgc-feint.json"));
}

#[test]
fn sheer_force_removes_secondaries_but_preserves_electro_shot_preparation_and_recoil() {
    replay(include_str!("fixtures/bss-sheer-force.json"));
}

#[test]
fn facade_doubles_power_without_the_standard_burn_penalty() {
    replay(include_str!("fixtures/bss-facade.json"));
}

#[test]
fn harvest_rolls_even_when_holding_a_berry_and_restores_it_in_sun() {
    replay(include_str!("fixtures/bss-harvest.json"));
}

#[test]
fn pickup_samples_adjacent_pokemon_that_consumed_items_this_turn() {
    replay(include_str!("fixtures/vgc-pickup.json"));
}

#[test]
fn berserk_checks_half_hp_before_a_single_hit_triggers_sitrus_berry() {
    replay(include_str!("fixtures/bss-berserk.json"));
}

#[test]
fn champions_resolves_berserk_and_sitrus_even_after_sheer_force_damage() {
    replay(include_str!("fixtures/bss-sheer-force-berserk.json"));
}

#[test]
fn anger_shell_adjusts_five_stats_before_eating_sitrus_berry() {
    replay(include_str!("fixtures/bss-anger-shell.json"));
}

#[test]
fn no_retreat_boosts_once_and_ghosts_can_still_switch() {
    replay(include_str!("fixtures/bss-no-retreat.json"));
    replay(include_str!("fixtures/bss-ghost-no-retreat.json"));
}

#[test]
fn analytic_checks_the_live_move_queue() {
    replay(include_str!("fixtures/bss-analytic.json"));
}

#[test]
fn gale_wings_grants_priority_at_full_health_only() {
    replay(include_str!("fixtures/bss-gale-wings.json"));
}

#[test]
fn gust_hits_bounce_and_fly_but_uses_their_distinct_damage_stages() {
    replay(include_str!("fixtures/bss-airborne-charge.json"));
}

#[test]
fn rivalry_distinguishes_same_opposite_and_genderless_targets() {
    replay(include_str!("fixtures/bss-rivalry.json"));
}

#[test]
fn cosmetic_formes_preserve_party_identity_and_switch_details() {
    replay(include_str!("fixtures/bss-cosmetic-forms.json"));
}

#[test]
fn perish_song_counts_down_and_batches_same_turn_faints() {
    replay(include_str!("fixtures/bss-perish-song-ending.json"));
    replay(include_str!("fixtures/vgc-perish-song.json"));
}

#[test]
fn hustle_and_zoom_lens_use_move_category_and_the_targets_pending_action() {
    replay(include_str!("fixtures/bss-hustle-zoom-lens.json"));
}

#[test]
fn fire_mane_boosts_physical_and_special_fire_attacks_after_mega_evolution() {
    replay(include_str!("fixtures/bss-fire-mane.json"));
}

#[test]
fn flying_press_combines_fighting_and_flying_effectiveness() {
    replay(include_str!("fixtures/bss-flying-press.json"));
}

#[test]
fn effect_spore_rolls_one_status_and_respects_powder_immunity() {
    replay(include_str!("fixtures/bss-effect-spore.json"));
    replay(include_str!("fixtures/bss-effect-spore-sleep.json"));
}

#[test]
fn electric_charge_accumulates_from_moves_and_abilities_until_an_electric_attempt() {
    replay(include_str!("fixtures/bss-electric-charge.json"));
    replay(include_str!("fixtures/vgc-wind-power.json"));
}

#[test]
fn palafin_permanently_changes_forme_and_only_announces_hero_once() {
    replay(include_str!("fixtures/bss-zero-to-hero.json"));
}

#[test]
fn throat_chop_interrupts_pending_sound_moves_and_disables_them_for_a_turn() {
    replay(include_str!("fixtures/bss-throat-chop.json"));
}

#[test]
fn klutz_suppresses_item_effects_without_removing_held_items() {
    replay(include_str!("fixtures/bss-klutz.json"));
}

#[test]
fn stomping_tantrum_distinguishes_failures_from_protected_attempts() {
    replay(include_str!("fixtures/bss-stomping-tantrum.json"));
}

#[test]
fn lash_out_only_boosts_when_a_stat_fell_this_turn() {
    replay(include_str!("fixtures/bss-lash-out.json"));
}

#[test]
fn iron_ball_slows_and_grounds_flying_and_levitating_holders() {
    replay(include_str!("fixtures/bss-iron-ball.json"));
}

#[test]
fn reversal_and_flail_use_the_native_hp_thresholds() {
    replay(include_str!("fixtures/bss-reversal-flail.json"));
}

#[test]
fn payback_requires_a_target_that_has_acted_and_did_not_switch_this_turn() {
    replay(include_str!("fixtures/bss-payback.json"));
}

#[test]
fn bug_bite_and_pluck_eat_berries_without_replacing_the_held_item() {
    replay(include_str!("fixtures/bss-steal-berry.json"));
}

#[test]
fn beat_up_uses_living_unstatused_roster_members_and_their_base_attack() {
    replay(include_str!("fixtures/bss-beat-up.json"));
    replay(include_str!("fixtures/bss-beat-up-moxie.json"));
}

#[test]
fn battle_bond_only_triggers_once_and_ash_water_shuriken_always_hits_three_times() {
    replay(include_str!("fixtures/bss-battle-bond.json"));
    replay(include_str!("fixtures/bss-ash-water-shuriken.json"));
}

#[test]
fn type_adding_moves_replace_the_previous_added_type_and_retarget_curse() {
    replay(include_str!("fixtures/bss-added-types.json"));
    replay(include_str!("fixtures/vgc-added-ghost-curse.json"));
}

#[test]
fn uproar_wakes_sleepers_locks_the_user_and_blocks_new_sleep_until_expiry() {
    replay(include_str!("fixtures/bss-uproar.json"));
}

#[test]
fn special_moves_can_target_physical_defense_without_changing_category() {
    replay(include_str!("fixtures/bss-special-physical-defense.json"));
}

#[test]
fn situational_attack_power_follows_held_items_current_hp_and_grounding() {
    replay(include_str!("fixtures/bss-situational-power.json"));
    replay(include_str!("fixtures/bss-rising-voltage.json"));
}

#[test]
fn contact_stat_reactions_rain_status_cures_and_fractional_priority_follow_native_order() {
    replay(include_str!("fixtures/bss-contact-stat-abilities.json"));
    replay(include_str!("fixtures/bss-stall-priority.json"));
    replay(include_str!("fixtures/bss-hydration.json"));
}

#[test]
fn confuse_ray_respects_existing_confusion_tangled_feet_and_own_tempo() {
    replay(include_str!("fixtures/bss-confusion-accuracy.json"));
}

#[test]
fn fluffy_punk_rock_and_spicy_spray_react_to_move_traits() {
    replay(include_str!("fixtures/bss-fluffy.json"));
    replay(include_str!("fixtures/bss-punk-rock.json"));
    replay(include_str!("fixtures/bss-spicy-spray.json"));
}

#[test]
fn belly_drum_pays_for_maximal_attack_or_reverses_with_contrary() {
    replay(include_str!("fixtures/bss-belly-drum.json"));
}

#[test]
fn quash_moves_an_eligible_queued_action_after_every_other_move() {
    replay(include_str!("fixtures/vgc-quash.json"));
}

#[test]
fn thief_covet_and_pickpocket_transfer_held_items_at_their_native_events() {
    replay(include_str!("fixtures/bss-thief-covet.json"));
    replay(include_str!("fixtures/bss-pickpocket.json"));
    replay(include_str!("fixtures/bss-pickpocket-pivot.json"));
}

#[test]
fn life_dew_and_heal_pulse_follow_doubles_targeting_and_healing_order() {
    replay(include_str!("fixtures/vgc-life-dew-heal-pulse.json"));
}

#[test]
fn final_gambit_and_innards_out_queue_faints_in_native_order() {
    replay(include_str!("fixtures/bss-final-gambit.json"));
    replay(include_str!("fixtures/bss-innards-out.json"));
}

#[test]
fn anticipation_and_forewarn_inspect_live_foes_and_sample_tied_threats() {
    replay(include_str!("fixtures/vgc-entry-move-threats.json"));
}

#[test]
fn seed_sower_starts_grassy_terrain_when_hit() {
    replay(include_str!("fixtures/bss-seed-sower.json"));
}

#[test]
fn forecast_tracks_weather_and_suppression_while_hunger_switch_changes_aura_wheel() {
    replay(include_str!("fixtures/bss-forecast.json"));
    replay(include_str!("fixtures/bss-hunger-switch.json"));
}

#[test]
fn phantom_force_breaks_protect_and_sky_attack_consumes_power_herb() {
    replay(include_str!("fixtures/vgc-phantom-force.json"));
    replay(include_str!("fixtures/bss-sky-attack.json"));
}

#[test]
fn contact_guards_apply_their_effects_and_unseen_fist_deals_quarter_damage() {
    replay(include_str!("fixtures/bss-personal-guards.json"));
    replay(include_str!("fixtures/bss-unseen-fist.json"));
}

#[test]
fn wide_and_quick_guard_protect_the_side_until_feint_or_turn_end() {
    replay(include_str!("fixtures/vgc-side-guards.json"));
}

#[test]
fn turn_outcomes_power_retaliation_and_trigger_burning_jealousy() {
    replay(include_str!("fixtures/bss-retaliation-power.json"));
    replay(include_str!("fixtures/vgc-coaching-burning-jealousy.json"));
}

#[test]
fn endure_caps_lethal_move_damage_before_focus_sash() {
    replay(include_str!("fixtures/bss-endure.json"));
}

#[test]
fn quick_draw_and_poison_touch_consume_rng_only_for_eligible_actions() {
    replay(include_str!("fixtures/bss-quick-draw.json"));
    replay(include_str!("fixtures/bss-poison-touch.json"));
}

#[test]
fn psychic_noise_blocks_healing_and_disables_moves_until_expiry_or_herb() {
    replay(include_str!("fixtures/bss-psychic-noise.json"));
    replay(include_str!("fixtures/bss-psychic-noise-herb.json"));
}

#[test]
fn speed_ratios_and_the_active_terrain_shape_move_power_without_leaking_turn_order() {
    replay(include_str!("fixtures/bss-speed-power.json"));
    replay(include_str!("fixtures/bss-terrain-pulse-aqua-ring.json"));
}

#[test]
fn upper_hand_checks_priority_and_steely_spirit_boosts_allied_steel_attacks() {
    replay(include_str!("fixtures/bss-upper-hand.json"));
    replay(include_str!("fixtures/vgc-steely-spirit.json"));
}

#[test]
fn replacement_batches_keep_outgoing_speed_and_allow_a_required_empty_slot() {
    replay(include_str!(
        "fixtures/random-doubles-tailwind-replacements.json"
    ));
    replay(include_str!(
        "fixtures/ranch-simultaneous-partial-replacements.json"
    ));
}

#[test]
fn a_fainted_doubles_slot_retains_its_native_mega_advertisement() {
    replay(include_str!("fixtures/vgc-fainted-slot-mega-request.json"));
}

#[test]
fn full_health_moonlight_preserves_update_rng_and_is_not_a_move_failure() {
    replay(include_str!("fixtures/vgc-full-health-moonlight.json"));
}

#[test]
fn instruct_repeats_the_last_move_now_using_its_original_target_and_pp() {
    replay(include_str!("fixtures/vgc-instruct.json"));
}

#[test]
fn shell_side_arm_uses_the_stronger_category_and_its_contact_rules() {
    replay(include_str!("fixtures/bss-shell-side-arm.json"));
}

#[test]
fn sleep_talk_selects_eligible_moves_and_pressure_consumes_the_callers_pp() {
    replay(include_str!("fixtures/bss-sleep-talk.json"));
    replay(include_str!("fixtures/bss-sleep-talk-rest.json"));
}

#[test]
fn torment_disables_repeats_and_octolock_tracks_the_trapper_until_it_leaves() {
    replay(include_str!("fixtures/bss-torment.json"));
    replay(include_str!("fixtures/bss-octolock.json"));
}

#[test]
fn safeguard_blocks_status_for_five_turns_unless_the_move_infiltrates() {
    replay(include_str!("fixtures/bss-safeguard.json"));
}

#[test]
fn ability_replacement_handles_sleep_boosts_immunity_and_ability_shield() {
    replay(include_str!("fixtures/bss-ability-replacement.json"));
}

#[test]
fn natdex_legacy_moves_use_friendship_and_hidden_power_type_in_damage_and_requests() {
    replay(include_str!("fixtures/natdex-legacy-moves.json"));
}

#[test]
fn cheek_pouch_recovers_after_the_berry_heals() {
    replay(include_str!("fixtures/bss-cheek-pouch.json"));
}

#[test]
fn solar_power_boosts_special_damage_and_drains_hp_except_under_an_umbrella() {
    replay(include_str!("fixtures/bss-solar-power.json"));
}

#[test]
fn piercing_drill_bypasses_protect_only_for_contact_moves() {
    replay(include_str!("fixtures/bss-piercing-drill.json"));
}

#[test]
fn auras_modify_power_and_aura_break_reverses_the_modifier() {
    replay(include_str!("fixtures/vgc-auras.json"));
}

#[test]
fn stalwart_targets_through_follow_me() {
    replay(include_str!("fixtures/vgc-stalwart.json"));
    replay(include_str!("fixtures/vgc-stalwart-side-target.json"));
}

#[test]
fn stench_adds_a_flinch_roll_only_to_attacks_that_lack_one() {
    replay(include_str!("fixtures/bss-stench.json"));
}

#[test]
fn red_card_forces_a_switch_unless_the_attacker_cannot_be_dragged() {
    replay(include_str!("fixtures/bss-red-card.json"));
    replay(include_str!("fixtures/bss-red-card-guard-dog.json"));
    replay(include_str!("fixtures/bss-red-card-life-orb.json"));
}

#[test]
fn knocking_off_the_appropriate_mega_stone_does_not_get_a_boost() {
    replay(include_str!("fixtures/bss-floettite-knock-off.json"));
}

#[test]
fn terrain_seeds_activate_on_field_changes_and_on_entry() {
    replay(include_str!("fixtures/bss-terrain-seeds.json"));
}

#[test]
fn linked_trapping_ends_when_the_other_pokemon_leaves() {
    replay(include_str!("fixtures/bss-spirit-shackle.json"));
    replay(include_str!("fixtures/bss-jaw-lock.json"));
}

#[test]
fn fling_uses_the_held_item_and_can_discard_it_without_hitting() {
    replay(include_str!("fixtures/bss-fling-iron-ball.json"));
    replay(include_str!("fixtures/bss-fling-poison-barb.json"));
    replay(include_str!("fixtures/bss-fling-berry.json"));
}

#[test]
fn kings_rock_adds_flinching_after_sheer_force_and_when_thrown() {
    replay(include_str!("fixtures/bss-kings-rock.json"));
}

#[test]
fn long_reach_removes_contact_for_damage_protection_and_items() {
    replay(include_str!("fixtures/bss-long-reach.json"));
}

#[test]
fn stockpile_counts_successful_boosts_and_spit_up_and_swallow_consume_it() {
    replay(include_str!("fixtures/bss-stockpile.json"));
}

#[test]
fn opportunist_copies_only_positive_foe_stages_after_moves_and_residuals() {
    replay(include_str!("fixtures/bss-opportunist.json"));
}

#[test]
fn magnet_rise_and_smack_down_change_ground_immunity_and_cancel_airtime() {
    replay(include_str!("fixtures/bss-magnet-rise.json"));
    replay(include_str!("fixtures/bss-smack-down-fly.json"));
}

#[test]
fn block_checks_substitute_and_trapping_immunity() {
    replay(include_str!("fixtures/bss-block.json"));
}

#[test]
fn howl_and_teeter_dance_apply_to_their_doubles_targets() {
    replay(include_str!("fixtures/vgc-howl.json"));
    replay(include_str!("fixtures/vgc-teeter-dance.json"));
}

#[test]
fn marvel_scale_and_grass_pelt_modify_physical_defense() {
    replay(include_str!("fixtures/bss-marvel-scale.json"));
    replay(include_str!("fixtures/bss-grass-pelt.json"));
}

#[test]
fn quick_feet_keeps_full_speed_through_paralysis() {
    replay(include_str!("fixtures/bss-quick-feet.json"));
}

#[test]
fn sand_spit_starts_sandstorm_when_damaged() {
    replay(include_str!("fixtures/bss-sand-spit.json"));
}

#[test]
fn moody_applies_two_distinct_residual_stage_changes() {
    replay(include_str!("fixtures/bss-moody.json"));
}

#[test]
fn magma_armor_prevents_freeze() {
    replay(include_str!("fixtures/bss-magma-armor.json"));
}

#[test]
fn memento_faints_the_user_only_after_a_hit() {
    replay(include_str!("fixtures/bss-memento.json"));
}

#[test]
fn minimize_keeps_its_separate_trampling_drawback_when_boosts_are_capped() {
    replay(include_str!("fixtures/bss-minimize.json"));
}

#[test]
fn haze_does_not_run_individual_hit_updates_during_speed_ties() {
    replay(include_str!("fixtures/bss-haze-speed-tie.json"));
}

#[test]
fn mega_uses_raw_speed_until_the_action_queue_recalculates_speed() {
    replay(include_str!("fixtures/bss-mega-speed-stage-tie.json"));
}

#[test]
fn grassy_residual_ties_follow_the_original_effect_discovery_order() {
    replay(include_str!("fixtures/vgc-grassy-residual-speed-tie.json"));
}

#[test]
fn dragon_cheer_copies_its_stored_critical_stage_bonus() {
    replay(include_str!("fixtures/vgc-dragon-cheer.json"));
}

#[test]
fn power_trick_toggles_raw_stats_and_can_be_passed() {
    replay(include_str!("fixtures/bss-power-trick.json"));
}

#[test]
fn topsy_turvy_and_psych_up_change_stages_without_boost_events() {
    replay(include_str!("fixtures/bss-invert-copy-stats.json"));
}

#[test]
fn pp_reduction_can_exhaust_a_move_after_it_is_queued() {
    replay(include_str!("fixtures/bss-spite-queued-pp.json"));
    replay(include_str!("fixtures/bss-eerie-spell.json"));
}

#[test]
fn attract_bypasses_substitute_and_ends_when_the_source_leaves() {
    replay(include_str!("fixtures/bss-attract.json"));
}

#[test]
fn snore_and_tri_attack_run_their_native_status_paths() {
    replay(include_str!("fixtures/bss-snore.json"));
    replay(include_str!("fixtures/bss-tri-attack.json"));
}

#[test]
fn axe_kick_can_crash_against_protection() {
    replay(include_str!("fixtures/bss-axe-kick.json"));
}

#[test]
fn forced_berry_meals_update_effects_and_available_moves() {
    replay(include_str!("fixtures/bss-stuff-cheeks.json"));
    replay(include_str!("fixtures/bss-teatime.json"));
}

#[test]
fn corrosive_gas_reports_item_loss_or_failure_per_target() {
    replay(include_str!("fixtures/vgc-corrosive-gas.json"));
}

#[test]
fn normal_gem_is_consumed_on_the_hit_and_boosts_damage_to_substitute() {
    replay(include_str!("fixtures/bss-normal-gem.json"));
}

#[test]
fn copied_hunger_switch_does_not_change_the_transformed_form() {
    replay(include_str!("fixtures/bss-imposter-morpeko.json"));
}

#[test]
fn friendly_pollen_puff_still_triggers_life_orb() {
    replay(include_str!("fixtures/vgc-pollen-puff-life-orb.json"));
}

#[test]
fn requests_keep_possible_traps_and_hide_disables_for_fainted_slots() {
    replay(include_str!("fixtures/bss-ghost-maybe-trapped.json"));
    replay(include_str!("fixtures/vgc-imprison-fainted-request.json"));
}

#[test]
fn forced_repeat_hints_follow_direct_faint_messages() {
    replay(include_str!("fixtures/bss-gigaton-faint-hint.json"));
}

#[test]
fn shed_tail_skips_the_pre_switch_update_even_during_a_speed_tie() {
    replay(include_str!("fixtures/vgc-shed-tail-speed-tie.json"));
}

#[test]
fn lock_on_reaches_the_original_airborne_target() {
    replay(include_str!("fixtures/bss-lock-on-fly.json"));
}

#[test]
fn syrup_bomb_slows_each_turn_until_expiry_or_source_switch() {
    replay(include_str!("fixtures/bss-syrup-bomb.json"));
}

#[test]
fn fairy_lock_traps_for_the_following_turn_with_ghost_immunity() {
    replay(include_str!("fixtures/bss-fairy-lock.json"));
}

#[test]
fn after_you_promotes_an_allied_or_opposing_move_in_doubles_only() {
    replay(include_str!("fixtures/vgc-after-you.json"));
    replay(include_str!("fixtures/bss-after-you-fails.json"));
}

#[test]
fn round_chains_across_both_sides_even_if_the_initial_hit_is_protected() {
    replay(include_str!("fixtures/vgc-round-chain.json"));
}

#[test]
fn ally_switch_swaps_queued_actors_but_preserves_selected_target_slots() {
    replay(include_str!("fixtures/vgc-ally-switch.json"));
    replay(include_str!("fixtures/bss-ally-switch-fails.json"));
}

#[test]
fn electrify_changes_the_queued_move_for_one_turn_and_fails_after_its_action() {
    replay(include_str!("fixtures/bss-electrify.json"));
}

#[test]
fn role_play_starts_the_new_ability_and_fails_when_already_copied() {
    replay(include_str!("fixtures/bss-role-play.json"));
}

#[test]
fn reflect_type_copies_base_and_added_types_with_correct_reveal_timing() {
    replay(include_str!("fixtures/bss-reflect-type.json"));
    replay(include_str!("fixtures/vgc-reflect-type-added.json"));
}

#[test]
fn gastro_acid_disables_an_ability_until_switch_out_and_respects_ability_shield() {
    replay(include_str!("fixtures/bss-gastro-acid.json"));
    replay(include_str!("fixtures/bss-gastro-acid-shield.json"));
}

#[test]
fn magic_room_suppresses_items_and_keeps_the_original_source_for_expiry() {
    replay(include_str!("fixtures/bss-magic-room.json"));
    replay(include_str!("fixtures/bss-magic-room-toggle.json"));
}

#[test]
fn copycat_uses_the_previous_move_and_charges_pressure_to_the_caller() {
    replay(include_str!("fixtures/bss-copycat.json"));
}

#[test]
fn copycat_can_resolve_a_forced_move_absent_from_the_users_moveset() {
    replay(include_str!("fixtures/bss-copycat-charge.json"));
}

#[test]
fn individual_status_berries_cure_only_their_status() {
    replay(include_str!("fixtures/bss-aspear-berry.json"));
    replay(include_str!("fixtures/bss-cheri-berry.json"));
    replay(include_str!("fixtures/bss-pecha-berry.json"));
    replay(include_str!("fixtures/bss-rawst-berry.json"));
    replay(include_str!("fixtures/bss-persim-berry.json"));
}

#[test]
fn oran_and_leppa_berries_apply_their_own_ripened_recovery() {
    replay(include_str!("fixtures/bss-oran-berry.json"));
    replay(include_str!("fixtures/bss-leppa-berry.json"));
}

#[test]
fn champions_eject_button_replaces_the_holder_without_canceling_the_attackers_pivot() {
    replay(include_str!("fixtures/bss-eject-button.json"));
}

#[test]
fn shell_bell_heals_from_real_damage_and_focus_band_rolls_on_every_damage_event() {
    replay(include_str!("fixtures/bss-shell-bell.json"));
    replay(include_str!("fixtures/bss-focus-band-draws.json"));
    replay(include_str!("fixtures/bss-focus-band-save.json"));
}

#[test]
fn metronome_item_stacks_successful_repeats_caps_and_resets() {
    replay(include_str!("fixtures/bss-metronome-item.json"));
}

#[test]
fn national_dex_legacy_moves_match_weather_status_and_effect_only_actions() {
    replay(include_str!("fixtures/natdex-hail.json"));
    replay(include_str!("fixtures/natdex-legacy-status.json"));
}

#[test]
fn baton_pass_skips_the_pre_switch_update_even_during_a_speed_tie() {
    replay(include_str!("fixtures/vgc-baton-pass-speed-tie.json"));
}

#[test]
fn custom_game_debug_explains_technician_weather_and_protect_rolls() {
    replay(include_str!("fixtures/custom-damage-debug.json"));
}

#[test]
fn sparkling_aria_bypasses_covert_cloak_only_when_it_hits_multiple_targets() {
    replay(include_str!("fixtures/bss-sparkling-aria-cloak.json"));
    replay(include_str!("fixtures/vgc-sparkling-aria-cloak.json"));
}

#[test]
fn national_dex_attacks_preserve_hp_limits_and_conditioned_damage() {
    replay(include_str!("fixtures/natdex-health-scaled-attacks.json"));
    replay(include_str!("fixtures/natdex-nonlethal-attacks.json"));
    replay(include_str!("fixtures/natdex-incinerate-and-order.json"));
    replay(include_str!("fixtures/natdex-sleep-attacks.json"));
    replay(include_str!("fixtures/natdex-retaliate.json"));
}

#[test]
fn national_dex_legacy_items_apply_only_their_native_held_effects() {
    replay(include_str!(
        "fixtures/natdex-black-sludge-and-inert-item.json"
    ));
    replay(include_str!("fixtures/natdex-defense-items.json"));
    replay(include_str!("fixtures/natdex-berry-juice.json"));
}

#[test]
fn magic_room_temporarily_ignores_but_does_not_remove_a_choice_lock() {
    replay(include_str!("fixtures/bss-magic-room-choice-lock.json"));
}

#[test]
fn national_dex_projects_both_generic_and_signature_crystal_requests() {
    replay(include_str!("fixtures/natdex-z-crystal-requests.json"));
}

#[test]
fn national_dex_z_attacks_replace_base_damage_even_through_protect() {
    replay(include_str!(
        "fixtures/natdex-z-damage-through-protect.json"
    ));
    replay(include_str!("fixtures/natdex-z-signature.json"));
    replay(include_str!("fixtures/natdex-z-weather-ball.json"));
    replay(include_str!("fixtures/custom-z-quick-guard.json"));
}

#[test]
fn national_dex_z_status_effects_are_applied_before_the_base_move() {
    replay(include_str!("fixtures/natdex-z-protect-under-taunt.json"));
    replay(include_str!("fixtures/natdex-z-splash.json"));
    replay(include_str!("fixtures/natdex-z-refresh.json"));
    replay(include_str!("fixtures/natdex-z-replacement-heal.json"));
    replay(include_str!("fixtures/natdex-z-evoboost.json"));
}

#[test]
fn legacy_signature_z_attacks_apply_their_inherited_damage_and_field_hooks() {
    replay(include_str!("fixtures/custom-z-soulblaze.json"));
    replay(include_str!("fixtures/natdex-z-genesis.json"));
    replay(include_str!("fixtures/natdex-z-guardian.json"));
    replay(include_str!("fixtures/natdex-z-stormshards.json"));
    replay(include_str!("fixtures/natdex-z-sunraze.json"));
    replay(include_str!("fixtures/natdex-z-moonraze.json"));
    replay(include_str!("fixtures/custom-z-ultra.json"));
    replay(include_str!("fixtures/custom-ultra-and-mega.json"));
}

#[test]
fn national_dex_rayquaza_mega_evolves_by_move_and_starts_delta_stream() {
    replay(include_str!("fixtures/natdex-rayquaza-delta-stream.json"));
}

#[test]
fn primal_reversion_keeps_the_form_and_only_the_current_weather_source() {
    replay(include_str!("fixtures/natdex-primal-sunlight.json"));
    replay(include_str!("fixtures/natdex-primal-switch-in-order.json"));
    replay(include_str!("fixtures/custom-primal-rain.json"));
    replay(include_str!("fixtures/custom-primal-clash.json"));
}

#[test]
fn inherited_attacks_respect_weather_power_recoil_and_guard_callbacks() {
    replay(include_str!("fixtures/natdex-inherited-storms.json"));
    replay(include_str!("fixtures/custom-inherited-power.json"));
    replay(include_str!("fixtures/natdex-chloroblast.json"));
    replay(include_str!("fixtures/custom-hyperspace.json"));
}

#[test]
fn inherited_recovery_moves_preserve_healing_and_status_order() {
    replay(include_str!("fixtures/natdex-shore-up-and-status.json"));
    replay(include_str!(
        "fixtures/natdex-purify-and-floral-healing.json"
    ));
    replay(include_str!("fixtures/custom-legacy-group-heals.json"));
    replay(include_str!(
        "fixtures/natdex-z-recover-under-heal-block.json"
    ));
}

#[test]
fn inherited_type_and_weight_moves_change_only_their_intended_matchups() {
    replay(include_str!(
        "fixtures/natdex-conversion-and-camouflage.json"
    ));
    replay(include_str!("fixtures/natdex-tar-shot.json"));
    replay(include_str!("fixtures/custom-autotomize-weight.json"));
    replay(include_str!(
        "fixtures/natdex-foresight-and-odor-sleuth.json"
    ));
    replay(include_str!("fixtures/natdex-miracle-eye.json"));
}

#[test]
fn inherited_critical_and_heal_conditions_preserve_rolls_and_expiration() {
    replay(include_str!(
        "fixtures/natdex-lucky-chant-and-laser-focus.json"
    ));
    replay(include_str!("fixtures/natdex-laser-focus-transform.json"));
    replay(include_str!("fixtures/natdex-direct-heal-block.json"));
}

#[test]
fn inherited_charged_attacks_retain_targets_and_consume_pp_once() {
    replay(include_str!("fixtures/natdex-freeze-shock-and-z-move.json"));
    replay(include_str!("fixtures/natdex-ice-burn-power-herb.json"));
    replay(include_str!("fixtures/custom-razor-wind-charge.json"));
}

#[test]
fn inherited_move_streaks_respect_owner_turn_and_previous_success() {
    replay(include_str!("fixtures/custom-fury-cutter-chain.json"));
    replay(include_str!("fixtures/custom-echoed-voice-chain.json"));
    replay(include_str!("fixtures/custom-fusion-sequence.json"));
}

#[test]
fn inherited_rolling_attacks_lock_the_target_and_expire_on_hit_count_or_protect() {
    replay(include_str!("fixtures/custom-rollout-defense-curl.json"));
    replay(include_str!(
        "fixtures/custom-ice-ball-lock-and-protect.json"
    ));
}

#[test]
fn inherited_damage_hooks_preserve_suppression_grounding_and_stage_order() {
    replay(include_str!(
        "fixtures/natdex-thousand-arrows-grounding.json"
    ));
    replay(include_str!("fixtures/natdex-thousand-waves-link.json"));
    replay(include_str!("fixtures/natdex-spectral-thief-contrary.json"));
    replay(include_str!(
        "fixtures/natdex-core-enforcer-substitute.json"
    ));
    replay(include_str!("fixtures/custom-flame-burst-splash.json"));
}

#[test]
fn inherited_stage_moves_keep_target_filters_and_hit_costs() {
    replay(include_str!("fixtures/custom-inherited-field-stages.json"));
    replay(include_str!(
        "fixtures/custom-captivate-and-venom-drench.json"
    ));
    replay(include_str!(
        "fixtures/custom-fillet-away-and-power-shift.json"
    ));
}

#[test]
fn inherited_damage_callbacks_keep_roll_timing_and_current_state() {
    replay(include_str!("fixtures/custom-magnitude-rolls.json"));
    replay(include_str!(
        "fixtures/custom-present-damage-and-healing.json"
    ));
    replay(include_str!("fixtures/custom-legacy-power-callbacks.json"));
    replay(include_str!("fixtures/natdex-secret-power-terrain.json"));
}

#[test]
fn inherited_typed_items_supply_power_and_respect_suppression_and_consumption() {
    replay(include_str!("fixtures/natdex-judgment-and-plate.json"));
    replay(include_str!("fixtures/natdex-multi-attack-memory.json"));
    replay(include_str!("fixtures/natdex-techno-blast-drive.json"));
    replay(include_str!("fixtures/natdex-masked-ivy-cudgel.json"));
    replay(include_str!("fixtures/custom-natural-gift-recycle.json"));
}

#[test]
fn inherited_field_types_and_power_modifiers_expire_on_the_native_turn() {
    replay(include_str!(
        "fixtures/custom-ion-deluge-and-plasma-fists.json"
    ));
    replay(include_str!(
        "fixtures/custom-mud-sport-and-water-sport.json"
    ));
}

#[test]
fn inherited_conditions_control_items_stats_accuracy_and_fire() {
    replay(include_str!("fixtures/custom-embargo-held-items.json"));
    replay(include_str!("fixtures/custom-mist-boosts-and-defog.json"));
    replay(include_str!("fixtures/custom-telekinesis-and-gravity.json"));
    replay(include_str!("fixtures/custom-powder-fire-moves.json"));
}

#[test]
fn inherited_guards_and_spotlight_apply_at_native_priority() {
    replay(include_str!(
        "fixtures/custom-crafty-shield-and-spotlight.json"
    ));
    replay(include_str!("fixtures/custom-mat-block-first-action.json"));
}

#[test]
fn inherited_reflection_and_snatch_keep_their_distinct_invocation_rules() {
    replay(include_str!("fixtures/custom-magic-coat-reflection.json"));
    replay(include_str!("fixtures/custom-snatch-and-z-status.json"));
    replay(include_str!("fixtures/custom-snatch-swallow.json"));
}

#[test]
fn inherited_transfer_pivot_and_nightmare_match_native_lifecycles() {
    replay(include_str!("fixtures/custom-bestow-and-teleport.json"));
    replay(include_str!("fixtures/custom-doodle-ability-shield.json"));
    replay(include_str!(
        "fixtures/custom-nightmare-sleep-lifecycle.json"
    ));
}

#[test]
fn inherited_move_reactions_and_relic_forme_match_native_order() {
    replay(include_str!("fixtures/custom-rage-reaction.json"));
    replay(include_str!("fixtures/custom-grudge-pp-source.json"));
    replay(include_str!("fixtures/custom-relic-song-forme-cycle.json"));
}

#[test]
fn commander_blocks_targeting_and_passing_ends_after_dondozo_faints() {
    replay(include_str!(
        "fixtures/custom-commander-release-and-order-up.json"
    ));
    replay(include_str!(
        "fixtures/custom-commander-cancels-queued-move.json"
    ));
}

#[test]
fn doubles_pledges_promote_the_second_action_and_preserve_side_timers() {
    replay(include_str!(
        "fixtures/custom-pledge-pairs-and-side-conditions.json"
    ));
}

#[test]
fn shell_trap_waits_for_an_enemy_physical_hit_that_reaches_the_user() {
    replay(include_str!(
        "fixtures/custom-shell-trap-trigger-order.json"
    ));
}

#[test]
fn pursuit_intercepts_the_outgoing_member_before_switch_and_pivot_requests() {
    replay(include_str!(
        "fixtures/custom-pursuit-before-voluntary-switch.json"
    ));
    replay(include_str!(
        "fixtures/custom-pursuit-cancels-fainted-switch.json"
    ));
    replay(include_str!(
        "fixtures/custom-pursuit-before-pivot-request.json"
    ));
}

#[test]
fn bide_stores_received_damage_and_releases_without_an_accuracy_roll() {
    replay(include_str!(
        "fixtures/custom-bide-fixed-damage-through-protect.json"
    ));
    replay(include_str!("fixtures/custom-bide-strikes-substitute.json"));
    replay(include_str!("fixtures/custom-bide-ghost-immunity.json"));
    replay(include_str!("fixtures/custom-bide-aborted-by-sleep.json"));
    replay(include_str!(
        "fixtures/custom-bide-retargets-after-switch.json"
    ));
}

#[test]
fn mimic_sketch_and_conversion_two_use_distinct_native_move_histories() {
    replay(include_str!(
        "fixtures/custom-mimic-restores-original-pp.json"
    ));
    replay(include_str!(
        "fixtures/custom-sketch-persists-across-switches.json"
    ));
    replay(include_str!(
        "fixtures/custom-conversion-two-uses-executed-type.json"
    ));
}

#[test]
fn z_sleep_talk_powers_the_called_attack_without_spending_a_second_z_move() {
    replay(include_str!(
        "fixtures/custom-z-sleep-talk-damaging-call.json"
    ));
}

#[test]
fn inherited_callers_choose_the_same_moves_and_targets_as_showdown() {
    replay(include_str!(
        "fixtures/custom-nature-power-terrain-and-z-call.json"
    ));
    replay(include_str!(
        "fixtures/custom-me-first-reads-queued-attack.json"
    ));
    replay(include_str!(
        "fixtures/custom-mirror-move-reads-selected-move.json"
    ));
    replay(include_str!(
        "fixtures/custom-assist-selects-from-team-moves.json"
    ));
    replay(include_str!(
        "fixtures/custom-metronome-chooses-pinned-attack.json"
    ));
    replay(include_str!(
        "fixtures/custom-metronome-chooses-pinned-hazard.json"
    ));
}

#[test]
fn sky_drop_locks_the_carrier_and_lifts_its_target_until_landing_or_gravity() {
    replay(include_str!(
        "fixtures/custom-sky-drop-lifts-and-releases-target.json"
    ));
    replay(include_str!(
        "fixtures/custom-sky-drop-rejects-heavy-target.json"
    ));
    replay(include_str!(
        "fixtures/custom-sky-drop-flying-target-is-immune.json"
    ));
    replay(include_str!(
        "fixtures/custom-sky-drop-airspeed-and-gravity.json"
    ));
    replay(include_str!("fixtures/custom-sky-drop-faster-rider.json"));
    replay(include_str!(
        "fixtures/custom-sky-drop-releases-rider-after-faint.json"
    ));
}

#[test]
fn rattled_responds_to_intimidate_and_a_dark_attack() {
    replay(include_str!("fixtures/bss-rattled.json"));
}

#[test]
fn cud_chew_repeats_a_berry_next_turn_even_if_it_was_eaten_after_residuals() {
    replay(include_str!("fixtures/bss-cud-chew.json"));
    replay(include_str!("fixtures/bss-cud-chew-after-residual.json"));
}

#[test]
fn mega_sol_uses_sun_only_for_the_users_move_and_keeps_real_weather() {
    replay(include_str!("fixtures/bss-mega-sol.json"));
}

#[test]
fn assurance_uses_same_turn_damage_even_if_the_target_healed() {
    replay(include_str!("fixtures/vgc-assurance.json"));
}

#[test]
fn clear_smog_removes_boosts_and_infernal_parade_doubles_on_status() {
    replay(include_str!("fixtures/bss-clear-smog.json"));
    replay(include_str!("fixtures/bss-infernal-parade.json"));
}

#[test]
fn electromorphosis_waits_for_the_attackers_stat_drops() {
    replay(include_str!("fixtures/bss-charge-after-drops.json"));
}

#[test]
fn heal_bell_includes_the_bench_and_respects_active_ability_immunity() {
    replay(include_str!("fixtures/bss-heal-bell.json"));
    replay(include_str!("fixtures/vgc-heal-bell-soundproof.json"));
}

#[test]
fn swagger_boosts_before_confusion_and_magic_powder_sets_psychic_typing() {
    replay(include_str!("fixtures/bss-swagger.json"));
    replay(include_str!("fixtures/bss-magic-powder.json"));
}

#[test]
fn ingrain_grounds_and_anchors_its_user_and_big_root_boosts_recovery() {
    replay(include_str!("fixtures/bss-ingrain-big-root.json"));
    replay(include_str!("fixtures/bss-strength-sap.json"));
}

#[test]
fn gravity_cancels_fly_grounds_targets_boosts_grav_apple_and_expires() {
    replay(include_str!("fixtures/bss-gravity.json"));
}

#[test]
fn steel_roller_requires_and_consumes_terrain() {
    replay(include_str!("fixtures/bss-steel-roller.json"));
}

#[test]
fn trace_starts_once_and_substitute_shields_from_intimidate() {
    replay(include_str!("fixtures/bss-trace-switch.json"));
    replay(include_str!("fixtures/bss-intimidate-substitute.json"));
}

#[test]
fn dire_claw_sleep_has_no_primary_move_suffix_and_fell_stinger_boosts_on_ko() {
    replay(include_str!("fixtures/bss-dire-claw-sleep.json"));
    replay(include_str!("fixtures/bss-fell-stinger.json"));
}

#[test]
fn stat_split_exchanges_raw_stats_until_switch_and_stat_swap_exchanges_only_stages() {
    replay(include_str!("fixtures/bss-stat-split-swap.json"));
}

#[test]
fn wonder_room_swaps_base_defenses_but_uses_special_defense_stages_for_body_press() {
    replay(include_str!("fixtures/bss-wonder-room.json"));
}

#[test]
fn parental_bond_repeats_single_target_moves_with_a_weaker_second_hit() {
    replay(include_str!("fixtures/bss-parental-bond.json"));
    replay(include_str!("fixtures/vgc-parental-bond-spread.json"));
}

#[test]
fn disable_checks_accuracy_before_failing_on_a_target_with_no_last_move() {
    replay(include_str!("fixtures/bss-disable-before-first-move.json"));
}

#[test]
fn sweet_veil_protects_allies_and_supersweet_syrup_triggers_once() {
    replay(include_str!("fixtures/vgc-sweet-veil.json"));
    replay(include_str!("fixtures/bss-supersweet-syrup.json"));
}

#[test]
fn guard_dog_reverses_intimidate_and_blocks_forced_switches() {
    replay(include_str!("fixtures/bss-guard-dog.json"));
}

#[test]
fn baton_pass_transfers_boosts_and_copyable_volatiles_but_not_status_or_salt_cure() {
    replay(include_str!(
        "fixtures/bss-baton-pass-boosts-and-shield.json"
    ));
    replay(include_str!("fixtures/bss-baton-pass-conditions.json"));
}
