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
(`*.fsm`, or `.fsmz` when bundled with imported mesh assets). Following the literate verification
style of Gerris and Basilisk, a single `.fsm` file can also carry structured `# @check:`,
`# @step:`, `# @theory_*`, and `# @mcp:` header directives alongside executable geometry and
boundary conditions. The browser UI, the CLI (`fieldsmith solve --report`), the Android app, and
an MCP agent all operate on that same single-file representation.

---

## 2. Architecture

```
┌────────────────── Browser / Android WebView main thread ──────────────────┐
│  web/ (TypeScript + Preact + WebGL2)                                      │
│   ├─ Document store   (*.fsm / *.fsmz round-trip, Literate .fsm parser)   │
│   ├─ Viewport         (GPU field raymarcher, compiled GLSL, overlays)     │
│   ├─ Picking          (ray → field hit → face / region selection)         │
│   ├─ Workbenches      (2D Sketch · Fixings/Weibull · Antenna VNA · PDE)   │
│   └─ Lenses           (Shape · Setup · Results · Fill · Build · Theory)   │
└──────────────────┬────────────────────────────────────────▲───────────────┘
                   │ serialised .fsm / let params / jobs    │ transferable buffers
┌──────────────────▼──── Web Worker / JNI / LAN Peer ───────┴───────────────┐
│  Rust workspace (`crates/*`, backed by `fenics-rs` submodule)             │
│   ├─ fieldsmith-doc     *.fsm + Literate header parser, printer, AST      │
│   ├─ fieldsmith-field   SDF / expression IR, interval bounds, GLSL codegen│
│   ├─ fieldsmith-mesh    isosurface stuffing (P2 tets), adaptive DC, slices│
│   ├─ fieldsmith-solve   elasticity, modal, heat, RF VNA, coupled adjoints │
│   ├─ fieldsmith-core    regions, grading, STEP/3MF/STL/OBJ/GLB, rungs     │
│   ├─ fieldsmith-api     versioned JSON request/response runner (sweeps)   │
│   ├─ fieldsmith-mcp     stdio & HTTP MCP server + LAN peer discovery      │
│   ├─ fieldsmith-worker  Noise-encrypted session/worker remote execution   │
│   └─ fieldsmith-cli / fieldsmith-wasm / fieldsmith-verify (V1–V60)        │
└───────────────────────────────────────────────────────────────────────────┘
```

Key technical choices:

- **One field IR across GPU and CPU.** The same field graph compiles to GLSL for 60 fps
  raymarching and evaluates in Rust with interval arithmetic for tet stuffing, volume
  integration, direct 2D slice stacks, and dual-contoured 3MF export.
- **Quality tetrahedra from implicit fields.** `fieldsmith-mesh` implements isosurface stuffing
  (Labelle & Shewchuk, 2007) with boundary-claim warping to generate graded P2 tetrahedral
  meshes directly from signed distance fields without a B-rep kernel.
- **Single-source physics in Rust/WASM.** Domain solvers (3D P2 FEA, coupled Newton + sparse
  adjoint PDEs, 900 MHz / 2.4 GHz dipole VNA and Smith Chart impedance matching, fastener
  torque/Weibull envelopes) live once in Rust (`fieldsmith-solve`) and run identically in the
  browser via WebAssembly, on Android via JNI/WASM, and on headless Linux servers via CLI/MCP.
- **Literate, self-verifying documents.** Every tutorial (`T1`–`T7`, `T14`–`T15`), analytic
  benchmark (`A1`–`A14`), and multiphysics case study (`T8`–`T13`, `T16`–`T17`) is a standalone
  `.fsm` file under [`examples/`](examples/) whose `# @check:` header table is parsed by both
  `fieldsmith-doc` (`fieldsmith solve --report`) and the browser `3. Results` stage.
- **Mobile-first agentic CAD with LAN compute offload.** The Android app (`sh.fieldsmith.app`,
  packaged reproducibly for F-Droid) embeds `fieldsmith-mcp` on `127.0.0.1:18789/mcp` so on-device
  agents (such as Haven) can drive CAD and physics live on a phone while discovering workstation
  peers over LAN/Tailscale/WireGuard (`:18790` UDP beacon + `/api/health`) for heavy solves.

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

### Delivered breakthroughs (`v0.8` – `v0.14`)

For full details and verification tables, see [`README.md`](README.md), [`docs/README.md`](docs/README.md),
and [`docs/verification/`](docs/verification/):

- **Multi-body assembly mechanics, fastener qualification & Weibull brittle fracture (`V53`, `V57`, `A10`, `A13`, `T10`, `T14`)** —
  per-volume materials (`material <id> on <node>`), axial bolt preload (`load <id> <region> preload=<N>`),
  VDI 2230 / Kellermann–Klein / NASA-STD-5020B 3-surface torque-preload envelopes (`T_min`, `T_rec`, `T_max`),
  Junker vibration self-loosening, and 2-parameter Weibull ceramic survival (`P_f ≤ 10⁻⁶`), with auto-tapped
  pilot bores and spotfaces on CSG host surfaces.
- **Spatially varying multi-zone lattices & level-set boundary evolution (`V54`, `V55`, `T11`, `T12`)** —
  smooth cell-size grading (`cell_min`), TPMS family morphing (`morph`), and stress-driven outer
  boundary erosion (`erode`) wrapped in watertight conformal skins (`skin` → `skin_max`).
- **2D constraint-solved parametric sketcher (`V58`, `T15`)** — Levenberg–Marquardt + rank-revealing
  QR Jacobian solver with DOF and over-constraint diagnostics feeding `extrude` and `revolve`.
- **Electromagnetics & dual-band drone antenna VNA workbench (`V59`, `A14`, `T16`)** — 900 MHz and
  2.4 GHz dipole impedance, 50 Ω Möbius Smith Chart trajectories, CFRP airframe eddy-current image
  loss, 3D radiation lobes, and L-network matching consolidated in `fieldsmith-solve::antenna`.
- **Regeneratively cooled rocket propulsion & multi-solid STEP assemblies (`V60`, `A3`–`A6`, `T17`)** —
  exact De Laval chamber/nozzle `revolve` contours from RPA configs, multi-solid STEP assembly placement
  (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`), Bartz sonic-throat heat flux + forced coolant Robin–Neumann
  thermo-elasticity, and stress-graded TPMS Gyroid cooling jackets.
- **Single-file Literate `.fsm` architecture & `Document → CoupledSystem` `let` binding (`v0.14.0`)** —
  all 31 tutorials, analytic verification benchmarks (`A1`–`A14`), and multiphysics case studies
  (`T8`–`T13`, `T16`–`T17`) live as self-defining `.fsm` files in [`examples/`](examples/), bind
  their `let` parameters directly into the coupled PDE/adjoint/UQ workbenches, and render in-stage
  verification tables in `3. Results` and `fieldsmith solve --report`.

### Next priorities (`v0.15+`)

1. **Executable `@check` / `@assert` expressions in `fieldsmith solve --verify`** —
   Evaluate closed-form formulas in Literate `.fsm` headers directly against live solved probe and
   summary fields (for example, checking `abs(probe(tip).uz - (-F*L^3/(3*E*I))) / abs(...) < 0.015`
   automatically at the end of a solve), turning every `.fsm` file into a self-checking test case in
   both `fieldsmith-cli` and the browser `3. Results` card.
2. **Nonlinear unilateral contact & flange gasket separation** —
   Extend multi-body bolted assemblies (`V53`, `T10`, `T14`) with penalty / augmented-Lagrangian
   non-penetration contact across unbonded mating faces so heel separation, prying factor, and gasket
   unloading under internal pressure are solved directly on 3D P2 surface patches.
3. **Transient 3D thermo-elastic time-stepping (`ρ c_p ∂T/∂t = ∇·(k ∇T)`)** —
   Add implicit BDF2 / Crank–Nicolson transient heat conduction and coupled thermal stress on 3D P2
   tetrahedral meshes so thermal shock during rocket ignition (`T17`), solar focal heating (`T13`),
   and power-electronics load steps can be stepped in time with adaptive Δt control.
4. **3D FEA adjoint shape & lattice-grading sensitivities (`dJ/d(let)`, `dJ/dρ_e`)** —
   Differentiate the 3D P2 stiffness and thermal residual with respect to `.fsm` `let` dimensions
   and element-wise TPMS density `ρ_e` so users and MCP agents can step 3D part dimensions and
   lattice grading along exact compliance, mass, and temperature gradients without finite-difference
   sweeps.
5. **Direct 1D/2D coupled-channel to 3D field boundary mapping** —
   Project axial temperature, pressure, and heat-transfer coefficient profiles `T_cool(z), h_cool(z), p(z)`
   solved in the 1D/2D monolithic coupled workbenches (`regen_nozzle_h2_v1`, `pilkington_float_bath_cvd_coater_v1`)
   directly onto 3D `.fsm` surface regions as spatially varying `convection` and `heatflux` bands.

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
