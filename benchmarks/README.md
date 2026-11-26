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

## Compact training observations

```sh
source scripts/env.sh
cargo build --locked --release
node benchmarks/throughput.mjs --trace-parity --compact-observations -- \
  ./target/release/pokemon-battle-worker --compact-observations
```

Both workers still receive byte-identical requests on the default workload.
The compact candidate sends the two player views once. The benchmark
reconstructs the two critic views and expands unrevealed public roster slots
before checking equality. Timed runs include that decoding. The default
worker still returns full JSON observations without requiring a new decoder.

At Rust commit `9c9c585`, the [compact release run](verified-compact-bss-result.json)
measured 2,980.2 steps/s against Showdown's 343.7 across seven replays (8.67×).
The [full JSON run](verified-cached-bss-result.json) measured 1,685.9 steps/s
against 342.5 (4.92×). Both runs used the same workload hash as the original
release benchmark and matched all 673 step replies and 32 complete traces.
Compact step replies used 63% fewer bytes than full Rust replies. A separate
160-game compact diagnostic matched another 2,856 steps.

For a Python client such as Poke-RL, launch the Rust worker with
`--compact-observations` and replace `json.loads(line)` in the reply reader
with `decode_reply(line)` from
[`adapters/poke_rl_observations.py`](../adapters/poke_rl_observations.py).
Treat decoded observations as read-only because the player and critic views
share nested data. Requests, public-only games, and non-game replies are
unchanged. No prior snapshots or client-side battle state are needed to
decode a reply.

## Delta training observations

```sh
source scripts/env.sh
cargo build --locked --release
node benchmarks/throughput.mjs --trace-parity --delta-observations -- \
  ./target/release/pokemon-battle-worker --delta-observations
```

This opt-in mode sends a complete training snapshot on `new`, then sends
only changed view fields, changed roster slots, and new history entries.
Original team sets are sent only with the snapshot. The two critic views
are rebuilt from the player views. The benchmark
includes this reconstruction in its timing and compares the same complete
decoded replies and spectator logs against Showdown. It does not change
the requests, legal actions, or battle mechanics.

For Poke-RL, instantiate one `DeltaDecoder()` from
[`adapters/poke_rl_observations.py`](../adapters/poke_rl_observations.py) per
worker subprocess. Launch the worker with `--delta-observations` and replace
`json.loads(line)` with `decoder.decode_reply(line)` in the reply reader.
Decode replies in order, including errors and `close` replies, and treat
the resulting observations as read-only. On an error, both ends discard
their saved bases, so the next observation for each game is a full
snapshot. A sequence gap without a reset is an error, not a partial
observation. The default and stateless compact modes are still available.

The [delta release run](verified-delta-bss-result.json) measured 6,488.8
steps/s against Showdown's 343.9 across seven replays (18.87×). The
workload hash, all 673 decoded step replies, and all 32 spectator traces
matched. A separate 160-game diagnostic matched another 2,856 steps.

To separately measure JSON decoding and delta reconstruction with Python,
capture the same workload and replay it through actual worker subprocesses:

```sh
mkdir -p benchmarks/results
node benchmarks/throughput.mjs --capture-replay benchmarks/results/full-workload.jsonl
python3 benchmarks/throughput-python.py benchmarks/results/full-workload.jsonl
```

The Python replay also compared every decoded reply before timing. With
Python 3.13.5 it measured 5,045.0 steps/s against Showdown's 312.8 (16.13×)
across five replays. Delta step replies took 2.72 MB per replay versus
31.02 MB in full Rust JSON. Both figures are in the delta report. This
measures the transport only, without model inference or training.

## Packed deltas and independent steps

```sh
source scripts/env.sh
cargo build --locked --release
node benchmarks/throughput.mjs --trace-parity --packed-delta-observations -- \
  ./target/release/pokemon-battle-worker --packed-delta-observations --parallel-steps 4
```

Packed deltas encode changes to individual Pokémon fields and reuse the
same hidden and public battle data. The JavaScript benchmark and Python
`DeltaDecoder` reconstruct the same complete observations as full JSON.
`--parallel-steps` only schedules batches that step distinct live games.
It preserves per-game order and RNG state and reports results in request
order. Mixed batches and repeated game IDs use the normal sequential path.
A parallel batch is not transactional on error. Independent games may have
reached their next states when the worker reports an error and discards
delta bases. Restart failed rollouts if a batch returns an error.

The optional `--stream-steps` flag can deliver parts of a parallel batch
before its completion marker. A Python reply reader must keep reading when
`DeltaDecoder.decode_reply` returns `None`. Decoded observations are still
available only after the complete response. The benchmark supports both
framing modes.

## Parity and earlier runs

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

The [release verification](verified-champions-release-bss-result.json) at
`b136918` matched all 32 traces, all 673 timed step replies, and another 2,856
step replies in a separate 160-game diagnostic. The release worker ran at
1,445.0 steps/s versus 340.9 steps/s for Showdown on the same workload
(4.24× as fast).

The [Primal mechanics trace milestone](verified-primal-bss-result.json) at
`dd60eb4` matched all 32 traces, 673 timed step replies, and another 2,856
steps in the separate 160-game diagnostic. Showdown ran at 338.6 steps/s and
Rust at 1,437.7 steps/s on the same workload (4.25×).

The [previous BSS trace milestone](verified-modern-items-bss-result.json) at
`f5322a6` matched all 32 traces and 673 step replies. On the same workload,
Showdown ran at 345.9 steps/s and Rust at 1,485.6 steps/s (4.29×). It also
matched 2,856 steps in a separate 160-game diagnostic. The
[previous items and mechanics run](verified-288-bss-result.json) measured
4.36× on the same traces. The
[previous run](verified-192-bss-result.json) at `3b50228` measured 4.37×. The
[earlier expanded-audit run](verified-expanded-bss-result.json) at `48ac725`
measured 4.32× and matched the same traces and replies.

The [earlier BSS trace milestone](verified-current-bss-result.json) at
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

## Native battle replay

Use the native simulator contract to sample longer continuations in Random
Battle, Random Doubles Battle, and Roster Ranch Rumble. This diagnostic is
separate from the timed Poke-RL workload.

```sh
source scripts/env.sh
node scripts/capture-wide-native.mjs .tools/wide-native.jsonl --games 8 --max-turns 30
cargo test --locked --test replay wide_native -- --ignored --nocapture
```

Each game uses explicit, separate battle, roster, and choice seeds. The test
checks every recorded public and private log, each side's current request,
the party state, and the battle RNG, even when the replay reaches its turn
cap. Wall-clock timestamps and the VGC client's Open Team Sheets approval
button are excluded. It reports the first mismatch in each game. Use
`CHAMPIONS_NATIVE_CORPUS` to select a different corpus.

For submitted-team formats, pass `--teams MANIFEST.json`, a JSON object
keyed by format ID with an array of Showdown teams under each format. Each
team is an array of Showdown sets. The capture validates the rosters against
the pinned oracle and uses a separate seed to select two distinct rosters
per game. This does not replace native Rust team validation.

To reproduce the reactive-mechanics NatDex sample in the
[release replay report](verified-champions-release-native-parity.json), run:

```sh
node scripts/capture-wide-native.mjs .tools/wide-native-reactive-72.jsonl \
  --teams benchmarks/native-reactive-rosters.json --games 72 --first-game 217 \
  --max-turns 250 --special-actions
CHAMPIONS_NATIVE_CORPUS=.tools/wide-native-reactive-72.jsonl \
  cargo test --locked --test replay wide_native -- --ignored --nocapture
```

The damage calculator is verified separately against the locked Poke-RL move
pool and does not contribute to the throughput figure.

```sh
node scripts/capture-tactics.mjs .tools/tactics-training.json
cargo test --locked --test tactics -- --include-ignored
```

To derive those pools from a Poke-RL training manifest for every submitted
format, run `node scripts/prepare-native-rosters.mjs INPUT.json OUTPUT.json`
and use the output with `--teams`. Add format IDs after the output path to
limit the pool to those formats.

The [submitted-team sample](verified-native-submitted-128-parity.json) uses
all 128 rosters from `../Poke-RL/data/teams_train.json` as the pool for both
BSS Reg M-C singles and VGC 2026 Reg M-C doubles. It matches all 2,177
checkpoints across 64 complete games in each format.

The [all-format submitted sample](verified-native-all-formats-parity.json)
matches 4,595 checkpoints in 180 complete games across all 15 submitted
formats, including both Custom Games and both VGC Bo3 battle formats. It
repeats 24 of the BSS and VGC games above. The Bo3 series API is tested
separately.

The [recorded 24-game sample](verified-native-generated-parity.json) matches
all 574 checkpoints. Eighteen games ended naturally. The other six reached
the 30-turn cap with exact results through their last recorded checkpoint.
The [expanded sample](verified-native-expanded-parity.json) matches all
2,541 checkpoints in 96 completed games (`--games 32 --max-turns 60`).
The [next sample](verified-native-192-parity.json) matches 5,585 checkpoints
in 192 completed games (`--first-game 33 --games 64 --max-turns 90`).
The [288-game sample](verified-native-288-parity.json) matches all 7,929
checkpoints (`--first-game 97 --games 96 --max-turns 100`), with every game
finishing naturally.
The [384-game sample](verified-native-384-parity.json) matches all 10,520
checkpoints (`--first-game 193 --games 128 --max-turns 120`), again with no
turn caps. Replaying the four non-overlapping samples together matches all
26,575 checkpoints in 960 completed games.
These are native parity samples and do not measure throughput or establish
coverage of every move, ability, or item.

The [legacy NatDex Draft sample](verified-native-legacy-72-parity.json)
matches 3,609 checkpoints in 72 completed games. It uses six native-validated
rosters and an opt-in choice sampler that also chooses Z-Moves and Ultra Burst
from the native request. Regenerate and replay it with the pinned oracle.

```sh
node scripts/capture-wide-native.mjs .tools/wide-native-legacy-72.jsonl \
  --teams benchmarks/native-legacy-rosters.json --special-actions \
  --games 72 --first-game 65 --max-turns 180
CHAMPIONS_NATIVE_CORPUS=.tools/wide-native-legacy-72.jsonl \
  cargo test --locked --test replay wide_native -- --ignored --nocapture
```

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
