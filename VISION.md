# Fieldsmith — Vision

> **Shape a part, load it, and grow its interior to fit the load — in a browser tab or on your phone, in minutes, with numbers you can defend.**

Fieldsmith is an open-source design, multiphysics analysis, and manufacturing qualification tool
for load-bearing parts and coupled physical systems, built around implicit fields. You model with
signed distance fields, 2D constraint-solved sketches, or imported CAD/mesh geometry; size the
operating envelope in coupled 1D/2D system workbenches; apply supports, loads, fastener preloads,
and thermal conditions directly on 3D surfaces; and grade the interior with a triply periodic
minimal surface (TPMS) or strut lattice whose density, cell size, and shell thickness follow the
solved stress and thermal fields. Nothing unencrypted leaves your machine, and compute beyond your
desk or phone runs only on hardware you control (§5).

---

## 1. Core idea: the part is a field, and the file is a proof

### 1.1 Unifying geometry, continuum physics, and verification in `p ∈ ℝ³`

Everything in Fieldsmith is a function of position `p ∈ ℝ³`:

- **Shape** — a signed distance field `d(p)`: negative inside, positive outside, composed from
  analytic primitives, extruded/revolved 2D constraint-solved sketches, and imported STEP/3MF/STL
  solids with exact smooth fillet and chamfer blends.
- **Regions** — predicates over position and surface distance: a bolt-hole bore, a conical nozzle
  throat band, a mounting face, or a keep-solid conformal skin `d(p) > −t`.
- **Material state** — per-volume continuum material assignment (`material <id> on <node>`),
  local relative density `ρ(p) ∈ [ρ_min, 1]`, spatially graded cell size `cell(p) ∈ [cell_min, cell]`,
  TPMS family morphing, and outer shell thickness `skin(p) ∈ [skin, skin_max]`.
- **Solved fields** — displacement `u(p)`, von Mises and principal stress `σ_vm(p), σ₁(p)`,
  equilibrated dual stress `σ*(p)`, temperature `T(p)`,vibration mode shapes, and far-field RF
  radiation gain `G(θ, φ)`.
- **The final part** — `d_final(p) = max(d_eroded(p), lattice(p; ρ(p), cell(p)))` unioned with
  protected solid skins and functional interface regions.

Because geometry, regions, analysis results, level-set boundary evolution, and lattice grading
share one representation, every stage composes with every other: grading is a field feeding a
field, a keep-out zone is a boolean, and a solved stress or temperature field drives geometry.

### 1.2 Why conventional CAD + CAE + 1D sizing fractures on real hardware

In conventional engineering workflows, a physical subsystem is split across four disconnected
representations:

1. **0D/1D system sizing and standards** in spreadsheets or standalone scripts (Bartz rocket
   cooling channels, VDI 2230 / NASA-STD-5020B fastener torque windows, 50 Ω Smith Chart impedance
   matching, effectiveness–NTU heat-exchanger sizing).
2. **3D B-rep geometry** that fails on cellular TPMS lattices, spatially graded wall thicknesses,
   or level-set eroded surfaces.
3. **3D FEA pre-processors** where meshing a filleted or lattice-filled solid is manual and
   brittle, and where boundary conditions are typed in by hand rather than bound to the 1D system
   model.
4. **Static documentation and verification reports** that drift out of date the moment a dimension
   or material changes.

Fieldsmith collapses all four into a **single human- and agent-readable text document (`*.fsm`, or
`.fsmz` when bundled with mesh assets)**. Following the literate verification tradition of
Stéphane Popinet's Gerris and Basilisk codes:

- Named scalar parameters (`let`) define the shared design levers across 1D system workbenches and
  3D CSG/sketch geometry.
- Structured literate header directives (`# @id:`, `# @category:`, `# @check:`, `# @step:`,
  `# @theory_*:`, `# @mcp:`) live inside the `.fsm` file itself alongside the executable field
  graph.
- The browser UI, `fieldsmith solve <file.fsm> --report`, the Android app (`sh.fieldsmith.app`),
  and an MCP co-pilot agent all read, solve, and verify that same single-file representation.

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

Key architectural choices:

- **One field IR across GPU and CPU.** The same field graph compiles to GLSL for 60 fps
  raymarching and evaluates in Rust with interval arithmetic for tet stuffing, volume
  integration, direct 2D slice stacks, and dual-contoured 3MF export.
- **Quality tetrahedra from implicit fields.** `fieldsmith-mesh` implements isosurface stuffing
  (Labelle & Shewchuk, 2007) with boundary-claim warping to generate graded P2 (10-node quadratic)
  tetrahedral meshes directly from signed distance fields without a B-rep kernel.
- **Single-source physics in Rust/WASM.** Domain solvers (3D P2 FEA, coupled Newton + sparse
  adjoint PDEs, 900 MHz / 2.4 GHz dipole VNA and Smith Chart impedance matching, fastener
  torque/Weibull envelopes) live once in Rust (`fieldsmith-solve`) and run identically in the
  browser via WebAssembly, on Android via JNI/WASM, and on headless Linux servers via CLI/MCP.
- **Literate, self-verifying documents.** Every guided tutorial (`T1`–`T7`, `T14`–`T15`), analytic
  benchmark (`A1`–`A14`), and multiphysics case study (`T8`–`T13`, `T16`–`T17`) is a standalone
  `.fsm` file under [`examples/`](examples/) whose `# @check:` header table is parsed by both
  `fieldsmith-doc` (`fieldsmith solve --report`) and the browser `3. Results` stage.
- **Pocket viewport, workstation muscle.** The Android app (`sh.fieldsmith.app`, built reproducibly
  for F-Droid) embeds `fieldsmith-mcp` on `127.0.0.1:18789/mcp` so on-device agents (such as Haven)
  can drive CAD and physics live on a phone while discovering workstation peers over
  LAN/Tailscale/WireGuard (`:18790` UDP beacon + `/api/health`) for heavy solves.

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
  compliance (`C_h ≤ C_true`), paired with witness-constrained equilibrated P2 stress recovery for
  the upper bound (`C_true ≤ C(σ*)`) across both isotropic and cubic-anisotropic elements.
- **Open standards alongside first-principles rederivation**: where engineering practice relies on
  standards (FED-STD-H28 screw-thread areas, NASA-STD-5020B / VDI 2230 fastener torque, NIST
  Weibull median-rank regression, NASA-HDBK-7005 / CARES ceramics), Fieldsmith evaluates both the
  standard formula and the continuum/statistical rederivation side-by-side so engineers see where
  empirical shortcuts hold and where they break down.
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
   lands with a new or updated Literate `.fsm` sheet in [`examples/`](examples/) and is verified
   by separate browser student subagent tracks (walking the tutorials, analytic benchmarks, and
   viewport/drawer flows) plus an MCP co-pilot pass driving `fieldsmith-mcp` / `/api/mcp`.

---

## 5. Delivered foundation (`v0.1` – `v0.14`) and the strategic direction ahead (`v0.15` → `v1.0`)

### 5.1 What is proven and shipping today (`V1` – `V60`)

Across versions `v0.1` through `v0.14` ([`README.md`](README.md), [`docs/README.md`](docs/README.md),
[`docs/verification/`](docs/verification/)), Fieldsmith has established:

- **Certified 3D P2 continuum mechanics (`V1`–`V48`)**: isotropic and cubic-anisotropic linear
  elasticity, steady Fourier conduction with Robin convection (`h, T_∞`) and Neumann heat flux (`q`),
  coupled Duhamel–Neumann thermo-elasticity, Lanczos/shift-invert modal vibration, Prager–Synge
  dual compliance bounds, 6 TPMS lattice families (`gyroid`, `schwarz_p`, `diamond`, `lidinoid`,
  `neovius`, `split_p`) with unit-cell homogenised yield/buckling laws, `∇f` overhang and LPBF
  layer-peel criteria, and multi-parameter Pareto sweeps.
- **Multi-body assemblies, fastener qualification & Weibull brittle fracture (`V53`, `V57`, `A10`, `A13`, `T10`, `T14`)**:
  per-volume materials (`material <id> on <node>`), axial bolt preload (`load <id> <region> preload=<N>`),
  Motosh / Kellermann–Klein / VDI 2230 3-surface torque-preload envelopes (`T_min`, `T_rec`, `T_max`),
  Junker vibration self-loosening, and 2-parameter Weibull ceramic survival (`P_f ≤ 10⁻⁶`), with
  auto-tapped pilot bores and spotfaces on CSG host surfaces.
- **Multi-zone lattices, level-set boundary evolution & 2D constraint sketching (`V54`, `V55`, `V58`, `T11`, `T12`, `T15`)**:
  smooth cell-size grading (`cell_min`), TPMS family morphing (`morph`), stress-driven outer
  boundary erosion (`erode`) wrapped in watertight skins (`skin` → `skin_max`), and a
  Levenberg–Marquardt + rank-revealing QR 2D sketch solver feeding `extrude` and `revolve`.
- **Electromagnetics & dual-band drone antenna VNA workbench (`V59`, `A14`, `T16`)**:
  900 MHz and 2.4 GHz dipole impedance, 50 Ω Möbius Smith Chart trajectories, CFRP airframe
  eddy-current image loss, 3D radiation lobes, and L-network matching in `fieldsmith-solve::antenna`.
- **Regeneratively cooled rocket propulsion & multi-solid STEP assemblies (`V60`, `A3`–`A6`, `T17`)**:
  exact De Laval chamber/nozzle `revolve` contours from RPA configs, multi-solid STEP assembly
  placement (`NEXT_ASSEMBLY_USAGE_OCCURRENCE`), Bartz sonic-throat heat flux + forced coolant
  Robin–Neumann thermo-elasticity, and stress-graded TPMS Gyroid cooling jackets.
- **Single-file Literate `.fsm` architecture & `Document → CoupledSystem` `let` binding (`v0.14.0`)**:
  all 31 tutorials, analytic verification benchmarks (`A1`–`A14`), and multiphysics case studies
  (`T8`–`T13`, `T16`–`T17`) live as self-defining `.fsm` files in [`examples/`](examples/), bind
  their `let` parameters directly into the coupled PDE/adjoint/UQ workbenches, and render in-stage
  verification tables in `3. Results` and `fieldsmith solve --report`.

---

### 5.2 Strategic direction (`v0.15` → `v1.0`): Five pillars for open-hardware engineering

The breakthroughs in `v0.8`–`v0.14` point toward a clear, disciplined long-term role for
Fieldsmith: **the open-source, self-verifying engineering workbench where system sizing, implicit
3D CAD, continuum FEA, and physical test data live in one reproducible `.fsm` file.**

Rather than adding disconnected point features or mimicking legacy B-rep CAD suites, future work
focuses on five compounding pillars:

#### Pillar I — Executable Literate Contracts & Public Benchmark Library (`fieldsmith solve --verify`)

1. **Live `@assert` / `@check` evaluation over solved fields**:
   Evolve `# @check:` in Literate `.fsm` headers from static reference strings into executable
   assertions evaluated against solved probe values, region surface integrals, compliance bounds,
   modal frequencies, and integrated mass:
   ```
   # @assert tip_deflection: rel_err(probe(tip).uz, -tip_force * span^3 / (3 * E * I)) <= 0.015
   # @assert safety_factor:  min_sf >= 2.5
   # @assert mass_budget:    mass_g <= 450.0
   ```
   Running `fieldsmith solve <file.fsm> --verify` in CI or opening `3. Results` in the browser
   computes the live formula value, the live FEA measurement, the relative error or safety margin,
   and a deterministic exit code (`0` when all contracts pass).
2. **Git-native library of verified open-hardware subsystems**:
   Expand [`examples/`](examples/) through the in-app `5. ➕ Submit / Export Case` builder into a
   curated, community-extensible library of analytic verification tests (`A#`), guided lessons
   (`T#`), and real open-hardware reference designs (regenerative thrust chambers, FPV/UAV
   airframes and RF mounts, kinematic optical mounts, counterflow TPMS heat exchangers, robotic
   actuators, and pressure vessels) where every contributed `.fsm` is automatically checked in CI.

#### Pillar II — Multi-Fidelity Closure: 1D/2D Adjoint System Models ↔ 3D Continuum Fields

1. **1D/2D coupled channel profiles projected onto 3D `.fsm` surfaces**:
   In `T17` (`regen_nozzle_h2_v1`), `T8` (`pilkington_float_bath_cvd_coater_v1`), and `T9` (swirl
   combustor), the 1D/2D monolithic Newton + adjoint workbench solves axial profiles for coolant
   bulk temperature `T_cool(z)`, convective film coefficient `h_cool(z)`, hot-gas heat flux
   `q_hot(z)`, and static pressure `p(z)` in `< 10 ms`. Projecting those solved profiles directly
   onto 3D `.fsm` surface regions (`heatflux`, `convection`, `pressure` as functions of axial
   coordinate `z`) eliminates manual piecewise band transcription and couples the 1D fluid/chemical
   march directly to the 3D P2 thermo-elastic stress solve.
2. **3D SDF geometry feeding back into 1D/2D hydraulic and thermal residuals**:
   Extract cross-sectional flow area `A(z)`, wetted perimeter `P_w(z)`, hydraulic diameter
   `D_h(z) = 4 A(z) / P_w(z)`, and local TPMS relative density `ρ(z)` directly from the 3D SDF
   graph so editing a revolve profile or Gyroid grading in `1. Shape` / `4. Fill` immediately
   updates the 1D channel pressure-drop and Nusselt correlations.

#### Pillar III — End-to-End 3D Adjoint Shape & Lattice-Grading Sensitivities (`dJ/d(let)`, `dJ/dρ_e`)

1. **Exact 3D P2 adjoint gradients with respect to `.fsm` `let` parameters**:
   The 1D/2D coupled workbenches already compute exact sparse adjoint sensitivities
   `dΦ/dp = ∂Φ/∂p − (∂Φ/∂W) (∂R/∂W)⁻¹ (∂R/∂p)`. Extending adjoint differentiation to the 3D P2
   tetrahedral elasticity and conduction solvers (`dC/d(let)`, `dT_max/d(let)`, `dm/d(let)` via
   boundary velocity fields `V_n = −∂d/∂(let) / |∇d|`) allows a user or MCP agent to optimise
   8–20 `let` dimensions in a single adjoint pass instead of a `N^k` grid sweep.
2. **Continuous element-wise density & cell-orientation sensitivities (`dJ/dρ_e`, `dJ/dθ_e`)**:
   Drive SIMP / level-set / homogenised cubic-anisotropic TPMS grading from exact 3D compliance and
   thermal-resistance adjoints so cooling jackets and structural brackets converge to certified
   minimum-mass grading in 5–10 iterations.

#### Pillar IV — Time-Dependent, Nonlinear Interface & Porous-Media Physics

1. **Transient 3D thermo-elastic time-stepping (`ρ c_p ∂T/∂t = ∇·(k ∇T)`)**:
   Add unconditional-stability implicit BDF2 / Crank–Nicolson time-stepping on 3D P2 tetrahedral
   meshes with adaptive time-step control (`Δt`), enabling thermal-shock analysis during rocket
   hot-fire ignition (`T17`), solar focal ignition (`T13`), and pulsed power-electronics loads (`T3`)
   where peak thermal stress occurs during the transient ramp rather than at steady state.
2. **Nonlinear unilateral contact & preloaded flange gasket separation**:
   Extend multi-body bolted assemblies (`V53`, `T10`, `T14`) with penalty / augmented-Lagrangian
   non-penetration contact across mating flange faces so bolt prying factor, heel liftoff, and
   gasket contact-pressure unloading under internal pressure are solved directly on 3D P2 surface
   patches.
3. **Darcy–Forchheimer porous-media flow & two-stream TPMS conjugate heat exchange (`T11`, `T17`)**:
   Solve macroscopic porous momentum (`∇p = −(μ / K(ρ)) u − β(ρ) ρ_f |u| u`) and two-fluid
   Effectiveness–NTU / two-temperature energy balances (`T_hot(p), T_cold(p)`) across graded
   Gyroid, Diamond, and Split-P domains separated by a conformal SDF mid-surface wall, using
   unit-cell calibrated permeability `K(ρ)` and interstitial area density `a_v(ρ)`.
4. **Touchstone `.s1p`/`.s2p` VNA import/export & 3D surface-current EM solver (`T16`, `A14`)**:
   Support importing and exporting industry-standard Touchstone `.s1p`/`.s2p` frequency sweeps in
   the Antenna VNA workbench so measured bench VNA traces from a physical drone or PCB antenna
   overlay directly onto the Smith Chart alongside the simulated locus.

#### Pillar V — Experimental & Manufacturing Qualification Loop + Pocket-to-Cluster Compute

1. **Closing the loop with physical test data inside `.fsm`**:
   Allow `.fsm` sheets to embed or reference empirical calibration rows (tensile/torsion coupon
   tables for Weibull `(m, σ₀)` fitting in `T14`, thermocouple traces for `h_cool` identification
   in `T17`, and `.s1p`/`.s2p` VNA traces in `T16`) so models are continuously anchored to bench
   hardware.
2. **Process-aware additive & subtractive qualification**:
   Combine `∇f` overhang area (`V45`), orthotropic layer-peel (`V46`), and inherent-strain thermal
   residual distortion previews in `5. Build`, while preserving exact analytical cylindrical/planar
   datum tags on STEP exports/imports (`V60e`) for post-print CNC boring and mating faces.
3. **Sovereign pocket-to-workstation co-design**:
   Keep the entire experience zero-account and user-owned: inspect, section, and direct an MCP
   agent from an Android phone in the workshop (`sh.fieldsmith.app` over local Haven MCP), automatically
   offloading multi-hundred-thousand-DOF 3D P2 solves, transient ramps, and adjoint steps over
   LAN / Tailscale / WireGuard (`:18790` discovery + Noise-encrypted `fieldsmith worker`) to
   personal workstations.

---

### 5.3 Non-goals

To keep Fieldsmith sharp, fast, and maintainable, the following remain explicit non-goals:

- **2D drafting sheets, GD&T title blocks, and enterprise PLM/BOM databases.**
- **General-purpose NURBS / B-rep surface modelling.** Fieldsmith imports STEP and 3MF into signed
  distance fields and models with implicit operations and 2D constraint sketches; it is not a
  B-rep kernel.
- **Machine-specific G-code generation.** Fieldsmith exports manifold 3MF/STL meshes and 3MF slice
  stacks; PrusaSlicer, OrcaSlicer, and Cura generate printer toolpaths.
- **Vendor-hosted SaaS, telemetry, or cloud lock-in.** All documents are plain git-friendly text
  (`*.fsm`), and all compute runs on devices and servers the user owns.

---

## 6. Principles, in one list

1. The user's part, not a canned demo.
2. Live preview and progressive background compute, never a blocking wizard.
3. Direct manipulation in the viewport; exact expressions and `let` variables in the inspector.
4. Every number carries units, provenance, and stated model assumptions.
5. Every `.fsm` benchmark or tutorial is a self-defining literate document verified in CI, and
   every solver module connects to a user-facing `.fsm` flow or calibrated material law (no engine
   islands).
6. One field representation; 1D system models, 2D sketches, 3D geometry, regions, continuum
   physics, and lattices compose.
7. Nothing unencrypted leaves the machine, and remote compute runs on user-owned hardware.
8. Quiet interface, loud geometry.
9. 5S in documentation and interface — sort, set in order, shine, standardise, sustain.

---

## 7. License

[AGPL-3.0-or-later](LICENSE), consistent with [`fenics-rs`](https://github.com/GlassOnTin/fenics-rs).
