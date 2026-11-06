# A7 — Analytic Benchmark A7: Spatially Distributed Surface Tractions — Triangular Ramp & Piecewise Load Profiles (V15)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V15 · Exact Face-Local Traction Quadrature · PASS (0.13%)
> **Repository Path**: `docs/analytic-tests/A7-distributed-face-tractions.md` · **Starter Model**: `a7-distributed-face-tractions.fsm` · **Verification Reports**: `docs/verification/V15.md`

Verify exact P2 quadrature of spatially varying face-local load profiles fn="(x + 50) / 100" against Euler–Bernoulli ramp deflection δ_tip = 11 w₀ L⁴ / (120 E I), resultant R_z, and centroidal moment M_y.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Triangular Ramp Tip Deflection δ_tip (w(x) = w₀ x / L, w₀ = 4 N/mm) | `δ_tip = 11 w₀ L⁴ / (120 E I)   (L = 100 mm, b = h = 10 mm, E = 200 GPa)` | **0.220000 mm** | **0.220286 mm** | 0.130% | ≤ 2.0% |
| Triangular Ramp Resultant Force R_z & Centroidal Moment M_y | `R_z = −½ q₀ b L = −200.0 N,   M_y = |R_z| (2L/3) = 13,333.33 N·mm` | **-200.00 N / 13,333.33 N·mm** | **-200.00 N / 13,332.89 N·mm** | 0.000% / 0.003% | ≤ 1.0% |
| Piecewise Step Load Tip Deflection & Moment (w = 2 N/mm on [0,75], 4 N/mm on (75,100]) | `δ_tip = ∫₀^L M(u) u du / (E I) = 38,574,219 / (E I);   R_z = −250 N, M_y = 14,375 N·mm` | **0.231445 mm / 14,375.0 N·mm** | **0.230546 mm / 14,299.7 N·mm** | 0.389% / 0.524% | ≤ 2.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A7 — Distributed Triangular & Piecewise Face Tractions (V15)"
custommaterial ref_steel name="Reference Elastic Steel (E=200 GPa, nu=0.30)" e=200000 nu=0.30 a=1.2e-5 yield=350 density=7.85
material ref_steel

box beam size=(100, 10, 10) at=(0, 0, 0)
region root = beam.nx
region top_face = beam.pz
support clamp root fixed=xyz

load ramp top_face local=(0, 0, -0.4) fn="(x + 50) / 100" case="Triangle"
load step_profile top_face local=(0, 0, -1) fn="x <= 25 ? 0.2 : 0.4" case="Piecewise"
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Face-Local Coordinate Frame & Symbolic fn="..." Traction Profiles *(Lens: `SETUP`)*

Open Setup (2). On the top face z = 10 mm (centroid at x = 50 mm, y = 5 mm), local x runs from −50 mm at the root to +50 mm at the free tip. Case "Triangle" applies local=(0, 0, -0.4) scaled by fn="(x + 50) / 100" (linear ramp from 0 at the root to 0.4 MPa at the tip), while Case "Piecewise" applies a step profile fn="x <= 25 ? 0.2 : 0.4".

- **Expected Check**: Both load cases are defined on top_face with symbolic spatial functions evaluated at P2 surface quadrature points.

### 2. Compare Ramp vs Piecewise Deflection & Reaction Moments in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). For the triangular ramp, Euler–Bernoulli integration gives δ_tip = 11 w₀ L⁴ / (120 E I) = 0.22000 mm with resultant R_z = −200.0 N and pressure-centroid moment M_y = 13,333.3 N·mm; Fieldsmith measures 0.22029 mm (0.130% error) and M_y = 13,332.9 N·mm (0.003% error).

- **Expected Check**: Triangle case tip deflection is ~0.220 mm; Piecewise case tip deflection is ~0.231 mm.

## 3. Theoretical Manual — Analytic Verification A7 — Face-Local Least-Squares Frames & Spatial Traction Quadrature (V15)

Derives the consistent P2 boundary traction vector f_a = ∫_Γ φ_a(x) t(x_local) dA for non-uniform spatial load profiles and proves exact resultant and first-moment conservation on linear ramps.

### A7.1 Euler–Bernoulli Fourth-Order ODE under Linearly Ramped Line Load w(x) = w₀ x / L

```text
E I w''''(x) = w₀ x / L,   w(0) = w'(0) = 0,   w''(L) = w'''(L) = 0
δ_tip = w(L) = 11 w₀ L⁴ / (120 E I),   R_z = ∫₀^L w(x) dx = ½ w₀ L,   x̄ = (∫ x w(x) dx)/R_z = 2L / 3
```

Because the P2 6-node triangle shape functions are quadratic and integrated by a degree-2 surface quadrature rule, any affine traction profile t(x) is integrated to machine precision over every boundary facet.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Distributed Ramp & Piecewise Load Cases via API (`solve`)

Run the multi-case P2 solver on A7.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 35000
  }
}
```
