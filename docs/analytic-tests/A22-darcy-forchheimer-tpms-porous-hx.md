# A22 — Analytic Benchmark A22: 3D P2 Darcy–Forchheimer TPMS Porous Flow & Balanced Counterflow ε–NTU Heat Exchange

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by A22 & V63 · Exact 3D P2 Nonlinear Darcy–Forchheimer Patch Test + Counterflow ε–NTU
> **Repository Path**: `docs/analytic-tests/A22-darcy-forchheimer-tpms-porous-hx.md` · **Starter Model**: `a22-darcy-forchheimer-tpms-porous-hx.fsm` · **Verification Reports**: `docs/verification/V36.md`, `docs/verification/V63.md`

Verify 3D quadratic P2 nonlinear Darcy–Forchheimer porous-media pressure-drop (`−∇·(κ_eff(|∇p|) ∇p) = 0`), minimal-surface partition area density `a_v`, Kozeny–Carman permeability `K`, Ergun–Forchheimer drag `β`, and balanced two-stream counterflow `ε = NTU / (1 + NTU)` conjugate heat transfer on a uniform AlSi10Mg Gyroid sheet core.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Gyroid Sheet Minimal-Surface Area Density a_v | `a_v = c_area / a = 3.0915 / 0.005 m (H = 0 parallel offset invariance)` | **618.30 m⁻¹** | **618.30 m⁻¹** | < 0.05% | ≤ 0.5% |
| Homogenised Kozeny–Carman Permeability K | `K = φ³ / (k_KC (2 a_v)²)  (φ = 0.75, k_KC = 5.0)` | **0.05518 mm²** | **0.05518 mm²** | < 0.05% | ≤ 0.5% |
| Nonlinear 3D P2 Darcy–Forchheimer Pressure Drop Δp | `Δp = L ( (μ/K) u_D + β ρ_f u_D² )  (u_D = 0.2006 m/s, Re_h = 727)` | **2.4819 kPa** | **2.4819 kPa (3D P2 Picard/BB)** | < 0.05% | ≤ 0.5% |
| Balanced Counterflow Effectiveness & Heat Duty | `ε = NTU / (1 + NTU),  Q = ε C_min (T_{h,in} − T_{c,in})` | **ε = 0.3180,  Q = 6914.7 W** | **ε = 0.3180,  Q = 6914.7 W** | < 0.05% | ≤ 0.5% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A22 — Darcy-Forchheimer TPMS Porous Flow & Counterflow HX"
material alsi10mg

box core size=(60, 20, 20) at=(30, 10, 10)

region in_port = core.nx
region out_port = core.px

support anchor in_port fixed=xyz
load internal_p out_port pressure=-0.5 case="ProofPressure"

fill gyroid sheet cell=5.0 skin=0.0 rho=(0.25, 0.25) wall=0.1
hx inlet=in_port outlet=out_port mdot=0.08 tin=85 fluid=water arrangement=counterflow cold_tin=20 pout=100

solve

assert "Gyroid minimal-surface area density a_v" metric="hx_area_density_per_m" target=618.30 tol="0.5%" unit="1/m"
assert "Gyroid Kozeny-Carman permeability K" metric="hx_permeability_mm2" target=0.055177 tol="0.5%" unit="mm^2"
assert "Wetted partition area A_part" metric="hx_wetted_area_cm2" target=148.392 tol="0.5%" unit="cm^2"
assert "Hydraulic diameter D_h" metric="hx_hydraulic_diam_mm" target=2.4260 tol="0.5%" unit="mm"
assert "Nonlinear Darcy-Forchheimer pressure drop" metric="hx_pressure_drop_kpa" target=2.4819 tol="0.5%" unit="kPa"
assert "Viscous Darcy share in inertial regime" metric="hx_darcy_share_pct" target=7.822 tol="1.0%" unit="%"
assert "Two-stream total pumping power" metric="hx_pumping_power_w" target=0.3983 tol="0.5%" unit="W"
assert "Balanced counterflow effectiveness" metric="hx_effectiveness" target=0.3180 tol="0.5%" unit="-"
assert "Conjugate counterflow heat duty" metric="hx_heat_duty_w" target=6914.7 tol="0.5%" unit="W"
assert "Hot stream outlet temperature" metric="hot_outlet_temp_c" target=64.332 tol="0.5%" unit="C"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Uniform Gyroid Core & Porous HX Statement in Fill (4) *(Lens: `FILL`)*

Open **4. Fill**. The `60 × 20 × 20 mm` AlSi10Mg core (`k_s = 130 W/(m·K)`) uses a uniform Gyroid sheet lattice (`cell = 5 mm`, `skin = 0`, `ρ = 0.25`, `wall = 0.1 mm`) with `hx inlet=in_port outlet=out_port mdot=0.08 tin=85 fluid=water arrangement=counterflow cold_tin=20 pout=100`.

- **Expected Check**: For a minimal surface (`H = 0`), parallel-sheet offset preserves area to second order (`a_v = 3.0915 / 5 mm = 618.30 m⁻¹`), giving total porosity `φ = 1 − 0.25 = 0.75`, channel hydraulic diameter `D_h = 4 φ / (2 a_v) = 2.426 mm`, and permeability `K = 0.05518 mm²`.

### 2. Verify 3D P2 Porous Pressure p(x) & Counterflow ε–NTU Balance in Results (3) *(Lens: `RESULTS`)*

Switch to **3. Results**, inspect the **TPMS Porous Flow & Conjugate HX** summary card, and click **Show Porous Pressure p** (or **💧 Porous Pressure p**).

- **Expected Check**: At `u_D = 0.2006 m/s` (`Re_h = 727`), quadratic Forchheimer inertial drag accounts for `92.2%` of total resistance (`Darcy 7.82%`), yielding `Δp = 2.4819 kPa` (`p: 102.48 → 100.00 kPa` linearly along `x`), `UA = 155.97 W/K`, `NTU = 0.4663`, `ε = NTU/(1+NTU) = 31.80%`, and `Q = 6.915 kW` (`288.1 kW/L`).

## 3. Theoretical Manual — Analytic Verification A22 — 3D P2 Darcy–Forchheimer Porous Flow & Two-Stream TPMS Conjugate ε–NTU

Verifies Fieldsmith's 3D quadratic P2 nonlinear Darcy–Forchheimer finite-element solver, minimal-surface geometric scaling laws, and two-stream conjugate thermal resistance + Effectiveness–NTU (`ε–NTU`) heat-exchanger formulation against closed-form 1D analytical solutions.

### A22.1 Nonlinear Darcy–Forchheimer Law & Safeguarded Log-Mobility Fixed Point

```text
−∇p = (μ / K + β ρ_f |u_D|) u_D   ⟺   u_D = −κ_eff(|∇p|) ∇p,   κ_eff = 2 / ( μ/K + √((μ/K)² + 4 β ρ_f |∇p|) )
−∇·(κ_eff(e, |∇p|) ∇p) = 0  in Ω,   ∫_{Γ_in} κ_eff ∂_n p dA = Q_vol,   p|_{Γ_out} = p_out
```

Evaluating element RMS |∇p|_e with exact degree-2 Keast tetrahedral quadrature and inlet/outlet port integrals with exact P2 isoparametric facet weights ensures linear pressure fields p(x) ∈ P₂ satisfy the 3D patch test to machine precision on unstructured tetrahedral meshes. In log-mobility coordinates y_e = ln κ_eff,e, the fixed-point map has derivative F'(y) ∈ [0, 1/2], so Barzilai–Borwein / Aitken acceleration converges in 5–8 solves even at strong Forchheimer nonlinearity (Re_h > 350).

### A22.2 Minimal-Surface Partition Area, Interstitial Nusselt Law & Counterflow ε–NTU

```text
a_v = c_area / a,   D_h = 2 φ_f / a_v,   1 / U = 1 / h_hot + w_wall / k_s + 1 / h_cold,   UA = ∫_Ω a_v U dV
ε_counterflow(NTU, C_r = 1) = NTU / (1 + NTU),   Q = ε C_min (T_{hot,in} − T_{cold,in})
```

Because Schoen's Gyroid has zero mean curvature (H = 0) everywhere on its mid-surface, offsetting by ±w/2 preserves interfacial area to O((w/a)²), dividing Ω into two non-mixing interpenetrating fluid labyrinths separated by thin conductive AlSi10Mg walls (w_wall = 0.25 × 5 mm / 3.0915 = 0.404 mm).

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve 3D P2 Darcy-Forchheimer porous flow and counterflow HX

```json
{
  "tool": "solve",
  "arguments": {
    "dofs": 14000
  }
}
```

### Verify all 10 analytical hydraulic and thermal HX contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 14000
  }
}
```

---
*Generated from `examples/analytic/A22-darcy-forchheimer-tpms-porous-hx.fsm` by `node scripts/gen-sheet-docs.mjs`.*
