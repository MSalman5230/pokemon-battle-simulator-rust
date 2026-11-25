// In-process profiling workload. This is diagnostic, not the throughput test:
// it attributes CPU time inside the exact locked worker, excluding IPC.
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require = createRequire(new URL('../.tools/poke-rl-simulator/worker.cjs', import.meta.url));
const {Game} = require('./worker.cjs');
const games = Number(process.argv[2] ?? 120);
const {teams} = JSON.parse(readFileSync(new URL('verified-bss-corpus.json', import.meta.url)));
let state = 1731;
const next = n => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return (state >>> 0) % n;
};
let steps = 0;
for (let i = 0; i < games; i++) {
    const game = new Game(teams.map(row => row.team), Array.from({length: 4}, () => next(65536)), 200, true);
    let result = game.result();
    while (!result.ended && !result.truncated) {
        result = game.step(result.views.map(view => {
            if (!view.legal.length) return null;
            const moves = view.legal.filter(c => c.kind === 'move');
            const choices = moves.length && next(10) !== 0 ? moves : view.legal;
            return choices[next(choices.length)].id;
        }));
        steps++;
    }
    game.battle.destroy();
}
console.log(JSON.stringify({games, steps}));
