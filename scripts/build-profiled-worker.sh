#!/usr/bin/env bash
# Tune the release build on a separate Showdown replay, not the timed replay.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"
source scripts/env.sh
profile_dir="$root/.tools/profiled-worker"
mkdir -p "$profile_dir"
raw="$(mktemp -d "$profile_dir/raw.XXXXXX")"
trap 'rm -rf -- "$raw"' EXIT
host="$(rustc -vV | sed -n 's/^host: //p')"
profdata="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-profdata"
if [[ ! -x "$profdata" ]]; then
    rustup component add llvm-tools >&2
fi

workload="$profile_dir/training.jsonl"
node benchmarks/throughput.mjs --games 80 --seed 50221 --capture-replay "$workload" \
    > "$profile_dir/training-coverage.json"
export CARGO_TARGET_DIR="$profile_dir/target"
export CARGO_PROFILE_RELEASE_LTO=fat
worker="$CARGO_TARGET_DIR/release/pokemon-battle-worker"
RUSTFLAGS="-C target-cpu=native -C profile-generate=$raw" cargo build --release --locked
for run in {1..8}; do
    LLVM_PROFILE_FILE="$raw/train-%p.profraw" "$worker" \
        --packed-delta-observations --parallel-steps 4 < "$workload" > /dev/null
done
"$profdata" merge "$raw"/train-*.profraw -o "$profile_dir/worker.profdata"
export RUSTFLAGS="-C target-cpu=native -C profile-use=$profile_dir/worker.profdata"
if [[ ${BATTLE_BOLT_RELOCATIONS:-0} == 1 ]]; then
    cargo rustc --release --locked --bin pokemon-battle-worker -- -C link-arg=-Wl,--emit-relocs
else
    cargo build --release --locked
fi
printf '%s\n' "$worker"
