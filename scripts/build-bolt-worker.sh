#!/usr/bin/env bash
# Optional Linux/x86-64 post-link tuning. No perf access or system install needed.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"
if [[ $# != 2 || ! -x "$1" || ! -r "$2" ]]; then
    printf 'Usage: %s /path/to/llvm-bolt /path/to/libbolt_rt_instr.a\n' "$0" >&2
    exit 2
fi
bolt="$(realpath -- "$1")"
runtime="$(realpath -- "$2")"
worker="$(BATTLE_BOLT_RELOCATIONS=1 scripts/build-profiled-worker.sh)"
dir="$root/.tools/profiled-worker"
profile="$(mktemp -d "$dir/bolt.XXXXXX")"
trap 'rm -rf -- "$profile"' EXIT
"$bolt" "$worker" --instrument --runtime-instrumentation-lib="$runtime" \
    --instrumentation-file="$profile/worker.fdata" -o "$profile/instrumented" >&2
"$profile/instrumented" --packed-delta-observations --parallel-steps 4 \
    < "$dir/training.jsonl" > /dev/null
"$bolt" "$worker" --data="$profile/worker.fdata" --reorder-blocks=ext-tsp \
    --reorder-functions=cdsort --split-functions --split-all-cold --split-eh \
    -o "$profile/optimized" >&2
mv -f -- "$profile/optimized" "$dir/pokemon-battle-worker-bolt"
printf '%s\n' "$dir/pokemon-battle-worker-bolt"
