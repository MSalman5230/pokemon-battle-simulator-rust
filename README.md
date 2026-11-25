# Pokémon Battle Simulator (Rust)

A headless Rust port of Pokémon Showdown's Gen 9 Champions singles and
doubles for Poke-RL, in progress. The crate currently contains the seeded RNG,
action and event ordering, Champions stat and damage primitives, native choice
parsing, and static dex data. It does not yet resolve battles. The benchmark
still runs Showdown.

## Local setup

Run from the repository root on glibc Linux (x86-64 or ARM64), with Git, Bash,
curl, tar, and a C linker installed.

```sh
scripts/setup.sh
source scripts/env.sh
cargo test --locked
```

The bootstrap installs Rust 1.100.0, Node.js 24.22.3, and rust-anything 4.1.0
under ignored `.tools/`. It stages the Poke-RL worker, lockfile, and training
team manifest there and builds Showdown from that lock with `npm ci`.
It does not launch a Showdown server or install Python/CUDA dependencies.
Use `POKE_RL_DIR=/path/to/Poke-RL scripts/setup-showdown.sh` to refresh the
staged reference from a different checkout. The benchmark expects the
Showdown commit `29f2d9969159fd23f8bb96e177a70072fe573b0e`, matching the
adjacent upstream checkout and the updated Poke-RL pin.

## Project-local MCP

[Codex](.codex/config.toml) and [OpenCode v2](opencode.jsonc) both launch
`scripts/rust-anything-mcp` over stdio using the local binary. Start the
clients from this project. If Codex prompts, trust this project to load
`.codex/config.toml`. Restart existing sessions to pick up the config.
There is no global MCP registration.

```sh
codex mcp get rust-anything
opencode mcp list
```

## Oracle fixtures and dex data

After setup, regenerate the checked-in fixtures and dex data with:

```sh
source scripts/env.sh
node scripts/export-champions-dex.mjs
node scripts/capture-oracle.mjs
```

The tests use checked-in fixtures and do not need Node. The fixture generator
records native singles and doubles replays for later turn-by-turn comparison.
The current Rust tests compare isolated mechanics against the locked simulator.

## Benchmark

```sh
source scripts/env.sh
node benchmarks/throughput.mjs
```

The benchmark measures batched battle steps per second through Poke-RL's
current singles worker. It can replay the same workload against a future
Rust worker using `-- path/to/release-worker`. See the
[benchmark guide](benchmarks/README.md) and the [porting plan](docs/porting.md).
