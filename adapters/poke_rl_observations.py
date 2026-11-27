"""Decode compact or delta replies from pokemon-battle-worker."""

import json
from itertools import permutations


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
        self._fragments = None

    def decode_reply(self, line):
        reply = json.loads(line)
        if "error" in reply:
            # A batch may have advanced several games before an error
            # discarded the whole response. Both ends drop their bases.
            self._games.clear()
            self._fragments = None
            return reply
        if "part" in reply:
            if self._fragments is None:
                self._fragments = reply["id"], {}
            key, rows = self._fragments
            if key != reply["id"]:
                raise ValueError("Mismatched streamed reply ID")
            offset, part = reply["part"]
            for index, entry in enumerate(part, offset):
                if index in rows:
                    raise ValueError("Repeated streamed batch result")
                rows[index] = self._expand(entry)
            return None
        result = reply.get("result")
        if isinstance(result, dict) and "$battle_stream" in result:
            if self._fragments is None:
                raise ValueError("Missing streamed batch")
            key, rows = self._fragments
            self._fragments = None
            count = result["$battle_stream"]
            if key != reply["id"] or len(rows) != count or any(i not in rows for i in range(count)):
                raise ValueError("Missing streamed batch result")
            reply["result"] = [rows[i] for i in range(count)]
            return reply
        if self._fragments is not None:
            raise ValueError("Missing streamed batch completion")
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
        shared = "$battle_delta_v3" in value
        packed = shared or "$battle_delta_v2" in value
        delta = value.get("$battle_delta_v3" if shared else
                          "$battle_delta_v2" if packed else "$battle_delta_v1")
        if delta is None:
            return value
        key, sequence, patches, (ended, truncated, winner), *seed = delta
        base = None
        if sequence != 0:
            base = self._games.get(key)
            if base is None or base[0] != sequence - 1:
                raise ValueError(f"Missing observation base for game {key} at sequence {sequence}")
        sets = seed[0] if seed else (None, None)
        previous = base[1] if base else (None, None)
        refs = shadows = None
        if shared:
            refs = base[2] if base else seed[1]
            previous_own = base[3] if base else (None, None)
            public = [
                _patch_slots(patch[2], view["opponent"] if view else None)
                for patch, view in zip(patches, previous)
            ]
            shadows = [
                _patch_slots(patch[1], shadow, True, original)
                for patch, shadow, original in zip(patches, previous_own, sets)
            ]
            owns = [
                [{**foe[ref], **slot} if ref is not None else slot
                 for ref, slot in zip(indexes, shadow)]
                for indexes, shadow, foe in zip(refs, shadows, reversed(public))
            ]
            p1, p2 = [
                _expand_view(patch, view, packed=True, own_slots=own, opponent_slots=opp)
                for patch, view, own, opp in zip(patches, previous, owns, public)
            ]
        else:
            p1, p2 = [
                _expand_view(patch, view, original, packed)
                for patch, view, original in zip(patches, previous, sets)
            ]
        views = [p1, p2]
        self._games[key] = sequence, views, refs, shadows
        return {
            "views": views, "ended": ended, "truncated": truncated, "winner": winner,
            "training": {"critic_views": [{**p1, "opponent": p2["own"]}, {**p2, "opponent": p1["own"]}]},
        }


def _patch_slots(changes, previous, own=False, initial_sets=None):
    if not changes and previous is not None:
        return previous
    current = [None] * (len(changes) // 2) if previous is None else previous.copy()
    for i in range(0, len(changes), 2):
        index = changes[i]
        mon = changes[i + 1]
        base = None if previous is None else previous[index]
        if isinstance(mon, list):
            present, changed = mon[:2]
            pos = 2
            patch = {}
            split_moves = bool(present & (7 << 29))
            moves = []
            while present:
                bit = present & -present
                present ^= bit
                key = _COLUMNS[bit.bit_length() - 1]
                if changed & bit:
                    value = mon[pos]
                    pos += 1
                    if own and (key in _EXTRA_MOVES or (
                        key == "moves" and value and isinstance(value[0], str)
                    )):
                        move, pp, maxpp, disabled = value
                        moves.append(dict(id=move, pp=pp, maxpp=maxpp, disabled=disabled))
                        continue
                    if own and key == "moves" and value and isinstance(value[0], list):
                        value = [
                            dict(id=id_, pp=pp, maxpp=maxpp, disabled=disabled)
                            for id_, pp, maxpp, disabled in value
                        ]
                    patch[key] = value
                elif own and key in _EXTRA_MOVES:
                    moves.append(base["moves"][_EXTRA_MOVES.index(key) + 1])
                elif own and key == "moves" and (split_moves or (
                    base is not None and "current_hp" in base and 1 <= len(base["moves"]) <= 4
                )):
                    moves.append(base["moves"][0])
                else:
                    patch[key] = base[key]
            if moves:
                patch["moves"] = moves
            mon = patch
        if own:
            mon["set"] = initial_sets[index] if base is None else base["set"]
        current[index] = mon
    return current


_COLUMNS = (
    "base_species", "species", "hp", "status", "boosts", "moves", "item", "ability",
    "seen", "active", "fainted", "types", "volatiles", "last_move", "times_attacked",
    "entry_turn", "moves_this_stint", "pp_used", "first_native_move", "status_progress",
    "identity_uncertain", "nickname", "status_since", "current_hp", "max_hp", "stats",
    "volatile_ids", "index", "selected", "move2", "move3", "move4",
)
_EXTRA_MOVES = _COLUMNS[29:]


def _history(history):
    fields = ("turn", "side", "move", "actor", "species", "order")
    return [dict(zip(fields, row)) if isinstance(row, list) else row for row in history[1:]]


_PREVIEW = [
    {"id": id_, "kind": "preview", "picks": [a, b, c], "choice": f"team {a + 1},{b + 1},{c + 1}"}
    for id_, (a, b, c) in enumerate(permutations(range(6), 3), 14)
]


def _legal(rows):
    if rows == 1:
        return _PREVIEW
    result = []
    for id_, slot, name, extra, *rest in rows:
        if rest:
            result.append(dict(id=id_, kind="switch", slot=slot, species=name,
                               party_slot=extra, revive=rest[0], choice=f"switch {extra + 1}"))
        else:
            choice = f"move {slot + 1}{' mega' if extra else ''}"
            result.append(dict(id=id_, kind="move", slot=slot, move=name, mega=extra, choice=choice))
    return result


def _expand_view(patch, previous, initial_sets=None, packed=False, own_slots=None, opponent_slots=None):
    fields, own, opponent, history = patch
    if packed and "legal" in fields:
        fields["legal"] = _legal(fields["legal"])
    if own_slots is None:
        own_slots = _patch_slots(own, previous["own"] if previous else None, True, initial_sets)
    if opponent_slots is None:
        opponent_slots = _patch_slots(opponent, previous["opponent"] if previous else None)
    past = previous["history"] if previous else []
    return {
        **(previous or {}), **fields,
        "own": own_slots, "opponent": opponent_slots,
        "history": past[history[0]:] + _history(history) if history else past,
    }
