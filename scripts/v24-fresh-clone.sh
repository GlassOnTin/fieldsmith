#!/usr/bin/env bash
# V24 — the fresh-clone contract (GOAL v0.5 section A, evidence docs/verification/V24.md).
#
# From the remote alone, the clone builds and the pinned golden api question answers
# byte-stably at the pinned version:
#   1. `gh repo clone` the private repo with submodules into a fresh temp dir. Needs a
#      gh auth with repo scope on this box — plain `git clone` cannot see a private repo.
#   2. `cargo build --workspace` in the clone, nothing else present. The caller's
#      CARGO_TARGET_DIR is unset (a stale value would cache the clone's build outside
#      it and fake the wall); ~/.cargo's registry cache is fair — that is what a normal
#      user brings.
#   3. The api verb answers the pin: the `ladder-ok` case of
#      crates/fieldsmith-verify/tests/fixtures/v22-golden.jsonl (apiv 2, the reference
#      bracket, rungs [30000, 150000]), the request built from the clone's own
#      examples/bracket.fsm. The answer's canon — the V22 canon: the timing leaves
#      dropped, every float quantized to 6 significant digits, keys sorted — is
#      byte-compared against the fixture line. The engine identity travels inside the
#      canon, so a version bump breaks it: the named, never silent, change.
#   4. `data/costs/costs.csv` travels intact: byte-equal to the tree this checkout is.
#
# The canon comparator here is python, and it is validated, not trusted: before the
# clone is compared, canon(the tree's own verb run on the same request) must reproduce
# the fixture's raw bytes. If the two implementations ever disagree, the fixture stops
# matching and this stage fails loudly — a finding, not a silent pass.
#
# Contract failures exit nonzero. Walls are printed against the declared budget; the
# ≤ 5 min clone-to-answer budget is reported, not gated (a miss is answered in a
# reviewed commit, never a quiet skip).

set -uo pipefail
cd "$(dirname "$0")/.."
TREE="$PWD"
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET  # the clone builds cold, in its own target/
CLONE="$(mktemp -d /tmp/fieldsmith-v24-clone.XXXXXX)"
WORK="$(mktemp -d /tmp/fieldsmith-v24-work.XXXXXX)"
trap 'rm -rf "$CLONE" "$WORK"' EXIT
fail=0
t_clone=0
t_build=0
t_answer=0

t_all=$SECONDS
t0=$SECONDS
echo "===== V24 leg 1: fresh clone from the remote ====="
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh repo clone GlassOnTin/fieldsmith "$CLONE" -- --recurse-submodules || { fail=1; echo "clone failed"; }
else
  git clone --recurse-submodules https://github.com/GlassOnTin/fieldsmith.git "$CLONE" || { fail=1; echo "clone failed"; }
fi
t_clone=$((SECONDS - t0))
[ -d "$CLONE/fenics-rs/crates" ] || { fail=1; echo "submodule fenics-rs missing from the clone"; }

if [ "$fail" = 0 ]; then
  t1=$SECONDS
  echo "===== V24 leg 2: cargo build --workspace in the clone ====="
  (cd "$CLONE" && cargo build --workspace) || { fail=1; echo "clone build failed"; }
  t_build=$((SECONDS - t1))
fi
echo "clone wall ${t_clone}s / build wall ${t_build}s"

# The pinned golden request, built from the clone's own reference document.
if [ "$fail" = 0 ]; then
  python3 -I - "$WORK/request.json" "$CLONE/examples/bracket.fsm" <<'PY' || fail=1
import json, sys
doc = open(sys.argv[2]).read()
open(sys.argv[1], "w").write(json.dumps(
    {"apiv": 2, "doc": doc, "question": {"kind": "ladder", "rungs": [30000, 150000]}}))
PY
fi

# costs.csv intact: present in the clone and byte-equal to the tree this checkout is.
if [ "$fail" = 0 ]; then
  if [ -f "$CLONE/data/costs/costs.csv" ] && cmp -s "$CLONE/data/costs/costs.csv" "$TREE/data/costs/costs.csv"; then
    echo "costs.csv: present and byte-identical in the clone"
  else
    echo "costs.csv: missing from the clone, or differs from the tree"
    fail=1
  fi
fi

# The tree's own verb run first — the comparator's validation leg — then the clone's.
if [ -x "$TREE/target/debug/fieldsmith" ] && [ "$fail" = 0 ]; then
  echo "===== V24 leg 3: the pinned golden api request ====="
  FIELDSMITH_COSTS_STORE=/dev/null "$TREE/target/debug/fieldsmith" api \
    < "$WORK/request.json" > "$WORK/env-tree.json" || { fail=1; echo "tree api run failed"; }
elif [ "$fail" = 0 ]; then
  echo "V24 leg 3 skipped comparator validation: $TREE/target/debug/fieldsmith missing (run a workspace build first)"
  fail=1
fi
if [ "$fail" = 0 ]; then
  t2=$SECONDS
  FIELDSMITH_COSTS_STORE=/dev/null "$CLONE/target/debug/fieldsmith" api \
    < "$WORK/request.json" > "$WORK/env-clone.json" || { fail=1; echo "clone api run failed"; }
  t_answer=$((SECONDS - t2))
fi

# The canon: v22_api.rs's transform (timing leaves dropped, floats at 6 significant
# digits, keys sorted), compared as raw bytes against the fixture line's own canon
# text. Structural equality is checked separately so a formatter divergence names
# itself instead of failing as a byte mismatch.
if [ "$fail" = 0 ]; then
  python3 -I - \
    "$WORK/env-tree.json" "$WORK/env-clone.json" \
    "$CLONE/crates/fieldsmith-verify/tests/fixtures/v22-golden.jsonl" <<'PY' || fail=1
import json, sys

DROPS = {"wall_ms", "total_ms", "mesh_ms", "solve_ms", "thermal_ms", "cost_fit",
         "stop_reason", "fit_ms", "seed_ms", "seed_n"}

def canon(o):
    if isinstance(o, dict):
        return {k: canon(v) for k, v in sorted(o.items()) if k not in DROPS}
    if isinstance(o, list):
        return [canon(v) for v in o]
    if isinstance(o, float):
        return float(f"{o:.5e}")
    return o

def canon_text(env):
    return json.dumps(canon(env), sort_keys=True, separators=(",", ":"), ensure_ascii=False)

with open(sys.argv[3]) as f:
    fixture = f.readline().rstrip("\n")  # the trailing newline is not part of the canon text
assert fixture.endswith(',"id":"ladder-ok"}') and fixture.startswith('{"canon":')
pinned = fixture[len('{"canon":'):-len(',"id":"ladder-ok"}')]

env_tree = json.load(open(sys.argv[1]))
env_clone = json.load(open(sys.argv[2]))
c_tree = canon_text(env_tree)
c_clone = canon_text(env_clone)

ok_tree = c_tree == pinned
ok_clone = c_clone == pinned
print(f"canon: tree run matches the pinned fixture bytes: {ok_tree} (comparator validation)")
print(f"canon: clone run matches the pinned fixture bytes: {ok_clone}")
if env_tree.get("status") != "ok" or env_clone.get("status") != "ok":
    print(f"status: tree={env_tree.get('status')} clone={env_clone.get('status')} — the pin expects ok")
    fail = 1
engine = env_clone.get("engine")
print(f"clone engine identity: {engine}")
if not ok_tree:
    print("the comparator itself disagrees with the fixture bytes on the tree's run — "
          "debug this before trusting the clone leg")
    sys.exit(1)
if not ok_clone:
    print("the fresh clone's canon diverges from the pinned golden — if the engine "
          "identity above moved, re-bless through v22_api with FIELDSMITH_BLESS=1 in a "
          "recorded commit; otherwise this is a defect")
    sys.exit(1)
sys.exit(0)
PY
fi

total=$((SECONDS - t_all))
echo "V24: answer wall ${t_answer}s / clone-to-answer total ${total}s (declared budget ≤ 300 s)"
if [ "$fail" = 0 ]; then
  echo "===== V24: PASS ====="
else
  echo "===== V24: FAIL ====="
fi
exit "$fail"