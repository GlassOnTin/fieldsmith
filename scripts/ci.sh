#!/usr/bin/env bash
# CI for GOAL.md v0.1: every gate that a machine can run.
#
#   scripts/ci.sh            (expects node_modules in web/ and a Rust toolchain)
#
# Stages, fail-soft so one run collects every result:
#   clippy (-D warnings) and the TypeScript typecheck first; workspace tests (verification
#   cases V1-V4, V8-V13, V15 and V14's standard tests run in it, plus V17-V23 since M13 —
#   rung records, estimator residual, thermal free expansion, Lamé cylinder, worker
#   privacy, the API contract, the sweep case; V6/V7/V14's chord ladder
#   excepted — they are #[ignore]d and run release below); the wasm package build; the
#   headless browser suite (V5 shader check, V16 interior, bracket, shape, make, mesh —
#   each refreshes its docs/verification/*.md; the interior case also runs isolated below);
#   the heavy release cases last (V6 gyroid law, V7 graded beam, V14 STEP chord ladder —
#   V8 in the workspace stage, V5/V16 in the browser suite, modal V13 in the workspace
#   stage: all of V1–V16 inside CI).
#
# Not scriptable and therefore not here: the GOAL hallway test (bracket with 3 new users)
# and human review of the produced reports. Release-blocking, but they run with people.
set -uo pipefail
cd "$(dirname "$0")/.."
fail=0
step() {
  local name="$1"; shift
  echo
  echo "===== $name ====="
  if "$@"; then
    echo "===== $name: PASS ====="
  else
    echo "===== $name: FAIL ====="
    fail=1
  fi
}
step 'cargo clippy -D warnings' cargo clippy --workspace --all-targets -- -D warnings
step 'TypeScript typecheck' npm --prefix web run typecheck
step 'workspace tests (V1-V4, V8-V13, V15, V17-V23 + V14 standard + unit tests)' cargo test --workspace
step 'wasm package build' npm --prefix web run wasm
step 'headless browser suite (V5 + V16 + bracket + shape + make + mesh)' npm --prefix web run test:e2e
step 'V16 interior case (isolated)' node web/tests/run-e2e.mjs --only interior
step 'V24 fresh-clone contract (V1-V23 workspace stage above built target/debug/fieldsmith; needs gh auth)' scripts/v24-fresh-clone.sh
step 'V6 gyroid law (heavy, release)' cargo test -p fieldsmith-verify --release --test v6_tpms_law -- --ignored --nocapture
step 'V7 graded beam (heavy, release)' cargo test -p fieldsmith-verify --release --test v7_graded_beam -- --ignored
step 'V14 STEP chord ladder (heavy, release)' cargo test -p fieldsmith-verify --release --test v14_step -- --ignored --nocapture
echo
if [ "$fail" = 0 ]; then echo 'CI: PASS'; else echo 'CI: FAIL'; fi
exit "$fail"