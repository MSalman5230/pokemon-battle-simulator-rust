#!/usr/bin/env node
// Filter a Poke-RL roster pool for each Champions format's native replay.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const [input, output, ...selected] = process.argv.slice(2);
assert(input && output, 'Usage: node scripts/prepare-native-rosters.mjs POKE-RL.json OUTPUT.json [FORMAT ...]');
const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const {Dex, TeamValidator} = require('pokemon-showdown');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));

const {teams} = JSON.parse(readFileSync(input, 'utf8'));
assert(Array.isArray(teams) && teams.length >= 2, 'Expected a Poke-RL roster pool');
const formats = selected.length ? selected : Dex.formats.all().filter(format =>
    format.id.startsWith('gen9champions') && !format.team).map(format => format.id);
const pools = Object.fromEntries(formats.map(format => {
    const config = Dex.formats.get(format);
    assert(config.exists && format.startsWith('gen9champions') && !config.team,
        `Not a submitted-team Champions format: ${format}`);
    const validator = new TeamValidator(format);
    const pool = teams.map(({team}) => team).filter(team => !validator.validateTeam(structuredClone(team)));
    assert(pool.length >= 2, `Found fewer than two admissible rosters for ${format}`);
    console.log(`${format} ${pool.length}/${teams.length} rosters`);
    return [format, pool];
}));
writeFileSync(output, JSON.stringify(pools) + '\n');
