#!/usr/bin/env python3
"""Serial, opt-in real-model benchmark. Run from the repository root."""
import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import time

MODELS = ("whisper", "qwen3", "seaco", "zipformer")
STRENGTHS = ("disabled", "light", "standard", "thorough")


def fixtures(directory):
    source = Path("apps/desktop/assets/asr-test/standard-zh.pcm").read_bytes()
    window = source + bytes(30 * 16000 * 2 - len(source))
    cases = {"short-zh": source, "segmented-zh": window * 2 + source}
    result = {}
    for name, data in cases.items():
        path = directory / (name + ".pcm")
        path.write_bytes(data)
        result[name] = (path, hashlib.sha256(data).hexdigest())
    return result


def matrix(repeats, cases):
    return list(itertools.product(MODELS, ("capture", "buffered"),
                                 STRENGTHS, cases, ("cold", "warm"), range(repeats)))


def inference_mode(model):
    # Describes the decoder API, independent of when PCM is submitted.
    return "streaming" if model == "zipformer" else "offline"


def coverage_matrix(repeats, cases):
    for cell in matrix(repeats, cases):
        for mode in ("streaming", "offline"):
            yield (*cell, mode)


def blocked(model, strength, environment):
    if not environment.get("SAYMORE_BENCH_" + model.upper() + "_DIR"):
        return "model_directory_not_configured"
    if strength != "disabled" and not all(environment.get(key) for key in
            ("SAYMORE_BENCH_LLM_URL", "SAYMORE_BENCH_LLM_MODEL")):
        return "refinement_provider_not_configured"
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--models", nargs="+", choices=MODELS, default=list(MODELS))
    parser.add_argument("--scheduling", nargs="+", choices=("during-capture", "finish-only"),
                        default=["during-capture"])
    parser.add_argument("--audit-modes", action="store_true",
                        help="Enumerate both decoder modes; unsupported adapters are explicit")
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("repeats must be positive")
    args.output.mkdir(parents=True, exist_ok=False)
    cases = fixtures(args.output)
    cells = coverage_matrix(args.repeats, cases) if args.audit_modes else (
        (*cell, inference_mode(cell[0])) for cell in matrix(args.repeats, cases))
    plan = [(*cell, scheduling) for cell in cells if cell[0] in args.models
            for scheduling in args.scheduling]
    random.Random(args.seed).shuffle(plan)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    binary = Path("target/release/examples/asr_latency_probe")
    metadata = {"revision": revision, "platform": platform.platform(), "seed": args.seed,
                "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"])),
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()
                if binary.is_file() else None, "cases": len(plan),
                "llm_model": os.environ.get("SAYMORE_BENCH_LLM_MODEL"),
                "llm_profile": os.environ.get("SAYMORE_BENCH_LLM_PROFILE", "portable")}
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2))
    with (args.output / "runs.jsonl").open("w") as output:
        for model, feed, strength, case, temperature, repeat, mode, scheduling in plan:
            pcm, digest = cases[case]
            row = dict(model=model, inference_mode=mode, scheduling=scheduling, feed=feed, refinement=strength, fixture=case,
                       temperature=temperature, repeat=repeat, pcm_sha256=digest)
            row["path"] = ("native_online" if model == "zipformer" else "background_segments") \
                if feed == "capture" else "deferred_same_decoder"
            reason = ("decoder_mode_not_implemented_for_pinned_artifact"
                      if mode != inference_mode(model) else blocked(model, strength, os.environ))
            row["status"] = ("unsupported" if mode != inference_mode(model)
                             else "blocked" if reason else "planned")
            row["reason"] = reason
            if args.execute and not reason:
                environment = dict(os.environ, SAYMORE_BENCH_COLD="1" if temperature == "cold" else "0",
                                   SAYMORE_BENCH_SCHEDULING=scheduling)
                start = time.monotonic()
                try:
                    run = subprocess.run([str(binary), model,
                        environment["SAYMORE_BENCH_" + model.upper() + "_DIR"],
                        str(pcm), feed, strength], env=environment, capture_output=True,
                        text=True, timeout=600, check=False)
                    row["status"] = "measured" if run.returncode == 0 else "failed"
                    row["exit_code"] = run.returncode
                    if run.returncode == 0:
                        row["measurement"] = json.loads(run.stdout)
                    # Do not persist stderr: a provider failure could include secrets.
                except (subprocess.TimeoutExpired, OSError, json.JSONDecodeError) as error:
                    row["status"] = "failed"
                    row["reason"] = type(error).__name__
                row["process_wall_seconds"] = time.monotonic() - start
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
            output.flush()
            print(model, feed, strength, case, temperature, row["status"], flush=True)


if __name__ == "__main__":
    main()
