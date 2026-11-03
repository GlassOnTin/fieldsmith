# Fieldsmith — Vision

> **Shape a part, load it, and grow its interior to fit the load — in a browser tab, in minutes, with numbers you can trust.**

Fieldsmith is a browser-native design and analysis tool for load-bearing parts and coupled
physical systems, built around implicit fields. You model with signed distance fields or imported
CAD/mesh geometry, apply supports, loads, and thermal conditions directly on surfaces, and fill
the interior with a lattice whose density and shell thickness follow the solved stress field.
Nothing unencrypted leaves your machine, and compute beyond your desk runs only on machines you
control (§5).

---

## 1. Core idea: the part is a field

Everything in Fieldsmith is a function of position `p ∈ ℝ³`:

- **Shape** — a signed distance field `d(p)`: negative inside, positive outside.
- **Regions** — predicates over position and surface distance: a bolt-hole bore, a mounting
  face, or a keep-solid skin `d(p) > −t`.
- **Material state** — relative density `ρ(p) ∈ [ρ_min, 1]` and outer shell thickness
  `skin(p) ∈ [skin, skin_max]`.
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

## 2. Architecture

```
┌─────────────────────────── Browser main thread ───────────────────────────┐
│  web/ (TypeScript + WebGL2)                                               │
│   ├─ Document store   (*.fsm / *.fsmz round-trip, undo/redo, validation)  │
│   ├─ Viewport         (GPU field raymarcher, compiled GLSL, overlays)     │
│   ├─ Picking          (ray → field hit → face / region selection)         │
│   └─ Lenses           (Shape · Setup · Results · Fill · Make · Tutorials) │
└──────────────────┬────────────────────────────────────────▲───────────────┘
                   │ serialised document / job request      │ transferable buffers
┌──────────────────▼────────── Web Worker / Native ─────────┴───────────────┐
│  Rust workspace (`crates/*`, backed by `fenics-rs` submodule)             │
│   ├─ fieldsmith-doc     *.fsm parser, printer, AST, address grammar       │
│   ├─ fieldsmith-field   SDF / expression IR, interval bounds, GLSL codegen│
│   ├─ fieldsmith-mesh    isosurface stuffing (P2 tets), adaptive DC, slices│
│   ├─ fieldsmith-solve   elasticity, modal, steady heat, coupled systems   │
│   ├─ fieldsmith-core    regions, grading, STEP/3MF/STL/OBJ/GLB, rungs     │
│   ├─ fieldsmith-api     versioned JSON request/response runner (sweeps)   │
│   ├─ fieldsmith-mcp     stdio & HTTP MCP server over fieldsmith-api       │
│   ├─ fieldsmith-worker  Noise-encrypted session/worker remote execution   │
│   └─ fieldsmith-cli / fieldsmith-wasm / fieldsmith-verify (docs/verif.*)  │
└───────────────────────────────────────────────────────────────────────────┘
```

Key technical choices:

- **One field IR across GPU and CPU.** The same field graph compiles to GLSL for 60 fps
  raymarching and evaluates in Rust with interval arithmetic for tet stuffing, volume
  integration, direct 2D slice stacks, and dual-contoured 3MF export.
- **Quality tetrahedra from implicit fields.** `fieldsmith-mesh` implements isosurface stuffing
  (Labelle & Shewchuk, 2007) with boundary-claim warping to generate graded P2 tetrahedral
  meshes directly from signed distance fields without a B-rep kernel.
- **Headless and AI-native by construction.** Every capability in the browser runs identically
  through `fieldsmith-cli`, the JSON `fieldsmith api` verb, and `fieldsmith-mcp`.

---

## 3. Honesty and engineering invariants

Engineers only use a tool whose numbers they can defend:

- **One internal unit system**: `mm · N · MPa · tonne · s · °C` (self-consistent).
- **Provenance on every result**: engine version, element type (P2 10-node tetrahedra), element
  and DOF counts, wall time, solver iterations, and load case.
- **Measured refinement rungs and cost-to-tolerance**: each analysis climbs a resolution ladder
  and reports the rung-to-rung change and wall time at every step. When a stated budget or rung
  cap (`750k` DOFs default in-browser) cannot meet a requested tolerance, Fieldsmith returns an
  explicit refusal rather than a stretched result.
- **Fitted and verified lattice laws**: each TPMS family uses homogenised stiffness, cubic
  anisotropy (`C11`, `C12`, `C44`), yield, and buckling laws fitted from unit-cell finite-element
  studies and checked against resolved full-lattice solves in [`docs/verification/`](docs/verification/).
- **Certified compliance brackets**: conforming P2 displacement solves provide a lower bound on
  compliance, paired with witness-constrained equilibrated P2 stress recovery for the upper end
  across both isotropic and cubic-anisotropic elements.
- **Integrated mass**: part mass comes from integrating occupancy of the final field, never
  from multiplying solid mass by a nominal target density.
- **5S in documentation and interface**:
  *sort* — the UI and docs carry only what earns its place, dead controls and stale text are
  removed; *set in order* — every on-screen number and control links to its governing formula,
  verification case (`V#`), and code path; *shine* — panels and links are cleaned as the code
  moves; *standardise* — one linkage convention everywhere; *sustain* — verified in CI.

---

## 4. Settled execution gates

Every feature or verification case (`V#`) added to the repository must satisfy five gates:

1. **No Engine Islands.** No solver module or verification case (`V#`) lands unless it is either
   (a) reachable end-to-end from a `.fsm` document or workbench in both `fieldsmith-cli` and the
   browser UI, or (b) directly calibrating a coefficient table consumed by `.fsm` solves.
2. **Pixels and first-user paths, not state-only assertions.** Every UI control joins the
   feature-combination pixel matrix (`web/tests/combinations.mjs`), surfaces rendering or
   empty-field failures loudly through `ui.renderError`, and keeps the fresh-clone README
   new-user gate (`scripts/v41-fresh-clone.sh`) green.
3. **Report immutability on routine test runs (`FIELDSMITH_BLESS`).** `cargo test -p fieldsmith-verify`
   checks every tolerance on every run but only overwrites existing `docs/verification/V*.md`
   files when `FIELDSMITH_BLESS=1` is set (or when a new report is created or a check fails).
4. **Hardware budget referee.** The 32-thread workstation ([`budgets.md`](docs/verification/budgets.md))
   is the pass/fail interactive referee; secondary hardware (`N100`, `SM8750`) populates
   empirical rows in [`data/costs/costs.csv`](data/costs/costs.csv).
5. **Interactive tutorials and exploratory agentic UX verification.** Every major capability
   lands with a new or updated interactive lesson in [`web/src/app/tutorials.tsx`](web/src/app/tutorials.tsx)
   and is verified by two separate subagent passes: (a) an exploratory browser student (`chrome-devtools`)
   that walks the tutorial while intentionally poking edge cases, clicking wrong buttons, and
   testing Undo/recovery, and (b) an MCP co-pilot agent driving the same workflow through
   `fieldsmith-mcp` / `/api/mcp`.

---

## 5. Compute, boundaries, and roadmap

### Compute beyond the desk

- **No Fieldsmith cloud, relay, or accounts.** Nothing is brokered, metered, or stored by this
  project.
- **Self-hosted outbound workers.** Run `fieldsmith worker` on any machine you control and
  connect it to your session with a one-time enrolment token over Noise encryption.
- **Empirical, community-seeded cost model.** Each rung records an anonymised, shape-only
  measurement in [`data/costs/costs.csv`](data/costs/costs.csv).

### Non-goals

- **2D manufacturing drawings, GD&T title blocks, and assembly BOM management.**
- **Editable B-rep / NURBS surface modelling.** Fieldsmith imports STEP and 3MF into signed
  distance fields and models with implicit operations; it is not a B-rep kernel.
- **G-code toolpath generation.** Fieldsmith exports manifold 3MF/STL meshes and 3MF slice
  stacks; PrusaSlicer, OrcaSlicer, and Cura generate machine toolpaths.
- **Vendor-hosted SaaS.** All collaboration and compute stay on git repositories and machines
  the user owns.

### Next priorities

For what Fieldsmith already does and verifies today, see [`README.md`](README.md) and
[`docs/verification/`](docs/verification/). In priority order, the next milestones are:

1. **Multi-body assembly mechanics & bolt preload (`V53`, `T10` — done)** — per-volume material
   stiffness, density, and thermal expansion (`material <id> on <node>`) plus axial bolt preload
   clamping (`load <id> <region> preload=<N> [axis=(...)]`). Next extension: nonlinear penalty
   gasket separation across unbonded contact interfaces.
2. **Spatially varying cell-size and multi-zone lattice fields (`V54`, `T11` — done)** — smooth
   transitions between lattice cell sizes (`cell_min=<mm>`) and TPMS families (`morph=<family>`)
   driven by the 3D field grade with exact CPU/GLSL parity and two-pass homogenised FEA.
3. **Topology optimization / level-set field evolution** — stress- and compliance-driven outer
   boundary evolution directly on the implicit field grid.

---

## 6. Principles, in one list

1. The user's part, not a canned demo.
2. Live preview and progressive background compute, never a blocking wizard.
3. Direct manipulation in the viewport; exact expressions in the inspector.
4. Every number carries units, provenance, and stated model assumptions.
5. Verification cases (`docs/verification/V*.md`) are executable tests in CI, and every solver
   module connects to a user-facing `.fsm` flow or a calibrated material law (no engine islands).
6. One field representation; geometry, regions, physics, and lattices compose.
7. Nothing unencrypted leaves the machine, and remote compute runs on user-owned hardware.
8. Quiet interface, loud geometry.
9. 5S in documentation and interface — sort, set in order, shine, standardise, sustain.

---

## 7. License

[AGPL-3.0-or-later](LICENSE), consistent with [`fenics-rs`](https://github.com/GlassOnTin/fenics-rs).
