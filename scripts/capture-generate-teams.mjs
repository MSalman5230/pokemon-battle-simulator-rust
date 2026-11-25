#!/usr/bin/env node
// Record complete teams and the final team PRNG state from the pinned build.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Teams, PRNG} = require('pokemon-showdown');
const format = 'gen9championsrosterranchrumble';
const count = process.argv[3] ? Number(process.argv[3]) : 28;
assert(Number.isSafeInteger(count) && count >= 5);
const seedSource = new PRNG('65535,7,4545,100');
const seeds = ['0,0,0,0', '1,2,3,4', '4,3,2,1', 'sodium,a', 'sodium,131415161718191a1b1c1d1e1f202122',
    ...Array.from({length: count - 5}, () => Array.from({length: 4}, () => seedSource.random(65536)).join(','))];
const cases = seeds.map(seed => {
    const rng = new PRNG(seed);
    const sets = Teams.getGenerator(format, rng).getTeam();
    return {seed, after: rng.getSeed(), sets};
});
const out = process.argv[2] ?? new URL('../tests/fixtures/ranch-generated.json', import.meta.url);
writeFileSync(out, `{"revision":${JSON.stringify(revision)},"format":${JSON.stringify(format)},"cases":[\n${cases.map(row => JSON.stringify(row)).join(',\n')}\n]}\n`);
