# A21 — Analytic Benchmark A21: 3D P2 Unilateral Signorini–Winkler Contact, Initial Clearance Gap & Prying Liftoff

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by A21 & V63 · Exact 3D P2 Signorini Gap Patch Test + Prying Liftoff Equilibrium
> **Repository Path**: `docs/analytic-tests/A21-signorini-unilateral-contact-and-prying-liftoff.md` · **Starter Model**: `a21-signorini-unilateral-contact-and-prying-liftoff.fsm` · **Verification Reports**: `docs/verification/V63.md`

Verify 3D quadratic P2 unilateral Signorini boundary contact (u·n − g₀ ≤ 0, p_n ≥ 0, p_n(u·n − g₀) = 0) integrated by positive-weight 9-point Duffy surface quadrature and solved via Semi-Smooth Newton (SSN) active-set iteration against exact analytical Winkler gap-closure equilibrium and eccentric prying liftoff.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Uniform Compression Normal Contact Pressure p_n | `p_n = P₀ (exact 3D uniaxial patch test on Γ_c = 400 mm²)` | **25.0000 MPa** | **25.00 MPa (3D P2 SSN, 9-pt Duffy)** | < 0.2% | ≤ 1.0% |
| Uniform Compression Bearing Force ∫ p_n dA | `F_c = P₀ A = 25 MPa × 400 mm²` | **10000.0 N** | **10000.0 N** | < 0.2% | ≤ 1.0% |
| Gap-Offset Top-Face Deflection \|u_z(z = H)\| | `g₀ + P₀ / k + P₀ H / E = 0.002 + 25/5000 + 25×8/200000` | **0.0080 mm (penetration u_n − g₀ = 0.0050 mm)** | **0.0050 mm penetration** | < 0.5% | ≤ 1.5% |
| Eccentric Prying Vertical Equilibrium & Heel Liftoff | `∫ p_n dA = F_down − F_up = 1000 N,  A_active / A_total < 0.45,  max(g₀ − u·n) > 0.05 mm` | **F_c = 1000.0 N,  liftoff > 0.05 mm** | **1000.0 N,  partial contact** | PASS | ≤ 1.5% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A21 — Signorini Unilateral Contact, Gap Closure & Prying Liftoff"
custommaterial steel_a21 name="A21 Benchmark Steel" e=200000 nu=0.30 yield=600 density=7.85 wall=0.5
material steel_a21

box left_span size=(21, 10, 8) at=(10.5, 5, 4)
box right_span size=(21, 10, 8) at=(29.5, 5, 4)

region x0 = left_span.nx
region y0 = left_span.ny + right_span.ny
region bottom = left_span.nz + right_span.nz
region top_all = left_span.pz + right_span.pz
region left_top = left_span.pz
region right_top = right_span.pz

support sx x0 fixed=x
support sy y0 fixed=y
contact seat bottom k=5000 gap=0.002

load crush top_all pressure=25.0 case="Compression"
load bolt_clamp left_top force=(0, 0, -3000) case="Prying"
load pry_pull right_top force=(0, 0, 2000) case="Prying"

solve

assert "Compression uniform contact pressure p_n" metric="max_contact_pressure:Compression" target=25.0 tol="1.5%" unit="MPa"
assert "Compression integrated bearing force F_c" metric="contact_force_n:Compression" target=10000.0 tol="1.5%" unit="N"
assert "Compression full-seat active contact ratio" metric="contact_area_ratio:Compression" target=1.0 tol="0.02" unit="-"
assert "Compression foundation elastic penetration u_n - g0" metric="max_penetration_mm:Compression" target=0.0050 tol="2.0%" unit="mm"
assert "Prying vertical bearing force equilibrium" metric="contact_force_n:Prying" target=1000.0 tol="2.0%" unit="N"
assert "Prying partial-seat active contact ratio" metric="contact_area_ratio:Prying" target="<= 0.45" tol="<= 0.45" unit="-"
assert "Prying heel unilateral liftoff separation" metric="max_liftoff_mm:Prying" target=">= 0.05" tol=">= 0.05" unit="mm"
assert "Prying toe peak pressure concentration" metric="max_contact_pressure:Prying" target=">= 20.0" tol=">= 20.0" unit="MPa"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Unilateral Contact Seat & Initial Clearance Gap in Setup (2) *(Lens: `SETUP`)*

Open Setup (2) and select `seat` under **Contacts**. The `40 × 10 × 8 mm` steel flange (`E = 200 GPa`, `ν = 0.30`) has symmetry rollers `sx` (`x = 0`) and `sy` (`y = 0`) with **zero** vertical `Z` Dirichlet supports: vertical equilibrium is carried entirely by the unilateral Signorini–Winkler contact foundation (`k = 5000 MPa/mm`, initial clearance gap `g₀ = 0.002 mm`) on `z = 0`.

- **Expected Check**: Because `sx` and `sy` exert zero `Z` reaction, 100% of net downward vertical load in every load case must be equilibrated by `∫_{Γ_c} p_n dA ≥ 0`, while any region tending into tension (`u·n < g₀`) detaches cleanly (`p_n = 0`).

### 2. Compare Full-Seat Compression vs. Eccentric Prying Liftoff in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). In **Compression**, `25 MPa` uniform top pressure closes the `0.002 mm` gap across 100% of the `400 mm²` seat (`p_n = 25.0 MPa`, `F_c = 10,000 N`, penetration `u_n − g₀ = 0.0050 mm`). Switch to **Prying** (`3,000 N` toe clamp on `left_top` minus `2,000 N` upward pull on `right_top`) and click **⊥ Contact Pressure p_n**.

- **Expected Check**: In **Prying**, the right overhang lifts off the foundation (`p_n = 0` on the detached heel, `contact_area_ratio ≤ 0.45`, `max_liftoff_mm ≥ 0.05 mm`), concentrating the exact `1,000 N` net vertical reaction onto the compressed left toe (`max_contact_pressure ≥ 20 MPa`, > 8× nominal `2.5 MPa`).

## 3. Theoretical Manual — Analytic Verification A21 — 3D P2 Signorini–Winkler Unilateral Contact & Semi-Smooth Newton Active Set

Verifies Fieldsmith's 3D quadratic P2 boundary unilateral contact formulation, positive-weight 9-point Duffy surface quadrature on 6-node curved/flat P2 triangles, and primal-dual active-set (Semi-Smooth Newton) solver against exact analytical gap-closure elasticity and eccentric prying liftoff.

### A21.1 Regularised Signorini Complementarity & Positive-Weight P2 Surface Quadrature

```text
p_n(x) = k_n max(0, u(x)·n − g₀),   p_n ≥ 0,   (p_n > 0 ⇒ u·n ≥ g₀)
K_c(𝒜) = ∑_{q ∈ Γ_c} w_q k_n H(u(x_q)·n − g₀) (N_qᵀ n)(nᵀ N_q),   f_c(𝒜) = ∑_{q ∈ Γ_c} w_q k_n g₀ H(u(x_q)·n − g₀) N_qᵀ n
```

Notice that vertex-only quadrature fails on 6-node P2 triangles because P2 vertex shape functions have zero mean integral on a triangle (∫_T φ_vertex dA = 0)! Mapping a 3×3 tensor Gauss–Legendre rule through the Duffy transform onto T yields 9 strictly positive weights of degree 4, guaranteeing symmetric positive semi-definite contact tangent matrices K_c(𝒜) and exact bilinear work integration.

### A21.2 Exact 3D Patch Test with Clearance Gap & Unilateral Prying Equilibrium

```text
u(x, y, z) = ( +(ν P₀ / E) x,  +(ν P₀ / E) y,  −(g₀ + P₀ / k_n) − (P₀ / E) z )
u_n(z = 0) − g₀ = P₀ / k_n = 25 / 5000 = 0.0050 mm,   ∫_{Γ_c} p_n dA = ∑ F_{z,ext}
```

Under uniform top compression P₀ with frictionless symmetry rollers on x = 0 and y = 0, the exact 3D displacement field is linear in (x, y, z) ⊂ P₂, closing the uniform gap g₀ = 0.002 mm and recovering p_n ≡ P₀ everywhere. Under eccentric upward pull on the right half-span, tension cannot cross the unilateral interface: the SSN active set deactivates lifted-off quadrature points in 3–5 iterations while preserving exact global vertical force balance ∫_{Γ_c} p_n dA = F_down − F_up.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve 3D P2 unilateral contact patch test and prying liftoff

```json
{
  "tool": "solve",
  "arguments": {
    "dofs": 14000
  }
}
```

### Verify all 8 Signorini gap-closure and prying liftoff contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 14000
  }
}
```

---
*Generated from `examples/analytic/A21-signorini-unilateral-contact-and-prying-liftoff.fsm` by `node scripts/gen-sheet-docs.mjs`.*
