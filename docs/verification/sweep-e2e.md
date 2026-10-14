# Sweep e2e — the results lens’ parameter sweep (M16 slice 3)

**Result: PASS**  ·  2026-10-14T08:41:12.136Z

The five-point question on the Cantilever V1 (`beam.size.2` 8 → 12 mm, rungs
30000/150000 asked explicitly on both sides): the native `fieldsmith api --csv` run once,
then the same question driven through the Results lens in the live app.

## Evidence A — the byte contract (CSV file ↔ in-page envelope mirror)

- Native: `fieldsmith api --csv <tmp>` reading the request on stdin; exit 0,
  8677 ms whole. Omitting `rungs` would ask the native three-rung default
  (`question.rs` `DEFAULT_RUNGS` = 30000/150000/750000) — the request pins the ladder
  coarse/refined pair the web mirrors, so the two sides answer the same question.
- `sweepCsvFromEnvelope(JSON.parse(stdout))` in the page (web/src/app/sweep.ts) reproduces
  the CLI's CSV file byte for byte: pass (mirror 643 chars, file 643 chars).
- Why that must hold: the CSV file and the JSON envelope are printed from one typed
  response, so their float values are the same doubles; both writers use
  shortest-round-trip decimal text, and parsing it returns those same doubles;
  rustFloat is the JS twin of Rust's plain-decimal Display, so each cell re-renders to
  the file's own bytes.

| rustFloat vector | rendered |
| :-- | :-- |
| small7 | 0.0000001 === 0.0000001 ? true |
| small8 | 0.000000015 === 0.000000015 ? true |
| small8b | 0.000000025 === 0.000000025 ? true |
| small8n | -0.000000025 === -0.000000025 ? true |
| big20 | 100000000000000000000 === 100000000000000000000 ? true |
| big21 | 1000000000000000000000 === 1000000000000000000000 ? true |
| negzero | -0 === -0 ? true |
| literal | 0.39066861588109364 === 0.39066861588109364 ? true |

## The native answer (per-point ladders, refined rung)

| point (mm) | dofs | elements | wall r1 (ms) | deflection (mm) | vm (MPa) |
| :-- | --: | --: | --: | --: | --: |
| 8 | 194445 | 41868 | 1484.967884 | 0.39066861588109364 | 91.91007662342635 |
| 9 | 184173 | 39644 | 1420.322418 | 0.2748421438885008 | 73.83946607323199 |
| 10 | 202473 | 43988 | 1592.605911 | 0.20073674735070385 | 61.48445501842035 |
| 11 | 196581 | 42700 | 1570.079871 | 0.1511276411885594 | 51.49186401720839 |
| 12 | 187575 | 40640 | 1415.223923 | 0.11666994594760402 | 43.65586159504041 |

## Evidence B — the live UI sweep

- Loaded through the store, Results lens, ladder ready; the panel's parameter select
  steps to `beam.size.2`, from/to committed to 8/12 on blur. Sweep run: 9377 ms.
- Rows streamed: 5 with progress shown mid-run; export through the real click
  → download flow (Cantilever V1-sweep.csv).

| Check | Result | Detail |
| :-- | :-- | :-- |
| sweep ends whole with no error | pass | {"running":false,"done":true,"error":null,"points":5,"docVersion":1,"storeVersion":1} |
| five points streamed | pass | 5 |
| the sweep measured this document edition | pass | docVersion 1 |
| web CSV header = the CLI header | pass | "value,dofs,elements,wall_ms,deflection_mm,vm_mpa,deflection_change_rel,vm_change_rel,status" |
| web CSV row count = the CLI row count | pass | 7 vs 7 lines |
| integer/status columns equal, tracked floats within solve-drift | pass | 5 data rows, 35 compared cells, gates direct 1e-7 / derived 1e-6, max observed drift direct 6.01e-11 / derived 1.09e-7 relative |
| row 0 wall columns recorded | pass | native 1484.967884 ms vs web 1594.1700000166893 ms (row 0, refined rung) |
| native point 0 deflection is the ship literal | pass | 0.39066861588109364 |
| web point 0 deflection within solve-drift of the ship literal | pass | 0.3906686158626879 vs 0.39066861588109364 (rel 4.71e-11) |
| beam.size.2 is still 10 after the sweep | pass | 10 |
| export names the document | pass | Cantilever V1-sweep.csv |
| no console errors | pass |  |
- What the panel showed while the run went (value | row text):
  - Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 45123 dofs · 9024 elements · wall 276 msrung 1refined · 150k DOF · 202473 dofs · 43988 elements · wall 1840 ms · case envelope vs rung 0 (estimate): defl +0.0% · vm +11.5% | 194445 dofs · 41868 elements · wall 1594 ms · deflect 0.3907 mm · 91.9101 MPa
  - Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 45123 dofs · 9024 elements · wall 276 msrung 1refined · 150k DOF · 202473 dofs · 43988 elements · wall 1840 ms · case envelope vs rung 0 (estimate): defl +0.0% · vm +11.5% | 184173 dofs · 39644 elements · wall 1547 ms · deflect 0.2748 mm · 73.8395 MPa
  - Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 45123 dofs · 9024 elements · wall 276 msrung 1refined · 150k DOF · 202473 dofs · 43988 elements · wall 1840 ms · case envelope vs rung 0 (estimate): defl +0.0% · vm +11.5% | 202473 dofs · 43988 elements · wall 1785 ms · deflect 0.2007 mm · 61.4845 MPa
  - Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 45123 dofs · 9024 elements · wall 276 msrung 1refined · 150k DOF · 202473 dofs · 43988 elements · wall 1840 ms · case envelope vs rung 0 (estimate): defl +0.0% · vm +11.5% | 196581 dofs · 42700 elements · wall 1766 ms · deflect 0.1511 mm · 51.4919 MPa
  - Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 45123 dofs · 9024 elements · wall 276 msrung 1refined · 150k DOF · 202473 dofs · 43988 elements · wall 1840 ms · case envelope vs rung 0 (estimate): defl +0.0% · vm +11.5% | 187575 dofs · 40640 elements · wall 1558 ms · deflect 0.1167 mm · 43.6559 MPa

- Wall columns: native 1484.967884 ms vs web 1594.1700000166893 ms (row 0, refined rung). Column 3 (wall_ms) is each engine's own clock. Columns
  4–7 are gated (the solve-drift contract, Limits): 35 cells compared across
  the five data rows — columns 0/1/2/8 exact, 4/5 within 1e-7, 6/7 within
  1e-6 — 0 mismatched, largest observed drift
  6.01e-11 (direct) and 1.09e-7
  (derived change columns) relative.

## The web CSV

```csv
value,dofs,elements,wall_ms,deflection_mm,vm_mpa,deflection_change_rel,vm_change_rel,status
8,194445,41868,1594.1700000166893,0.3906686158626879,91.91007661790529,0.00028965910752881815,0.09873661647823584,ok
9,184173,39644,1547.2950000166893,0.27484214388072675,73.83946607096517,0.000269755982025618,0.08887243888680711,ok
10,202473,43988,1785.3650000095367,0.2007367473508361,61.48445501823126,0.00027730030800738275,0.11489173043320122,ok
11,196581,42700,1765.5850000083447,0.15112764118732241,51.491864016710004,0.0002980052818040602,0.1273747424231885,ok
12,187575,40640,1557.6450000107288,0.11666994594564993,43.65586159410244,0.00031080742832269515,0.13433039529625276,ok
```

## Method

`web/tests/sweep.mjs`: the native reply comes from one `api --csv` run (both artifacts
from the same typed response); the mirror and the rustFloat vectors run in the page
against the module the app itself uses; the UI question is driven through the real
NumberInput blur-commit path and the Run button, and the export through a real download.
The two targets are not assumed bit-identical: columns 0/1/2/8 must equal the CLI cell
for cell, columns 4–7 are gated at SOLVE_DRIFT_REL — named and measured under Limits.

## Limits

- wall_ms cannot match between native and wasm, and the report does not ask it to; the
  walls that agree are Evidence A's (one run).
- No tolerance is declared on either side, so every row's verdict is `ok` on both sides;
  the met/refusal cell paths are exercised by the Rust cases, not this browser run.
- Point 0's deflection literal (0.39066861588109364) is pinned from the referee ship run
  (V23, native). The native CLI reproduces it exactly in this run; the web lands within
  SOLVE_DRIFT_REL of it.
- Cross-target bit-identity does not hold here, and no case requires it: V23 pins
  native-vs-native and M15 checked x86_64-vs-x86_64. Measured on a diagnostic ladder
  (outside this run): the mesh stage is bit-identical across targets at every size
  (dofs/elements/order/element_size and the two boundary integrals); the solve is
  bit-identical at total 8247 dofs (coarse factor 1437) and drifts at 77535 and 202473
  (coarse 11679/29250) — the same iteration count on both targets (24 at the refined
  rung), relative residual differing ~5.6e-8 relative (4.694681002945911e-10 native vs
  4.694681003002073e-10 web at 202473 dofs), tracked floats differing ≤ 7e-11 relative
  on this question. Both answers sit inside the PCG convergence ball (rtol 1e-9). The
  mechanism, consistent-with not proven: faer 0.20.2's sparse Cholesky — the two-level
  preconditioner's coarse factor — dispatches its numeric kernels through pulp per ISA
  (AVX2/FMA on x86_64, wasm SIMD elsewhere), so accumulation order differs below the
  identical combinatorial symbolic choice; the fieldsmith solver's own reductions are
  sequential and single-threaded on both sides.
- The float gates: SOLVE_DRIFT_REL = 1e-7 on the direct columns (4/5) is
  100× the solver's own rtol (1e-9) — a tighter gate would test convergence noise, not
  the pipeline — and 10⁵× below the 1e-2 scale of the tolerance grammar this feature
  serves. CHANGE_DRIFT_REL = 1e-6 on the derived change columns (6/7)
  accounts for the web's own JS division: the relative error of a difference of
  drift-affected doubles scales as 1/change fraction (~3400× on this question,
  change_rel ≈ 2.9e-4), so the ≤ 7e-11 direct drift shows up as ~1.1e-7 — above the
  direct gate, below the derived one. Even so the derived gate implies tighter absolute
  discipline on the deflections than the direct gate does (chg·2.9e-10 vs 1e-7
  relative). Unverified: the exact faer kernel where the bits diverge; whether the
  multithreaded pkg (the isolated page) drifts the same way; the N100 native leg, which
  is slice 4, not this run.
- The panel's rung schedule is the ladder's coarse/refined pair (30k/150k, the worker
  wasm.ts constants); the native three-rung default is only avoided by the explicit
  `rungs` in this test's request.

---
*Generated by `npm run test:e2e` (web/tests/sweep.mjs) — do not edit by hand.*
