# A11 — Analytic Benchmark A11: TPMS Lattice Homogenisation — Cubic Zener Anisotropy & Hill–Mandel Yield/Buckling Laws (V6 / V36 / V42 / V48)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Verified by V6, V36, V42 & V48 · Cubic Homogenisation & Failure Laws · PASS (0.001%)
> **Repository Path**: `docs/analytic-tests/A11-tpms-cubic-anisotropy-and-gibson-ashby.md` · **Starter Model**: `a11-tpms-cubic-homogenisation.fsm` · **Verification Reports**: `docs/verification/V6.md`, `docs/verification/V36.md`, `docs/verification/V42.md`, `docs/verification/V48.md`

Verify the routed 3-parameter cubic compliance tensor S_ijkl(ρ) across all 6 TPMS families (`gyroid`, `primitive`, `diamond` × `sheet`, `network`), directional modulus ratio E_[110] / E_[100], and coupon-fitted yield and buckling power laws.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Cubic Directional Modulus Ratio E_[110] / E_[100] (Gyroid / Primitive / Diamond Sheet at ρ=0.22) | `1/E(n) = S₁₁ − 2(S₁₁ − S₁₂ − ½ S₄₄)(n₁²n₂² + n₂²n₃² + n₃²n₁²)` | **1.13584 / 2.06942 / 0.81279** | **1.13583 / 2.06943 / 0.81278** | 0.001% / 0.000% / 0.001% | ≤ 0.02% |
| 3D Affine Patch Stress Recovery vs Σ n ⊗ n ([100] & [110] Cubic Directions) | `u(x) = (S : σ) x,   σ = Σ (n ⊗ n)   (Σ = 10 MPa)` | **0.000 relative error** | **5.33×10⁻¹⁵ / 4.00×10⁻¹⁵ rel** | 0.000% | ≤ 10⁻⁶ |
| Hill–Mandel Gyroid Sheet vs Network Yield & Elastic Buckling Power Laws (ρ ∈ [0.2, 0.4]) | `Gyroid Sheet: σ_y*/σ_ys = 0.3976 ρ^1.064,  σ_el*/E_s = 0.9212 ρ^3.263 (Stretch > Bend: 1.313×)` | **0.0710 / 0.1084 / 0.1501 (ρ=0.2/0.3/0.4)** | **0.0708 / 0.1090 / 0.1496** | 0.21% / 0.59% / 0.31% | ≤ 2.0% |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "A11 — TPMS Cubic Anisotropy & Yield/Buckling Law Benchmark (V36/V42/V48)"
material pa12

box coupon size=(30, 10, 10) at=(0, 0, 0)
region root = coupon.nx
region tip = coupon.px
support clamp root fixed=xyz
load bend tip force=(0, 0, -45)

fill gyroid cell=4 wall=0.6..1.6
build layer=0.1 nozzle=0.4
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Two-Pass Stress-Graded TPMS Specification (`fill` + `build`) in Make (4) *(Lens: `MAKE`)*

Open Make (4). Every TPMS unit cell (`gyroid`, `primitive`, `diamond` in `sheet` or `network` mode) has cubic symmetry (O_h point group) characterised by three independent compliance constants (S₁₁, S₁₂, S₄₄) and Zener anisotropy ratio A_Z = 2(S₁₁ − S₁₂)/S₄₄ (A_Z = 1.219 for gyroid sheet, 4.384 for primitive sheet, 0.734 for diamond sheet).

- **Expected Check**: Make (4) displays the graded Gyroid wall range 0.6..1.6 mm bounded by nozzle/layer constraints.

### 2. Verify Yield & Buckling Utilization Readouts in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). During pass 2, each element uses its local relative density ρ_e to evaluate the cubic stiffness tensor C_ijkl(ρ_e) and checks local principal stresses against the coupon-fitted Hill–Mandel yield law σ_y*(ρ_e) = c_y σ_ys ρ_e^{n_y} and buckling law σ_el*(ρ_e) = c_b E_s ρ_e^{n_b}.

- **Expected Check**: Results report both continuum von Mises stress and lattice yield/buckling utilization factors.

## 3. Theoretical Manual — Analytic Verification A11 — Cubic TPMS Homogenisation, Zener Anisotropy & Hill–Mandel Failure Scaling (V6 / V36 / V42 / V48)

Derives the 4th-order cubic elasticity tensor of triply periodic minimal surfaces from [100] and [110] periodic unit-cell coupon solves and establishes the stretching- vs bending-dominated failure exponents.

### A11.1 Cubic Direction Formula & [110] Shear Compliance Inversion

```text
1 / E(n) = S₁₁ − 2(S₁₁ − S₁₂ − ½ S₄₄)(n₁² n₂² + n₂² n₃² + n₃² n₁²)
For n = [1, 1, 0]/√2:   1 / E_[110] = S₁₁ − ½(S₁₁ − S₁₂ − ½ S₄₄)   ⇒   S₄₄ = 4 / E_[110] − 2(S₁₁ + S₁₂)
```

Sheet-mode gyroids deform primarily by membrane stretching (n_y = 1.064 ≈ 1), giving a 1.313× higher yield allowable at ρ = 0.22 than strut-like network gyroids (n_y = 1.376).

## 4. Reproducible Automation (MCP / REST API & CLI)

### Run Two-Pass Graded TPMS Solve via API (`solve`)

Execute pass-1 sizing and pass-2 cubic homogenised solve on A11.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 25000
  }
}
```
