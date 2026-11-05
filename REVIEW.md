# Multi-Tier Tutorial, Solver, and Mobile Review (`REVIEW.md`)

New tutorials, continuum physics models, and interactive workbenches are reviewed before release across four independent tiers with different model capacities, tool access, and execution targets. Running smaller and mid-sized models alongside a live desktop browser/MCP student pass and an on-device Android `adb` smoke test catches continuum-mechanics bugs in the Rust kernel, usability traps in the web workbench, and mobile layout or native `arm64-v8a` regressions before release.

## The Four Review & Verification Tiers

1. **Interactive Browser & MCP Student Subagent (`flash` tier)**
   - **Role**: Works through the tutorial step by step in a real headless browser (`http://localhost:5173`) and via the `fieldsmith` MCP server, clicking every preset, dragging sliders to extreme bounds, editing spreadsheet cells, and syncing back to the 3D `.fsm` document.
   - **What it catches**: Silent UI state desynchronisation, clipped SVG plots, unlabelled synthetic reference data, confusing control labels (for example, when a secondary failure mode overrides a fitted curve), and missing warnings when a design window becomes infeasible (`T_min > T_max`).

2. **Cloud Fast-Reasoning Code & Physics Reviewer (`oc` bash alias)**
   - **Role**: Reads the Rust (`crates/fieldsmith-doc/`, `crates/fieldsmith-verify/`) and TypeScript (`web/src/doc/`, `web/src/app/`) source files directly and audits the analytical derivations against open standards (`FED-STD-H28`, `NASA-STD-5020B`, `VDI 2230`, `NIST/SEMATECH`, `NASA-HDBK-7005`).
   - **Invocation**:

         bash -ic 'oc run "Review crates/fieldsmith-doc/src/fixings.rs and web/src/app/FixingsModal.tsx as an aerospace mechanical engineering reviewer. Check the equations, CSG coordinates, and edge cases; list concrete bugs and fixes."'

   - **What it catches**: Sign and stiffness-ratio inversions in analytical formulas (such as `(1 - Φ_k)` clamp-loss factors), missing failure modes in `min()` candidate sets, CSG seat/pocket depth offsets, and Rust-vs-TypeScript drift.

3. **Local Small-Model Student Reviewer (`lclaude` bash alias / local `llama-server` on `:8090`)**
   - **Role**: Evaluates the tutorial prose, equations, and UI controls from the perspective of a first-time undergraduate engineering student using a smaller local model (`qwen3.8-flash-next` on `http://127.0.0.1:8090/v1/chat/completions`).
   - **Invocation**:
     - Interactive CLI: `bash -ic 'lclaude'`
     - Non-interactive batch query (avoids CLI wrapper startup overhead):

           python3 -c '
           import urllib.request, json
           payload = {
               "model": "qwen3.8-flash-next",
               "messages": [
                   {"role": "system", "content": "You are an undergraduate mechanical engineering student reviewing a Fieldsmith tutorial."},
                   {"role": "user", "content": open("/tmp/review_prompt.txt").read()}
               ],
               "temperature": 0.3,
               "max_tokens": 1200
           }
           req = urllib.request.Request(
               "http://127.0.0.1:8090/v1/chat/completions",
               data=json.dumps(payload).encode(),
               headers={"Content-Type": "application/json"},
           )
           resp = json.loads(urllib.request.urlopen(req, timeout=180).read().decode())
           print(resp["choices"][0]["message"]["content"])
           '

   - **What it catches**: Implicit domain assumptions (for example, showing rotational screwdriver torque `N·m` without explaining push-on/pull-off force `N` and root flexural strain `ε_root` when a cantilever snap-fit clip is selected), unexplained notation, and leaps in the Theory Manual derivations.

4. **Android APK Build, `adb install` & On-Device Smoke Test (`arm64-v8a`)**
   - **Role**: Builds the web bundle and native `aarch64-linux-android` JNI solver + MCP server (`libfieldsmith_mcp.so`), packages and installs `sh.fieldsmith.app` onto a real Android phone via `adb`, exercises the new MCP tools and native solver on-device (`127.0.0.1:18789`), and verifies portrait mobile layout and touch navigation.
   - **Invocation**:

         # 1. Build web bundle + native arm64-v8a JNI MCP/solver library + APK
         npm --prefix web run build
         ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/29.0.14206865" \
           cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p fieldsmith-mcp
         (cd android && ./gradlew --no-daemon assembleArm64Debug)

         # 2. Install APK and launch on the connected phone
         adb install -r android/app/build/outputs/apk/arm64/debug/app-arm64-debug.apk
         adb shell am force-stop sh.fieldsmith.app
         adb shell am start -n sh.fieldsmith.app/.MainActivity

         # 3. Forward on-device daemon (127.0.0.1:18789) and smoke-test native arm64 MCP + UI
         adb forward tcp:18789 tcp:18789
         curl -fsS http://127.0.0.1:18789/api/status
         adb exec-out screencap -p > /tmp/fieldsmith_adb_smoke.png

   - **What it catches**: Mobile portrait overflow, safe-area inset collisions with status/navigation bars, touch scrolling traps inside drawers/modals, and any divergence between desktop `x86_64` and mobile `aarch64` native solver or MCP tool execution.

## Review Checklist for Every New Tutorial or Physics Feature

1. **Dual-Mode Standards Verification**:
   - Link unencumbered public standards (`NIST`, `NASA-STD`, `FED-STD`, `MIL-HDBK`) alongside any ISO/VDI cross-reference.
   - Provide both **(a) the quick handbook rule** and **(b) the first-principles a-priori derivation** in code and UI, and assert their agreement (or expected regime divergence) in `crates/fieldsmith-verify/tests/`.
2. **Rust ↔ TypeScript Parity**:
   - Keep the browser preview evaluator in `web/src/doc/` and the authoritative Rust evaluator in `crates/fieldsmith-doc/` numerically aligned, and verify both with `cargo test` and `npx tsc --noEmit`.
3. **End-to-End Desktop & On-Device Evidence**:
   - Run the three reviewer models (`flash` browser/MCP subagent, `oc`, and `lclaude`) in parallel and fix every physics, geometry, or UX defect they surface.
   - Re-run `cargo test`, `npx tsc --noEmit`, and a headless Playwright verification script.
   - Build and `adb install` the updated `arm64-v8a` APK onto the connected Android device, smoke-test the native `:8931` MCP/solver endpoint and mobile UI, and capture on-device screenshot evidence before closing the task.
