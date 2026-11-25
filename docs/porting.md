# Porting plan

## Scope

Target the battle-relevant behavior of the `[Gen 9 Champions]` formats
in the adjacent Showdown checkout at
`29f2d9969159fd23f8bb96e177a70072fe573b0e`. Use the full
`config/formats.ts`, not just the main Champions section. Include singles
and doubles format rules, including random or rental teams, team validation
and preview, and regulation-specific legality where the formats require
them. Implement shared inherited behavior only when a Champions format
needs it. Local series and team-sheet behavior can be handled outside turn
resolution. Skip networking, multiplayer services, and real-time clocks.

BSS Reg M-C (`gen9championsbssregmc`) is the first integration target.
Use VGC 2026 Reg M-C (`gen9championsvgc2026regmc`) as the first doubles
parity target. Add the remaining Champions formats on the same engine.
Do not reuse the singles action IDs as the doubles action model. Doubles
needs choices per active slot, explicit targeting, combined choices,
and valid partial replacement when fewer bench Pokémon are available.

## Engine and training boundary

Use upstream Showdown's battle input contract, as defined by
`sim/battle-stream.ts` and the simulator objects it calls. Preserve the
semantics of battle setup (format, rules, teams, and seed) and player choices
(preview picks, moves, targets, and switches). The Rust core can represent
these as typed inputs independent of any wire format. Preserve RNG
consumption, event order, and per-side visibility.

The initial compatibility adapter should let Poke-RL swap workers without
changing its Python training logic or observation schema. Translate Poke-RL's
JSON-lines requests and model-facing action IDs at that boundary. Changes to
model inputs or features should not change the simulator's battle inputs.
Poke-RL also uses validation, tactics, lookahead, and traces. Identify
which operations each training configuration needs before calling the
replacement complete. Keep critic-only views opt-in and separate from
player observations. Consider in-process Python bindings or a bulk
interface only after measuring where time goes.

## Parity and performance

1. Capture native Showdown battle inputs and outputs for seeded parity
   fixtures. Test Poke-RL action IDs, observations, and truncation separately
   at the adapter boundary. Check preview,
   switches, hidden trap and disable, Mega Evolution, event order, and
   simultaneous effects. Add doubles fixtures for targeting, spread
   moves, joint choices, and forced replacement.
2. Port by behavior and compare the Rust core against those fixtures.
   Use rust-anything to profile or replay isolated JS/TS work where it fits.
   The full-worker replay checks the integration boundary and is not a
   substitute for individual mechanics tests.
3. Benchmark identical replay workloads and compare release builds on the
   same machine. Keep the current singles JSON-lines baseline separate
   from future doubles, in-process core, and end-to-end training tests.

## Current replay coverage

The checked-in replays compare exact private requests, public and private log
lines, party state, and RNG state. They cover BSS and VGC Reg M-B/M-C, packable
teams, full-team preview, spread damage, partial replacement, pivots,
form changes, weather, hazards, screens, status, and selected move, item,
and ability effects. Passing a preview fixture does not imply that all rules
for that format are implemented.

The experimental Poke-RL worker passes exact response replay on the curated
BSS benchmark and the fixed 32-game default benchmark using the training
manifest. The [recorded default run](../benchmarks/verified-default-bss-result.json)
matched 673 steps without truncation and measured a 4.45× release speedup.
This is a benchmark milestone, not general parity. The Rust core does not
yet validate legality or generate teams. Continue testing more rosters,
seeds, and formats and record focused native fixtures for missing mechanics.
The benchmark rejects any mismatch before timing the adapter.
