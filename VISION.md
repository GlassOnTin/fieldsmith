# Fieldsmith — Vision

> **Shape a part, load it, and grow its interior to fit the load — in a browser tab, in minutes, with numbers you can trust.**

Fieldsmith is a browser-native design tool for parts that are going to be 3D printed and
are going to carry load. You model with implicit fields, pin and push the part directly
in the viewport, and the interior is filled with a lattice whose density follows the
stress. Nothing unencrypted leaves your machine, and data goes only to machines you
own. There is no Fieldsmith server, and no Fieldsmith compute either; when an analysis
outgrows your desk it runs on machines you control, under the standing rule in section 6.

---

## 1. Where this comes from

MorphoForge (an earlier experiment by another agent) found the right ingredients:

| Kept idea | Why it is right |
| :--- | :--- |
| Implicit / SDF geometry | Booleans never fail; lattices with millions of features are just another function. |
| In-browser FEA via `fenics-rs` | Real physics, no cloud, works air-gapped. |
| Stress-graded TPMS infill | The genuinely new thing: geometry that responds to load. |
| Direct-to-print export + slice preview | The output is a physical object, not a picture. |
| Literate, executable verification cases | Credibility comes from reproducible comparison with theory. |

What did not come together, and what Fieldsmith does differently:

| MorphoForge | Problem | Fieldsmith |
| :--- | :--- | :--- |
| A dropdown of preset "envelopes" (and "Bracket" / "Cylinder" were secretly boxes) | You cannot design *your* part. A CAD tool where you cannot shape geometry is a demo. | **Modelling comes first.** Primitives, booleans, smooth blends, shells, offsets, patterns — composed by you. |
| Loads as a single `Fz` slider applied as a body force; clamp hard-coded at `x = 0` | Physics does not match intent. | **Pick a face, drag an arrow.** Supports and loads are placed on regions of the real geometry, in N, on surfaces. |
| Lattice coloured by height but shown with a stress legend; mass reduction from a clamped density average; "target mass" slider unused | Numbers looked authoritative and weren't. | **Honesty is a feature.** Every number has units and provenance. Nothing is labelled stress unless it was solved. |
| Fixed six-step "Feature History (DAG)" checklist | Called a graph, behaved like a wizard. | **A real, editable model graph** that is also a readable text file. |
| Three rows of chrome, status pills, emoji buttons, gradient logo, theme picker, "Basilisk Workflows" | Attention spent on the tool instead of the part. | **Calm interface.** One viewport, one inspector, one model tree. The part is the brightest thing on screen. |
| One 1600-line `index.html`, a 23-positional-argument WASM call, a global mutex mesh | Hard to extend, hard to test. | Typed document model, small WASM surface, worker-based compute. |
| "nTop killer", "predatory monopolies", marketing tables of savings | Erodes the trust it needs from engineers. | Let the verification cases speak. Plain language. |

---

## 2. The core idea: the part is a field

Everything in Fieldsmith is a function of position:

- **Shape** — a signed distance field `d(p)`: negative inside, positive outside.
- **Regions** — fields too: "the bolt hole face" is `|d_hole(p)| < ε`; a keep-solid skin is `d(p) > −t`.
- **Material state** — relative density `ρ(p) ∈ [ρ_min, 1]`.
- **Results** — displacement `u(p)`, von Mises stress `σ_vm(p)`, interpolated from the FE solution.
- **The final part** — `d_final(p) = max(d(p), lattice(p; ρ(p)))` unioned with protected solid regions.

Because everything shares one representation, every stage composes with every other:
grading is just a field feeding a field; a keep-out zone is a boolean; a result can drive geometry.

The model is a **directed acyclic graph of field operations**, stored as a small,
human-readable text document (`*.fsm`). The UI is a view onto that document; so is a text
editor; so is an LLM agent or a script. Diffable in git. Reproducible bit-for-bit.

---

## 3. The experience

A first-time user should be able to do this without reading anything:

1. **Shape.** Drop a box, drop a cylinder, subtract it — a hole. Drag handles to size.
   Round the edges with one slider. The preview never stutters: it is raymarched on the GPU
   directly from the field, no meshing in the loop.
2. **Hold.** Click the inside of the hole → *Fixed*. The face highlights; a support glyph appears.
3. **Push.** Click the far face → *Load*. Drag the arrow; type `250 N`. The arrow's length and
   direction *are* the load.
4. **See.** A first stress result appears in about a second at coarse resolution and keeps
   refining in the background. A small badge says how converged it is.
5. **Fill.** Choose *Gyroid*, set a 1.2 mm skin and a 6 mm cell. The interior grades from dense
   (near the hole, where it hurts) to airy (where it doesn't). Mass updates live, in grams.
6. **Check.** Re-solve the graded part using homogenised lattice stiffness. See the new peak
   stress, deflection and safety factor against the chosen material — before and after, side by side.
7. **Make.** Export a watertight 3MF in millimetres. Scrub through slices to see what the printer will see.

### Interaction principles

- **Direct manipulation over forms.** If something has a position, direction or size, you
  set it in the viewport. The inspector is for precision, not for primary input.
- **Always live, never blocking.** Geometry preview at display refresh rate. Analysis runs in
  a worker, progressively, cancellable on edit. There is no "Run" button you must remember.
- **Stages are lenses, not steps.** Shape / Setup / Results / Fill / Make are views of the same
  model, freely revisited. Change the hole diameter after grading and everything downstream updates.
- **Keyboard-first, discoverable.** A command palette (`Ctrl K`) reaches every action;
  every button shows its shortcut on hover. No hidden modes.
- **Undo everything.** Every edit is a document transaction.

### Visual principles

- Neutral, low-contrast chrome; one accent colour reserved for selection and the active tool.
- The part is the brightest object; results use perceptual colour maps (viridis / cividis or
  a diverging map for signed quantities), always with a labelled legend and units.
- Sans-serif UI, tabular monospace for numbers. No gradients on text, no emoji as icons.
- Light and dark themes derived from the same tokens — chosen by OS preference, not a dropdown.
- Clear empty states: an empty document says what to do next.

---

## 4. Honesty as a product requirement

Engineers will only use a tool whose numbers they can defend.

- **One unit system internally**: mm · N · MPa · tonne · s (self-consistent). Display units are a preference.
- **Every result carries provenance**: element type, element count, solve time, convergence
  indicator, and which load case produced it.
- **Lattice properties are modelled, not assumed**: each TPMS family has a fitted homogenised
  stiffness law `E*(ρ) = C·E_s·ρ^n` (Gibson–Ashby form), with the coefficients and their
  source documented and verified by a shipped unit-cell study.
- **Mass is measured** from the final field (integrated occupancy), not inferred from a target.
- **Verification is part of the product**: a public set of literate cases compares Fieldsmith
  against closed-form solutions and reports the error, whatever it is. A failing case is a bug, not a footnote.
- **Limits are stated in the UI**: linear elastic, small strain, isotropic homogenisation —
  shown where the results are shown.

---

## 5. Architecture

```
┌──────────────────────── Browser main thread ────────────────────────┐
│  UI (TypeScript)                                                    │
│   ├─ Document store  (model graph, undo/redo, *.fsm text round-trip)│
│   ├─ Viewport        (WebGL2 / WebGPU raymarcher, generated shaders)│
│   ├─ Picking         (ray → field hit → region field)               │
│   └─ Inspector, model tree, command palette                         │
└───────────────┬─────────────────────────────────────▲───────────────┘
                │ graph IR (serialised)               │ results (transferable buffers)
┌───────────────▼──────────── Web Worker(s) ──────────┴───────────────┐
│  fieldsmith-core (Rust → WASM)                                      │
│   ├─ field     graph IR, CPU evaluation, interval bounds, gradients │
│   ├─ shader    IR → WGSL / GLSL codegen for preview                 │
│   ├─ mesh      dual contouring (surface), isosurface stuffing (tets)│
│   ├─ solve     fenics-rs linear elasticity, load/support from fields│
│   ├─ grade     σ → ρ mapping, homogenised stiffness, re-solve       │
│   └─ export    3MF (units, metadata), binary STL, slice contours    │
└─────────────────────────────────────────────────────────────────────┘
```

Key technical choices:

- **One IR, two back-ends.** The same field graph is evaluated in Rust (meshing, FEA, export)
  and compiled to a shader (preview). A shared test suite asserts they agree.
- **Interval arithmetic** over the IR to skip empty space — fast meshing and fast raymarching
  over lattices.
- **Quality tetrahedra from fields.** `fenics-rs` solves; it does not mesh arbitrary geometry.
  Fieldsmith owns that step using isosurface stuffing (Labelle & Shewchuk, 2007) on an adaptive
  grid — guaranteed element quality, no B-rep required.
- **Progressive solving.** Coarse → fine resolution ladder, warm-started; results stream to the UI.
- **No framework lock-in.** Small, typed TypeScript; Vite for dev/build; static hosting only.
- **Testable headless.** Every pipeline stage runs from `cargo test` and from a CLI with no browser.

---

## 6. Who it is for

- **Makers and small engineering shops** printing functional parts — brackets, mounts, jigs,
  drone arms — who want lighter parts without guessing.
- **Educators and students** who want to *see* load paths and why a lattice grades the way it does.
- **Engineers with confidential geometry** (medical, defence, pre-release product) who cannot
  upload to a cloud solver.
- **Agents and scripts**: the text model format and headless CLI make Fieldsmith a target for
  automated design loops, not just a GUI.

### Compute beyond the desk, by the same rule

The capabilities this vision reaches for, refined rungs and parameter sweeps now and the
engineering flows later, are compute-bound. The v0.3 budget work measured the coarse,
refined and export legs of the reference task on a £100 passively cooled box; a molding
or wave study will not stay in that class. The licence is what this project removes; the
hardware class is the user's choice, desk machine or cloud account, and the compute cost
matters as soon as that choice does: what a commercial tool blends into a per-seat price
or a quote appears here as a line in the user's own provider bill, unbundled and visible.

The standing rule, the same one the opening paragraph commits to:

- No Fieldsmith compute service and no accounts. Nothing is brokered, metered or stored
  by us.
- The pattern is the self-hosted runner: start a Fieldsmith worker on any machine you own,
  a box under the desk or an instance in your cloud account, and it connects out to your
  session. No inbound ports; nothing for a firewall to open toward Fieldsmith. Provisioner
  helpers that describe and start instances can layer on later; the worker route is
  provider-agnostic first.
- Transport is user-keyed and nothing leaves unencrypted; artifacts on the worker land
  only under the user's stated handling. Verified rather than promised, like everything
  else here: captured session traffic and worker-side storage scanned for plaintext model
  or field bytes, and the same document re-solved locally and remotely with any divergence
  measured and reported.

The group this serves most is the confidential-geometry engineer in the list above: a
hosted solver is exactly what they cannot use, whereas a worker on machines they control is.

### Non-goals (for now)

- Full B-rep / NURBS parametric CAD with drawings and tolerances. Reading STEP to make a
  field is on the v0.3 goal (see the landscape below); editing B-rep is not a non-goal,
  it is a different product.
- Nonlinear, dynamic, thermal or CFD analysis. Linear static elasticity, done well, first.
- Full topology optimisation (SIMP). Grading a lattice inside a fixed envelope is the scope;
  topology optimisation may come once the honest grading loop is solid.
- Being a slicer. We preview slices; Prusa/Orca/Cura print.

---

## 7. The competitive landscape

Recorded 2026-10-09 from product documents and published pricing, competitors read but
not exercised by hand. The Fieldsmith column is this repo at v0.2.0.

There are two markets with our name in them.

**Team CAD.** OnShape Professional ($2,500/user/year), Fusion ($680 base, $2,190 with
simulation added), SolidWorks (~$2,820/year by subscription). Sketches with constraints,
assemblies, drawings, BOM, sheet metal; STEP/IGES/Parasolid and native-CAD exchange;
managed versions, release management, live co-editing. OnShape's simulation, bundled at
Professional tier, does linear static and modal with no user-visible meshing and results
that track model versions. OnShape is the team-CAD spine most engineers sit behind.
This market wins wherever we have nothing to offer: a shop that needs drawings for a
 machinist needs this family. Fieldsmith does not pretend otherwise, and no part of the
roadmap should chase mates, BOM columns or title blocks.

**Design-for-additive.** nTop ($7.5k–$20k/seat/year by quote, Windows, closed) is the true
sibling: an implicit SDF kernel, field-driven design, TPMS plus beam, honeycomb and
stochastic lattices, voxel FEA, heat-exchanger and implant workflows, Automate scripting.
Fieldsmith shares the representation and one flagship workflow (stress-graded TPMS
infill), with a smaller surface: three TPMS families in sheet and network modes, no beam
or stochastic cells, no conformal mapping, no thermal or CFD, one material, one part.
What Fieldsmith adds is uncommon anywhere: the fill is graded by stress from the same
solver loop, its homogenised stiffness laws are fitted and measured (V6 within 10 %,
V7 at 2.1 % against a resolved reference), imported meshes become editable field leaves
with labelled faces, and every number in the verification set carries a measured error
against theory (V1–V11).

**OnShape specifically.** Where it is strictly ahead of us, named: sketch-driven B-rep,
assemblies, drawings, STEP/IGES/Parasolid/native-CAD import, sheet metal, managed
collaboration, modal analysis. Where it does not reach us: it reads meshes only as
reference geometry, has no lattice or graded fill of any kind, does no slicer-quality
AM export, keeps every feature of the tool behind accounts in a cloud we do not use,
and its published verification is marketing, not measured error bars.

**What follows for the roadmap.** The nearest real users are the ones in section 6, and
the files they already have are STEP and 3MF. OnShape and every mainstream tool exports
3MF; we read 3MF (single object so far, multi-volume next, with per-volume materials recorded
for the slicer even though analysis stays single-material). The named gap versus OnShape's
simulation is modal, and `fenics-rs` already carries an eigen route, so that one is cheap
to close. STEP stays on the old route this file always allowed: read the file, voxelise,
make it a field, never edit a B-rep. That is the v0.3 goal: the CAD round-trip.

---

## 8. Principles, in one list

1. The user's part, not ours.
2. Live, not wizard.
3. Direct manipulation, precise inspector.
4. Every number has units and a source.
5. Verification cases are tests, and tests are documentation.
6. One representation; everything composes.
7. Nothing unencrypted leaves the machine, and compute beyond it is the user's.
8. Quiet interface, loud geometry.

---

## 9. License

AGPL-3.0-or-later, consistent with `fenics-rs`.
