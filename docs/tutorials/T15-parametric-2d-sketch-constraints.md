# T15 — Parametric 2D Sketching, Jacobian QR Constraint Solver & Surface-Attached 3D Feature Engine

> **Category**: Guided Tutorial · **Level**: Intermediate · **Verification**: Verified by V58 (LM + Homotopy + QR Rank DOF + Surface-Attached Extrude/Revolve + MCP Round-Trip)
> **Repository Path**: `docs/tutorials/T15-parametric-2d-sketch-constraints.md` · **Starter Model**: `t15-parametric-sketch-bracket.fsm` · **Verification Reports**: `docs/verification/V4.md`

Solve geometric & dimensional 2D sketch constraints via Damped Levenberg–Marquardt with homotopy continuation and column-pivoted QR rank diagnostics (DOF = N_vars − rank(J)), drag along the constraint nullspace, and project profiles onto any 3D face as Boss Extrudes, Pocket Cuts, or Revolves.

## 1. Starter `.fsm` Model

Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):

```fsm
fieldsmith 1
name "T15 Parametric Sketch & Surface-Feature Bracket"
material alsi10mg

let base_w = 54.0
let base_d = 36.0
let base_h = 10.0
let boss_h = 16.0
let hole_r = 4.5
let pull_n = 1200.0

box base_block size=(base_w, base_d, base_h) at=(0, 0, 5) round=1.2
extrude flange_boss profile=[(-22,-13),(22,-13),(22,13),(-22,13)] h=boss_h at=(0, 0, 18) blend=1.5
cylinder flange_boss_hole_1 r=hole_r h=20 at=(-12, 0, 18) subtract
cylinder flange_boss_hole_2 r=hole_r h=20 at=(12, 0, 18) subtract
revolve spool_collar profile=[(6,-4),(11,-4),(11,-1.5),(8.5,-1.5),(8.5,1.5),(11,1.5),(11,4),(6,4)] angle=360 at=(0, 0, 29) blend=0.8

region mount_bottom = base_block.nz
region load_bore = flange_boss_hole_2.side
support base_fix mount_bottom fixed=xyz
load pin_pull load_bore force=(pull_n, 0, -450)
```

## 2. Step-by-Step Guided Walkthrough

### 1. Inspect Color-Coded Degrees of Freedom (DOF = N_vars − rank(J)) *(Lens: `SHAPE`)*

In the embedded Parametric 2D Sketcher & Constraint Solver workbench above (or by clicking "✏️ Sketch…" in the Shapes tree), inspect the top-right status badge: "✓ Fully Constrained (0 DOF)" in green. Click the preset dropdown and select "Under-Constrained Four-Bar (2 DOF — Blue Drag Demo)". Notice how unconstrained vertices turn sky-blue with a "+1" or "+2" DOF badge computed directly from the column-pivoted Householder QR rank of the constraint Jacobian J.

- **Expected Check**: Switching to the Under-Constrained preset changes the status badge to "2 DOF Remaining (Drag Blue Points)" and marks movable vertices in blue.

### 2. Drag Vertices Along the Constraint Nullspace & Test Smart Dimensioning (D) *(Lens: `SHAPE`)*

While in the "Under-Constrained Four-Bar" preset, drag vertex p3 on the 2D canvas: the Damped Levenberg–Marquardt solver projects your cursor motion onto the Jacobian nullspace P_null = I − J⁺J so all existing Horizontal, Fixed, and Distance constraints remain satisfied in real time. Or click any dimension badge (e.g. "40 mm" or "let base_w") on the canvas to open the inline Smart Dimension popover and type a new millimeter value or .fsm let variable name.

- **Expected Check**: Dragging a blue point moves the linkage smoothly without breaking any active geometric or dimensional constraint.

### 3. Diagnose Conflicting Constraints via RREF Pivot Analysis *(Lens: `SHAPE`)*

Click the "⚡ Test Conflict" button in the Sketch Workbench toolbar. This injects a contradictory HorizontalDistance constraint on an already-fixed span. Observe how the solver never explodes or silently corrupts your sketch: it isolates the conflicting constraint IDs via Gauss–Jordan RREF pivot analysis and highlights them in red ("✕ Conflicting Constraints"). Click "✕" next to the conflicting constraint in the right-hand list (or reload "Flanged Mounting Bracket (0 DOF)") to restore a clean 0-DOF state.

- **Expected Check**: Injecting a redundant conflict turns the status badge red and lists the exact conflicting constraint IDs so you can delete the offending rule in one click.

### 4. Apply Surface-Attached 3D Features: Boss Extrude, Pocket Cut & Revolve (Stage 2) *(Lens: `SHAPE`)*

In the bottom "Stage 2 — Surface-Attached 3D Feature Builder" bar, choose preset "1. Flanged Mounting Bracket (0 DOF)" or "3. Bolt Flange Plate (bolt_circle macro)", set Target Plane / Face to "base_block.top" (or "xy"), choose Feature Operation "+ Boss Extrude (Union + Auto-Holes)" or "− Pocket Cut (Subtract)", and click "⚡ Apply 3D Feature to Document". Exterior closed loops automatically extrude as a boss while interior closed loops (circles/polygons) automatically subtract as matching 3D through-holes. Try preset "2. Turned Aerospace Spool (Revolve)" to generate a 360° axisymmetric Revolve feature.

- **Expected Check**: Clicking Apply 3D Feature compiles the solved 2D profile into exact 3D Extrude, Subtracted Hole, or Revolve nodes attached to the target face.

### 5. Solve 3D P2 Stress on the Sketch-Driven Part & Inspect Theory (6) *(Lens: `RESULTS`)*

Click "Go to Results →" (or press 3) to run the 3D quadratic (P2) tetrahedral FEA solver on the sketch-driven bracket and spool collar under the 1200 N pin load, then open Theory (6) to inspect the active Stage 1 Parametric Sketch Solver equations alongside the Stage 2/3 continuum elasticity equations.

- **Expected Check**: The 3D P2 solver meshes the extruded boss, subtracted bolt bores, and revolved collar seamlessly and reports peak von Mises stress and safety factor.

## 3. Theoretical Manual — Chapter 15: Parametric 2D Variational Sketching — Damped Levenberg–Marquardt, Homotopy Continuation, Jacobian QR Rank & Surface-Attached 3D SDF Calculus

Formulates the mathematical engine behind modern variational 2D CAD sketchers (SolveSpace, FreeCAD Sketcher / PlaneGCS, Onshape) and couples it directly to Fieldsmith’s implicit Signed Distance Field (SDF) CSG tree for surface-attached Boss Extrudes, Pocket Cuts, and Axisymmetric Revolves.

### 15.1 Variational Constraint Residuals & Analytical Jacobian Assembly

```text
q = [ x₁, y₁, x₂, y₂, …, x_n, y_n, r₁, …, r_m ]ᵀ ∈ ℝ^(N_vars)
r_Horizontal(p_i, p_j) = y_j − y_i = 0,     r_Perpendicular(e₁, e₂) = (t̂₁ · t̂₂) · L̄ = 0
r_Distance(p_i, p_j; d) = ‖p_j − p_i‖₂ − d(let) = 0,     J_km = ∂r_k(q) / ∂q_m
```

Every 2D sketch entity contributes free coordinates to the state vector q ∈ ℝ^(N_vars) (2 DOFs per point (x_i, y_i) and 1 additional radius DOF r_c per circle/arc). Each geometric rule (Coincident, Horizontal, Vertical, Parallel, Perpendicular, EqualLength, Tangent, Concentric, Midpoint, Symmetric, PointOnLine) and dimensional rule (Distance, HorizontalDistance, VerticalDistance, Angle, Radius, Diameter) is normalized to millimeter length units so the analytical Jacobian matrix J_km = ∂r_k/∂q_m is well-conditioned across mixed angular and linear constraints.

### 15.2 Damped Levenberg–Marquardt with Homotopy Continuation & Nullspace Dragging

```text
(Jᵀ J + λ I) δq = −Jᵀ r(q; α_s),     α_s ∈ { 0.25, 0.50, 0.75, 1.00 }
d_target(α_s) = (1 − α_s) · d_initial(q⁰) + α_s · d_final
δq_drag = (I − J⁺ J) δq_cursor
```

Large dimensional edits in standard Newton–Raphson sketchers frequently flip triangle orientations or diverge near kinematic singularities. Fieldsmith combines adaptive Levenberg–Marquardt Marquardt-diagonal damping λ ∈ [10⁻¹⁰, 10⁶] with 4-stage homotopy continuation α_s ∈ {0.25, 0.5, 0.75, 1.0}: when a dimension change exceeds 25% of the current geometry scale, target dimensions are smoothly interpolated from the initial configuration q⁰ to the final target, while the LM trust-region parameter λ I regularizes near-degenerate or collinear intermediate Jacobians (preventing singular JᵀJ stalls even when two lines pass within 0.001° of collinearity). During interactive mouse dragging, cursor displacements δq_cursor are projected onto the constraint nullspace P_null = I − J⁺J at 60 Hz.

### 15.3 Exact Degrees-of-Freedom (DOF) & Conflict Isolation via Column-Pivoted QR / RREF

```text
J P = Q R   (Householder column-pivoted QR,  rank(J) = #{ k : |R_kk| > ε_rank })
DOF = N_vars − rank(J)
DOF = 0 ∧ ‖r(q*)‖_∞ ≤ 10⁻⁶ ⇒ FullyConstrained,   ‖r(q*)‖_∞ > 10⁻³ ⇒ OverConstrainedConflict
```

Counting constraints naively (N_vars − N_eqs) fails whenever a user adds a redundant constraint (such as a fourth 90° angle on a rectangle). By factoring J via column-pivoted QR and Gauss–Jordan Reduced Row Echelon Form (RREF), Fieldsmith computes the exact numerical rank(J) and true remaining degrees of freedom DOF = N_vars − rank(J). Zero-pivot rows in RREF of Jᵀ identify dependent constraint rows: if their converged residual vanishes, they are flagged Amber (RedundantConstraints); if non-zero, they are flagged Red (OverConstrainedConflict) with per-constraint IDs.

### 15.4 Stage 2 Surface-Attached Coordinate Frames & Exact 2D-to-3D Extrude / Revolve SDFs

```text
(n̂, û, v̂, o_face) = FaceFrame(shape_id, face ∈ {top, bottom, front, back, left, right})
d_extrude(p) = max_±( d_poly2d(p_u, p_v), |p_n| − h/2 ),     d_revolve(p) = d_poly2d( √(p_x² + p_y²), p_z )
```

In Stage 2, a solved 2D sketch is bound either to a canonical plane (xy, xz, yz) or parametrically to a face of an existing 3D primitive (on = "base_block.top"). Exterior closed loops compile into 3D Extrude or Revolve primitives oriented along the face normal n̂, while interior closed loops (circles or polygons inside the outer boundary) are automatically classified by winding/containment and subtracted as matching 3D through-holes or pockets.

## 4. Reproducible Automation (MCP / REST API & CLI)

### Step 1: Solve a Parametric 2D Sketch with Macros & Inspect DOF/Rank (`solve_sketch`) (`solve_sketch`)

Calls `solve_sketch` with a declarative `center_rect` + `bolt_circle` macro sketch. Returns converged vertex coordinates, exact Jacobian rank, `dof = 0` (`FullyConstrained`), and outer/inner closed profile loops in one round-trip.

```json
{
  "tool": "solve_sketch",
  "arguments": {
    "preset": "flanged_mounting_bracket"
  }
}
```

### Step 2: Project Solved Sketch onto `base_block.top` as a 3D Boss + Auto-Holes (`apply_sketch_feature`) (`apply_sketch_feature`)

Solves the sketch and compiles it into a 3D `boss_extrude` feature attached to `base_block.top`, automatically subtracting the interior mounting holes and updating the live `.fsm` workspace.

```json
{
  "tool": "apply_sketch_feature",
  "arguments": {
    "feature_id": "top_mounting_boss",
    "preset": "flanged_mounting_bracket",
    "on": "base_block.top",
    "operation": {
      "kind": "boss_extrude",
      "height": 14,
      "blend": 1.2
    }
  }
}
```

### Step 3: Add an Axisymmetric Turned Collar via `upsert_shape` (`revolve`) & Solve 3D P2 Stress (`upsert_shape`)

Uses `upsert_shape` with `kind: "revolve"` to add a 360° turned aerospace collar from a 2D `(r, z)` meridional profile, ready for 3D P2 finite-element analysis.

```json
{
  "tool": "upsert_shape",
  "arguments": {
    "id": "turned_collar",
    "kind": "revolve",
    "angle": 360,
    "profile": [
      [
        7,
        -3.5
      ],
      [
        12,
        -3.5
      ],
      [
        12,
        -1.2
      ],
      [
        9.2,
        -1.2
      ],
      [
        9.2,
        1.2
      ],
      [
        12,
        1.2
      ],
      [
        12,
        3.5
      ],
      [
        7,
        3.5
      ]
    ],
    "at": [
      0,
      0,
      29
    ],
    "blend": 0.6
  }
}
```
