#!/usr/bin/env bash
# CI for GOAL.md v0.1: every gate that a machine can run.
#
#   scripts/ci.sh            (expects node_modules in web/ and a Rust toolchain)
#
# Stages, fail-soft so one run collects every result:
#   clippy (-D warnings) and the TypeScript typecheck first; workspace tests (verification
#   cases V1/V2/V3/V4/V6/V9 run in it, V7 excepted — it is #[ignore]d and runs release);
#   the wasm package build; the headless browser suite (V5 shader check, bracket, shape,
#   make — each refreshes its docs/verification/*.md); V7 heavy last.
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
step 'workspace tests (V1-V4, V6, V9 + unit tests)' cargo test --workspace
step 'wasm package build' npm --prefix web run wasm
step 'headless browser suite (V5 + bracket + shape + make)' npm --prefix web run test:e2e
step 'V7 graded beam (heavy, release)' cargo test -p fieldsmith-verify --release --test v7_graded_beam -- --ignored
echo
if [ "$fail" = 0 ]; then echo 'CI: PASS'; else echo 'CI: FAIL'; fi
exit "$fail"