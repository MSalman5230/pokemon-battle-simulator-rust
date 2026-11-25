#!/usr/bin/env node
// Export static data only. Never serialize or execute JS battle handlers in Rust.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Dex} = require('pokemon-showdown');
const root = new URL('../', import.meta.url);
const sorted = entries => Object.fromEntries(entries.sort(([a], [b]) => a.localeCompare(b, 'en')));
const table = (entries, project) => sorted(entries.map(e => [e.id, project(e)]));
const props = (e, keys) => Object.fromEntries(keys.map(key => [key, e[key]]));
const handlers = e => Object.keys(e).filter(k =>
    typeof e[k] === 'function' || /^on[A-Z]/.test(k)).sort();

const data = mod => {
    const dex = Dex.mod(mod);
    return {
        species: table(dex.species.all(), e => {
            const packedSlots = Dex.species.get(e.name).abilities;
            return {
            ...props(e, ['name', 'baseSpecies', 'types', 'baseStats', 'gender', 'abilities',
                'isNonstandard', 'maxHP', 'requiredItem']),
            hasCondition: dex.conditions.getByID(e.id).exists,
            unreleasedHidden: !!e.unreleasedHidden,
            packedAbilitySlots: JSON.stringify(packedSlots) === JSON.stringify(e.abilities) ? undefined : packedSlots,
            };
        }),
        moves: table(dex.moves.all(), e => {
            const move = props(e, ['name', 'type', 'category', 'target', 'basePower', 'priority',
                'pp', 'noPPBoosts', 'accuracy', 'critRatio', 'flags', 'isNonstandard', 'heal', 'status', 'ignoreImmunity']);
            return {...move, handlers: handlers(e), secondary: !!e.secondary || !!e.secondaries?.length,
                self: !!e.self, condition: !!e.condition, boosts: Object.entries(e.boosts ?? {}),
                selfBoosts: Object.entries(e.self?.boosts ?? {}), selfChance: e.self?.chance,
                secondaries: (e.secondaries ?? []).map(s => ({chance: s.chance, status: s.status,
                    volatileStatus: s.volatileStatus, boosts: Object.entries(s.boosts ?? {})}))};
        }),
        abilities: table(dex.abilities.all(), e =>
            ({...props(e, ['name', 'isNonstandard']), handlers: handlers(e)})),
        items: table(dex.items.all(), e =>
            ({...props(e, ['name', 'isNonstandard', 'megaStone']), handlers: handlers(e)})),
        natures: table(dex.natures.all(), e => props(e, ['plus', 'minus'])),
        types: table(dex.types.all(), e => props(e, ['damageTaken'])),
        aliases: sorted([...Dex.loadAliases()]),
    };
};
const main = data('champions');
const mb = data('championsregmb');
// Store different records only. Some Reg M-B moves change PP, not just legality.
const regmb = Object.fromEntries(Object.entries(mb).map(([key, records]) => {
    assert.deepEqual(Object.keys(records), Object.keys(main[key]));
    return [key, Object.fromEntries(Object.entries(records)
        .filter(([id, value]) => JSON.stringify(value) !== JSON.stringify(main[key][id])))];
}).filter(([, records]) => Object.keys(records).length));

const formats = {};
for (const f of Dex.formats.all().filter(f => /^\[Gen 9 Champions\]/.test(f.name))) {
    const r = Dex.formats.getRuleTable(f);
    formats[f.id] = {
        name: f.name, mod: f.mod, gameType: f.gameType, generator: f.team || null,
        rules: [...r.keys()], minTeamSize: r.minTeamSize, maxTeamSize: r.maxTeamSize,
        maxMoveCount: r.maxMoveCount, pickedTeamSize: r.pickedTeamSize, defaultLevel: r.defaultLevel,
        adjustLevel: r.adjustLevel, adjustLevelDown: r.adjustLevelDown, maxLevel: r.maxLevel,
        unboundedTruncation: !!f.battle?.trunc,
    };
}
function recordLines(obj) {
    return '{\n' + Object.entries(obj).map(([key, value]) => JSON.stringify(key) + ': ' +
        (typeof value === 'object' && !Array.isArray(value) ?
            (key === 'regmb' ? recordLines(value) : '{\n' + Object.entries(value)
                .map(([id, row]) => JSON.stringify(id) + ': ' + JSON.stringify(row)).join(',\n') + '\n}') :
            JSON.stringify(value))).join(',\n') + '\n}';
}
writeFileSync(new URL('data/champions.json', root), recordLines({revision, formats, ...main, regmb}) + '\n');
