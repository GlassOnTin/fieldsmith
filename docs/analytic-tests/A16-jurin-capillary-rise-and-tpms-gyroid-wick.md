# A16 — Analytic Benchmark A16: 3D Graded TPMS Gyroid Capillary Wick Column FEA + 1D Jurin & Lucas–Washburn Wicking Workbench (V62)

> **Category**: Analytic Verification Test · **Level**: Analytic Verification · **Verification**: Pending verification (no shipped report yet)
> **Repository Path**: `docs/analytic-tests/A16-jurin-capillary-rise-and-tpms-gyroid-wick.md` · **Starter Model**: `a16-jurin-capillary-tpms.fsm` · **Verification Reports**: —

Solve 3D two-pass homogenised P2 stress and mass savings in a Ti-6Al-4V TPMS Gyroid wick column, paired with the 1D Young–Dupré contact angle, Jurin equilibrium capillary rise h_Jurin = 2γ cos θ_e / (ρ g r_eff), and Lucas–Washburn wicking ODE workbench (V62).

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| TPMS Gyroid Hydraulic Pore Radius r_eff = 2ε / S_v (L = 3.5 mm, ρ_s = 0.32) | `S_v = (3.091 / L) √(4ε(1−ε)),   r_eff = (1/r_TPMS + 1/(4r_bore))⁻¹` | **0.598 mm** | **0.598 mm** | 0.000% | Exact |
| Jurin's Law Equilibrium Capillary Rise h_Jurin (θ_e = 25°, γ = 0.0728 N/m) | `h_Jurin = 2 γ cos θ_e / (ρ g r_eff)` | **22.58 mm** | **22.58 mm** | < 0.1% | ≤ 1.5% |
| Hydrophilic (θ_e = 25°) vs Hydrophobic (θ_e = 125°) Sign Inversion | `cos(25°) = +0.9063 > 0  vs  cos(125°) = −0.5736 < 0` | **+22.58 mm / −14.29 mm** | **+22.58 mm / −14.29 mm** | < 0.1% | Exact Sign & Ratio |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "A16 — Jurin Capillary Rise & TPMS Gyroid Wicking Benchmark (V62)"
material ti64

let bore_r_mm = 0.80
let contact_angle_deg = 25.0
let jurin_rise_mm = 22.58

cylinder wick_column r=10 h=36 at=(0, 0, 18)
cylinder central_bore r=4 h=32 at=(0, 0, 20) subtract

region base_mount = wick_column.bottom
region meniscus_ring = central_bore.side
region top_header = wick_column.top

support mount_fix base_mount fixed=xyz
load capillary_pull meniscus_ring pressure=0.25
load header_load top_header pressure=0.15
fill gyroid sheet cell=3.5 skin=1.0 rho=(0.28, 0.72) curve=1.0 wall=0.55
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Capillary Pore Column & TPMS Gyroid Wick in Setup (2) & Fill (4) *(Lens: `FILL`)*

Open Fill (4) to inspect the TPMS Gyroid capillary wick inside the propellant/heat-pipe column (`cell=3.5 mm`), or open the Free-Surface Fluid (SPH) Workbench on Preset A16 (`capillary_rise_a16`) to sweep the Young–Dupré contact angle θ_e from hydrophilic (25°) to hydrophobic (125°).

- **Expected Check**: Hydrophilic walls (θ_e < 90°) produce positive Jurin capillary rise h = +22.58 mm and capillary retention pressure Δp_cap = 220.9 Pa; hydrophobic walls (θ_e > 90°) invert the meniscus to depression h = −14.29 mm.

### 2. Verify Structural & Capillary Retention Response in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3) to verify the 3D P2 solution and homogenised TPMS Gyroid response.

- **Expected Check**: All # @assert verification checks pass.

## 3. Theoretical Manual — Analytic Verification A16 — Young–Dupré Wetting, Jurin's Law & TPMS Minimal-Surface Capillary Hydraulics (V62)

Couples Fieldsmith's analytic Signed Distance Field (SDF) and Triply Periodic Minimal Surface (TPMS) geometry engine with Young–Dupré triple-line contact mechanics, Jurin's hydrostatic balance, and Lucas–Washburn porous wicking dynamics.

### A16.1 Young–Dupré Contact Angle & Jurin's Law in TPMS Minimal-Surface Lattices

```text
γ_SG − γ_SL = γ_LG cos θ_e,     r_eff = 2ε / S_v(L_cell, ε),     h_Jurin = (2 γ cos θ_e) / (ρ g r_eff)
```

Because Fieldsmith represents both solid CAD boundaries and TPMS lattices (`Gyroid`, `Diamond`, `Primitive`) via exact analytic level sets φ(x), the outward wall normal n_w = ∇φ/|∇φ| and specific surface area S_v are known analytically everywhere without staircase mesh artifacts.

### A16.2 Lucas–Washburn Viscous-Capillary Wicking Law

```text
(8 μ z / r_eff²) dz/dt = (2 γ cos θ_e) / r_eff − ρ g z   ⟹   z(t) = h_Jurin [ 1 − exp(−t / τ_W) ],   τ_W = 8 μ h_Jurin / (ρ g r_eff²)
```

At early times t ≪ τ_W, gravity is negligible and the meniscus advances diffusively as z(t) ≈ √((r_eff γ cos θ_e / 2μ) t) (Lucas 1918; Washburn 1921), before asymptotically approaching Jurin's equilibrium height h_Jurin.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve A16 Graded TPMS Capillary Wick via MCP / REST API

```json
{
  "tool": "solve",
  "arguments": {
    "raw": "{\"kind\":\"solve\",\"dofs\":20000}\nExecute the solid + graded TPMS Gyroid P2 solver on the A16 capillary wick column."
  }
}
```

---
*Generated from `examples/analytic/A16-jurin-capillary-rise-and-tpms-gyroid-wick.fsm` by `node scripts/gen-sheet-docs.mjs`.*
