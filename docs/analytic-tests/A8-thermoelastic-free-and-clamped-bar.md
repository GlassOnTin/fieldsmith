# A8 — Analytic Benchmark A8: Thermo-Elastic Free Expansion vs End-Clamped Uniaxial Thermal Stress (V19)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V19 · Exact Duhamel–Neumann Patch Identities · PASS (0.000%)
> **Repository Path**: `docs/analytic-tests/A8-thermoelastic-free-and-clamped-bar.md` · **Starter Model**: `a8-thermoelastic-clamped-bar.fsm` · **Verification Reports**: `docs/verification/V19.md`

Verify the two fundamental discrete Duhamel–Neumann thermo-elastic identities: (a) stress-free affine expansion u = α ΔT x under kinematic sliding supports, and (b) exact axial compressive stress σ_xx = −E α ΔT under end-clamped expansion.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Leg (a) Free Expansion Nodal Displacement u(x) = α ΔT x & Stress | `u* = α ΔT (x, y, z),   σ_ij* ≡ 0 MPa ∀ x ∈ Ω   (ΔT = 40 K)` | **max |u − u*| = 0 mm,  |σ| = 0 MPa** | **9.98×10⁻¹² mm,  2.99×10⁻⁷ MPa** | 0.000% | ≤ 10⁻⁴ mm / 0.1 MPa |
| Leg (b) End-Clamped Uniaxial Thermal Stress σ_xx (u_x(0) = u_x(L) = 0) | `σ_xx = −E α ΔT = −2300 × (7.0×10⁻⁵) × 40 = −6.44 MPa  (or −64.40 MPa at E=23 GPa)` | **-64.40000 MPa** | **-64.40000 MPa** | 0.000% | ≤ 0.1% |
| Leg (b) Transverse Free-Poisson Expansion u_y(y), u_z(z) & Transverse Stress | `u_y = (1 + ν) α ΔT y,   u_z = (1 + ν) α ΔT z,   σ_yy = σ_zz = 0 MPa` | **0.00000 MPa** | **-1.59×10⁻⁸ / -1.13×10⁻⁷ MPa** | 0.000% | ≤ 0.1 MPa |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A8 — End-Clamped Thermo-Elastic Bar (V19)"
material pla

box bar size=(40, 10, 10) at=(0, 0, 0)
region x0 = bar.nx
region x1 = bar.px
region y0 = bar.ny
region z0 = bar.nz

support fix_x0 x0 fixed=x
support fix_x1 x1 fixed=x
support fix_y0 y0 fixed=y
support fix_z0 z0 fixed=z
temperature uniform_hot bar.nx+bar.px+bar.ny+bar.py+bar.nz+bar.pz t=60 case="ClampedHot"
solve tref=20
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Kinematic Sliding + End-Clamped Supports in Setup (2) *(Lens: `SETUP`)*

Open Setup (2). Fixing u_x = 0 on both ends (x = 0 and x = 40 mm) while fixing only u_y = 0 on y = 0 and u_z = 0 on z = 0 removes all 6 rigid-body modes while leaving the bar completely free to Poisson-expand laterally in Y and Z when heated from T_ref = 20 °C to T = 60 °C (ΔT = 40 K).

- **Expected Check**: All six boundary faces carry T = 60 °C, and the four single-component supports eliminate rigid motion without inducing artificial transverse shear.

### 2. Verify Uniform Axial Thermal Stress σ_xx = −E α ΔT in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). Because u = (0, (1+ν)αΔT y, (1+ν)αΔT z) is affine and lies inside the P2 finite-element space, the discrete thermo-elastic solve reproduces σ_xx = −E α ΔT and σ_yy = σ_zz = 0 to 8+ significant figures.

- **Expected Check**: Uniform compressive thermal stress across the entire bar with zero transverse stress.

## 3. Theoretical Manual — Analytic Verification A8 — Exact Discrete Thermo-Elastic Patch Test (V19)

Proves that the consistent thermal load vector f_th,a = ∫_Ω B_aᵀ C α ΔT(x) dV reproduces both stress-free thermal expansion and uniaxial constrained thermal stress to machine precision on arbitrary tetrahedral meshes.

### A8.1 Uniaxial Constrained Thermal Expansion & Poisson Lateral Strain

```text
ε_xx = 0  ⇒  σ_xx = −E α ΔT,   σ_yy = σ_zz = 0
ε_yy = ε_zz = (1/E)(−ν σ_xx) + α ΔT = (1 + ν) α ΔT
```

When axial expansion is blocked (ε_xx = 0) while lateral faces are traction-free (σ_yy = σ_zz = 0), Poisson compression adds ν α ΔT to the free thermal strain α ΔT in the transverse directions, amplifying lateral expansion by (1 + ν).

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Thermo-Elastic Bar via API (`solve`)

Run the coupled conduction + thermo-elastic solver on A8.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 25000
  }
}
```
