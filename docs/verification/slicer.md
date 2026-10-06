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