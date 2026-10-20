#!/usr/bin/env bash
# V41 — the new-user path as a gate (VISION §7 item 1, evidence docs/verification/V41.md).
#
# From the remote alone, the README's own commands produce a working first session:
#   1. `gh repo clone` the private repo with submodules into a fresh temp dir (plain
#      `git clone` cannot see a private repo — the same clone leg as V24).
#   2. The README's build, verbatim: `npm --prefix web install`, then
#      `bash scripts/build-wasm.sh`. Nothing else is installed into the clone.
#   3. The README's run command: `npm --prefix web -- run dev`, on a fixed port so the
#      gate can find it (strictPort — a taken port fails the gate instead of silently
#      driving someone else's server).
#   4. `node web/tests/newuser.mjs <url>` from inside the clone (its own node_modules,
#      its own wasm packages): open the page, open the example through the empty-state
#      button, visit every lens with non-blank pixel checks, build the export, and
#      report every console error and failed request — what a first user would hit.
#
# The report lands in this checkout's docs/verification/V41.md, written only on PASS
# (the e2e family convention). Walls are printed against the declared budget; the
# ≤ 10 min clone-to-answer budget is reported, not gated (a miss is answered in a
# reviewed commit, never a quiet skip).
#
# FIELDSMITH_V41_CLONE_SRC overrides the clone source (a local path or URL). The gate
# defaults to the remote — that is the point — but the override lets the gate itself be
# tested against an unpushed commit; the report names which source was used.

set -uo pipefail
cd "$(dirname "$0")/.."
TREE="$PWD"
CLONE="$(mktemp -d /tmp/fieldsmith-v41-clone.XXXXXX)"
LOG="$(mktemp -d /tmp/fieldsmith-v41-log.XXXXXX)/dev-server.log"
PORT=5191
SERVER_PID=""
SERVER_PGID=""
fail=0

cleanup() {
  # setsid makes the server its own process group; npm spawns vite as a child, so the
  # whole group must die, not just the npm wrapper.
  [ -n "$SERVER_PGID" ] && kill -- "-$SERVER_PGID" 2>/dev/null
  wait "$SERVER_PID" 2>/dev/null
  rm -rf "$CLONE" "$(dirname "$LOG")"
}
trap cleanup EXIT

SRC="${FIELDSMITH_V41_CLONE_SRC:-}"
if [ -n "$SRC" ]; then
  SRC_DESC="$SRC (FIELDSMITH_V41_CLONE_SRC override)"
else
  SRC_DESC="https://github.com/GlassOnTin/fieldsmith (the remote)"
fi

t_all=$SECONDS
t0=$SECONDS
echo "===== V41 leg 1: fresh clone — source: $SRC_DESC ====="
if [ -n "$SRC" ]; then
  git clone --recurse-submodules "$SRC" "$CLONE" || { fail=1; echo "clone failed"; }
elif command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh repo clone GlassOnTin/fieldsmith "$CLONE" -- --recurse-submodules || { fail=1; echo "clone failed"; }
else
  git clone --recurse-submodules https://github.com/GlassOnTin/fieldsmith.git "$CLONE" || { fail=1; echo "clone failed"; }
fi
t_clone=$((SECONDS - t0))

if [ "$fail" = 0 ]; then
  t1=$SECONDS
  echo "===== V41 leg 2: the README's build, verbatim ====="
  (cd "$CLONE" && npm --prefix web install) || { fail=1; echo "npm install failed"; }
  (cd "$CLONE" && bash scripts/build-wasm.sh) || { fail=1; echo "build-wasm.sh failed"; }
  t_build=$((SECONDS - t1))
fi

t_serve=0
t_pass=0
if [ "$fail" = 0 ]; then
  echo "===== V41 leg 3: the README's run command ====="
  # Refuse to start on a port something already serves: an earlier run's leaked server
  # (or any other vite) would answer the probe below and the pass would silently drive
  # that tree instead of the clone. Vite's own --strictPort dies on EADDRINUSE, but the
  # probe would never notice — it only checks the port, not whose port it is.
  for host in localhost 127.0.0.1; do
    if curl -sf -o /dev/null --max-time 2 "http://$host:$PORT/"; then
      echo "port $PORT is already served by $host — the gate must drive its own clone's server"; fail=1; break
    fi
  done
fi
if [ "$fail" = 0 ]; then
  # Identity marker: served from the clone's web/ root, so the probe can prove the
  # answering server is this run's clone, not a stranger on the same port.
  MARKER="v41-$$-$RANDOM"
  echo "$MARKER" > "$CLONE/web/v41-marker.txt"
  # setsid (exec'd, so $! is the session leader) puts npm and its vite child in their
  # own process group; cleanup kills the group, not just the wrapper.
  (cd "$CLONE" && exec setsid npm --prefix web run dev -- --port "$PORT" --strictPort) >"$LOG" 2>&1 &
  SERVER_PID=$!
  SERVER_PGID=$SERVER_PID
  # Probe both localhost and 127.0.0.1: Vite binds ::1 by default on this stack but
  # 127.0.0.1 on others, and the README tells the reader to open http://localhost:5173.
  URL=""
  for _ in $(seq 1 120); do
    for host in localhost 127.0.0.1; do
      body=$(curl -sf --max-time 2 "http://$host:$PORT/v41-marker.txt" 2>/dev/null) || continue
      if [ "$body" = "$MARKER" ]; then URL="http://$host:$PORT/"; break; fi
    done
    [ -n "$URL" ] && break
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
      echo "dev server exited early:"; cat "$LOG"; fail=1; break
    fi
    sleep 1
  done
  [ -n "$URL" ] || { echo "dev server never served its marker on $PORT:"; cat "$LOG"; fail=1; }
  t_serve=$((SECONDS - t_build - t_clone))
fi

if [ "$fail" = 0 ]; then
  t2=$SECONDS
  echo "===== V41 leg 4: the first-user pass (web/tests/newuser.mjs) ====="
  node "$CLONE/web/tests/newuser.mjs" "$URL" \
    --report "$TREE/docs/verification/V41.md" --source "$SRC_DESC" || fail=1
  t_pass=$((SECONDS - t2))
fi

total=$((SECONDS - t_all))
echo "V41: clone ${t_clone}s / install+build ${t_build}s / serve-up ${t_serve}s / pass ${t_pass}s / total ${total}s (declared budget ≤ 600 s)"
if [ "$fail" = 0 ]; then
  echo "===== V41: PASS ====="
else
  echo "===== V41: FAIL ====="
fi
exit "$fail"
