# T23 — Host-Stamped DC04 Clinching S-Lock Co-Design, Optimal X* Dial-In, & 45° Mixed-Peel J2 FEA

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Verified by A19 & V63 · Unified Rust Clinching Kernel + 3D J2 Secant FEA
> **Repository Path**: `docs/tutorials/T23-clinching-process-window-and-slock-codesign.md` · **Starter Model**: `t23-clinching-process-window-and-slock-codesign.fsm` · **Verification Reports**: `docs/verification/V53.md`, `docs/verification/V63.md`

Stamp a parametric 3D S-Lock clinched joint onto a DC04 mild-steel lap coupon via `DocCommand`, evaluate the 51-point Lee–Kim / Breda hat-curve process window, dial residual coin thickness to optimal `X*`, rotate the service load to 45° mixed peel/shear, and verify 3D J2 secant elasto-plastic contracts.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "DC04 Lap Sheet for S-Lock Clinching Co-Design"
material mat_dc04

box sheet size=(40, 26, 3.0) at=(0, 0, 1.5)

region clamp_edges = sheet.nx + sheet.px
support fixture clamp_edges fixed=xyz

solve build=(0, 0)

assert "DC04 clinch neck J2 von Mises flow stress active" metric="max_vm_elem" target=">= 20.0" tol=">= 20.0" unit="MPa"
assert "DC04 clinch coupon mixed-peel deflection bounded" metric="max_displacement" target="<= 2.5" tol="<= 2.5" unit="mm"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Stamp 3D S-Lock Joint & Inspect Hat-Curve Process Window *(Lens: `RESULTS`)*

Starting from the clamped DC04 lap stack (`sheet`), `apply_clinching` stamps `clinch_slock` (`clinch_die_button` + `clinch_upper_boss`) and subtractive `clinch_recess` (`clinch_die_ring_groove` + `clinch_punch_pit` with `height = 3.8 - X`), binds `clinch_pull` on `clinch_pull_ring`, and enables `solve plasticity=j2`.

- **Expected Check**: In **Results (3)**, the **Sheet-Metal Clinching Explorer** plots `F_neck(X)`, `F_pull(X)`, and `X* = 0.78 mm` directly from `fieldsmith-doc::clinching` via WASM.

### 2. Dial Optimal X* & Test 45° Mixed Peel in 3D J2 Secant FEA *(Lens: `RESULTS`)*

Click **`✨ Dial X*`** (`dial_optimal_x: true`) and **`45° Mixed Peel`** (`load_angle_deg: 45`). Both the browser UI and MCP `apply_clinching` dispatch `DocCommand::ApplyClinching`, updating `let X`, recomputing `clinch_punch_pit.height`, and rotating the 3D force vector to `(F sin 45°, 0, F cos 45°)`.

- **Expected Check**: Both `.fsm` `assert` contracts pass under 3D P2 J2 secant elasto-plasticity, and `Undo` restores the prior coin thickness and load angle in one step.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Stamp under-clinched 3D S-Lock joint (X = 1.20 mm) onto DC04 lap coupon with 3D J2 plasticity

```json
{
  "tool": "apply_clinching",
  "arguments": {
    "insert_joint": true,
    "host_id": "sheet",
    "bottom_thickness_x_mm": 1.2,
    "service_load_n": 2200,
    "j2_plasticity": true,
    "bottom_x_std_mm": 0.04
  }
}
```

### Evaluate 51-point Lee-Kim / Breda hat-curve process window and springback-adjusted interlock

```json
{
  "tool": "evaluate_clinching",
  "arguments": {}
}
```

### Dial coin thickness automatically to optimal X* and rotate load to 45 deg mixed peel

```json
{
  "tool": "apply_clinching",
  "arguments": {
    "dial_optimal_x": true,
    "load_angle_deg": 45
  }
}
```

### Verify 3D J2 secant stress and deflection contracts on dialed DC04 S-Lock joint

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 12000
  }
}
```

---
*Generated from `examples/tutorials/T23-clinching-process-window-and-slock-codesign.fsm` by `node scripts/gen-sheet-docs.mjs`.*
