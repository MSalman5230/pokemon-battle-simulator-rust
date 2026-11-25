#!/usr/bin/env node
// Capture Poke-RL's versioned classification of set and HP dependencies.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const tacticsSha256 = createHash('sha256').update(readFileSync(new URL('tactics.cjs', stage))).digest('hex');
const metadata = require('./tactics.cjs').metadata();
writeFileSync(new URL('../data/tactics-metadata.json', import.meta.url),
    JSON.stringify({revision, tacticsSha256, metadata}, null, 2) + '\n');
