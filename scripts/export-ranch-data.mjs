#!/usr/bin/env node
// Project the pinned Roster Ranch catalogue in its RNG-sensitive source order.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {Teams} = require('pokemon-showdown');
const generator = Teams.getGenerator('gen9championsrosterranchrumble', '1,2,3,4');
const dex = generator.dex;
const moves = new Map();
const sets = Object.entries(generator.rrrSets).map(([id, set]) => {
    const species = dex.species.get(id);
    assert(species.exists, id);
    for (const name of [set.flagshipMove, set.secondaryMove, ...(set.otherMoves ?? [])].filter(Boolean)) {
        const move = dex.moves.get(name);
        assert(move.exists, name);
        moves.set(move.id, {
            fixedDamage: !!(move.damage || move.damageCallback),
            sheerForce: !!(move.secondaries || move.hasSheerForceBoost),
            crash: !!move.hasCrashDamage,
            partiallyTrapped: move.volatileStatus === 'partiallytrapped',
        });
    }
    const abilities = Object.values(species.abilities).filter(a => a !== species.abilities.S);
    const megaStones = (dex.species.get(species.baseSpecies).otherFormes || [])
        .map(forme => dex.species.get(forme))
        .filter(mega => !mega.isNonstandard && mega.forme.includes('Mega') && mega.requiredItem &&
            mega.battleOnly === species.name && !/ [XZ]$/.test(mega.requiredItem))
        .map(mega => mega.requiredItem);
    return {id, ...set, abilities, megaStones};
});
const output = {revision, natures: dex.natures.all().map(n => n.name), sets, moves: Object.fromEntries(moves)};
writeFileSync(new URL('../data/roster-ranch.json', import.meta.url), JSON.stringify(output, null, 2) + '\n');
