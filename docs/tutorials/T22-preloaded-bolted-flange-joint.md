# T22 — Host-Associative Bolted Flange Joint, VDI 2230 Preload Envelope, & 3D ISO 16047 Scatter UQ

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Verified by V57 & V63 · Unified Rust Fastener Kernel + 3D Preload Adjoint UQ
> **Repository Path**: `docs/tutorials/T22-preloaded-bolted-flange-joint.md` · **Starter Model**: `t22-preloaded-bolted-flange-joint.fsm` · **Verification Reports**: `docs/verification/V57.md`, `docs/verification/V63.md`

Evaluate Motosh + FED-STD-H28 + VDI 2230 fastener mechanics, insert a 4-corner M5 cap-bolt + washer + nyloc-nut pattern onto a 3D Al7075 flange via `DocCommand`, reconfigure hardware in place, and propagate 15% ISO 16047 tightening-torque scatter in O(1) 3D FEA solves.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "Preloaded 4-Bolt Al7075 Cantilever Flange"
material al7075

box flange size=(70, 50, 12) at=(35, 0, 6)

region root = flange.nx
region tip = flange.px

support clamp root fixed=xyz
load tip_shear tip force=(0, 0, -600)

solve build=(0, 0)

assert "Flange 90th-percentile peak von Mises stress bounded" metric="p90.max_vm_elem" target="<= 480.0" tol="<= 480.0" unit="MPa"
assert "Flange 10th-percentile yield safety factor bounded" metric="p10.min_safety_factor" target=">= 1.05" tol=">= 1.05" unit="-"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Open Fixings & Fasteners Catalogue and Inspect VDI 2230 / Motosh Envelope *(Lens: `PART`)*

Select `flange` in the scene tree, open **Fixings & Fasteners**, choose **Hex Cap Bolt (M5)** with **Flat Washer (ISO 7089)** + **Nyloc Nut (ISO 10511)** in **4-Corner Flange** mode, and enable **Emit Preload UQ (CV = 15%, ISO 16047)**. The WASM kernel computes `T_rec`, `F_preload`, `F_sep`, `p_brg`, and `P_f` directly from `fieldsmith-doc::fixings`.

- **Expected Check**: Inserting carves 4 collapsible `<inst>_bore` clearance holes, places 4 hardware groups, binds `hex_cap_bolt_1_preload_n` across all 4 `preload` clamp loads, and appends `uncertainty` on preload scatter.

### 2. Reconfigure Hardware in Inspector & Run O(1)-Solve 3D Preload UQ *(Lens: `RESULTS`)*

Select `hex_cap_bolt_1` to swap nut/washer/grade in **Bolt Hardware & Holdable Forces** (`reconfigure_fixing` via `DocCommand`), then switch to **Results (3)** and click **Compute UQ**.

- **Expected Check**: `results-uq-table` propagates the 15% preload scatter across all 4 bolted seats in ≤ 4 linear FEA solves.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Evaluate M5 Class 10.9 bolt + washer + nyloc envelope & open-standard benchmarks

```json
{
  "tool": "evaluate_fixing",
  "arguments": {
    "input": {
      "fixing_kind": "hex_cap_bolt",
      "nominal_d_mm": 5,
      "pitch_mm": 0.8,
      "engagement_length_mm": 10,
      "substrate_material": "al7075",
      "fastener_material": "steel_10_9",
      "include_washer": true,
      "associated_nut": "nyloc_nut",
      "vibration_g": 12,
      "payload_mass_kg": 1.8
    }
  }
}
```

### Insert 4-corner M5 cap-bolt + washer + nyloc stack with 15% ISO 16047 preload UQ

```json
{
  "tool": "insert_fixing",
  "arguments": {
    "kind": "hex_cap_bolt",
    "size": "M5",
    "engagement_mm": 10,
    "host_id": "flange",
    "host_face": "top",
    "placement_mode": "flange_4_corners",
    "flange_inset_x_mm": 12,
    "flange_inset_y_mm": 10,
    "fastener_material": "steel_10_9",
    "include_washer": true,
    "associated_nut": "nyloc_nut",
    "preload_cv": 0.15,
    "vibration_g": 12,
    "payload_mass_kg": 1.8
  }
}
```

### Reconfigure inserted bolt stack to Ti-6Al-4V + ISO 4032 hex nut

```json
{
  "tool": "reconfigure_fixing",
  "arguments": {
    "selected_node_id": "hex_cap_bolt_1",
    "include_washer": true,
    "associated_nut": "hex_nut",
    "fastener_material": "ti64"
  }
}
```

### Propagate 15% ISO 16047 preload scatter across the 3D bolted flange in O(1) FEA solves

```json
{
  "tool": "uq",
  "arguments": {
    "dofs": 14000
  }
}
```

### Verify 3D flange stress and yield safety contracts under bolt preload + tip shear

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 14000
  }
}
```

---
*Generated from `examples/tutorials/T22-preloaded-bolted-flange-joint.fsm` by `node scripts/gen-sheet-docs.mjs`.*
