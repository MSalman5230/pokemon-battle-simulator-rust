"""Decode replies from pokemon-battle-worker --compact-observations."""

import json


def _expand_opponent(mon):
    if not isinstance(mon, list):
        return mon
    species, types = mon
    return {
        "base_species": species, "species": species, "hp": 1, "status": "",
        "boosts": {}, "moves": [], "item": None, "ability": None,
        "seen": False, "active": False, "fainted": False, "types": types,
        "volatiles": {}, "last_move": None, "times_attacked": 0, "entry_turn": 0,
        "moves_this_stint": [], "pp_used": {}, "first_native_move": None,
        "status_progress": {}, "identity_uncertain": False,
    }


def _expand(game):
    if not isinstance(game, dict):
        return
    training = game.get("training")
    if not isinstance(training, dict) or training.get("critic_views") != "shared-rosters-v1":
        return
    p1, p2 = game["views"]
    for view in (p1, p2):
        view["opponent"] = [_expand_opponent(mon) for mon in view["opponent"]]
    training["critic_views"] = [
        {**p1, "opponent": p2["own"]},
        {**p2, "opponent": p1["own"]},
    ]


def decode_reply(line):
    """Drop-in replacement for json.loads on worker replies.

    Treat decoded observations as read-only. A critic and player view share
    nested fields, and the critic's opponent roster is the other player's own.
    Ordinary replies and errors pass through unchanged.
    """
    reply = json.loads(line)
    result = reply.get("result")
    if isinstance(result, list):
        for entry in result:
            _expand(entry)
    else:
        _expand(result)
    return reply
