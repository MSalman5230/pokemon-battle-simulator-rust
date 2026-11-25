# AGENTS.md

This crate is for a headless, deterministic Rust port of Pokémon Showdown's
**Gen 9 Champions** battle simulator, including singles and doubles. Poke-RL is
the first consumer. Do not port the Showdown server, networking, ladders,
accounts, chat, UI, or battle logic for unrelated generations and formats.

## References

- `../pokemon-showdown-upstream` is the upstream source. Use `sim/`,
  `data/mods/champions*/`, their inherited data and handlers, and
  `config/formats.ts`. The reference commit for this scaffold is
  `29f2d9969159fd23f8bb96e177a70072fe573b0e`.
- `../Poke-RL` defines the training contract. Start with
  `simulator/worker.cjs`, `champions_rl/engine.py`, and
  `champions_rl/rollout_workers.py`. It uses BSS Reg M-C singles today.
  The worker and its locked Showdown dependency are the throughput oracle.
- Use Showdown's native simulator inputs as the battle contract. Poke-RL
  action IDs, observations, and model features belong in a separate adapter.
- See [the porting plan](docs/porting.md) for scope, parity cases, and the
  distinction between Showdown mechanics and Poke-RL's observation contract.

## Development

- Run `scripts/setup.sh` and `source scripts/env.sh`. Install tools in
  `.tools/`; keep MCP registration in this project. Do not run the helper's
  global `setup` command.
- During the port, use rust-anything
  to profile and record small JS/TS boundaries where useful. Use seeded
  differential tests against upstream Showdown for game behavior.
  Keep gameplay, I/O, and Python-facing observations separate. Do not
  approximate or silently fall back for missing mechanics.
- Run `cargo fmt --check`, `cargo clippy --locked --all-targets`, and
  `cargo test --locked`. Use the [same benchmark workload](benchmarks/README.md)
  and a release build for speed comparisons. Do not count Python inference
  steps, in-game turns, and simulator iterations as the same thing.

## Git workflow

- Use `origin` at `https://github.com/MSalman5230/pokemon-battle-simulator-rust`.
- Work and commit directly on `main` only. Do not create branches or pull requests.
- Configure the owner's GitHub-linked identity with repository-local `user.name`,
  `user.email`, and `user.useConfigOnly=true`. Before committing, verify both
  `git var GIT_AUTHOR_IDENT` and `git var GIT_COMMITTER_IDENT`. Never use Git's
  hostname-derived email.
- Commit regularly at small, coherent milestones after running the relevant checks.
  Push verified commits to `origin/main` without force-pushing. Do not wait until
  the whole port is finished to commit.
