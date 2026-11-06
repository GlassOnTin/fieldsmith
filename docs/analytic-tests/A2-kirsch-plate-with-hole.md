# A2 — Analytic Benchmark A2: Kirsch Plate with a Central Circular Hole Stress Concentration (V3)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V3 · Kirsch (1898) Exact Elasticity · PASS (< 2.5%)
> **Repository Path**: `docs/analytic-tests/A2-kirsch-plate-with-hole.md` · **Starter Model**: `a2-kirsch-plate-hole.fsm` · **Verification Reports**: `docs/verification/V3.md`

Verify peak hoop stress concentration around a Ø 10 mm hole in a 100 × 100 × 4 mm tension plate against Ernst Kirsch’s 1898 exact elasticity solution (K_t = 3.0 infinite-plate / 3.03 finite-width).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Far-Field Nominal Tensile Stress σ₀ | `σ₀ = F_x / (W_oct · t) = 2000 N / (50 mm × 4 mm)` | **10.00 MPa** | **10.00 MPa** | 0.00% | Exact Equilibrium |
| Kirsch Radial Decay σ_xx(r, θ=90°) Along Transverse ligament | `σ_xx(0, y) = σ₀ [1 + ½(a/y)² + 3/2(a/y)⁴]  (at y = 6.0 mm, a = 5.0 mm)` | **20.70 MPa** | **20.92 MPa** | 1.06% | ≤ 3.0% |
| Peak Hole-Edge Stress Concentration K_t = σ_max / σ₀ | `K_t,∞ = 3.000;  Heywood/Peterson finite-width (2a/W = 0.10): K_t = 3.030` | **30.30 MPa (K_t = 3.03)** | **29.85 MPa (Element Centroid)** | 1.49% | ≤ 3.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A2 — Kirsch Plate with Circular Hole (V3)"
custommaterial steel200 name="Steel (E=200 GPa, nu=0.30)" e=200000 nu=0.30 yield=250 density=7.85
material steel200

box plate size=(50, 50, 4) at=(25, 25, 2)
cylinder hole r=5 h=12 at=(0, 0, 2) subtract
region sym_x = plate.nx
region sym_y = plate.ny
region sym_z = plate.nz
region far_x = plate.px
support kx sym_x fixed=x
support ky sym_y fixed=y
support kz sym_z fixed=z
load pull far_x force=(2000, 0, 0)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Quarter-Symmetry Kirsch Plate Model (a = 5 mm, W/2 = 50 mm) *(Lens: `SETUP`)*

Open Setup (2). By symmetry, one quadrant (x ∈ [0, 50] mm, y ∈ [0, 50] mm, z ∈ [0, 4] mm) of a 100 × 100 mm plate with a central radius a = 5 mm hole models the full plate exactly when normal displacements are fixed on the symmetry planes x = 0 (fixed=x), y = 0 (fixed=y), and z = 0 (fixed=z). The far right face x = 50 mm carries F_x = 2,000 N over A = 50 × 4 = 200 mm², producing a nominal far-field tension σ₀ = 10.0 MPa.

- **Expected Check**: Quarter-symmetry supports on x = 0, y = 0, z = 0 eliminate all 6 rigid-body modes without inducing artificial Poisson restraint.

### 2. Inspect the 3× Stress Concentration Peak at (x = 0, y = a = 5 mm) *(Lens: `RESULTS`)*

Switch to Results (3). Ernst Kirsch’s 1898 biharmonic Airy stress solution predicts that along the transverse ligament (x = 0, θ = 90°), hoop stress rises as σ_xx(0, y) = σ₀ [1 + ½(a/y)² + (3/2)(a/y)⁴], reaching 3 σ₀ = 30.0 MPa at the hole equator y = a (30.3 MPa with finite-width correction for 2a/W = 0.10) while the longitudinal pole (x = a, y = 0) goes into compression (−σ₀ = −10 MPa).

- **Expected Check**: Stress hot-spot localises sharply at the top of the quarter-hole (x = 0, y = 5 mm) at ~30 MPa, decaying rapidly to 10 MPa within two hole radii.

## 3. Theoretical Manual — Analytic Verification A2 — Kirsch’s Biharmonic Airy Stress Solution Around a Circular Hole (V3)

Derives Kirsch’s exact polar stress tensor (σ_rr, σ_θθ, τ_rθ) for a circular cavity in a tension sheet and explains why stress concentration depends only on r/a.

### A2.1 Kirsch Polar Stress Components (∇⁴Φ = 0)

```text
σ_rr(r, θ) = (σ₀/2)(1 − a²/r²) + (σ₀/2)(1 − 4a²/r² + 3a⁴/r⁴) cos 2θ
σ_θθ(r, θ) = (σ₀/2)(1 + a²/r²) − (σ₀/2)(1 + 3a⁴/r⁴) cos 2θ
τ_rθ(r, θ) = −(σ₀/2)(1 + 2a²/r² − 3a⁴/r⁴) sin 2θ
```

At θ = π/2 (cos 2θ = −1) and r = a, radial stress σ_rr(a, π/2) = 0 (traction-free hole wall) while hoop stress reaches σ_θθ(a, π/2) = 3 σ₀. Because the fourth-order term 3(a/r)⁴ decays rapidly with distance, coarse elements far from the hole and fine P2 elements near r = a capture the gradient accurately.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Kirsch Quarter-Plate via API (`solve`)

Run the P2 elasticity solver on the Kirsch quarter-plate and verify peak stress ~30 MPa.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 40000
  }
}
```
