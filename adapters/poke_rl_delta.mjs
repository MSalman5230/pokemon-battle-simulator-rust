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
        const delta = value?.$battle_delta_v2 ?? value?.$battle_delta_v1;
        if (!delta) return value;
        const [key, sequence, patches, [ended, truncated, winner], initialSets] = delta;
        const previous = sequence === 0 ? undefined : this.#games.get(key);
        if (sequence !== 0 && previous?.sequence !== sequence - 1) {
            throw new Error(`Missing observation base for game ${key} at sequence ${sequence}`);
        }
        const packed = !!value.$battle_delta_v2;
        const p1 = expandView(patches[0], previous?.views[0], initialSets?.[0], packed);
        const p2 = expandView(patches[1], previous?.views[1], initialSets?.[1], packed);
        const views = [p1, p2];
        this.#games.set(key, {sequence, views});
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
const monData = new WeakMap();

function unpackMon(row, previous, own, set) {
    const mask = row[0];
    let changed = row[1];
    // The column buffer is internal. Only the object materialized below is
    // shared with a caller, so we can reuse this buffer without mutating views.
    const values = previous ? monData.get(previous) : new Array(columns.length);
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
    let make = table.get(objectMask);
    if (!make) {
        const fields = [];
        for (let i = 0; i < 29; ++i) {
            if (mask & (1 << i)) fields.push(`${columns[i]}:a[${own && i === 5 ? 32 : i}]`);
        }
        if (own) fields.push('set:set');
        make = new Function('a', 'set', `return {${fields.join(',')}}`);
        table.set(objectMask, make);
    }
    const mon = make(values, set);
    monData.set(mon, values);
    return mon;
}

function slots(changes, previous, own = false, initialSets) {
    if (changes.length === 0 && previous) return previous;
    const current = previous?.slice() ?? [];
    for (let i = 0; i < changes.length; i += 2) {
        const index = changes[i];
        const value = changes[i + 1];
        const set = own ? (initialSets ? initialSets[index] : previous[index].set) : undefined;
        const packed = Array.isArray(value);
        const mon = packed ? unpackMon(value, previous?.[index], own, set) : value;
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

function expandView([fields, own, opponent, history], previous, initialSets, packed) {
    if (packed && fields.legal !== undefined) fields.legal = unpackLegal(fields.legal);
    const ownSlots = slots(own, previous?.own, true, initialSets);
    const opponentSlots = slots(opponent, previous?.opponent);
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
