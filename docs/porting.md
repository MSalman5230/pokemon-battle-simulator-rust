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
corpus. The [latest trace run](../benchmarks/verified-modern-items-bss-result.json)
matched 673 step replies and 32 complete spectator traces without truncation.
It measured a 4.29× release speedup with native validation enabled. A separate
160-game diagnostic replay in the same report matched all 2,856 step replies.
This is a benchmark milestone, not general parity. Native validation covers
ordinary modern sets under Flat Rules, OU, UU, Doubles, Draft, 4v4 Doubles UU,
and 4v4 Doubles Draft, including their corresponding team bans. It admits all
128 rosters in the locked training corpus. NatDex Draft also checks ordinary
sets, with National Dex transfer learnsets and a Champions fallback. Some
origin, special form, and event constraints are still unsupported. Focused
National Dex and Custom Game replays cover the once-per-side Z-Crystal
selection, generic damage and status Z effects, and signature moves such as
Catastropika and Extreme Evoboost. Other inherited mechanics still need
coverage. Roster
Ranch Rumble generates ten-member rosters with a separate
PRNG and supports native pick-four doubles setup. Seeded tests compare complete
rosters and the final team PRNG state. Random Battle and Random Doubles Battle
generate teams, skip preview, and retain level-based stats and Illusion.
Continue testing more rosters, seeds, and formats and record focused native
fixtures for gaps.

The Random Battle fixtures cover species and forme sampling, compatibility
rules, lead order, moves and their RNG consumption, abilities, items, effort
values, and the final move shuffle. Additional native replays start and play
singles and doubles from player seeds with no supplied rosters. An
opt-in generated-team diagnostic samples longer continuations in Random
Singles, Random Doubles, and Roster Ranch Rumble. The
[expanded sample](../benchmarks/verified-native-expanded-parity.json)
matched all 2,541 checkpoints across 96 completed games. It includes 32
games in each generated format. The
[next sample](../benchmarks/verified-native-192-parity.json) matched all 5,585
checkpoints across another 192 completed games, 64 per format. The
[288-game sample](../benchmarks/verified-native-288-parity.json) matched all
7,929 checkpoints in 288 completed games, 96 per format. The
[384-game sample](../benchmarks/verified-native-384-parity.json) matched all
10,520 checkpoints in 384 completed games, 128 per format. A combined replay
of these four samples matched 26,575 checkpoints in 960 completed games.
The captures test those specific seeds and choices. Keep expanding the
corpus to exercise other mechanics.

The [submitted-team sample](../benchmarks/verified-native-submitted-128-parity.json)
reuses Poke-RL rosters in both BSS Reg M-C and VGC 2026 Reg M-C. It matched
all 2,177 checkpoints in 128 completed games, including exact private move
requests and PRNG state in both formats. A single run at `f5322a6` reproduced
both the generated and submitted samples, with 28,752 exact checkpoints
across 1,088 complete games.

The [all-format submitted sample](../benchmarks/verified-native-all-formats-parity.json)
matched 4,595 checkpoints in 180 completed games across the remaining format
entry points and repeated BSS and VGC controls. This includes Custom Game
debug output, but uses a modern roster pool. It does not establish complete
National Dex transfer or legacy battle coverage.

The Poke-RL adapter compares terminal spectator traces, excluding wall-clock
markers. Its hypothetical damage calculator runs in an isolated battle and
reuses the native damage formula. Tests compare each enumerated damage roll,
accuracy, speed, and survival flags. Unsupported preparation, multihit, and
other conditional cases do not fall back to heuristic damage estimates.

The public-belief lookahead endpoint restores an isolated battle from each
hypothetical world. It pairs candidates on the same opponent replies and
rollout seeds and follows interrupted turns through switches. Replays compare
public and critic observations and reconstruction assumptions. Unported world
effects are reported per world rather than borrowed from a live game.
The benchmark rejects any mismatch before timing the adapter.

The local `series::Series` API runs the explicit Reg M-B and Reg M-C Bo3
formats with fixed submitted rosters and caller-supplied game seeds. It
starts a fresh battle for each game, preserves the format's open team sheets,
and follows Showdown's win threshold when games tie. The series API does not
implement rooms, timers, or server state.
