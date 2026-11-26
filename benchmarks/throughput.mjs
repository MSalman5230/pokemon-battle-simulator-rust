#!/usr/bin/env node
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {arch, availableParallelism, cpus, platform} from 'node:os';
import {join} from 'node:path';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';
import {DeltaDecoder} from '../adapters/poke_rl_delta.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const oracle = join(root, '.tools/poke-rl-simulator');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const format = 'gen9championsbssregmc';
const timeoutMs = 60_000;
const compactEncoding = 'shared-rosters-v1';

function options(args) {
    const split = args.indexOf('--');
    const candidate = split < 0 ? [] : args.slice(split + 1);
    const defaults = {'games': 32, 'batch-size': 16, 'max-turns': 200, 'repeats': 5, 'seed': 1731};
    const {values} = parseArgs({
        args: split < 0 ? args : args.slice(0, split),
        options: {
            help: {type: 'boolean', short: 'h'},
            'teams-file': {type: 'string'},
            'debug-parity': {type: 'boolean'},
            'diagnose-parity': {type: 'boolean'},
            'capture-replay': {type: 'string'},
            'trace-parity': {type: 'boolean'},
            'compact-observations': {type: 'boolean'},
            'delta-observations': {type: 'boolean'},
            ...Object.fromEntries(Object.entries(defaults).map(
                ([name, value]) => [name, {type: 'string', default: String(value)}],
            )),
        },
    });
    if (values.help) {
        console.log('Usage: node benchmarks/throughput.mjs [--games 32] [--batch-size 16] ' +
            '[--max-turns 200] [--repeats 5] [--seed 1731] [--teams-file FILE] ' +
            '[--debug-parity | --diagnose-parity | --capture-replay FILE] [--trace-parity] ' +
            '[--compact-observations | --delta-observations] [-- CANDIDATE [ARGS...]]');
        return null;
    }
    assert(split < 0 || candidate.length, 'Pass a worker command after --');
    const diagnose = !!values['diagnose-parity'];
    const traceParity = !!values['trace-parity'];
    const compact = !!values['compact-observations'];
    const delta = !!values['delta-observations'];
    assert(!(delta && compact), 'Choose only one observation encoding');
    assert(!(delta || compact) || candidate.length, 'Observation decoding requires a candidate worker');
    const encoding = delta ? 'battle-delta-v1' : compact ? compactEncoding : undefined;
    assert(!diagnose || (candidate.length && !values['capture-replay']), '--diagnose-parity requires a candidate worker');
    assert(!traceParity || (candidate.length && !diagnose && !values['capture-replay']),
        '--trace-parity requires a candidate worker and the batched replay');
    const settings = {};
    for (const name of Object.keys(defaults)) {
        const value = Number(values[name]);
        assert(Number.isSafeInteger(value) && value > 0, `--${name} must be a positive integer`);
        settings[name] = value;
    }
    assert(settings['batch-size'] <= settings.games, '--batch-size must not exceed --games');
    assert(settings.seed <= 0xffffffff, '--seed must fit in an unsigned 32-bit integer');
    return {settings, candidate, teamsPath: values['teams-file'], debugParity: values['debug-parity'],
        capturePath: values['capture-replay'], diagnose, traceParity, encoding};
}

function expandObservations(result) {
    function expandMon(mon) {
        if (!Array.isArray(mon)) return mon;
        const [species, types] = mon;
        return {
            base_species: species, species, hp: 1, status: '', boosts: {},
            moves: [], item: null, ability: null, seen: false, active: false,
            fainted: false, types, volatiles: {}, last_move: null, times_attacked: 0,
            entry_turn: 0, moves_this_stint: [], pp_used: {}, first_native_move: null,
            status_progress: {}, identity_uncertain: false,
        };
    }
    function expand(game) {
        if (!game?.training) return;
        assert.equal(game.training.critic_views, compactEncoding,
            'Expected compact training observations from the candidate worker');
        const [p1, p2] = game.views;
        p1.opponent = p1.opponent.map(expandMon);
        p2.opponent = p2.opponent.map(expandMon);
        game.training.critic_views = [
            {...p1, opponent: p2.own},
            {...p2, opponent: p1.own},
        ];
    }
    if (Array.isArray(result)) result.forEach(expand);
    else expand(result);
    return result;
}

function openWorker([command, ...args], encoding) {
    const delta = encoding === 'battle-delta-v1' ? new DeltaDecoder() : undefined;
    const child = spawn(command, args, {cwd: root, stdio: ['pipe', 'pipe', 'inherit']});
    const lines = createInterface({input: child.stdout, crlfDelay: Infinity});
    const replies = lines[Symbol.asyncIterator]();
    let failure;
    function onError(error) {
        failure = error;
        lines.close();
    }
    child.on('error', onError);
    child.stdin.on('error', onError);
    return {
        async exchange({id, wire, steps}) {
            if (failure) throw failure;
            let timer;
            try {
                child.stdin.write(wire);
                const {done, value} = await Promise.race([
                    replies.next(),
                    new Promise((_, reject) => {
                        timer = setTimeout(() => reject(new Error(`Worker timed out at request ${id}`)), timeoutMs);
                    }),
                ]);
                if (failure) throw failure;
                assert(!done, `Worker exited before replying to request ${id}`);
                const reply = JSON.parse(value);
                assert.equal(reply.id, id, 'Mismatched worker reply ID');
                if (reply.error != null) {
                    delta?.clear();
                    throw new Error(`Worker error for request ${id}: ${reply.error}`);
                }
                assert(Object.hasOwn(reply, 'result'), `Missing result for request ${id}`);
                if (steps) {
                    assert(Array.isArray(reply.result) && reply.result.length === steps,
                        `Expected ${steps} battle steps in reply ${id}`);
                }
                if (delta) return delta.expand(reply.result);
                return encoding === compactEncoding ? expandObservations(reply.result) : reply.result;
            } finally {
                clearTimeout(timer);
            }
        },
        close() {
            lines.close();
            child.stdin.end();
            if (child.exitCode === null && child.signalCode === null) child.kill();
        },
    };
}

const sha256 = data => createHash('sha256').update(data).digest('hex');
const frame = (id, request, steps = 0) => ({id, steps, wire: JSON.stringify({id, ...request}) + '\n'});
// Ignore JSON object key order. Array order and all values must match.
const fingerprint = result => sha256(JSON.stringify(result, (_, value) =>
    value && typeof value === 'object' && !Array.isArray(value) ?
        Object.fromEntries(Object.keys(value).sort().map(key => [key, value[key]])) : value));

// Native headless traces omit Showdown's wall-clock markers. Every other
// spectator-protocol line is compared in order.
const withoutTime = trace => trace.filter(line => !line.startsWith('|t:|'));

async function record(worker, teams, settings, debugParity, captureCoverage, traceParity) {
    let rng = settings.seed;
    function word() {
        rng ^= rng << 13;
        rng ^= rng >>> 17;
        rng ^= rng << 5;
        return rng >>> 0;
    }
    function action({legal}) {
        if (!legal.length) return null;
        const moves = legal.filter(choice => choice.kind === 'move');
        const pool = moves.length && word() % 10 !== 0 ? moves : legal;
        return pool[word() % pool.length].id;
    }
    const frames = [];
    const coverage = Object.fromEntries(['moves', 'abilities', 'items', 'species', 'megas'].map(k => [k, new Map()]));
    function count(type, name) {
        if (name) coverage[type].set(name, (coverage[type].get(name) ?? 0) + 1);
    }
    function choose(view) {
        const id = action(view);
        if (captureCoverage) {
            const choice = view.legal.find(c => c.id === id);
            const pokemon = view.own.find(mon => mon.active);
            count('abilities', pokemon?.ability);
            count('items', pokemon?.item);
            count('species', pokemon?.species);
            count('moves', choice?.move);
            if (choice?.mega) count('megas', pokemon?.species);
        }
        return id;
    }
    let started = 0;
    let ended = 0;
    let truncated = 0;
    async function capture(request, steps = 0) {
        const entry = frame(frames.length + 1, request, steps);
        const result = await worker.exchange(entry);
        frames.push({...entry, digest: fingerprint(result), ...(debugParity && {expected: result})});
        return result;
    }
    async function reset(slot) {
        started++;
        const a = word() % teams.length;
        const b = (a + 1 + word() % (teams.length - 1)) % teams.length;
        return capture({
            op: 'new', game: String(slot), teams: [teams[a], teams[b]],
            seed: Array.from({length: 4}, () => word() % 65536),
            max_turns: settings['max-turns'], training: true,
        });
    }
    const active = new Map();
    for (let slot = 0; slot < settings['batch-size']; slot++) active.set(slot, await reset(slot));
    const roundLimit = settings.games * (20 * settings['max-turns'] + 32);
    for (let round = 0; active.size; round++) {
        assert(round < roundLimit, 'Reference failed to reach terminal or truncated states');
        const slots = [...active.keys()];
        const states = await capture({
            op: 'batch',
            requests: slots.map(slot => ({
                op: 'step', game: String(slot), actions: active.get(slot).views.map(choose),
            })),
        }, slots.length);
        const stepFrame = frames.at(-1);
        for (const [i, slot] of slots.entries()) {
            const state = states[i];
            if (state.ended || state.truncated) {
                if (state.truncated) truncated++;
                else ended++;
                if (traceParity) {
                    const query = frame(-(ended + truncated), {op: 'trace', game: String(slot)});
                    const expected = withoutTime(await worker.exchange(query));
                    (stepFrame.traces ??= []).push({query, expected});
                }
                if (started < settings.games) active.set(slot, await reset(slot));
                else active.delete(slot);
            } else {
                active.set(slot, state);
            }
        }
    }
    await capture({
        op: 'batch',
        requests: Array.from({length: settings['batch-size']}, (_, slot) => ({op: 'close', game: String(slot)})),
    });
    assert.equal(ended + truncated, settings.games);
    return {frames, ended, truncated, coverage, iterations: frames.reduce((sum, entry) => sum + entry.steps, 0)};
}

function firstDifference(actual, expected, path = []) {
    if (actual === expected) return null;
    if (actual == null || expected == null || typeof actual !== 'object' || typeof expected !== 'object'
            || Array.isArray(actual) !== Array.isArray(expected)) {
        return {path, actual, expected};
    }
    const keys = new Set([...Object.keys(actual), ...Object.keys(expected)]);
    for (const key of keys) {
        const difference = firstDifference(actual[key], expected[key], [...path, key]);
        if (difference) return difference;
    }
    return null;
}

async function warmAndVerify(worker, frames, name) {
    for (const entry of frames) {
        const result = await worker.exchange(entry);
        const actual = fingerprint(result);
        if (actual !== entry.digest && Object.hasOwn(entry, 'expected')) {
            const difference = firstDifference(result, entry.expected);
            throw new Error(JSON.stringify({backend: name, request: JSON.parse(entry.wire), difference}));
        }
        assert.equal(actual, entry.digest, `${name} differs from the reference at request ${entry.id}`);
        for (const {query, expected} of entry.traces ?? []) {
            const actualTrace = withoutTime(await worker.exchange(query));
            const difference = firstDifference(actualTrace, expected);
            if (difference) throw new Error(JSON.stringify({backend: name, request: JSON.parse(query.wire), difference}));
        }
    }
}

// One failing game must not obscure the first difference in unrelated games.
// This is deliberately untimed. The benchmark still verifies original batches.
async function diagnoseParity(worker, workload) {
    const active = new Map();
    const games = [];
    for (const entry of workload.frames) {
        const request = JSON.parse(entry.wire);
        const requests = request.op === 'batch' ? request.requests : [request];
        for (const [i, op] of requests.entries()) {
            if (op.op === 'close') continue;
            if (op.op === 'new') {
                const game = {game: games.length + 1, slot: op.game, first_request: entry.id, verified_steps: 0};
                games.push(game);
                active.set(op.game, game);
            }
            const game = active.get(op.game);
            if (game.failure) continue;
            try {
                const actual = await worker.exchange(frame(entry.id, op));
                const expected = request.op === 'batch' ? entry.expected[i] : entry.expected;
                if (fingerprint(actual) === fingerprint(expected)) {
                    if (op.op === 'step') game.verified_steps++;
                } else {
                    game.failure = {request: entry.id, actions: op.actions, difference: firstDifference(actual, expected)};
                }
            } catch (error) {
                game.failure = {request: entry.id, actions: op.actions, error: error.message};
            }
        }
    }
    console.log(JSON.stringify({
        workload_sha256: sha256(workload.frames.map(entry => entry.wire).join('')),
        passed: games.filter(game => !game.failure).length,
        games,
    }, null, 2));
    if (games.some(game => game.failure)) process.exitCode = 1;
}

async function timeReplay(worker, workload, games) {
    let stepMs = 0;
    let totalMs = 0;
    for (const entry of workload.frames) {
        const start = performance.now();
        await worker.exchange(entry);
        const elapsed = performance.now() - start;
        totalMs += elapsed;
        if (entry.steps) stepMs += elapsed;
    }
    return {
        step_seconds: stepMs / 1000,
        workload_seconds: totalMs / 1000,
        iterations_per_second: 1000 * workload.iterations / stepMs,
        games_per_second_including_truncations: 1000 * games / totalMs,
    };
}

function median(values) {
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

async function main() {
    const config = options(process.argv.slice(2));
    if (!config) return;
    const {settings, candidate, teamsPath, debugParity, capturePath, diagnose, traceParity, encoding} = config;
    const lock = readFileSync(join(oracle, 'package-lock.json'));
    const teamsFile = readFileSync(teamsPath ?? join(oracle, 'teams_train.json'));
    const workerPath = join(oracle, 'worker.cjs');
    const locked = JSON.parse(lock).packages['node_modules/pokemon-showdown'];
    assert(locked?.resolved?.endsWith(`/tar.gz/${revision}`), 'Poke-RL lockfile uses a different Showdown revision');
    const teams = JSON.parse(teamsFile).teams.map(row => row.team);
    assert(teams.length >= 2 && teams.every(team => team.length === 6), 'Expected six-member Poke-RL training rosters');
    const reference = [process.execPath, workerPath];
    const recorder = openWorker(reference);
    let ping, workload;
    try {
        ping = await recorder.exchange(frame(0, {op: 'ping'}));
        assert.equal(ping.format, format);
        assert.equal(ping.revision, revision);
        workload = await record(recorder, teams, settings, debugParity || diagnose, !!capturePath, traceParity);
    } finally {
        recorder.close();
    }

    if (capturePath) {
        assert(!candidate.length, '--capture-replay records the oracle only');
        const coverage = Object.fromEntries(Object.entries(workload.coverage).map(([key, counts]) =>
            [key, Object.fromEntries([...counts].sort((a, b) => b[1] - a[1]))]));
        writeFileSync(capturePath, workload.frames.map(frame => frame.wire).join(''));
        const digest = sha256(workload.frames.map(frame => frame.wire).join(''));
        console.log(JSON.stringify({digest, coverage, iterations: workload.iterations}, null, 2));
        return;
    }
    if (diagnose) {
        const worker = openWorker(candidate, encoding);
        try {
            await diagnoseParity(worker, workload);
        } finally {
            worker.close();
        }
        return;
    }
    const backends = [{name: 'showdown', worker: openWorker(reference), samples: []}];
    if (candidate.length) backends.push({name: 'candidate', worker: openWorker(candidate, encoding), samples: []});
    try {
        for (const backend of backends) await warmAndVerify(backend.worker, workload.frames, backend.name);
        for (let i = 0; i < settings.repeats; i++) {
            // Alternate order so the same backend does not always run first.
            for (const backend of (i % 2 ? [...backends].reverse() : backends)) {
                backend.samples.push(await timeReplay(backend.worker, workload, settings.games));
            }
        }
    } finally {
        for (const {worker} of backends) worker.close();
    }
    const results = Object.fromEntries(backends.map(({name, samples}) => [name, {
        median_iterations_per_second: median(samples.map(sample => sample.iterations_per_second)),
        median_games_per_second_including_truncations: median(samples.map(sample => sample.games_per_second_including_truncations)),
        samples,
    }]));
    console.log(JSON.stringify({
        workload: 'poke-rl-bss-reg-mc-jsonl-v1',
        measured_at_utc: new Date().toISOString(),
        machine: {platform: platform(), arch: arch(), cpu: cpus()[0]?.model, available_cpus: availableParallelism()},
        node: process.version,
        reference: {
            format,
            showdown_revision: revision,
            showdown_integrity: locked.integrity,
            poke_rl_revision: readFileSync(join(oracle, 'poke-rl-revision'), 'utf8').trim(),
            poke_rl_dirty: readFileSync(join(oracle, 'poke-rl-dirty'), 'utf8').trim() === 'true',
            worker_sha256: sha256(readFileSync(workerPath)),
            lock_sha256: sha256(lock),
            teams_sha256: sha256(teamsFile),
        },
        settings: {...settings, training: true},
        ...(teamsPath && {teams_file: teamsPath}),
        workload_sha256: sha256(workload.frames.map(entry => entry.wire).join('')),
        iterations_per_replay: workload.iterations,
        ended_per_replay: workload.ended,
        truncated_per_replay: workload.truncated,
        ...(traceParity && {verified_traces: workload.ended + workload.truncated}),
        results,
        ...(candidate.length && {
            candidate_command: candidate,
            ...(encoding && {candidate_observations: encoding}),
            speedup: results.candidate.median_iterations_per_second / results.showdown.median_iterations_per_second,
        }),
    }, null, 2));
}

main().catch(error => {
    console.error(error.code === 'ENOENT' ? `${error.message}\nRun scripts/setup.sh if the reference is missing.` : error);
    process.exitCode = 1;
});
