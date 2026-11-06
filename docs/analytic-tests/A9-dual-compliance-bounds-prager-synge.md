# A9 — Analytic Benchmark A9: Certified Dual Prager–Synge / Fenchel Compliance Bounds C_h ≤ C_true ≤ C(σ*) (V34 / V35)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V34 & V35 · Certified Primal–Dual Energy Bracket · PASS (0.001%)
> **Repository Path**: `docs/analytic-tests/A9-dual-compliance-bounds-prager-synge.md` · **Starter Model**: `a9-dual-compliance-bounds.fsm` · **Verification Reports**: `docs/verification/V34.md`, `docs/verification/V35.md`

Verify Galerkin primal compliance lower bound C_h = fᵀ u_h ≤ C_true (V34) and H(div)-equilibrated quadratic stress recovery upper bound C(σ*) (V35) on in-span uniaxial tension and off-span cubic manufactured fields.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| In-Span Uniaxial Tension Primal Lower Bound C_h & Dual Upper Bound C(σ*) | `C_true = F² L / (E A) = c² V / E   (40×10×10 mm bar, F = 10 N, E = 1500 MPa)` | **0.02666667 N·mm (Pipeline) / 0.4000000 N·mm (T1)** | **0.02666667 N·mm / 0.4000000 N·mm** | 0.000% | ≤ 0.0001% |
| Off-Span Cubic Field (T2) Primal Lower Bound C_h ≤ C_true (24 → 192 tets) | `C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0   (Strict Galerkin Orthogonality)` | **18,432.00 N·mm** | **18,430.86 → 18,431.93 N·mm** | 0.006% → 0.0004% | C_h ≤ C_true |
| Off-Span Cubic Field (T2) Equilibrated Stress Certificate C(σ*) & Witnesses | `J(σ*) = ∫ σ* : S : σ* dV  s.t.  div σ* + b = 0,  ⟦σ* n⟧ = 0,  σ* n|_ΓN = t̄` | **18,432.00 N·mm (Witnesses = 0 N)** | **18,431.99 N·mm (Equil 1.04×10⁻⁸ N)** | 0.00003% | ≤ 0.01% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A9 — Certified Dual Compliance Bounds (V34 / V35)"
custommaterial ref_iso name="Pinned Isotropic Reference (E=1500 MPa, nu=0.30)" e=1500 nu=0.30 a=1.0e-5 yield=50 density=1.2
material ref_iso

box bar size=(40, 10, 10) at=(0, 0, 0)
region x0 = bar.nx
region x1 = bar.px
region y0 = bar.ny
region z0 = bar.nz

support sx x0 fixed=x
support sy y0 fixed=y
support sz z0 fixed=z
load pull x1 force=(10, 0, 0)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Pure-Tension Bar with Exact Kinematic Supports in Setup (2) *(Lens: `SETUP`)*

Open Setup (2). A 40 × 10 × 10 mm bar (A = 100 mm², V = 4,000 mm³, E = 1,500 MPa) is supported by single-component rollers on x = 0, y = 0, z = 0 and pulled by F_x = 10 N on x = 40 mm (traction c = 0.1 MPa).

- **Expected Check**: The exact displacement u* = (c/E)(x, −νy, −νz) lies inside the P2 span, so both primal and dual compliance bounds collapse onto the exact energy.

### 2. Verify Exact Compliance C = F² L / (E A) = 0.026667 N·mm and Peak u_x = 0.002667 mm in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). The exact work of the external load is C_true = F u_x(L) = 10 × (0.1 × 40 / 1500) = 0.02666667 N·mm. Fieldsmith reports CaseResult.compliance = 0.02666667 N·mm and peak displacement 0.00266667 mm (0.000% error).

- **Expected Check**: Displacement is linear in X and compliance equals 0.02667 N·mm.

## 3. Theoretical Manual — Analytic Verification A9 — Prager–Synge Hypercircle & Fenchel Dual Compliance Certificates (V34 / V35)

Proves that P2 Galerkin finite elements provide a guaranteed lower bound C_h ≤ C_true on structural compliance, while H(div)-equilibrated piecewise-quadratic stress recovery provides the complementary Fenchel certificate.

### A9.1 Galerkin Orthogonality Lower Bound & H(div) Equilibrated Recovery

```text
a(u* − u_h, v_h) = 0  ∀ v_h ∈ V_h   ⇒   C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0
C(σ*) = ∫_Ω σ* : S : σ* dV,   with  div σ* + b = 0,  ⟦σ* n⟧_F = 0,  σ* n|_{Γ_N} = t̄
```

Together, C_h and C(σ*) bracket the true continuum compliance and measure discretization error without requiring an analytic solution.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Dual Compliance Benchmark via API (`solve`)

Run the P2 solver on A9 and verify compliance and peak displacement.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 25000
  }
}
```
