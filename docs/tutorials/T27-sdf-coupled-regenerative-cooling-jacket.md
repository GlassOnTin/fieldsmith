# T27 — 3D Contoured AlSi10Mg Regenerative Cooling Jacket with Section-Coupled Graded Gyroid Core

> **Category**: Guided Tutorial · **Level**: Flagship Multi-Physics · **Verification**: Verified by A23 & V63 · Two-Way 3D SDF + Graded TPMS Sections ↔ 1D Adjoint System ↔ 3D P2 Thermo-Elastic FEA
> **Repository Path**: `docs/tutorials/T27-sdf-coupled-regenerative-cooling-jacket.md` · **Starter Model**: `t27-sdf-coupled-regenerative-cooling-jacket.fsm` · **Verification Reports**: `docs/verification/V20.md`, `docs/verification/V36.md`, `docs/verification/V63.md`

Co-design an annular AlSi10Mg regenerative thruster jacket (`R_bore = 8 mm`, `R_jacket = 16 mm` with mid-span throat stiffening torus and `let`-driven `fill gyroid sheet cell=c_cell`) where `section` declarations extract 3D SDF & TPMS cross-section profiles (`fill_area`, `fill_perm`, `fill_beta`, `fill_awet`, `fill_dh`) directly into a Dual-AD 1D Darcy–Forchheimer + regenerative thermal system and project `lerp(z, ...)` BCs back onto 3D P2 thermo-elastic FEA.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "T27 — 3D SDF & TPMS Section-Coupled Regenerative Cooling Jacket"
material alsi10mg

let r_jacket = 16.0
let r_bore = 8.0
let c_cell = 4.2
let mdot = 0.08

cylinder jacket r=r_jacket h=72 at=(0, 0, 36)
torus throat_rib major=15.5 minor=3.2 at=(0, 0, 36) blend=2.5
cylinder hot_bore r=r_bore h=76 at=(0, 0, 36) subtract

region base_flange = jacket.bottom
region hot_liner = hot_bore.side
region outer_case = jacket.side

support mount_ring base_flange fixed=xyz
load chamber_p hot_liner pressure=3.5 case="ChamberPressure"
heatflux liner_q hot_liner q=1.20 case="ThermalRegen"
convection jacket_sink hot_liner h=0.025 tinf=30.0 case="ThermalRegen"

fill gyroid sheet cell=c_cell cell_min=3.0 skin=1.2 rho=(0.24, 0.50) curve=1.0 wall=0.30

system regen_tpms_1d title="3D SDF & Gyroid Section-Coupled Regenerative Jacket" sector="Propulsion & Thermal-Fluids" nodes=25 span=(2, 70) axis=z {
  section A_bore prop=area mode=void h=0.30
  section P_bore prop=perimeter mode=void h=0.30
  section A_core prop=fill_area mode=solid h=0.30
  section K_perm prop=fill_perm mode=solid h=0.30
  section beta_f prop=fill_beta mode=solid h=0.30
  section a_wet prop=fill_awet mode=solid h=0.30
  section D_cell prop=fill_dh mode=solid h=0.30
  design c_cell nominal=4.2 min=2.8 max=6.0 unit="mm" symbol="a_cell" category=geometry
  design r_jacket nominal=16.0 min=13.5 max=19.5 unit="mm" symbol="R_out" category=geometry
  design mdot nominal=0.08 min=0.03 max=0.20 unit="kg/s" symbol="m_dot" category=process
  epistemic q_mult nominal=1.0 sigma=0.08 min=0.80 max=1.20 unit="-" symbol="k_q"
  state p_c init="250" scale=250 unit="kPa" symbol="p_c"
  state T_c init="22" scale=60 unit="C" symbol="T_c"
  state T_w init="95" scale=120 unit="C" symbol="T_w"
  aux u_D expr="(mdot / 998.0) / (max(A_core, 20.0) * 1e-6)"
  aux dp_ds expr="-((0.0009 * u_D / max(K_perm, 1e-4)) + 1e-3 * beta_f * 998.0 * u_D * u_D)"
  aux Re_p expr="998.0 * u_D * (D_cell * 1e-3) / 0.0009"
  aux Nu_p expr="2.0 + 0.65 * pow(max(Re_p, 1.0), 0.6) * pow(4182.0 * 0.0009 / 0.62, 0.333)"
  aux h_int expr="Nu_p * 0.00062 / max(D_cell, 0.2)"
  aux G_core expr="0.12 * A_core * a_wet * h_int"
  aux G_skin expr="0.130 * max(P_bore, 10.0) / 1.2"
  aux G_eff expr="(G_core * G_skin) / max(G_core + G_skin, 1e-6)"
  aux q_gas expr="q_mult * (0.95 + 0.85 * pow(sin(pi * (s - 2.0) / 68.0), 2.0))"
  aux h_liner_eff expr="G_eff / max(P_bore, 10.0)"
  residual p_c eq="d_ds(p_c) - dp_ds" bc_left="p_c - 250.0"
  residual T_c eq="4182.0 * mdot * d_ds(T_c) - G_eff * (T_w - T_c)" bc_left="T_c - 22.0"
  residual T_w eq="-0.130 * 120.0 * d2_ds2(T_w) + G_eff * (T_w - T_c) - P_bore * q_gas" bc_left="d_ds(T_w)" bc_right="d_ds(T_w)"
  objective dp_jacket expr="250.0 - p_c" reduce=exit kind=upper limit=12.0 unit="kPa" symbol="dp_c"
  objective t_liner_max expr="T_w" reduce=max kind=upper limit=165.0 unit="C" symbol="T_w,max"
  objective t_cool_out expr="T_c" reduce=exit kind=upper limit=65.0 unit="C" symbol="T_c,out"
  bind liner_q attr=q expr="q_gas"
  bind jacket_sink attr=h expr="h_liner_eff"
  bind jacket_sink attr=tinf expr="T_c"
}

solve tref=22 build=(0, 0)

assert "Extracted Gyroid annular core area exceeds 400 mm^2" metric="section.A_core.mean" target=">= 400.0" tol=">= 400.0" unit="mm^2"
assert "Mid-span stiffening rib widens local TPMS core area" metric="section.A_core.max" target=">= 520.0" tol=">= 520.0" unit="mm^2"
assert "Regenerative coolant pressure drop stays below 12 kPa" metric="sys.dp_jacket" target="<= 12.0" tol="<= 12.0" unit="kPa"
assert "Peak liner wall temperature stays below 165 C" metric="sys.t_liner_max" target="<= 165.0" tol="<= 165.0" unit="C"
assert "Coolant exit temperature stays subcooled below 65 C" metric="sys.t_cool_out" target="<= 65.0" tol="<= 65.0" unit="C"
assert "Graded lattice structural yield utilization stays safe" metric="graded.yield_utilization:ChamberPressure" target="<= 0.75" tol="<= 0.75" unit="-"
assert "Graded TPMS jacket saves over 35% mass vs solid shell" metric="mass_saved_pct" target=">= 35.0" tol=">= 35.0" unit="%"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect 3D SDF & Graded-TPMS Cross-Section Profiles in Coupled Systems *(Lens: `SYSTEMS`)*

Open **Coupled Systems**. `system regen_tpms_1d` slices the 3D AlSi10Mg annular jacket (`cylinder jacket` + mid-span `torus throat_rib` minus `cylinder hot_bore`, filled with `fill gyroid sheet cell=c_cell skin=1.2`) across `25` axial stations (`z ∈ [2, 70] mm`), extracting `A_core` (`fill_area`), `K_perm` (`fill_perm`), `beta_f` (`fill_beta`), `a_wet` (`fill_awet`), and `D_cell` (`fill_dh`).

- **Expected Check**: The **Extracted 3D SDF & TPMS Cross-Section Profiles** table shows `A_core(z)` expanding smoothly across the mid-span `torus` stiffening rib (`z = 36 mm`), lowering local Darcy–Forchheimer velocity `u_D(z)` while expanding total wetted Gyroid partition conductance `G_wet(z) = A_core(z) a_wet(z) h_int(z)` right where hot-gas throat heat flux peaks.

### 2. Explore Two-Way `let` Geometry & TPMS Cell Adjoint Levers (`c_cell`, `r_jacket`, `mdot`) *(Lens: `SYSTEMS`)*

Drag the **`c_cell`**, **`r_jacket`**, or **`mdot`** design sliders (or click **Adjoint Step (−dΦ/dp)**) in **Coupled Systems**.

- **Expected Check**: In the **Sparse Adjoint Jacobian** table, decreasing `c_cell` (`4.2 → 3.2 mm`) increases `a_wet` (`∝ 1/c_cell`) to cool `t_liner_max` (`d(T_liner)/d(c_cell) > 0`) at the cost of lower permeability `K_perm` (`∝ c_cell²`, `d(dp_jacket)/d(c_cell) < 0`), while widening `r_jacket` simultaneously lowers `dp_jacket` and `t_liner_max`.

### 3. Sync Projected 1D Profiles & Verify 3D Graded Thermo-Mechanical Margins in Results (3) *(Lens: `RESULTS`)*

Click **Sync Coupled State to 3D Part** (or close Coupled Systems) and open **3. Results**.

- **Expected Check**: The 1D regenerative profiles `q_gas(z)`, `h_eff(z)`, and `T_c(z)` project as `lerp(z, ...)` spatial boundary conditions onto `hot_liner` and `outer_case`, while the stress-graded Gyroid AlSi10Mg core satisfies `yield_utilization ≤ 0.75` under `3.5 MPa` chamber pressure and saves `> 35%` mass relative to a solid shell.

## 3. Theoretical Manual — Tutorial T27 — Closing the Two-Way Loop: 3D SDF + Graded TPMS ↔ 1D Adjoint System ↔ 3D P2 FEA

Demonstrates how declarative `section` bindings eliminate hardcoded analytical duct widths by sampling arbitrary 3D CSG/SDF shapes and `let`-driven TPMS `fill` laws directly inside a Dual-AD 1D conservation system.

### T27.1 Station-Wise 2D SDF + Transverse Skin-Inset TPMS Homogenisation

```text
A_core(s) = ∫_{Ω_2D(s)} H(−d_2D(x_⊥) − t_skin) dA,   −dp_c/ds = (μ / K_perm(s)) u_D(s) + 10⁻³ β_f(s) ρ_f u_D(s)²
m_dot c_p dT_c/ds = G_eff(s) (T_w(s) − T_c(s)),   −k_s A_wall d²T_w/ds² + G_eff(s) (T_w − T_c) = P_bore(s) q_gas(s)
```

At each axial station `s_i`, Fieldsmith extracts exact 2D marching-squares contours of the 3D solid and void regions, evaluates transverse Euclidean distance to enforce the leak-tight `skin = 1.2 mm` wall without false port-endcap collars, and integrates homogenised Kozeny–Carman `K_perm`, Ergun–Forchheimer `β_f`, and minimal-surface `a_wet` over the active lattice core.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve 3D-section-coupled 1D regenerative TPMS system

```json
{
  "tool": "solve_coupled_system",
  "arguments": {}
}
```

### Refine Gyroid unit-cell size c_cell from 4.2 to 3.6 mm

```json
{
  "tool": "set_let",
  "arguments": {
    "name": "c_cell",
    "value": 3.6
  }
}
```

### Re-solve coupled 1D system after 3D TPMS cell edit

```json
{
  "tool": "solve_coupled_system",
  "arguments": {}
}
```

### Solve 3D P2 graded thermo-mechanical FEA and verify all 7 contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 14000
  }
}
```

---
*Generated from `examples/tutorials/T27-sdf-coupled-regenerative-cooling-jacket.fsm` by `node scripts/gen-sheet-docs.mjs`.*
