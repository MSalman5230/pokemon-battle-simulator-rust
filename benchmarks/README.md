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
node benchmarks/throughput.mjs -- ./target/release/pokemon-battle-worker
```

The Rust worker validates rosters itself and does not use Showdown at runtime.
For this test a candidate
must support `new` and `batch` with
`step`/`close` entries from Poke-RL's protocol. It must echo the request
`id` and return the corresponding JSON `result`, in order. Both backends
receive byte-identical lines. A full untimed replay checks replies before
timing, accepting differences only in JSON object-key order. If there is
a mismatch, the harness fails without reporting a speedup.

Run `node benchmarks/throughput.mjs --help` for game count, batch size,
seed, turn cap, and repeat options.

Add `--trace-parity` before `--` to check every terminal game's spectator
trace, excluding only Showdown's wall-clock markers. Trace requests are
verification-only and do not change the timed workload or its hash.
The [extended-mechanics trace run](verified-extended-bss-result.json) verified
all 32 traces and all 673 steps at Rust commit `a60fb32`. It measured 334
steps/s for Showdown and 1,460 for Rust across five replays (4.37×). The
[previous trace run](verified-trace-bss-result.json) measured 4.45× on the
same workload hash.

The [current BSS trace milestone](verified-current-bss-result.json) at
commit `2d9e2b2` matched all 32 traces and all 673 step replies. On the
same workload, the five-replay medians were 325.4 steps/s for Showdown and
1,467.8 for Rust (4.51×).

The [recorded native-validation run](verified-native-bss-result.json) used
Rust commit `70653a09946713dc4593e467e83d3ef6bb6c7a2c`. Both workers matched
all replies in the untimed parity replay, including 673 steps across 32
completed games with no truncations. The median across five timed replays on
the same i5-6300HQ was 342.5 steps/s for Showdown and 1,526.0 for Rust
(4.46×). This verifies the fixed workload, not every team combination, seed,
mode, or worker operation.

To reproduce the [earlier run's](verified-default-bss-result.json) external
admission mode, generate the pinned
manifest with `node scripts/verify-bss-teams.mjs .tools/poke-rl-simulator/teams_train.json .tools/verified-training-corpus.json`
and append `--verified-teams .tools/verified-training-corpus.json` to the worker
command after `--`. In that mode, only exact rosters from the manifest are admitted.

## Small checked-in BSS corpus

The checked-in, oracle-validated BSS corpus provides a smaller benchmark.

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

## Native generated-team replay

Use the native simulator contract to sample longer continuations in Random
Battle, Random Doubles Battle, and Roster Ranch Rumble. This diagnostic is
separate from the timed Poke-RL workload.

```sh
source scripts/env.sh
node scripts/capture-wide-native.mjs .tools/wide-native.jsonl --games 8 --max-turns 30
cargo test --locked --test replay wide_generated -- --ignored --nocapture
```

Each game uses explicit, separate battle, roster, and choice seeds. The test
checks every recorded public and private log, each side's current request,
the party state, and the battle RNG, even when the replay reaches its turn
cap. It reports the first mismatch in each game. Use
`CHAMPIONS_NATIVE_CORPUS` to select a different generated corpus.

The [recorded 24-game sample](verified-native-generated-parity.json) matches
all 574 checkpoints. Eighteen games ended naturally. The other six reached
the 30-turn cap with exact results through their last recorded checkpoint.
This is a native parity sample and does not measure throughput or establish
coverage of every move, ability, or item.

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
The [160-game diagnostic replay](verified-wide-bss-parity.json) with seed
`20261125` matched all 2,856 step replies end to end at commit `2d9e2b2`.
This was untimed and did not compare spectator traces. It does not
establish parity on other seeds, rosters, or modes.
