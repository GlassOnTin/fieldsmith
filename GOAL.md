# Fieldsmith — Goal (v0.8: Coupon-Fitted Yield/Buckling Laws, Direct Slice Stacks, Stress-Driven Skins & Agentic Synthesis)

This file defines **what "done" means for v0.8**. The [VISION](VISION.md) defines *why* and
*where next*; this file defines the concrete, testable acceptance bar for the current release.
Previous goals are preserved in git history (`v0.2.0` at `a52b6b8`, `v0.3` at `0b92e1c`,
`v0.4.0` at `295e80e`, `v0.5.0` at `ab59756`, `v0.6.0` at `d3cdd37`, and `v0.7.0` at `d806c55`,
summarised in the appendix below). If an acceptance criterion or tolerance turns out to be wrong,
update this file in a reviewed commit with the measured reason rather than skipping it.

---

## The goal in one sentence

> An engineer can evaluate graded TPMS parts against per-family coupon-fitted yield and buckling
> laws (`V48`), preview and export 2D Z-contour slice stacks (3MF Slice Extension) directly from
> the implicit field without building a 3D triangle mesh (`V49`), thicken outer load-bearing
> shells in proportion to the solved stress field (`fill ... skin_max=<mm>`, `V50`), and run a
> closed-loop agentic design synthesis pass through `fieldsmith-mcp` to meet explicit structural
> and mass targets (`V51`).

---

## Settled policy decisions

1. **No Engine Islands:** No solver module or verification case (`V#`) lands unless it is either
   (a) reachable end-to-end from a `.fsm` document in both `fieldsmith-cli` and the browser UI,
   or (b) directly calibrating a coefficient table consumed by `.fsm` solves (as `V6`, `V36`,
   and `V48` calibrate `fill.rs`). Standalone solver modules exercised only by a synthetic test
   are out of bounds.
2. **Pixels and first-user paths, not state-only assertions:** Every UI control added in v0.8
   joins the `V39` feature-combination pixel matrix (`web/tests/combinations.mjs`), surfaces
   any rendering or empty-field failure through `ui.renderError` (`V40`), and keeps the
   fresh-clone README new-user gate (`V41`) green.
3. **Report immutability on routine test runs (`FIELDSMITH_BLESS`):** `cargo test -p fieldsmith-verify`
   checks every tolerance on every run but only overwrites existing `docs/verification/V*.md`
   files when `FIELDSMITH_BLESS=1` is set (or when a new report is created or a check fails),
   keeping the working tree clean of wall-time jitter.
4. **Carried v0.4/v0.5 policies remain in force:** The 32-thread workstation (`budgets.md`) is
   the pass/fail interactive referee; secondary hardware (`N100`, `SM8750`) populates empirical
   rows in `data/costs/costs.csv`; and the default browser ladder stays capped at `750k` DOFs
   with explicit refusal when a tighter tolerance requires `--rungs`.

---

## The reference task

1. **Coupon-fitted per-family TPMS yield and buckling laws (`M26`, `V48`).** Extend the
   resolved-FE unit-cell coupon harness (`fieldsmith_verify::coupon`) to measure the interior
   95th-percentile von Mises stress concentration `k_y(ρ) = σ_vm,95 / |Σ|` (giving initial
   yield `σ_y*(ρ) / σ_ys = 1 / k_y(ρ)`) and the feature-slenderness / compressive-concentration
   elastic buckling ratio `σ_el*(ρ) / E_s` across all six `(Family, FillMode)` classes. Fit
   per-family power laws `c_y · ρ^{n_y}` and `c_b · ρ^{n_b}`, wire them into
   `strength_extended(family, mode, yield_mpa, phi)` and
   `buckling_extended(family, mode, young_mpa, phi)` in `fill.rs`, expose the active law
   constants in `GradedOut::metaJson` and `Results`, and verify in `V48` (plus `V37`/`V38`).
2. **Direct field slice stacks & 3MF Slice Extension (`M27`, `V49`).** Implement 2D marching
   squares with gradient root polishing and closed-polygon contour stitching (`slice_field` and
   `slice_stack` in `fieldsmith-mesh`), plus a 3MF Slice Extension writer and reader
   (`write_3mf_slicestack` / `read_3mf_slicestack`). Expose `--slices` in `fieldsmith export`,
   `sliceFieldAt` / `exportSliceStack3mf` in `fieldsmith-wasm`, and instant field-direct layer
   preview plus `Download Slice 3MF` in `Make` without requiring a 3D mesh export first (`V49`).
3. **Field-native stress-driven shell thickening (`M28`, `V50`).** Add `skin_max=<mm>` to the
   `fill` statement in `.fsm` so the outer solid skin thickness `skin(p) ∈ [skin, skin_max]`
   scales continuously with the solved stress field alongside the interior lattice grading across
   the CPU evaluator, the GLSL raymarcher (`fs_field` and `fs_core`), the graded FEA re-solve,
   and the `Fill` lens inspector (`V50`).
4. **Closed-loop agentic synthesis via `fieldsmith-mcp` (`M29`, `V51`).** Extend `fieldsmith-mcp`
   with parametric design mutation tools (`set_let`, `set_fill`, `set_build`, and `sweep`
   support in `solve` plus `slices` in `export`) and verify an end-to-end closed-loop synthesis
   workflow in `V51`: start from an under-designed bracket `.fsm`, run an MCP sweep/solve,
   inspect yield, buckling, layer-peel, and overhang metrics, apply the Pareto-optimal
   parameters and build orientation through MCP tools, and export a verified 3MF meeting all
   stated structural and mass targets.

---

## Acceptance criteria

### A. Coupon-fitted TPMS yield & buckling laws (`M26` — `V48`)
- [x] `fieldsmith_verify::coupon` measures interior `vm_int_p95`, `yield_ratio`, `comp_int_p95`,
      and `buckling_ratio` on resolved unit-cell coupons.
- [x] `fieldsmith_core::fill` replaces the scalar open-cell placeholders (`0.3 · ρ^1.5` and
      `0.05 · ρ^2`) with per-`(Family, FillMode)` coupon-fitted power laws `strength_law` and
      `buckling_law`, exposed in `GradedOut::metaJson` and `fieldsmith-api`.
- [x] `V48` verifies the fitted yield and buckling laws across all six TPMS classes and confirms
      that stretching-dominated sheets vs. bending-dominated networks produce the expected
      directional shift in `yield_utilization` and `buckling_utilization` on a `.fsm` solve.

### B. Direct field slice stacks (`M27` — `V49`)
- [x] `fieldsmith_mesh::slice` extracts closed, consistently oriented 2D polygon contours
      directly from `f(x, y, z) = 0` and writes/reads 3MF Slice Extension (`<s:slicestack>`)
      archives.
- [x] `fieldsmith-cli` (`export --slices`), `fieldsmith-wasm`, and the `Make` lens preview and
      export slice stacks directly from the implicit field without building a 3D triangle mesh.
- [x] `V49` verifies closed-form slice areas on a sphere (`π(R² − z²)` within `1 %`), closed
      polygon loops (`0` open chains) on a graded lattice bracket, 3MF `<s:slicestack>`
      round-trip, and `Make` slice preview pixel updates (`V39`).

### C. Stress-driven outer shell thickening (`M28` — `V50`)
- [x] `.fsm` parser and printer round-trip `fill ... skin_max=<mm>` (`V9` extended), with
      validation requiring `skin_max >= skin`.
- [x] `FillExpr` (CPU) and `glsl.rs` (`fs_field` and `fs_core`) interpolate local skin thickness
      `skin(p) ∈ [skin, skin_max]` from the solved stress parameter grid `u_param`, preserving
      GPU/CPU `V16` agreement.
- [x] `V50` verifies that setting `skin_max > skin` on a loaded cantilever/bracket thickens the
      high-stress root skin in `FillExpr`, increases integrated mass, reduces graded compliance
      and peak deflection in the FEA re-solve, and visibly updates viewport pixels in `Fill`
      (`V39`).

### D. Closed-loop agentic synthesis via `fieldsmith-mcp` (`M29` — `V51`)
- [x] `fieldsmith-mcp` exposes `set_let`, `set_fill`, `set_build`, `solve` (`kind: "sweep"`),
      and `export` (`slices: true`) over stdio JSON-RPC.
- [x] `V51` drives an automated closed-loop synthesis session over `fieldsmith-mcp`: starting
      from an initial `.fsm` design that violates yield/peel targets, the loop queries `solve` /
      `sweep`, updates `let` variables, lattice rotation/skin, and build orientation `(θ, φ)`,
      verifies the refined design meets all safety-factor, layer-peel, overhang, and mass gates,
      and exports a defect-free 3MF.

---

## Build order

| # | Milestone | Status | Deliverable |
| :-- | :-- | :-- | :-- |
| **M26** | Coupon-fitted TPMS yield & buckling laws | Done (`e8765f3`) | Per-family yield/buckling laws in `fill.rs` + `coupon.rs` + `V48.md` |
| **M27** | Direct field slice stacks (3MF Slice Ext) | Done (`0f4d75a`) | `fieldsmith_mesh::slice` + CLI `--slices` + `Make` field slice preview/export + `V49.md` |
| **M28** | Stress-driven outer shell thickening | Done (`76f9a50`) | `fill ... skin_max=` across `.fsm`, GLSL, CPU, solver, and `Fill` UI + `V50.md` |
| **M29** | Closed-loop agentic synthesis (`mcp`) | Done (`cf724d3`) | `set_let`/`set_fill`/`set_build`/sweep in `fieldsmith-mcp` + closed-loop `V51.md` |

---

## Explicitly out of scope for v0.8

- Standalone continuum solvers without a `.fsm` workflow (such as displacement-only J2 shear
  plasticity or Nédélec RF/acoustic edge elements).
- Publishing `fenics-rs` to `crates.io` (consumed via git submodule).
- Any Fieldsmith-hosted compute, relay, or telemetry service.

---

## Appendix: v0.6 (`V26`–`V43`), v0.7 (`V44`–`V47`), & v0.8 (`V48`–`V51`) Closure Summary

All 26 verification cases added across `v0.6.0` (`d3cdd37`), `v0.7.0` (`d806c55`), and `v0.8.0` pass in CI:

| ID | Capability delivered | Release / Commit | Report |
| :-- | :-- | :-- | :-- |
| `V26` | Interactive GPU section-plane clipping (`x`/`y`/`z`, flip, `0..1` slider, mid-part default) | v0.6 (`3a5056c`) | [V26.md](docs/verification/V26.md) |
| `V27` | Named `let` variables, arithmetic expressions, and bare-name parameter sweeps | v0.6 (`2c52bfe`) | [V27.md](docs/verification/V27.md) |
| `V28` | Modal mode-shape animation (`play`/`pause`/phase slider, `2π` & `Off` bit-exact) | v0.6 (`62bea33`) | [V28.md](docs/verification/V28.md) |
| `V29` | Robin convective thermal boundary conditions (`convection ... h= tinf=`) | v0.6 (`549aca5`) | [V29.md](docs/verification/V29.md) |
| `V30` | Prescribed Neumann heat-flux boundary conditions (`heatflux ... q=`) | v0.6 (`755d6b8`) | [V30.md](docs/verification/V30.md) |
| `V31` | In-UI 2D profile sketching on the Extrude inspector | v0.6 (`1023d3d`) | [V31.md](docs/verification/V31.md) |
| `V32` | Two-domain TPMS core thermal conduction re-solve (`conductivity_of(class, φ)`) | v0.6 (`1b4d6df`) | [V32.md](docs/verification/V32.md) |
| `V33` | `Results` lens surfacing and envelope integration of graded thermal cases | v0.6 (`eee8985`) | [V33.md](docs/verification/V33.md) |
| `V34` | Certified compliance lower bound (`C_h = fᵀu`) across static and thermal cases | v0.6 (`ebaef72`) | [V34.md](docs/verification/V34.md) |
| `V35` | Witness-constrained equilibrated P2 stress recovery upper bound (`C(σ*)`) | v0.6 (`e943b5a`) | [V35.md](docs/verification/V35.md) |
| `V36` | Rotated unit-cell coupon measurement of TPMS directional anisotropy (`45°/0°`) | v0.6 (`9de699c`) | [V36.md](docs/verification/V36.md) |
| `V37` | Per-case Gibson–Ashby yield-utilization factor in graded solves and `Results` | v0.6 (`1ed52b5`) | [V37.md](docs/verification/V37.md) |
| `V38` | Per-case Gibson–Ashby elastic-buckling load factor in graded solves and `Results` | v0.6 (`1980f67`) | [V38.md](docs/verification/V38.md) |
| `V39` | Feature-combination pixel matrix (`overlays × document kinds × lenses`) | v0.6 (`c5f16ed`) | [V39.md](docs/verification/V39.md) |
| `V40` | Loud rendering failure banner and detectors (shader link, empty surface, mesh throw) | v0.6 (`80cf98a`) | [V40.md](docs/verification/V40.md) |
| `V41` | Fresh-clone headless new-user gate driving the README's exact commands | v0.6 (`69a03ff`) | [V41.md](docs/verification/V41.md) |
| `V42` | Per-element cubic anisotropic stiffness kernel (`ortho.rs`) and corrected `[110]` fit | v0.6 (`38e95de`) | [V42.md](docs/verification/V42.md) |
| `V43` | Cubic compliance Gram equilibration certifying the upper bound on cubic elements | v0.6 (`e283ba1`) | [V43.md](docs/verification/V43.md) |
| `V44` | Rotated TPMS lattice alignment (`fill ... rotate=(rx, ry, rz)`) across field, solver, and UI | v0.7 (`3c3026e`) | [V44.md](docs/verification/V44.md) |
| `V45` | Exact-gradient overhang analysis (`n(p) = ∇f/‖∇f‖`) in CLI, API, and `Make` overlay | v0.7 (`0eeb0b2`) | [V45.md](docs/verification/V45.md) |
| `V46` | Orthotropic inter-layer peel/shear utilization and build-orientation sweep `(θ, φ)` | v0.7 (`0456a86`) | [V46.md](docs/verification/V46.md) |
| `V47` | Interactive 1D/2D `let` Pareto frontier explorer in `Results` with click-to-apply & Undo | v0.7 (`2c44e7d`) | [V47.md](docs/verification/V47.md) |
| `V48` | Coupon-fitted per-family TPMS yield and buckling laws across all six `(Family, FillMode)` classes | v0.8 (`e8765f3`) | [V48.md](docs/verification/V48.md) |
| `V49` | Direct field slice stacks (`slice_field`, `slice_stack`) & 3MF Slice Extension (`<s:slicestack>`) | v0.8 (`0f4d75a`) | [V49.md](docs/verification/V49.md) |
| `V50` | Field-native stress-driven outer shell thickening (`fill ... skin_max=<mm>`) | v0.8 (`76f9a50`) | [V50.md](docs/verification/V50.md) |
| `V51` | Closed-loop agentic synthesis via `fieldsmith-mcp` (`set_let`, `set_fill`, `set_build`, `sweep`) | v0.8 (`cf724d3`) | [V51.md](docs/verification/V51.md) |
