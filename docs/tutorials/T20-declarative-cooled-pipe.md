# T20 — Declarative 1D Coolant Channel + 3D Staggered Thermo-Elastic Adjoint Co-Design

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Verified by V63 · Dual-AD 1D System + 3D Staggered Thermo-Elastic Adjoints
> **Repository Path**: `docs/tutorials/T20-declarative-cooled-pipe.md` · **Starter Model**: `t20-declarative-cooled-pipe.fsm` · **Verification Reports**: `docs/verification/V63.md`

Author a composable 1D thermal-hydraulic `system` block directly inside `.fsm`, project station-resolved `lerp(z, ...)` film and pressure profiles onto a 3D SS316L cooling liner, verify engineering `assert` contracts, and differentiate 3D peak temperature and von Mises stress in O(1) staggered adjoint solves.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "Declarative Cooled SS316L Liner"
material ss316l

let mdot = 0.012
let wall_t = 3

cylinder outer_pipe r=6 + wall_t h=60 at=(0, 0, 30)
cylinder inner_bore r=6 h=64 at=(0, 0, 30) subtract

region bore_wall = inner_bore.side
region outer_wall = outer_pipe.side
region pipe_base = outer_pipe.bottom

support base_fix pipe_base fixed=xyz
load bore_press bore_wall pressure=0.8 case="Operating"
convection bore_conv bore_wall h=0.025 tinf=25 case="Thermal"
heatflux outer_flux outer_wall q=0.12 case="Thermal"

system coolant_liner_1d title="1D Cooled SS316L Liner" sector="Thermal-Fluids" nodes=25 span=(0, 60) axis=z {
  design mdot nominal=0.012 min=0.004 max=0.060 unit="kg/s" symbol="m_dot" category=process
  design wall_t nominal=3 min=1.5 max=5.0 unit="mm" symbol="t_w" category=geometry
  epistemic h_mult nominal=1.0 sigma=0.08 min=0.75 max=1.25 unit="-" symbol="k_h"
  state T_f init="25" scale=50 unit="C" symbol="T_f"
  state P_f init="0.8" scale=1 unit="MPa" symbol="P_f"
  aux cp expr="4180"
  aux r_out expr="6 + wall_t"
  aux q_lin expr="2 * pi * r_out * 0.12 * (1 + 0.2 * sin(pi * s / 60))"
  aux h_film expr="h_mult * 0.025 * pow(mdot / 0.012, 0.8)"
  residual T_f eq="mdot * cp * d_ds(T_f) - q_lin" bc_left="T_f - 25"
  residual P_f eq="d_ds(P_f) + 0.0025 * pow(mdot / 0.012, 1.8)" bc_left="P_f - 0.8"
  objective exit_temp expr="T_f" reduce=exit kind=upper limit=65 unit="C" symbol="T_out"
  objective mdot_min expr="mdot" reduce=value kind=minimize unit="kg/s" symbol="m_dot"
  audit enthalpy inflow="mdot * cp * entry(T_f)" outflow="mdot * cp * exit(T_f)" source="integral(q_lin)" tol=0.02 unit="W"
  bind bore_conv attr=tinf expr="T_f"
  bind bore_conv attr=h expr="h_film"
  bind bore_press attr=pressure expr="P_f"
}

solve size=4.5 tref=20 build=(0, 0)

assert "Peak 3D liner temperature bounded" metric="max_temp" target="<= 95.0" tol="<= 95.0" unit="C"
assert "Peak 3D thermo-elastic von Mises stress bounded" metric="max_vm_elem" target="<= 220.0" tol="<= 220.0" unit="MPa"
assert "Structural safety factor above 1.0" metric="min_safety_factor" target=">= 1.0" tol=">= 1.0" unit="-"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the 1D Declarative Coolant System in Coupled Systems *(Lens: `SYSTEMS`)*

Open Coupled Systems to inspect `coolant_liner_1d`: an axial 25-station plug-flow energy and pressure-drop balance compiled via forward-mode Dual-Number AD. Drag `mdot` or edit the `.fsm` `system` equations directly to project updated `lerp(z, ...)` Robin and pressure profiles onto the 3D bore wall (`⌀ 12 mm` ID).

- **Expected Check**: Conservation audit closes within tolerance and `bore_conv` / `bore_press` receive station-resolved `lerp(z, ...)` fields.

### 2. Run 3D Thermo-Elastic Solve, Contract Verification & Adjoint Sensitivities in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). Standalone 3D solves, contract verifications, and sensitivity runs automatically solve the embedded `system` equilibrium first. Click **Verify Asserts** and **Compute ∂Φ/∂p** to inspect `∂T_max/∂p` and `∂σ_vM/∂p` across `mdot` and `wall_t` in O(1) staggered adjoint solves.

- **Expected Check**: All 3 contracts pass and `∂T_max/∂mdot < 0` confirms higher coolant flow lowers peak 3D wall temperature.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Solve 1D coolant channel and sync 3D lerp BCs

```json
{
  "tool": "solve_coupled_system",
  "arguments": {
    "sync_cad": true
  }
}
```

### Verify initial 3D thermo-elastic contracts

```json
{
  "tool": "verify",
  "arguments": {
    "h": 4.5
  }
}
```

### Compute 3D staggered thermo-elastic adjoints wrt mdot and wall_t

```json
{
  "tool": "sensitivity",
  "arguments": {
    "params": [
      "mdot",
      "wall_t"
    ],
    "h": 4.5
  }
}
```

### Add temporary mass budget contract

```json
{
  "tool": "upsert_assert",
  "arguments": {
    "label": "Mass budget",
    "metric": "mass_g",
    "target": "<= 100",
    "unit": "g"
  }
}
```

### Re-verify contracts with mass budget

```json
{
  "tool": "verify",
  "arguments": {
    "h": 4.5
  }
}
```

### Remove temporary mass budget contract

```json
{
  "tool": "delete_assert",
  "arguments": {
    "label": "Mass budget"
  }
}
```

### Co-optimise 1D coolant flow and 3D wall thickness for minimum mass

```json
{
  "tool": "optimise",
  "arguments": {
    "objective": "mass_g",
    "params": [
      "mdot",
      "wall_t"
    ],
    "max_iters": 5,
    "h": 4.5
  }
}
```

### Re-verify contracts on optimised 1D+3D liner

```json
{
  "tool": "verify",
  "arguments": {
    "h": 4.5
  }
}
```

---
*Generated from `examples/tutorials/T20-declarative-cooled-pipe.fsm` by `node scripts/gen-sheet-docs.mjs`.*
