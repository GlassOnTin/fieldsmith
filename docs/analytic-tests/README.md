# Analytic Verification Test Suite (`A1`–`A14` · `V1`–`V60`)

Following the literate verification methodology pioneered by Stéphane Popinet in [**Gerris**](https://gfs.sourceforge.net/tests/tests/index.html) and [**Basilisk**](http://basilisk.fr/src/test/README), every physics use case in Fieldsmith is backed by self-contained test cases that pair:
1. **The governing continuum PDEs and boundary conditions** with an exact closed-form analytical solution (or certified dual variational bracket).
2. **The exact executable `.fsm` input script** and headless Rust test harness (`crates/fieldsmith-verify/tests/v*.rs`).
3. **Quantitative error norms and pass/fail tolerances** comparing Fieldsmith's 10-node quadratic (P2) tetrahedral finite-element solution against the closed-form reference.

To run the verification suite and regenerate `docs/verification/V*.md`:

```bash
cargo test -p fieldsmith-verify --release
```

To inspect or run any test interactively in the browser or desktop/mobile app, open **Tutorials & Analytic Tests (`?`)** and select **`📐 Analytic Verification Tests (14)`**, or click **`➕ Submit Case`** to package a new benchmark.

---

## Index by Physics Discipline

### 1. Linear Elasticity, Beam Theory & Stress Concentration (`A1`, `A2`, `A7`, `A9`)
- **[`A1` — Euler–Bernoulli & Timoshenko Cantilever Tip Deflection & Root Bending Stress](A1-cantilever-beam-theory.md)** ([`V1`](../verification/V1.md), [`V2`](../verification/V2.md))
  - *Governing Law*: `δ_Timo = F L³ / (3 E I) + F L / (κ G A)`, `σ_xx(0, c) = M c / I`
  - *Measured vs Analytic*: Tip deflection `0.1994 mm` vs `0.2016 mm` (`1.09%`); root stress `29.96 MPa` vs `30.00 MPa` (`0.13%`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v1_v2_cantilever`
- **[`A2` — Kirsch (1898) Infinite Plate with a Central Circular Hole](A2-kirsch-plate-with-hole.md)** ([`V3`](../verification/V3.md))
  - *Governing Law*: `σ_θθ(a, ±π/2) = 3 σ₀` (`K_t = 3.0`)
  - *Measured vs Analytic*: Nominal `10.00 MPa` (`0.00%`); element-centroid ring `20.92 MPa` vs `20.70 MPa` (`1.06%`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v3_plate_hole`
- **[`A7` — Spatially Distributed Surface Tractions (`fn` Ramp & Piecewise Load Profiles)](A7-distributed-face-tractions.md)** ([`V15`](../verification/V15.md))
  - *Governing Law*: `E I w''''(x) = w₀ (x/L) ⇒ δ_tip = 11 w₀ L⁴ / (120 E I)`, `R_z = −½ w₀ L`, `M_y = (2/3) L |R_z|`
  - *Measured vs Analytic*: Ramp `δ_tip = 0.22029 mm` vs `0.22000 mm` (`0.130%`); `M_y = 13,332.89 N·mm` vs `13,333.33 N·mm` (`0.003%`); Piecewise `δ_tip = 0.23055 mm` vs `0.23145 mm` (`0.389%`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v15_distributed`
- **[`A9` — Certified Prager–Synge / Fenchel Dual Compliance Bounds (`C_h ≤ C_true ≤ C(σ*)`)](A9-dual-compliance-bounds-prager-synge.md)** ([`V34`](../verification/V34.md), [`V35`](../verification/V35.md))
  - *Governing Law*: Galerkin orthogonality `C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0` and `H(div)` equilibrated stress recovery `J(σ*)`
  - *Measured vs Analytic*: In-span `0.4000000 N·mm` vs `0.4000000 N·mm` (`0.000%`); off-span cubic `C_h = 18,431.93 N·mm`, `C(σ*) = 18,431.99 N·mm` vs `C_true = 18,432.00 N·mm`
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v34_bounds --test v35_bounds`

### 2. Differential Geometry, Exact CSG Volume & STEP B-Rep Assemblies (`A3`)
- **[`A3` — De Laval Converging–Diverging Nozzle Shell Volume vs Pappus & Conical Frusta](A3-de-laval-revolve-pappus-volume.md)** ([`V4`](../verification/V4.md), [`V60`](../verification/V60.md))
  - *Governing Law*: `V = (1/3) ∫ (x · n) dA = 2π ∑ r̄_k A_k = ∑ (π L_k / 3) [(R_o1² + R_o1 R_o2 + R_o2²) − (R_i1² + R_i1 R_i2 + R_i2²)]`
  - *Measured vs Analytic*: `136,358.73 mm³` (72-segment revolve) vs `136,722.11 mm³` exact continuum (`0.266%` polygonal chord error; `0.000%` against exact 72-gon prismoid)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v60_regenerative_chamber_multiphysics --test v4_volume`

### 3. Pressure Vessels, Steady Fourier Conduction & Coupled Thermo-Elasticity (`A4`, `A5`, `A8`)
- **[`A4` — Thick-Walled Lamé (1852) Pressurized Rocket Chamber & Coolant Jacket Bicylinder](A4-lame-pressurized-bicylinder.md)** ([`V20`](../verification/V20.md), [`V60`](../verification/V60.md))
  - *Governing Law*: `σ_rr(r) = A − B/r²`, `σ_θθ(r) = A + B/r²`, `σ_rr + σ_θθ = 2A = const` under inner bore pressure `p_i = 15 MPa` and outer jacket pressure `p_o = 3 MPa`
  - *Measured vs Analytic*: Radial displacement `u_r(r)` within `0.56%–1.16%`; von Mises `σ_vm(r)` within `0.57%–1.23%`; invariant `σ_rr + σ_θθ = 1.997 MPa` vs `2.000 MPa`
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v60_regenerative_chamber_multiphysics v60b`
- **[`A5` — Cylindrical Regenerative Cooling: Hot-Gas Heat Flux + Coolant Convection Thermo-Elastic Lamé](A5-regenerative-cooling-robin-lame.md)** ([`V20`](../verification/V20.md), [`V60`](../verification/V60.md))
  - *Governing Law*: Mixed Neumann–Robin cylindrical conduction `T(r) = T_∞ + (r_i q_in)/(r_o h_c) + r_i q_in ln(r_o / r)` coupled to Duhamel–Neumann radial thermo-elasticity
  - *Measured vs Analytic*: `T(r)` within `0.31%–0.36%`; `u_r(r)` within `1.28%–1.42%`; axial thermal stress `σ_zz(r)` within `1.24%–2.01%`
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v60_regenerative_chamber_multiphysics v60c`
- **[`A8` — Thermo-Elastic Free Expansion vs End-Clamped Uniaxial Thermal Stress](A8-thermoelastic-free-and-clamped-bar.md)** ([`V19`](../verification/V19.md))
  - *Governing Law*: Free expansion `u* = α ΔT x` (`σ* = 0`) vs end-fixed `σ_xx = −E α ΔT`, `u_y = (1 + ν) α ΔT y`
  - *Measured vs Analytic*: Free stress `2.99e-7 MPa` vs `0 MPa`; clamped `σ_xx = -64.40000 MPa` vs `-64.40000 MPa` (`0.000%`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v19_thermal_free_expansion`

### 4. Structural Dynamics & Modal Eigenproblems (`A6`)
- **[`A6` — Axisymmetric Tubular Thrust Chamber Cantilever Modal Bending Doublet](A6-tubular-chamber-modal-doublet.md)** ([`V13`](../verification/V13.md), [`V60`](../verification/V60.md))
  - *Governing Law*: `K φ_k = ω_k² M φ_k` on a hollow circular tube (`r_i = 6 mm, r_o = 12 mm, L = 160 mm`) with Cowper (1966) shear coefficient `κ_tube = 0.6032`
  - *Measured vs Analytic*: Modes 1 & 2 = `717.12 Hz / 717.12 Hz` vs `728.26 Hz` Cowper–Timoshenko (`1.53%` error, `0.000%` SO(2) doublet split)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v60_regenerative_chamber_multiphysics v60d`

### 5. Multi-Material Continuum Assemblies, Fasteners & Bolt Preload (`A10`, `A13`)
- **[`A10` — Multi-Material Series Composite Bar, Bimetal Thermal Mismatch & Bolt Preload](A10-multimaterial-series-bar-and-preload.md)** ([`V53`](../verification/V53.md))
  - *Governing Law*: Reuss series bar `δ_series = σ₀ L (1/E₁ + 1/E₂)`, composite mass `m = ∑ ρ_k V_k`, and opposed preload linearity
  - *Measured vs Analytic*: `δ_tip = 0.02016 mm` vs `0.01947 mm` (`3.56%`); mass `3.7894 g` vs `3.8376 g` (`1.26%`); preload linearity `2.00000×` (`0.000%`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v53_multibody_preload`
- **[`A13` — Fastener Inclined-Plane Thread Torque Envelope `[T_min, T_max]` & Weibull Failure Regression](A13-fastener-thread-torque-and-weibull.md)** ([`V53`](../verification/V53.md))
  - *Governing Law*: Kellermann–Klein `T = F [½ d₂ tan(ψ + ρ') + μ_b r_b]`, Alexander thread stripping, and 2-parameter Weibull OLS/MLE
  - *Measured vs Analytic*: `4 / 4` NASA-STD-5020B, ECSS-E-ST-32-08C, VDI 2230 & Bossard benchmarks pass (`0.000%` decomposition residual, `R² = 0.996`)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v57_fixings_torque_weibull`

### 6. Architected TPMS Lattices & Additive Manufacturing Physics (`A11`, `A12`)
- **[`A11` — TPMS Lattice Homogenisation: Cubic Zener Anisotropy & Hill–Mandel Yield/Buckling Laws](A11-tpms-cubic-anisotropy-and-gibson-ashby.md)** ([`V6`](../verification/V6.md), [`V36`](../verification/V36.md), [`V42`](../verification/V42.md), [`V48`](../verification/V48.md))
  - *Governing Law*: Cubic directional modulus `1/E(n) = S₁₁ − 2(S₁₁ − S₁₂ − ½ S₄₄)(n₁²n₂² + n₂²n₃² + n₃²n₁²)` and power-law yield/buckling across all 6 TPMS families
  - *Measured vs Analytic*: `E_[110] / E_[100] = 1.13583` vs `1.13584` (`0.001%`); 3D cubic patch stress error `5.33e-15`; coupon yield/buckling within `0.06%–0.59%` on gyroid sheet
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v42_anisotropic_route --test v48_tpms_yield_buckling`
- **[`A12` — Additive Manufacturing Physics: Spherical-Cap Overhang Area & Puck/Hoffman Layer Peel](A12-additive-overhang-and-layer-peel.md)** ([`V45`](../verification/V45.md), [`V46`](../verification/V46.md))
  - *Governing Law*: `A_overhang(α_max) = 2π r² (1 − sin α_max)` and `U_peel(θ) = √((σ₀ cos²θ / S_z)² + (σ₀ sin θ cos θ / S_s)²)`
  - *Measured vs Analytic*: Overhang cap `314.27 mm²` vs `314.16 mm²` (`0.035%` at `30°`); 3D FEA `U_peel(45°) = 0.901388×` (`3.05e-10` error)
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v45_overhang --test v46_layer_peel`

### 7. Electromagnetics, 50 Ω RF Transmission Lines & Dipole Radiation (`A14`)
- **[`A14` — 50 Ω Smith Chart Möbius Reflection `Γ(Z_L)`, VSWR, CFRP Eddy Loss & Dipole Lobes](A14-rf-smith-chart-vswr-dipole-lobes.md)** ([`V42`](../verification/V42.md))
  - *Governing Law*: `Γ(Z_L) = (Z_L − Z₀)/(Z_L + Z₀)`, `VSWR = (1 + |Γ|)/(1 − |Γ|)`, and half-wave dipole gain `G(θ) = 1.6409 [cos((π/2)cos θ)/sin θ]²`
  - *Measured vs Analytic*: `Γ(50 Ω) = 0`, `Γ(100 Ω) = +1/3` (`VSWR = 2.000000`, `0.000%`); dual-band L-matched `S₁₁ = -24.1 / -26.8 dB`
  - *Reproduce*: `cargo test -p fieldsmith-verify --test v59_drone_antenna_vna_lobes`
