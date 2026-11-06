# T12 — Topology Optimization & Level-Set Boundary Evolution — V55

> **Category**: Multiphysics Case Study · **Level**: Flagship Multi-Physics · **Verification**: V55 · Level-Set Topology Evolution & Conformal Infill
> **Repository Path**: `docs/case-studies/T12-levelset-evolver-aerospace-bracket.md` · **Starter Model**: `T12-levelset-topology-bracket.fsm` · **Verification Reports**: `docs/verification/V50.md`, `docs/verification/V55.md`

Evolve the outer boundary level-set inward in low-stress zones (erode=3 mm) while preserving protected bosses, primary load ribs, conformal skin, and multi-zone TPMS infill.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T12 — Level-Set Topology-Evolved Actuator Bracket"
material ti64

box design_envelope size=(64, 22, 20) round=2 at=(32, 0, 0)
cylinder mount_boss r=8 h=20 at=(8, 0, 0) blend=2
cylinder mount_bore r=3.5 h=26 at=(8, 0, 0) subtract
cylinder tip_clevis r=8 h=20 at=(56, 0, 0) blend=2
cylinder tip_pin_bore r=3.5 h=26 at=(56, 0, 0) subtract

region mount_wall = mount_bore.side
region clevis_pin = tip_pin_bore.side

support clamp_mount mount_wall fixed=xyz
load actuator_shear clevis_pin force=(200, 0, -1400) case="1. Peak Actuator Shear"
load side_gust clevis_pin force=(0, 550, -600) case="2. Lateral Gust + Shear"

fill gyroid sheet cell=8 cell_min=4 morph=diamond skin=1.2 skin_max=2.0 erode=3.0 rho=(0.20, 0.65) curve=1 wall=0.6 protect=(mount_boss, tip_clevis, mount_bore, tip_pin_bore) margin=2.0
solve
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspecting Level-Set Outer Boundary Evolution in Fill (4) *(Lens: `FILL`)*

Classical lattice infill hollows the inside of a part while keeping the outer CAD box untouched; classical SIMP topology optimization carves the outer shape into jagged voxels without a conformal skin or internal lattice. Fieldsmith unifies both on one implicit field grid via "Erode" (`erode=<mm>`). Tap "Go to Fill →" (tab 4): notice that in low-stress zones (`t → 0`), the outer boundary level-set shifts inward by up to `Erode = 3 mm`, while inside the protected mounting bosses (`mount_boss`, `tip_clevis`) and along peak load paths (`t → 1`), erosion is suppressed to zero so the bosses stand proud at full height with a conformal outer skin (`1.2 → 2.0 mm`) and graded Gyroid → Diamond core.

- **Expected Check**: In Fill (4), Erode (`[data-testid="fill-erode"]`) is set to 3 mm, Morph to is "Diamond", Cell is 8 mm, Min cell is 4 mm, and both mounting bosses remain un-eroded.

### 2. Experimenting Live with "Erode" (0 mm ↔ 3.5 mm) & Undo *(Lens: `FILL`)*

In the Fill card, try setting "Erode" (`[data-testid="fill-erode"]`) to `0` mm (full un-eroded box envelope) and then to `3.5` mm (deeper inward level-set carving in low-stress zones). Watch how both the outer surface shader (`fs_outer`) and the interior lattice pass (`fs_core`) update together in real time without mesh boolean failures, because the conformal skin `-(d_outer(p) + skin(p))` is evaluated directly on the evolved signed distance field `d_outer(p) = min(d_body + erode·(1 − t), max(d_body, d_keep))`. Press Undo (Ctrl Z) to restore `Erode = 3 mm`.

- **Expected Check**: Adjusting Erode between 0 mm and 3.5 mm smoothly carves or restores the outer envelope around the protected bosses, and Undo restores Erode = 3 mm.

### 3. Fixed-Grid Ersatz-Void FEA in Results (3) & Direct Slicing in Make (5) *(Lens: `RESULTS`)*

Tap "Go to Results →" (tab 3) to run the two-pass solve across both load cases ("1. Peak Actuator Shear" and "2. Lateral Gust + Shear"). In Pass 2, elements whose centroids lie outside the evolved outer boundary `d_outer(p) > 0` transition to `CLASS_VOID` (`φ = 0`, carried on the fixed background mesh by the ersatz stiffness floor `E_void = 10⁻⁶ E_s`), while interior elements use the blended Gyroid→Diamond homogenised law. Toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) with Colourmap = "Turbo" to inspect internal load-path iso-shells, and use the Transient Evolution & Video Recap bar (`[data-testid="results-transient-bar"]`) to step through the 4 load-ramp milestones or click "🎬 Render Video" (`[data-testid="results-render-video"]`). Then switch to Make (5) to verify watertight 2D slice contours across the evolved outer boundary and internal lattice.

- **Expected Check**: Results (3) reports the additional mass savings from outer boundary evolution alongside graded deflection, 3D volume iso-shells, and `.webm` video playback across both cases, and Make (5) slices the evolved geometry cleanly.

## 3. Theoretical Manual — Chapter 12 — Level-Set Outer Boundary Evolution & Fixed-Grid Topology Optimization (V55)

How Fieldsmith evolves the outer boundary level-set directly on the implicit field grid while preserving protected functional interfaces, conformal skin, and graded TPMS infill.

### 12.1 Stress-Driven Level-Set Shift & Protected Interface Clamping

```text
δ(p) = erode · (1 − t(p)),   t(p) = clamp((param(p) − p_lo) / (p_hi − p_lo), 0, 1)
d_outer(p) = min(d_body(p) + δ(p),  max(d_body(p), d_keep(p)))
```

Let d_body(p) be the signed distance field of the authored design envelope and d_keep(p) be the union of protected shapes offset outward by margin. Shifting d_body(p) by δ(p) = erode · (1 − t(p)) ≥ 0 moves the zero level-set inward by erode mm in low-stress zones (t = 0) and leaves peak load paths (t = 1) at the original envelope. Intersecting the protection clamp max(d_body, d_keep) via min(...) guarantees that inside any protected boss or bore (d_keep < 0) the outer surface is identically d_body(p) = 0, with a C⁰-continuous ramp across the protection margin.

### 12.2 Conformal Skin Composition & Fixed-Mesh Ersatz-Material FEA

```text
d_filled(p) = max(d_outer(p),  min(−(d_outer(p) + skin(p)),  d_lattice(p),  d_keep(p)))
E_e = E_s · max(10⁻⁶,  E*_blend(φ_e, h / a(c_e)))
```

Because the inward shell offset −(d_outer(p) + skin(p)) is evaluated on the evolved outer distance field d_outer(p) rather than the un-eroded box d_body(p), the solid outer skin automatically wraps the newly evolved topology boundary at full thickness skin(p) ∈ [skin, skin_max]. During Pass 2 FEA, jittered Dirichlet quadrature samples d_filled(pt) < 0 over each P2 tetrahedron on the fixed background mesh: eroded exterior elements yield φ_e = 0 (CLASS_VOID, weighted by the ersatz floor 10⁻⁶ E_s), avoiding remeshing failures while capturing the true structural compliance of the evolved geometry.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Configure Level-Set Outer Boundary Evolution (erode=3.0 mm) via API (`set_fill`)

Set family=gyroid, morph=diamond, cell=8.0, cell_min=4.0, skin=1.2, skin_max=2.0, erode=3.0 via set_fill.

```json
{
  "tool": "set_fill",
  "arguments": {
    "family": "gyroid",
    "morph": "diamond",
    "mode": "sheet",
    "cell": 8,
    "cell_min": 4,
    "skin": 1.2,
    "skin_max": 2,
    "erode": 3,
    "rho_min": 0.2,
    "rho_max": 0.65
  }
}
```

### Run Two-Pass Level-Set Topology + Lattice FEA Solve via API (`solve_study`)

Solve Pass 1 driving stress envelope and Pass 2 level-set evolved + multi-zone lattice mechanics.

```json
{
  "tool": "solve_study",
  "arguments": {
    "include_fine": false
  }
}
```
