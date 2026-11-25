#!/usr/bin/env node
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {arch, availableParallelism, cpus, platform} from 'node:os';
import {join} from 'node:path';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';

const root = fileURLToPath(new URL('..', import.meta.url));
const oracle = join(root, '.tools/poke-rl-simulator');
const revision = '29f2d9969159fd23f8bb96e177a70072fe573b0e';
const format = 'gen9championsbssregmc';
const timeoutMs = 60_000;

function options(args) {
    const split = args.indexOf('--');
    const candidate = split < 0 ? [] : args.slice(split + 1);
    const defaults = {'games': 32, 'batch-size': 16, 'max-turns': 200, 'repeats': 5, 'seed': 1731};
    const {values} = parseArgs({
        args: split < 0 ? args : args.slice(0, split),
        options: {
            help: {type: 'boolean', short: 'h'},
            ...Object.fromEntries(Object.entries(defaults).map(
                ([name, value]) => [name, {type: 'string', default: String(value)}],
            )),
        },
    });
    if (values.help) {
        console.log('Usage: node benchmarks/throughput.mjs [--games 32] [--batch-size 16] ' +
            '[--max-turns 200] [--repeats 5] [--seed 1731] [-- CANDIDATE [ARGS...]]');
        return null;
    }
    assert(split < 0 || candidate.length, 'Pass a worker command after --');
    const settings = {};
    for (const name of Object.keys(defaults)) {
        const value = Number(values[name]);
        assert(Number.isSafeInteger(value) && value > 0, `--${name} must be a positive integer`);
        settings[name] = value;
    }
    assert(settings['batch-size'] <= settings.games, '--batch-size must not exceed --games');
    assert(settings.seed <= 0xffffffff, '--seed must fit in an unsigned 32-bit integer');
    return {settings, candidate};
}

function openWorker([command, ...args]) {
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
                if (reply.error != null) throw new Error(`Worker error for request ${id}: ${reply.error}`);
                assert(Object.hasOwn(reply, 'result'), `Missing result for request ${id}`);
                if (steps) {
                    assert(Array.isArray(reply.result) && reply.result.length === steps,
                        `Expected ${steps} battle steps in reply ${id}`);
                }
                return reply.result;
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

async function record(worker, teams, settings) {
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
    let started = 0;
    let ended = 0;
    let truncated = 0;
    async function capture(request, steps = 0) {
        const entry = frame(frames.length + 1, request, steps);
        const result = await worker.exchange(entry);
        frames.push({...entry, digest: fingerprint(result)});
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
                op: 'step', game: String(slot), actions: active.get(slot).views.map(action),
            })),
        }, slots.length);
        for (const [i, slot] of slots.entries()) {
            const state = states[i];
            if (state.ended || state.truncated) {
                if (state.truncated) truncated++;
                else ended++;
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
    return {frames, ended, truncated, iterations: frames.reduce((sum, entry) => sum + entry.steps, 0)};
}

async function warmAndVerify(worker, frames, name) {
    for (const entry of frames) {
        const actual = fingerprint(await worker.exchange(entry));
        assert.equal(actual, entry.digest, `${name} differs from the reference at request ${entry.id}`);
    }
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
    const {settings, candidate} = config;
    const lock = readFileSync(join(oracle, 'package-lock.json'));
    const teamsFile = readFileSync(join(oracle, 'teams_train.json'));
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
        workload = await record(recorder, teams, settings);
    } finally {
        recorder.close();
    }

    const backends = [{name: 'showdown', worker: openWorker(reference), samples: []}];
    if (candidate.length) backends.push({name: 'candidate', worker: openWorker(candidate), samples: []});
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
        workload_sha256: sha256(workload.frames.map(entry => entry.wire).join('')),
        iterations_per_replay: workload.iterations,
        ended_per_replay: workload.ended,
        truncated_per_replay: workload.truncated,
        results,
        ...(candidate.length && {
            candidate_command: candidate,
            speedup: results.candidate.median_iterations_per_second / results.showdown.median_iterations_per_second,
        }),
    }, null, 2));
}

main().catch(error => {
    console.error(error.code === 'ENOENT' ? `${error.message}\nRun scripts/setup.sh if the reference is missing.` : error);
    process.exitCode = 1;
});
