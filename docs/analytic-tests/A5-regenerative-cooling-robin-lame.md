# A5 — Analytic Benchmark A5: Cylindrical Regenerative Cooling — Hot-Gas Heat Flux + Coolant Convection Thermo-Elastic Lamé (V60c)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V60(c) · Exact Robin–Neumann Logarithmic Thermo-Elasticity · PASS (< 2.0%)
> **Repository Path**: `docs/analytic-tests/A5-regenerative-cooling-robin-lame.md` · **Starter Model**: `a5-regenerative-cooling-robin-lame.fsm` · **Verification Reports**: `docs/verification/V20.md`, `docs/verification/V60.md`

Verify coupled steady conduction and thermo-elastic stress on a CuCrZr chamber wall with inward hot-gas Neumann heatflux (q = 5.0) on the bore and outward Robin convection (h = 0.25, T_inf = 20 °C) on the coolant jacket.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Robin–Neumann Logarithmic Temperature T(r) at r = 11 / 15 / 19 mm | `T(r) = T_∞ + r_i q_in [1/(r_o h_c) + ln(r_o / r)] = 30.0 + 50.0 ln(20 / r)` | **59.892 / 44.384 / 32.565 °C** | **59.694 / 44.224 / 32.464 °C** | 0.33% / 0.36% / 0.31% | ≤ 2.0% |
| Thermo-Elastic Radial Expansion u_r(r) at r = 11 / 15 / 19 mm | `u_r(r) = C₁ r + (C₂ + γ α ∫ s ΔT(s) ds) / r` | **6.493 / 9.410 / 10.580 µm** | **6.402 / 9.290 / 10.430 µm** | 1.41% / 1.28% / 1.42% | ≤ 2.5% |
| Thermo-Elastic Axial Stress σ_zz(r) & In-Plane Sum σ_rr + σ_θθ | `σ_zz(r) = 2λ C₁ − 2μ γ α ΔT(r)  (at r = 11 / 15 / 19 mm)` | **-105.24 / -54.11 / -15.14 MPa** | **-103.73 / -52.21 / -12.71 MPa** | 1.24% / 1.57% / 2.01% (of bore scale) | ≤ 3.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A5 — Regenerative Cooling Robin-Neumann Thermo-Elastic Lame (V60c)"
custommaterial cucrzr_regen name="CuCrZr (E=128 GPa, nu=0.34, a=1.7e-5)" e=128000 nu=0.34 a=1.7e-5 yield=380 density=8.9
material cucrzr_regen

cylinder outer r=20 h=60 at=(0, 0, 30)
cylinder bore r=10 h=60 at=(0, 0, 30) subtract
box cutx size=(90, 45, 160) at=(-45, 0, 0) subtract
box cuty size=(45, 90, 160) at=(0, -45, 0) subtract

region skin_in = bore.side
region skin_out = outer.side
region xplane = cutx.px
region yplane = cuty.py
region zbot = outer.bottom
region ztop = outer.top

support kx xplane fixed=x
support ky yplane fixed=y
support kz_bot zbot fixed=z
support kz_top ztop fixed=z
heatflux hot_gas_flux skin_in q=5.0 case="RegenHotFire"
convection coolant_conv skin_out h=0.25 tinf=20.0 case="RegenHotFire"
solve tref=20.0
```

## 2. Step-by-Step Guided Walkthrough

### 1. Coupled Neumann Hot-Gas Heat Flux + Robin Coolant Convection Boundary Conditions *(Lens: `SETUP`)*

Open Setup (2). In a regeneratively cooled rocket chamber, the wall temperatures are not known a priori: instead, hot combustion gas injects a surface heat flux q_in = 5.0 into the inner bore (r_i = 10 mm) while liquid coolant flowing through the outer jacket (r_o = 20 mm) extracts heat by forced convection q_out = h_c (T(r_o) − T_∞) with h_c = 0.25 and T_∞ = 20.0 °C.

- **Expected Check**: Setup shows Heat Flux hot_gas_flux (q = 5.0) on skin_in and Convection coolant_conv (h = 0.25, tinf = 20 °C) on skin_out.

### 2. Verify the Exact Logarithmic Temperature Profile T(r) = 30 + 50 ln(20/r) in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) and select "Temperature". Radial conservation of energy 2π r_i q_in = 2π r_o h_c (T(r_o) − T_∞) pins the outer jacket wall at exactly T(r_o) = 20 + (10 × 5.0)/(20 × 0.25) = 30.0 °C, rising logarithmically through the wall to T(r_i) = 30 + 50 ln(2) = 64.657 °C at the hot-gas bore. Then switch to "Stress" to inspect the compressive bore stress (σ_zz = −121 MPa, σ_θθ = −70 MPa at r_i) balanced by outer jacket hoop tension (+49.6 MPa at r_o).

- **Expected Check**: Temperature ranges from 30.0 °C on the outer wall to 64.7 °C on the inner bore, matching the exact Robin–Neumann formula to 0.35%.

## 3. Theoretical Manual — Analytic Verification A5 — Cylindrical Robin–Neumann Conduction & Duhamel–Neumann Thermo-Elasticity (V60c)

Derives the exact 1D cylindrical temperature distribution under mixed Neumann–Robin boundary conditions and integrates the resulting logarithmic thermal strain field in closed form.

### A5.1 Mixed Neumann–Robin Cylindrical Conduction & Exact Thermal Moment Integral f(r)

```text
−(1/r) d/dr(r dT/dr) = 0,   −dT/dr|_{r_i} = q_in,   −dT/dr|_{r_o} = h_c(T(r_o) − T_∞)
T(r) = T_∞ + (r_i q_in)/(r_o h_c) + r_i q_in ln(r_o / r)
f(r) = ∫_{r_i}^r s ΔT(s) ds = ½ ΔT_in (r² − r_i²) − r_i q_in [½ r² ln(r/r_i) − ¼(r² − r_i²)]
```

Substituting f(r) into the Duhamel–Neumann radial equilibrium equation yields the exact radial displacement u_r(r) = C₁ r + (C₂ + γα f(r))/r and thermal stress components σ_rr(r), σ_θθ(r), σ_zz(r) with zero free-parameter fitting.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Regenerative Cooling Thermo-Elastic Benchmark via API (`solve`)

Run the coupled P2 conduction + thermo-elastic solver on A5.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 35000
  }
}
```
