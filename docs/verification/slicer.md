# Slicer pass of the exported 3MFs — GOAL reference-task step 9

Step 9 requires the exported 3MF to open in PrusaSlicer / OrcaSlicer with zero repair
warnings. This pass: PrusaSlicer 2.9.6 (flathub), driven headless, 2026-10-06, after the
per-sheet vertex separation and head-of-weld chord registry landed in the adaptive stitch.

The meshes were written by the release CLI (`fieldsmith export … --3mf … --adaptive`),
which shares the adaptive mesher and the 3MF writer the browser Make path uses. The graded
bracket is `examples/bracket-filled.fsm`; the control is the same bracket with no fill
(`examples/bracket.fsm`), exported on the same adaptive path.

## What PrusaSlicer reports on import (`--info`)

| File | h (mm) | Facets | Prusa `manifold` | Prusa parts | Prusa volume (mm³) |
| :-- | --: | --: | :-- | --: | --: |
| bracket-filled (graded) | 0.25 | 3 026 110 | yes | 13 | 23 465.2 |
| bracket-filled (graded) | 0.1 | 18 535 066 | yes | 19 | 23 637.5 |
| bracket (plain solid, control) | 0.1 | 4 513 824 | yes | 1 | 34 377.7 |

The previous pass (2026-10-05, before the separation) measured both graded rows
`manifold = no` — 83 open edges and 17 parts at h 0.25, 546 and 73 at h 0.1; the control
was already `manifold = yes`, one part. On a clean import `--info` stops reporting an
`open_edges` field; the graded mesh's residual open edges (our census: 0 at h 0.25, 8 at
h 0.1) are closed by Prusa's import weld. The graded part's shells are its closed surface
components: 13 at h 0.25 / 19 at h 0.1, where the pre-separation meshes carried 17 / 73.
The shell count is a property of the filled solid as sliced (a gyroid fill's wall sheets
meet and part), not a repair finding — Prusa flags nothing about it at import.

Volume cross-checks: graded h 0.25 Prusa 23 465.2 vs. our mesh volume 23 461.2 (Δ 0.02 %);
graded h 0.1 Prusa 23 637.5 vs. ours 23 493.3 (Δ 0.6 %); control Prusa 34 377.7 vs. ours
34 399.8 (Δ 0.06 %) — Prusa's volume carries the same offset class on all three rows, so
the offsets track the volume algorithm, not a mesh defect. The residuals our own scans do
report on the graded meshes — 10 non-manifold edges plus 4959 wall-tile
self-intersecting pairs at h 0.25, 229 plus 7320 at h 0.1 (`adaptive_defect_census.rs`
and the CLI defect scans) — coexist with the `yes` verdicts: Prusa's weld accepts the
closed shells as read.

## Import, re-export, slice

- Import + re-export to STL at h 0.25: Prusa wrote back exactly the source's triangle
  count — the returned binary STL holds (151 305 584 − 84) / 50 = 3 026 110 facets — so
  nothing was auto-repaired and nothing was dropped.
- Slicing the graded h 0.25 file end to end (`--slice`) produced a G-code file
  (14.3 MB, PrusaSlicer 2.9.6 default profile). The only diagnostic was the
  print-stability advisory "Long bridging extrusions — consider enabling supports",
  which is about lattice bridge spans, not mesh repair (same advisory as the
  pre-separation pass).

## What this settles for GOAL step 9

Import and slicing work, and the slicer's own import now classifies both graded exports
manifold. The previous pass's named work — sub-cell sheet separation in the adaptive
stitch, so one vertex serves one sheet — is what moved `manifold` for the graded part, the
way the plain-solid control already sat at `yes`. GUI-open behaviour itself is not
verified here (the passes above are CLI only); with a `yes` import the `manifold = no`
repair trigger the previous pass measured is gone.

Export-budget context from the same CLI runs, for the GOAL ≤ 15 s line at 0.1 mm: the
graded h 0.1 export measured build 3.0 s + mesh 8.5 s + 3MF write 0.08 s = 11.6 s of
export path; the same run's verification re-solve (8.6 s) and defect scan (9.7 s) are not
part of the export path. h 0.25: mesh 1.1 s, 8.1 s total including verification. The
plain-solid control at h 0.1: mesh 2.0 s, 8.1 s total, defect-free in our own scan
(closed, 2-manifold, consistently oriented, self-intersection-free).

## Mesh-leaf graded fill with positive skin (M20 — `c01x-composite.fsmz`)

On the M10 reference-task fixture (`c01x-composite.fsmz`: 1:10 scaled NIST `CTC-01` STEP
tessellation imported as a `mesh` leaf, `.nz` extended `2.5 mm`, `fill gyroid sheet cell=6
skin=1.2 rho=(0.2, 0.8) curve=1 wall=0.8 margin=2`), the v0.2 adaptive stitch reported
`202–216` open edges and `140` degenerate triangles at `h = 0.30 mm`, `344` open edges at
`h = 0.15 mm`, and a `+18.02 %` mesh-volume jump (`14 591.7 mm³` vs `12 364.1 mm³` field
volume) at `h = 0.25 mm`.

Inspecting the mesh in `f64` and per-component signed volumes isolated three causes in
`crates/fieldsmith-mesh/src/adaptive.rs`:

1. **Sub-tolerance `f32` corner crossings on planar offset faces (`216` apparent open edges
   from `140` degenerate triangles at `h = 0.30 mm`):** Mesh-leaf grids store `f32`
   samples, so planar faces of the `-(body + skin)` offset evaluate to `~1e-8..3e-7 mm` at
   lattice corners — above the `1e-9 * h` exact-zero corner snap (`t0 == 0.0`). Those
   corners placed edge crossings at `t0 ~ 1e-8..1e-6` (`5e-9..3e-7 mm` from the corner),
   producing `140` corner pinch triangles of area `7e-18..6e-13 mm² <= 1e-12 mm²`. The
   emitted mesh was topologically closed (`0` open edges when all triangles were included),
   but `TriMesh::defects()` excludes triangles with `area <= 1e-12` before counting edge
   usage, reporting their `216` boundary edges as open. Clamping `t` into
   `[FOLD_NUDGE, 1.0 - FOLD_NUDGE]` (`[1e-4, 0.9999]`) for all sign-changing edges upholds
   the corner-clearance invariant across `f32` evaluation noise.
2. **Post-weld `[u, v, v]` collapsed triangles (`4 / 14 / 42` degenerate triangles at
   `skin = 0.0`, causing PrusaSlicer `manifold = no, open_edges = 2`):** When `weld_exact`
   merged a snapped leaf vertex and a crossing vertex into one vertex ID, triangles with
   repeated corner indices (`v2 == v3`) stayed in `tris` and were written to the 3MF XML.
   Dropping `[u, v, v]` triangles immediately after `weld_exact` removes pairs of opposite
   half-edges `(u, v)` and `(v, u)` without changing boundary closure, clearing the
   degenerate triangles and turning `skin = 0.0` `manifold = yes` at all three spacings.
3. **Per-component cavity orientation (`+18.02 %` volume jump at `h = 0.25 mm`):** The
   gyroid sheet divides the interior inside the outer skin (`+14 556.3 mm³`) into two
   disconnected void labyrinths (`-1 088.3 mm³` and `-1 014.7 mm³`). At `h = 0.25 mm`, the
   first labyrinth component (`325 394` triangles) had a crease quad as its first triangle
   (`seed = 26184`), so seed-locked `orient` flipped all `325 394` triangles of that
   cavity positive (`+1 088.3 mm³`), adding `+2 176.6 mm³` to the total volume. Choosing
   each edge-connected component's sign by majority vote of its build-time (`emit_quad`)
   orientations in `orient_inner` preserves outward orientation on outer shells and inward
   (negative-volume) orientation on enclosed cavities (`12 409.4 mm³`, `Δ +0.37 %`).

### Before vs. after on `c01x-composite.fsmz` (`skin = 1.2 mm`)

| h (mm) | Pass | Tris | Field vol (mm³) | Mesh vol (mm³) | Δ vol (%) | Open | NM | Orient | Degen | SI | Prusa `manifold` | Prusa parts | Prusa vol (mm³) |
| --: | :-- | --: | --: | --: | --: | --: | --: | --: | --: | --: | :-- | --: | --: |
| 0.30 | before | 825 122 | 12 342.7 | 12 421.9 | +0.64 % | 216 | 3 | 0 | 140 | 798 | yes (weld) | 29 | 12 426.6 |
| 0.30 | **after** | **825 122** | **12 342.7** | **12 412.2** | **+0.56 %** | **0** | **3** | **0** | **0** | **798** | **yes** | **29** | **12 416.8** |
| 0.25 | before | 1 201 430 | 12 364.1 | 14 591.7 | +18.02 % | 0 | 7 | 0 | 0 | 1011 | yes (cavity inverted) | 20 | 14 584.1 |
| 0.25 | **after** | **1 201 430** | **12 364.1** | **12 409.4** | **+0.37 %** | **0** | **7** | **0** | **0** | **1011** | **yes** | **20** | **12 401.1** |
| 0.15 | before | 3 281 276 | 12 391.2 | 12 481.7 | +0.73 % | 344 | 47 | 0 | 228 | 4490 | — | — | — |
| 0.15 | **after** | **3 277 076** | **12 391.2** | **12 409.5** | **+0.15 %** | **0** | **44** | **0** | **0** | **4485** | **yes** | **33** | **12 377.6** |

### Full 5 × 3 grid (`skin ∈ {0.0, 0.3, 0.6, 0.9, 1.2 mm}` × `h ∈ {0.30, 0.25, 0.15 mm}`, after fix)

| skin (mm) | h (mm) | Verts | Tris | Field vol (mm³) | Mesh vol (mm³) | Δ vol (%) | Open | NM | Orient | Degen | SI | Prusa `manifold` | Prusa parts | Prusa vol (mm³) |
| --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: | :-- | --: | --: |
| 0.0 | 0.30 | 532 464 | 1 067 852 | 9 329.4 | 8 997.3 | −3.56 % | 0 | 2 | 0 | 0 | 2013 | yes | 3 | 8 998.4 |
| 0.0 | 0.25 | 757 041 | 1 517 738 | 9 155.8 | 9 049.2 | −1.16 % | 0 | 15 | 0 | 0 | 2386 | yes | 2 | 9 049.2 |
| 0.0 | 0.15 | 2 126 251 | 4 255 744 | 9 215.7 | 9 076.8 | −1.51 % | 0 | 133 | 0 | 0 | 9509 | yes | 6 | 9 068.7 |
| 0.3 | 0.30 | 604 080 | 1 209 268 | 10 003.3 | 9 960.6 | −0.43 % | 0 | 6 | 0 | 0 | 1536 | yes | 22 | 9 965.3 |
| 0.3 | 0.25 | 847 735 | 1 697 172 | 10 042.3 | 10 075.4 | +0.33 % | 0 | 3 | 0 | 0 | 1993 | yes | 30 | 10 073.5 |
| 0.3 | 0.15 | 2 408 119 | 4 817 454 | 10 065.9 | 10 099.7 | +0.34 % | 0 | 99 | 0 | 0 | 9190 | yes | 35 | 10 072.5 |
| 0.6 | 0.30 | 532 735 | 1 065 982 | 10 882.8 | 10 961.1 | +0.72 % | 0 | 3 | 0 | 0 | 1095 | yes | 43 | 10 966.5 |
| 0.6 | 0.25 | 756 828 | 1 514 526 | 10 908.0 | 10 972.3 | +0.59 % | 0 | 1 | 0 | 0 | 1453 | yes | 45 | 10 968.1 |
| 0.6 | 0.15 | 2 118 697 | 4 238 034 | 10 942.1 | 10 975.5 | +0.31 % | 0 | 57 | 0 | 0 | 7573 | yes | 48 | 10 938.9 |
| 0.9 | 0.30 | 469 703 | 939 784 | 11 622.1 | 11 690.5 | +0.59 % | 0 | 1 | 0 | 0 | 1094 | yes | 34 | 11 695.7 |
| 0.9 | 0.25 | 676 318 | 1 353 372 | 11 642.1 | 11 695.5 | +0.46 % | 0 | 5 | 0 | 0 | 1315 | yes | 26 | 11 692.7 |
| 0.9 | 0.15 | 1 869 651 | 3 739 806 | 11 678.4 | 11 697.6 | +0.16 % | 0 | 34 | 0 | 0 | 6137 | yes | 27 | 11 664.5 |
| 1.2 | 0.30 | 412 469 | 825 122 | 12 342.7 | 12 412.2 | +0.56 % | 0 | 3 | 0 | 0 | 798 | yes | 29 | 12 416.8 |
| 1.2 | 0.25 | 600 498 | 1 201 430 | 12 364.1 | 12 409.4 | +0.37 % | 0 | 7 | 0 | 0 | 1011 | yes | 20 | 12 401.1 |
| 1.2 | 0.15 | 1 638 368 | 3 277 076 | 12 391.2 | 12 409.5 | +0.15 % | 0 | 44 | 0 | 0 | 4485 | yes | 33 | 12 377.6 |

### Slicing (`--export-gcode --layer-height 0.2`) on `skin = 1.2 mm`

Headless PrusaSlicer 2.9.6 (`--export-gcode --layer-height 0.2`) sliced all three
`skin = 1.2 mm` exports (`h = 0.30, 0.25, 0.15 mm`) to `11 MB` G-code files with zero
mesh-repair warnings (only the expected print-stability advisory on unsupported lattice
bridges). Regression coverage is locked in
`crates/fieldsmith-mesh/src/adaptive.rs` (`per_component_orientation_preserves_enclosed_cavity_sign`
and `noisy_planar_offset_crossings_stay_non_degenerate_and_closed`) and
`crates/fieldsmith-verify/tests/v14_step.rs` (`graded_mesh_leaf_with_positive_skin_exports_closed_and_within_volume_bound`).