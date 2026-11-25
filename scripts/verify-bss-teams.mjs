// Build an offline admission manifest with the TeamValidator used by Poke-RL.
// This artifact is for parity experiments, not native Rust team validation.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const [input, output] = process.argv.slice(2);
assert(input && output, 'Usage: node scripts/verify-bss-teams.mjs INPUT.json OUTPUT.json');
const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const {TeamValidator} = require('pokemon-showdown');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const format = 'gen9championsbssregmc';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const validator = new TeamValidator(format);
const {teams} = JSON.parse(readFileSync(input));
assert(Array.isArray(teams) && teams.length > 0, 'Expected a manifest with a nonempty teams array');
for (const [i, {team}] of teams.entries()) {
    assert.equal(team.length, 6, 'Expected six team members');
    const errors = validator.validateTeam(structuredClone(team));
    if (errors) throw new Error(`Roster ${i} fails the oracle validator: ${errors.join('; ')}`);
}
writeFileSync(output, JSON.stringify({revision, format, teams}) + '\n');
console.log(`Verified ${teams.length} rosters against ${revision}`);
