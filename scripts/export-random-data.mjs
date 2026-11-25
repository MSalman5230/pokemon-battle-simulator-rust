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

const ids = ['gen9championsrandombattle', 'gen9championsrandomdoublesbattle'];
const seeded = '1,2,3,4';
const recipes = ids.map((id, i) => {
    const generator = Teams.getGenerator(id, seeded);
    const catalog = i ? generator.randomDoublesSets : generator.randomSets;
    return Object.entries(catalog).map(([speciesId, row]) => ({
        id: speciesId, mega: generator.dex.species.get(speciesId).isMega || false, ...row,
    }));
});
const dex = Teams.getGenerator(ids[0], seeded).dex;
const types = dex.types.names();
writeFileSync(new URL('../data/random-champions.json', import.meta.url),
    `{"revision":${JSON.stringify(revision)},"types":${JSON.stringify(types)},"formats":[\n${recipes.map(rows =>
        '[\n' + rows.map(row => JSON.stringify(row)).join(',\n') + '\n]').join(',\n')}\n]}\n`);

const seedSource = new PRNG('45825,7,4545,101');
const seeds = [seeded, '0,0,0,0', 'sodium,a', 'sodium,131415161718191a1b1c1d1e1f202122',
    ...Array.from({length: 24}, () => Array.from({length: 4}, () => seedSource.random(65536)).join(','))];
const cases = ids.flatMap(format => seeds.map(seed => {
    const g = Teams.getGenerator(format, seed);
    const generate = g.randomSet.bind(g);
    const sets = [];
    g.randomSet = (species, context, lead, doubles) => {
        const before = g.prng.getSeed();
        const details = structuredClone(context);
        const set = generate(species, context, lead, doubles);
        sets.push({id: species.id, before, after: g.prng.getSeed(), details, lead, set});
        return set;
    };
    const order = g.getTeam().map(p => p.speciesId);
    return {format, seed, after: g.prng.getSeed(), sets, order};
}));
writeFileSync(new URL('../tests/fixtures/random-assembly.json', import.meta.url),
    `{"revision":${JSON.stringify(revision)},"cases":[\n${cases.map(row => JSON.stringify(row)).join(',\n')}\n]}\n`);
