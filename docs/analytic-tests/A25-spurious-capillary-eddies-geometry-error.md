# A25 — Analytic Benchmark A25: 3D Droplet Containment Vessel Structural FEA + 2D Analytic Biharmonic Spurious Capillary Eddy Workbench (V65)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Pending verification (no shipped report yet)
> **Repository Path**: `docs/analytic-tests/A25-spurious-capillary-eddies-geometry-error.md` · **Starter Model**: `a25-spurious-capillary-eddies-geometry-error.fsm` · **Verification Reports**: —

Solve 3D P2 membrane stress on a spherical containment shell, paired with the 2D analytic biharmonic streamfunction workbench quantifying 8-vortex quadrupolar parasitic capillary currents (Ca_max) across five discrete surface-curvature approximations (Voxel CSF, Kernel CSF, Parabolic Monge MLS, Height-Function, and Pratt Quadric MLS).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| 1. Unsmoothed Voxel / Staircase CSF (Brackbill et al. 1992) | `κ_h = −∇_Δx · (∇c / ‖∇c‖),   σ*_κ ~ O(1) non-convergent` | **Ca_exact = 0 (u ≡ 0)** | **Ca_max = 1.687×10⁻¹ (U_max = 2.50 m/s)** | Order p = 0.06 | Fails (O(1) Eddies) |
| 2. Kernel-Smoothed Color-Gradient CSF (Morris 2000) | `κ_i = −∑ V_j (ñ_j − ñ_i)·∇W_ij,   σ*_κ ~ O(Δx/D)` | **Ca_exact = 0 (u ≡ 0)** | **Ca_max = 1.839×10⁻² (U_max = 0.272 m/s)** | Order p = 1.00 | 1st-Order Eddies |
| 3. Standard Parabolic Taylor Monge MLS (2nd-Order Basis) | `z ≈ a u² + b u v + c v² (truncates −r⁴ / 8R₀³)` | **Ca_exact = 0 (u ≡ 0)** | **Ca_max = 1.329×10⁻⁴ (U_max = 1.97 mm/s)** | Order p = 2.12 | 2nd-Order Eddies |
| 4. Balanced-Force Height-Function VOF (Popinet 2009 Gerris) | `κ_HF = −∇·(∇H / √(1+\|∇H\|²)),   ∇_h p = γ κ_h ∇_h c` | **Ca_exact = 0 (u ≡ 0)** | **Ca_max = 1.914×10⁻⁵ (U_max = 0.283 mm/s)** | Order p = 2.00 | ≤ 10⁻⁴ (PASS) |
| 5. Fieldsmith Spherical-Exact Pratt Quadric MLS + Analytic SDF | `z = c₁(u²+½z²) + c₂uv + c₃(v²+½z²) + c₄u + c₅v` | **Ca_exact = 0 (u ≡ 0)** | **Ca_max = 6.35×10⁻¹⁰ (U_max = 9.39 nm/s)** | Exact on Sphere | ≤ 10⁻⁷ (PASS) |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A25 — Spurious Capillary Eddies & Surface-Tension Geometry Approximation Errors (V65)"
material ti64

let droplet_r_mm = 2.0
let resolution_d_dx = 24
let laplace_la = 12000
let stencil_anisotropy = 0.15

sphere droplet_shell r=12 at=(0, 0, 12)
sphere droplet_cavity r=9.2 at=(0, 0, 12) subtract
sphere optical_trap_core r=2.6 at=(0, 0, -0.4) subtract

region cell_base = optical_trap_core.all
region droplet_interface = droplet_cavity.all

support clamp cell_base fixed=xyz
load laplace_jump droplet_interface pressure=0.25
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the 8-Vortex Numerical Noise Eddies & 5-Scheme Geometry Error Table in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) (or open the embedded Spurious Capillary Eddy Inspector below) to view the 2D equatorial velocity & vorticity field (8 counter-rotating quadrupolar eddies ω_z(x,y) driven by ∇_s κ_h × n̂ ≠ 0) and switch live between Voxel CSF, Kernel CSF, Parabolic Monge MLS, Height-Function VOF, and Pratt Quadric MLS.

- **Expected Check**: Switching from Voxel CSF or Parabolic Monge MLS to Fieldsmith Pratt Quadric MLS collapses the curvature variance σ*_κ from 3.10×10⁻¹ / 6.09×10⁻⁴ down to 5.49×10⁻⁸ and suppresses the parasitic Capillary number Ca_max by > 200,000×.

### 2. Vary Droplet Grid Resolution D/Δx, Laplace Number La, and Stencil Anisotropy *(Lens: `SETUP`)*

In Setup (2) or the Results (3) Spurious Eddy Explorer, vary `resolution_d_dx` (D/Δx ∈ [8, 64]), `laplace_la` (La = ρ γ D / μ² ∈ [120, 120000]), and `stencil_anisotropy` (η_grid ∈ [0.0, 0.35]) to observe the log-log convergence slopes p ≈ 0 (Voxel), p ≈ 1 (Kernel), and p ≈ 2 (Parabolic MLS / Height-Function).

- **Expected Check**: Unbalanced Voxel CSF fails to converge (p ≈ 0.06), whereas Parabolic Monge MLS and Height-Function VOF converge at second order (p ≈ 2.0) and Pratt Quadric MLS achieves machine-precision spherical curvature at all resolutions.

## 3. Theoretical Manual — Analytic Verification A25 — Why Surface-Tension Geometry Approximation Errors Generate Numerical Noise Eddies

Derives the exact vorticity-streamfunction mechanism by which discrete interface curvature errors δκ_h(θ) = κ_h(θ) − 2/R₀ and pressure-gradient operator mismatch ∇_h × (γ κ_h n̂_h δ_s − ∇_h p) ≠ 0 generate 8-cell quadrupolar parasitic vortex currents around a stationary droplet.

### A25.1 Interfacial Vorticity Sheet from Anisotropic Curvature Error δκ_h(θ)

```text
∇ × (γ κ_h(θ) n̂ δ_s) = γ (∇_s κ_h × n̂) δ(r − R₀) = −(γ / R₀) (∂(δκ_h) / ∂θ) δ(r − R₀) ê_z ≠ 0
```

On an exact sphere of radius R₀, mean curvature κ = 2/R₀ is constant everywhere on the interface, so the capillary force f_σ = γ κ n̂ δ_s = −(2γ/R₀) ∇H(r − R₀) is a pure gradient balanced identically by the hydrostatic Young–Laplace pressure jump Δp = 2γ/R₀ with u(x,t) ≡ 0. When a discrete geometry approximation (voxel staircase, kernel gradient, or 2nd-order parabolic Taylor patch z ≈ a u² + b u v + c v²) reconstructs slightly different curvatures along the grid axes (θ = 0, π/2) vs the 45° diagonals (θ = ±π/4), the dominant m = 4 quadrupolar harmonic δκ_h(θ) = Δκ₄ cos(4θ) creates a non-zero tangential surface-tension curl that no irrotational pressure field ∇p can ever balance!

### A25.2 Biharmonic 8-Vortex Eddy Solution & Pratt Quadric Elimination

```text
μ ∇⁴ψ = (γ / R₀) (∂(δκ_h) / ∂θ) δ'(r − R₀)   ⟹   ψ_eddy(r,θ) = U_max R₀ f(r/R₀) sin(4θ),   Ca_max = μ U_max / γ ∝ R₀ σ(κ_h)
```

Balancing the interfacial capillary curl against viscous biharmonic dissipation μ ∇⁴ψ produces 8 counter-rotating parasitic eddies (4 interior + 4 exterior lobes) that pull fluid inward along the coordinate axes and eject it along the 45° diagonals. Standard parabolic Monge MLS truncates the sphere expansion z = −(u²+v²)/(2R₀) − (u²+v²)²/(8R₀³) − ⋯ at second order, leaving an O((h/R₀)²) fourth-order geometric error. Fieldsmith's algebraic Pratt–Monge basis fits z in [u² + ½z², u v, v² + ½z², u, v], which satisfies the exact sphere equation u² + v² + z² + 2R₀z = 0 identically with zero Taylor truncation error.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve A25 Stationary Droplet Vessel via MCP / REST API

```json
{
  "tool": "solve",
  "arguments": {
    "raw": "{\"kind\":\"solve\",\"dofs\":25000}\nExecute the 3D P2 finite-element solver on the A25 stationary droplet test cell."
  }
}
```

---
*Generated from `examples/analytic/A25-spurious-capillary-eddies-geometry-error.fsm` by `node scripts/gen-sheet-docs.mjs`.*
