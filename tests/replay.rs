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
