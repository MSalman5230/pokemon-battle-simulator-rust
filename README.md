# Pokémon Battle Simulator (Rust)

A headless Rust port of Pokémon Showdown's Gen 9 Champions singles and
doubles for Poke-RL, in progress. The Rust core accepts native Showdown input
and replays seeded singles and doubles battles across a growing set of rules
and mechanics. The experimental Poke-RL worker matched all 673 step replies
and all 32 spectator traces in the default BSS Reg M-C benchmark and ran
4.37× faster in release mode. That result covers the fixed workload. The worker validates
ordinary BSS rosters in Rust, without a reference manifest. Unsupported
mechanics and unimplemented legality constraints return errors. Random
Battle, Random Doubles Battle, and Roster Ranch Rumble generate teams in Rust.
Full battle parity is not complete. The worker implements tactics metadata
and tested subsets of public-state damage queries and one-turn lookahead.

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
node scripts/export-champions-legality.mjs
node scripts/export-ranch-data.mjs
node scripts/export-random-data.mjs
node scripts/export-tactics-metadata.mjs
node scripts/capture-oracle.mjs
node scripts/capture-validation.mjs
node scripts/capture-generate-teams.mjs
node scripts/capture-tactics.mjs
node scripts/capture-lookahead.mjs
```

The tests use checked-in fixtures and do not need Node. The fixture generator
records seeded native battles from the locked simulator. Replay tests compare
player requests, all three battle-log views, party state, and the RNG state at
each checkpoint. Separate fixtures isolate RNG, ordering, stats, and HP display.
The legality fixtures compare set and team validation to the same pinned build.

## Benchmark

```sh
source scripts/env.sh
node benchmarks/throughput.mjs
```

The benchmark measures batched battle steps per second through Poke-RL's
current singles worker. It checks exact replay parity before measuring
the Rust worker. See the
[benchmark guide](benchmarks/README.md) and the [porting plan](docs/porting.md).
