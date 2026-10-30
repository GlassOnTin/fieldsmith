# Fieldsmith — Vision

> **Shape a part, load it, and grow its interior to fit the load — in a browser tab, in minutes, with numbers you can trust.**

Fieldsmith is a browser-native design and analysis tool for load-bearing parts, built around
implicit fields. You model with signed distance fields or imported CAD/mesh geometry, apply
supports and loads directly on surfaces, and fill the interior with a lattice whose density
follows the solved stress field. Nothing unencrypted leaves your machine, and compute beyond
your desk runs only on machines you control (section 6).

---

## 1. Where this comes from

An earlier prototype (MorphoForge) proved that five ingredients belong together in a single
browser-capable tool:

| Ingredient | Why it works |
| :--- | :--- |
| Implicit / SDF geometry | Booleans do not fail on tangencies; lattices with millions of struts or sheets are a single closed-form function. |
| In-browser FEA via `fenics-rs` | Real finite-element physics running locally in WebAssembly or native CLI, air-gapped and account-free. |
| Stress-graded TPMS infill | Geometry that adapts continuously to the solved stress distribution. |
| Direct-to-print export + slice preview | The output is a watertight 3MF ready for the slicer and a physical part on the build plate. |
| Literate, executable verification cases | Engineering trust comes from reproducible comparison against closed-form theory and resolved reference meshes. |

Fieldsmith turns those ingredients into a real engineering tool through five structural choices:

1. **Modelling and CAD import come first.** Compose primitives, booleans, smooth blends, shells,
   offsets, and patterns, or bring in existing STEP, 3MF, STL, OBJ, and GLB parts as editable
   field leaves with segmented faces.
2. **Boundary conditions live on geometry.** Supports, forces, pressures, distributed loads, and
   temperatures attach to geometric regions in physical units (N, MPa, °C).
3. **Every number has units, provenance, and stated limits.** Nothing is labelled stress unless it
   came from a solve; every result records its mesh, element order, rung convergence, and material law.
4. **The model is a readable text programme (`*.fsm`).** The browser UI, the headless CLI, the
   stdio MCP server, and remote workers all read and write the same diffable document.
5. **Calm, viewport-first interface.** One viewport, one model tree, one inspector. The part and
   its physical fields are the brightest things on screen.

---

## 2. The core idea: the part is a field

Everything in Fieldsmith is a function of position `p ∈ ℝ³`:

- **Shape** — a signed distance field `d(p)`: negative inside, positive outside.
- **Regions** — predicates over position and surface distance: a bolt-hole bore, a mounting
  face, or a keep-solid skin `d(p) > −t`.
- **Material state** — relative density `ρ(p) ∈ [ρ_min, 1]`.
- **Solved fields** — displacement `u(p)`, von Mises stress `σ_vm(p)`, temperature `T(p)`, and
  vibration mode shapes, interpolated from the finite-element solution.
- **The final part** — `d_final(p) = max(d(p), lattice(p; ρ(p)))` unioned with protected solid
  skins and regions.

Because geometry, regions, analysis results, and lattice grading share one representation,
every stage composes with every other: grading is a field feeding a field, a keep-out zone is a
boolean, and an analysis result drives geometry.

The document is a **directed acyclic graph of field operations**, stored as a small text file
(`*.fsm`, or `.fsmz` when bundled with imported mesh assets). The browser UI, a text editor, a
script, and an AI agent all operate on the same representation.

---

## 3. The experience

A first-time user can complete a full design-to-print loop without reading a manual:

1. **Shape.** Drop primitives, sketch 2D extrusion profiles (`V31`), bind dimensions to named
   `let` variables (`V27`), or import a STEP/3MF bracket, subtract bolt holes, and fillet
   stress-concentrating corners. The preview is raymarched on the GPU directly from the field
   without waiting on a triangle mesh, with live section-plane clipping (`V26`) and multi-layer
   transparency (`V16`).
2. **Hold.** Click the inside of the mounting holes and mark them *Fixed*.
3. **Push.** Select the loaded face, apply a force in N (or a pressure, distributed traction,
   prescribed temperature, Robin convection `V29`, or heat flux `V30`), and orient it on the part.
4. **See.** A coarse P2 finite-element solve returns in roughly a second and refines in the
   background across a resolution ladder. The results table shows each rung's DOF count, wall
   time, rung-to-rung change, certified compliance bounds (`V34`, `V35`, `V43`), inter-layer
   peel utilization across build orientations (`V46`), animated vibration mode shapes (`V28`),
   and an interactive Pareto frontier explorer (`V47`).
5. **Fill.** Choose *Gyroid*, *Schwarz-P*, or *Diamond* (sheet or network), set skin thickness
   and cell size, orient the lattice axes (`V44`), and grade density by stress. Mass updates in
   grams from integrating the final field.
6. **Check.** Re-solve the graded part using fitted homogenised lattice stiffness (`V6`, `V7`),
   direction-aware rotated cubic anisotropy (`V36`, `V42`, `V44`), two-domain core thermal
   conduction (`V32`, `V33`), inter-layer bond peel/shear (`V46`), and per-family yield/buckling
   indicators (`V37`, `V38`). Compare solid vs. graded peak stress, deflection, safety factor,
   and mass side by side.
7. **Make.** Inspect exact-gradient overhang area (`V45`), export a watertight 3MF in
   millimetres, and scrub through layer slices before sending to PrusaSlicer or OrcaSlicer.

### Interaction principles

- **Direct manipulation with a precise inspector.** Set positions and regions in the viewport;
  type exact numbers and expressions in the inspector.
- **Live and non-blocking.** Raymarched preview runs at display refresh rate. Meshing and FEA
  run in background Web Workers (with shared-memory rayon pools when cross-origin isolated) and
  cancel cleanly on edit.
- **Stages are lenses, not a one-way wizard.** Shape (`1`), Setup (`2`), Results (`3`),
  Fill (`4`), and Make (`5`) are views over one document. Change a hole diameter after grading
  and downstream results update automatically.
- **Keyboard-first and discoverable.** `Ctrl K` opens the command palette for every action;
  every control shows its shortcut on hover.
- **Every control and number explains itself.** Each result, load type, and stated model limit
  carries a link to the governing formula and the verification case (`V#`) that pins its
  behaviour; the inspector shows units and assumptions beside the value it edits.
- **Undo everything.** Every edit is a reversible document transaction.

### Visual principles

- Neutral, low-contrast chrome; a single accent colour reserved for selection and the active tool.
- Perceptual colour maps (viridis / cividis, or diverging maps for signed quantities) with an
  explicit legend, range controls, and units.
- Sans-serif UI labels, tabular monospace for numbers, OS-driven light/dark theme tokens.

---

## 4. Honesty as a product requirement

Engineers only use a tool whose numbers they can defend.

- **One internal unit system**: `mm · N · MPa · tonne · s · °C` (self-consistent).
- **Provenance on every result**: engine version, element type (P2 10-node tetrahedra), element
  and DOF counts, wall time, solver iterations, and load case.
- **Measured refinement rungs and cost-to-tolerance**: each analysis climbs a resolution ladder
  and reports the rung-to-rung change and wall time at every step (`V17`, `V18`). Users can
  declare a target tolerance and see the rungs required to meet it, or declare a time budget
  and see the convergence achieved. When a stated budget or rung cap cannot meet the requested
  tolerance, Fieldsmith returns an explicit refusal rather than a stretched result.
- **Fitted and verified lattice laws**: each TPMS family uses a homogenised stiffness law
  `E*(ρ) = C · E_s · ρ^n` and cubic stiffness tensor (`C11`, `C12`, `C44`) whose coefficients
  are fitted from unit-cell finite-element studies (`V6`, `V36`) and checked against resolved
  full-lattice beam and patch solves (`V7`, `V42`).
- **Certified compliance brackets**: conforming P2 displacement solves provide a lower bound on
  compliance (`V34`), paired with witness-constrained equilibrated P2 stress recovery for the
  upper end across both isotropic and cubic-anisotropic elements (`V35`, `V43`).
- **Integrated mass**: part mass comes from integrating occupancy of the final field, never
  from multiplying solid mass by a nominal target density.
- **Executable verification suite**: `docs/verification/` (`V1`–`V47`) compares Fieldsmith
  against closed-form elasticity, beam, plate-hole, modal, thermal, mesh, additive-orientation,
  and pixel-level UI combination benchmarks (`V39`–`V41`) in CI.
- **Documentation at the manual standard of the reference codes** (*Ansys Fluent*, *Abaqus*,
  *FreeFem++*, Stéphane Popinet's *Gerris* / *Basilisk*): every solver, element space, grading
  law, and constitutive model documents its mathematical foundation — the derivation, its
  assumptions, and where its discretisation is exact — alongside the verification case (`V#`)
  and the source path that pin its behaviour. Documentation, interface, and code stay coupled:
  every formula links the case exercising it, every case links back to the formula and the
  file, and every on-screen number links to both.
- **The 5S standard for documentation and interface** (Toyota Production System):
  *sort* — the UI and the docs carry only what earns its place, dead controls and stale text
  are removed; *set in order* — a place for everything: each on-screen number and control
  links to its governing formula, its verification case (`V#`), and the code path, and each
  paragraph and case links back; *shine* — documents, panels, and links are cleaned as the
  code moves, and a stale range or broken link is a defect, not a chore; *standardise* — the
  same linkage conventions everywhere, stated once and repeated; *sustain* — the coupling is
  audited on a cadence in CI like any other verification, not on request.
- **Stated model limits**: linear elastic, small strain, isotropic homogenisation, and steady
  conduction assumptions are stated in the UI and reports alongside the numbers.

---

## 5. Architecture

```
┌─────────────────────────── Browser main thread ───────────────────────────┐
│  web/ (TypeScript + WebGL2)                                               │
│   ├─ Document store   (*.fsm / *.fsmz round-trip, undo/redo, validation)  │
│   ├─ Viewport         (GPU field raymarcher, compiled GLSL, overlays)     │
│   ├─ Picking          (ray → field hit → face / region selection)         │
│   └─ Lenses           (Shape · Setup · Results · Fill · Make)             │
└──────────────────┬────────────────────────────────────────▲───────────────┘
                   │ serialised document / job request      │ transferable buffers
┌──────────────────▼────────── Web Worker / Native ─────────┴───────────────┐
│  Rust workspace (`crates/*`, backed by `fenics-rs` submodule)             │
│   ├─ fieldsmith-doc     *.fsm parser, printer, AST, address grammar       │
│   ├─ fieldsmith-field   SDF / expression IR, interval bounds, GLSL codegen│
│   ├─ fieldsmith-mesh    isosurface stuffing (P2 tets), adaptive DC (3MF)  │
│   ├─ fieldsmith-solve   linear elasticity, modal eigensolve, steady heat  │
│   ├─ fieldsmith-core    regions, grading, STEP/3MF/STL/OBJ/GLB, rungs     │
│   ├─ fieldsmith-api     versioned JSON request/response runner (sweeps)   │
│   ├─ fieldsmith-mcp     stdio MCP server over fieldsmith-api              │
│   ├─ fieldsmith-worker  Noise-encrypted session/worker remote execution   │
│   └─ fieldsmith-cli / fieldsmith-wasm / fieldsmith-verify (V1–V47)        │
└───────────────────────────────────────────────────────────────────────────┘
```

Key technical choices:

- **One field IR across GPU and CPU.** The same field graph compiles to GLSL for 60 fps
  raymarching and evaluates in Rust with interval arithmetic for tet stuffing, volume
  integration, and dual-contoured 3MF export (`V5`).
- **Quality tetrahedra from implicit fields.** `fieldsmith-mesh` implements isosurface stuffing
  (Labelle & Shewchuk, 2007) with boundary-claim warping to generate graded P2 tetrahedral
  meshes directly from signed distance fields without a B-rep kernel.
- **Headless and AI-native by construction.** Every capability in the browser runs identically
  through `fieldsmith-cli`, the JSON `fieldsmith api` verb, and `fieldsmith-mcp` (`V22`).

---

## 6. Who it is for

- **Makers and engineering shops** printing functional brackets, mounts, jigs, and airframe
  parts who want lighter hardware backed by stress and deflection numbers.
- **Educators and students** who want to inspect load paths, stress concentrations, vibration
  modes, and lattice grading interactively.
- **Engineers with confidential geometry** (medical, defence, pre-release hardware) who cannot
  upload CAD to a vendor cloud.
- **Agents and automated design loops** driving parameter sweeps and structural checks over
  `.fsm` text files via CLI or MCP.

### Compute beyond the desk, by the same rule

Refined convergence ladders, multi-point parameter sweeps, and coupled physics studies quickly
outgrow a laptop or passively cooled mini-PC. Commercial CAE tools bundle compute into
high per-seat subscriptions or vendor clouds. Fieldsmith removes the software licence and keeps
hardware choice in the user's hands:

- **No Fieldsmith cloud, relay, or accounts.** Nothing is brokered, metered, or stored by this
  project.
- **Self-hosted outbound workers.** Run `fieldsmith worker` on any machine you control (a
  workstation under the desk or a VM in your own cloud account) and connect it to your session
  with a one-time enrolment token. Transport uses Noise encryption (`V21`), leaves no plaintext
  geometry on disk, and requires no inbound firewall ports.
- **Empirical, community-seeded cost model.** Each rung records an anonymised, shape-only
  measurement (DOF, element count, leaf count, CPU class, cores, RAM, wall time, and rung
  change) in `data/costs/costs.csv` (`V18`). Contributions happen via standard git pull
  requests after a local redaction preview (`fieldsmith costs`), giving new machines realistic
  wall-time projections with stated fit residuals.
- **Measured cross-machine behaviour.** Floating-point solves across different SIMD widths and
  architectures (such as x86_64 vs. aarch64 in `V25`) are compared field by field and bounded
  against canonical tolerances rather than assumed bit-identical.

### Non-goals

- **2D manufacturing drawings, GD&T title blocks, and assembly BOM management.** Shops that
  need machinist drawings belong in OnShape, SolidWorks, or FreeCAD.
- **Editable B-rep / NURBS surface modelling.** Fieldsmith imports STEP and 3MF into signed
  distance fields (`V10`, `V12`, `V14`) and models with implicit operations; it is not a B-rep
  kernel.
- **G-code toolpath generation.** Fieldsmith exports verified, manifold 3MF/STL meshes and
  previews layer slices; PrusaSlicer, OrcaSlicer, and Cura generate machine toolpaths.
- **Out-of-domain continuum physics (CFD, electromagnetics/RF, acoustics).** Fieldsmith focuses
  on structural and thermal design of load-bearing manufactured parts; non-structural continuum
  domains belong in dedicated `fenics-rs` applications rather than inside Fieldsmith.
- **Vendor-hosted SaaS.** All collaboration and compute stay on git repositories and machines
  the user owns.

---

## 7. Competitive landscape and roadmap direction

### Where Fieldsmith sits

Two commercial categories border Fieldsmith:

- **Parametric Team CAD** (*OnShape Professional ~$2,500/yr, Fusion + Simulation ~$2,190/yr,
  SolidWorks ~$2,820/yr*): built for sketch-constrained B-rep assemblies, machinist drawings,
  sheet metal, and PDM. OnShape includes cloud-hosted linear static and modal FEA, but treats
  meshes as secondary reference geometry, offers no lattice infill or field grading, requires a
  vendor cloud account, and publishes no executable verification suite.
- **Implicit Design-for-Additive** (*nTop, ~$7.5k–$20k/seat/yr, Windows-only, closed*): shares
  Fieldsmith's core representation (implicit SDF kernel + field-driven lattices + FEA).
  Fieldsmith covers a focused subset today (three TPMS families in sheet and network modes,
  single-material linear static, modal, and steady thermo-elastic analysis) while adding
  properties nTop does not offer: zero-install browser execution, open AGPL source, a diffable
  text format, fitted and verified Gibson–Ashby and cubic-anisotropic lattice laws in the repo
  (`V6`, `V7`, `V36`, `V42`, `V44`), certified compliance bounds (`V34`, `V35`, `V43`),
  exact-gradient overhang and layer-peel checks (`V45`, `V46`), interactive Pareto sweep
  exploration (`V47`), face-segmented mesh/STEP field leaves (`V10`, `V14`), and an automated
  verification ladder (`V1`–`V47`).

### Roadmap priorities

New capabilities are ranked by one question: **does this put a high-value engineering
capability into the hands of anyone with a browser and their own hardware, while keeping the
numbers verifiable?**

Every roadmap item is governed by two execution gates:

- **No Engine Islands.** No solver module or verification case (`V#`) lands unless it is either
  (a) reachable end-to-end from a `.fsm` document in both `fieldsmith-cli` and the browser UI,
  or (b) directly calibrating a coefficient table consumed by `.fsm` solves (as `V6` and `V36`
  calibrate `fill.rs`).
- **Pixels and first-user paths, not state-only assertions.** Every UI capability joins the
  feature-combination pixel matrix (`V39`), surfaces rendering or empty-field errors loudly
  (`V40`), and keeps the fresh-clone README new-user gate (`V41`) green.

In priority order beyond `v0.7` (`V1`–`V47`):

1. **Coupon-fitted per-family TPMS yield and buckling laws (`V48`).**
   - Replace the literature Gibson–Ashby open-cell placeholders (`0.3 · ρ^1.5` and `0.05 · ρ^2`
     in `fill.rs`) with per-family and per-mode constants (`c_y · ρ^{n_y}` and `c_b · ρ^{n_b}`)
     fitted from resolved unit-cell finite-element studies across all six TPMS classes,
     distinguishing stretching-dominated sheets from bending-dominated strut networks.
2. **Direct field slice stacks & 3MF Slice Extension (`V49`).**
   - Emit 2D Z-contour slice stacks (`<s:slicestack>` in the 3MF Slice Extension) and instant
     in-browser layer contours directly from `f(x, y, z_layer) = 0` (`slice_field`) without
     requiring a full 3D intermediate triangle mesh on dense lattices.
3. **Field-native stress-driven shell and wall thickening (`V50`).**
   - Feed the solved 3D stress parameter field back into the outer body wall (`fill ...
     skin_max=<mm>`), thickening load-bearing shells at stress risers and fillets smoothly in
     both the GPU raymarcher and CPU field evaluator without NURBS self-intersection failures.
4. **Closed-loop agentic synthesis via `fieldsmith-mcp` & multi-body assembly mechanics (`V51`).**
   - Extend `fieldsmith-mcp` with parametric design mutation tools (`set_let`, `set_fill`,
     `set_build`, and `sweep` solves) so an automated loop can propose `.fsm` → `sweep` /
     `ladder` → inspect yield, buckling, overhang, and layer-peel limits → refine parameters
     and orientation → export a verified 3MF meeting explicit structural and mass targets.
   - Tied/frictional contact and bolt preload across multi-volume 3MF and STEP assemblies.

---

## 8. Principles, in one list

1. The user's part, not a canned demo.
2. Live preview and progressive background compute, never a blocking wizard.
3. Direct manipulation in the viewport; exact expressions in the inspector.
4. Every number carries units, provenance, and stated model assumptions.
5. Verification cases (`V1`–`V47`) are executable tests in CI, and every solver module connects
   to a user-facing `.fsm` flow or a calibrated material law (no engine islands).
6. One field representation; geometry, regions, physics, and lattices compose.
7. Nothing unencrypted leaves the machine, and remote compute runs on user-owned hardware.
8. Quiet interface, loud geometry.
9. 5S in documentation and interface — sort, set in order, shine, standardise, sustain: a
   place for everything, the clutter gone, the links audited.

---

## 9. License

AGPL-3.0-or-later, consistent with `fenics-rs`.
