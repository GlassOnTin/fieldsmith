# Fieldsmith — Goal (v0.5: Foundation & Verification Closure)

**In progress (M17–M19 closed; M20–M21 open).**

This file defines **what "done" means for v0.5**. The [VISION](VISION.md) defines *why* and
*where next*; this file defines the concrete, testable acceptance bar for the current release.
Previous goals are preserved in git history (`v0.2.0` at `a52b6b8`, `v0.3` at `0b92e1c`,
`v0.4.0` at `295e80e`). If an acceptance criterion or tolerance turns out to be wrong, update
this file in a reviewed commit with the measured reason rather than skipping it.

---

## The goal in one sentence

> Anyone can clone Fieldsmith from its git remote and build and verify it from the clone alone
> (`V24`); the verified solver and parameter sweep are measured on a non-x86_64 architecture
> (`V25`); and the three unresolved technical findings carried out of v0.3/v0.4 — the `V3`
> quarter-hole tangent-junction mesh defect, the mesh-leaf graded-fill slicer manifold seam at
> `skin = 1.2 mm`, and the N100 Chrome WebGL/Worker wall-time split — are diagnosed and closed
> with reproducible evidence.

---

## The reference task

1. **Standalone fresh clone (`V24`).** `git clone --recurse-submodules` from the remote into a
   clean directory, `cargo build`, and run the pinned golden `fieldsmith api` request; the
   canonical response matches byte-for-byte (`scripts/v24-fresh-clone.sh`).
2. **Cross-architecture verification (`V25`).** Run the reference tolerance ladder and the `V23`
   5-point cantilever sweep through `fieldsmith api` on an aarch64 device the user owns
   (OnePlus 13, Snapdragon 8 Elite `SM8750`). Record wall times, throughput (points/hour), and
   field-by-field divergence against the x86_64 referee in `docs/verification/V25.md` and
   `data/costs/costs.csv`.
3. **Quarter-plate tangent junction (`V3x`).** Diagnose the `V3` quarter-plate (`x ≥ 0, y ≥ 0`)
   mouth-junction mesh defect at the `x = 0` hole tangency, fix the boundary-claim bisection in
   `fieldsmith-mesh`, and run the 5-row mesh study (`h = 0.2, 0.14, 0.1, 0.07, 0.05 mm`)
   recorded in `docs/verification/V3.md`.
4. **Mesh-leaf graded fill in PrusaSlicer (`Sx`).** Locate the manifold boundary for graded TPMS
   fill on a mesh/STEP-imported leaf across a grid of `skin` (`0 / 0.6 / 1.2 mm`) and cell size
   using headless PrusaSlicer 2.9.6 (`--info`), diagnose why the `-skin` offset on a trilinear
   mesh-leaf SDF produces open/non-manifold seams in adaptive dual contouring (`V14.md`), and
   either fix the export stitch to reach `manifold = yes` at `skin = 1.2 mm` or bound and
   document the exact geometric limit in `docs/verification/slicer.md`.
5. **N100 Chrome wall-time split (`M21`).** Run a controlled factorial experiment on the N100
   box (`ian-quieter.lan`) testing the hypotheses behind the M13 Chrome wall-time shift
   (`4.9 / 15.1 s` vs. `9.1 / 24.1 s` while native CLI and Firefox held steady) and record the
   measured cause or falsified hypotheses in `docs/verification/budgets.md`.

---

## Acceptance criteria

### A. Standalone repository & fresh-clone gate (M17 — closed at `7e5c1db`, [V24.md](docs/verification/V24.md))
- [x] Annotated tag `v0.4.0` at `295e80e` recording verified results and stated limits.
- [x] `fenics-rs` linked as a git submodule (`14b0dc8`, `exclude = ["fenics-rs"]` in the root
      workspace so nested workspace inheritance resolves cleanly).
- [x] Tree scanned for credentials/tokens prior to push; pushed `main` and tags (`v0.2.0`,
      `v0.4.0`) to `GlassOnTin/fieldsmith`.
- [x] `scripts/v24-fresh-clone.sh` runs in `scripts/ci.sh`: clones into a temporary directory,
      builds offline from the clone alone, and verifies byte-identical canonical output on the
      pinned `V22` golden request (`104 s` clone-to-answer on the referee).

### B. Cross-architecture `aarch64` datapoint (M18 — closed at `cfee968`, [V25.md](docs/verification/V25.md))
- [x] Cross-compiled static `aarch64-unknown-linux-musl` `fieldsmith` binary executed on the
      OnePlus 13 (`SM8750`, 8 cores, 15.8 GB RAM) inside Haven's Debian `arm64` guest.
- [x] Reference tolerance ladder and `V23` 5-point cantilever sweep measured (`1966 points/hour`
      vs. `≥ 100 points/hour` target; 1.16×–1.97× wall ratio against the Ryzen 9 9950X referee).
- [x] All `259` tracked scalar fields compared against x86_64 (`scripts/v25-arm-compare.py`):
      mesh counts and DOFs match identically; raw `faer` SIMD solves diverge by `1e-13..3.3e-11`
      relative on primary fields (`7.2e-8` relative / `3.2e-12` absolute on small rung-change
      differences); `259 / 259` fields tie under the 6-sig-fig `V22` canonical quantisation.
- [x] 13 `SM8750` rows added to `data/costs/costs.csv` and pinned by `V18`
      (`v18_costs_csv_ties_to_ship_data`, 59 total rows); `README.md` updated with measured
      cross-architecture divergence scope.

### C. Quarter-plate tangent junction (M19 — closed, [V3.md](docs/verification/V3.md))
- [x] **Diagnose from mesh output and fix boundary-claim bisection (`f620a91`).** Reconstructed
      the quarter plate (`crates/fieldsmith-verify/examples/v3quarter.rs`) and isolated two
      mechanisms in `fieldsmith-mesh`:
      1. `positive_boundary` bisected on `raw > tiny`, pinning C2 zero-claims to the `tiny`
         band edge (`c_y = tiny/slope = −8.28e-10`) and emitting planar slivers (`6.75e-11 · h³`)
         that prevented solver convergence on `extend_face_region`.
      2. Composite fields carry `±ulp` coordinate noise (`5.6e-17` to `3.6e-15`) on zero
         patches, causing a `raw > 0` predicate to collapse the bisection to the bracket end
         and drop the `(0, 1)` mouth apex node (`0.013 · h` miss).
      Fixed by bisecting `positive_boundary` on `raw(p) > NOISE` (`NOISE = 1e-14`, 60
      iterations), covered by `crates/fieldsmith-mesh/tests/mouth_junction.rs` and
      `cap_chime.rs`.
- [x] **Complete the 5-row quarter-model mesh study (`h = 0.2, 0.14, 0.1, 0.07, 0.05 mm`).**
      Both the half-plate and quarter-plate 5-row studies now run in
      `crates/fieldsmith-verify/tests/v3_plate_hole.rs` and emit full tables into
      `docs/verification/V3.md`. The quarter model's graze peaks dropped from `45.48 / 45.59 MPa`
      (`+50 %`) to `29.322 MPa` (`Kt = 2.6390`, `−3.30 %` at `h = 0.14`) and `32.399 MPa`
      (`Kt = 2.9159`, `+6.85 %` at `h = 0.07`, inside the `8.5 %` graze bound), with `0`
      off-plane pinned nodes and the mouth apex placed at `(0, 1, z)`.
- [x] **Explain and bound the remaining `+0.95 %` half-vs-quarter offset at `h = 0.05`.** Across
      the four spacings that share the same peak centroid element (`h = 0.2, 0.1, 0.07, 0.05`),
      the quarter model reads a uniform `+0.93 %` to `+0.98 %` above the half model (`+1.59 %`,
      `+1.47 %`, `+6.85 %`, `+5.39 %` vs. `+0.60 %`, `+0.53 %`, `+5.85 %`, `+4.40 %`) because
      `ux = 0` on `x = 0` models symmetric uniform traction on both `x = ±10`, whereas the half
      model constrains the entire `x = −10` end flat (`ux = 0`) and pulls `x = +10`. Documented
      in `V3.md` and pinned at `5.5 %` (`KT_TOL_QUARTER_FINE`) in `v3_plate_hole.rs`.

### D. Mesh-leaf graded fill reaches the slicer (M20 — closed, [slicer.md](docs/verification/slicer.md))
- [x] **Map the manifold boundary on a mesh-imported leaf.** Ran the 5 × 3 grid over
      `skin ∈ {0, 0.3, 0.6, 0.9, 1.2 mm}` and adaptive DC spacing `h ∈ {0.30, 0.25, 0.15 mm}`
      on `c01x-composite.fsmz` (`slicer.md`) and recorded `TriMesh::defects()` alongside
      headless PrusaSlicer 2.9.6 (`--info`) verdicts.
- [x] **Diagnose and fix the three adaptive-stitch causes (`crates/fieldsmith-mesh/src/adaptive.rs`).**
      1. *Sub-tolerance `f32` corner crossings on planar offset faces (`216` apparent open edges
         from `140` degenerate triangles at `h = 0.30 mm`):* clamped sign-changing edge crossings
         into `[FOLD_NUDGE, 1.0 - FOLD_NUDGE]` (`[1e-4, 0.9999]`), upholding the corner-clearance
         invariant across `f32` mesh-grid evaluation noise (`~1e-8..3e-7 mm`).
      2. *Post-weld `[u, v, v]` collapsed triangles (`4 / 14 / 42` degenerate triangles at
         `skin = 0.0`, causing PrusaSlicer `manifold = no, open_edges = 2`):* dropped
         repeated-vertex triangles immediately after `weld_exact`.
      3. *Per-component cavity orientation (`+18.02 %` volume jump at `h = 0.25 mm`, where a
         crease seed inverted one `325 394`-triangle interior gyroid labyrinth from
         `−1 088.3 mm³` to `+1 088.3 mm³`):* oriented each edge-connected component by majority
         vote of its build-time (`emit_quad`) normals in `orient_inner`.
- [x] **Close with measured `manifold = yes` across all 15 cells (`skin = 0..1.2 mm`,
      `h = 0.30..0.15 mm`).** Every cell in the 5 × 3 grid now reports `0` open edges, `0`
      degenerate triangles, `0` orientation defects, and PrusaSlicer `manifold = yes`;
      `skin = 1.2 mm` slices at `0.2 mm` layer height (`--export-gcode`) to `11 MB` G-code with
      zero repair diagnostics (`slicer.md`, `V14.md`), and regression tests are locked in
      `adaptive.rs` and `v14_step.rs`.


### E. N100 Chrome empirical compute profile (M21 — evidence: appendix in [budgets.md](docs/verification/budgets.md))
- [ ] **Measure controllable runtime factors on `ian-quieter.lan`.** On the 4-core N100, Chrome
      (using software SwiftShader WebGL) recorded two distinct wall bands during M13
      (`4918 / 15064 ms` vs. `~9100–9700 / 24100–25000 ms` coarse / refined) while native CLI
      (`569 / 2643 ms`) and Firefox (`4526 / 16068 ms`) stayed within `0.4–2.8 %`. Measure a
      compact factorial run set on the N100 covering:
      1. *Main-thread SwiftShader WebGL rAF contention*: active 60 Hz software raymarching vs.
         paused/throttled preview while the 4-core Web Worker rayon pool solves.
      2. *Chromium profile / shader cache state* (warm vs. fresh profile).
      3. *Thermal / package power state* across back-to-back runs.
- [ ] **Record the empirical compute band in `budgets.md` and `data/costs/costs.csv`.** Add a
      concise appendix to `docs/verification/budgets.md` (and `budgets-laptop-chrome.md`)
      stating the measured walls across those conditions (and keep any rAF throttling improvement
      if WebGL contention is significant on 4 cores).

### F. Release hygiene & carried items
- [ ] `scripts/ci.sh` passes end-to-end across `V1`–`V24` plus `V25`'s portable tie-in assertions.
- [ ] `README.md` status header updated from `v0.2.0` to match the current release and reflects
      the updated `V3`, `slicer.md`, and `budgets.md` findings without unmeasured claims.

---

## Verification cases

Cases `V1`–`V23` run unchanged in CI. New and extended cases for v0.5:

| ID | Case | Reference | Acceptance bar |
| :-- | :-- | :-- | :-- |
| `V24` | Fresh-clone build & golden API contract (`scripts/v24-fresh-clone.sh`) | `V22` golden canonical response | Byte-identical canonical output from a standalone clone; passes in CI (**measured: PASS, 104 s**) |
| `V25` | `aarch64` (`SM8750` OnePlus 13) ladder + `V23` 5-point sweep (`scripts/v25-arm-compare.py`) | x86_64 referee envelopes & `V18` `costs.csv` tie-in | Per-field divergence measured and documented; 259/259 canon-equal; sweep `≥ 100 pts/h` (**measured: PASS, 1966 pts/h**) |
| `V3x` | `V3` quarter-plate (`x ≥ 0, y ≥ 0`) 5-row mesh study | Heywood finite-width `Kt = 2.729` & half-plate study | 5-row quarter table in `V3.md` (**measured: PASS, +1.59 / −3.30 / +1.47 / +6.85 / +5.39 %**) |
| `Sx` | Mesh-leaf graded-fill slicer manifold grid | Headless PrusaSlicer 2.9.6 `--info` (`manifold = yes`) | `manifold = yes` at `skin = 1.2 mm` on mesh-leaf graded part (**measured: PASS across all 15 cells, `0` open edges, slices at `0.2 mm` to `11 MB` G-code**) |

---

## Performance & quality budgets

| Metric | Target | Status |
| :-- | :-- | :-- |
| Fresh clone → build → golden API answer (`V24`) | `≤ 5 min` on referee | **104 s** (clone 2 s + build 98 s + run 2 s) — PASS |
| `SM8750` (`aarch64`) 5-point sweep throughput (`V25`) | `≥ 100 points/hour` | **1966 points/hour** (9.16 s engine sum across 5 points) — PASS |
| `V3x` quarter-plate peak stress error (`h = 0.2..0.05`) | `≤ 5 %` (`≤ 8.5 %` graze, `≤ 5.5 %` at `0.05`) | **+1.59 / −3.30 / +1.47 / +6.85 / +5.39 %** (`V3.md`) — PASS |
| Mesh-leaf graded fill in PrusaSlicer (`Sx`) | `manifold = yes` at `skin = 1.2 mm` | **`manifold = yes` across all 15 cells (`skin = 0..1.2 mm`, `h = 0.30..0.15 mm`), `0` open edges** (`slicer.md`) — PASS |
| N100 Chrome compute profile (`M21`) | Empirical walls measured across WebGL/worker conditions | Pending M21 run on `ian-quieter.lan` |

---

## Settled policy decisions (from v0.4)

1. **N100 rows are empirical compute measurements, not pass/fail workstation gates:** The
   32-thread workstation (`budgets.md`) is the pass/fail referee for the interactive browser
   targets (`1.5 s` coarse / `10 s` refined). Secondary hardware (`N100` x86_64 mini-PC,
   `SM8750` `aarch64` phone) exists to populate `data/costs/costs.csv` and `budgets-laptop-*.md`
   with real empirical compute walls across machine classes.
2. **Default `750k`-DOF ladder cap & refusal contract:** The default 3-rung schedule
   (`30k → 150k → 750k` DOFs) stays capped at `750k` DOFs so browser WASM (`4 GB` `wasm32`
   address limit) and small-RAM hosts never OOM by default. When a part's geometry requires
   more than `750k` DOFs to bring rung-to-rung change inside a tight declared tolerance (such as
   `1 %` on the unfilleted holes of `bracket.fsm`, which reaches `−4.1 %` at `750k` DOFs in
   `10.9 s`), returning an explicit refusal with the measured residual satisfies the contract;
   users on native CLI or remote workers can pass `--rungs` to climb higher when desired.

---

## Build order

| # | Milestone | Status | Deliverable |
| :-- | :-- | :-- | :-- |
| **M17** | Standalone repo & fresh-clone gate | **Done** (`7e5c1db`) | `fenics-rs` submodule, `GlassOnTin/fieldsmith` remote, `V24.md`, CI gate |
| **M18** | `aarch64` (`SM8750`) architecture datapoint | **Done** (`cfee968`) | `V25.md`, `scripts/v25-arm-compare.py`, 13 `SM8750` rows in `costs.csv` |
| **M19** | Quarter-plate tangent junction | **Done** (`38495e8`) | Mesher fix (`f620a91`) + 5-row quarter table in `V3.md` and `v3_plate_hole.rs` |
| **M20** | Mesh-leaf graded fill in PrusaSlicer | **Done** | 5 × 3 manifold grid + adaptive crossing/weld/component-orient fix + `slicer.md` & `v14_step.rs` |
| **M21** | N100 Chrome compute profile | **Open** | Empirical WebGL/worker run set on `ian-quieter.lan` + appendix in `budgets.md` |

---

## Explicitly out of scope for v0.5

- Flipping `GlassOnTin/fieldsmith` from private to public or enabling billed GitHub Actions
  runners (publishing is the owner's decision; CI runs locally via `scripts/ci.sh`).
- Publishing `fenics-rs` to `crates.io` (consumed via git submodule).
- New physics flows, viewport section-plane clipping, `.fsm` variables, or model-tree part
  groups (scheduled for v0.6+ per [VISION §7](VISION.md#7-competitive-landscape-and-roadmap-direction)).
- Any Fieldsmith-hosted compute, relay, or telemetry service.