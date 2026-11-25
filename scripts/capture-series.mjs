#!/usr/bin/env node
// Exercise only Showdown's series scoring callback, with no running server.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {BestOfGame} = require(fileURLToPath(new URL('node_modules/pokemon-showdown/dist/server/room-battle-bestof.js', stage)));
const output = [];
for (const g1 of ['p1', 'p2', '']) for (const g2 of ['p1', 'p2', '']) for (const g3 of ['p1', 'p2', '']) {
    const players = [1, 2].map(num => ({num, id: 'p' + num, name: 'player' + num, wins: 0}));
    const series = {bestOf: 3, winThreshold: 2, ties: 0, ended: false, games: [], players,
        playerTable: Object.fromEntries(players.map(player => [player.id, player])),
        room: {add: () => ({update() {}})}, promptNextGame() {},
        end(winner) {this.ended = true; this.winner = winner || null;}};
    const decisions = [];
    for (const winner of [g1, g2, g3]) {
        series.games.push({});
        BestOfGame.prototype.onBattleWin.call(series, {battle: {players: []}}, winner);
        decisions.push({result: winner || null, wins: players.map(p => p.wins), ties: series.ties,
            win_threshold: series.winThreshold, ended: series.ended, winner: series.winner ?? null});
        if (series.ended) break;
    }
    output.push(decisions);
}
writeFileSync(new URL('../tests/fixtures/best-of-three.json', import.meta.url), `{"revision":${JSON.stringify(revision)},"series":[\n${output.map(row => JSON.stringify(row)).join(',\n')}\n]}\n`);
