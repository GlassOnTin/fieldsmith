# T25 — Preloaded Bolted Flange Gasket Contact, Inner-Bore Liftoff & Toe Prying Concentration

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Verified by A21 & V63 · 3D P2 Unilateral Signorini Contact + Semi-Smooth Newton Active Set
> **Repository Path**: `docs/tutorials/T25-preloaded-flange-gasket-liftoff-contact.md` · **Starter Model**: `t25-preloaded-flange-gasket-liftoff-contact.fsm` · **Verification Reports**: `docs/verification/V63.md`

Replace artificial bilateral `fixed=z` flange supports with a true 3D P2 unilateral Signorini contact seat (`contact gasket_seat mating_face k=8000`), compare pure M6 bolt preload clamping against axial pipe blow-off prying, and visualise 3D normal bearing pressure `p_n` and inner-heel liftoff separation.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
name "T25 — Preloaded Al7075 Flange Gasket Contact & Liftoff"
material al7075

let preload_n = 6000
let service_pull_n = 3600
let gasket_k = 8000

box flange_base size=(44, 20, 8) at=(22, 10, 4)
box hub_neck size=(10, 20, 8) at=(5, 10, 12)
cylinder washer_boss r=8.0 h=3.0 at=(22, 10, 9.5)
cylinder bolt_bore r=3.3 h=22.0 at=(22, 10, 6.0) subtract

region x0 = flange_base.nx + hub_neck.nx
region y0 = flange_base.ny + hub_neck.ny
region mating_face = flange_base.nz
region washer_seat = washer_boss.top
region hub_top = hub_neck.pz

support sx x0 fixed=x
support sy y0 fixed=y
contact gasket_seat mating_face k=gasket_k gap=0

load bolt_clamp_1 washer_seat force=(0, 0, -preload_n) case="PreloadOnly"
load bolt_clamp_2 washer_seat force=(0, 0, -preload_n) case="ServicePrying"
load pipe_pull hub_top force=(0, 0, service_pull_n) case="ServicePrying"

solve build=(0, 0)

assert "PreloadOnly bearing force matches bolt clamp preload" metric="contact_force_n:PreloadOnly" target=6000.0 tol="2.5%" unit="N"
assert "PreloadOnly clamps broad gasket footprint" metric="contact_area_ratio:PreloadOnly" target=">= 0.70" tol=">= 0.70" unit="-"
assert "ServicePrying net bearing force equilibrium" metric="contact_force_n:ServicePrying" target=2400.0 tol="2.5%" unit="N"
assert "ServicePrying inner bore heel liftoff separation" metric="max_liftoff_mm:ServicePrying" target=">= 0.015" tol=">= 0.015" unit="mm"
assert "ServicePrying active gasket zone shrinks under prying" metric="contact_area_ratio:ServicePrying" target="<= 0.65" tol="<= 0.65" unit="-"
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect the Unilateral Gasket Contact Seat & Parametric Preload in Setup (2) *(Lens: `SETUP`)*

Open Setup (2). Instead of pinning `mating_face` (`z = 0`) with a bilateral `support ... fixed=z` that unphysically pulls downward under tension, `contact gasket_seat mating_face k=gasket_k gap=0` enforces Signorini unilateral bearing (`p_n ≥ 0` in compression, `p_n = 0` wherever the flange lifts off). Adjust `preload_n` (`6000 N`) or `gasket_k` (`8000 MPa/mm`) under **Parameters**.

- **Expected Check**: Lateral symmetry rollers `sx` (`x = 0`) and `sy` (`y = 0`) remove horizontal rigid-body motion without adding any vertical reaction, so 100% of vertical equilibrium is carried by the unilateral gasket seat.

### 2. Visualise Bearing Pressure Concentration & Inner-Bore Liftoff in Results (3) *(Lens: `RESULTS`)*

Switch to Results (3). Compare **PreloadOnly** (`6,000 N` washer clamp → broad gasket seal, `contact_area_ratio ≥ 0.70`) against **ServicePrying** (`6,000 N` bolt clamp minus `3,600 N` upward pipe tension on `hub_top`). Click **⊥ Contact Pressure p_n** on the **Unilateral Signorini Contact & Flange Liftoff** card to inspect the 3D normal bearing traction map on the bottom face.

- **Expected Check**: Under **ServicePrying**, axial hub tension pulls the inner bore heel upward (`max_liftoff_mm ≥ 0.015 mm`), shrinking the active sealing band (`contact_area_ratio ≤ 0.65`) onto the outer flange rim while maintaining exact `2,400 N` net vertical equilibrium.

## 3. Theoretical Manual — Tutorial T25 — Why Bilateral Supports Over-Predict Bolted-Flange Joint Sealing

Classical linear FEA tutorials often model a mating flange face with a roller support (`u_z = 0`), silently allowing adhesive tensile tractions across the gasket interface. Unilateral Signorini contact (`u·n − g₀ ≤ 0`, `p_n ≥ 0`, `p_n(u·n − g₀) = 0`) captures real gasket separation, active sealing width loss, and outer-toe prying fulcrum shifts.

### T25.1 Bolt Preload Cone vs. Axial Hub Prying Fulcrum

```text
p_n(x) = k_gasket max(0, −u_z(x, y, 0) − g₀),   ∫_{Γ_seat} p_n dA = F_preload − F_service
```

Under pure bolt preload (`PreloadOnly`), the Rotscher compression frustum spreads beneath `washer_boss` across the gasket face. When internal pressure or pipe tension applies upward pull `F_service` at the inner hub (`x ∈ [0, 10] mm`), the flange pivots about the bolt circle and outer toe (`x → 44 mm`), unloading the inner bore heel first — the exact leak-initiation mechanism governed by VDI 2230.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Inspect unilateral contact and parametric preload setup

```json
{
  "tool": "inspect",
  "arguments": {}
}
```

### Solve both PreloadOnly and ServicePrying unilateral contact cases

```json
{
  "tool": "solve",
  "arguments": {
    "dofs": 14000
  }
}
```

### Verify gasket bearing equilibrium, sealing ratio, and inner-heel liftoff contracts

```json
{
  "tool": "verify",
  "arguments": {
    "dofs": 14000
  }
}
```

---
*Generated from `examples/tutorials/T25-preloaded-flange-gasket-liftoff-contact.fsm` by `node scripts/gen-sheet-docs.mjs`.*
