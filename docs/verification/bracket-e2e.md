# Bracket test — headless UI run

**Result: PASS** (scripted; the human hallway test is separate — see docs/hallway-test.md)

| Step | Time | Screenshot |
| :-- | --: | :-- |
| Empty document | 0.41 s | bracket-01.png |
| Base plate 60 × 40 × 8 | 1.17 s | bracket-02.png |
| Upright plate with 8 mm fillet | 1.52 s | bracket-03.png |
| Base hole hole1 ⌀6.5 | 2.19 s | bracket-04.png |
| Base hole hole2 ⌀6.5 | 2.15 s | bracket-05.png |
| Upright hole ⌀8.5 | 2.99 s | bracket-06.png |
| Setup lens | 0.83 s | bracket-07.png |
| Region: base-hole walls | 0.86 s | bracket-08.png |
| Region: upright-hole wall | 1.08 s | bracket-09.png |
| Support on the base_holes | 1.01 s | bracket-10.png |
| Load on the upright_hole | 1.36 s | bracket-11.png |
| Coarse result streamed | 1.08 s | bracket-12.png |
| Refined solve finished | 2.10 s | bracket-13.png |
| Results lens: stress field | 0.23 s | bracket-14.png |
| Probe the base plate | 0.20 s | bracket-15.png |
| Set the legend upper bound to 1 MPa | 0.26 s | bracket-16.png |
| Switch to deflection: range falls back to auto bounds | 0.30 s | bracket-17.png |
| Back to stress: the stored range still governs | 0.26 s | bracket-18.png |
| Reset the legend to the auto bounds | 0.27 s | bracket-19.png |
| Reset the deform scale | 0.34 s | bracket-20.png |
| Fill lens: add lattice fill | 6.87 s | bracket-21.png |
| Fill lens: badge and results rows | 0.19 s | bracket-22.png |
| Fill lens: ρ band cross-clamp and undo/redo | 2.03 s | bracket-23.png |
| Results lens: the temperature mode on a thermal doc | 6.81 s | bracket-24.png |

Total scripted time: 36.6 s.

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
| edit→coarse ≤ 1500 ms (GOAL budget) | pass | 1054 ms measured in-page |
| edit→refined ≤ 10 s (GOAL budget) | pass | 3167 ms measured in-page |
| element order P2 | pass | order 2 |
| probe hint names the surface click before any probe | pass | 1 hint elements |
| probe returns a stress in MPa on the base plate | pass | 8.79 MPa at (30.0, 19.5, 7.5) mm |
| probe hint replaced by the probe reading | pass | 0 hint elements after probing |
| results panel, badge and legend render | pass |  |
| provenance badge shows the refined level | pass | analysis-badge present once |
| rung table lists both levels and labels the change an estimate | pass | Rungsfieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not boundsrung 0coarse mesh · 48822 dofs · 9588 elements · wall 370 msrung 1refined · 150k DOF · 190440 dofs · 40643 elements · wall 2113 ms · case envelope vs run |
| Show seg fits the results panel (no cropped chip) | pass | seg 320px holds 320px of chips, panel 372px wide |
| legend range stored against this mode and rung | pass | {"mode":"stress","levelKey":"48:refined · 150k DOF","lo":0,"hi":1} |
| legend caption notes the saturation | pass | von Mises stress, element centroids; values outside the range render at the ramp's end colours |
| stress range kept but deflection legend shows auto bounds | pass | stress range kept; deflection upper bound 1.6763 ≈ auto 1.6763 |
| stress range survives the mode round trip | pass | upper bound restored to 1 MPa from the stored range |
| legend reset returns to the auto bounds | pass | range cleared, upper bound back to 47.028 MPa |
| deform scale reset returns to ×5 | pass | was ×20, back to ×5 |
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
| thermal doc solved: two temperature statements, nodal °C on the thermal case only | pass | temps 2, tref 20, cases 2, Thermal 1 at index 1, mechanical slots null true |
| nodal field keeps the Dirichlet bounds with a real gradient | pass | nodal range 20.0–80.0 °C |
| the legend names the envelope read in °C | pass | caption on screen |
| thermal case selectable, caption narrows to the nodal read | pass | resultsCase 1, caption 'temperature, nodal (°C)' shown true |

## Ladder provenance

| Level | Elements | DOFs | Order | Iterations | Mesh ms | Solve ms | Total ms | Peak vm | Max deflection |
| :-- | --: | --: | :-- | --: | --: | --: | --: | --: | --: |
| coarse mesh | 9588 | 48822 | 2 | 27 | 27 | 341 | 368 | 41.98 MPa | 2.331 mm |
| refined · 150k DOF | 40643 | 190440 | 2 | 24 | 47 | 2062 | 2109 | 47.03 MPa | 1.676 mm |

- Edit → coarse: measured on the page clock from the last edit commit to levelArrived[0]: 1054 ms of the 1500 ms budget; launch fired 397 ms after the edit (150 ms of that is the edit debounce).
- Edit → refined: 3167 ms of the 10000 ms budget.
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
