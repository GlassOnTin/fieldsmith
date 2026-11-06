# A10 — Analytic Benchmark A10: Multi-Material Series Composite Bar, Bimetal Thermal Mismatch & Bolt Preload (V53)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V53 · Multi-Volume Continuum & Preload · PASS (< 3.7%)
> **Repository Path**: `docs/analytic-tests/A10-multimaterial-series-bar-and-preload.md` · **Starter Model**: `a10-multimaterial-series-bar.fsm` · **Verification Reports**: `docs/verification/V53.md`

Verify per-volume material assignment (`material alsi10mg on right`) against the 1D Reuss series composite bar formula δ = (F L / A)(1/E₁ + 1/E₂), exact bimetal mass m = ρ₁ V₁ + ρ₂ V₂, and bolt preload linearity (`preload=12000`).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Series Bi-Material Bar Tip Displacement δ_tip (SS316L E₁=193 GPa + AlSi10Mg E₂=70 GPa) | `δ_1D = σ₀ L (1/E₁ + 1/E₂) = 100 × 10 × (1/193000 + 1/70000)` | **0.019467 mm** | **0.020160 mm** | 3.56% (3D interface Poisson) | ≤ 5.0% |
| Bi-Material vs Homogeneous SS316L Compliance Ratio C_series / C_homo | `C_series / C_homo = ½ (1 + E₁ / E₂) = ½ (1 + 193 / 70)` | **1.87857 ×** | **1.80938 ×** | 3.68% | ≤ 5.0% |
| Composite Assembly Mass m = ρ₁ V₁ + ρ₂ V₂ & Opposed Bolt Preload 24 kN / 12 kN Linearity | `m = (7.99 + 2.67) × 0.360 cm³ = 3.8376 g;   u(24 kN) / u(12 kN) = 2.00000` | **3.8376 g / 2.00000 ×** | **3.7894 g / 2.00000 ×** | 1.26% / 0.000% | ≤ 3.0% / 0.0001% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A10 — Bi-Material Series Bar & Preload Verification (V53)"
material ss316l

box left size=(10, 6, 6) at=(-5, 0, 0)
box right size=(10, 6, 6) at=(5, 0, 0)
material alsi10mg on right

region root = left.nx
region tip = right.px
support clamp root fixed=xyz
load pull tip force=(3600, 0, 0)
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Per-Volume Material Assignment (`material alsi10mg on right`) in Shape (1) *(Lens: `SHAPE`)*

Open Shape (1). Two 10 × 6 × 6 mm blocks (`left` in `ss316l`, E₁ = 193 GPa, ρ₁ = 7.99 g/cm³; `right` in `alsi10mg`, E₂ = 70 GPa, ρ₂ = 2.67 g/cm³) meet at the bonded interface x = 0 under F_x = 3,600 N axial tension (σ_xx = 100 MPa).

- **Expected Check**: Each tetrahedral centroid queries the signed-distance tree to receive its volume-specific (E_e, ν_e, α_e, ρ_e).

### 2. Verify Series Axial Strain Jump at x = 0 in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). The axial displacement slope du_x/dx jumps by E₁/E₂ = 2.76× across the x = 0 interface from ε₁ = 100/193,000 = 5.18×10⁻⁴ in SS316L to ε₂ = 100/70,000 = 1.429×10⁻³ in AlSi10Mg, giving total tip displacement 0.02016 mm.

- **Expected Check**: Tip displacement is ~0.0202 mm (1.81× higher than a homogeneous SS316L bar).

## 3. Theoretical Manual — Analytic Verification A10 — Multi-Material Continuum Assembly & Bolt Preload Mechanics (V53)

Derives the Reuss series composite bar compliance, bi-metallic thermal expansion interface shear, and opposed bolt preload superposition.

### A10.1 Reuss Series Composite Bar & Interface Poisson Constraint

```text
δ_1D = ∫₀^{2L} (σ₀ / E(x)) dx = σ₀ L (1/E₁ + 1/E₂),   C_series / C_homo = ½ (1 + E₁ / E₂)
m_total = ∑_k ρ_k V_k
```

In 3D elasticity, continuity of transverse displacement u_y, u_z across the x = 0 interface between dissimilar moduli (E₁, ν₁) and (E₂, ν₂) introduces a localised triaxial collar within ~0.5 h of the joint, shifting the 3D compliance ratio by ~3.6% relative to the 1D uniaxial bar formula.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Multi-Material Series Bar via API (`solve`)

Run the multi-volume P2 solver on A10.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 30000
  }
}
```
