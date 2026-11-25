#!/usr/bin/env node
// Generate reproducible native singles/doubles continuations for the Rust replay test.
import assert from 'node:assert/strict';
import {closeSync, openSync, readFileSync, writeSync} from 'node:fs';
import {createRequire} from 'node:module';
import {parseArgs} from 'node:util';

const stage = new URL('../.tools/poke-rl-simulator/', import.meta.url);
const require = createRequire(new URL('worker.cjs', stage));
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const lock = JSON.parse(readFileSync(new URL('package-lock.json', stage)));
assert(lock.packages['node_modules/pokemon-showdown'].resolved.endsWith('/' + revision));
const {BattleStream, Dex, PRNG, TeamValidator} = require('pokemon-showdown');
const {extractChannelMessages} = require('pokemon-showdown/dist/sim/battle');

const {values, positionals: [path]} = parseArgs({allowPositionals: true, options: {
    games: {type: 'string', default: '8'},
    'first-game': {type: 'string', default: '1'},
    'max-turns': {type: 'string', default: '30'},
    format: {type: 'string', multiple: true, default: []},
    teams: {type: 'string'},
    'special-actions': {type: 'boolean', default: false},
}});
assert(path, 'Usage: node scripts/capture-wide-native.mjs OUTPUT.jsonl [--games 8] [--first-game 1] [--max-turns 30] [--format FORMAT] [--teams MANIFEST.json] [--special-actions]');
const submitted = values.teams && JSON.parse(readFileSync(values.teams, 'utf8'));
const formats = values.format.length ? values.format : submitted ? Object.keys(submitted) : [
    'gen9championsrandombattle', 'gen9championsrandomdoublesbattle', 'gen9championsrosterranchrumble',
];
assert(formats.length, 'No formats specified');
const games = Number(values.games), limit = Number(values['max-turns']), first = Number(values['first-game']);
for (const value of [games, limit, first]) assert(Number.isSafeInteger(value) && value > 0 && value <= 65535);
assert(first + games - 1 <= 65535, 'Last game must fit in a 16-bit seed word');
const selectableTargets = new Set(['normal', 'any', 'adjacentFoe', 'adjacentAlly', 'adjacentAllyOrSelf']);
const rosters = new Map();
for (const format of formats) {
    const config = Dex.formats.get(format);
    assert(config.exists && format.startsWith('gen9champions'), `Not a Champions format: ${format}`);
    if (config.team) {
        assert(!submitted?.[format], `Cannot submit teams for ${format}`);
        continue;
    }
    const pool = submitted?.[format];
    assert(Array.isArray(pool) && pool.length >= 2, `Provide at least two rosters for ${format}`);
    const validator = new TeamValidator(format);
    rosters.set(format, pool.map((original, index) => {
        const team = structuredClone(original);
        assert(Array.isArray(team), `${format} roster ${index + 1} must be an array of sets`);
        const errors = validator.validateTeam(team);
        assert(!errors, `${format} roster ${index + 1}: ${errors?.join('; ')}`);
        return team;
    }));
}

function choice(side, rng) {
    const request = side.activeRequest;
    if (!request || request.wait) return null;
    const roster = request.side.pokemon;
    if (request.teamPreview) {
        const order = roster.map((_, i) => i + 1);
        rng.shuffle(order);
        return 'team ' + order.slice(0, request.maxChosenTeamSize ?? order.length).join(',');
    }
    const bench = roster.flatMap((p, index) => p.active || p.condition.endsWith('fnt') ? [] : [index + 1]);
    if (request.forceSwitch) {
        rng.shuffle(bench);
        return request.forceSwitch.map((required, slot) => {
            if (!required) return 'pass';
            if (roster[slot]?.reviving) {
                const fainted = roster.flatMap((p, index) =>
                    p.condition.endsWith('fnt') ? [index + 1] : []);
                return fainted.length ? 'switch ' + rng.sample(fainted) : 'pass';
            }
            const next = bench.pop();
            return next === undefined ? 'pass' : 'switch ' + next;
        }).join(', ');
    }
    let megaUsed = false;
    let zMoveUsed = false;
    let ultraUsed = false;
    const choices = request.active.map((active, slot) => {
        const mon = side.active[slot];
        if (!active || mon.fainted) return 'pass';
        if (bench.length && !active.trapped && !active.maybeTrapped && rng.random(20) === 0) {
            return 'switch ' + bench.splice(rng.random(bench.length), 1)[0];
        }
        function targetMove(m, index) {
            let choice = 'move ' + (index + 1);
            if (request.active.length > 1 && selectableTargets.has(m.target)) {
                const possible = [...side.foe.active, ...side.active].filter(p => p && p.hp &&
                    side.battle.validTargetLoc(mon.getLocOf(p), mon, m.target));
                const opponents = possible.filter(p => p.side !== side);
                if (opponents.length && rng.random(8) !== 0) {
                    choice += ' ' + mon.getLocOf(rng.sample(opponents));
                } else if (possible.length) {
                    choice += ' ' + mon.getLocOf(rng.sample(possible));
                } else {
                    return null;
                }
            }
            return choice;
        }
        if (values['special-actions'] && !zMoveUsed && active.canZMove && rng.random(3) === 0) {
            const powered = active.canZMove.map((m, index) => m && targetMove(m, index)).filter(Boolean);
            if (powered.length) {
                zMoveUsed = true;
                return rng.sample(powered) + ' zmove';
            }
        }
        const moves = active.moves.map((m, index) => m.disabled || m.pp === 0 ? null : targetMove(m, index))
            .filter(Boolean);
        if (!moves.length) return null;
        if (values['special-actions'] && !ultraUsed && active.canUltraBurst && rng.random(3) === 0) {
            ultraUsed = true;
            return rng.sample(moves) + ' ultra';
        }
        const mega = active.canMegaEvo && !megaUsed && rng.random(3) === 0;
        megaUsed ||= mega;
        return rng.sample(moves) + (mega ? ' mega' : '');
    });
    return choices.includes(null) ? 'default' : choices.join(', ');
}

async function capture(formatid, n) {
    const seed = [31415, 9265, 35897, n].join(',');
    const rng = new PRNG([321, 626, 459, n]);
    const pool = rosters.get(formatid);
    const pairing = pool && new PRNG([337, 911, 626, n]);
    const firstTeam = pairing?.random(pool.length);
    const otherTeam = pairing?.random(pool.length - 1);
    const teams = pool && [pool[firstTeam], pool[otherTeam + Number(otherTeam >= firstTeam)]];
    const stream = new BattleStream({noCatch: true});
    let logCursor = 0;
    const checkpoints = [];
    function snapshot(lines) {
        const b = stream.battle;
        const newLog = b.log.slice(logCursor).filter(l => !l.startsWith('|t:|'));
        logCursor = b.log.length;
        const channels = extractChannelMessages(newLog.join('\n'), [0, 1, 2]);
        checkpoints.push({
            lines, turn: b.turn, seed: b.prng.getSeed(), requestState: b.requestState,
            waiting: b.sides.map(s => s.isChoiceDone()),
            channels: [0, 1, 2].map(id => channels[id].filter(l => !/^\|uhtml(?:change)?\|otsrequest\|/.test(l))),
            requests: b.sides.map(s => structuredClone(s.activeRequest)),
            teams: b.sides.map(s => s.pokemon.map(p => ({
                name: p.name, species: p.species.name, details: p.details, hp: p.hp, maxhp: p.maxhp,
                status: p.status, stats: {...p.storedStats}, boosts: {...p.boosts}, ability: p.ability,
                item: p.item, active: p.isActive,
                moves: p.moveSlots.map(m => ({id: m.id, pp: m.pp, maxpp: m.maxpp})),
            }))),
        });
    }
    const initial = [
        '>start ' + JSON.stringify({formatid, seed, strictChoices: true}),
        ...[1, 2].map(p => '>player p' + p + ' ' + JSON.stringify({
            name: 'player' + (p - 1), seed: [1361, 533, p, n].join(','),
            ...(teams && {team: teams[p - 1]}),
        })),
    ];
    for (const line of initial) await stream.write(line);
    snapshot(initial);
    const b = stream.battle;
    while (!b.ended && b.turn <= limit) {
        assert(checkpoints.length <= limit * 8 + 16, 'Request loop did not advance');
        const lines = [];
        const choices = b.sides.map(side => choice(side, rng));
        for (const side of b.sides.keys()) {
            if (choices[side] === null) continue;
            const prefix = '>p' + (side + 1) + ' ';
            try {
                await stream.write(prefix + choices[side]);
                lines.push(prefix + choices[side]);
            } catch (error) {
                if (!/^\[(Invalid|Unavailable) choice\]/.test(error.message)) {
                    throw error;
                }
                // Only record the choice Showdown accepts.
                await stream.write(prefix + 'default');
                lines.push(prefix + 'default');
            }
        }
        assert(lines.length, 'No side produced a choice');
        snapshot(lines);
    }
    await stream.writeEnd();
    return {revision, formatid, seed, checkpoints};
}

const out = openSync(path, 'w');
try {
    for (const format of formats) for (let n = first; n < first + games; n++) {
        writeSync(out, JSON.stringify(await capture(format, n)) + '\n');
    }
} finally {
    closeSync(out);
}
