#!/usr/bin/env python3
# V25 — the architecture datapoint: tracked-field comparison, field by field
# (GOAL v0.5 section B, evidence docs/verification/V25.md).
#
# Takes two api envelopes that answered the SAME request — A, the x86_64
# reference; B, the subject (aarch64) — and prints one line per tracked scalar:
# the path, both numbers, whether the raw bytes (bit pattern) are equal, whether
# the V22 canon quantization (floats at 6 significant digits) makes them equal,
# and B's relative deviation from A. Walls and other run-varying leaves are
# reported, not compared — for walls the datapoint IS the difference (the
# machine ratio), the same honesty as costs.csv.
#
# The V22 canon transform lives in fieldsmith-verify's v22_api.rs; the
# quantization here mirrors it (format `:.5e` round-trip), the transform that
# the fresh-clone gate already cross-checks against the pinned fixture bytes.
#
# Usage:
#   python3 -I scripts/v25-arm-compare.py A.json B.json [--walls] \
#     [--label-a "referee x86_64"] [--label-b "phone aarch64"]
# Exit 0 when every compared scalar is canon-equal; exit 1 when any is not
# (raw inequality alone does not fail — that is why the datapoint runs).

import argparse
import json
import sys

# The run-varying leaves, wherever they appear — the v22_api drop list (this
# script drops nothing else: any field not named here is compared).
DROPS = {"wall_ms", "total_ms", "mesh_ms", "solve_ms", "thermal_ms", "cost_fit",
         "stop_reason", "fit_ms", "seed_ms", "seed_n"}

count = [0, 0]  # [canon_equal, total]


def canon_float(x):
    return float(f"{x:.5e}")


def rel(a, b):
    if a == 0 or a is None or b is None:
        return None
    return (b - a) / a


def walk(a, b, path, rows):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(a.keys()):
            if k in DROPS:
                continue  # run-varying subtree: the V22 canon drops the whole key
            walk(a[k], b[k], f"{path}.{k}", rows)
        return
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            rows.append((path, f"[len {len(a)}]", f"[len {len(b)}]", False, True, None))
            return
        for i, (ai, bi) in enumerate(zip(a, b)):
            walk(ai, bi, f"{path}[{i}]", rows)
        return
    name = path.split(".")[-1].split("[")[0]
    if name in DROPS:
        return  # run-varying walls: not compared here
    if a is None and b is None:
        return
    if isinstance(a, str) and isinstance(b, str):
        ok = a == b
        rows.append((path, a, b, ok, ok, None))
        count[0] += ok
        count[1] += 1
        return
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        if isinstance(a, float) or isinstance(b, float):
            raw_eq = a == b
            ca, cb = canon_float(a), canon_float(b)
            ok = ca == cb
            rows.append((path, repr(a), repr(b), raw_eq, ok, rel(a, b)))
            count[0] += ok
            count[1] += 1
        else:
            ok = a == b
            rows.append((path, str(a), str(b), ok, ok, None))
            count[0] += ok
            count[1] += 1
        return
    if a != b:
        rows.append((path, repr(a), repr(b), False, False, None))
        count[1] += 1


def wall_rows(a, b, path, out):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(a.keys()):
            if k in DROPS:
                out.append(f"{path}.{k}: {a[k]!r} / {b[k]!r}")
                continue
            wall_rows(a[k], b[k], f"{path}.{k}", out)
        return
    if isinstance(a, list) and isinstance(b, list):
        for i, (ai, bi) in enumerate(zip(a, b)):
            wall_rows(ai, bi, f"{path}[{i}]", out)
        return
    if a is None and b is None:
        return
    name = path.split(".")[-1].split("[")[0]
    if name in DROPS and isinstance(a, (int, float)) and isinstance(b, (int, float)):
        out.append(f"{path}: {a} ms / {b} ms")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("--walls", action="store_true",
                    help="print the run-varying wall leaves beside the comparison")
    ap.add_argument("--label-a", default="A")
    ap.add_argument("--label-b", default="B")
    ns = ap.parse_args()

    env_a = json.load(open(ns.a))
    env_b = json.load(open(ns.b))

    print(f"engine   A: {env_a.get('engine')} / B: {env_b.get('engine')}")
    print(f"status   A: {env_a.get('status')} / B: {env_b.get('status')}  "
          f"name A: {env_a.get('name')} / B: {env_b.get('name')}")
    if env_a.get("engine") != env_b.get("engine"):
        print("ENGINE MISMATCH — the versions differ; tracked fields below are "
              "compared anyway, name the versions in the report")

    walls_out = []
    if ns.walls:
        wall_rows(env_a, env_b, "$", walls_out)

    rows = []
    walk(env_a, env_b, "$", rows)

    header = f"{'field':<52} {ns.label_a:<28} {ns.label_b:<28}"
    print(header)
    for path, va, vb, raw_eq, canon_eq, dr in rows:
        mark = "canon=" + ("=" if canon_eq else "X") + (" raw=" if canon_eq else " raw=") + ("=" if raw_eq else "X")
        if dr is None:
            delta = ""
        else:
            delta = f" rel Δ {dr:+.3e}"
        print(f"{path:<52} {va:<28} {vb:<28} {mark}{delta}")
    if ns.walls:
        print("--- walls (not compared):")
        for line in walls_out:
            print(line)
    canon_ok, total = count
    print(f"compared {total} fields: canon-equal {canon_ok}, canon-divergent {total - canon_ok}")
    sys.exit(0 if total == canon_ok else 1)


if __name__ == "__main__":
    main()