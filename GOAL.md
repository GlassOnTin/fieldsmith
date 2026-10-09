# Fieldsmith — Goal

This file defines **what "done" means for v0.1**. It is deliberately concrete and testable.
The [VISION](VISION.md) says *why*; this says *what, exactly, by when it counts as finished*.

---

## The goal in one sentence

> A first-time user, with no documentation, can model a bolted bracket, fix it, load it,
> fill it with a stress-graded gyroid, check that it is still strong enough, and export a
> print-ready 3MF — in under five minutes, in a browser, with every reported number verified
> against theory.

---

## The reference task — "the bracket test"

This single scenario is the acceptance test for v0.1. Every feature in scope exists to make it work well.

1. Start from an empty document.
2. Model an L-bracket: two 60 × 40 × 8 mm plates at 90°, an 8 mm fillet on the inside corner,
   two ⌀ 6.5 mm holes in the base plate, one ⌀ 8.5 mm hole in the upright.
3. Fix the inner faces of the two base holes.
4. Apply 400 N on the upright hole, pointing down and 15° outward.
5. Choose material **PETG** (from the built-in library).
6. View von Mises stress and deflection.
7. Fill the interior with a gyroid: 1.2 mm solid skin, 6 mm cell, solid rings of 2 mm around every hole,
   density graded by stress.
8. Re-check: peak stress, max deflection, safety factor, mass — solid vs. graded, side by side.
9. Export 3MF; it opens in PrusaSlicer / OrcaSlicer with zero repair warnings.
10. Save the document as `bracket.fsm`, reload it, get identical results.

**Pass:** 3 out of 3 people unfamiliar with the project complete steps 1–9 in ≤ 5 minutes
without help, and the outputs satisfy the verification criteria below.

---

## Acceptance criteria

### A. Modelling
- [x] Primitives: box, cylinder, sphere, capsule, torus, extruded 2D profile (polyline + arcs).
- [x] Operations: union, subtract, intersect; smooth (filleted) variants with a radius in mm.
- [x] Modifiers: shell (inward/outward, thickness in mm), offset, linear and circular pattern, mirror.
- [x] Transform with in-viewport gizmo; snap to grid and to other features; exact values in inspector.
- [x] Editable model tree: select, rename, reorder, suppress, delete — all undoable.
- [x] Document is a readable text file (`*.fsm`) that round-trips losslessly through the UI.

### B. Setup
- [x] Click a surface to create a **region**; region selection is a field (works on curved and blended faces).
- [x] Supports: fixed; fixed in selected axes.
- [x] Loads: force (N, distributed over a region), pressure (MPa); direction typed as a vector (arrow-dragging is not built).
- [x] Multiple load cases; results per case and envelope (max over cases).
- [x] Material library (PLA, PETG, PA12, ABS, AlSi10Mg, Ti-6Al-4V, 316L) with sources cited; custom materials allowed.

### C. Solve
- [x] Automatic quality tet meshing of the field (isosurface stuffing), quadratic (P2) elements by default.
- [x] Progressive results: coarse result streams to the viewport, then refines; edit cancels and restarts.
- [x] Results: von Mises, displacement magnitude and vector, deformed shape (scalable), safety factor field.
- [x] Probe tool: click anywhere for the value, with units, at that point.
- [x] Provenance badge on every result: element count, element order, solve time, refinement level, change since previous level.

### D. Fill (grading)
- [ ] TPMS families: gyroid, Schwarz-P, diamond. Sheet and network modes.
- [ ] Controls: cell size (mm), skin thickness (mm), ρ_min / ρ_max, grading curve; protected solid regions (e.g. around holes).
- [ ] Density driven by stress from any load case or the envelope.
- [ ] Mass reported from the **final field**, in grams, using material density.
- [ ] Re-solve the graded part using documented homogenised stiffness laws per family; show solid vs. graded comparison.

### E. Make
- [ ] 3MF export (millimetres, part name, material metadata); binary STL export.
- [ ] Export mesh is watertight, 2-manifold, consistently oriented, free of self-intersections.
- [ ] Slice preview: scrub layers at a chosen layer height; shows contours of the exact export mesh.

### F. Interface
- [ ] Layout: one viewport, model tree (left), inspector (right), status line (bottom). Nothing else permanent.
- [ ] Lenses: Shape · Setup · Results · Fill · Make — switch with `1`–`5`, never lose state.
- [ ] Command palette (`Ctrl K`) reaches every action; shortcuts shown on hover.
- [ ] Light/dark themes from OS preference; perceptual colour maps with labelled legends and units.
- [ ] Empty states guide the next action; errors are specific and recoverable.
- [ ] No emoji icons, no gradient text, no status-pill clutter. Zero console errors on the bracket test.

### G. Import (M6 — user-set milestone, outside v0.1's definition of done)
- [x] Import GLB, OBJ, binary and ASCII STL; units and up-axis baked at import with an explicit choice in the dialog.
- [x] The imported mesh becomes a field leaf (voxelised signed distance + segmentation patches) that composes with primitives and modifiers.
- [x] Faces label on imported meshes through the shared normal-cone rule; patch-click labelling works.
- [x] Region-driven extension: `extend` sweeps a labelled band along a vector, e.g. the meshy bottom plate extended downward 10 mm.
- [x] Sidecar `.fsmz` round-trips a mesh-heavy document bit-consistently (V9, V10).
- [x] CLI `import` / `solve` / `export` run the mesh milestone end to end headless (V10).
- [ ] 3MF import (our export format today); mesh-convergence verification case; multi-material labelling of volumes.

### H. Editing (M7 — user-set milestone, outside v0.1's definition of done)
- [x] Booleans on imported meshes are verified compositions, not presumed: mesh×primitive and mesh×mesh union/cut volumes against closed form on the voxel test harness, and patch labels surviving each composition. (Landed: `boolean_volume.rs` — union/subtract/intersect volumes against closed form at 0.1–0.5 % error, patch labels surviving each composition; label claims through composites landed earlier — union/subtract/intersect attribute the surface to the nearest operand, blend bands to neither, GLSL twin included.)
- [ ] The mesh-convergence verification case deferred in G: patch-labelled boundary loads on an imported body, convergence against a resolved reference.
- [ ] `Filled` (graded lattice) on a mesh document measured — declared untested in V10.
- [x] Modifiers on mesh leaves: offset and shell with thickness in mm; `.fsm` text and validate rules ride along. (Landed: a hydrated single-mesh operand bakes a keyed derived grid — pointwise within one band, mask bake beyond it; groups of several operands, unhydrated meshes and refused bakes keep the eval-time wrap, which validate now names.)
- [ ] Face selectors on derived solids: extend nodes and boolean results expose labelled faces, closing V10's gap — the skirt wall is clampable.
- [ ] 3MF import rides along if cheap; multi-material labelling stays out (its own milestone).

---

## Verification cases (must pass in CI)

Each case is a literate, executable test that produces a Markdown report with the measured
error. Tolerances are on the **reported** value, as the user would see it.

| ID | Case | Reference | Tolerance |
| :-- | :-- | :-- | :-- |
| V1 | Cantilever 100 × 10 × 10 mm, E = 200 GPa, 100 N tip load | Tip deflection `FL³/3EI` = 0.200 mm | ≤ 2 % |
| V2 | Same beam, bending stress at mid-span top fibre | `Mc/I` = 30.0 MPa | ≤ 3 % |
| V3 | Plate in tension with central hole, d/W = 0.1 | Heywood `Kt = 2 + (1 − d/W)³` = 2.729 | ≤ 5 % |
| V4 | Field volume of sphere r = 10, box-minus-cylinder | Closed-form volume | ≤ 0.5 % |
| V5 | Preview shader vs. Rust evaluator, 10 000 random points, bracket model | Identical field | ≤ 1 × 10⁻⁴ mm |
| V6 | Gyroid sheet unit cell at ρ = 0.2, 0.3, 0.4 — resolved FE vs. homogenised law | Fitted `E*(ρ)` | ≤ 10 % |
| V7 | Graded-lattice beam: fully resolved FE vs. homogenised re-solve, tip deflection | Resolved FE | ≤ 15 % |
| V8 | Exported mesh of the bracket test | Mesh volume vs. field volume; manifold checks | ≤ 1 %, 0 defects |
| V9 | Save → reload → re-solve of `bracket.fsm` | Previous run | Bit-identical |
| V10 | Import a 87k-triangle meshy STL, label its faces, extend the bottom 10 mm, solve, export (M6) | Facet-histogram cone coverage; cross-section continuity; PrusaSlicer manifold | Cone ≥ 88 %; Δ ≤ 10 µm; slicer-clean |

---

## Performance budgets

Measured on a mid-range laptop (integrated GPU, 8 cores, 16 GB), Chrome and Firefox current.

| Metric | Budget |
| :-- | :-- |
| Preview frame rate, bracket with gyroid fill, 1080p | ≥ 60 fps |
| Edit → coarse stress result visible | ≤ 1.5 s |
| Edit → refined result (P2, ≈ 150 k DOF) | ≤ 10 s |
| Export 3MF of graded bracket at 0.1 mm feature resolution | ≤ 15 s |
| Cold load, compressed transfer size | ≤ 3 MB |
| Main thread long tasks during solve | none > 50 ms |

The probe that measures these rows also measures mesh-document rows — import of a GLB
fixture through the file palette, and export of the imported part (see
`docs/verification/budgets.md`); budget-scale mesh numbers are in V10.

---

## Engineering criteria

- [ ] Cargo workspace (`crates/`) + TypeScript web app (`web/`), built with Vite; static hosting only.
- [ ] `fenics-rs` consumed as a dependency, not vendored or forked.
- [ ] Every Rust stage runs headless via `fieldsmith-cli` (`fieldsmith solve bracket.fsm --report`).
- [ ] WASM surface is a handful of functions taking serialised documents, not positional argument lists.
- [ ] All compute in Web Workers; buffers passed as transferables.
- [ ] CI: `cargo test`, `cargo clippy -D warnings`, TypeScript typecheck, verification cases V1–V9, a headless browser run of the bracket test script.
- [ ] README states limitations plainly: linear elastic, small strain, isotropic homogenisation.

---

## Build order

Each milestone ends with something usable and demonstrable.

| # | Milestone | Done when |
| :-- | :-- | :-- |
| M0 | **Skeleton** — document model, field IR, Rust evaluator, shader codegen, raymarched viewport, CLI | A `.fsm` file with a box-minus-cylinder renders live and V4, V5 pass. |
| M1 | **Shape** — primitives, booleans, blends, modifiers, gizmos, model tree, undo | The bracket in step 2 can be modelled entirely in the UI. |
| M2 | **Setup + Solve** — region picking, supports, loads, tet meshing, fenics-rs solve, progressive results | V1–V3 pass; bracket shows stress. |
| M3 | **Fill + Check** — TPMS grading, protected regions, homogenisation, re-solve, comparison | V6, V7 pass; mass in grams. |
| M4 | **Make** — dual-contouring export, 3MF/STL, slice preview | V8 passes; file opens clean in a slicer. |
| M5 | **Polish + proof** — command palette, themes, empty states, literate case reports, hallway test | The bracket test passes with 3 / 3 new users. Tag `v0.1.0`. |
| M6 | **Import** — meshy-style GLB/OBJ/STL as field leaves, patch + face labelling, region-driven extension, `.fsmz` sidecar | V10 passes; the meshy bottom-extend test solves and slices clean. |
| M7 | **Editing** — booleans on imported meshes (verified, not presumed), offset/shell modifiers, face selectors on derived solids | Boolean volumes verify against closed form; the mesh-convergence case passes; the skirt wall clamps. |

---

## Explicitly out of scope for v0.1

STEP / B-rep import · topology optimisation (SIMP) · nonlinear, modal, thermal or CFD analysis ·
multi-material parts · collaboration / accounts / cloud storage · G-code generation · mobile layout.

---

## Definition of done

v0.1 is done when **every box above is ticked, V1–V9 pass in CI, the performance budgets are met,
and the bracket test passes with three new users** — and not before. If a criterion turns out
to be wrong, change this file in a reviewed commit; do not quietly skip it.
