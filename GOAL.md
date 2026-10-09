# Fieldsmith — Goal (v0.3)

This file defines **what "done" means for v0.3**. It is deliberately concrete and testable.
The [VISION](VISION.md) says *why* (section 7 records the competitive landscape this version
answers); this says *what.* The previous goal, with its close-out audit, is in git history at
`9606890` (v0.1 definition of done, M6/M7 user-set milestones, tag `v0.2.0`).

---

## The goal in one sentence

> A user drops a real CAD part (a STEP a mainstream tool exported, or a 3MF another tool
> wrote) into Fieldsmith, fixes and loads it, reads how the part will ring, fills it with a
> stress-graded lattice, and hands a slicer-clean 3MF to the printer — in a browser, with
> every number carrying a measured error.

---

## The reference task — "the CAD round-trip"

The acceptance test for v0.3, on a part Fieldsmith did not author:

1. Obtain the part (fixture ladder named in the cases; provenance stated in the report).
2. Import via CLI or the file palette; units and up-axis from the file, overridable.
3. Label faces; fix the mounting face, load the working face with 400 N at 15° outward.
4. Solve in PETG; probe stress and deflection.
5. Run modal: the first three natural frequencies, with provenance (element count, order,
   solve time, change since the previous refinement rung).
6. Fill: gyroid, cell 6 mm, skin 1.2 mm, density graded by envelope stress; mass in grams,
   solid vs graded side by side.
7. Export 3MF; PrusaSlicer reports `manifold = yes`; scrub slices at 0.2 mm.
8. Save as `.fsmz`, reload, re-solve, get bit-identical results.

**Pass:** the machine-side legs of steps 2–8 are covered by V12–V14 plus the carried ladder
(V1–V11, still in CI); the hallway leg is 3 out of 3 new users ≤ 5 minutes, as with v0.1.

---

## New acceptance criteria

### A. Modal (M8)
- [ ] Elastic mass matrix is assembled through `fenics-rs` (`assemble_elasticity_mass_3d`),
      and first modes via `solve_vibration_modes` — consumed as a dependency, not reimplemented.
- [ ] `fieldsmith solve --modes 3` prints the first three natural frequencies (Hz) with
      per-mode provenance; the web Results lens shows the first mode beside the static results.
- [ ] Modal takes the same region-based supports (fixed DOFs from the same masks); it needs
      no load input and states that in `--help`.
- [ ] V13 passes (below).

### B. 3MF beyond one object (M9)
- [ ] The reader accepts a model with several mesh objects: each becomes its own field leaf,
      named; malformed entries still fail with the existing strict, named errors.
- [ ] Our 3MF writer records every volume's material in `basematerials`.
- [ ] Per-volume material lives in the document model and `.fsm` text with inspector editing;
      `validate` names unknown materials and warns when a solve would see mixed volumes
      (analysis stays single-material — labelling is for the slicer, not for the stiffness).
- [ ] V12 passes (below).

### C. STEP in (M10)
- [ ] A maintained STEP reader crate is consumed as a dependency, not vendored or forked
      (candidates checked 2026-10-09: `step-io` 0.2.x, experimental, MIT/Apache; 
      `cadmpeg-codec-step` 0.5.x, Apache-2.0 — neither tessellates B-rep).
- [ ] Tessellation is ours: planes, cylinders, spheres, cones and tori are flattened from
      their stored analytic forms; B-spline faces are triangulated in parameter space; a face
      whose surfaces the reader cannot produce is refused by name (no silent drops, the
      V10 dirty-data lesson applied up front).
- [ ] Units come from the file's `SI_UNIT`; up-axis override as with mesh imports.
- [ ] The tessellated body enters the same voxelisation path as GLB/OBJ/STL/3MF: field leaf,
      patch labelling, face cones, region-driven extension, `.fsmz` sidecar, CLI `import`.
- [ ] V14 passes (below).
- [ ] If the reader crates stall, the honest fallback is recorded: accept only AP242 files
      that carry `tessellated_lods`, named as such in the case report — and if that is still
      undoable, this file is changed in a reviewed commit, not quietly skipped.

### D. Distributed loading (M11 — user-set, 2026-10-09; evidence: docs/verification/V15.md, `crates/fieldsmith-core/tests/distributed_load.rs`, CLI `solve --report` load table, the Setup-lens Fn editor)
- [x] Face-local coordinates for a labelled region: its facets fit a least-squares plane;
      local frame (x, y, n) with a deterministic rule for which in-plane direction is x
      (documented in the load module's header). The fit residual is reported with the
      load's provenance, and a region whose residual exceeds a stated bound is refused
      by name rather than silently flattened.
- [x] A distributed load compiles: chained band terms (`lo < x < hi -> value`) plus an
      arithmetic expression body over {x, y} (sin, cos, exp, polynomials), compiled once
      in the doc layer and evaluated at facet centroids; `.fsm` text round-trips losslessly.
- [x] Semantics: the function returns force per unit area (N/mm²) applied in a stated
      direction (global vector or components in {x, y, n}); the applied resultant (N) and
      moment (N·mm about the document origin) are computed and shown with the load's
      provenance; `total=` optionally rescales the function to a stated resultant.
- [x] The solver consumes it as consistent nodal loads through the same area-weighted
      path `press` already uses; no per-node special cases. (constant density matches a
      `force=` load to 1e-9 relative at the nodes)
- [x] The Setup lens gains a distribution editor: the text body with a parsed preview of
      F(x) over the band extent and the integrated total; `validate()` errors surface
      specifically (unknown variable, non-finite value, overlapping bands).
- [x] V15 passes (below): deflections within 0.4 % of the Euler–Bernoulli references
      (tolerance 15 %), resultants within 0.5 % (tolerance 2 %), fit RMS ~1e−13 mm on the
      flat face.

### E. Carried engineering criteria (already shipped, audited in git at `9606890`)
- [x] Cargo workspace + TypeScript web app, Vite, static hosting; `fenics-rs` as a dependency;
      everything headless via the CLI; small wasm surface; workers with transferables; CI runs
      the whole ladder. For v0.3 the CI row widens to V1–V14 and gains the modal and STEP stages.
- [x] README states limitations plainly — for v0.3 it gains: modal is linear (small strain),
      frequencies from inverse iteration with mode shapes indicative; per-volume materials are
      recorded for the slicer while analysis remains single-material.

---

## Verification cases (must pass in CI)

Cases V1–V11 are the audited ladder and run in CI unchanged (their definitions, references
and tolerances are in git history at `9606890`). The new cases:

| ID | Case | Reference | Tolerance |
| :-- | :-- | :-- | :-- |
| V12 | Import a 3MF written by another tool (provenance ladder: a 3MF Consortium public sample; else an export from desktop CAD available here; else the gap is stated, not faked), compose it, solve on labelled faces, re-export | The file's own geometry; PrusaSlicer `manifold` | Volume ≤ 2 % of the slicer's measurement; slicer-clean |
| V13 | Modal on a cantilever 100 × 10 × 10 mm, steel: first three modes vs Euler-Bernoulli `ωn = (βnL)² √(EI/(ρAL⁴))`, βnL = 1.8751, 4.6941, 7.8548 | Closed form | ≤ 10 % (f1), ≤ 15 % (f2, f3); ≤ 10 % change between the two finest refinement rungs; the measured shear/rotary gap is documented in the report |
| V14 | Import a mainstream-CAD STEP (fixture ladder: NIST CTC test files, else any local desktop-CAD export, else state the gap), voxelize, label faces, re-export | The B-rep's analytic volume; PrusaSlicer `manifold` | Analytic-surface fixtures: volume ≤ 1 %, 0 open edges, slicer-clean; name the tessellation chord tolerance used; cone coverage of three named faces ≥ 85 % |
| V15 | Distributed load on the cantilever: triangle profile q(x) = q₀·x/L on the top face (zero at the clamp, peak at the tip) and a two-band piecewise profile | Euler-Bernoulli δ_tip = 11 q₀ L⁴/(120 EI); analytic resultant and moment of the applied profile | Deflection ≤ 15 %; resultant force and moment ≤ 2 %; the fitted plane's residual and the integrated total appear in the report |

---

## Performance budgets

Carried from v0.1 (measured on the workstation Chromium referee; the laptop and Firefox
numbers remain named-but-unmeasured from v0.1) plus two provisional rows, to be confirmed
after first measurement:

| Metric | Budget |
| :-- | :-- |
| Preview frame rate, bracket with gyroid fill, 1080p | ≥ 60 fps |
| Edit → coarse stress result visible | ≤ 1.5 s |
| Edit → refined result (P2, ≈ 150 k DOF) | ≤ 10 s |
| Export 3MF of graded bracket at 0.1 mm feature resolution | ≤ 15 s |
| Cold load, compressed transfer size | ≤ 3 MB |
| Main thread long tasks during solve | none > 50 ms |
| Modal: first 3 modes bracket-scale | ≤ 30 s (provisional) |
| STEP: parse + tessellate a ≤ 50 MB file | ≤ 30 s (provisional) |

---

## Build order

| # | Milestone | Done when |
| :-- | :-- | :-- |
| M8 | **Modal** — mass assembly, eigen route, CLI and web surface | V13 passes; first three modes visible headless and in the Results lens. |
| M9 | **Multi-volume 3MF** — reader, per-volume materials, writer metadata | V12 passes; materials recorded and exported per volume. |
| M10 | **STEP in** — reader dependency, tessellation, voxelisation path | V14 passes, or the fallback ladder's stall is documented with this file changed in a reviewed commit. |
| M11 | **Distributed loading** — face-local coordinates, compiled F(x, y), consistent nodal loads, Setup-lens editor | V15 passes; bands and expression bodies round-trip and integrate to the stated resultant. |

---

## Explicitly out of scope for v0.3

B-rep editing · STEP *export* · topology optimisation (SIMP) · nonlinear, thermal or CFD
analysis · multi-material analysis (per-volume labelling is in; mixed-material stiffness is
not) · assemblies, drawings or BOM · conformal lattice mapping · beam/stochastic lattice
classes · collaboration, accounts or cloud storage · G-code generation · mobile layout.

---

## Definition of done

v0.3 is done when every box above is ticked, V1–V15 pass in CI, the budgets are met on the
referee, the two provisional budgets are confirmed or honestly revised by measurement, and
the reference task is run once end to end on a foreign fixture with its report filed next to
`docs/verification/` — and not before. If a criterion turns out to be wrong, change this
file in a reviewed commit; do not quietly skip it.