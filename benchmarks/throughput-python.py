"""Replay a captured training workload over pipes with Poke-RL's Python decoder."""

import argparse
from contextlib import ExitStack
from hashlib import sha256
import json
from pathlib import Path
from statistics import median
from subprocess import PIPE, Popen
import sys
from time import perf_counter

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from adapters.poke_rl_observations import DeltaDecoder


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workload", type=Path, help="JSON-lines from throughput.mjs --capture-replay")
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be positive")

    wire = args.workload.read_bytes()
    frames = wire.splitlines(keepends=True)
    batches = [json.loads(frame).get("requests", ()) for frame in frames]
    steps = [len(batch) if batch and all(op["op"] == "step" for op in batch) else 0 for batch in batches]
    iterations = sum(steps)
    if not iterations:
        parser.error("workload must contain batched steps")

    candidate = "./target/release/pokemon-battle-worker"
    commands = {
        "showdown": ["node", ".tools/poke-rl-simulator/worker.cjs"],
        "rust_full": [candidate],
        "rust_delta": [candidate, "--delta-observations"],
    }
    names = list(commands)
    decode = dict.fromkeys(names, json.loads)
    decode["rust_delta"] = DeltaDecoder().decode_reply
    with ExitStack() as stack:
        workers = {
            name: stack.enter_context(Popen(cmd, cwd=ROOT, stdin=PIPE, stdout=PIPE))
            for name, cmd in commands.items()
        }

        def exchange(name, request):
            worker = workers[name]
            worker.stdin.write(request)
            worker.stdin.flush()
            line = worker.stdout.readline()
            if not line:
                raise RuntimeError(f"{name} exited before replying")
            return decode[name](line), len(line)

        sizes = dict.fromkeys(names, 0)
        for request, count in zip(frames, steps):
            oracle, length = exchange("showdown", request)
            if "error" in oracle:
                raise RuntimeError(oracle["error"])
            if count:
                sizes["showdown"] += length
            for name in names[1:]:
                reply, length = exchange(name, request)
                if reply != oracle:
                    raise RuntimeError(f"{name} mismatch at request {oracle['id']}")
                if count:
                    sizes[name] += length

        samples = {name: [] for name in names}
        for repeat in range(args.repeats):
            for name in names if repeat % 2 == 0 else reversed(names):
                elapsed = 0.0
                for request, count in zip(frames, steps):
                    start = perf_counter()
                    reply, _ = exchange(name, request)
                    if count:
                        elapsed += perf_counter() - start
                        if len(reply["result"]) != count:
                            raise RuntimeError(f"{name} returned the wrong number of step results")
                samples[name].append(iterations / elapsed)

    medians = {name: median(values) for name, values in samples.items()}
    print(json.dumps({
        "python": sys.version.split()[0],
        "workload_sha256": sha256(wire).hexdigest(),
        "verified_frames": len(frames),
        "steps_per_replay": iterations,
        "step_reply_bytes": sizes,
        "median_iterations_per_second": medians,
        "samples": samples,
        "delta_speedup": medians["rust_delta"] / medians["showdown"],
    }, indent=2))


if __name__ == "__main__":
    main()
