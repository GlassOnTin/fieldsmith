// Interactive 3-Column Tutorial Guide & Theoretical Manual (T1–T8):
// 1) Step-by-step Human User Tutorial (with 1-click Lens navigation & model loader)
// 2) Textbook Theoretical Manual Chapter (governing equations, discretisation & V01–V52 proofs)
// 3) Live MCP / REST API Flow (with 1-click execution against `POST /api/mcp` & shared workspace sync)

import { useState } from 'preact/hooks';
import { applyAddrs, parseAddr } from '../doc/param';
import { store, type Lens } from '../doc/store';
import { printFsm } from '../doc/wasm';
import { extraCommands, openText, viewApi } from './commands';
import { viewOverlays, statusItems } from './App';
import { markWorkspaceRevisionSeen } from './systems';
import { ui, useApp } from './ui';

export interface TutorialStep {
  title: string;
  lens?: Lens | 'systems';
  instruction: string;
  expected: string;
}

export interface TheorySection {
  heading: string;
  equations: string;
  body: string;
}

export interface McpFlowStep {
  title: string;
  description: string;
  tool: string;
  arguments: Record<string, unknown>;
}

export interface TutorialSheet {
  id: string;
  code: string;
  level: 'Beginner' | 'Intermediate' | 'Advanced' | 'Flagship Multi-Physics';
  title: string;
  subtitle: string;
  verificationBadge: string;
  starterFsmName: string;
  starterFsm: string;
  opensSystemsWorkbench?: boolean;
  userSteps: TutorialStep[];
  theoryChapter: {
    chapterTitle: string;
    summary: string;
    sections: TheorySection[];
  };
  mcpSteps: McpFlowStep[];
}

export const TUTORIAL_SHEETS: TutorialSheet[] = [
  {
    id: 'T1',
    code: 'T1',
    level: 'Beginner',
    title: 'Designing & Stress-Testing a Load-Bearing L-Bracket',
    subtitle:
      'Build a wall bracket from simple 3D blocks, round its sharp inside corner so it does not snap, bolt it down, and check whether it safely holds a 40 kg (400 N) load.',
    verificationBadge: 'Lesson 1 · Beginner · Solid Geometry & Stress',
    starterFsmName: 't1-cantilever-bracket.fsm',
    starterFsm: `fieldsmith 1
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
`,
    userSteps: [
      {
        title: '1. How Parts Are Built: Adding Blocks, Subtracting Holes & Rounding Corners',
        lens: 'shape',
        instruction:
          'We want to hang a 400 Newton weight (about 40 kg) from a plastic wall bracket made of PETG (Polyethylene Terephthalate Glycol, a tough, common 3D-printing plastic). In Fieldsmith, you build parts like a sculptor: you start with simple blocks ("base" and "upright") and add them together, then subtract three cylinders ("hole_l", "hole_r", "load_pin") to drill bolt holes. Open the Shapes tree (top-left ≡ on mobile) and select "upright". Notice "Blend = 8 mm": a sharp 90° inside corner concentrates stress and cracks easily, so an 8 mm smooth fillet rounds that inner corner to spread the load.',
        expected:
          '5 shapes in the tree (2 added boxes + 3 subtracted cylinder holes). Selecting any shape shows its dimensions and named surface faces (top, bottom, side for cylinders; +X/-X/+Y/-Y/+Z/-Z for boxes).',
      },
      {
        title: '2. Holding the Part Still (Supports) & Applying Forces (Loads)',
        lens: 'setup',
        instruction:
          'Before a simulator can calculate how much a part bends, it needs two pieces of physical information: where the part is bolted down so it cannot move (a Support), and where external forces push or pull on it (a Load). Tap "Go to Setup →" (or tab 2 at the top). Here, the inner cylindrical walls of the two base bolt holes ("mount") are held fixed in all three directions (X, Y, Z), while the upper pin hole ("pin") carries a 400 N pull angled 15° outward.',
        expected:
          'In the 3D view, fixed bolt-hole surfaces are highlighted as the "bolt" support, and force arrows on the upper hole show the 400 N "pull" load.',
      },
      {
        title: '3. Checking Stress, Deflection & Whether the Calculation Has Settled',
        lens: 'results',
        instruction:
          'Tap "Go to Results →" (or tab 3). To compute stress inside a 3D shape, Fieldsmith divides the bracket into thousands of tiny 4-sided pyramids (tetrahedra) and solves for how every corner moves. How do we know the grid is fine enough to trust? Fieldsmith automatically solves twice — first on a Coarse mesh, then on a finer Refined mesh — so you can verify the numbers settle. Compare the Peak von Mises stress (a combined measure of tension and shear) against PETG’s yield strength (47 MPa, where the plastic starts to permanently bend).',
        expected:
          'Peak von Mises stress settles around ~37.9 MPa (safely below the 47 MPa yield limit of PETG), and the tip bends downward by ~0.72 mm.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 1 — Distance-Field Solid Modelling & Quadratic Finite Elements',
      summary:
        'Why do traditional CAD fillets often fail when two curved surfaces meet, and why does Fieldsmith use 10-node curved tetrahedra instead of simple 4-node pyramids? This chapter explains both foundations from first principles.',
      sections: [
        {
          heading: '1.1 Combining Solids with Signed Distance Fields (CSG & SDF)',
          equations: 'h = clamp(0.5 + 0.5·(d_B − d_A)/r, 0, 1)\nd_union(x) = mix(d_B, d_A, h) − r·h·(1 − h)',
          body: 'Combining simple primitives by adding, subtracting, and intersecting is called Constructive Solid Geometry (CSG). Fieldsmith represents every shape as a Signed Distance Field (SDF) — a function d(x) that returns the distance from any 3D point x to the nearest surface (negative inside the solid, zero on the surface, positive in empty air). Because d(x) is a smooth formula everywhere in space, rounding a corner of radius r between two shapes d_A and d_B is a simple algebraic blend that never fails.',
        },
        {
          heading: '1.2 Why 10-Node Quadratic Tetrahedra (P2) Beat Simple 4-Node Elements (P1)',
          equations: 'K u = f,   K_e = ∫_Ωe Bᵀ D B dΩ',
          body: 'In the Finite Element Method (FEM), a 4-node pyramid (called a linear or P1 element) assumes strain is constant inside each element, making thin beams artificially stiff ("shear locking"). Fieldsmith adds a midpoint node on every edge (10 nodes per tetrahedron, called a quadratic or P2 element) so each element can bend quadratically. The global stiffness equation K u = f relates the nodal displacements u (Degrees of Freedom, or DOFs) to the applied forces f.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Inspect the Part’s Shapes & Bolt Regions via API',
        description:
          'An AI assistant or script can inspect the exact same shape tree you see on screen by calling list_shapes.',
        tool: 'list_shapes',
        arguments: {},
      },
      {
        title: 'Add a Diagonal Stiffening Rib Live',
        description:
          'Click "▶ Run upsert_shape" to insert a 6 mm reinforcing rib between the base and upright and watch the 3D bracket update.',
        tool: 'upsert_shape',
        arguments: {
          id: 'stiffener_rib',
          kind: 'box',
          op: 'add',
          blend: 2.0,
          size: [18, 6, 18],
          at: [14, 20, 14],
          color: '#38BDF8',
        },
      },
      {
        title: 'Re-Solve Stress & Deflection via API',
        description:
          'Run the finite-element solver via the API to verify how much the new stiffening rib reduces bending.',
        tool: 'solve',
        arguments: {
          kind: 'solve',
          dofs: 30000,
        },
      },
    ],
  },
  {
    id: 'T2',
    code: 'T2',
    level: 'Beginner',
    title: 'Named Dimensions (let) & Automated Design Trade-Off Sweeps',
    subtitle:
      'Drive part dimensions with named variables (like thickness = 10 mm) and automatically sweep a range of thicknesses to see the weight-vs-stiffness trade-off curve.',
    verificationBadge: 'Lesson 2 · Beginner · Parametric Design',
    starterFsmName: 't2-parametric-cantilever.fsm',
    starterFsm: `fieldsmith 1
name "T2 — Parametric Cantilever"
material petg

let span = 80
let width = 16
let thick = 10
let tip_force = 240

box beam size=(span, width, thick) at=(span / 2, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -tip_force)
`,
    userSteps: [
      {
        title: '1. Why Use Named Variables ("let") Instead of Fixed Numbers?',
        lens: 'shape',
        instruction:
          'When designing a beam, changing its length ("span = 80 mm") also means moving its centre position ("at = span / 2") so the left end stays anchored at the wall. By defining named variables ("let span = 80", "let thick = 10"), every formula that references them updates together. Open the Inspector panel (right panel on desktop, or the sliders button below ≡ on mobile) with nothing selected. Change "thick" from 10 to 12 mm and watch the 3D beam thicken immediately.',
        expected:
          'Under "Parameters (let)", editing "thick" updates the 3D beam thickness and keeps all dependent geometry aligned.',
      },
      {
        title: '2. Sweep Thickness Automatically to Plot Weight vs. Bending',
        lens: 'results',
        instruction:
          'Suppose you want the lightest beam that bends less than 1.5 mm. Instead of guessing and re-running by hand, switch to Results (3). Expand "More ▾" if compact, scroll to the "Sweep" section, pick Parameter = "thick" from 8 mm to 14 mm (5 steps), and click "Run". Fieldsmith solves all 5 designs and plots a trade-off chart (called a Pareto frontier) showing weight on one axis and peak stress or deflection on the other.',
        expected:
          '5 design points appear on the trade-off chart. Doubling beam thickness cuts bending deflection by 8× (because bending stiffness grows with the cube of thickness, thick³) while only doubling weight.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 2 — Dependency Graphs & Bracketing the True Answer from Above and Below',
      summary:
        'How does Fieldsmith update linked formulas in the right order without circular loops, and how can a computer guarantee how close its finite-element approximation is to the exact continuum physics?',
      sections: [
        {
          heading: '2.1 Directed Acyclic Graphs (DAGs) for Parametric Formulas',
          equations: 'span = 80   →   beam.size.x = span,   beam.at.x = span / 2',
          body: 'When one variable or dimension depends on another, Fieldsmith organises those dependencies into a one-way network called a Directed Acyclic Graph (DAG — "directed" because values flow from inputs to outputs, and "acyclic" because circular loops like A = B + 1 and B = A + 1 are caught and refused). Whenever you edit or sweep a let variable, Fieldsmith walks the graph in order and updates every shape and load.',
        },
        {
          heading: '2.2 Prager–Synge Dual Energy Bounds (Lower & Upper Brackets)',
          equations: 'C_h = fᵀ u_h  ≤  C_exact  ≤  C*(σ*) = ∫_Ω σ* : D⁻¹ : σ* dΩ',
          body: 'Standard displacement-based finite elements always err slightly on the stiff side, giving a guaranteed lower bound (C_h) on the true deformation energy (C_exact). Fieldsmith can also construct a separate, purely force-balanced stress field σ* that satisfies Newton’s third law (div σ* + f = 0) across every element face. By the Prager–Synge theorem, that dual stress field gives a guaranteed upper bound C*(σ*), bracketing the exact physical answer between two known numbers.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Set Beam Thickness thick = 12.5 mm via API',
        description:
          'Update a named parameter via set_let and watch the 3D viewport recompute all dependent geometry.',
        tool: 'set_let',
        arguments: {
          name: 'thick',
          value: 12.5,
        },
      },
      {
        title: 'Run a 4-Point Thickness Sweep via API',
        description:
          'Evaluate thick = [8, 10, 12, 14] mm in one call and return mass, peak stress, and deflection for each.',
        tool: 'solve',
        arguments: {
          kind: 'sweep',
          param: 'thick',
          values: [8, 10, 12, 14],
          rungs: [15000],
        },
      },
    ],
  },
  {
    id: 'T3',
    code: 'T3',
    level: 'Intermediate',
    title: 'How Heat Warps Parts: Thermal Conduction & Expansion Stress',
    subtitle:
      'See how heat flows from a 220 °C hot pin to a 20 °C cold mount, and how restrained thermal expansion creates internal stress even with zero external force.',
    verificationBadge: 'Lesson 3 · Intermediate · Heat & Thermal Stress',
    starterFsmName: 't3-thermo-elastic-bracket.fsm',
    starterFsm: `fieldsmith 1
name "T3 — Thermo-Elastic Manifold Bracket"
material ss316l

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(8, 40, 40) at=(4, 20, 20) blend=8
cylinder hole1 r=3.25 h=20 at=(20, 10, 4) subtract
cylinder hole2 r=3.25 h=20 at=(20, 30, 4) subtract
cylinder hole3 r=4.25 h=20 at=(4, 20, 28) rotate=(0, 90, 0) subtract

region mount = hole1.side + hole2.side
region load_pin = hole3.side
support s1 mount fixed=xyz
load l1 load_pin force=(103.5, 0, -386.4) case="Mechanical"
temperature t_cold mount temp=20 case="Thermal"
temperature t_hot load_pin temp=220 case="Thermal"
solve tref=20
`,
    userSteps: [
      {
        title: '1. Why Heating a Bolted Part Creates Stress',
        lens: 'setup',
        instruction:
          'Almost all metals and plastics expand slightly when heated. If a part floats freely in space and warms up uniformly, it grows slightly larger with zero internal stress. But if the part is bolted down to a cold frame ("mount" held at 20 °C) while its upper port touches a hot exhaust pipe ("load_pin" at 220 °C), the hot metal tries to expand against the cold bolts. Switch to Setup (2) to inspect the two separate load cases on this 316L stainless-steel bracket: "Mechanical" (400 N force) and "Thermal" (20 °C to 220 °C temperature difference).',
        expected:
          'Setup tree shows Support s1 (fixed bolts), Load l1 (Mechanical case), and Temperatures t_cold (20 °C) and t_hot (220 °C).',
      },
      {
        title: '2. View the Temperature Map & Compare Mechanical vs. Thermal Stress',
        lens: 'results',
        instruction:
          'Switch to Results (3). First tap "Temperature" in the results mode bar to see how heat conducts steadily from the 220 °C upper hole down to the 20 °C mounting bolts. Next tap "Stress", expand "More ▾", and switch the "Case" selector between "Mechanical", "Thermal", and "Envelope" (which shows the worst-case stress across both).',
        expected:
          'The Temperature view shows a smooth 20 °C → 220 °C gradient; the Thermal stress view shows high stress around the two base bolt holes where expansion is prevented.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 3 — Steady-State Heat Conduction & Thermo-Elastic Strain',
      summary:
        'Fieldsmith solves thermal stress in two linked steps on the same mesh: first it finds the steady-state temperature T(x) everywhere inside the part, and then it converts local temperature rise into thermal expansion strain.',
      sections: [
        {
          heading: '3.1 Fourier’s Law of Heat Conduction & Boundary Conditions',
          equations: '−∇·(k ∇T) = 0  in Ω\nFixed Temp (Dirichlet): T = T_0,   Convection (Robin): −k ∂T/∂n = h·(T − T_air)',
          body: 'Heat flows from hot to cold at a rate proportional to the material’s thermal conductivity k (Fourier’s law). At steady state, heat entering any tiny volume equals heat leaving it, giving Laplace’s equation −∇·(k ∇T) = 0. Boundaries can specify a known surface temperature (called a Dirichlet condition), a known heat-input rate in Watts/mm² (Neumann condition), or cooling by surrounding air/liquid with heat-transfer coefficient h (Robin convection condition).',
        },
        {
          heading: '3.2 Thermal Expansion Strain (ε_th = α · ΔT)',
          equations: 'ε_th(x) = α · (T(x) − T_ref) · [1, 1, 1, 0, 0, 0]ᵀ\nσ = D · (ε_total − ε_th)',
          body: 'Every material has a coefficient of thermal expansion α (how much a 1 mm bar expands per 1 °C temperature rise above the reference temperature T_ref). Notice that mechanical stress σ depends only on the elastic strain (ε_total − ε_th): if a bar is free to expand so ε_total = ε_th, stress σ is zero; if fixed bolts force ε_total = 0, a compressive thermal stress σ = −D ε_th builds up.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Increase Hot Pin Temperature to 320 °C via API',
        description:
          'Raise the hot boundary temperature from 220 °C to 320 °C and observe how thermal stress scales with ΔT.',
        tool: 'set_let',
        arguments: {
          name: 't_hot.celsius',
          value: 320,
        },
      },
      {
        title: 'Solve Both Mechanical & Thermal Load Cases via API',
        description:
          'Run the coupled heat-conduction and thermo-elasticity solver and return the per-case stress summary.',
        tool: 'solve',
        arguments: {
          kind: 'solve',
          dofs: 30000,
        },
      },
    ],
  },
  {
    id: 'T4',
    code: 'T4',
    level: 'Intermediate',
    title: 'Natural Vibration Frequencies: Why Parts Hum, Shake or Resonate',
    subtitle:
      'Every structure has natural frequencies (measured in Hertz, cycles per second) where it wants to vibrate like a tuning fork. Find and animate the first 4 vibration modes.',
    verificationBadge: 'Lesson 4 · Intermediate · Vibration & Resonance',
    starterFsmName: 't4-modal-tuning-fork.fsm',
    starterFsm: `fieldsmith 1
name "T4 — Optical Sensor Mount (Modal)"
material alsi10mg

box flange size=(36, 36, 6) at=(0, 0, 3)
cylinder stem r=6 h=34 at=(0, 0, 20) blend=3
box stage size=(28, 28, 8) at=(0, 0, 38) blend=2

region base = flange.nz
region top = stage.pz
support fix_base base fixed=xyz
load gravity_tip top force=(0, 200, -300)
solve modes=4
`,
    userSteps: [
      {
        title: '1. What Are Natural Frequencies and Mode Shapes?',
        lens: 'shape',
        instruction:
          'If you pluck a ruler held over the edge of a desk, it vibrates back and forth at a specific pitch — its fundamental natural frequency (measured in Hz, or cycles per second). If a motor, fan, or vehicle engine shakes a bracket at that exact frequency, vibrations amplify dramatically (resonance). Here we have a 3D-printed aluminium (AlSi10Mg) optical sensor pedestal. Open the Inspector with nothing selected to see "Modes = On (Count = 4)".',
        expected:
          'The pedestal consists of a bolted base flange, a slender cylindrical stem, and a heavy top sensor stage — a classic mass-on-a-spring geometry.',
      },
      {
        title: '2. Animate the 4 Vibration Modes in Results (3)',
        lens: 'results',
        instruction:
          'Switch to Results (3). In the "Modes" row of the Results card, tap Mode 1, 2, 3, or 4 and tap "Play". Mode 1 and Mode 2 are side-to-side rocking of the heavy top stage in X and Y; Mode 3 is twisting (torsion) around the vertical stem; Mode 4 is vertical bouncing.',
        expected:
          'All 4 natural frequencies (in Hz) are listed; tapping Play animates the exact 3D flexing motion of each vibration mode.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 4 — Mass Matrices, Stiffness Matrices & Eigenvalue Vibrations',
      summary:
        'For a single weight m on a spring of stiffness k, the natural frequency is ω = √(k / m). For a 3D solid, k becomes the stiffness matrix K and m becomes the mass matrix M.',
      sections: [
        {
          heading: '4.1 The Generalised Eigenvalue Problem (K φ = ω² M φ)',
          equations: 'M ü(t) + K u(t) = 0   ⇒   K φ_k = ω_k² M φ_k,   f_k = ω_k / (2π)  [Hz]',
          body: 'When a structure vibrates harmonically without external damping, Newton’s second law (Force = Mass × Acceleration) balances elastic restoring forces (−K u) against inertial forces (M ü). Substituting a sinusoidal motion u(t) = φ cos(ω t) turns the differential equation into an algebraic eigenvalue problem: each eigenvector φ_k is a 3D mode shape, and each eigenvalue ω_k² gives the natural frequency f_k in Hz.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Compute the First 4 Natural Frequencies via API',
        description:
          'Ask the solver for the lowest 4 natural vibration frequencies (in Hz) of the sensor pedestal.',
        tool: 'solve',
        arguments: {
          kind: 'modes',
          n: 4,
          dofs: 30000,
        },
      },
    ],
  },
  {
    id: 'T5',
    code: 'T5',
    level: 'Intermediate',
    title: '3D Printing Strength: Preventing Layer Splitting & Choosing Print Orientation',
    subtitle:
      'Desktop 3D printers stack melted plastic layer by layer. Like wood grain, bonds between layers are weaker than along the strands — learn how to detect layer splitting and rotate the part.',
    verificationBadge: 'Lesson 5 · Intermediate · 3D Printing & Layer Peel',
    starterFsmName: 't5-additive-peel-arm.fsm',
    starterFsm: `fieldsmith 1
name "T5 — Vertical FDM Cantilever (Layer Peel)"
material petg

let h_root = 12
box beam size=(60, 16, h_root) at=(30, 0, 0)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load tip_load tip force=(0, 0, -450) case="Peak"
solve size=3.5 build=(90, 0)
`,
    userSteps: [
      {
        title: '1. Why Print Orientation Matters (Wood-Grain Analogy)',
        lens: 'results',
        instruction:
          'When a filament 3D printer — called Fused Deposition Modelling (FDM) — prints a part, it lays down continuous plastic strands in horizontal XY layers stacked along the vertical build direction. Just like wood splits easily along its grain, a 3D-printed part is weaker when pulled apart across its layer seams ("Layer Peel") than when pulled along the continuous strands. Tap "Go to Results →" and tap the "Peel" button at the top of the Results card. Notice the white slice lines drawn on the 3D beam: because this part was oriented upright at (90°, 0°), the layer seams run vertically right across the beam, and bending pulls those seams apart!',
        expected:
          'White slice lines on the 3D beam show the layer seams; the "Layer peel" row reports the current peel utilization and the best print orientation.',
      },
      {
        title: '2. Rotate to the Best Print Orientation in One Click',
        lens: 'results',
        instruction:
          'Fieldsmith automatically checks candidate print angles across the sphere to find the orientation where bending tension runs along the strong filament strands instead of pulling the layer seams apart. Tap the "Apply best" button right next to the "Layer peel" readout (or switch to tab 5, "Make", to inspect the build plate orientation and steep downward overhangs that would need support material).',
        expected:
          'Tapping "Apply best" rotates the print orientation so the white layer lines align with the length of the beam, reducing layer-splitting risk.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 5 — Anisotropic Layer-Bond Strength & Overhang Angles',
      summary:
        'Metals cast from a mould are isotropic (equally strong in every direction), whereas layer-by-layer 3D prints are orthotropic / transversely isotropic: inter-layer tensile strength S_z is significantly lower than in-plane yield strength σ_Y.',
      sections: [
        {
          heading: '5.1 Calculating Inter-Layer Peel & Shear Traction Across Layer Seams',
          equations: 't = σ · b,   t_normal = b · t,   τ_shear = ||t − t_normal b||\nU_peel(b) = √( (max(0, t_normal) / S_z)² + (τ_shear / S_shear)² )',
          body: 'If the printer stacks layers along unit vector b(θ, φ), then t = σ · b is the force-per-unit-area acting directly on the flat interface between two adjacent layers. Only positive tension (t_normal > 0) tries to peel the layers apart (compression pushes them together), while τ_shear tries to slide one layer across the next. A part is safe against layer splitting when U_peel < 1.0 everywhere.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Set Print Build Angle to Flat (0°, 0°) via API',
        description:
          'Align the 3D-printed layers horizontally along the beam length so continuous filament strands carry the bending tension.',
        tool: 'set_build',
        arguments: {
          theta: 0,
          phi: 0,
        },
      },
      {
        title: 'Check Downward Overhang Area (> 45°) via API',
        description:
          'Measure how many square millimetres of downward-facing surface tilt past 45° and would need print supports.',
        tool: 'solve',
        arguments: {
          kind: 'overhang',
          build: [0, 0, 1],
          threshold_deg: 45,
        },
      },
    ],
  },
  {
    id: 'T6',
    code: 'T6',
    level: 'Advanced',
    title: 'Lightweighting Like Bone: Stress-Graded Internal Gyroid Lattices',
    subtitle:
      'Living bone is dense and thick near heavily loaded joints, but porous and lightweight inside. Learn how to fill a part with a smooth Gyroid lattice that automatically thickens where stress is high.',
    verificationBadge: 'Lesson 6 · Advanced · Graded Gyroid Infill & 3MF',
    starterFsmName: 't6-gyroid-graded-bracket.fsm',
    starterFsm: `fieldsmith 1
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
`,
    userSteps: [
      {
        title: '1. Look Inside the Part: Smooth Gyroid Lattice & Protected Bolt Collars',
        lens: 'fill',
        instruction:
          'Why print a part 100% solid if the middle carries almost no stress? Tap "Go to Fill →" (which automatically turns on a 50% cutaway section and translucent outer skin so you can see inside). Inside the outer shell is a Gyroid — a smooth, wave-like 3D honeycomb belonging to a family called Triply Periodic Minimal Surfaces (TPMS). Unlike sharp criss-cross struts that concentrate stress at corners, a Gyroid has smooth soap-film curves with zero sharp junctions, and we keep a 2 mm solid protective collar around all three bolt holes ("protect").',
        expected:
          'In the 3D viewport, the cutaway reveals the wavy internal Gyroid sheet network, the outer skin, and solid collars around the bolt holes.',
      },
      {
        title: '2. How Fieldsmith Grades Density, Cell Size & Skin Thickness by Stress',
        lens: 'results',
        instruction:
          'In the Fill card, notice "ρ min = 0.2" (20% dense in low-stress zones) to "ρ max = 0.6" (60% dense in high-stress zones), and "Skin = 1.2 mm" up to "Max skin = 2.2 mm" (plus optional "Min cell" and "Morph to" controls that can refine unit-cell size or morph between TPMS families in high-stress zones — covered in Lesson T11). Fieldsmith first solves the solid bracket to map where stress is highest, then automatically thickens both the internal Gyroid walls and the outer skin near the high-stress fillet while keeping unloaded regions light. Switch to Results (3) to check the weight savings and verify that the thin lattice walls will not buckle.',
        expected:
          'The part weighs ~30% less than a solid bracket while passing both material yield and lattice cell-wall buckling checks.',
      },
      {
        title: '3. Inspect 2D Printer Slices & Export a 3MF Manufacturing File in Make (5)',
        lens: 'make',
        instruction:
          'Switch to Make (5). Because a fine Gyroid lattice has thousands of curved internal surfaces, exporting an old-fashioned STL triangle file can produce huge files. Fieldsmith can export either a watertight 3D mesh or slice the mathematical distance field directly into exact 2D layer contours inside an industry-standard 3MF (3D Manufacturing Format) file.',
        expected:
          'Make (5) reports manifold mesh health and lets you scrub through 2D printer layer slices.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 6 — Minimal-Surface Lattices (TPMS) & Two-Pass Homogenised Mechanics',
      summary:
        'Instead of meshing millions of tiny individual lattice walls during stress analysis, Fieldsmith uses homogenisation theory — representing each lattice region by its effective directional stiffness and checking local wall yield and buckling.',
      sections: [
        {
          heading: '6.1 The Gyroid Equation & Stress-Driven Density Grading',
          equations: 'g_gyroid(u) = sin u_x cos u_y + sin u_y cos u_z + sin u_z cos u_x\nρ(x) = ρ_min + (ρ_max − ρ_min) · (σ_vM(x) / σ_ref)^(1 / curve)',
          body: 'A Triply Periodic Minimal Surface (TPMS) is a surface that repeats in X, Y, and Z ("triply periodic") and has zero average curvature everywhere like a soap film ("minimal surface"). By comparing |g_gyroid(u)| against a threshold controlled by the local relative density ρ(x), Fieldsmith smoothly varies wall thickness from thin in lightly loaded zones to thick near fillets and supports.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Switch Internal Lattice to Diamond Sheet via API',
        description:
          'Update the internal fill pattern from Gyroid to Diamond with stress-thickened skin (1.0 to 2.4 mm).',
        tool: 'set_fill',
        arguments: {
          family: 'diamond',
          mode: 'sheet',
          cell: 6.0,
          skin: 1.0,
          skin_max: 2.4,
          rho_min: 0.22,
          rho_max: 0.65,
        },
      },
      {
        title: 'Verify Direct 2D Slicestack Contours via API',
        description:
          'Slice the graded lattice directly into 2D printer layers and verify zero open contour loops.',
        tool: 'export',
        arguments: {
          h: 0.5,
          layer_h: 1.0,
          slices: true,
        },
      },
    ],
  },
  {
    id: 'T7',
    code: 'T7',
    level: 'Advanced',
    title: 'Human + AI Co-Design: Repairing a Weak Part Together in Real Time',
    subtitle:
      'Start with an under-sized arm that both bends past yield and splits between 3D-printed layers, then watch an AI co-pilot repair its thickness, print orientation, and internal lattice live.',
    verificationBadge: 'Lesson 7 · Advanced · Human + AI Co-Pilot',
    starterFsmName: 't7-underdesigned-arm.fsm',
    starterFsm: `fieldsmith 1
name "T7 — Under-Designed Arm (Pre-Synthesis)"
material petg

let h_root = 8
box beam size=(60, 16, h_root) at=(30, 0, 0)

region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load tip_load tip force=(0, 0, -450) case="Peak"

solve size=3.5 build=(90, 0)
`,
    userSteps: [
      {
        title: '1. Diagnose Why the Initial Design Fails',
        lens: 'results',
        instruction:
          'This starter part is an 8 mm thin PETG arm carrying a heavy 450 N tip load, and it is set to print standing on end at (90°, 0°). Tap "Go to Results →" to inspect the damage: the Peak von Mises stress exceeds PETG’s 47 MPa yield strength, and the inter-layer peel utilization is well over 1.0× (meaning the printed layers will split apart before the plastic even yields).',
        expected:
          'The initial 8 mm arm fails both solid yield stress and 3D-printed layer peel.',
      },
      {
        title: '2. Run the 3 AI Co-Pilot Repair Steps in Tab 3 ("3. MCP / REST API")',
        lens: 'fill',
        instruction:
          'Fieldsmith lets an AI assistant connect to your live workspace through the Model Context Protocol (MCP — an open standard that lets AI tools read and edit engineering models safely). Switch to the "3. MCP / REST API" tab above (or look at the 3rd column on desktop) and click the blue "▶ Run" button on Step 1 (thicken h_root from 8 mm to 16 mm), Step 2 (rotate print orientation to 0°, 0°), and Step 3 (carve a stress-graded Gyroid interior with thickened top/bottom skins). Then tap "Go to Fill →" to see the repaired, lightweighted beam inside!',
        expected:
          'Each click executes a live API call and updates your 3D viewport automatically: the arm doubles in height to 16 mm, rotates its print layers flat, and hollows its core with a Gyroid lattice to save 44% weight.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 7 — Shared Human–AI Workspace & Three-Stage Mechanical Synthesis',
      summary:
        'Because Fieldsmith represents every part as a readable text script (.fsm) backed by a fast, deterministic physics solver, a human engineer and an AI agent can collaborate on the exact same model without losing precision.',
      sections: [
        {
          heading: '7.1 Decomposing Structural Co-Design into Three Clear Decisions',
          equations: 'Minimise Mass(p, ρ)   subject to:\n1) Yield Stress ≤ 85%   2) Layer Peel ≤ 60%   3) Lattice Buckling ≤ 50%',
          body: 'Rather than treating design as a black box, a systematic synthesis workflow solves three physical problems in order: (1) size the outer load-bearing dimensions (h_root) so bending stress is safe; (2) align the additive manufacturing layer normal (θ, φ) so tension runs along filaments; and (3) hollow out lightly stressed interior material with a graded lattice while keeping thick outer skins where bending tension and compression peak.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Step 1: Thicken Root Depth h_root to 16 mm',
        description:
          'Doubling beam height increases its bending resistance by 4× (section modulus scales with height²).',
        tool: 'set_let',
        arguments: {
          name: 'h_root',
          value: 16,
        },
      },
      {
        title: 'Step 2: Align 3D-Printed Layers Flat (0°, 0°)',
        description:
          'Rotate the build orientation so continuous filament strands carry the horizontal bending tension.',
        tool: 'set_build',
        arguments: {
          theta: 0,
          phi: 0,
          size: 3.5,
        },
      },
      {
        title: 'Step 3: Carve a Stress-Graded Gyroid Core + Thick Bending Skins',
        description:
          'Replace the low-stress neutral axis inside the beam with a lightweight Gyroid lattice while thickening outer skins up to 2.6 mm.',
        tool: 'set_fill',
        arguments: {
          family: 'gyroid',
          mode: 'sheet',
          cell: 8.0,
          skin: 1.0,
          skin_max: 2.6,
          rho_min: 0.18,
          rho_max: 0.55,
          curve: 1.4,
          min_wall: 0.7,
        },
      },
    ],
  },
  {
    id: 'T8',
    code: 'T8',
    level: 'Flagship Multi-Physics',
    title: 'Coupled Multi-Physics Systems: When Fluid Flow, Heat, Chemistry & Structure Interact',
    subtitle:
      'In real industrial machines — like a molten-tin float-glass bath or a liquid-hydrogen rocket nozzle — fluid flow, heat transfer, and structural bending all affect one another simultaneously.',
    verificationBadge: 'Lesson 8 · Advanced · Coupled Multi-Physics',
    starterFsmName: 't8-uk5-float-bath-twin.fsm',
    opensSystemsWorkbench: true,
    starterFsm: `fieldsmith 1
name "T8 — Multi-Physics Float-Bath & Coater Twin"
material ss316l

let bath_len = 216
let bath_w = 52
let sill_step = 8
let beam_x = 6

box support_pedestal size=(180, 40, 12) at=(0, 0, 6)
color "#475569" on support_pedestal
box refractory_casing size=(bath_len, bath_w, 16) at=(0, 0, 14)
box tin_pool_cavity size=(200, 42, 10) at=(0, 0, 18) subtract
box transition_sill size=(36, 42, sill_step) at=(10, 0, 19)
color "#8c6239" on transition_sill
box ribbon_hot_spread size=(78, 38, 6) at=(-58, 0, 17)
color "#f59e0b" on ribbon_hot_spread
box glass_ribbon size=(142, 26, 5) at=(28, 0, 16.5)
color "#38bdf8" on glass_ribbon
box cvd_coater_beam size=(14, bath_w + 8, 9) at=(beam_x, 0, 24.5)
color "#e05a47" on cvd_coater_beam

region casing_base = support_pedestal.nz
region coater_slot = cvd_coater_beam.pz
region hot_spout = refractory_casing.nx
support fix_base casing_base fixed=xyz
load coater_weight coater_slot force=(0, 0, -28000) case="Coater Thermal & Gravity"
temperature spout_hot hot_spout temp=310
temperature coater_water coater_slot temp=46
solve tref=20
`,
    userSteps: [
      {
        title: '1. What Is a "Coupled Multi-Physics" Problem?',
        lens: 'systems',
        instruction:
          'In Lessons 1–7, we studied a single solid part where forces or temperatures were already known. In a full industrial process — such as floating a 55-metre ribbon of molten glass on a pool of liquid tin, or cooling a rocket engine throat with supercritical hydrogen — multiple laws of physics (governed by Partial Differential Equations, or PDEs) feed back into each other: moving the glass ribbon stirs the liquid tin; tin currents carry heat upstream; temperature changes glass viscosity; and radiant heat bends the overhead coating beam. Tap "Go to Coupled Systems →" to open the interactive Multi-Physics Workbench.',
        expected:
          'The Coupled Systems Workbench opens with design sliders on the left, a 3D multi-physics digital twin in the centre, and live pass/fail performance cards with p10/p50/p90 uncertainty bands on the right.',
      },
      {
        title: '2. How "Adjoint Sensitivities" Tell You Which Design Dial to Turn',
        lens: 'systems',
        instruction:
          'When a complex system has 7 different design dials (sill depth, coater position, hydrogen atmosphere %, linear motor current, etc.), how do you know which dial fixes a defect without running hundreds of trial-and-error simulations? At the bottom of the workbench is the Adjoint Sensitivity Table: using a mathematical technique called the Adjoint Method, Fieldsmith works backward from each defect to calculate the exact influence of every design dial in a single pass. Click "Adjoint Step (−dΦ/dp)" in the top bar to automatically nudge all design dials in the direction that eliminates defects.',
        expected:
          'Clicking "Adjoint Step" updates the design levers using exact gradients and turns red failing objectives green in under 50 milliseconds.',
      },
      {
        title: '3. Switch Domains to a Liquid-Hydrogen Rocket Nozzle & Run 3D Solid FEA',
        lens: 'results',
        instruction:
          'Use the top dropdown in the Coupled Systems Workbench to switch from the Float-Glass Bath to the "[Aerospace & Thermal Propulsion] Supercritical-H₂ Regeneratively Cooled Rocket Chamber". Then click "Load 3D Nozzle into Viewport" (top-right of the workbench) and switch to Results (3) to run a full 3D solid thermal-stress analysis on the copper-alloy chamber and cooling jacket.',
        expected:
          'The 3D rocket chamber loads into the main CAD viewport and solves 3D thermal conduction and hoop stress.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 8 — Coupled Differential Equations, Adjoint Gradients & Uncertainty Bands',
      summary:
        'How can one solver engine handle both glass manufacturing and rocket propulsion, compute exact design gradients in milliseconds, and quantify real-world uncertainty?',
      sections: [
        {
          heading: '8.1 Solving Coupled Equations Together & The Adjoint Shortcut',
          equations: 'Residual Balance: R(W; p) = 0,   Jacobian Matrix: J = ∂R / ∂W\nAdjoint Equation: Jᵀ λ = (∂Φ / ∂W)ᵀ   ⇒   Exact Gradient: dΦ/dp = ∂Φ/∂p − λᵀ (∂R/∂p)',
          body: 'Let W be the vector of all physical unknowns across the machine (velocities, temperatures, chemical concentrations) and p be the design dials we can adjust. Solving R(W; p) = 0 by Newton’s method uses the Jacobian matrix J (how every equation reacts to every state variable). Once J is factored, solving one linear system with its transpose (Jᵀ λ = ∂Φ/∂Wᵀ) gives the sensitivity dΦ/dp of a performance metric Φ with respect to all design dials p at once — whether there are 5 dials or 5,000.',
        },
        {
          heading: '8.2 Propagating Real-World Uncertainty (p10 / p50 / p90 Confidence Bands)',
          equations: 'Variance: σ²(Φ) = ∑ (dΦ / dθ_l)² · σ²(θ_l),   p90 Bound = Φ_nominal + 1.2816 · σ(Φ)',
          body: 'In real engineering, some physical constants θ_l (such as furnace roof insulation aging or rough-wall cooling turbulence) are not known to 4 decimal places. By differentiating the coupled physics with respect to those uncertain parameters θ_l, Fieldsmith computes the 10th, 50th (median), and 90th percentile (p90 worst-case) bands in real time so you can design with a safety margin against real-world variability.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'List Pluggable Multi-Physics Engineering Packs via API',
        description:
          'Query all registered coupled-physics domain models and their design dials and objectives.',
        tool: 'list_coupled_systems',
        arguments: {},
      },
      {
        title: 'Run 2 Adjoint Optimisation Steps on the Float-Bath via API',
        description:
          'Automatically step the float-bath design dials along their Adjoint gradients and sync the 3D CAD model.',
        tool: 'optimise_coupled_system',
        arguments: {
          system_id: 'pilkington_float_bath_cvd_coater_v1',
          steps: 2,
          sync_cad: true,
        },
      },
      {
        title: 'Optimise the Supercritical-H₂ Rocket Nozzle & Load 3D Chamber via API',
        description:
          'Switch to the rocket nozzle pack, run 2 Adjoint optimisation steps, and project the 3D rocket chamber into the viewport.',
        tool: 'optimise_coupled_system',
        arguments: {
          system_id: 'regen_nozzle_h2_v1',
          steps: 2,
          sync_cad: true,
        },
      },
    ],
  },
  {
    id: 'T9',
    code: 'T9',
    title: 'Swirl Atomiser Nozzle, Combustor & Heat Exchanger — Predicting NOx',
    subtitle:
      'Build a multi-part swirl fuel atomiser inside a cooled combustion chamber, and tune nozzle geometry to shrink fuel droplets and cut NOx emissions.',
    level: 'Flagship Multi-Physics',
    verificationBadge: 'Lesson 9 · Multi-Physics · Swirl Spray, Heat Exchanger & NOx',
    starterFsmName: 'T9-swirl-atomiser-combustor-hx.fsm',
    starterFsm: `fieldsmith 1
name "T9 — Swirl Atomiser Nozzle, Combustor & Heat Exchanger"
material ss316l

let orifice_r = 3.2
let swirl_slot_r = 2.4
let swirl_offset = 5.5
let chamber_r = 22
let liner_wall = 5
let hx_r = 3.2
let flame_temp = 665
let coolant_temp = 60

cylinder injector_flange r=34 h=8 at=(0, 0, 4)
color "#64748b" on injector_flange
cylinder combustor_casing r=chamber_r + liner_wall + 8 h=64 at=(0, 0, 38) blend=3
color "#475569" on combustor_casing
cylinder chamber_bore r=chamber_r h=58 at=(0, 0, 43) subtract blend=4
cylinder nozzle_body r=12 h=24 at=(0, 0, 16) blend=2.5
material cucrzr on nozzle_body
color "#d97706" on nozzle_body
cylinder swirl_chamber r=8 h=14 at=(0, 0, 11) subtract blend=1.5
cylinder exit_orifice r=orifice_r h=16 at=(0, 0, 22) subtract
cylinder swirl_port_pos r=swirl_slot_r h=22 at=(0, swirl_offset, 12) rotate=(0, 60, 0) subtract
cylinder swirl_port_neg r=swirl_slot_r h=22 at=(0, -swirl_offset, 12) rotate=(0, -60, 0) subtract
cylinder dilution_jet_x r=3.5 h=80 at=(0, 0, 48) rotate=(0, 90, 0) subtract
cylinder dilution_jet_y r=3.5 h=80 at=(0, 0, 48) rotate=(90, 0, 0) subtract
cylinder hx_chan_px r=hx_r h=56 at=(chamber_r + liner_wall + 3.5, 0, 40) subtract
cylinder hx_chan_nx r=hx_r h=56 at=(-chamber_r - liner_wall - 3.5, 0, 40) subtract
cylinder hx_chan_py r=hx_r h=56 at=(0, chamber_r + liner_wall + 3.5, 40) subtract
cylinder hx_chan_ny r=hx_r h=56 at=(0, -chamber_r - liner_wall - 3.5, 40) subtract

region flange_mount = injector_flange.bottom
region hot_combustor_wall = chamber_bore.side + exit_orifice.side
region hx_coolant_walls = hx_chan_px.side + hx_chan_nx.side + hx_chan_py.side + hx_chan_ny.side
support fix_flange flange_mount fixed=xyz
load combustion_pressure hot_combustor_wall pressure=1.8 case="Combustor Pressure (18 bar)"
temperature flame_zone hot_combustor_wall temp=0 + flame_temp case="Combustor Thermal & HX"
temperature hx_cooling hx_coolant_walls temp=0 + coolant_temp case="Combustor Thermal & HX"
convection hx_convection hx_coolant_walls h=0.015 tinf=0 + coolant_temp case="Combustor Thermal & HX"
solve tref=20
`,
    userSteps: [
      {
        title: '1. Inside a Swirl Atomiser Nozzle & Cooled Combustor Cutaway',
        lens: 'shape',
        instruction:
          'Liquid fuel cannot burn cleanly as a solid stream — it must first be shattered into a fine mist of microscopic droplets (atomisation) so it evaporates and mixes evenly with air. Look at the 3D cutaway in the viewport (sliced across X = 0): at the bottom is the bronze Swirl Atomiser ("nozzle_body"). Two angled tangential fuel ports ("swirl_port_pos" and "swirl_port_neg", offset by "swirl_offset = 5.5 mm") inject fuel off-centre into the internal vortex chamber ("swirl_chamber"). Because the fuel enters tangentially, it spins into a high-speed whirlpool and shoots out through the central orifice ("exit_orifice", radius "orifice_r = 3.2 mm") as a thin hollow cone that breaks up into droplets inside the combustion chamber ("chamber_bore").',
        expected:
          'In the 3D cutaway, you can see the internal swirl chamber, the two angled tangential swirl ports, the central exit orifice, the secondary dilution-air quenching holes, and the 4 vertical heat-exchanger channels bored into the outer combustor jacket.',
      },
      {
        title: '2. Why Droplet Size Controls Thermal NOx — Tuning the Nozzle Live',
        lens: 'shape',
        instruction:
          'When nitrogen (N₂) and oxygen (O₂) in air are heated above ~1530 °C (1800 K), they react to form Nitrogen Oxides (NOx) — a regulated pollutant that causes smog and acid rain. Why does nozzle geometry control NOx? In the interactive NOx Predictor card above, notice that with the wide baseline orifice ("orifice_r = 3.2 mm") and wide swirl slots ("swirl_slot_r = 2.4 mm"), the fuel spins slowly and forms large 54.1 μm droplets (Sauter Mean Diameter, D₃₂). Large droplets take 3.26 ms to evaporate, burning in hot diffusion-flame pockets around each droplet and spiking NOx to 44.3 ppm (exceeding the 25 ppm limit). Click "Apply Low-NOx Fine-Swirl Nozzle (18.5 ppm)" (or drag "orifice_r" down to 2.1 mm and "swirl_offset" up to 7.2 mm) and watch both the 3D nozzle orifice and predicted NOx update live!',
        expected:
          'Tightening orifice_r to 2.1 mm, swirl_slot_r to 1.6 mm, and swirl_offset to 7.2 mm raises the Swirl Number S_g from 1.53 to 2.95, shrinks droplet diameter D₃₂ from 54.1 μm to 29.8 μm, cuts evaporation time to 0.99 ms, and drops NOx from 44.3 ppm to 18.5 ppm (PASS ≤ 25 ppm).',
      },
      {
        title: '3. Setting Up Flame, Pressure & Heat-Exchanger Regions',
        lens: 'setup',
        instruction:
          'Surrounding the combustion chamber are 4 axial cooling channels ("hx_chan_px/nx/py/ny") that act as a regenerative heat exchanger: cool fluid ("coolant_temp = 60 °C") flows through the jacket wall to absorb heat from the liner while dilution jets ("dilution_jet_x/y") quench post-flame gases. Tap "Go to Setup →" (tab 2) and inspect the Regions list: "flange_mount" holds the base flange fixed, "hot_combustor_wall" applies both the 1.8 MPa (18 bar) combustion pressure and the flame-side wall temperature ("flame_temp"), and "hx_coolant_walls" applies convective heat extraction into the heat exchanger. Try clicking the pencil icon (✎) on any region row to rename it inline, or click a face on the 3D cutaway to select its region.',
        expected:
          '3 named regions (flange_mount, hot_combustor_wall, hx_coolant_walls) connect the 3D combustor geometry to the structural support, 1.8 MPa internal pressure load, and heat-exchanger thermal boundary conditions.',
      },
      {
        title: '4. Solving 3D Multi-Material Combustor Liner & CuCrZr Nozzle Thermal Stress',
        lens: 'results',
        instruction:
          'Notice that "nozzle_body" is assigned CuCrZr copper alloy ("material cucrzr on nozzle_body", E = 128 GPa, α = 17.0×10⁻⁶ /K) inside the 316L stainless steel combustor jacket ("material ss316l", E = 193 GPa, α = 16.0×10⁻⁶ /K). Tap "Go to Results →" (tab 3). In the Results panel on the bottom-left, click "Combustor Thermal & HX" in the Case selector, then click "Temperature" in the Show selector to see the steep radial temperature drop from the hot inner combustor liner & CuCrZr nozzle tip (~650–665 °C) across "liner_wall" into the 55–60 °C heat-exchanger channels. Then click "Stress" in the Show selector to inspect the bi-metallic thermal-expansion stress across the CuCrZr nozzle and 316L liner wall.',
        expected:
          'The 3D solver computes the steady-state temperature field between the combustion chamber and the 4 heat-exchanger channels, plus the resulting multi-material thermal and pressure stress fields across the CuCrZr nozzle and SS316L liner.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 9 — Swirl Atomisation Hydrodynamics, D² Droplet Evaporation & Zeldovich Thermal-NOx Kinetics',
      summary:
        'How do the geometric dimensions of a swirl atomiser nozzle (r_orifice, r_slot, R_offset) determine hollow-cone droplet diameter D₃₂, evaporation time, and thermal NOx emissions in a cooled combustor?',
      sections: [
        {
          heading: '9.1 Internal Vortex Flow, Air-Core Film Thickness & Sauter Mean Diameter (D₃₂)',
          equations: 'Swirl Number: S_g = (R_offset · r_orifice) / (n_ports · r_slot²),   Spray Half-Angle: tan(θ) ∝ S_g\nFilm Thickness: t_f ∝ r_orifice^0.62 · ΔP_fuel^(-0.28) · S_g^(-0.42),   Droplet SMD: D₃₂ ∝ t_f^0.38 · ΔP_fuel^(-0.32)',
          body: 'Inside a pressure-swirl atomiser, fuel injected through n_ports tangential slots of radius r_slot at radial offset R_offset conserves angular momentum (r · u_θ = const) as it spirals toward the exit orifice of radius r_orifice. Centrifugal force opens a central air core, pressing the liquid into a thin annular film of thickness t_f against the orifice lip. Exiting into the chamber, tangential velocity fans the sheet into a hollow cone of half-angle θ; aerodynamic Kelvin-Helmholtz waves shred the thinning sheet into ligaments and droplets whose Sauter Mean Diameter D₃₂ (volume-to-surface-area mean diameter) scales directly with film thickness t_f.',
        },
        {
          heading: '9.2 The D² Droplet Evaporation Law & Mixture-Fraction Unmixedness (s_φ)',
          equations: 'D(t)² = D₀² − K_evap · t   ⇒   Evaporation Time: τ_evap = D₃₂² / K_evap\nDamköhler Mixing Ratio: Da_evap = τ_evap / τ_mix   ⇒   Equivalence-Ratio Variance: s_φ = f(Da_evap)',
          body: 'By Godsave and Spalding’s D² law, a spherical droplet of initial diameter D₃₂ shrinks at a constant surface-area regression rate K_evap = (8 k_g / ρ_L c_p,g) ln(1 + B_M). Because τ_evap scales with the square of droplet diameter (D₃₂²), cutting droplet diameter in half (from 56 μm to 28 μm) reduces evaporation time by a factor of 4 (from ~3.4 ms to ~0.85 ms). When τ_evap is short compared to turbulent mixing time τ_mix, fuel vapour premixes uniformly with air at the lean primary-zone equivalence ratio φ_pz before ignition; when droplets are large, each droplet trails a stoichiometric diffusion envelope (φ ≈ 1.0) that burns at peak flame temperature T_st.',
        },
        {
          heading: '9.3 Extended Zeldovich Thermal NOx & Conjugate Heat-Exchanger Wall Cooling',
          equations: 'Zeldovich Chain: O + N₂ ⇌ NO + N,   N + O₂ ⇌ NO + O,   N + OH ⇌ NO + H\nd[NO]/dt = 1.45×10¹⁷ · T^(-1/2) · exp(−69,460 / T) · [O₂]_eq^(1/2) · [N₂]_eq\nConjugate Liner Balance: q″_flame = (k_wall / liner_wall)·(T_wall,hot − T_wall,cold) = h_HX·(T_wall,cold − T_coolant)',
          body: 'Breaking the triple bond of atmospheric N₂ has a huge activation temperature (E_a / R = 69,460 K), making thermal NOx production double for roughly every 35–40 K increase in local flame temperature above 1850 K. Combining fine swirl atomisation (low D₃₂ → small s_φ → uniform lean flame temperature T_eff) with regenerative heat-exchanger wall cooling and secondary dilution jets quenches hot spots and suppresses NOx formation while preheating the working fluid.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Tune Swirl Atomiser to Low-NOx Orifice Geometry via API',
        description:
          'Shrink exit orifice radius orifice_r from 3.2 mm to 2.1 mm to thin the swirling fuel film and halve droplet Sauter Mean Diameter D₃₂.',
        tool: 'set_param',
        arguments: {
          addr: 'orifice_r',
          value: 2.1,
        },
      },
      {
        title: 'Increase Tangential Swirl Offset & Update Liner Flame Temperature via API',
        description:
          'Increase tangential swirl port lever arm swirl_offset to 7.2 mm (raising Swirl Number S_g to 2.95 and cutting predicted NOx from 52 ppm to 16 ppm).',
        tool: 'set_param',
        arguments: {
          addr: 'swirl_offset',
          value: 7.2,
        },
      },
      {
        title: 'Run 3D Combustor & Heat-Exchanger Thermo-Mechanical Solve via API',
        description:
          'Solve the 3D heat conduction and thermal-stress field across the swirl nozzle, combustor liner, and 4 regenerative heat-exchanger channels.',
        tool: 'solve_study',
        arguments: {
          include_fine: false,
        },
      },
    ],
  },
  {
    id: 'T10',
    code: 'T10',
    title: 'Multi-Material Bolted Assembly & Bolt Preload — V53',
    subtitle:
      'Assemble a CuCrZr copper-alloy injector spool bolted to a 316L stainless-steel housing flange, apply axial bolt preload clamping, and evaluate cold clamping vs hot bi-metallic differential expansion.',
    level: 'Flagship Multi-Physics',
    verificationBadge: 'V53 · Multi-Body Assembly · Per-Volume Material & Bolt Preload',
    starterFsmName: 'T10-multibody-bolted-flange-preload.fsm',
    starterFsm: `fieldsmith 1
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
`,
    userSteps: [
      {
        title: '1. Inspecting a Multi-Material Assembly: CuCrZr Spool on an SS316L Flange',
        lens: 'shape',
        instruction:
          'High-heat-flux propulsion and power hardware frequently joins a high-conductivity copper alloy ("CuCrZr", E = 128 GPa, α = 17.0×10⁻⁶ /K, bronze top half) to a structural stainless-steel housing ("SS316L", E = 193 GPa, α = 16.0×10⁻⁶ /K, slate bottom half). In the Model (1) feature tree on the left, click "upper_spool" to open its Node Inspector: notice the "Volume material" dropdown is set to "CuCrZr copper alloy (LPBF)", overriding the document’s default "316L stainless (LPBF)". Each tetrahedron in the 3D mesh automatically inherits the Young’s modulus E_e, thermal expansion α_e, and density ρ_e of the volume containing its centroid.',
        expected:
          'Selecting "upper_spool" or "boss_neck" in the Model tree displays "CuCrZr copper alloy (LPBF)" in the Volume material selector (`[data-testid="node-material"]`), while "lower_housing" uses the document default "316L stainless (LPBF)".',
      },
      {
        title: '2. Configuring Axial Bolt Preload Clamping in Setup (2)',
        lens: 'setup',
        instruction:
          'Before a pressurized joint sees internal pressure, its bolts are torqued to clamp the mating flanges together. Tap "Go to Setup →" (tab 2) and click the load "m8_preload" under Loads. Notice its Type is set to "Preload" with "Preload = 16000 N" on region "washer_seat". A Preload load applies a self-equilibrating clamping traction (16 kN total axial clamping force) distributed across the washer bearing face (or opposing pinch traction across a split bolt bore when a Clamp axis is set). You can edit the Preload (N) value directly in the Inspector to test tighter or looser bolt torque.',
        expected:
          'Clicking "m8_preload" in Setup opens the Load Inspector with Type = "Preload" (`[data-testid="preload"]`) set to 16,000 N alongside the 8.5 MPa internal bore pressure load.',
      },
      {
        title: '3. Solving & Comparing Cold Bolt Preload vs Hot Bi-Metallic Expansion in Results (3)',
        lens: 'results',
        instruction:
          'Tap "Go to Results →" (tab 3) and click "Solve" if results are not yet shown. First inspect Case "1. Cold Bolt Preload + Bore Pressure" to verify how the 16 kN bolt preload compresses the upper CuCrZr flange down onto the SS316L base while resisting the 8.5 MPa bore pressure. Next, switch the Case selector to "2. Hot Bi-Metallic Thermal Mismatch" and toggle between "Temperature" and "Stress": because CuCrZr expands faster per degree than SS316L (α = 17.0 vs 16.0 ppm/K) and runs hotter near the 340 °C bore, differential thermal expansion generates interface shear and hoop stress across the bonded joint.',
        expected:
          'Both load cases solve on the multi-material P2 tetrahedral mesh, reporting the combined CuCrZr + SS316L assembly mass, cold clamping stress/deflection, and hot bi-metallic thermal stress.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 10 — Multi-Body Assembly Mechanics, Per-Element Thermo-Elasticity & Bolt Preload Clamping (V53)',
      summary:
        'How Fieldsmith assigns per-volume constitutive properties (E_e, α_e, ρ_e) across multi-body CSG assemblies and models axial bolt preload clamping alongside bi-metallic thermal mismatch.',
      sections: [
        {
          heading: '10.1 Centroid-Classified Per-Element Constitutive Assembly',
          equations: 'K = ∑_e ∫_(Ω_e) B^T D(E_e, ν) B dΩ,   M_total = ∑_e V_e · ρ_e\nf_th = ∑_e ∫_(Ω_e) B^T D(E_e, ν) · [α_e · (T(x) − T_ref) · I] dΩ',
          body: 'When additive primitives carry distinct `material <id> on <node>` overrides, Fieldsmith evaluates each non-suppressed additive volume’s signed distance field φ_i(c_e) at every P2 tetrahedron centroid c_e. Following CSG last-writer-wins semantics, element e receives the Young’s modulus E_e, thermal expansion coefficient α_e, and mass density ρ_e of the last volume containing c_e. Even under uniform temperature change ΔT, a jump in α_e across an internal material interface produces an unbalanced thermal strain jump Δα · ΔT that drives realistic bi-metallic interface stress.',
        },
        {
          heading: '10.2 Bolt Preload Clamping Traction & Joint Separation Margin',
          equations: 'Single Washer Face: t(x) = −(F_preload / A_washer) · n(x)\nOpposed Bore Pinch (axis â): t(x) = ∓(F_preload / A_±) · â   for   (x − x̄)·â ≷ 0',
          body: 'Torquing a fastener stretches the bolt shank to tension F_preload while clamping the flange washer seat with an equal and opposite compressive traction. In Fieldsmith, `load <id> <region> preload=<N>` integrates the region’s surface area and applies an inward normal clamping traction summing to F_preload Newtons (or, when `axis=(ax, ay, az)` is provided on a shared bore/collar region, splits the region across its axial centroid into opposing ±F_preload pinch tractions that squeeze the upper and lower flanges together).',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Assign CuCrZr Alloy to Upper Spool Volume via API',
        description:
          'Set per-volume material cucrzr on the upper_spool node while keeping the lower housing on ss316l.',
        tool: 'set_material',
        arguments: {
          node: 'upper_spool',
          material: 'cucrzr',
        },
      },
      {
        title: 'Increase Bolt Preload Clamping Force to 22 kN via API',
        description:
          'Update the bolt_preload parameter from 16,000 N to 22,000 N to increase flange clamping compression.',
        tool: 'set_param',
        arguments: {
          addr: 'bolt_preload',
          value: 22000,
        },
      },
      {
        title: 'Run Multi-Material Assembly & Bolt Preload FEA Solve via API',
        description:
          'Solve both the cold bolt-preload case and the hot bi-metallic thermal mismatch case.',
        tool: 'solve_study',
        arguments: {
          include_fine: false,
        },
      },
    ],
  },
  {
    id: 'T11',
    code: 'T11',
    title: 'Spatially Varying Cell-Size & Multi-Zone Lattice Morphing — V54',
    subtitle:
      'Design a structural cold-plate manifold whose interior lattice smoothly refines from coarse 8 mm Schwarz-P cells in low-stress flow zones to fine 4 mm Diamond cells in high-stress load paths.',
    level: 'Flagship Multi-Physics',
    verificationBadge: 'V54 · Multi-Zone TPMS · Cell-Size Grading & Family Morphing',
    starterFsmName: 'T11-multizone-lattice-coldplate.fsm',
    starterFsm: `fieldsmith 1
name "T11 — Multi-Zone Lattice Cold-Plate Bracket (Primitive → Diamond)"
material alsi10mg

box coldplate_core size=(64, 24, 18) at=(32, 0, 0) round=2
cylinder mount_boss_lo r=8 h=18 at=(8, 0, 0) blend=2
cylinder mount_hole_lo r=3.5 h=24 at=(8, 0, 0) subtract
cylinder load_boss_hi r=8 h=18 at=(56, 0, 0) blend=2
cylinder load_hole_hi r=3.5 h=24 at=(56, 0, 0) subtract

region mount_wall = mount_hole_lo.side
region tip_pin = load_hole_hi.side
region hot_face = coldplate_core.pz
region cool_face = coldplate_core.nz

support fix_mount mount_wall fixed=xyz
load actuator_pull tip_pin force=(250, 0, -650) case="1. Structural Bending & Shear"
temperature power_input hot_face temp=185 case="2. Cold-Plate Thermal Conduction"
temperature coolant_sink cool_face temp=35 case="2. Cold-Plate Thermal Conduction"

fill primitive sheet cell=8 cell_min=4 morph=diamond skin=1.2 skin_max=2.2 rho=(0.22, 0.65) curve=1 wall=0.6 protect=(mount_hole_lo, load_hole_hi) margin=2.2
solve size=3.5 tref=20
`,
    userSteps: [
      {
        title: '1. Inspecting a Multi-Zone Lattice: Coarse Primitive → Fine Diamond in Fill (4)',
        lens: 'fill',
        instruction:
          'Why settle for a single unit-cell size or a single TPMS family across an entire component? In thermal-structural hardware like an AlSi10Mg cold-plate bracket, lightly loaded regions benefit from a coarse, open "Primitive" (Schwarz-P) cell ("Cell = 8 mm") for low pressure drop and low mass, whereas high-stress bending/shear zones need a fine, shear-stiff "Diamond" cell ("Min cell = 4 mm", "Morph to = Diamond") with a thickened outer shell ("Skin = 1.2 mm → Max skin = 2.2 mm"). Tap "Go to Fill →" (tab 4): notice the cutaway view immediately shows the smooth, seamless transition from large 8 mm Primitive cells to fine 4 mm Diamond cells without any sharp interface!',
        expected:
          'In Fill (4), Pattern is set to "Primitive", Morph to (`[data-testid="fill-morph"]`) is set to "Diamond", Cell (`[data-testid="fill-cell"]`) is 8 mm, and Min cell (`[data-testid="fill-cell-min"]`) is 4 mm.',
      },
      {
        title: '2. Experimenting Live with "Morph to" & "Min cell" in the Fill Card',
        lens: 'fill',
        instruction:
          'Try clicking "Gyroid" or "Same" in the "Morph to" row (`[data-testid="fill-morph"]`), or adjusting "Min cell" (`[data-testid="fill-cell-min"]`) between 3.5 mm and 8 mm in the Fill card (and notice "Erode = 0 mm" right below Max skin, which we will use in Lesson T12 to evolve the outer boundary level-set). Notice how the WebGL shader updates the 3D cutaway in real time using a single 3D parameter texture: as local grade t(p) rises from 0 to 1, cell size interpolates linearly a(p) = cell + (cell_min − cell)·t(p), sheet wall thickness scales proportionally w(p) = max(param(p)·a(p)/cell, min_wall), and the TPMS surface blends smoothly via mix(s_family, s_morph, t(p)). Press Undo (Ctrl Z) to restore Morph to = Diamond and Min cell = 4 mm.',
        expected:
          'Toggling Morph to and Min cell updates the 3D lattice preview live, and Undo cleanly restores the 8 mm → 4 mm Primitive → Diamond specification.',
      },
      {
        title: '3. Solving Two-Pass Multi-Zone Mechanics & Inspecting 2D Slices in Results (3) & Make (5)',
        lens: 'results',
        instruction:
          'Tap "Go to Results →" (tab 3) to run the two-pass homogenised FEA solve: Pass 1 solves the solid AlSi10Mg bracket under both the 650 N tip load and the 185 °C → 35 °C thermal gradient to map the driving von Mises envelope; Pass 2 builds the 3D grade field, evaluates each element’s local cell size a(c_e) and blended Primitive→Diamond constitutive stiffness, and re-solves every load case. Then switch to Make (5) to verify that direct 2D slicing produces clean, closed printer contours across the entire multi-zone transition.',
        expected:
          'Results (3) reports the lightweighted mass, graded deflection, and thermal conduction across the multi-zone core, and Make (5) slices the variable-cell morphed field with zero open contours.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 11 — Spatially Varying Cell-Size Fields & Multi-Zone TPMS Family Morphing (V54)',
      summary:
        'How Fieldsmith varies unit-cell wavelength and morphs between distinct TPMS topologies (Gyroid, Schwarz-P, Diamond) across a single continuous implicit field with exact CPU/GPU parity.',
      sections: [
        {
          heading: '11.1 Normalized Grade Coordinate & Proportional Wall Scaling',
          equations: 't(p) = clamp((param(p) − p_lo) / (p_hi − p_lo), 0, 1)\na(p) = a_max + (a_min − a_max) · t(p),   w(p) = max(param(p) · a(p) / a_max, w_min)',
          body: 'Because the calibration table maps relative density ρ to wall thickness per unit cell q(ρ) = param_ref / a_max, storing the nominal-cell parameter param(p) = q(ρ(p)) · a_max in the 3D texture u_param guarantees that p_hi > p_lo is strictly monotone whenever ρ_max > ρ_min, even when a_min ≪ a_max. At evaluation time, both the CPU evaluator and the WebGL fragment shader recover the normalized grade t(p) ∈ [0, 1], compute the local cell size a(p), and scale the physical sheet wall thickness w(p) by a(p) / a_max (floored at min_wall w_min) so relative density ρ(p) is preserved accurately across all cell sizes.',
        },
        {
          heading: '11.2 Seamless Multi-Zone TPMS Level-Set Morphing & Homogenised Stiffness Blending',
          equations: 's_blend(p) = (1 − t(p)) · d_TPMS(fam_0, a(p), p) + t(p) · d_TPMS(fam_1, a(p), p)\nE*_blend(φ, t) = (1 − t) · E*_(fam_0)(φ, h / a(c_e)) + t · E*_(fam_1)(φ, h / a(c_e))',
          body: 'Joining two separate lattice meshes with different cell sizes or topologies in traditional CAD leaves disconnected struts and stress concentrations at the seam. Because every TPMS family is represented as a normalized signed distance field d_TPMS = g / |∇g|, convex linear interpolation s_blend(p) = (1 − t) d_0 + t d_1 produces a C¹-continuous minimal-surface transition where struts and sheets bifurcate and merge organically across the functional gradient.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Configure Multi-Zone Primitive → Diamond Lattice with 8 → 4 mm Cell Refinement via API',
        description:
          'Set family=primitive, morph=diamond, cell=8.0, cell_min=4.0, skin=1.2, skin_max=2.2 via set_fill.',
        tool: 'set_fill',
        arguments: {
          family: 'primitive',
          morph: 'diamond',
          mode: 'sheet',
          cell: 8.0,
          cell_min: 4.0,
          skin: 1.2,
          skin_max: 2.2,
          rho_min: 0.22,
          rho_max: 0.65,
        },
      },
      {
        title: 'Run Two-Pass Multi-Zone Graded FEA Solve via API',
        description:
          'Solve the solid driving stress and re-solve the graded Primitive → Diamond cold-plate bracket.',
        tool: 'solve_study',
        arguments: {
          include_fine: false,
        },
      },
    ],
  },
  {
    id: 'T12',
    code: 'T12',
    title: 'Topology Optimization & Level-Set Boundary Evolution — V55',
    subtitle:
      'Evolve the outer boundary level-set inward in low-stress zones (erode=3 mm) while preserving protected bosses, primary load ribs, conformal skin, and multi-zone TPMS infill.',
    level: 'Flagship Multi-Physics',
    verificationBadge: 'V55 · Level-Set Topology Evolution & Conformal Infill',
    starterFsmName: 'T12-levelset-topology-bracket.fsm',
    starterFsm: `fieldsmith 1
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
solve size=3.5
`,
    userSteps: [
      {
        title: '1. Inspecting Level-Set Outer Boundary Evolution in Fill (4)',
        lens: 'fill',
        instruction:
          'Classical lattice infill hollows the inside of a part while keeping the outer CAD box untouched; classical SIMP topology optimization carves the outer shape into jagged voxels without a conformal skin or internal lattice. Fieldsmith unifies both on one implicit field grid via "Erode" (`erode=<mm>`). Tap "Go to Fill →" (tab 4): notice that in low-stress zones (`t → 0`), the outer boundary level-set shifts inward by up to `Erode = 3 mm`, while inside the protected mounting bosses (`mount_boss`, `tip_clevis`) and along peak load paths (`t → 1`), erosion is suppressed to zero so the bosses stand proud at full height with a conformal outer skin (`1.2 → 2.0 mm`) and graded Gyroid → Diamond core.',
        expected:
          'In Fill (4), Erode (`[data-testid="fill-erode"]`) is set to 3 mm, Morph to is "Diamond", Cell is 8 mm, Min cell is 4 mm, and both mounting bosses remain un-eroded.',
      },
      {
        title: '2. Experimenting Live with "Erode" (0 mm ↔ 3.5 mm) & Undo',
        lens: 'fill',
        instruction:
          'In the Fill card, try setting "Erode" (`[data-testid="fill-erode"]`) to `0` mm (full un-eroded box envelope) and then to `3.5` mm (deeper inward level-set carving in low-stress zones). Watch how both the outer surface shader (`fs_outer`) and the interior lattice pass (`fs_core`) update together in real time without mesh boolean failures, because the conformal skin `-(d_outer(p) + skin(p))` is evaluated directly on the evolved signed distance field `d_outer(p) = min(d_body + erode·(1 − t), max(d_body, d_keep))`. Press Undo (Ctrl Z) to restore `Erode = 3 mm`.',
        expected:
          'Adjusting Erode between 0 mm and 3.5 mm smoothly carves or restores the outer envelope around the protected bosses, and Undo restores Erode = 3 mm.',
      },
      {
        title: '3. Fixed-Grid Ersatz-Void FEA in Results (3) & Direct Slicing in Make (5)',
        lens: 'results',
        instruction:
          'Tap "Go to Results →" (tab 3) to run the two-pass solve across both load cases ("1. Peak Actuator Shear" and "2. Lateral Gust + Shear"). In Pass 2, elements whose centroids lie outside the evolved outer boundary `d_outer(p) > 0` transition to `CLASS_VOID` (`φ = 0`, carried on the fixed background mesh by the ersatz stiffness floor `E_void = 10⁻⁶ E_s`), while interior elements use the blended Gyroid→Diamond homogenised law. Then switch to Make (5) to verify watertight 2D slice contours across the evolved outer boundary and internal lattice.',
        expected:
          'Results (3) reports the additional mass savings from outer boundary evolution alongside graded deflection and stress across both cases, and Make (5) slices the evolved geometry cleanly.',
      },
    ],
    theoryChapter: {
      chapterTitle: 'Chapter 12 — Level-Set Outer Boundary Evolution & Fixed-Grid Topology Optimization (V55)',
      summary:
        'How Fieldsmith evolves the outer boundary level-set directly on the implicit field grid while preserving protected functional interfaces, conformal skin, and graded TPMS infill.',
      sections: [
        {
          heading: '12.1 Stress-Driven Level-Set Shift & Protected Interface Clamping',
          equations: 'δ(p) = erode · (1 − t(p)),   t(p) = clamp((param(p) − p_lo) / (p_hi − p_lo), 0, 1)\nd_outer(p) = min(d_body(p) + δ(p),  max(d_body(p), d_keep(p)))',
          body: 'Let d_body(p) be the signed distance field of the authored design envelope and d_keep(p) be the union of protected shapes offset outward by margin. Shifting d_body(p) by δ(p) = erode · (1 − t(p)) ≥ 0 moves the zero level-set inward by erode mm in low-stress zones (t = 0) and leaves peak load paths (t = 1) at the original envelope. Intersecting the protection clamp max(d_body, d_keep) via min(...) guarantees that inside any protected boss or bore (d_keep < 0) the outer surface is identically d_body(p) = 0, with a C⁰-continuous ramp across the protection margin.',
        },
        {
          heading: '12.2 Conformal Skin Composition & Fixed-Mesh Ersatz-Material FEA',
          equations: 'd_filled(p) = max(d_outer(p),  min(−(d_outer(p) + skin(p)),  d_lattice(p),  d_keep(p)))\nE_e = E_s · max(10⁻⁶,  E*_blend(φ_e, h / a(c_e)))',
          body: 'Because the inward shell offset −(d_outer(p) + skin(p)) is evaluated on the evolved outer distance field d_outer(p) rather than the un-eroded box d_body(p), the solid outer skin automatically wraps the newly evolved topology boundary at full thickness skin(p) ∈ [skin, skin_max]. During Pass 2 FEA, jittered Dirichlet quadrature samples d_filled(pt) < 0 over each P2 tetrahedron on the fixed background mesh: eroded exterior elements yield φ_e = 0 (CLASS_VOID, weighted by the ersatz floor 10⁻⁶ E_s), avoiding remeshing failures while capturing the true structural compliance of the evolved geometry.',
        },
      ],
    },
    mcpSteps: [
      {
        title: 'Configure Level-Set Outer Boundary Evolution (erode=3.0 mm) via API',
        description:
          'Set family=gyroid, morph=diamond, cell=8.0, cell_min=4.0, skin=1.2, skin_max=2.0, erode=3.0 via set_fill.',
        tool: 'set_fill',
        arguments: {
          family: 'gyroid',
          morph: 'diamond',
          mode: 'sheet',
          cell: 8.0,
          cell_min: 4.0,
          skin: 1.2,
          skin_max: 2.0,
          erode: 3.0,
          rho_min: 0.2,
          rho_max: 0.65,
        },
      },
      {
        title: 'Run Two-Pass Level-Set Topology + Lattice FEA Solve via API',
        description:
          'Solve Pass 1 driving stress envelope and Pass 2 level-set evolved + multi-zone lattice mechanics.',
        tool: 'solve_study',
        arguments: {
          include_fine: false,
        },
      },
    ],
  },
  {
    id: 'T13',
    code: 'T13',
    title: 'Solar-Focused Optical Lens Candle Ignition, Buoyant Flame, Smoke Plume & Real-Time Video',
    subtitle:
      'ParaView-Style 3D Volumetric Ray-Marched Radiant Flame, BK7 Solar Caustic Refraction, C₂₅H₅₂ Pyrolysis, Soot/Smoke Plume, Transient Recap & .webm Video Synthesis',
    level: 'Flagship Multi-Physics',
    verificationBadge: 'V56 Verified (144-DOF Monolithic Newton · 40 Transient Frames · 5 Recap Milestones)',
    starterFsmName: 't13-solar-candle-ignition.fsm',
    opensSystemsWorkbench: false,
    starterFsm: `fieldsmith 1
name "T13 — Solar-Focused Optical Lens Candle Ignition & Buoyant Flame"
material alsi10mg

let lens_r = 32.5
let lens_z = 111.5
let wick_len = 7.5

cylinder brass_base r=24 h=6 at=(0, 0, 3)
color "#d4a84f" on brass_base

cylinder candle_column r=16 h=34 at=(0, 0, 20)
color "#f5efe2" on candle_column
sphere molten_wax_cup r=13 at=(0, 0, 39) subtract

cylinder candle_wick r=2.2 h=18 at=(0, 0, 35)
color "#3a2e28" on candle_wick

box mast_bracket size=(8, 44, 6) at=(0, -21, 3) round=1.0
color "#6d7888" on mast_bracket

box support_mast size=(6, 8, lens_z) at=(0, -42, 0.5 * lens_z) round=1.5
color "#6d7888" on support_mast

box lens_arm size=(6, 42, 5) at=(0, -21, lens_z) round=1.0
color "#6d7888" on lens_arm

cylinder lens_gimbal_outer r=37.0 h=5 at=(0, 0, lens_z)
color "#7b889a" on lens_gimbal_outer
cylinder lens_gimbal_bore r=lens_r h=8 at=(0, 0, lens_z) subtract

cylinder optical_glass_lens r=lens_r h=6 at=(0, 0, lens_z)
color "#88c8ff" on optical_glass_lens

material cucrzr on brass_base

region base_pad = brass_base.bottom
region wick_hot_zone = candle_wick.side
region candle_wall = candle_column.side
region lens_top_face = optical_glass_lens.top

support clamp_base base_pad fixed=xyz
load lens_gravity lens_top_face force=(0, 0, -12) case="1. Solar Caustic Ignition & Thermal Plume"
temperature wick_ignition wick_hot_zone temp=397
convection ambient_plume candle_wall h=18 tinf=22
heatflux solar_lens_heating lens_top_face q=0.015

solve size=3.5 tref=22
`,
    userSteps: [
      {
        title: '1. ParaView-Style 3D Volumetric Radiant Flame, Solar Caustic Cone & Smoke Plume in the Main Viewport',
        lens: 'shape',
        instruction:
          'Loading T13 places the 9-body 3D Solar-Focused Candle Assembly (brass base, paraffin column, concave molten wax cup, braided cotton wick, and overhead Ø 65 mm biconvex BK7 optical glass lens) in the main 3D viewport alongside the live "3D Volumetric Solar Candle Flame & Plume" HUD (`[data-testid="coupled-viewport-hud"]`). Switch the Volumetric Transfer Function tabs (`Radiant Flame`, `Temp T_gas`, `Soot / Smoke f_v`, `Blue OH*`, `Updraft u_z`) to inspect the blue OH*/CH* chemiluminescence base sheet, the incandescent Planckian soot core (`~2100 K`), the Beer–Lambert absorbing grey smoke plume aloft, the golden converging solar caustic ray cone (`C ≈ 2355×`), and the buoyant convective entrainment streamlines.',
        expected:
          'The 3D viewport renders the volumetric radiant flame, blue OH* base, smoke plume, solar caustic cone, and buoyant airflow directly over the 3D candle and lens.',
      },
      {
        title: '2. Scrubbing Time-Dependent Results, Milestone Recap Cards & Real-Time .webm Video Generation',
        lens: 'systems',
        instruction:
          'Either directly in the 3D Viewport HUD or by tapping "Go to Systems →" (`[data-testid="systems-domain-select"]` → `solar_candle_ignition_v1`), use the Transient Timeline Scrubber (`[data-testid="transient-time-slider"]`, 40 frames from t = 0.00 s to 5.50 s) and click the 5 Milestone Recap Cards (`[data-testid="recap-event-0"]` .. `4`: 1. Solar Caustic Focus → 2. Wax Pool Melting at 331 K → 3. Pyrolysis Vapor Plume at 620 K → 4. Flash Auto-Ignition at 670 K → 5. Steady Buoyant Diffusion Flame & Smoke Plume). Then click "🎬 Render Real-Time Video (.webm)" (`[data-testid="render-transient-video"]`) to record a real-time 30 fps WebM video with embedded scientific telemetry HUD.',
        expected:
          'Clicking any milestone recap card jumps the 3D volumetric scene to that physical regime, and "Render Real-Time Video" produces an inline playable `.webm` video (`[data-testid="transient-video-player"]`) + download link.',
      },
      {
        title: '3. ParaView Colourmap Transfer Functions & 3D Volumetric Iso-Shells in Results (3)',
        lens: 'results',
        instruction:
          'Tap "Go to Results →" (tab 3) to solve the 3D thermo-mechanical FEA study on the candle, wick, and optical lens assembly. Switch to "Temperature" or "Stress", select the new ParaView "Flame" (Blackbody) or "Turbo" colourmap (`[data-testid="results-colormap"]`), and toggle "3D Volume" (`[data-testid="results-volume-toggle"]`) to render 5 nested additive volumetric iso-shells highlighting the thermal plume and stress gradients inside the 3D mesh.',
        expected:
          'In Results (3), the Flame/Turbo colourmaps and 3D Volume iso-shells render translucent glowing scalar contours across the 3D FEA mesh.',
      },
    ],
    theoryChapter: {
      chapterTitle:
        'Chapter 13 — Solar Caustic Optics, Paraffin Diffusion Flame Chemistry, Buoyant Plume & Volumetric Rendering (V56)',
      summary:
        'Coupled thick-lens solar refraction, C₂₅H₅₂ phase change and capillary pyrolysis, Burke–Schumann mixture-fraction combustion, OH* chemiluminescence, Moss–Brookes soot/smoke kinetics, and ParaView emission-absorption volume rendering.',
      sections: [
        {
          heading: '13.1 Thick-Lens BK7 Solar Caustic & Wick Auto-Ignition Delay',
          equations:
            '1 / f = (n_BK7 − 1) · [ 2 / R_lens − (n_BK7 − 1) · t_c / (n_BK7 · R_lens²) ]\nw(Δz) = √( (f · θ_sun)² + σ_sph² + (D_lens · |z_lens − f| / (2 f))² ),   C_opt = τ_opt · (D_lens / (2 w(Δz)))²',
          body: 'Sunlight subtends a half-angle θ_sun = 4.65 mrad (0.266°). Refracted through a biconvex BK7 borosilicate crown glass lens (n = 1.5168), the beam converges to a caustic waist w(0) limited by the solar disk image and spherical aberration σ_sph, achieving optical concentrations C_opt > 2000× and peak focal fluxes > 4000 kW/m². Axial defocus Δz = z_lens − f expands the spot radius conically, reducing absorbed power on the braided cotton wick and lengthening the time to cross T_melt = 331 K, T_vap = 620 K, and T_ign = 670 K.',
        },
        {
          heading: '13.2 Burke–Schumann Mixture Fraction, Blue OH* Base & Soot/Smoke Plume',
          equations:
            'C₂₅H₅₂ + 38 O₂ → 25 CO₂ + 26 H₂O   (Z_st = 0.0637 in air)\nu_z(z) = √( u_0² + 2 g β ∫ (T_cl(ζ) − T_∞) dζ ),   d(r_f² u_z)/dz = 2 α_ent r_f u_z',
          body: 'Once ignited, radiant feedback melts a concave liquid paraffin pool that feeds the wick by Darcy–Lucas–Washburn capillary suction. Vaporized C₂₅H₅₂ mixes with radially entrained air (α_ent ≈ 0.115); along the stoichiometric reaction sheet Z(r, z) = Z_st, excited OH*/CH* radicals emit royal-blue chemiluminescence (431 nm / 308 nm) near the oxygen-rich base. Higher in the fuel-rich interior (0.08 < Z < 0.25, T > 1300 K), polycyclic aromatic hydrocarbons nucleate carbonaceous soot (f_v ~ 1–4 ppm) whose Planckian T⁴ incandescence produces the warm yellow-white candle glow. Unoxidized soot escaping the tip cools into the dark Beer–Lambert absorbing smoke plume.',
        },
      ],
    },
    mcpSteps: [
      {
        title: '1. Solve Defocused Solar Candle Scenario (Slow Ignition & Heavy Smoke) via MCP',
        description:
          'Evaluate Scenario B (D_lens = 55 mm, z_lens = 92 mm defocused, L_wick = 12 mm long wick) and sync its 3D .fsm CAD twin.',
        tool: 'solve_coupled_system',
        arguments: {
          system_id: 'solar_candle_ignition_v1',
          design_p: [55.0, 68.0, 92.0, 780.0, 12.0, 0.21],
          sync_cad: true,
        },
      },
      {
        title: '2. Run 3 Sparse Adjoint (−dΦ/dp) Optimisation Steps to Refocus Lens & Suppress Smoke',
        description:
          'Use exact sparse Adjoint gradients dΦ/dp across all 144 DOFs to refocus the optical lens, enlarge collection aperture, and trim wick smoke.',
        tool: 'optimise_coupled_system',
        arguments: {
          system_id: 'solar_candle_ignition_v1',
          steps: 3,
          sync_cad: true,
        },
      },
    ],
  },
];

export function loadTutorialSheetModel(sheetId: string, openSystemsIfFlagged = false) {
  const sheet = TUTORIAL_SHEETS.find((s) => s.id === sheetId) ?? TUTORIAL_SHEETS[0];
  ui.set('bodyAlpha', 1);
  if (sheet.id === 'T9' || sheet.id === 'T10') {
    ui.set('clip', { axis: 0, dir: 1, on: true, t: 0 });
  } else {
    ui.set('clip', { axis: 1, dir: 1, on: false, t: 0 });
  }
  const ok = openText(sheet.starterFsm, sheet.starterFsmName);
  viewApi.fit(false);
  if (openSystemsIfFlagged && sheet.opensSystemsWorkbench) {
    ui.set('systemsOpen', true);
  } else {
    ui.set('systemsOpen', false);
    store.setLens('shape');
  }
  if (ok) {
    ui.notify(`Loaded ${sheet.code}: ${sheet.title}`);
    void fetch('/api/workspace', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        actor: 'web-ui',
        last_tool: `load-${sheet.code}`,
        summary: `Loaded ${sheet.code}: ${sheet.title}`,
        doc_fsm: printFsm(store.doc),
      }),
    })
      .then((r) => r.json())
      .then((ws) => {
        if (typeof ws?.revision === 'number') markWorkspaceRevisionSeen(ws.revision);
      })
      .catch(() => {});
  }
}

export function toggleTutorialsDrawer() {
  const nextOpen = !ui.tutorialsOpen;
  ui.set('tutorialsOpen', nextOpen);
  if (nextOpen) {
    const activeSheet = TUTORIAL_SHEETS.find((s) => s.id === ui.tutorialSheetId) ?? TUTORIAL_SHEETS[0];
    // If the viewport is empty, holds the Float-Bath from a previous session, or doesn't match the active tutorial, load the active tutorial sheet immediately.
    const curName = store.doc.name || '';
    if (
      store.doc.root.length === 0 ||
      (curName.includes('Float-Bath') && activeSheet.id !== 'T8') ||
      !curName.startsWith(activeSheet.code)
    ) {
      loadTutorialSheetModel(activeSheet.id, false);
    }
  }
}

function computeSwirlCombustorNox(params: {
  orificeR: number;
  swirlSlotR: number;
  swirlOffset: number;
  linerWall: number;
  coolantTemp: number;
  fuelPressureBar: number;
  equivRatio: number;
}) {
  const { orificeR, swirlSlotR, swirlOffset, linerWall, coolantTemp, fuelPressureBar, equivRatio } = params;
  // Geometric Swirl Number S_g = (R_offset * r_orifice) / (n_ports * r_slot^2) with n_ports = 2
  const swirlNumber = (swirlOffset * orificeR) / Math.max(0.5, 2.0 * swirlSlotR * swirlSlotR);
  // Hollow-cone spray half-angle (deg)
  const sprayHalfAngleDeg = Math.min(72.0, Math.max(22.0, 18.0 + 23.5 * Math.pow(swirlNumber, 0.56)));
  // Air-core liquid film thickness at the orifice lip (um)
  const filmThicknessUm =
    295.0 *
    Math.pow(orificeR / 2.6, 0.62) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.28) *
    Math.pow(1.65 / Math.max(0.35, swirlNumber), 0.42);
  // Lefebvre Sauter Mean Diameter D_32 (um)
  const smdUm =
    46.5 *
    Math.pow(filmThicknessUm / 255.0, 0.38) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.32) *
    Math.pow(orificeR / 2.6, 0.28) *
    Math.pow(Math.cos((sprayHalfAngleDeg * Math.PI) / 360.0), 0.25);
  // D^2-law droplet evaporation time tau_evap = D_32^2 / K_evap (ms, with K_evap = 0.90 mm^2/s)
  const evapTimeMs = (smdUm * smdUm * 1e-3) / 0.9;
  // Mixture-fraction unmixedness s_phi driven by slow droplet evaporation
  const unmixedness = Math.min(0.38, 0.02 + 0.085 * Math.pow(evapTimeMs, 0.95));
  // Homogeneous lean adiabatic flame temp vs stoichiometric diffusion-pocket temp (2350 K)
  const tAdiabaticK = 1280.0 + 1020.0 * equivRatio - 260.0 * Math.pow(Math.max(0, equivRatio - 1.02), 2);
  const diffWeight = Math.min(0.75, 1.95 * unmixedness);
  const hxCoolingDropK = 22.0 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.35) * ((180.0 - coolantTemp) / 120.0);
  const tFlameEffK = (1.0 - diffWeight) * tAdiabaticK + diffWeight * 2350.0 - hxCoolingDropK;
  // Conjugate hot-side combustor liner wall temperature (deg C)
  const linerHotWallC = Math.round(
    coolantTemp + (tFlameEffK - 273.15 - coolantTemp) / (1.0 + 1.85 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.42)),
  );
  // Extended Zeldovich Thermal NOx + Fenimore Prompt NOx (ppm @ 15% O2)
  const promptNoxPpm = 3.6 * Math.pow(equivRatio, 1.3);
  const o2RadicalFactor = Math.sqrt(Math.max(0.015, 1.08 - equivRatio));
  const residenceMs = 3.2;
  const thermalNoxPpm =
    6.8e11 * (residenceMs * 1e-3) * o2RadicalFactor * Math.exp(-38200.0 / Math.max(1200.0, tFlameEffK));
  const totalNoxPpm = promptNoxPpm + thermalNoxPpm;
  // Heat-exchanger thermal recovery power (kW)
  const hxPowerKw = 14.5 + 0.052 * (linerHotWallC - coolantTemp) * Math.pow(5.0 / Math.max(2.0, linerWall), 0.3);

  return {
    swirlNumber,
    sprayHalfAngleDeg,
    filmThicknessUm,
    smdUm,
    evapTimeMs,
    tFlameEffK,
    linerHotWallC,
    totalNoxPpm,
    hxPowerKw,
  };
}

function SwirlAtomiserNoxPredictor() {
  useApp();
  const [fuelPressureBar, setFuelPressureBar] = useState<number>(12.0);
  const [equivRatio, setEquivRatio] = useState<number>(0.88);

  const getLet = (name: string, fallback: number): number => {
    const found = (store.doc.lets ?? []).find((l) => l.name === name);
    return found && Number.isFinite(found.value) ? found.value : fallback;
  };

  const orificeR = getLet('orifice_r', 3.2);
  const swirlSlotR = getLet('swirl_slot_r', 2.4);
  const swirlOffset = getLet('swirl_offset', 5.5);
  const linerWall = getLet('liner_wall', 5.0);
  const coolantTemp = getLet('coolant_temp', 60.0);

  const metrics = computeSwirlCombustorNox({
    orificeR,
    swirlSlotR,
    swirlOffset,
    linerWall,
    coolantTemp,
    fuelPressureBar,
    equivRatio,
  });

  const applyNozzleLet = (updates: Record<string, number>, nextPBar = fuelPressureBar, nextPhi = equivRatio) => {
    const nextOrifice = updates.orifice_r ?? orificeR;
    const nextSlot = updates.swirl_slot_r ?? swirlSlotR;
    const nextOffset = updates.swirl_offset ?? swirlOffset;
    const nextWall = updates.liner_wall ?? linerWall;
    const nextCool = updates.coolant_temp ?? coolantTemp;
    const nextMetrics = computeSwirlCombustorNox({
      orificeR: nextOrifice,
      swirlSlotR: nextSlot,
      swirlOffset: nextOffset,
      linerWall: nextWall,
      coolantTemp: nextCool,
      fuelPressureBar: nextPBar,
      equivRatio: nextPhi,
    });
    const allUpdates: Record<string, number> = {
      ...updates,
      flame_temp: nextMetrics.linerHotWallC,
    };
    store.transact('Tune swirl atomiser & NOx parameters', (d) => {
      const pairs: [ReturnType<typeof parseAddr>, number][] = [];
      for (const [k, v] of Object.entries(allUpdates)) {
        if ((d.lets ?? []).some((l) => l.name === k)) {
          pairs.push([parseAddr(k), v]);
        }
      }
      if (pairs.length > 0) {
        applyAddrs(d, pairs, true);
      }
    });
  };

  const noxPass = metrics.totalNoxPpm <= 25.0;

  return (
    <div class="tut-step-card" data-testid="t9-nox-predictor" style={{ borderColor: 'var(--accent)' }}>
      <div class="tut-step-top">
        <strong>Interactive Swirl Atomiser Spray &amp; NOx Predictor (Synced to 3D CAD)</strong>
        <span
          class={`num ${noxPass ? 'ok-pill' : 'bad-pill'}`}
          data-testid="t9-nox-badge"
          style={{
            padding: '2px 7px',
            borderRadius: '4px',
            fontWeight: 700,
            fontSize: '11px',
            background: noxPass ? 'color-mix(in srgb, var(--ok) 18%, transparent)' : 'color-mix(in srgb, var(--danger) 18%, transparent)',
            color: noxPass ? 'var(--ok)' : 'var(--danger)',
          }}
        >
          NOx: {metrics.totalNoxPpm.toFixed(1)} ppm {noxPass ? '(PASS ≤ 25 ppm)' : '(HIGH > 25 ppm)'}
        </span>
      </div>

      <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap', margin: '2px 0' }}>
        <button
          type="button"
          class="btn small"
          data-testid="t9-preset-coarse"
          onClick={() => {
            setFuelPressureBar(12.0);
            setEquivRatio(0.88);
            applyNozzleLet({ orifice_r: 3.2, swirl_slot_r: 2.4, swirl_offset: 5.5, liner_wall: 5.0, coolant_temp: 60 }, 12.0, 0.88);
          }}
        >
          Baseline Coarse Spray (44.3 ppm)
        </button>
        <button
          type="button"
          class="btn small primary"
          data-testid="t9-preset-low-nox"
          onClick={() => {
            setFuelPressureBar(22.0);
            setEquivRatio(0.78);
            applyNozzleLet({ orifice_r: 2.1, swirl_slot_r: 1.6, swirl_offset: 7.2, liner_wall: 4.2, coolant_temp: 55 }, 22.0, 0.78);
          }}
        >
          Apply Low-NOx Fine-Swirl Nozzle (18.5 ppm)
        </button>
      </div>

      {/* Live Multi-Physics Readout Strip */}
      <div
        class="num"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(115px, 1fr))',
          gap: '6px',
          padding: '6px 8px',
          borderRadius: '5px',
          background: 'var(--panel)',
          border: '1px solid var(--line)',
          fontSize: '11px',
        }}
      >
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Swirl Number S_g</div>
          <strong>{metrics.swirlNumber.toFixed(2)}</strong> ({metrics.sprayHalfAngleDeg.toFixed(0)}° cone)
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Droplet SMD (D₃₂)</div>
          <strong data-testid="t9-smd-readout">{metrics.smdUm.toFixed(1)} μm</strong>
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Evap Time (τ_evap)</div>
          <strong>{metrics.evapTimeMs.toFixed(2)} ms</strong>
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Flame Zone T_eff</div>
          <strong>{metrics.tFlameEffK.toFixed(0)} K</strong> ({(metrics.tFlameEffK - 273.15).toFixed(0)} °C)
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Liner Wall / HX</div>
          <strong>{metrics.linerHotWallC} °C</strong> ({metrics.hxPowerKw.toFixed(1)} kW)
        </div>
      </div>

      {/* Interactive Nozzle & Combustor Sliders */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: '6px 12px', fontSize: '11px' }}>
        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Orifice Radius (orifice_r)</span>
            <strong class="num">{orificeR.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="1.8"
            max="4.4"
            step="0.1"
            value={orificeR}
            data-testid="t9-slider-orifice"
            onInput={(e) => applyNozzleLet({ orifice_r: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Swirl Port Radius (swirl_slot_r)</span>
            <strong class="num">{swirlSlotR.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="1.3"
            max="3.0"
            step="0.1"
            value={swirlSlotR}
            data-testid="t9-slider-slot"
            onInput={(e) => applyNozzleLet({ swirl_slot_r: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Swirl Lever Arm (swirl_offset)</span>
            <strong class="num">{swirlOffset.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="3.5"
            max="8.0"
            step="0.1"
            value={swirlOffset}
            data-testid="t9-slider-offset"
            onInput={(e) => applyNozzleLet({ swirl_offset: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Fuel Pressure (ΔP_fuel)</span>
            <strong class="num">{fuelPressureBar.toFixed(0)} bar</strong>
          </div>
          <input
            type="range"
            min="6"
            max="32"
            step="1"
            value={fuelPressureBar}
            data-testid="t9-slider-pressure"
            onInput={(e) => {
              const p = parseFloat((e.target as HTMLInputElement).value);
              setFuelPressureBar(p);
              applyNozzleLet({}, p, equivRatio);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Primary Equivalence Ratio (φ_pz)</span>
            <strong class="num">{equivRatio.toFixed(2)}</strong>
          </div>
          <input
            type="range"
            min="0.65"
            max="1.05"
            step="0.01"
            value={equivRatio}
            data-testid="t9-slider-phi"
            onInput={(e) => {
              const phi = parseFloat((e.target as HTMLInputElement).value);
              setEquivRatio(phi);
              applyNozzleLet({}, fuelPressureBar, phi);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Liner Wall Thickness (liner_wall)</span>
            <strong class="num">{linerWall.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="3.0"
            max="7.0"
            step="0.2"
            value={linerWall}
            data-testid="t9-slider-wall"
            onInput={(e) => applyNozzleLet({ liner_wall: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>
      </div>
    </div>
  );
}

type ColumnTab = 'user' | 'theory' | 'mcp';

function TutorialsDrawer() {
  useApp();
  const [activeTab, setActiveTab] = useState<ColumnTab>('user');
  const [threeColMode, setThreeColMode] = useState<boolean>(false);
  const [splitMobile, setSplitMobile] = useState<boolean>(false);
  const [mcpLog, setMcpLog] = useState<Record<string, string>>({});
  const [runningKey, setRunningKey] = useState<string | null>(null);

  if (!ui.tutorialsOpen) return null;

  const sheet = TUTORIAL_SHEETS.find((s) => s.id === ui.tutorialSheetId) ?? TUTORIAL_SHEETS[0];

  const loadScenarioModel = () => {
    loadTutorialSheetModel(sheet.id, true);
  };

  const jumpToLens = (target?: Lens | 'systems') => {
    if (!target) return;
    if (target === 'systems') {
      ui.set('systemsOpen', true);
    } else {
      ui.set('systemsOpen', false);
      store.setLens(target);
      if (target === 'fill' && store.doc.fill && ui.bodyAlpha >= 0.99) {
        ui.set('bodyAlpha', 0.35);
        ui.set('clip', { ...ui.clip, axis: 1, on: true, dir: 1, t: 0.5 });
      }
    }
  };

  const runLiveMcpStep = async (step: McpFlowStep, idx: number) => {
    const key = `${sheet.id}-${idx}`;
    setRunningKey(key);
    try {
      // Ensure the current live document is pushed to `/api/workspace` first so `fieldsmith-mcp` operates on it cumulatively
      const currentFsm = store.doc.root.length > 0 ? printFsm(store.doc) : sheet.starterFsm;
      await fetch('/api/workspace', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          actor: 'web-ui',
          last_tool: 'tutorial-sync',
          summary: `Synced before ${step.tool}`,
          doc_fsm: currentFsm,
        }),
      });
      const resp = await fetch('/api/mcp', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          tool: step.tool,
          arguments: step.arguments,
        }),
      });
      const data = await resp.json();
      const textPayload = data?.rpc?.result?.content?.[0]?.text;
      let formatted = JSON.stringify(data?.rpc ?? data, null, 2);
      if (typeof textPayload === 'string') {
        try {
          formatted = JSON.stringify(JSON.parse(textPayload), null, 2);
        } catch {
          formatted = textPayload;
        }
      }
      setMcpLog((prev) => ({ ...prev, [key]: formatted.slice(0, 1200) }));
      if (typeof data?.workspace?.revision === 'number') {
        markWorkspaceRevisionSeen(data.workspace.revision);
      }
      if (typeof data?.workspace?.doc_fsm === 'string' && data.workspace.doc_fsm.trim().length > 0) {
        openText(data.workspace.doc_fsm, `${sheet.id.toLowerCase()}-mcp.fsm`);
        viewApi.fit(false);
      }
      ui.notify(`Executed MCP ${step.tool} (Workspace rev #${data?.workspace?.revision ?? '?'})`);
    } catch (e) {
      setMcpLog((prev) => ({ ...prev, [key]: `Error: ${e instanceof Error ? e.message : String(e)}` }));
    } finally {
      setRunningKey(null);
    }
  };

  const renderUserColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-user">
      <div class="tut-col-head">1. Step-by-Step User Tutorial</div>
      {sheet.id === 'T9' && <SwirlAtomiserNoxPredictor />}
      {sheet.userSteps.map((st, i) => (
        <div key={i} class="tut-step-card">
          <div class="tut-step-top">
            <strong>{st.title}</strong>
            {st.lens && (
              <button
                type="button"
                class="btn small"
                data-testid={`tutorial-jump-${i}`}
                onClick={() => jumpToLens(st.lens)}
              >
                Go to {st.lens === 'systems' ? 'Coupled Systems' : `${st.lens[0].toUpperCase()}${st.lens.slice(1)}`} →
              </button>
            )}
          </div>
          <p class="tut-step-text">{st.instruction}</p>
          <div class="tut-expected">
            <span>Expected Check:</span> {st.expected}
          </div>
        </div>
      ))}
    </div>
  );

  const renderTheoryColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-theory">
      <div class="tut-col-head">2. Theoretical Manual Chapter</div>
      <div class="tut-theory-card">
        <h4>{sheet.theoryChapter.chapterTitle}</h4>
        <p class="tut-step-text">{sheet.theoryChapter.summary}</p>
        {sheet.theoryChapter.sections.map((sec, i) => (
          <div key={i} class="tut-theory-sec">
            <div class="tut-theory-sec-title">{sec.heading}</div>
            <pre class="tut-eq num">{sec.equations}</pre>
            <p class="tut-step-text">{sec.body}</p>
          </div>
        ))}
      </div>
    </div>
  );

  const renderMcpColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-mcp">
      <div class="tut-col-head">3. AI &amp; API Automation (MCP / REST)</div>
      {sheet.mcpSteps.map((ms, i) => {
        const key = `${sheet.id}-${i}`;
        const out = mcpLog[key];
        return (
          <div key={i} class="tut-mcp-card">
            <div class="tut-step-top">
              <strong>{ms.title}</strong>
              <button
                type="button"
                class="btn small primary"
                data-testid={`tutorial-run-mcp-${i}`}
                disabled={runningKey === key}
                onClick={() => runLiveMcpStep(ms, i)}
              >
                {runningKey === key ? 'Running…' : `▶ Run ${ms.tool}`}
              </button>
            </div>
            <p class="tut-step-text">{ms.description}</p>
            <pre class="tut-code num">
              {`POST /api/mcp\n` + JSON.stringify({ tool: ms.tool, arguments: ms.arguments }, null, 2)}
            </pre>
            {out && (
              <pre class="tut-mcp-out num" data-testid={`tutorial-mcp-out-${i}`}>
                {out}
              </pre>
            )}
          </div>
        );
      })}
    </div>
  );

  return (
    <aside
      class={`tut-drawer${threeColMode ? ' wide-3col' : ''}${splitMobile ? ' split-mobile' : ''}`}
      data-testid="tutorials-drawer"
    >
      <div class="tut-header">
        <div class="tut-header-top">
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <span class="sys-badge">TUTORIALS &amp; THEORY</span>
            <span class="num" style={{ fontSize: '11px', color: 'var(--text-3)' }}>
              {sheet.verificationBadge}
            </span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <button
              type="button"
              class="btn small"
              data-testid="tutorial-split-toggle"
              title="Dock tutorial to bottom half so you can see and rotate the 3D model above"
              onClick={() => setSplitMobile(!splitMobile)}
            >
              {splitMobile ? 'Full ▴' : 'Split 3D ▾'}
            </button>
            <button
              type="button"
              class="btn small"
              data-testid="tutorial-3col-toggle"
              title="Toggle between Compact Companion Drawer and Full 3-Column Textbook View"
              onClick={() => setThreeColMode(!threeColMode)}
            >
              {threeColMode ? 'Compact' : '3-Col ⊞'}
            </button>
            <button
              type="button"
              class="btn small"
              data-testid="close-tutorials-drawer"
              onClick={() => ui.set('tutorialsOpen', false)}
            >
              Close
            </button>
          </div>
        </div>

        {/* Sheet selector strip (T1..T8) */}
        <div class="tut-sheet-strip" role="tablist" aria-label="Tutorial Sheets">
          {TUTORIAL_SHEETS.map((s) => (
            <button
              key={s.id}
              type="button"
              data-testid={`tutorial-sheet-${s.id}`}
              class={`tut-sheet-pill${s.id === sheet.id ? ' active' : ''}`}
              title={`${s.code}: ${s.title} (${s.level})`}
              onClick={() => {
                ui.set('tutorialSheetId', s.id);
                loadTutorialSheetModel(s.id, false);
              }}
            >
              <strong>{s.code}</strong>
              <span>{s.level === 'Flagship Multi-Physics' ? 'Multi-Physics' : s.level}</span>
            </button>
          ))}
        </div>

        {/* Active Sheet Banner + 1-Click Starter Model Loader */}
        <div class="tut-banner">
          <div>
            <div class="tut-title">
              {sheet.code}: {sheet.title}
            </div>
            <div class="tut-subtitle">{sheet.subtitle}</div>
          </div>
          <button
            type="button"
            class="btn small primary"
            data-testid="tutorial-load-model"
            onClick={loadScenarioModel}
          >
            Load Starting Model
          </button>
        </div>

        {/* In compact companion mode, show the 3-tab switcher */}
        {!threeColMode && (
          <div class="tut-tabs" role="tablist">
            <button
              type="button"
              class={activeTab === 'user' ? 'on' : ''}
              data-testid="tutorial-tab-user"
              onClick={() => setActiveTab('user')}
            >
              1. User Tutorial
            </button>
            <button
              type="button"
              class={activeTab === 'theory' ? 'on' : ''}
              data-testid="tutorial-tab-theory"
              onClick={() => setActiveTab('theory')}
            >
              2. Theory Manual
            </button>
            <button
              type="button"
              class={activeTab === 'mcp' ? 'on' : ''}
              data-testid="tutorial-tab-mcp"
              onClick={() => setActiveTab('mcp')}
            >
              3. AI &amp; API
            </button>
          </div>
        )}
      </div>

      {threeColMode ? (
        <div class="tut-3col-grid">
          {renderUserColumn()}
          {renderTheoryColumn()}
          {renderMcpColumn()}
        </div>
      ) : (
        <div class="tut-single-body">
          {activeTab === 'user' && renderUserColumn()}
          {activeTab === 'theory' && renderTheoryColumn()}
          {activeTab === 'mcp' && renderMcpColumn()}
        </div>
      )}
    </aside>
  );
}

function McpSetupModal() {
  useApp();
  if (!ui.mcpSetupOpen) return null;
  const origin = typeof window !== 'undefined' ? window.location.origin : 'http://192.168.0.180:5173';
  return (
    <div
      class="sys-scrim"
      data-testid="mcp-setup-modal"
      onClick={(e) => e.target === e.currentTarget && ui.set('mcpSetupOpen', false)}
    >
      <div
        class="sys-modal"
        style={{
          width: 'min(820px, calc(100vw - 24px))',
          height: 'min(740px, calc(100vh - 24px))',
          padding: '0',
        }}
      >
        <div class="sys-header">
          <div class="sys-header-left">
            <span class="sys-badge">MCP &amp; REST API CO-PILOT SETUP</span>
            <strong>Connect Any AI Agent or Script to This Live 3D Workspace</strong>
          </div>
          <div class="sys-header-right">
            <button
              type="button"
              class="btn small"
              data-testid="close-mcp-setup"
              onClick={() => ui.set('mcpSetupOpen', false)}
            >
              Close
            </button>
          </div>
        </div>
        <div
          style={{
            padding: '14px 16px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '12px',
            fontSize: '12px',
            lineHeight: 1.5,
          }}
        >
          <div class="tut-step-card">
            <strong>1. HTTP REST &amp; MCP Bridge (Zero-Install — Active Now on {origin})</strong>
            <p class="tut-step-text">
              The Fieldsmith web server exposes a live two-way workspace bridge at <code>{origin}/api/mcp</code> and{' '}
              <code>{origin}/api/workspace</code>. Any tool call sent to <code>POST /api/mcp</code> executes against the
              shared workspace and automatically updates this live 3D browser viewport within 1 second:
            </p>
            <pre class="tut-code num" style={{ maxHeight: '220px' }}>{`# 1) Inspect current shared workspace (.fsm model + revision)
curl -s ${origin}/api/workspace | jq .

# 2) Load or update a parameter (e.g. T9 Swirl Atomiser low-NOx orifice_r = 2.1 mm & swirl_offset = 7.2 mm)
curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"set_param","arguments":{"addr":"orifice_r","value":2.1}}'

curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"set_param","arguments":{"addr":"swirl_offset","value":7.2}}'

# 3) Run the 3D thermo-mechanical FEA solver and return peak stress, deflection & temperature
curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"solve_study","arguments":{"include_fine":false}}'`}</pre>
          </div>

          <div class="tut-step-card">
            <strong>2. Stdio MCP Server Setup (Claude Code / Antigravity / Cursor / mcp_config.json)</strong>
            <p class="tut-step-text">
              To register <code>fieldsmith-mcp</code> as a native stdio Model Context Protocol server sharing the same
              live browser workspace file, add this entry to your <code>mcp_config.json</code>:
            </p>
            <pre class="tut-code num" style={{ maxHeight: '200px' }}>{`{
  "mcpServers": {
    "fieldsmith": {
      "command": "/home/ian/Code/fieldsmith/target/release/fieldsmith-mcp",
      "args": [],
      "env": {
        "FIELDSMITH_WORKSPACE": "/home/ian/Code/fieldsmith/web/.fieldsmith-workspace.json"
      }
    }
  }
}`}</pre>
          </div>

          <div class="tut-step-card">
            <strong>3. Available Fieldsmith MCP Tools (15 Tools)</strong>
            <pre class="tut-code num" style={{ maxHeight: '220px' }}>{`• new_document(name, material)            — Reset workspace to a clean .fsm document
• load_fsm(fsm)                           — Load a complete parametric .fsm script (with let variables, CSG & BCs)
• export_fsm()                            — Return canonical .fsm text of the active workspace
• add_primitive(kind, id, op, blend, ...) — Add/cut/intersect box, cylinder, sphere, capsule, or torus
• set_param(addr, value)                  — Update a let variable (e.g. "orifice_r") or attribute ("nozzle.radius")
• set_material(material, node?)           — Set document material or per-volume material override (e.g. cucrzr on nozzle_body)
• add_region(id, selectors)               — Define a named surface region (e.g. ["chamber_bore.side"])
• add_support(id, region, fixed)          — Fix [x, y, z] displacements on a region
• add_load(id, region, kind/preload, ...) — Apply force (N), pressure (MPa), or bolt preload (N) on a region
• add_temperature(id, region, celsius)    — Apply fixed temperature (°C) boundary condition
• add_convection(id, region, h, t_inf)    — Apply heat-exchanger convective cooling h·(T − T_inf)
• add_heatflux(id, region, q)             — Apply surface heat flux (W/mm²)
• set_fill(family, morph?, cell, cell_min?, erode?, ...) — Configure level-set boundary evolution & multi-zone TPMS infill
• solve_study(include_fine)               — Run 3D P2 tetrahedral FEA + thermal conduction + modal solver
• list_coupled_systems()                  — List registered multi-physics PDE + Adjoint packs
• optimise_coupled_system(system_id, ...) — Step coupled PDE design levers along exact Adjoint gradients`}</pre>
          </div>
        </div>
      </div>
    </div>
  );
}

function TutorialsOverlayHost() {
  useApp();
  return (
    <>
      <TutorialsDrawer />
      <McpSetupModal />
    </>
  );
}
{
  const idx = viewOverlays.findIndex((fn) => fn.name === 'TutorialsOverlayHost');
  if (idx >= 0) viewOverlays[idx] = TutorialsOverlayHost;
  else viewOverlays.push(TutorialsOverlayHost);
}

function TutorialsStatusItem() {
  return (
    <>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-tutorials"
        style={{ height: '20px', padding: '0 6px', fontSize: '11px' }}
        onClick={toggleTutorialsDrawer}
      >
        Tutorials (T1–T13)
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-mcp-setup"
        style={{ height: '20px', padding: '0 6px', fontSize: '11px', color: 'var(--accent)' }}
        title="View MCP Server & REST API Co-Pilot Setup Instructions"
        onClick={() => ui.set('mcpSetupOpen', !ui.mcpSetupOpen)}
      >
        MCP / API Setup
      </button>
    </>
  );
}
{
  const idx = statusItems.findIndex((fn) => fn.name === 'TutorialsStatusItem');
  if (idx >= 0) statusItems[idx] = TutorialsStatusItem;
  else statusItems.push(TutorialsStatusItem);
}

extraCommands.push(() => [
  {
    id: 'tutorials.open',
    group: 'Help & Theory',
    label: 'Open Interactive Tutorials & Theoretical Manual (T1–T13)',
    run: () => toggleTutorialsDrawer(),
  },
  {
    id: 'mcp.setup',
    group: 'Help & Theory',
    label: 'Open Fieldsmith MCP & REST API Setup Guide',
    run: () => ui.set('mcpSetupOpen', true),
  },
]);
