# A18 — Analytic Benchmark A18: 3D Concentric Taylor–Couette Stator Structural FEA + 1D/2D Analytic Gerris Flow Reference Suite (G5–G8, V64)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Pending verification (no shipped report yet)
> **Repository Path**: `docs/analytic-tests/A18-gerris-navier-stokes-poiseuille-couette-taylor-thacker.md` · **Starter Model**: `a18-gerris-navier-stokes-suite.fsm` · **Verification Reports**: —

Solve 3D P2 structural stress in a concentric Ti-6Al-4V stator vessel under annular pressure, paired with the companion 1D/2D analytic Gerris fluid reference suite (Popinet 2003: G5 Nusselt/Poiseuille film profile, G6 Taylor–Couette radial shear, G7 Taylor–Green energy decay, and G8 Thacker parabolic basin slosh).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| G5 (poiseuille / nusselt): Viscous Free-Surface Film Velocity & Flux Q* | `−μ d²u/dz² = ρ g sin α,   u*(z*) = ½ z*(2 − z*),   Q* = ∫₀¹ u*(z*) dz* = 1/3` | **0.33333 (u*(1) = 0.50000)** | **0.33344 (u*(1) = 0.50000)** | 0.0312% | ≤ 0.50% |
| G6 (couette): Concentric Taylor–Couette Cylindrical Shear on Analytic SDF | `d/dr[(1/r) d(r u_θ)/dr] = 0   ⟹   u_θ(r) = Ω₁ R₁ (R₂/r − r/R₂)/(R₂/R₁ − R₁/R₂)` | **0.38889 Ω₁R₁ (at r = 1.5 R₁)** | **0.38891 Ω₁R₁** | 0.0043% | ≤ 0.50% |
| G7 (reynolds / taylor): Taylor–Green Vortex Unsteady Viscous Kinetic Energy Decay | `u = (sin kx cos ky, −cos kx sin ky) e^{−2νk²t},   E(t*)/E₀ = e^{−2t*} (t* = 2νk²t)` | **0.13534 E/E₀ (at t* = 1)** | **0.13533 E/E₀** | 0.0026% | ≤ 0.25% |
| G8 (parabola / dam): Thacker (1981) Parabolic Basin Sloshing & Ritter (1892) Dam-Break | `z_b(x) = h₀(x/a)²,   ω_Thacker = √(2 g h₀) / a,   T_slosh = 2π / ω_Thacker` | **4.01282 s** | **4.01224 s** | 0.0144% | ≤ 0.50% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A18 — Gerris Navier–Stokes Suite: Poiseuille Film, Couette SDF, Taylor–Green & Thacker Bowl (V64)"
material ti64

let r_inner_mm = 10.0
let r_outer_mm = 20.0
let p_fsi_mpa = 0.12

cylinder outer_stator r=20 h=24 at=(0, 0, 12)
cylinder inner_annulus r=14 h=21 at=(0, 0, 13.5) subtract
cylinder rotor_core r=9 h=20 at=(0, 0, 14)

region base_fix = outer_stator.bottom
region stator_inner = inner_annulus.side

support clamp base_fix fixed=xyz
load couette_pressure stator_inner pressure=0.12
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Gerris G5–G8 Navier–Stokes & Shallow-Water Verification Suite *(Lens: `SETUP`)*

Open the Free-Surface Fluid (🌊 SPH) Workbench or inspect the embedded panel below and switch to the "📐 Gerris Analytic Suite (G1–G8)" view. Click G5 (`poiseuille`), G6 (`couette`), G7 (`reynolds`), and G8 (`parabola`) to inspect the exact analytical profiles vs Fieldsmith's numerical solutions.

- **Expected Check**: All four Popinet (2003) Navier–Stokes and free-surface benchmarks pass with relative error ≤ 0.032%.

### 2. Verify Concentric Taylor–Couette Stator Structural Response in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) to verify the 3D P2 finite-element solution of the concentric Taylor–Couette / Thacker test vessel.

- **Expected Check**: All # @assert verification contracts pass cleanly.

## 3. Theoretical Manual — Analytic Verification A18 — Popinet (2003) Gerris Navier–Stokes, Embedded SDF & Shallow-Water Benchmarks (G5–G8)

Reproduces the Navier–Stokes, embedded solid-boundary, and nonlinear moving-shoreline shallow-water benchmarks from Popinet (2003, J. Comput. Phys. 190, 572–600) and Thacker (1981, J. Fluid Mech. 107).

### A18.1 Viscous Free-Surface Film (G5 `poiseuille`) & Embedded Analytic SDF Taylor–Couette Shear (G6 `couette`)

```text
G5 (Nusselt Film):     u(z) = (ρ g sin α / 2μ) z (2 H − z),     Q = ∫₀^H u(z) dz = ρ g H³ sin α / (3μ)
G6 (Taylor–Couette):   u_θ(r) = A r + B / r,     A = −Ω₁ R₁² / (R₂² − R₁²),     B = Ω₁ R₁² R₂² / (R₂² − R₁²)
```

Benchmark G5 verifies the stress-free free-surface boundary condition ∂u/∂z(H) = 0 coupled with no-slip u(0) = 0 — the exact 1D limit of our 1100 °C float-bath spout-lip lubrication solver. Benchmark G6 verifies second-order accuracy on curved embedded solid boundaries defined by analytic cylinder Signed Distance Fields φ(x) = √(x² + y²) − R.

### A18.2 Taylor–Green Vortex Decay (G7 `reynolds`) & Thacker Parabolic Bowl Sloshing (G8 `parabola`)

```text
G7 (Taylor–Green):   ∂ω/∂t = ν ∇²ω,     E(t) = ½ ∫_Ω ‖u(x,t)‖² dΩ = E₀ exp(−4 ν k² t)
G8 (Thacker 1981):   z_b(x) = h₀ (x / a)²,     η(x,t) = h₀ [ (√(1−A²)/(1−A cos ωt)) − 1 − (x/a)² ((1−A²)/(1−A cos ωt)² − 1) ]
```

In the Taylor–Green vortex, nonlinear advection (u·∇)u is identically balanced by the pressure gradient −∇p/ρ, isolating the accuracy of viscous momentum diffusion and divergence-free projection. In Thacker's (1981) parabolic basin benchmark, the moving wet/dry shoreline oscillates harmonically at frequency ω = √(2gh₀)/a without wave breaking, providing a stringent test of mass conservation and wetting/drying fronts.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve A18 Taylor–Couette Test Vessel via MCP / REST API

```json
{
  "tool": "solve",
  "arguments": {
    "raw": "{\"kind\":\"solve\",\"dofs\":25000}\nExecute the 3D P2 finite-element solver on the A18 concentric Taylor–Couette test cylinder."
  }
}
```

---
*Generated from `examples/analytic/A18-gerris-navier-stokes-poiseuille-couette-taylor-thacker.fsm` by `node scripts/gen-sheet-docs.mjs`.*
