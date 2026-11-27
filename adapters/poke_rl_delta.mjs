// Decodes one persistent JSON-lines connection. Observations are read-only,
// including their nested data, which is shared across views and replies.
export class DeltaDecoder {
    #games = new Map();

    clear() {
        this.#games.clear();
    }

    expand(result) {
        if (!Array.isArray(result)) return this.#expand(result);
        for (let i = 0; i < result.length; i++) result[i] = this.#expand(result[i]);
        return result;
    }

    #expand(value) {
        if (value?.$battle_closed !== undefined) {
            this.#games.delete(value.$battle_closed);
            return true;
        }
        const shared = !!value?.$battle_delta_v3;
        const delta = value?.$battle_delta_v3 ?? value?.$battle_delta_v2 ?? value?.$battle_delta_v1;
        if (!delta) return value;
        const [key, sequence, patches, [ended, truncated, winner], initialSets, initialRefs] = delta;
        const previous = sequence === 0 ? undefined : this.#games.get(key);
        if (sequence !== 0 && previous?.sequence !== sequence - 1) {
            throw new Error(`Missing observation base for game ${key} at sequence ${sequence}`);
        }
        const packed = shared || !!value.$battle_delta_v2;
        const refs = shared ? initialRefs ?? previous?.refs : undefined;
        const state = previous?.state ?? [[[], []], [[], []]];
        let p1, p2;
        if (shared) {
            const public1 = slots(patches[0][2], previous?.views[0].opponent, state[0][1]);
            const public2 = slots(patches[1][2], previous?.views[1].opponent, state[1][1]);
            const own1 = sharedOwn(patches[0][1], previous?.views[0].own, initialSets?.[0],
                refs[0], public2, previous?.views[1].opponent, state[0][0], state[1][1]);
            const own2 = sharedOwn(patches[1][1], previous?.views[1].own, initialSets?.[1],
                refs[1], public1, previous?.views[0].opponent, state[1][0], state[0][1]);
            p1 = expandView(patches[0], previous?.views[0], initialSets?.[0], true, state[0], own1, public1);
            p2 = expandView(patches[1], previous?.views[1], initialSets?.[1], true, state[1], own2, public2);
        } else {
            p1 = expandView(patches[0], previous?.views[0], initialSets?.[0], packed, state[0]);
            p2 = expandView(patches[1], previous?.views[1], initialSets?.[1], packed, state[1]);
        }
        const views = [p1, p2];
        this.#games.set(key, {sequence, views, refs, state});
        const critic = (view, opponent) => packed
            ? packedView(view, empty, view.own, opponent, view.history)
            : {...view, opponent};
        return {
            views, ended, truncated, winner,
            training: {critic_views: [critic(p1, p2.own), critic(p2, p1.own)]},
        };
    }
}

// Column IDs form part of the v2 wire contract. Keep in sync with the
// Rust MON_FIELDS/EXTRA_FIELDS and the Python decoder.
const columns = [
    'base_species', 'species', 'hp', 'status', 'boosts', 'moves', 'item', 'ability',
    'seen', 'active', 'fainted', 'types', 'volatiles', 'last_move', 'times_attacked',
    'entry_turn', 'moves_this_stint', 'pp_used', 'first_native_move', 'status_progress',
    'identity_uncertain', 'nickname', 'status_since', 'current_hp', 'max_hp', 'stats',
    'volatile_ids', 'index', 'selected', 'move2', 'move3', 'move4',
];
const shapes = [new Map(), new Map()];
const shapeBase = 2 ** 29;

function unpackMon(row, values, own, set, base, baseMask) {
    // Each game/side/slot has a reusable column buffer. It is never exposed
    // in the materialized observation, which remains unchanged across replies.
    const mask = row ? row[0] : values[33];
    let changed = row ? row[1] : 0;
    const movesMask = 0xe0000020;
    const refreshMoves = own && ((changed & movesMask) || ((values[33] ^ mask) & movesMask));
    for (let i = 2; changed !== 0; ++i) {
        const bit = changed & -changed;
        const column = 31 - Math.clz32(bit);
        const value = row[i];
        if (own && (column >= 29 || (column === 5 && typeof value?.[0] === 'string'))) {
            const [id, pp, maxpp, disabled] = value;
            values[column] = {id, pp, maxpp, disabled};
        } else if (own && column === 5 && Array.isArray(value?.[0])) {
            values[column] = value.map(([id, pp, maxpp, disabled]) => ({id, pp, maxpp, disabled}));
        } else {
            values[column] = value;
        }
        changed ^= bit;
    }
    if (refreshMoves) {
        if (Array.isArray(values[5])) {
            values[32] = values[5];
        } else {
            const moves = [values[5]];
            for (let i = 29; i < 32; ++i) if (mask & (1 << i)) moves.push(values[i]);
            values[32] = moves;
        }
    }
    values[33] = mask;
    const table = shapes[+own];
    const objectMask = mask & 0x1fffffff;
    // Public rows only use the first 23 columns, so the two masks
    // together fit exactly in a JavaScript integer (29 + 23 bits).
    const publicOnly = baseMask & ~objectMask & 0x7fffff;
    const key = objectMask + shapeBase * publicOnly;
    let make = table.get(key);
    if (!make) {
        const fields = [];
        for (let i = 0; i < 29; ++i) {
            if (mask & (1 << i)) fields.push(`${columns[i]}:a[${own && i === 5 ? 32 : i}]`);
            else if (publicOnly & (1 << i)) fields.push(`${columns[i]}:b.${columns[i]}`);
        }
        if (own) fields.push('set:set');
        make = new Function('a', 'set', 'b', `return {${fields.join(',')}}`);
        table.set(key, make);
    }
    return make(values, set, base);
}

function sharedOwn(changes, previous, sets, refs, publicSlots, previousPublic, state, publicState) {
    let slots = previous ?? new Array(refs.length);
    let change = 0;
    for (let i = 0; i < refs.length; ++i) {
        const ref = refs[i];
        const base = ref === null ? undefined : publicSlots[ref];
        let row;
        if (changes[change] === i) {
            row = changes[change + 1];
            change += 2;
        }
        if (!row && (ref === null || base === previousPublic?.[ref])) continue;
        if (slots === previous) slots = previous.slice();
        slots[i] = unpackMon(row, state[i] ??= new Array(columns.length), true,
            sets ? sets[i] : previous[i].set, base, ref === null ? 0 : publicState[ref][33]);
    }
    return slots;
}

function slots(changes, previous, state, own = false, initialSets) {
    if (changes.length === 0 && previous) return previous;
    const current = previous?.slice() ?? [];
    for (let i = 0; i < changes.length; i += 2) {
        const index = changes[i];
        const value = changes[i + 1];
        const set = own ? (initialSets ? initialSets[index] : previous[index].set) : undefined;
        const packed = Array.isArray(value);
        const mon = packed ? unpackMon(value, state[index] ??= new Array(columns.length), own, set, undefined, 0) : value;
        if (own && !packed) mon.set = set;
        current[index] = mon;
    }
    return current;
}

let preview;
const moveChoices = ['move 1', 'move 2', 'move 3', 'move 4'];
const megaChoices = moveChoices.map(choice => `${choice} mega`);
const switchChoices = ['switch 1', 'switch 2', 'switch 3', 'switch 4', 'switch 5', 'switch 6'];
const previewLegal = () => {
    const rows = [];
    for (let a = 0; a < 6; ++a) for (let b = 0; b < 6; ++b) for (let c = 0; c < 6; ++c) {
        if (a !== b && a !== c && b !== c) {
            rows.push({id: rows.length + 14, kind: 'preview', picks: [a, b, c],
                choice: `team ${a + 1},${b + 1},${c + 1}`});
        }
    }
    return rows;
};
function unpackLegal(rows) {
    if (rows === 1) return preview ??= previewLegal();
    for (let i = 0; i < rows.length; i++) {
        const [id, slot, name, extra, revive] = rows[i];
        rows[i] = revive === undefined
            ? {id, kind: 'move', slot, move: name, mega: extra,
                choice: (extra ? megaChoices : moveChoices)[slot] ?? `move ${slot + 1}${extra ? ' mega' : ''}`}
            : {id, kind: 'switch', slot, species: name, party_slot: extra, revive,
                choice: switchChoices[extra] ?? `switch ${extra + 1}`};
    }
    return rows;
}

const empty = {};
function packedView(base, patch, own, opponent, history) {
    return {
        format: patch.format ?? base.format,
        turn: patch.turn ?? base.turn,
        side: patch.side ?? base.side,
        phase: patch.phase ?? base.phase,
        own, opponent,
        legal: patch.legal ?? base.legal,
        weather: patch.weather ?? base.weather,
        terrain: patch.terrain ?? base.terrain,
        pseudo: patch.pseudo ?? base.pseudo,
        weather_since: patch.weather_since ?? base.weather_since,
        terrain_since: patch.terrain_since ?? base.terrain_since,
        conditions: patch.conditions ?? base.conditions,
        mega: patch.mega ?? base.mega,
        history,
        faints: patch.faints ?? base.faints,
        ended: patch.ended ?? base.ended,
        truncated: patch.truncated ?? base.truncated,
    };
}

function expandView([fields, own, opponent, history], previous, initialSets, packed, state, sharedOwnSlots, sharedPublicSlots) {
    if (packed && fields.legal !== undefined) fields.legal = unpackLegal(fields.legal);
    const ownSlots = sharedOwnSlots ?? slots(own, previous?.own, state[0], true, initialSets);
    const opponentSlots = sharedPublicSlots ?? slots(opponent, previous?.opponent, state[1]);
    const past = previous?.history ?? [];
    let current = past;
    if (history) {
        current = past.slice(history[0]);
        for (let i = 1; i < history.length; i++) {
            const event = history[i];
            current.push(Array.isArray(event) ? {
                turn: event[0], side: event[1], move: event[2], actor: event[3], species: event[4], order: event[5],
            } : event);
        }
    }
    if (packed) return packedView(previous ?? fields, fields, ownSlots, opponentSlots, current);
    return Object.assign({}, previous, fields, {own: ownSlots, opponent: opponentSlots, history: current});
}
