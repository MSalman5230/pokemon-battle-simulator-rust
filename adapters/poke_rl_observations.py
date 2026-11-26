"""Decode compact or delta replies from pokemon-battle-worker."""

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


class DeltaDecoder:
    """Decode one persistent worker's --delta-observations replies in order.

    Create a new decoder when starting a new worker. Treat all observations as
    read-only, including their nested data, which is shared across replies.
    """

    def __init__(self):
        self._games = {}

    def decode_reply(self, line):
        reply = json.loads(line)
        if "error" in reply:
            # A batch may have advanced several games before an error
            # discarded the whole response. Both ends drop their bases.
            self._games.clear()
            return reply
        result = reply.get("result")
        if isinstance(result, list):
            reply["result"] = [self._expand(entry) for entry in result]
        else:
            reply["result"] = self._expand(result)
        return reply

    def _expand(self, value):
        if not isinstance(value, dict):
            return value
        if "$battle_closed" in value:
            self._games.pop(value["$battle_closed"], None)
            return True
        if "$battle_delta_v1" not in value:
            return value
        key, sequence, patches, (ended, truncated, winner), *seed = value["$battle_delta_v1"]
        if sequence == 0:
            (initial_sets,) = seed
            p1 = _expand_view(patches[0], None, initial_sets[0])
            p2 = _expand_view(patches[1], None, initial_sets[1])
        else:
            base = self._games.get(key)
            if base is None or base[0] != sequence - 1:
                raise ValueError(f"Missing observation base for game {key} at sequence {sequence}")
            p1 = _expand_view(patches[0], base[1][0])
            p2 = _expand_view(patches[1], base[1][1])
        views = [p1, p2]
        self._games[key] = sequence, views
        return {
            "views": views, "ended": ended, "truncated": truncated, "winner": winner,
            "training": {"critic_views": [{**p1, "opponent": p2["own"]}, {**p2, "opponent": p1["own"]}]},
        }


def _patch_slots(changes, previous, own=False, initial_sets=None):
    if previous is None:
        current = changes[1::2]
        if own:
            for mon, original in zip(current, initial_sets):
                mon["set"] = original
        return current
    if not changes:
        return previous
    current = previous.copy()
    for i in range(0, len(changes), 2):
        index = changes[i]
        mon = changes[i + 1]
        if own:
            mon["set"] = previous[index]["set"]
        current[index] = mon
    return current


def _expand_view(patch, previous, initial_sets=None):
    fields, own, opponent, history = patch
    if previous is None:
        return {
            **fields, "own": _patch_slots(own, None, True, initial_sets),
            "opponent": _patch_slots(opponent, None),
            "history": history[1:] if history else [],
        }
    return {
        **previous, **fields,
        "own": _patch_slots(own, previous["own"], True),
        "opponent": _patch_slots(opponent, previous["opponent"]),
        "history": previous["history"][history[0]:] + history[1:] if history else previous["history"],
    }
