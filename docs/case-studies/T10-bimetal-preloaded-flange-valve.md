# T10 — Multi-Material Bolted Assembly & Bolt Preload — V53

> **Category**: Multiphysics Case Study · **Level**: Flagship Multi-Physics · **Verification**: V53 · Multi-Body Assembly · Per-Volume Material & Bolt Preload
> **Repository Path**: `docs/case-studies/T10-bimetal-preloaded-flange-valve.md` · **Starter Model**: `T10-multibody-bolted-flange-preload.fsm` · **Verification Reports**: `docs/verification/V12.md`, `docs/verification/V53.md`

Assemble a CuCrZr copper-alloy injector spool bolted to a 316L stainless-steel housing flange, apply axial bolt preload clamping, and evaluate cold clamping vs hot bi-metallic differential expansion.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T10 — Multi-Material Bolted Injector Flange (CuCrZr + SS316L)"
material ss316l

let bolt_preload = 16000
let hot_wall_temp = 340

cylinder lower_housing r=26 h=10 at=(0, 0, -5) blend=1
color "#475569" on lower_housing
cylinder upper_spool r=26 h=10 at=(0, 0, 5) blend=1
material cucrzr on upper_spool
color "#d97706" on upper_spool
cylinder boss_neck r=15 h=12 at=(0, 0, 14) blend=2
material cucrzr on boss_neck
color "#d97706" on boss_neck
cylinder central_bore r=9 h=36 at=(0, 0, 4) subtract
cylinder bolt_pocket_ring r=20 h=3 at=(0, 0, 9.5) subtract
cylinder bolt_relief_ring r=17 h=3 at=(0, 0, 9.5) add
material cucrzr on bolt_relief_ring

region base_mount = lower_housing.bottom
region washer_seat = upper_spool.top
region inner_bore = central_bore.side
region outer_rim = lower_housing.side

support mount_fix base_mount fixed=xyz
load m8_preload washer_seat preload=bolt_preload case="1. Cold Bolt Preload + Bore Pressure"
load chamber_p inner_bore pressure=8.5 case="1. Cold Bolt Preload + Bore Pressure"
temperature hot_bore inner_bore temp=0 + hot_wall_temp case="2. Hot Bi-Metallic Thermal Mismatch"
temperature cool_rim outer_rim temp=45 case="2. Hot Bi-Metallic Thermal Mismatch"
solve size=3.5 tref=20
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspecting a Multi-Material Assembly: CuCrZr Spool on an SS316L Flange *(Lens: `SHAPE`)*

High-heat-flux propulsion and power hardware frequently joins a high-conductivity copper alloy ("CuCrZr", E = 128 GPa, α = 17.0×10⁻⁶ /K, bronze top half) to a structural stainless-steel housing ("SS316L", E = 193 GPa, α = 16.0×10⁻⁶ /K, slate bottom half). In the Model (1) feature tree on the left, click "upper_spool" to open its Node Inspector: notice the "Volume material" dropdown is set to "CuCrZr copper alloy (LPBF)", overriding the document’s default "316L stainless (LPBF)". Each tetrahedron in the 3D mesh automatically inherits the Young’s modulus E_e, thermal expansion α_e, and density ρ_e of the volume containing its centroid.

- **Expected Check**: Selecting "upper_spool" or "boss_neck" in the Model tree displays "CuCrZr copper alloy (LPBF)" in the Volume material selector (`[data-testid="node-material"]`), while "lower_housing" uses the document default "316L stainless (LPBF)".

### 2. Configuring Axial Bolt Preload Clamping in Setup (2) *(Lens: `SETUP`)*

Before a pressurized joint sees internal pressure, its bolts are torqued to clamp the mating flanges together. Tap "Go to Setup →" (tab 2) and click the load "m8_preload" under Loads. Notice its Type is set to "Preload" with "Preload = 16000 N" on region "washer_seat". A Preload load applies a self-equilibrating clamping traction (16 kN total axial clamping force) distributed across the washer bearing face (or opposing pinch traction across a split bolt bore when a Clamp axis is set). You can edit the Preload (N) value directly in the Inspector to test tighter or looser bolt torque.

- **Expected Check**: Clicking "m8_preload" in Setup opens the Load Inspector with Type = "Preload" (`[data-testid="preload"]`) set to 16,000 N alongside the 8.5 MPa internal bore pressure load.

### 3. Solving & Comparing Cold Bolt Preload vs Hot Bi-Metallic Expansion in Results (3) *(Lens: `RESULTS`)*

Tap "Go to Results →" (tab 3) and click "Solve" if results are not yet shown. First inspect Case "1. Cold Bolt Preload + Bore Pressure" to verify how the 16 kN bolt preload compresses the upper CuCrZr flange down onto the SS316L base while resisting the 8.5 MPa bore pressure. Next, switch the Case selector to "2. Hot Bi-Metallic Thermal Mismatch", toggle between "Temperature" and "Stress", switch the Colourmap (`[data-testid="results-colormap"]`) to "Flame" or "Coolwarm", toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) to inspect internal interface stress iso-shells, and use the Transient Evolution & Video Recap bar (`[data-testid="results-transient-bar"]`) to step through the 4 milestone recap cards or click "🎬 Render Video" (`[data-testid="results-render-video"]`) to record a real-time `.webm` movie.

- **Expected Check**: Both load cases solve on the multi-material P2 tetrahedral mesh, reporting the combined CuCrZr + SS316L assembly mass, cold clamping stress/deflection, hot bi-metallic thermal stress, 3D volume iso-shells, and `.webm` video playback.

## 3. Theoretical Manual — Chapter 10 — Multi-Body Assembly Mechanics, Per-Element Thermo-Elasticity & Bolt Preload Clamping (V53)

How Fieldsmith assigns per-volume constitutive properties (E_e, α_e, ρ_e) across multi-body CSG assemblies and models axial bolt preload clamping alongside bi-metallic thermal mismatch.

### 10.1 Centroid-Classified Per-Element Constitutive Assembly

```text
K = ∑_e ∫_(Ω_e) B^T D(E_e, ν) B dΩ,   M_total = ∑_e V_e · ρ_e
f_th = ∑_e ∫_(Ω_e) B^T D(E_e, ν) · [α_e · (T(x) − T_ref) · I] dΩ
```

When additive primitives carry distinct `material <id> on <node>` overrides, Fieldsmith evaluates each non-suppressed additive volume’s signed distance field φ_i(c_e) at every P2 tetrahedron centroid c_e. Following CSG last-writer-wins semantics, element e receives the Young’s modulus E_e, thermal expansion coefficient α_e, and mass density ρ_e of the last volume containing c_e. Even under uniform temperature change ΔT, a jump in α_e across an internal material interface produces an unbalanced thermal strain jump Δα · ΔT that drives realistic bi-metallic interface stress.

### 10.2 Bolt Preload Clamping Traction & Joint Separation Margin

```text
Single Washer Face: t(x) = −(F_preload / A_washer) · n(x)
Opposed Bore Pinch (axis â): t(x) = ∓(F_preload / A_±) · â   for   (x − x̄)·â ≷ 0
```

Torquing a fastener stretches the bolt shank to tension F_preload while clamping the flange washer seat with an equal and opposite compressive traction. In Fieldsmith, `load <id> <region> preload=<N>` integrates the region’s surface area and applies an inward normal clamping traction summing to F_preload Newtons (or, when `axis=(ax, ay, az)` is provided on a shared bore/collar region, splits the region across its axial centroid into opposing ±F_preload pinch tractions that squeeze the upper and lower flanges together).

## 4. Reproducible Automation (MCP / REST API & CLI)

### Assign CuCrZr Alloy to Upper Spool Volume via API (`set_material`)

Set per-volume material cucrzr on the upper_spool node while keeping the lower housing on ss316l.

```json
{
  "tool": "set_material",
  "arguments": {
    "node": "upper_spool",
    "material": "cucrzr"
  }
}
```

### Increase Bolt Preload Clamping Force to 22 kN via API (`set_param`)

Update the bolt_preload parameter from 16,000 N to 22,000 N to increase flange clamping compression.

```json
{
  "tool": "set_param",
  "arguments": {
    "addr": "bolt_preload",
    "value": 22000
  }
}
```

### Run Multi-Material Assembly & Bolt Preload FEA Solve via API (`solve_study`)

Solve both the cold bolt-preload case and the hot bi-metallic thermal mismatch case.

```json
{
  "tool": "solve_study",
  "arguments": {
    "include_fine": false
  }
}
```
