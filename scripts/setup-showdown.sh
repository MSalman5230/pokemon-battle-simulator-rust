#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
source "$root/scripts/env.sh"
poke_rl="$(cd -- "${POKE_RL_DIR:-$root/../Poke-RL}" && pwd -P)"
oracle="$root/.tools/poke-rl-simulator"
mkdir -p "$oracle"

# Copy the real worker and its lock, without installing into or editing Poke-RL.
cp -- "$poke_rl/simulator/"{worker.cjs,tactics.cjs,lookahead.cjs,hypothetical.cjs,package.json,package-lock.json} "$oracle/"
cp -- "$poke_rl/data/teams_train.json" "$oracle/"
git -C "$poke_rl" rev-parse HEAD > "$oracle/poke-rl-revision"
if [[ -n "$(git -C "$poke_rl" status --porcelain --untracked-files=no)" ]]; then
    printf 'true\n' > "$oracle/poke-rl-dirty"
else
    printf 'false\n' > "$oracle/poke-rl-dirty"
fi

# Build Showdown exactly as Poke-RL's postinstall does. Unrelated server/native
# package lifecycle scripts are not needed for the headless battle worker.
cd "$oracle"
"$root/.tools/node/bin/npm" ci --ignore-scripts --no-audit --no-fund
"$root/.tools/node/bin/node" node_modules/pokemon-showdown/build
