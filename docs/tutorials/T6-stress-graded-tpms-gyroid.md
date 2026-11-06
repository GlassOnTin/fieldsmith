# T6 — Lightweighting Like Bone: Stress-Graded Internal Gyroid Lattices

> **Category**: Guided Tutorial · **Level**: Advanced · **Verification**: Lesson 6 · Advanced · Graded Gyroid Infill & 3MF
> **Repository Path**: `docs/tutorials/T6-stress-graded-tpms-gyroid.md` · **Starter Model**: `t6-gyroid-graded-bracket.fsm` · **Verification Reports**: `docs/verification/V6.md`, `docs/verification/V7.md`, `docs/verification/V48.md`

Living bone is dense and thick near heavily loaded joints, but porous and lightweight inside. Learn how to fill a part with a smooth Gyroid lattice that automatically thickens where stress is high.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T6 — Stress-Graded Gyroid Bracket"
material petg

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(8, 40, 40) at=(4, 20, 20) blend=8
cylinder hole1 r=3.25 h=20 at=(20, 10, 4) subtract
cylinder hole2 r=3.25 h=20 at=(20, 30, 4) subtract
cylinder hole3 r=4.25 h=20 at=(4, 20, 28) rotate=(0, 90, 0) subtract

region mount = hole1.side + hole2.side
region load_pin = hole3.side
support s1 mount fixed=xyz
load l1 load_pin force=(103.5, 0, -386.4)
fill gyroid cell=6 skin=1.2 skin_max=2.2 rho=(0.2, 0.6) curve=1.5 min_wall=0.8 protect=(hole1, hole2, hole3) margin=2
```

## 2. Step-by-Step Guided Walkthrough

### 1. Look Inside the Part: Smooth Gyroid Lattice & Protected Bolt Collars *(Lens: `FILL`)*

Why print a part 100% solid if the middle carries almost no stress? Tap "Go to Fill →" (which automatically turns on a 50% cutaway section and translucent outer skin so you can see inside). Inside the outer shell is a Gyroid — a smooth, wave-like 3D honeycomb belonging to a family called Triply Periodic Minimal Surfaces (TPMS). Unlike sharp criss-cross struts that concentrate stress at corners, a Gyroid has smooth soap-film curves with zero sharp junctions, and we keep a 2 mm solid protective collar around all three bolt holes ("protect").

- **Expected Check**: In the 3D viewport, the cutaway reveals the wavy internal Gyroid sheet network, the outer skin, and solid collars around the bolt holes.

### 2. How Fieldsmith Grades Density, Cell Size & Skin Thickness by Stress *(Lens: `RESULTS`)*

In the Fill card, notice "ρ min = 0.2" (20% dense in low-stress zones) to "ρ max = 0.6" (60% dense in high-stress zones), and "Skin = 1.2 mm" up to "Max skin = 2.2 mm" (plus optional "Min cell" and "Morph to" controls that can refine unit-cell size or morph between TPMS families in high-stress zones — covered in Lesson T11). Fieldsmith first solves the solid bracket to map where stress is highest, then automatically thickens both the internal Gyroid walls and the outer skin near the high-stress fillet while keeping unloaded regions light. Switch to Results (3) to check the weight savings and verify that the thin lattice walls will not buckle.

- **Expected Check**: The part weighs ~30% less than a solid bracket while passing both material yield and lattice cell-wall buckling checks.

### 3. Inspect 2D Printer Slices & Export a 3MF Manufacturing File in Make (5) *(Lens: `MAKE`)*

Switch to Make (5). Because a fine Gyroid lattice has thousands of curved internal surfaces, exporting an old-fashioned STL triangle file can produce huge files. Fieldsmith can export either a watertight 3D mesh or slice the mathematical distance field directly into exact 2D layer contours inside an industry-standard 3MF (3D Manufacturing Format) file.

- **Expected Check**: Make (5) reports manifold mesh health and lets you scrub through 2D printer layer slices.

## 3. Theoretical Manual — Chapter 6 — Minimal-Surface Lattices (TPMS) & Two-Pass Homogenised Mechanics

Instead of meshing millions of tiny individual lattice walls during stress analysis, Fieldsmith uses homogenisation theory — representing each lattice region by its effective directional stiffness and checking local wall yield and buckling.

### 6.1 The Gyroid Equation & Stress-Driven Density Grading

```text
g_gyroid(u) = sin u_x cos u_y + sin u_y cos u_z + sin u_z cos u_x
ρ(x) = ρ_min + (ρ_max − ρ_min) · (σ_vM(x) / σ_ref)^(1 / curve)
```

A Triply Periodic Minimal Surface (TPMS) is a surface that repeats in X, Y, and Z ("triply periodic") and has zero average curvature everywhere like a soap film ("minimal surface"). By comparing |g_gyroid(u)| against a threshold controlled by the local relative density ρ(x), Fieldsmith smoothly varies wall thickness from thin in lightly loaded zones to thick near fillets and supports.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Switch Internal Lattice to Diamond Sheet via API (`set_fill`)

Update the internal fill pattern from Gyroid to Diamond with stress-thickened skin (1.0 to 2.4 mm).

```json
{
  "tool": "set_fill",
  "arguments": {
    "family": "diamond",
    "mode": "sheet",
    "cell": 6,
    "skin": 1,
    "skin_max": 2.4,
    "rho_min": 0.22,
    "rho_max": 0.65
  }
}
```

### Verify Direct 2D Slicestack Contours via API (`export`)

Slice the graded lattice directly into 2D printer layers and verify zero open contour loops.

```json
{
  "tool": "export",
  "arguments": {
    "h": 0.5,
    "layer_h": 1,
    "slices": true
  }
}
```
