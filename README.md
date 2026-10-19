# Fieldsmith

Fieldsmith is an implicit-field CAD and analysis tool: a document is a small text programme,
its geometry is an SDF-like field, and every measurement (volume, stress, printed mass) comes
from integrating that field, not from a boundary representation. You shape a part, load it,
and grow its interior to fit the load, all in the browser, with no server.

## Run the web app

Download the static web bundle from the latest
[GitHub Releases](https://github.com/GlassOnTin/fieldsmith/releases) (`fieldsmith-*-web.tar.gz`)
and serve the unpacked folder with any static file server:

    tar -xzf fieldsmith-*-web.tar.gz
    cd fieldsmith-*-web
    ls index.html   # confirm you are in the unpacked folder, not the download dir
    python3 -m http.server 8931 --bind 127.0.0.1    # then open http://127.0.0.1:8931

What you should see is the app itself. A **directory listing** means the server is rooted
at the wrong folder — the unpacked `fieldsmith-*-web` directory carries `index.html` at
its top; the repository (or the folder you downloaded into) does not.

If the port is already taken, `http.server` exits with "Address already in use", and then
the browser's address belongs to whatever else listens there (on one machine that is a
file server asking for a sign-in). Pick another port; any static file server works, and
this path is tested on `python3 -m http.server` (`.wasm` files must arrive as
`application/wasm`, which python's default does).

No install beyond unpacking. Without extra host headers the app runs on one worker thread
(meshing and export take several times longer than the threaded build); the threaded build
needs two response headers on every file the host serves:

    Cross-Origin-Opener-Policy: same-origin
    Cross-Origin-Embedder-Policy: require-corp

The Vite dev and preview servers send both; the budget numbers in
[docs/verification/budgets.md](docs/verification/budgets.md) are measured on the threaded
build.

### From the source checkout

No download is needed in a clone of this repository; the app runs from source:

    cd ~/Code/fieldsmith
    npm --prefix web install                  # once
    bash scripts/build-wasm.sh                # one-time wasm build (re-run after rust changes)
    npm --prefix web -- run dev               # then open http://localhost:5173

The dev server hot-reloads the app and serves the threaded build (both headers above), so
it is the faster path, and the right one while editing.

## Run the CLI solver

Static Linux archives (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, carrying
`fieldsmith` and `fieldsmith-mcp`) publish on the same release page. The example documents
live in `examples/`:

    fieldsmith check  examples/bracket-filled.fsm
    fieldsmith solve  examples/bracket.fsm --report
    fieldsmith export examples/bracket-filled.fsm --3mf bracket.3mf --stl bracket.stl

## The document

A `.fsm` document is the whole thing: shapes, materials, supports, loads, and the lattice
fill. This is `examples/bracket-filled.fsm`:

```fsm
fieldsmith 1
name "Bracket"
material petg

box base size=(60, 40, 8) at=(30, 20, 4)
box upright size=(60, 8, 40) at=(30, 4, 20) blend=8
cylinder hole1 r=3.25 h=20 at=(15, 26, 4) subtract
cylinder hole2 r=3.25 h=20 at=(45, 26, 4) subtract
cylinder hole3 r=4.25 h=20 at=(30, 4, 28) rotate=(90, 0, 0) subtract

region mount = hole1.side + hole2.side
region pin = hole3.side
support fixed mount fixed=xyz
load pull pin force=(0, -103.53, -386.37) case="Load 1"

fill gyroid sheet cell=6 skin=1.2 rho=(0.2, 0.8) curve=1 wall=0.8
```

You can type this into the web app, edit it there, or write it in a file and drive the CLI.
Named variables (`let wall_t = ...`) work throughout. The app's lenses walk the flow
(model, setup, solve, fill, results, export), with interactive section-plane clipping into
the solved field.

## What it does

- Implicit-field modelling with blend, intersect, subtract, rotate, and mirrored patterns;
  in-app 2D profile sketching.
- Import of GLB, OBJ, STL (binary and ASCII), and 3MF; a mesh becomes a field leaf with
  labelled faces so regions and loads extend to imported parts (V10). STEP imports via the CLI.
- P2 finite-element solve with one Young's modulus per element; steady thermal
  conduction (`temperature`), convection bands (`convection`), prescribed heat fluxes
  (`heatflux`); modal analysis.
- TPMS lattice fill (gyroid, primitive, diamond; sheet and network), graded by solved
  stress, with per-element yield and buckling factors in the Results lens.
- Certified numbers where the physics allows: the compliance is a lower bound (V34), and
  an equilibrated stress field raises an upper end (V35) to bracket the truth.
- Binary STL and 3MF export (millimetres, part name and material recorded).
- Automation: `fieldsmith api` (one JSON request in, one envelope out), the
  `fieldsmith-mcp` binary (stdio MCP server over that API), and the `session` and
  `worker` verbs to hand jobs to machines you own over Noise-encrypted connections.

## Where to look next

By what you want to do:

| You want to | Look at |
| :-- | :-- |
| see the language and the flow | this README, `examples/*.fsm`, then the app itself |
| check one measured claim | [docs/verification/](docs/verification/): one report per case, numbered from V1 in gate order, each with Method and Limits sections |
| know the performance | [budgets.md](docs/verification/budgets.md) and the machine variants beside it; per-run wall times in [data/costs/costs.csv](data/costs/costs.csv) |
| know where the project is going | [VISION.md](VISION.md) (the roadmap, §7 for the current rungs) and [GOAL.md](GOAL.md) |
| automate or distribute | each verb's help text (`fieldsmith --help`, and the `fieldsmith-mcp` binary's own help) |
| build it | the next section |

## Building the release artefacts

The full build, including the verification suite:

    cargo test -p fieldsmith-verify           # verification cases (V1–V38; V24/V25 in scripts/)
    npm --prefix web install                  # once (if not done in Run the web app)
    bash scripts/build-wasm.sh                # build the WebAssembly analysis
    npm --prefix web -- run build             # production build into web/dist

`build-wasm.sh` produces two packages of the same crate: `web/src/pkg` (single thread,
stable toolchain) and `web/src/pkg-mt` (shared memory plus a rayon pool on Web Workers;
needs a nightly toolchain with `rust-src` for `-Z build-std`; set
`FIELDSMITH_WASM_ST_ONLY=1` to skip it). The browser fetches the threaded package only
when the page is cross-origin isolated (the two headers above).

For headless work, the CLI:

    cargo run -p fieldsmith-cli -- volume examples/bracket.fsm
    cargo run -p fieldsmith-cli -- solve examples/bracket.fsm --report
    cargo run -p fieldsmith-cli -- export examples/bracket-filled.fsm --3mf bracket.3mf --stl bracket.stl

The CLI exports binary STL and 3MF; fill documents export the graded part, `--solid` the
body alone. A plain stack of add-shaped nodes (no subtract, intersect, group modifiers,
blends, or fill) exports one labelled object per node so slicers can union them; anything
that shapes the body further exports as one labelled union, and the in-app export always
does.

## What is verified, and what is not

`docs/verification/` carries a report per case: volumes against closed-form answers (V4),
plates with holes against Heywood theory (V3), the cantilever against theory (V2), the
graded lattice's stiffness against a resolved reference mesh (V7), the lattice law's
direction dependence against rotated coupons (V36), the graded fill's thermal re-solve
(V32), and imports against their own headers (V10, V14, [slicer.md](docs/verification/slicer.md)).
The web app's flows have browser-driven e2e families beside them (the graded-lens runs
and the *-e2e.md reports). Heavy release-run cases are ignore-marked in the test suite
and run in the release stage. Limits that are not covered get stated in those reports rather than
glossed: the graded fill's surface carries measured non-manifold spoke edges and
interpenetrating wall tiles where TPMS sheets cross or pinch inside one cell (V8), while
adaptive dual contouring closes both analytic and mesh-leaf graded fills with `0` open
edges and `manifold = yes` in PrusaSlicer 2.9.6 (slicer.md).

## Limitations

Physics

- Linear-elastic, small strain. The body's material is isotropic per element. The fill's
  lattice elements carry the measured cubic anisotropy law on the five TPMS families whose
  45°/0° coupon ratio V36 measured from 13.6 % to 130 %, homogenising in the lattice's own
  axes, with the density exponent the family's measured isotropic one; that the direction exponents stay equal at other
  densities is an assumption, stated in the law. Families without a coupon fit (the
  primitive network) keep the isotropic law.
- The yield and buckling factors are literature-law estimates (Gibson–Ashby open-cell
  strength and elastic collapse): a linear-elastic solver cannot measure plasticity or
  buckling. The rigorous buckling route (geometric-stiffness eigenproblem) is tracked, not
  built.
- The compliance is a certified lower bound, and on the isotropic route the equilibrated
  upper end certifies the bracket (V34, V35). The cubic-anisotropic route refuses
  equilibration rather than mis-certifying. Lattice elements homogenise at sub-cell
  mesh sizes; the resolved geometry is what V6/V7 measure.

Modelling and files

- One material per part: the solve is single-material, and a volume's material label
  travels with the 3MF export (validation reports unknown label ids as errors and mixed
  labels as a warning). Per-part colours are render colours only.
- No topology optimisation, no CFD analysis, no g-code generation, no mobile layout.
- Per-node material labels are not adopted on 3MF import (another tool's ids are not
  fieldsmith material ids). STEP import is CLI-only, tessellated; no B-rep editing.

Thermal

- Steady conduction only. `temperature`, `convection`, `heatflux` on labelled regions;
  everything else insulated. Each thermal case expands the part by α·(T − tref) (tref is
  the solve statement's, 20 °C when unstated). One isotropic conductivity: its value drops
  out of the steady solution, so there is no k to set; the graded fill's re-solve instead
  maps k per element through its material fraction. Radiation, transient heat, and
  per-region conductivity are not modelled. Materials need a sourced CTE for thermal
  cases, which validation refuses without one (the web setup lens has no editor for a
  custom material's `a=` value; custom materials state it through the `.fsm` statement).

Working across machines and rungs

- Rung-to-rung changes on the refinement ladder are estimates, not bounds (V17, V18).
  Worker-channel solves are not promised bit-identical; the divergence is measured. On the
  two measured x86_64 hosts the tracked values reproduced bit-identically and only wall
  time differed (data/costs/costs.csv). On the measured aarch64 phone (SM8750, V25) the
  divergence is real: ~1e-13..3e-11 relative on tracked values (faer SIMD kernels). The
  V22 canon quantization ties every compared field; other machine classes carry a claim
  only once measured.
- One load case at a time in the setup lens; envelopes exist in the solver but the UI
  shows one case or the envelope, not both side by side. Modal analysis takes the same
  supports and ignores loads. The in-browser eigensolve completed without a memory error
  at a 190 k-DOF rung; larger meshes may fail at the wasm memory limit with a named
  error.

## Project

Successor to the MorphoForge experiment. Built on
[`fenics-rs`](https://github.com/GlassOnTin/fenics-rs), consumed as a submodule.

License: [AGPL-3.0-or-later](LICENSE).