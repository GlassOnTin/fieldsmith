# A20 — Analytic Benchmark A20: 3D P2 BDF2 Transient Fourier–Biot Quench & Pure Thermal-Shock Spike

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by A20 · Exact 1D Fourier–Biot Eigenfunction Series (Bi = 5, Fo = 0.10) · PASS (< 1.5%)
> **Repository Path**: `docs/analytic-tests/A20-transient-biot-quench-thermal-shock.md` · **Starter Model**: `a20-transient-biot-quench-thermal-shock.fsm` · **Verification Reports**: `docs/verification/V20.md`, `docs/verification/V63.md`

Verify 3D quadratic P2 backward-differentiation-formula (BDF2) transient heat conduction and Duhamel–Neumann thermo-elasticity on a water-spray-quenched 316L slab (Bi = 5.0, Fo = 0.10) against the exact 40-term 1D Fourier–Biot eigenfunction series and zero-stress steady limit.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| P2 Consistent Thermal Capacity Volume Integral 1ᵀ M 1 | `∫_Ω ρ c_p dV = (8.0×10⁻⁶ kg/mm³)(500 J/kg·K)(360 mm³)` | **1.4400 J/K (α_th = k / ρc_p = 4.000 mm²/s)** | **1.4400 J/K (Exact Degree-4 Simplicial Moment)** | 0.00% | ≤ 0.01% |
| Fourier–Biot Insulated Core Temperature T(z = 0, t = 2.5 s) | `T_∞ + ΔT₀ Σ [4 sin λ_n / (2λ_n + sin 2λ_n)] e^{−λ_n² Fo} (Bi = 5.0, Fo = 0.10)` | **313.19 °C** | **313.2 °C (3D P2 BDF2, Δt = 0.10 s)** | < 0.5% | ≤ 1.5% |
| Fourier–Biot Quenched Skin Temperature T(z = L, t = 2.5 s) | `T_∞ + ΔT₀ Σ [4 sin λ_n / (2λ_n + sin 2λ_n)] e^{−λ_n² Fo} cos λ_n` | **112.64 °C** | **112.6 °C (3D P2 BDF2, Δt = 0.10 s)** | < 1.0% | ≤ 2.5% |
| Pure Transient Thermal-Shock Spike vs. Unrestrained Steady Limit | `σ_vM(t → ∞) = 0 MPa (uniform T ≡ 20 °C),  max_{t>0} σ_vM(t) > 150 MPa` | **σ_vM(∞) = 0.00 MPa,  σ_vM(t*) > 150 MPa** | **Steady < 0.05 MPa,  Peak Shock > 150 MPa** | PASS | Exact |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A20 — Transient Fourier–Biot Quench & Thermal-Shock Benchmark"
custommaterial ss316_quench name="316L Quench Benchmark" e=180000 nu=0.30 yield=470 density=8.00 wall=0.4 a=0.000016 k=16.0 cp=500
material ss316_quench

box slab size=(6, 6, 10) at=(3, 3, 5)

region x0 = slab.nx
region y0 = slab.ny
region z0 = slab.nz
region quench = slab.pz

support sx x0 fixed=x
support sy y0 fixed=y
support sz z0 fixed=z

convection spray quench h=0.008 tinf=20 case="SprayQuench"
solve tref=320 t_end=2.5 steps=25 t_init=320

assert "Fourier-Biot insulated core temperature T(0, t_end)" metric="final_max_temp" target=313.1927 tol="1.5%" unit="°C"
assert "Fourier-Biot quenched skin temperature T(L, t_end)" metric="final_min_temp" target=112.6371 tol="2.5%" unit="°C"
assert "Pure transient skin thermal-shock spike sigma_vM(t*)" metric="peak_transient_vm" target=">= 150.0" tol=">= 150.0" unit="MPa"
assert "Unrestrained uniform-temperature steady limit vanishes" metric="steady_limit_vm_elem" target="<= 0.05" tol="<= 0.05" unit="MPa"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Symmetry-Octant Quench Slab & Physical Diffusivity in Shape (1) *(Lens: `SHAPE`)*

Open Shape (1) and Setup (2). The symmetry-octant slab [0, 6] × [0, 6] × [0, 10] mm has roller symmetry planes on x = 0, y = 0, z = 0 (`sx`, `sy`, `sz`), adiabatic lateral/midplane faces, and a water-spray Robin quench (`h = 0.008 W/(mm²·K)`, `T_∞ = 20 °C`, `Bi = hL/k = 5.0`) on z = 10 mm from initial temperature `T₀ = 320 °C`.

- **Expected Check**: With `k = 16.0 W/(m·K)`, `ρ = 8.00 g/cm³`, and `c_p = 500 J/(kg·K)`, thermal diffusivity is `α_th = 4.000 mm²/s` and `t_end = 2.5 s` corresponds to exact Fourier number `Fo = α_th t_end / L² = 0.10`.

### 2. Scrub the BDF2 Thermal-Shock Timeline in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). Inspect the **3D P2 BDF2 Transient Conduction & Thermal Shock** timeline card: observe that steady-state analysis (`t → ∞`, uniform 20 °C) predicts `0.00 MPa` stress on the unrestrained slab, whereas transient quenching drives a `> 150 MPa` biaxial tensile shock spike on the cooled `z = 10 mm` skin! Drag the time-step slider from `t = 0` to `2.5 s` in both **Stress** and **Temperature** modes.

- **Expected Check**: All 4 live `@assert` contracts pass (`final_max_temp ≈ 313.2 °C`, `final_min_temp ≈ 112.6 °C`, `peak_transient_vm ≥ 150 MPa`, `steady_limit_vm_elem ≤ 0.05 MPa`).

## 3. Theoretical Manual — Analytic Verification A20 — 3D P2 BDF2 Transient Conduction & Kingery Thermal-Shock Mechanics

Verifies Fieldsmith's 3D quadratic P2 consistent thermal capacity matrix M, unconditionally A-stable 2-step backward-differentiation-formula (BDF2) time integrator, and batched multi-RHS Duhamel–Neumann thermo-elastic solver against Carslaw & Jaeger's exact 1D Fourier–Biot series and unrestrained thermal-shock theory.

### A20.1 Semidiscrete P2 Conduction & Constant-Operator BDF2 Time Stepping

```text
M_ij = ∫_Ω ρ c_p φ_i φ_j dV,   A_ij = ∫_Ω k ∇φ_i · ∇φ_j dV + ∫_{Γ_c} h φ_i φ_j dA
BDF1 (m = 1):  (M / Δt + A) T¹ = (M / Δt) T⁰ + q
BDF2 (m ≥ 2):  (3M / (2Δt) + A) Tᵐ = M (4 Tᵐ⁻¹ − Tᵐ⁻²) / (2Δt) + q
```

On 10-node quadratic tetrahedra, φ_i φ_j is degree 4 in barycentric coordinates and is integrated to machine precision via closed-form simplicial factorial moments β! / (5 + |β|)!. Holding Δt constant allows every BDF2 step m ≥ 2 to reuse a single P1-coarse Cholesky two-level preconditioner on (3/(2Δt))M + A.

### A20.2 Fourier–Biot Eigenfunction Series & Transient Thermal-Shock Paradox

```text
λ_n tan λ_n = Bi = h L / k,   Fo = α_th t / L²
T(z, t) = T_∞ + (T₀ − T_∞) Σ_{n=1}^∞ [4 sin λ_n / (2λ_n + sin 2λ_n)] exp(−λ_n² Fo) cos(λ_n z / L)
σ_xx(L, 0⁺) → E α (T₀ − T_skin) / (1 − ν),   σ_ij(x, t → ∞) ≡ 0
```

An unrestrained slab cooled from T₀ = T_ref = 320 °C to T_∞ = 20 °C contracts freely once isothermal, so its steady-state stress at t → ∞ vanishes identically (σ_ij ≡ 0). Steady-state FEA misses the hazard completely: at short Fourier times (Fo ≈ 0.02–0.06), the chilled skin at z = L is prevented from contracting by the hot interior core, inducing severe biaxial skin tension captured directly by batched multi-RHS thermo-elasticity.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve 25-step 3D P2 BDF2 transient quench timeline and thermal-shock peak

```json
{
  "tool": "transient_solve",
  "arguments": {
    "dofs": 12000
  }
}
```

### Verify Fourier-Biot temperatures and thermal-shock contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 12000
  }
}
```

---
*Generated from `examples/analytic/A20-transient-biot-quench-thermal-shock.fsm` by `node scripts/gen-sheet-docs.mjs`.*
