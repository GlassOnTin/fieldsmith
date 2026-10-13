# Bracket test — headless UI run

**Result: PASS** (scripted; the human hallway test is separate — see docs/hallway-test.md)

| Step | Time | Screenshot |
| :-- | --: | :-- |
| Empty document | 0.41 s | bracket-01.png |
| Base plate 60 × 40 × 8 | 1.10 s | bracket-02.png |
| Upright plate with 8 mm fillet | 1.50 s | bracket-03.png |
| Base hole hole1 ⌀6.5 | 2.06 s | bracket-04.png |
| Base hole hole2 ⌀6.5 | 2.74 s | bracket-05.png |
| Upright hole ⌀8.5 | 2.98 s | bracket-06.png |
| Setup lens | 0.81 s | bracket-07.png |
| Region: base-hole walls | 1.42 s | bracket-08.png |
| Region: upright-hole wall | 1.27 s | bracket-09.png |
| Support on the base_holes | 0.77 s | bracket-10.png |
| Load on the upright_hole | 1.16 s | bracket-11.png |
| Coarse result streamed | 0.74 s | bracket-12.png |
| Refined solve finished | 2.42 s | bracket-13.png |
| Results lens: stress field | 0.21 s | bracket-14.png |
| Probe the base plate | 0.21 s | bracket-15.png |
| Fill lens: add lattice fill | 7.50 s | bracket-16.png |
| Fill lens: badge and results rows | 0.55 s | bracket-17.png |
| Fill lens: ρ band cross-clamp and undo/redo | 2.36 s | bracket-18.png |

Total scripted time: 30.6 s.

- Modelled shapes equal `examples/bracket.fsm`: **yes**
- Console errors: **0**

## Checks

| Check | Result | Detail |
| :-- | :-- | :-- |
| document material is PETG | pass | petg |
| two regions were created | pass | ["base_holes","upright_hole"] |
| base_holes region covers hole1 + hole2 walls | pass | [["hole1","side"],["hole2","side"]] |
| upright_hole region covers hole3 wall | pass | [["hole3","side"]] |
| support fixes all three axes on the base_holes | pass | [{"id":"support1","region":"base_holes","fixed":[true,true,true]}] |
| force on the upright_hole, 400 N ±0.5 | pass | 400.00 N |
| force points down and 15° outward ±0.2° (y negative, z negative) | pass | 15.00° from vertical, y -103.53, z -386.37 |
| solve reached ready with coarse and refined levels | pass | ready; labels ["coarse mesh","refined · 150k DOF"];  |
| peak von Mises 20–100 MPa | pass | 47.0 MPa |
| max deflection 0.01–3 mm | pass | 1.676 mm |
| edit→coarse ≤ 1500 ms (GOAL budget) | pass | 724 ms measured in-page |
| edit→refined ≤ 10 s (GOAL budget) | pass | 3005 ms measured in-page |
| element order P2 | pass | order 2 |
| probe returns a stress in MPa on the base plate | pass | 8.79 MPa at (30.0, 19.5, 7.5) mm |
| results panel, badge and legend render | pass |  |
| provenance badge shows the refined level | pass | analysis-badge present once |
| rung table lists both levels and labels the change an estimate | pass | Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 48822 dofs · 9588 elements · wall 387 msrung 1refined · 150k DOF · 190440 dofs · 40643 elements · wall 2281 ms · case envelope vs run |
| fill panel shows one protect chip per shape | pass | 5 chips |
| graded ladder ran without error | pass | ready |
| both levels carried fill results | pass | ["coarse mesh","refined · 150k DOF"] |
| fill mass 20–40 g on the bracket | pass | 29.43 g of 43.69 g solid |
| achieved design density inside the spec band | pass | 0.380–0.800 of [0.2, 0.8] |
| lattice deflects more than solid (1.0–1.6× for −33 % mass) | pass | 1.215× deflection |
| graded peak stress within 0.9–1.5× of solid | pass | 1.053× peak vm |
| fill lens badge shows mass, peak and deflection | pass | mass 29.43 g · deflect 2.0375 mm · peak 49.5407 MPa |
| no duplicate analysis badge in the fill lens | pass | 0 analysis badges |
| fill summary lists mass and per-case deltas | pass | Mass29.43 g · solid 43.69 g (−33%)Achieved design ρ0.38…0.8σ ref4.37 MPamain vs  |
| ρ max below ρ min clamps up to ρ min | pass | [0.2,0.2] |
| undo restores the ρ band | pass | [0.2,0.8] |
| redo restores the clamped band | pass | [0.2,0.2] |

## Ladder provenance

| Level | Elements | DOFs | Order | Iterations | Mesh ms | Solve ms | Total ms | Peak vm | Max deflection |
| :-- | --: | --: | :-- | --: | --: | --: | --: | --: | --: |
| coarse mesh | 9588 | 48822 | 2 | 27 | 27 | 358 | 385 | 41.98 MPa | 2.331 mm |
| refined · 150k DOF | 40643 | 190440 | 2 | 24 | 50 | 2228 | 2278 | 47.03 MPa | 1.676 mm |

- Edit → coarse: measured on the page clock from the last edit commit to levelArrived[0]: 724 ms of the 1500 ms budget; launch fired 164 ms after the edit (150 ms of that is the edit debounce).
- Edit → refined: 3005 ms of the 10000 ms budget.
- Solve is cold-started at every level (PCG from zero displacement; no warm start is carried between refinement levels), so the level-to-level deltas are of cold solves.
- Timings measure this machine (software WebGL, JS shell): treat them as wall-clock evidence, not a performance guarantee.

## Document produced

```text
fieldsmith 1
name "Untitled"
material petg

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(60, 8, 40) at=(30, 4, 20) blend=8
cylinder hole1 r=3.25 h=20 at=(15, 26, 4) subtract
cylinder hole2 r=3.25 h=20 at=(45, 26, 4) subtract
cylinder hole3 r=4.25 h=20 at=(30, 4, 28) rotate=(90, 0, 0) subtract
```

---
*Generated by `npm run test:e2e` — do not edit by hand.*
