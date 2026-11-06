# A1 — Analytic Benchmark A1: Euler–Bernoulli & Timoshenko Cantilever Tip Deflection & Root Stress (V1 / V2)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V1 & V2 · Closed-Form Beam Theory · PASS (< 1.5%)
> **Repository Path**: `docs/analytic-tests/A1-cantilever-beam-theory.md` · **Starter Model**: `a1-euler-bernoulli-cantilever.fsm` · **Verification Reports**: `docs/verification/V1.md`, `docs/verification/V2.md`

Verify Fieldsmith’s 10-node quadratic (P2) tetrahedral elasticity solver against closed-form Euler–Bernoulli and Timoshenko beam theory on a 100 × 10 × 10 mm steel cantilever under a 100 N tip shear load.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Tip Vertical Deflection u_z(L) | `δ_EB = F L³ / (3 E I) = 0.2000 mm;  δ_Timo = δ_EB + F L / (κ A G) = 0.2016 mm` | **0.2016 mm** | **0.1994 mm** | 1.09% | ≤ 2.0% |
| Mid-Span Fiber Bending Stress σ_xx(x=50 mm, z=10 mm) | `σ_xx(x, z) = M(x) · (z - z₀) / I = F (L - x) c / I` | **30.00 MPa** | **29.96 MPa** | 0.13% | ≤ 1.5% |
| External Work Compliance C = fᵀ u | `C_Timo = F · δ_Timo = 20.156 N·mm (Galerkin lower bound C_h ≤ C_exact)` | **20.156 N·mm** | **19.940 N·mm** | 1.07% | ≤ 2.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A1 — Euler-Bernoulli Cantilever Benchmark (V1/V2)"
custommaterial steel200 name="Steel (E=200 GPa, nu=0.30)" e=200000 nu=0.30 yield=250 density=7.85
material steel200

box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load shear tip force=(0, 0, -100)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Canonical 100 × 10 × 10 mm Cantilever Geometry & Boundary Conditions *(Lens: `SETUP`)*

Open Setup (2). The beam has length L = 100 mm along X and a square b × h = 10 × 10 mm cross-section (area A = 100 mm², second moment of area I = b h³ / 12 = 833.333 mm⁴) with Young’s modulus E = 200,000 MPa and Poisson’s ratio ν = 0.30. The root face x = 0 ("root") is clamped (fixed=xyz) and the free tip x = 100 mm ("tip") carries a downward shear force F = 100 N.

- **Expected Check**: Setup highlights the x = 0 root clamp and the −100 N Z-load distributed over the 10 × 10 mm tip face.

### 2. Verify Tip Deflection & Mid-Span Bending Stress Against Closed-Form Beam Theory *(Lens: `RESULTS`)*

Switch to Results (3). By Euler–Bernoulli theory, tip deflection is δ_EB = F L³ / (3 E I) = 100 × 10⁶ / (3 × 200,000 × 833.333) = 0.2000 mm; adding Cowper’s transverse shear deflection δ_shear = F L / (κ A G) with κ = 5/6 and G = 76,923 MPa gives δ_Timo = 0.2016 mm. At mid-span (x = 50 mm, away from the 3D Poisson root clamp), outer-fiber bending stress is σ_xx = F (L − x) (h/2) / I = 100 × 50 × 5 / 833.333 = 30.00 MPa.

- **Expected Check**: Fieldsmith P2 FEA reproduces mid-span fiber stress within 0.15% and tip deflection within 1.1% of the closed-form Timoshenko reference.

## 3. Theoretical Manual — Analytic Verification A1 — Euler–Bernoulli & Timoshenko Cantilever Flexure (V1 / V2)

Derives the exact 1D Euler–Bernoulli and Timoshenko cantilever deflection and bending stress formulas and explains why 10-node quadratic (P2) tetrahedra avoid the shear locking of 4-node linear (P1) elements.

### A1.1 Closed-Form Flexural & Transverse Shear Deflection

```text
I = b h³ / 12 = 833.333 mm⁴,     G = E / (2(1 + ν)) = 76,923.1 MPa
δ_total = F L³ / (3 E I) + F L / (κ A G) = 0.20000 + 0.00156 = 0.20156 mm
σ_xx(x, z) = F (L − x) (z − h/2) / I
```

Because the P2 basis contains complete degree-2 polynomials on each tetrahedron, the linear bending strain ε_xx(z) ∝ (z − h/2) lies inside the finite-element approximation space, eliminating P1 shear locking.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Run Cantilever Solve via API (`solve`)

Solve the V1/V2 cantilever benchmark and return peak deflection and compliance.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 30000
  }
}
```
