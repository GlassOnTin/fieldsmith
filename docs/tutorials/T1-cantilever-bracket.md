# T1 — Designing & Stress-Testing a Load-Bearing L-Bracket

> **Category**: Guided Tutorial · **Level**: Beginner · **Verification**: Lesson 1 · Beginner · Solid Geometry & Stress
> **Repository Path**: `docs/tutorials/T1-cantilever-bracket.md` · **Starter Model**: `t1-cantilever-bracket.fsm` · **Verification Reports**: `docs/verification/V1.md`, `docs/verification/V2.md`

Build a wall bracket from simple 3D blocks, round its sharp inside corner so it does not snap, bolt it down, and check whether it safely holds a 40 kg (400 N) load.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T1 — L-Bracket (PETG)"
material petg

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(8, 40, 40) at=(4, 20, 20) blend=8
cylinder hole_l r=3.25 h=20 at=(22, 10, 4) subtract
cylinder hole_r r=3.25 h=20 at=(22, 30, 4) subtract
cylinder load_pin r=4.25 h=20 at=(4, 20, 28) rotate=(0, 90, 0) subtract

region mount = hole_l.side + hole_r.side
region pin = load_pin.side
support bolt mount fixed=xyz
load pull pin force=(103.5, 0, -386.4)
```

## 2. Step-by-Step Guided Walkthrough

### 1. How Parts Are Built: Adding Blocks, Subtracting Holes & Rounding Corners *(Lens: `SHAPE`)*

We want to hang a 400 Newton weight (about 40 kg) from a plastic wall bracket made of PETG (Polyethylene Terephthalate Glycol, a tough, common 3D-printing plastic). In Fieldsmith, you build parts like a sculptor: you start with simple blocks ("base" and "upright") and add them together, then subtract three cylinders ("hole_l", "hole_r", "load_pin") to drill bolt holes. Open the Shapes tree (top-left ≡ on mobile) and select "upright". Notice "Blend = 8 mm": a sharp 90° inside corner concentrates stress and cracks easily, so an 8 mm smooth fillet rounds that inner corner to spread the load.

- **Expected Check**: 5 shapes in the tree (2 added boxes + 3 subtracted cylinder holes). Selecting any shape shows its dimensions and named surface faces (top, bottom, side for cylinders; +X/-X/+Y/-Y/+Z/-Z for boxes).

### 2. Holding the Part Still (Supports) & Applying Forces (Loads) *(Lens: `SETUP`)*

Before a simulator can calculate how much a part bends, it needs two pieces of physical information: where the part is bolted down so it cannot move (a Support), and where external forces push or pull on it (a Load). Tap "Go to Setup →" (or tab 2 at the top). Here, the inner cylindrical walls of the two base bolt holes ("mount") are held fixed in all three directions (X, Y, Z), while the upper pin hole ("pin") carries a 400 N pull angled 15° outward.

- **Expected Check**: In the 3D view, fixed bolt-hole surfaces are highlighted as the "bolt" support, and force arrows on the upper hole show the 400 N "pull" load.

### 3. Checking Stress, Deflection, 3D Volumetric Iso-Shells & Real-Time Video *(Lens: `RESULTS`)*

Tap "Go to Results →" (or tab 3). To compute stress inside a 3D shape, Fieldsmith divides the bracket into thousands of tiny 4-sided pyramids (tetrahedra) and solves twice — first on a Coarse mesh, then on a finer Refined mesh — so you can verify the numbers settle. Compare the Peak von Mises stress against PETG’s yield strength (47 MPa). Try switching the "Colourmap" (`[data-testid="results-colormap"]`) to "Flame" or "Turbo", toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) to see glowing volumetric stress shells inside the fillet, click the 4 Milestone Recap pills (`1. Unloaded (0%)` → `4. Full Load (100%)`), and click "🎬 Render Video" (`[data-testid="results-render-video"]`) to record a real-time 30 fps `.webm` video!

- **Expected Check**: Peak von Mises stress settles around ~37.9 MPa (safely below the 47 MPa yield limit of PETG), tip deflection is ~0.72 mm, and "🎬 Render Video" produces an upright 560×704 `.webm` video with embedded telemetry.

## 3. Theoretical Manual — Chapter 1 — Distance-Field Solid Modelling & Quadratic Finite Elements

Why do traditional CAD fillets often fail when two curved surfaces meet, and why does Fieldsmith use 10-node curved tetrahedra instead of simple 4-node pyramids? This chapter explains both foundations from first principles.

### 1.1 Combining Solids with Signed Distance Fields (CSG & SDF)

```text
h = clamp(0.5 + 0.5·(d_B − d_A)/r, 0, 1)
d_union(x) = mix(d_B, d_A, h) − r·h·(1 − h)
```

Combining simple primitives by adding, subtracting, and intersecting is called Constructive Solid Geometry (CSG). Fieldsmith represents every shape as a Signed Distance Field (SDF) — a function d(x) that returns the distance from any 3D point x to the nearest surface (negative inside the solid, zero on the surface, positive in empty air). Because d(x) is a smooth formula everywhere in space, rounding a corner of radius r between two shapes d_A and d_B is a simple algebraic blend that never fails.

### 1.2 Why 10-Node Quadratic Tetrahedra (P2) Beat Simple 4-Node Elements (P1)

```text
K u = f,   K_e = ∫_Ωe Bᵀ D B dΩ
```

In the Finite Element Method (FEM), a 4-node pyramid (called a linear or P1 element) assumes strain is constant inside each element, making thin beams artificially stiff ("shear locking"). Fieldsmith adds a midpoint node on every edge (10 nodes per tetrahedron, called a quadratic or P2 element) so each element can bend quadratically. The global stiffness equation K u = f relates the nodal displacements u (Degrees of Freedom, or DOFs) to the applied forces f.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Inspect the Part’s Shapes & Bolt Regions via API (`list_shapes`)

An AI assistant or script can inspect the exact same shape tree you see on screen by calling list_shapes.

```json
{
  "tool": "list_shapes",
  "arguments": {}
}
```

### Add a Diagonal Stiffening Rib Live (`upsert_shape`)

Click "▶ Run upsert_shape" to insert a 6 mm reinforcing rib between the base and upright and watch the 3D bracket update.

```json
{
  "tool": "upsert_shape",
  "arguments": {
    "id": "stiffener_rib",
    "kind": "box",
    "op": "add",
    "blend": 2,
    "size": [
      18,
      6,
      18
    ],
    "at": [
      14,
      20,
      14
    ],
    "color": "#38BDF8"
  }
}
```

### Re-Solve Stress & Deflection via API (`solve`)

Run the finite-element solver via the API to verify how much the new stiffening rib reduces bending.

```json
{
  "tool": "solve",
  "arguments": {
    "kind": "solve",
    "dofs": 30000
  }
}
```
