#!/usr/bin/env node
// Project ordered Random Battle set recipes and capture the team/set boundary.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {PRNG, Teams} = require('pokemon-showdown');
const output = process.argv[2] ?? new URL('../tests/fixtures/random-assembly.json', import.meta.url);
const count = process.argv[3] ? Number(process.argv[3]) : 28;
assert(Number.isSafeInteger(count) && count >= 4);

const ids = ['gen9championsrandombattle', 'gen9championsrandomdoublesbattle'];
const seeded = '1,2,3,4';
const moves = new Map();
function moveId(dex, name) {
    const move = dex.moves.get(name);
    assert(move.exists, name);
    moves.set(move.id, {
        fixedDamage: !!(move.damage || move.damageCallback),
        sheerForce: !!(move.secondaries || move.hasSheerForceBoost),
        crash: !!move.hasCrashDamage,
    });
    return move.id;
}
const recipes = ids.map((id, i) => {
    const generator = Teams.getGenerator(id, seeded);
    const catalog = i ? generator.randomDoublesSets : generator.randomSets;
    return Object.entries(catalog).map(([speciesId, row]) => {
        const species = generator.dex.species.get(speciesId);
        return {
            id: speciesId, mega: !!species.isMega, level: row.level,
            battleForm: typeof species.battleOnly === 'string' ? species.battleOnly : null,
            cosmetics: species.cosmeticFormes, otherFormes: species.otherFormes,
            moveAbilities: species.battleOnly && !species.requiredAbility ? Object.values(species.abilities) : null,
            requiredMove: species.requiredMove ? moveId(generator.dex, species.requiredMove) : null,
            requiredItems: species.requiredItems,
            sets: row.sets.map(set => ({...set, movepool: set.movepool.map(name => moveId(generator.dex, name))})),
        };
    });
});
const dex = Teams.getGenerator(ids[0], seeded).dex;
moveId(dex, 'Tri Attack');
const types = dex.types.names();
const statusMoves = dex.moves.all().filter(move => move.category === 'Status').map(move => move.id);
writeFileSync(new URL('../data/random-champions.json', import.meta.url),
    `{"revision":${JSON.stringify(revision)},"types":${JSON.stringify(types)},"statusMoves":${JSON.stringify(statusMoves)},"moves":${JSON.stringify(Object.fromEntries(moves))},"formats":[\n${recipes.map(rows =>
        '[\n' + rows.map(row => JSON.stringify(row)).join(',\n') + '\n]').join(',\n')}\n]}\n`);

const seedSource = new PRNG('45825,7,4545,101');
const seeds = [seeded, '0,0,0,0', 'sodium,a', 'sodium,131415161718191a1b1c1d1e1f202122',
    ...Array.from({length: count - 4}, () => Array.from({length: 4}, () => seedSource.random(65536)).join(','))];
const cases = ids.flatMap(format => seeds.map(seed => {
    const g = Teams.getGenerator(format, seed);
    const generate = g.randomSet.bind(g);
    const chooseMoves = g.randomMoveset.bind(g);
    const sets = [];
    let moveset;
    g.randomMoveset = (types, abilities, details, species, lead, pool, preferred, role, doubles) => {
        const before = g.prng.getSeed();
        const input = [...pool];
        const output = chooseMoves(types, abilities, details, species, lead, pool, preferred, role, doubles);
        moveset = {before, after: g.prng.getSeed(), input, moves: [...output], preferred, role, abilities};
        return output;
    };
    g.randomSet = (species, context, lead, doubles) => {
        const before = g.prng.getSeed();
        const details = structuredClone(context);
        const set = generate(species, context, lead, doubles);
        sets.push({id: species.id, before, after: g.prng.getSeed(), details, lead, set, moveset});
        return set;
    };
    const order = g.getTeam().map(p => p.speciesId);
    return {format, seed, after: g.prng.getSeed(), sets, order};
}));
writeFileSync(output,
    `{"revision":${JSON.stringify(revision)},"cases":[\n${cases.map(row => JSON.stringify(row)).join(',\n')}\n]}\n`);
