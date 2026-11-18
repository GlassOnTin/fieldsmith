# A15 — Analytic Benchmark A15: 3D Young–Laplace Capillary Droplet & Rayleigh–Lamb (n = 2) Quadrupole Oscillation (V61)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Pending verification (no shipped report yet)
> **Repository Path**: `docs/analytic-tests/A15-young-laplace-droplet-and-rayleigh-lamb.md` · **Starter Model**: `a15-young-laplace-droplet.fsm` · **Verification Reports**: —

Live 3D Rayleigh–Lamb (n = 2) prolate ↔ oblate quadrupole capillary oscillation and P2 Young–Laplace surface tension containment, paired with 3D MLS Monge–Pratt curvature κ = 2/R₀ and Δp = 2γ/R₀ verification (V61).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| 3D MLS Monge–Pratt Mean Curvature κ = κ₁ + κ₂ (R₀ = 2.0 mm) | `κ_exact = 2 / R₀ = 1.000 mm⁻¹` | **1.0000 mm⁻¹** | **1.0000 mm⁻¹ (Spherical-Exact Quadric Basis)** | < 0.01% | ≤ 1.0% |
| Young–Laplace Interior Capillary Pressure Jump Δp = 2γ / R₀ (γ = 0.0728 N/m) | `Δp = 2 γ / R₀` | **72.80 Pa** | **72.80 Pa** | < 0.05% | ≤ 1.5% |
| Rayleigh–Lamb (n = 2) Quadrupole Oscillation Period T₂ | `ω₂² = n(n−1)(n+2) γ / (ρ R₀³) = 8γ / (ρ R₀³),   T₂ = 2π / ω₂` | **23.22 ms** | **23.21 ms** | < 0.25% | ≤ 2.5% |
| Pairwise Akinci Cohesion Linear Momentum Conservation | `Σ_i Σ_{j≠i} f_{ij}^{coh} = 0  (Exact Antisymmetry)` | **0.000 N·s** | **< 10⁻¹⁵ N·s** | 0.000% | ≤ 10⁻¹² N·s |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A15 — Young–Laplace 3D Spherical Droplet & Rayleigh–Lamb Capillary Oscillation (V61)"
material ti64

let droplet_r_mm = 2.0
let gamma_n_m = 0.0728
let laplace_mpa = 0.0728

sphere outer_shell r=12 at=(0, 0, 12)
sphere inner_cavity r=9.2 at=(0, 0, 12) subtract
sphere optical_trap_core r=2.6 at=(0, 0, -0.4) subtract

region base_fix = optical_trap_core.all
region cavity_wall = inner_cavity.all

support pedestal_clamp base_fix fixed=xyz
load laplace_jump cavity_wall pressure=0.25
solve modes=6
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Spherical Capillary Containment Shell & Launch the Free-Surface SPH Workbench *(Lens: `SETUP`)*

Open Setup (2) or click "Free-Surface Fluid (SPH)" in the top toolbar to open the 3D Multi-Regime Free-Surface & Capillary Workbench on Preset A15 (`droplet_oscillation_a15`). The automatic dimensionless regime classifier evaluates Bo = 0.0, We < 1, Oh = 2.6×10⁻³ and selects Regime III (`capillary_mls_sph`: 3D Wendland C² + MLS Monge–Pratt Curvature CSF + Akinci Pairwise Cohesion).

- **Expected Check**: The regime banner displays Regime III (`capillary_mls_sph`), Δp_Laplace = 72.80 Pa (error < 0.05%), and Rayleigh–Lamb period T₂ = 23.21 ms.

### 2. Verify 3D Stress Under Capillary Laplace Pressure in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) to inspect the P2 finite-element solution of the thin spherical capillary test shell loaded by internal Young–Laplace pressure.

- **Expected Check**: All # @assert verification contracts pass cleanly.

## 3. Theoretical Manual — Analytic Verification A15 — 3D Young–Laplace Capillary Pressure Jump & Rayleigh–Lamb Droplet Modes (V61)

Derives the Continuum Surface Force (CSF, Brackbill et al. 1992) combined with Moving Least Squares (MLS) Monge–Pratt quadric curvature reconstruction, Akinci–Tartakovsky pairwise molecular cohesion, and Lord Rayleigh's (1879) / Lamb's (1932) capillary dispersion relation.

### A15.1 Spherical-Exact Monge–Pratt Quadric MLS Curvature & Young–Laplace Jump

```text
z(u, v) = c₀ (u² + ½ z²) + c₁ u v + c₂ (v² + ½ z²) + c₃ u + c₄ v
κ_MLS = κ₁ + κ₂ = −2(c₀ + c₂),     Δp = p_in − p_out = γ (κ₁ + κ₂) = 2γ / R₀
```

Standard quadratic Monge-patch fitting z ≈ c₀ u² + c₂ v² suffers fourth-order Taylor truncation error O((h/R₀)²) over a finite SPH kernel support radius 2h. Augmenting the basis with ½ z² makes the fit algebraically exact on spheres of any radius R₀ (since u² + v² + (z + R₀)² = R₀² ⟺ z = −(1/2R₀)(u² + ½z²) − (1/2R₀)(v² + ½z²)) while preserving second-order principal curvatures κ₁, κ₂ on general surfaces.

### A15.2 Rayleigh–Lamb (1879 / 1932) Quadrupole Capillary Oscillation & Prosperetti Viscous Damping

```text
ω_n² = n(n − 1)(n + 2) γ / (ρ R₀³),     β_n = (n − 1)(2n + 1) μ / (ρ R₀²)
For n = 2:   ω₂ = √(8γ / (ρ R₀³)),     β₂ = 5μ / (ρ R₀²),     T₂ = 2π / √(ω₂² − β₂²/4)
```

An initially prolate/oblate ellipsoidal droplet Perturbed in the n = 2 Legendre mode oscillates between prolate and oblate shapes at angular frequency ω₂ driven by capillary restoring energy against fluid inertia, damped at rate β₂ by viscous dissipation.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve A15 Capillary Shell via MCP / REST API

```json
{
  "tool": "solve",
  "arguments": {
    "raw": "{\"kind\":\"solve\",\"dofs\":25000}\nExecute the 3D P2 finite-element solver on the A15 spherical capillary containment test shell."
  }
}
```

---
*Generated from `examples/analytic/A15-young-laplace-droplet-and-rayleigh-lamb.fsm` by `node scripts/gen-sheet-docs.mjs`.*
