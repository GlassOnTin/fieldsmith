# T17 — Open-Hardware Regeneratively Cooled Liquid Rocket Chamber: GOX/Ethanol De Laval Contour & Stress-Graded TPMS Gyroid Jacket

> **Category**: Multiphysics Case Study · **Level**: Flagship Multi-Physics · **Verification**: Verified by V60 (De Laval Pappus Volume + Bicylinder Lamé + Robin–Neumann Regenerative Cooling + Tubular Modal)
> **Repository Path**: `docs/case-studies/T17-regenerative-rocket-nozzle.md` · **Starter Model**: `t17-gox-ethanol-regen-rocket.fsm` · **Verification Reports**: `docs/verification/V60.md`, `docs/verification/V20.md`

Model the open-source gNSortino/OSREngines 2014-GOXEthanolRegenEngine from its exact RPA contour (Ø 36.05 mm bore, Ø 14.72 mm throat, Ø 26.86 mm exit), apply 3.0 MPa chamber pressure + Bartz hot-gas heat flux + ethanol coolant convection, and replace solid walls with a stress-graded TPMS Gyroid cooling jacket.

## Closed-Form Analytic Reference vs Fieldsmith P2 FEA

| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |
| :--- | :--- | ---: | ---: | ---: | :--- |
| Solid Chamber Mass -> TPMS Gyroid Jacket Mass (Ti-6Al-4V) | `V_SDF = ∫_Ω H(-d(x)) dV,  m = ρ_eff · V` | **655.5 g (Solid Wall)** | **426.5 g (Graded Gyroid)** | -34.9% Mass Saved | Watertight 1.25 mm Skin |
| Case 1: 3.0 MPa Chamber Pressure Peak von Mises | `σ_θθ = p_i (r_o² + r_i²) / (r_o² - r_i²)` | **13.52 MPa (Solid Wall)** | **18.59 MPa (Graded Gyroid)** | SF = 51.4 (σ_y = 955 MPa) | SF ≥ 4.0 |
| Case 2: Bartz Hot-Fire Peak Thermal von Mises & Layer Peel | `-k ∇²T = 0,  q_throat = 11.0,  h_cool = 0.018` | **278.92 MPa (Peel 0.19×)** | **265.94 MPa (Peel 0.13×)** | -4.7% Stress / -31.6% Peel | SF = 3.61, Peel < 1.0× |

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T17 — GOX/Ethanol Regenerative Rocket Chamber (Ti-6Al-4V TPMS Gyroid)"
material ti64

revolve outer_body profile=[(0, 0), (30, 0), (30, 12), (24.5, 12), (24.5, 92), (14.5, 114.62), (20.5, 134.81), (0, 134.81)]
revolve hot_gas_bore profile=[(0, -1), (18.024, -1), (18.024, 92.59), (7.36, 114.62), (13.43, 134.81), (13.43, 136), (0, 136)] subtract
cylinder chamber_tag r=18.06 h=88 at=(0, 0, 45) subtract
cylinder throat_tag r=7.46 h=14 at=(0, 0, 114.62) subtract
cylinder bolt_1 r=2.6 h=16 at=(26, 0, 6) subtract
cylinder bolt_2 r=2.6 h=16 at=(-26, 0, 6) subtract
cylinder bolt_3 r=2.6 h=16 at=(0, 26, 6) subtract
cylinder bolt_4 r=2.6 h=16 at=(0, -26, 6) subtract

region injector_bolts = bolt_1.side + bolt_2.side + bolt_3.side + bolt_4.side
region chamber_wall = chamber_tag.side
region throat_wall = throat_tag.side
region jacket_outer = outer_body.side

support head_mount injector_bolts fixed=xyz
load chamber_p chamber_wall pressure=3.0 case="1: Chamber Pressure (3.0 MPa)"
load throat_p throat_wall pressure=2.4 case="1: Chamber Pressure (3.0 MPa)"
heatflux chamber_q chamber_wall q=4.5 case="2: Bartz Regenerative Hot-Fire"
heatflux throat_q throat_wall q=11.0 case="2: Bartz Regenerative Hot-Fire"
convection ethanol_cool jacket_outer h=0.018 tinf=40 case="2: Bartz Regenerative Hot-Fire"
fill gyroid cell=6.0 skin=1.25 rho=(0.22, 0.75) curve=1.0 wall=0.6
build orient=(0, 0)
solve tref=25
```

## 2. Step-by-Step Guided Walkthrough

### 1. Exact De Laval Chamber & Nozzle Contour from Rocket Propulsion Analysis (RPA) *(Lens: `SHAPE`)*

Open Shape (1) to inspect the axisymmetric converging-diverging De Laval chamber built directly from the gNSortino/OSREngines 2014-GOXEthanolRegenEngine.RPA.cfg specification: Ø 36.05 mm combustion chamber bore (r = 18.024 mm), Ø 14.72 mm sonic throat (r = 7.36 mm at z = 114.62 mm), and Ø 26.86 mm nozzle exit (r = 13.43 mm at z = 134.81 mm). Unlike the repository’s exported STL mesh (which contained 1,088 open non-manifold edges inside its 0.708 mm helical slots), the native revolve + SDF formulation has zero open edges.

- **Expected Check**: The 3D viewport shows the complete thrust chamber with injector flange bolt holes (bolt_1..bolt_4) and coaxial chamber/throat bore regions.

### 2. Coupled Chamber Pressure + Bartz Throat Heat Flux + Ethanol Regenerative Convection *(Lens: `SETUP`)*

Switch to Setup (2). Two multiphysics cases are configured: Case 1 applies 3.0 MPa chamber pressure and 2.4 MPa sonic throat pressure while clamping the 4 × M5 injector flange bolts. Case 2 applies Bartz convective hot-gas heat flux on the chamber bore (q = 4.5) and sonic throat (q = 11.0, where boundary-layer thinning peaks) balanced by forced ethanol coolant convection (h = 0.018, T_inf = 40 °C) on the outer jacket.

- **Expected Check**: Setup tree lists Support head_mount, Pressure loads chamber_p (3.0 MPa) and throat_p (2.4 MPa), Heat Fluxes chamber_q and throat_q, and Convection ethanol_cool.

### 3. Compare Solid-Wall vs Stress-Graded TPMS Gyroid Regenerative Cooling Jacket *(Lens: `FILL`)*

Switch to Fill (4) and Results (3). Replacing the solid outer wall with a stress-graded TPMS Gyroid Sheet infill (cell = 6.0 mm, watertight skin = 1.25 mm, density ρ graded from 0.22 in low-stress zones up to 0.75 at the sonic throat and injector flange) cuts chamber mass by 34.9% (655.5 g → 426.5 g in Ti-6Al-4V; 1,324.4 g → 861.8 g in CuCrZr). Because the compliant cellular jacket accommodates radial thermal growth of the hot inner liner, peak hot-fire thermal stress drops from 278.92 MPa to 265.94 MPa (SF = 3.61) and LPBF inter-layer peel drops by 31.6% (0.19× → 0.13×).

- **Expected Check**: Section-clipping the chamber in Fill (4) reveals the continuous self-supporting Gyroid cooling network bounded by 1.25 mm inner hot-gas and outer manifold skins.

## 3. Theoretical Manual — Chapter 17 — Regeneratively Cooled De Laval Rocket Chambers: Bartz Throat Heat Flux, Robin–Neumann Conduction & TPMS Cooling Jackets

Combines axisymmetric De Laval nozzle gas-dynamic pressure and Bartz throat heat-transfer scaling with cylindrical Robin–Neumann thermo-elasticity and Gibson–Ashby homogenised TPMS Gyroid lattice mechanics.

### 17.1 De Laval Conical Frusta Volume & Bartz Sonic Throat Heat-Flux Peak

```text
V = Σ (π h_k / 3) · [(R_o1² + R_o1 R_o2 + R_o2²) − (R_i1² + R_i1 R_i2 + R_i2²)]
q_Bartz(z) ∝ (D* / D(z))^1.8 · p_c^0.8
```

In a converging-diverging rocket chamber, the sonic throat (M = 1, minimum diameter D*) experiences the highest mass flux ρu and thinnest turbulent thermal boundary layer, driving peak inward heat flux q_throat ≈ 2.4× the cylindrical chamber wall flux.

### 17.2 Thermal Compliance of a Stress-Graded TPMS Gyroid Jacket

```text
E_eff(ρ) = E_s · (C₁ ρ² + C₂ ρ),     σ_zz,liner ≈ −E_liner α ΔT / (1 + A_liner E_liner / (A_jacket E_eff(ρ)))
```

In a conventional thick-walled monolithic chamber, the cold outer wall rigidly restrains the hot inner liner, generating compressive thermal stress σ ≈ −EαΔT/(1−ν) that exceeds yield. Grading the regenerative jacket with a TPMS Gyroid Sheet (ρ ∈ [0.22, 0.75]) provides continuous tortuous coolant wet area while lowering the jacket’s radial/axial stiffness E_eff(ρ), relieving liner thermal stress and LPBF inter-layer peel.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve Both Mechanical & Bartz Hot-Fire Cases via API (`solve_study`)

Run the two-pass homogenised P2 thermo-structural solver on the Ti-6Al-4V TPMS Gyroid rocket chamber.

```json
{
  "tool": "solve_study",
  "arguments": {
    "include_fine": false
  }
}
```

### Switch Chamber Alloy to CuCrZr Copper via API (`set_material`)

Switch the chamber material from Ti-6Al-4V to CuCrZr (E = 128 GPa, α = 1.7e-5 /K) to compare copper vs titanium thermal stress.

```json
{
  "tool": "set_material",
  "arguments": {
    "material": "cucrzr"
  }
}
```
