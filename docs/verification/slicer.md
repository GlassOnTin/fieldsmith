# Slicer pass of the exported 3MFs — GOAL reference-task step 9

Step 9 requires the exported 3MF to open in PrusaSlicer / OrcaSlicer with zero repair
warnings. This is the measured first pass: PrusaSlicer 2.9.6 (flathub), driven headless,
2026-10-05.

The meshes were written by the release CLI (`fieldsmith export … --3mf … --adaptive`),
which shares the adaptive mesher and the 3MF writer the browser Make path uses. The
graded bracket is `examples/bracket-filled.fsm`; the control is the same bracket with no
fill (`examples/bracket.fsm`), exported on the same adaptive path.

## What PrusaSlicer reports on import (`--info`)

| File | h (mm) | Facets | Prusa `manifold` | Prusa `open_edges` | Prusa parts | Slice to G-code |
| :-- | --: | --: | :-- | --: | --: | :-- |
| bracket-filled (graded) | 0.25 | 3 018 006 | no | 83 | 17 | yes |
| bracket-filled (graded) | 0.1 | 18 514 666 | no | 546 | 73 | not attempted (same class expected) |
| bracket (plain solid, control) | 0.1 | 4 495 082 | yes | — (clean) | 1 | not attempted |

Volume cross-check: graded h 0.25 Prusa 23 465 mm³ vs. our mesh volume 23 461 mm³
(Δ 0.02 %); graded h 0.1 Prusa 23 637 vs. ours 23 493 (Δ 0.6 %). The defect census our own
`TriMesh::defects` reports for the same files (997 / 8200 open edges at h 0.25 / 0.1) is
larger than Prusa's 83 / 546: Prusa's import welds coincident vertices and closes most of
the slits, so the two scanners disagree in Prusa's favour; it still lands on `manifold = no`
from what remains.

## Import, re-export, slice

- Import + re-export to STL at h 0.25: Prusa wrote back exactly the source's triangle
  count, 3 018 006, so nothing was auto-repaired and nothing was dropped.
- Slicing the graded h 0.25 file end to end (`--slice`) produced a G-code file
  (14.4 MB, 668 714 lines, PrusaSlicer 2.9.6 default profile). The only diagnostic was
  its print-stability advisory "Long bridging extrusions — consider enabling supports",
  which is about lattice bridge spans, not mesh repair.

## What this settles for GOAL step 9

Import and slicing work. "Zero repair warnings" is NOT established: the slicer's own
import classifies the graded exports non-manifold, and on a GUI open that is the flag the
repair dialog keys on (GUI behaviour itself is not verified here — the passes above are
CLI only). The causes are the fill-surface defect classes classified in `V8.md`; the
plain-solid control on the same mesher imports clean. The named work — sub-cell sheet
separation in the adaptive stitch, so one vertex serves one sheet — is what would move
`manifold` for the graded part the way it already sits at `yes` for the plain solid.