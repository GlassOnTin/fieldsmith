# Fieldsmith — Goal (v0.7: Additive-Native Orientation, Lattice Alignment & Pareto Exploration)

This file defines **what "done" means for v0.7**. The [VISION](VISION.md) defines *why* and
*where next*; this file defines the concrete, testable acceptance bar for the current release.
Previous goals are preserved in git history (`v0.2.0` at `a52b6b8`, `v0.3` at `0b92e1c`,
`v0.4.0` at `295e80e`, `v0.5.0` at `ab59756`, and `v0.6` closed across `V26`–`V43` at
`e283ba1`, summarised in the appendix below). If an acceptance criterion or tolerance turns out
to be wrong, update this file in a reviewed commit with the measured reason rather than
skipping it.

---

## The goal in one sentence

> An engineer can orient a lattice's cubic-anisotropic axes (`fill ... rotate=(rx, ry, rz)`)
> independently of the part (`V44`), evaluate exact-gradient overhang bounds and inter-layer
> bond peel/shear utilization across build orientations `(θ, φ)` (`V45`, `V46`), and explore a
> multi-parameter sweep's Pareto frontier interactively in `Results` where clicking a point
> snaps the 3D viewport and `.fsm` document to that candidate (`V47`).

---

## Settled policy decisions

1. **No Engine Islands:** No solver module or verification case (`V#`) lands unless it is either
   (a) reachable end-to-end from a `.fsm` document in both `fieldsmith-cli` and the browser UI,
   or (b) directly calibrating a coefficient table consumed by `.fsm` solves (as `V6` and `V36`
   calibrate `fill.rs`). Standalone solver modules exercised only by a synthetic test are
   out of bounds.
2. **Pixels and first-user paths, not state-only assertions:** Every UI control added in v0.7
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

1. **Rotated TPMS lattice alignment (`M22`, `V44`).** Add `rotate=(rx, ry, rz)` (degrees, ZYX
   Euler matching primitive rotation) to the `fill` statement in `.fsm`, the GLSL raymarcher,
   the CPU field evaluator / adaptive dual contourer, the per-element rotated cubic stiffness
   tensor (`R · C · Rᵀ`), and the `Fill` lens inspector. Verify on a uniaxial bar that rotating
   a diamond-network or primitive-sheet fill by `45°` about Z reproduces the `V36` `[110]`
   modulus (`2.22×` / `2.07×` of `[100]`) and updates the raymarched preview pixels.
2. **Exact-gradient overhang map and orientation check (`M23`, `V45`).** Compute the exact
   outward surface normal `n(p) = ∇f(p) / ‖∇f(p)‖` across surface facets and in the shader for
   a declared build axis `b̂(θ, φ)` (default `+Z`, threshold `α_max = 45°` from build vertical).
   Expose overhang area fraction and worst overhang angle in `Make` / CLI (`--overhang`) and
   render the overhang highlight overlay in the viewport, verified against closed-form sphere
   and inclined-plane overhang areas (`V45`).
3. **Inter-layer peel and shear utilization (`M24`, `V46`).** Add build-axis layer-bond
   evaluation (`σ_nn = b̂ᵀ σ b̂` inter-layer tension and `τ_nt = ‖σ b̂ − σ_nn b̂‖` inter-layer
   shear, combined via a quadratic layer-bond criterion `(max(0, σ_nn) / S_z)² + (τ_nt / S_s)²`)
   with material `yield_z=` / `shear_z=` parameters (or a stated default z-bond knockdown ratio
   when omitted). Expose the `layer-peel` mode in `Results` and a build-orientation sweep over
   `(θ, φ)` that identifies the orientation minimizing peak layer-peel utilization (`V46`).
4. **Interactive Pareto frontier explorer in `Results` (`M25`, `V47`).** Extend the `Results`
   sweep runner to support 2-parameter `let` grids (or multi-point sweeps) and plot a 2D scatter
   frontier (`mass (g)` on X vs. `max von Mises`, `safety factor`, `compliance`, `layer-peel`,
   or `f₁` on Y) with Pareto-optimal points highlighted. Clicking a point in the plot updates
   the `.fsm` `let` bindings, re-renders the 3D viewport, and supports `Undo` back to the prior
   state (`V47`).

---

## Acceptance criteria

### A. Lattice axis rotation (`M22` — `V44`)
- [ ] `.fsm` grammar and printer round-trip `fill ... rotate=(rx, ry, rz)` (`V9` extended).
- [ ] CPU field evaluator, GLSL raymarcher (`fs_core`), and adaptive 3MF exporter apply the
      inverse rotation to lattice sample coordinates `p`, leaving outer body skins unrotated.
- [ ] `fieldsmith-solve` rotates each lattice element's cubic stiffness and compliance tensors
      by `R(rx, ry, rz)` during assembly, stress recovery, and `dual.rs` upper-bound
      equilibration.
- [ ] `V44` verifies that `fill ... rotate=(0, 0, 45)` on a `[100]`-loaded bar matches the
      unrotated `[110]` patch stiffness and equilibrated compliance bound within `0.1 %`, and
      that changing `rotate` in the `Fill` lens visibly changes viewport pixels (`V39`).

### B. Overhang analysis from `∇f(p)` (`M23` — `V45`)
- [ ] Surface overhang metric (`n(p) · (−b̂) > sin(α_max)`) implemented in `fieldsmith-core`,
      exposed in `fieldsmith check` / `fieldsmith api`, and rendered as an overlay in `Make`.
- [ ] `V45` verifies overhang area on a sphere (`r = 10 mm`, exact cap area
      `2π r² (1 − sin α_max)`) and a `60°` tilted plate within `1.5 %` at default mesh spacing,
      plus pixel-delta verification of the `Make` overhang overlay.

### C. Orthotropic layer-peel criterion & build orientation (`M24` — `V46`)
- [ ] `.fsm` material/solve statements accept build orientation `build=(θ, φ)` and inter-layer
      tensile/shear strengths (`yield_z`, `shear_z`).
- [ ] `fieldsmith-core` computes per-element and nodal layer-peel utilization alongside bulk
      von Mises; `Results` lens mode chip adds `Layer peel` and reports peak utilization and
      governing orientation.
- [ ] `V46` verifies exact closed-form layer-peel utilization on an off-axis uniaxial bar across
      `θ ∈ {0°, 30°, 45°, 60°, 90°}` (`σ_nn = σ₀ cos²θ`, `τ_nt = σ₀ sin θ cos θ`) to solver
      precision (`≤ 1e-6`), and verifies the `Results` lens ramp in headless e2e.

### D. Interactive Pareto explorer (`M25` — `V47`)
- [ ] `Results` sweep panel supports 1D or 2D `let` parameter sweeps and renders an SVG scatter
      plot (`mass` vs. selected metric) marking the non-dominated Pareto front.
- [ ] Clicking a plotted candidate updates the `.fsm` document's `let` bindings in a single
      reversible transaction, updates the 3D viewport, and restores bit-identically on `Undo`.
- [ ] `V47` drives a multi-point bracket sweep in headless Chromium, asserts the Pareto frontier
      markers match the computed non-dominated set, clicks a frontier point, asserts both `.fsm`
      source and viewport pixels update, and asserts `Ctrl+Z` restores the initial state.

---

## Build order

| # | Milestone | Status | Deliverable |
| :-- | :-- | :-- | :-- |
| **M22** | Lattice axis rotation (`fill rotate=`) | Planned | Rotated cubic tensor + GLSL/CPU lattice transform + `Fill` inspector + `V44.md` |
| **M23** | Exact-gradient overhang check | Planned | `∇f` overhang area & angle in CLI/API + `Make` viewport overlay + `V45.md` |
| **M24** | Inter-layer peel criterion & orientation | Planned | Build-Z peel/shear utilization + `Results` mode chip + orientation check + `V46.md` |
| **M25** | Interactive Pareto explorer in `Results` | Planned | 1D/2D `let` sweep frontier plot + click-to-apply + undo round-trip + `V47.md` |

---

## Explicitly out of scope for v0.7

- Standalone continuum solvers without a `.fsm` workflow (such as displacement-only J2 shear
  plasticity or Nédélec RF/acoustic edge elements).
- Publishing `fenics-rs` to `crates.io` (consumed via git submodule).
- Any Fieldsmith-hosted compute, relay, or telemetry service.

---

## Appendix: v0.6 Closure Summary (`ab59756` → `e283ba1`, `V26`–`V43`)

All 18 verification cases added after `v0.5.0` pass in CI:

| ID | Capability delivered in v0.6 | Commit | Report |
| :-- | :-- | :-- | :-- |
| `V26` | Interactive GPU section-plane clipping (`x`/`y`/`z`, flip, `0..1` slider, mid-part default) | `3a5056c`, `8ea66f2` | [V26.md](docs/verification/V26.md) |
| `V27` | Named `let` variables, arithmetic expressions, and bare-name parameter sweeps | `2c52bfe` | [V27.md](docs/verification/V27.md) |
| `V28` | Modal mode-shape animation (`play`/`pause`/phase slider, `2π` & `Off` bit-exact) | `62bea33` | [V28.md](docs/verification/V28.md) |
| `V29` | Robin convective thermal boundary conditions (`convection ... h= tinf=`) | `549aca5` | [V29.md](docs/verification/V29.md) |
| `V30` | Prescribed Neumann heat-flux boundary conditions (`heatflux ... q=`) | `755d6b8` | [V30.md](docs/verification/V30.md) |
| `V31` | In-UI 2D profile sketching on the Extrude inspector | `1023d3d` | [V31.md](docs/verification/V31.md) |
| `V32` | Two-domain TPMS core thermal conduction re-solve (`conductivity_of(class, φ)`) | `1b4d6df` | [V32.md](docs/verification/V32.md) |
| `V33` | `Results` lens surfacing and envelope integration of graded thermal cases | `eee8985` | [V33.md](docs/verification/V33.md) |
| `V34` | Certified compliance lower bound (`C_h = fᵀu`) across static and thermal cases | `ebaef72` | [V34.md](docs/verification/V34.md) |
| `V35` | Witness-constrained equilibrated P2 stress recovery upper bound (`C(σ*)`) | `0bc3dcf`, `e943b5a` | [V35.md](docs/verification/V35.md) |
| `V36` | Rotated unit-cell coupon measurement of TPMS directional anisotropy (`45°/0°`) | `9eb2cb3`, `9de699c` | [V36.md](docs/verification/V36.md) |
| `V37` | Per-case Gibson–Ashby yield-utilization factor in graded solves and `Results` | `36139a2`, `1ed52b5` | [V37.md](docs/verification/V37.md) |
| `V38` | Per-case Gibson–Ashby elastic-buckling load factor in graded solves and `Results` | `1980f67` | [V38.md](docs/verification/V38.md) |
| `V39` | Feature-combination pixel matrix (`overlays × document kinds × lenses`) | `c5f16ed` | [V39.md](docs/verification/V39.md) |
| `V40` | Loud rendering failure banner and detectors (shader link, empty surface, mesh throw) | `80cf98a` | [V40.md](docs/verification/V40.md) |
| `V41` | Fresh-clone headless new-user gate driving the README's exact commands | `b07a1a0`, `69a03ff` | [V41.md](docs/verification/V41.md) |
| `V42` | Per-element cubic anisotropic stiffness kernel (`ortho.rs`) and corrected `[110]` fit | `7c1a662`..`38e95de` | [V42.md](docs/verification/V42.md) |
| `V43` | Cubic compliance Gram equilibration certifying the upper bound on cubic elements | `e283ba1` | [V43.md](docs/verification/V43.md) |
