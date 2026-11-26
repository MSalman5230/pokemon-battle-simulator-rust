// Decodes one persistent JSON-lines connection. Observations are read-only,
// including their nested data, which is shared across views and replies.
export class DeltaDecoder {
    #games = new Map();

    clear() {
        this.#games.clear();
    }

    expand(result) {
        return Array.isArray(result) ? result.map(game => this.#expand(game)) : this.#expand(result);
    }

    #expand(value) {
        if (value?.$battle_closed !== undefined) {
            this.#games.delete(value.$battle_closed);
            return true;
        }
        if (!value?.$battle_delta_v1) return value;
        const [key, sequence, patches, [ended, truncated, winner], initialSets] = value.$battle_delta_v1;
        const previous = sequence === 0 ? undefined : this.#games.get(key);
        if (sequence !== 0 && previous?.sequence !== sequence - 1) {
            throw new Error(`Missing observation base for game ${key} at sequence ${sequence}`);
        }
        const p1 = expandView(patches[0], previous?.views[0], initialSets?.[0]);
        const p2 = expandView(patches[1], previous?.views[1], initialSets?.[1]);
        const views = [p1, p2];
        this.#games.set(key, {sequence, views});
        return {
            views, ended, truncated, winner,
            training: {critic_views: [{...p1, opponent: p2.own}, {...p2, opponent: p1.own}]},
        };
    }
}

function slots(changes, previous, own = false, initialSets) {
    if (changes.length === 0 && previous) return previous;
    const current = previous?.slice() ?? [];
    for (let i = 0; i < changes.length; i += 2) {
        const index = changes[i];
        const mon = changes[i + 1];
        if (own) mon.set = initialSets ? initialSets[index] : previous[index].set;
        current[index] = mon;
    }
    return current;
}

function expandView([fields, own, opponent, history], previous, initialSets) {
    const result = Object.assign({}, previous, fields);
    result.own = slots(own, previous?.own, true, initialSets);
    result.opponent = slots(opponent, previous?.opponent);
    const past = previous?.history ?? [];
    if (history) {
        const current = past.slice(history[0]);
        for (let i = 1; i < history.length; i++) current.push(history[i]);
        result.history = current;
    } else {
        result.history = past;
    }
    return result;
}
