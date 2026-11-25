# Showdown throughput

This measures the Poke-RL adapter boundary only. It does not define the
Rust simulator's battle inputs, which follow upstream Showdown.

```sh
source scripts/env.sh
mkdir -p benchmarks/results
node benchmarks/throughput.mjs > benchmarks/results/showdown-bss-reg-mc.json
```

The original backend is Poke-RL's actual `simulator/worker.cjs` using
the locked Showdown build staged by `scripts/setup.sh`. The benchmark
selects seeded teams from Poke-RL's training manifest and records one
deterministic workload using legal action IDs returned by that worker.
It uses one persistent subprocess, batched `step` requests, and private
critic observations, matching the shape of Poke-RL's parallel rollout
traffic. It does not run player models, feature encoding, or training.

- **One iteration** is one completed battle `step` result. A batch's
  iterations are the number of battles stepped, not one per JSON request.
  Preview and forced switches count when they are steps.
- **Iterations/s** includes waiting for the worker, battle processing,
  observations, JSON and pipe traffic, and decoding. It excludes worker
  startup, `new`/cleanup, action selection, and the untimed reference replay.
  The separate games/s metric includes resets and cleanup. Truncations
  are reported and included in that metric.
- The report includes settings, per-run samples, medians, dependency and
  corpus hashes, and a workload hash. Compare the same workload hash on
  the same machine. This is transport-level singles throughput. Poke-RL
  has no equivalent doubles worker yet.

## Compare the Rust worker on the default workload

```sh
source scripts/env.sh
cargo build --locked --release
node scripts/verify-bss-teams.mjs .tools/poke-rl-simulator/teams_train.json \
  .tools/verified-training-corpus.json
node benchmarks/throughput.mjs -- ./target/release/pokemon-battle-worker \
  --verified-teams .tools/verified-training-corpus.json
```

The admission manifest validates rosters offline with the locked reference.
The Rust worker does not use Showdown at runtime. For this test a candidate
must support `new` and `batch` with
`step`/`close` entries from Poke-RL's protocol. It must echo the request
`id` and return the corresponding JSON `result`, in order. Both backends
receive byte-identical lines. A full untimed replay checks replies before
timing, accepting differences only in JSON object-key order. If there is
a mismatch, the harness fails without reporting a speedup.

Run `node benchmarks/throughput.mjs --help` for game count, batch size,
seed, turn cap, and repeat options.

The [recorded default-workload run](verified-default-bss-result.json) used
Rust commit `3b7fd74c3107132feed6398d54fe8575397c2b26`. Both workers matched
all replies in the untimed parity replay, including 673 steps across 32
completed games with no truncations. The median across five timed replays on
the same i5-6300HQ was 337.7 steps/s for Showdown and 1,501.9 for Rust
(4.45×). This verifies the fixed workload, not every team combination, seed,
mode, or worker operation.

## Small checked-in BSS corpus

Without an admission manifest, the experimental worker accepts only the exact
rosters in the checked-in, oracle-validated BSS corpus. This gives a smaller
benchmark without preparing a manifest.

```sh
source scripts/env.sh
cargo build --locked --release
node benchmarks/throughput.mjs --teams-file benchmarks/verified-bss-corpus.json \
  -- ./target/release/pokemon-battle-worker
```

Both backends complete the same replay before any timing is reported. Add
`--debug-parity` to retain reference outputs and print the first differing JSON
path on an observation mismatch. The report hashes the corpus and replay.

In the [recorded subset run](verified-bss-result.json), both workers matched
all responses across 538 steps, 32 completed games, and no truncations. On
the same Intel i5-6300HQ host, the median across five replays was 474.1
steps/s for Showdown and 1,673.5 for Rust (3.53×). This is a result on the
verified subset, not a speed or parity claim about the default Poke-RL corpus.

## Profile adapter overhead

`benchmarks/profile-oracle.mjs` runs the locked worker's `Game` in-process
so a JavaScript CPU profile attributes time to the actual worker functions.
It is diagnostic and excludes JSON-lines pipe traffic. On a 120-game subset
sample, the profiler attributed about 34% of CPU time directly to the worker's
JSON round-trip `clone` helper. The Rust adapter serializes borrowed views
and avoids copying either player's observation tree for the critic.

To capture the original training workload without timing:

```sh
node benchmarks/throughput.mjs --capture-replay benchmarks/results/full-workload.jsonl \
  > benchmarks/results/full-coverage.json
```

The coverage report counts selected moves and the active species, abilities,
items, and Mega choices at selection time. A selected move may still fail or
never execute. This diagnostic keeps the same deterministic workload hash.

Add `--diagnose-parity` before `--` to report the first error or mismatch
for each battle. This runs untimed, isolates games from failures in other
slots, and exits nonzero if any game fails. The timed benchmark still
requires parity on the original batched workload.
