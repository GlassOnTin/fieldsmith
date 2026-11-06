# T5 — 3D Printing Strength: Preventing Layer Splitting & Choosing Print Orientation

> **Category**: Guided Tutorial · **Level**: Intermediate · **Verification**: Lesson 5 · Intermediate · 3D Printing & Layer Peel
> **Repository Path**: `docs/tutorials/T5-additive-layer-peel-orientation.md` · **Starter Model**: `t5-additive-peel-arm.fsm` · **Verification Reports**: `docs/verification/V45.md`, `docs/verification/V46.md`

Desktop 3D printers stack melted plastic layer by layer. Like wood grain, bonds between layers are weaker than along the strands — learn how to detect layer splitting and rotate the part.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T5 — Vertical FDM Cantilever (Layer Peel)"
material petg

let h_root = 12
box beam size=(60, 16, h_root) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load tip_load tip force=(0, 0, -450) case="Peak"
solve size=3.5 build=(90, 0)
```

## 2. Step-by-Step Guided Walkthrough

### 1. Why Print Orientation Matters (Wood-Grain Analogy) *(Lens: `RESULTS`)*

When a filament 3D printer — called Fused Deposition Modelling (FDM) — prints a part, it lays down continuous plastic strands in horizontal XY layers stacked along the vertical build direction. Just like wood splits easily along its grain, a 3D-printed part is weaker when pulled apart across its layer seams ("Layer Peel") than when pulled along the continuous strands. Tap "Go to Results →" and tap the "Peel" button at the top of the Results card. Notice the white slice lines drawn on the 3D beam: because this part was oriented upright at (90°, 0°), the layer seams run vertically right across the beam, and bending pulls those seams apart!

- **Expected Check**: White slice lines on the 3D beam show the layer seams; the "Layer peel" row reports the current peel utilization and the best print orientation.

### 2. Rotate to the Best Print Orientation in One Click *(Lens: `RESULTS`)*

Fieldsmith automatically checks candidate print angles across the sphere to find the orientation where bending tension runs along the strong filament strands instead of pulling the layer seams apart. Tap the "Apply best" button right next to the "Layer peel" readout (or switch to tab 5, "Make", to inspect the build plate orientation and steep downward overhangs that would need support material).

- **Expected Check**: Tapping "Apply best" rotates the print orientation so the white layer lines align with the length of the beam, reducing layer-splitting risk.

## 3. Theoretical Manual — Chapter 5 — Anisotropic Layer-Bond Strength & Overhang Angles

Metals cast from a mould are isotropic (equally strong in every direction), whereas layer-by-layer 3D prints are orthotropic / transversely isotropic: inter-layer tensile strength S_z is significantly lower than in-plane yield strength σ_Y.

### 5.1 Calculating Inter-Layer Peel & Shear Traction Across Layer Seams

```text
t = σ · b,   t_normal = b · t,   τ_shear = ||t − t_normal b||
U_peel(b) = √( (max(0, t_normal) / S_z)² + (τ_shear / S_shear)² )
```

If the printer stacks layers along unit vector b(θ, φ), then t = σ · b is the force-per-unit-area acting directly on the flat interface between two adjacent layers. Only positive tension (t_normal > 0) tries to peel the layers apart (compression pushes them together), while τ_shear tries to slide one layer across the next. A part is safe against layer splitting when U_peel < 1.0 everywhere.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Set Print Build Angle to Flat (0°, 0°) via API (`set_build`)

Align the 3D-printed layers horizontally along the beam length so continuous filament strands carry the bending tension.

```json
{
  "tool": "set_build",
  "arguments": {
    "theta": 0,
    "phi": 0
  }
}
```

### Check Downward Overhang Area (> 45°) via API (`solve`)

Measure how many square millimetres of downward-facing surface tilt past 45° and would need print supports.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "overhang",
    "build": [
      0,
      0,
      1
    ],
    "threshold_deg": 45
  }
}
```
