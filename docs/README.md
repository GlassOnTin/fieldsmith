# Fieldsmith Documentation, Basilisk-Style Analytic Verification Suite & Multiphysics Case Studies

Every sheet below is available both as a standalone Markdown document in this repository (`docs/`) and as an interactive guided walkthrough inside the Fieldsmith application (**Tutorials & Analytic Tests (`?`)** drawer, with 1-click `.fsm` loading, step-by-step lens guidance, theoretical derivations, live MCP/REST API calls, an in-app Markdown reader, and a **`➕ Submit Case`** packager).

## Quick Links & Categories

- **[📐 Analytic Verification Suite (`docs/analytic-tests/` — `A1`–`A14`)](#1-analytic-verification-tests-a1a14--basilisk--gerris-style)**: Full index in **[`docs/analytic-tests/README.md`](analytic-tests/README.md)**. Closed-form continuum mechanics, heat transfer, modal dynamics, composite assemblies, TPMS homogenisation, additive overhang/peel, fastener mechanics, and 50 Ω RF problems compared quantitatively against Fieldsmith P2 FEA (`V1`–`V60`).
- **[📘 Guided Tutorials (`docs/tutorials/` — `T1`–`T7`, `T14`–`T15`)](#2-guided-tutorials-t1t7-t14t15)**: Progressive hands-on lessons covering linear elasticity, parametric sweeps, thermo-elasticity, modal vibration, additive manufacturing overhang/peel, stress-graded TPMS gyroids, fasteners/Weibull preload, 2D constraint sketches, and MCP/REST co-design.
- **[🚀 Multiphysics Case Studies (`docs/case-studies/` — `T8`–`T13`, `T16`–`T17`)](#3-multiphysics-case-studies-t8t13-t16t17)**: End-to-end engineering and open-source hardware analyses, including the open-source GOX/Ethanol regeneratively cooled liquid rocket thrust chamber (`T17`, `V60`), DeadCat FPV drone dual-band antenna (`T16`, `V59`), multi-zone TPMS heat exchanger (`T11`, `V54`), and level-set topology evolution (`T12`, `V50/V55`).
- **[➕ Contributing Examples & Analytic Benchmarks (`docs/CONTRIBUTING_EXAMPLES.md`)](CONTRIBUTING_EXAMPLES.md)**: How to package and submit a new `.fsm` example, tutorial, or analytic verification benchmark from the Fieldsmith UI or CLI.
- **[✓ Automated Verification Reports (`docs/verification/V1.md`–`V60.md`)](#4-automated-verification-suite-reports-v1v60)**: Self-contained test reports emitted by `cargo test -p fieldsmith-verify`.

---

## 1. Analytic Verification Tests (`A1`–`A14` · Basilisk / Gerris Style)

See **[`docs/analytic-tests/README.md`](analytic-tests/README.md)** for the discipline-by-discipline PDE breakdown and 1-line `cargo test` commands.

| ID | Title | Closed-Form Benchmark | Measured vs Analytic | Verification Report |
| :--- | :--- | :--- | :--- | :--- |
| **A1** | [A1 — Analytic Benchmark A1: Euler–Bernoulli & Timoshenko Cantilever Tip Deflection & Root Stress (V1 / V2)](analytic-tests/A1-cantilever-beam-theory.md) | Verified by V1 & V2 · Closed-Form Beam Theory · PASS (< 1.5%) | 0.2016 mm vs 0.1994 mm (1.09%); 30.00 MPa vs 29.96 MPa (0.13%) | [`V1`](verification/V1.md), [`V2`](verification/V2.md) |
| **A2** | [A2 — Analytic Benchmark A2: Kirsch Plate with a Central Circular Hole Stress Concentration (V3)](analytic-tests/A2-kirsch-plate-with-hole.md) | Verified by V3 · Kirsch (1898) Exact Elasticity · PASS (< 2.5%) | 10.00 MPa vs 10.00 MPa (0.00%); 20.70 MPa vs 20.92 MPa (1.06%) | [`V3`](verification/V3.md) |
| **A3** | [A3 — Analytic Benchmark A3: De Laval Converging–Diverging Nozzle Shell Volume vs Pappus & Conical Frusta (V4 / V60a)](analytic-tests/A3-de-laval-revolve-pappus-volume.md) | Verified by V4 & V60(a) · Exact Pappus / Frusta Volume · PASS (0.27%) | 88,467.25 mm³ vs Exact Algebraic Anchor (0.000%); 48,254.86 mm³ vs Exact Algebraic Anchor (0.000%) | [`V4`](verification/V4.md), [`V60`](verification/V60.md) |
| **A4** | [A4 — Analytic Benchmark A4: Thick-Walled Lamé Pressurized Rocket Chamber & Coolant Jacket (V20 / V60b)](analytic-tests/A4-lame-pressurized-bicylinder.md) | Verified by V60(b) · Exact Lamé (1852) Bicylinder · PASS (< 1.2%) | 1.560 / 1.167 / 0.945 µm vs 1.568 / 1.178 / 0.956 µm (0.56% / 0.94% / 1.16%); 2.000 MPa (Constant ∀ r) vs 1.997 / 2.143 / 2.125 MPa (Δ ≤ 0.143 MPa) | [`V20`](verification/V20.md), [`V60`](verification/V60.md) |
| **A5** | [A5 — Analytic Benchmark A5: Cylindrical Regenerative Cooling — Hot-Gas Heat Flux + Coolant Convection Thermo-Elastic Lamé (V60c)](analytic-tests/A5-regenerative-cooling-robin-lame.md) | Verified by V60(c) · Exact Robin–Neumann Logarithmic Thermo-Elasticity · PASS (< 2.0%) | 59.892 / 44.384 / 32.565 °C vs 59.694 / 44.224 / 32.464 °C (0.33% / 0.36% / 0.31%); 6.493 / 9.410 / 10.580 µm vs 6.402 / 9.290 / 10.430 µm (1.41% / 1.28% / 1.42%) | [`V20`](verification/V20.md), [`V60`](verification/V60.md) |
| **A6** | [A6 — Analytic Benchmark A6: Axisymmetric Tubular Thrust Chamber Cantilever Modal Bending Doublet (V13 / V60d)](analytic-tests/A6-tubular-chamber-modal-doublet.md) | Verified by V13 & V60(d) · Cowper–Timoshenko Tubular Eigenmodes · PASS (1.53%) | r_gyr = √(I/A) = 6.7082 mm vs Exact Section (0.00%); 728.26 Hz vs 717.12 Hz / 717.12 Hz (1.53%) | [`V13`](verification/V13.md), [`V60`](verification/V60.md) |
| **A7** | [A7 — Analytic Benchmark A7: Spatially Distributed Surface Tractions — Triangular Ramp & Piecewise Load Profiles (V15)](analytic-tests/A7-distributed-face-tractions.md) | Verified by V15 · Exact Face-Local Traction Quadrature · PASS (0.13%) | 0.220000 mm vs 0.220286 mm (0.130%); -200.00 N / 13,333.33 N·mm vs -200.00 N / 13,332.89 N·mm (0.000% / 0.003%) | [`V15`](verification/V15.md) |
| **A8** | [A8 — Analytic Benchmark A8: Thermo-Elastic Free Expansion vs End-Clamped Uniaxial Thermal Stress (V19)](analytic-tests/A8-thermoelastic-free-and-clamped-bar.md) | Verified by V19 · Exact Duhamel–Neumann Patch Identities · PASS (0.000%) | max |u − u*| = 0 mm,  |σ| = 0 MPa vs 9.98×10⁻¹² mm,  2.99×10⁻⁷ MPa (0.000%); -64.40000 MPa vs -64.40000 MPa (0.000%) | [`V19`](verification/V19.md) |
| **A9** | [A9 — Analytic Benchmark A9: Certified Dual Prager–Synge / Fenchel Compliance Bounds C_h ≤ C_true ≤ C(σ*) (V34 / V35)](analytic-tests/A9-dual-compliance-bounds-prager-synge.md) | Verified by V34 & V35 · Certified Primal–Dual Energy Bracket · PASS (0.001%) | 0.02666667 N·mm (Pipeline) / 0.4000000 N·mm (T1) vs 0.02666667 N·mm / 0.4000000 N·mm (0.000%); 18,432.00 N·mm vs 18,430.86 → 18,431.93 N·mm (0.006% → 0.0004%) | [`V34`](verification/V34.md), [`V35`](verification/V35.md) |
| **A10** | [A10 — Analytic Benchmark A10: Multi-Material Series Composite Bar, Bimetal Thermal Mismatch & Bolt Preload (V53)](analytic-tests/A10-multimaterial-series-bar-and-preload.md) | Verified by V53 · Multi-Volume Continuum & Preload · PASS (< 3.7%) | 0.019467 mm vs 0.020160 mm (3.56% (3D interface Poisson)); 1.87857 × vs 1.80938 × (3.68%) | [`V53`](verification/V53.md) |
| **A11** | [A11 — Analytic Benchmark A11: TPMS Lattice Homogenisation — Cubic Zener Anisotropy & Hill–Mandel Yield/Buckling Laws (V6 / V36 / V42 / V48)](analytic-tests/A11-tpms-cubic-anisotropy-and-gibson-ashby.md) | Verified by V6, V36, V42 & V48 · Cubic Homogenisation & Failure Laws · PASS (0.001%) | 1.13584 / 2.06942 / 0.81279 vs 1.13583 / 2.06943 / 0.81278 (0.001% / 0.000% / 0.001%); 0.000 relative error vs 5.33×10⁻¹⁵ / 4.00×10⁻¹⁵ rel (0.000%) | [`V6`](verification/V6.md), [`V36`](verification/V36.md), [`V42`](verification/V42.md), [`V48`](verification/V48.md) |
| **A12** | [A12 — Analytic Benchmark A12: Additive Manufacturing Physics — Spherical-Cap Overhang Area & Orthotropic Layer-Peel Criterion (V45 / V46)](analytic-tests/A12-additive-overhang-and-layer-peel.md) | Verified by V45 & V46 · Exact Overhang & Orthotropic Peel · PASS (< 0.15%) | 314.16 / 184.03 / 84.18 mm² vs 314.27 / 183.82 / 83.72 mm² (0.035% / 0.115% / 0.541%); 1.00000 / 0.90139 / 0.00000 × vs 1.00000 / 0.90139 / 4.66×10⁻¹⁰ × (3.05×10⁻¹⁰ ×) | [`V45`](verification/V45.md), [`V46`](verification/V46.md) |
| **A13** | [A13 — Analytic Benchmark A13: Fastener Inclined-Plane Thread Torque Envelope [T_min, T_max] & Weibull Regression (V57)](analytic-tests/A13-fastener-thread-torque-and-weibull.md) | Verified by V57 · NASA/ECSS/VDI 2230 Fastener Standards · PASS (< 0.1%) | Exact Algebraic Identity (T_thread + T_collar = T_total) vs Exact Match across all 7 Fixing Kinds (0.000%); 4 / 4 Open Standards PASS (NASA, ECSS, VDI 2230, Bossard) vs 4 / 4 PASS (T_rec within [T_min, T_max]) (0.000%) | [`V53`](verification/V53.md) |
| **A14** | [A14 — Analytic Benchmark A14: Electromagnetics & RF — 50 Ω Smith Chart Möbius Reflection Γ(Z_L), VSWR & Dipole Lobes (V59)](analytic-tests/A14-rf-smith-chart-vswr-dipole-lobes.md) | Verified by V59 · Exact Smith Chart & RF Scattering Identities · PASS (0.000%) | Γ(50)=0,  Γ(100)=+1/3 (VSWR=2.000),  |Γ(j50)|=1.000 vs 0.000,  +0.333333 (VSWR=2.000000),  1.000 (0.000%); S₁₁ ≤ −15 dB (VSWR ≤ 1.43),  CFRP Loss < 8% vs 915 MHz: −24.1 dB (1.13); 2.44 GHz: −26.8 dB (1.10) (All 6 V59 Targets Met) | [`V42`](verification/V42.md) |

---

## 2. Guided Tutorials (`T1`–`T7`, `T14`–`T15`)

| ID | Title | Level | Literate `.fsm` Source | Verification & Proof Links |
| :--- | :--- | :--- | :--- | :--- |
| **T1** | [T1 — Designing & Stress-Testing a Load-Bearing L-Bracket](tutorials/T1-cantilever-bracket.md) | Beginner | [`T1-cantilever-bracket.fsm`](../examples/tutorials/T1-cantilever-bracket.fsm) | [`V1`](verification/V1.md), [`V2`](verification/V2.md) |
| **T2** | [T2 — Named Dimensions (let) & Automated Design Trade-Off Sweeps](tutorials/T2-parametric-cantilever-sweeps.md) | Beginner | [`T2-parametric-cantilever-sweeps.fsm`](../examples/tutorials/T2-parametric-cantilever-sweeps.fsm) | [`V23`](verification/V23.md), [`V34`](verification/V34.md), [`V35`](verification/V35.md) |
| **T3** | [T3 — How Heat Warps Parts: Thermal Conduction & Expansion Stress](tutorials/T3-thermo-elastic-bracket.md) | Intermediate | [`T3-thermo-elastic-bracket.fsm`](../examples/tutorials/T3-thermo-elastic-bracket.fsm) | [`V19`](verification/V19.md), [`V20`](verification/V20.md) |
| **T4** | [T4 — Natural Vibration Frequencies: Why Parts Hum, Shake or Resonate](tutorials/T4-modal-vibration-frequencies.md) | Intermediate | [`T4-modal-vibration-frequencies.fsm`](../examples/tutorials/T4-modal-vibration-frequencies.fsm) | [`V13`](verification/V13.md) |
| **T5** | [T5 — 3D Printing Strength: Preventing Layer Splitting & Choosing Print Orientation](tutorials/T5-additive-layer-peel-orientation.md) | Intermediate | [`T5-additive-layer-peel-orientation.fsm`](../examples/tutorials/T5-additive-layer-peel-orientation.fsm) | [`V45`](verification/V45.md), [`V46`](verification/V46.md) |
| **T6** | [T6 — Lightweighting Like Bone: Stress-Graded Internal Gyroid Lattices](tutorials/T6-stress-graded-tpms-gyroid.md) | Advanced | [`T6-stress-graded-tpms-gyroid.fsm`](../examples/tutorials/T6-stress-graded-tpms-gyroid.fsm) | [`V6`](verification/V6.md), [`V7`](verification/V7.md), [`V48`](verification/V48.md) |
| **T7** | [T7 — Human + AI Co-Design: Repairing a Weak Part Together in Real Time](tutorials/T7-human-ai-codesign-mcp.md) | Advanced | [`T7-human-ai-codesign-mcp.fsm`](../examples/tutorials/T7-human-ai-codesign-mcp.fsm) | [`V22-MCP`](verification/V22-MCP.md), [`V51`](verification/V51.md) |
| **T14** | [T14 — Engineering Fixings: Minimal & Maximal Torque, Weibull/Creep Spreadsheet & JWST Qualification](tutorials/T14-fastener-torque-weibull-preload.md) | Flagship Multi-Physics | [`T14-fastener-torque-weibull-preload.fsm`](../examples/tutorials/T14-fastener-torque-weibull-preload.fsm) | [`V53`](verification/V53.md) |
| **T15** | [T15 — Parametric 2D Sketching, Jacobian QR Constraint Solver & Surface-Attached 3D Feature Engine](tutorials/T15-parametric-2d-sketch-constraints.md) | Intermediate | [`T15-parametric-2d-sketch-constraints.fsm`](../examples/tutorials/T15-parametric-2d-sketch-constraints.fsm) | [`V4`](verification/V4.md) |

---

## 3. Multiphysics Case Studies (`T8`–`T13`, `T16`–`T17`)

| ID | Title | Domain & Coupled Physics | Literate `.fsm` Source | Verification Report |
| :--- | :--- | :--- | :--- | :--- |
| **T8** | [T8 — Coupled Multi-Physics Systems: When Fluid Flow, Heat, Chemistry & Structure Interact](case-studies/T8-float-bath-cvd-coater.md) | Lesson 8 · Advanced · Coupled Multi-Physics | [`T8-float-bath-cvd-coater.fsm`](../examples/case-studies/T8-float-bath-cvd-coater.fsm) | [`V52`](verification/V52.md) |
| **T9** | [T9 — Swirl Atomiser Nozzle, Combustor & Heat Exchanger — Predicting NOx](case-studies/T9-swirl-atomiser-low-nox-combustor.md) | Lesson 9 · Multi-Physics · Swirl Spray, Heat Exchanger & NOx | [`T9-swirl-atomiser-low-nox-combustor.fsm`](../examples/case-studies/T9-swirl-atomiser-low-nox-combustor.fsm) | [`V51`](verification/V51.md) |
| **T10** | [T10 — Multi-Material Bolted Assembly & Bolt Preload — V53](case-studies/T10-bimetal-preloaded-flange-valve.md) | V53 · Multi-Body Assembly · Per-Volume Material & Bolt Preload | [`T10-bimetal-preloaded-flange-valve.fsm`](../examples/case-studies/T10-bimetal-preloaded-flange-valve.fsm) | [`V12`](verification/V12.md), [`V53`](verification/V53.md) |
| **T11** | [T11 — Spatially Varying Cell-Size & Multi-Zone Lattice Morphing — V54](case-studies/T11-multizone-tpms-heat-exchanger.md) | V54 · Multi-Zone TPMS · Cell-Size Grading & Family Morphing | [`T11-multizone-tpms-heat-exchanger.fsm`](../examples/case-studies/T11-multizone-tpms-heat-exchanger.fsm) | [`V36`](verification/V36.md), [`V54`](verification/V54.md) |
| **T12** | [T12 — Topology Optimization & Level-Set Boundary Evolution — V55](case-studies/T12-levelset-evolver-aerospace-bracket.md) | V55 · Level-Set Topology Evolution & Conformal Infill | [`T12-levelset-evolver-aerospace-bracket.fsm`](../examples/case-studies/T12-levelset-evolver-aerospace-bracket.fsm) | [`V50`](verification/V50.md), [`V55`](verification/V55.md) |
| **T13** | [T13 — Solar-Focused Optical Lens Candle Ignition, Buoyant Flame, Smoke Plume & Real-Time Video](case-studies/T13-solar-parabolic-candle-igniter.md) | V56 Verified (144-DOF Monolithic Newton · 40 Transient Frames · 5 Recap Milestones) | [`T13-solar-parabolic-candle-igniter.fsm`](../examples/case-studies/T13-solar-parabolic-candle-igniter.fsm) | [`V56`](verification/V56.md) |
| **T16** | [T16 — 4-Inch DeadCat FPV Drone & Gemini Dual-Band (900 MHz + 2.4 GHz) Antenna VNA, 50 Ω Smith Chart & 3D Lobe Optimisation](case-studies/T16-deadcat-fpv-drone-gemini-antenna.md) | Verified by V59 (Dual-Band VNA S₁₁/S₂₁ + 50 Ω Smith Chart + 3D Polar Lobes + CFRP Eddy Loss + Auto-Optimiser) | [`T16-deadcat-fpv-drone-gemini-antenna.fsm`](../examples/case-studies/T16-deadcat-fpv-drone-gemini-antenna.fsm) | [`V42`](verification/V42.md) |
| **T17** | [T17 — Open-Hardware Regeneratively Cooled Liquid Rocket Chamber: GOX/Ethanol De Laval Contour & Stress-Graded TPMS Gyroid Jacket](case-studies/T17-regenerative-rocket-nozzle.md) | Verified by V60 (De Laval Pappus Volume + Bicylinder Lamé + Robin–Neumann Regenerative Cooling + Tubular Modal) | [`T17-regenerative-rocket-nozzle.fsm`](../examples/case-studies/T17-regenerative-rocket-nozzle.fsm) | [`V60`](verification/V60.md), [`V20`](verification/V20.md) |

---

## 4. Automated Verification Suite Reports (`V1`–`V60`)

All verification reports live under [`docs/verification/`](verification/) and are generated directly from the Rust verification suite:

```bash
cargo test -p fieldsmith-verify --release
cargo test -p fieldsmith-verify --test v60_regenerative_chamber_multiphysics -- --nocapture
```

To contribute a new example or analytic verification test, see **[`docs/CONTRIBUTING_EXAMPLES.md`](CONTRIBUTING_EXAMPLES.md)** or click **`➕ Submit Case`** inside the Fieldsmith **Tutorials & Analytic Tests** drawer.
