# Fieldsmith

Shape a part, load it, and grow its interior to fit the load — in the browser, with no server.
Fieldsmith is an implicit-field CAD and analysis tool: a document is a small text programme,
its geometry is an SDF-like field, and every measurement (volume, stress, printed mass) comes
from integrating that field, not from a boundary representation.

**Status:** v0.2.0. The model → setup → solve → fill → export flow works locally and is
verified by the cases in [docs/verification/](docs/verification/); imported meshes
(GLB, OBJ, binary and ASCII STL, and 3MF — our own export format) become field leaves
with labelled faces and region-driven extension (V10); a multi-object 3MF imports as
one field leaf per placed object (build items expand), with the file's base-material
names recorded on the leaves. The GOAL.md performance budgets are measured in the headed
Chromium referee on the workstation and on the mid-range N100 box ([budgets.md](docs/verification/budgets.md)
and the budgets-* reports beside it); every run's walls live in
[data/costs/costs.csv](data/costs/costs.csv). The same verified crates also reach other
programs: the `api` CLI verb takes one JSON request on stdin and writes one envelope on
stdout, `fieldsmith-mcp` is an stdio MCP server over that API, and `fieldsmith session`
/ `fieldsmith worker` host and serve jobs between machines the user owns over
Noise-encrypted connections — usage in each binary's help text.

## Building and running

    git clone --recurse-submodules https://github.com/GlassOnTin/fieldsmith
    cargo test -p fieldsmith-verify           # verification cases (V1–V23)
    npm --prefix web install                  # once
    bash scripts/build-wasm.sh                # build the WebAssembly analysis
    npm --prefix web -- run build             # production build into web/dist

For development, `bash scripts/build-wasm.sh && npm --prefix web -- run dev` serves the app
locally.

### Threads and hosting

`build-wasm.sh` builds two packages of the same crate: `web/src/pkg` (single thread, stable
toolchain) and `web/src/pkg-mt` (shared memory plus a rayon pool on Web Workers via
wasm-bindgen-rayon; needs a nightly toolchain with `rust-src` for `-Z build-std`;
`FIELDSMITH_WASM_ST_ONLY=1` skips it). The browser fetches one of them: the threaded package
only when the page is cross-origin isolated, which takes two response headers on every file
the host serves:

    Cross-Origin-Opener-Policy: same-origin
    Cross-Origin-Embedder-Policy: require-corp

The Vite dev and preview servers send both (`web/vite.config.ts`). A static host without them
still serves a working app on one thread, with meshing and export several times slower; the
budget numbers in `docs/verification/budgets.md` are measured on the threaded build.

Or work headless with the CLI:

    cargo run -p fieldsmith-cli -- volume examples/bracket.fsm
    cargo run -p fieldsmith-cli -- solve examples/bracket.fsm --report
    cargo run -p fieldsmith-cli -- export examples/bracket-filled.fsm --3mf bracket.3mf --stl bracket.stl

The CLI exports binary STL and 3MF (millimetres, part name and material recorded); fill
documents export the graded part, `--solid` the body alone. When the document is a plain
stack of add-shaped nodes (no subtract/intersect, group modifiers, blends, or fill), the
3MF carries one object per node, each labelled with its material (the node's label, else
the document material); slicers union the objects into the printed solid. Anything that
shapes the body beyond the volumes' own shapes exports as one labelled union, and the
in-app export always does.

## What is verified, and what is not

`docs/verification/` carries a report per verification case: volumes against closed-form
answers (V4), a plate-with-hole against theory (V3), the cantilever against theory (V2), the
graded lattice's stiffness against a resolved reference mesh (V7), and an imported mesh
against its file's own header and the slicer that consumes it (V10). Limits that are not
covered get stated in those reports rather than glossed: the graded fill's surface carries
measured non-manifold edges where TPMS sheets cross or pinch inside one cell
(V8, "Non-manifold edges"); slicers take the solid as the union of the closed shell.

## Limitations

Linear-elastic, small strain, isotropic material only; homogenisation of the fill is
isotropic (the actual TPMS sheets are not, and the error against a resolved reference is
measured, not assumed, in V7). One material per part: the solve is single-material, and a
volume's material label travels with the 3MF export for the slicer (validation reports
unknown label ids as errors and mixed labels as a warning). STEP import is CLI-only: a
STEP file is tessellated to a mesh, and there is no B-rep editing. Per-part colours are
render colours, in the app and on the 3MF export; neither affects what is printed or what
the solve assumes. No topology optimisation, no CFD analysis, per-node material labels are not
adopted on 3MF import (another tool's ids are not fieldsmith material ids), no g-code
generation, no mobile layout. Modal analysis reports the first natural frequencies of
the same linear small-strain model; it takes the same supports and ignores loads. The
shapes are computed but not visualised; the in-browser eigensolve completed without a
memory error at a 190 k-DOF rung, and larger meshes may fail at the wasm memory limit
with a named error. Thermal analysis is steady conduction with fixed temperatures on
labelled regions: the region's facet nodes take the stated value, everything else is
insulated, and each thermal case then expands the part by α·(T − tref), the reference
being the solve statement's tref (20 °C when unstated). With one isotropic conductivity
the value of k drops out of the steady solution, so there is no conductivity property to
set. Materials need a sourced thermal expansion coefficient for thermal cases, which
validation refuses without one (the web setup lens has no editor for a custom material's
`a=` value, so custom materials state it through the `.fsm` statement); convection,
radiation, transient heat and per-region conductivity are not modelled. The graded fill re-solves mechanical cases only, so the
fill lens has no temperature field. Rung-to-rung changes on the refinement ladder are estimates, not
bounds: on the V1 cantilever the ratio of remaining error to change spans 13× between
regimes (V17), and the wall-time estimator reports its own residual instead of none
(V18). Solving the same document on another
machine over the worker channel is not promised bit-identical, and the divergence is
measured rather than assumed: on the two measured x86_64 hosts the tracked values
reproduced bit-identically and only wall time differed, by the machine ratio
(data/costs/costs.csv). Cross-architecture the divergence is real: on the measured
aarch64 phone (SM8750, V25) tracked values diverge at ~1e-13..3e-11 relative (faer SIMD
kernels; the sweep's small change fields to ~7e-8), while the V22 canon quantization
ties every compared field; any other machine class carries its claim only once measured. One load
case at a time in the setup lens; envelopes exist in
the solver but the UI shows one case or the envelope, not both side by side.

Successor to the MorphoForge experiment. Built on
[`fenics-rs`](https://github.com/GlassOnTin/fenics-rs), consumed as a submodule.

License: AGPL-3.0-or-later.
