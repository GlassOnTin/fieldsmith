# Fieldsmith

Shape a part, load it, and grow its interior to fit the load — in the browser, with no server.
Fieldsmith is an implicit-field CAD and analysis tool: a document is a small text programme,
its geometry is an SDF-like field, and every measurement (volume, stress, printed mass) comes
from integrating that field, not from a boundary representation.

**Status:** v0.2.0. The model → setup → solve → fill → export flow works locally and is
verified by the cases in [docs/verification/](docs/verification/); imported meshes
(GLB, OBJ, binary and ASCII STL, and 3MF — our own export format) become field leaves
with labelled faces and region-driven extension (V10). The GOAL.md performance budgets
pass in the headed Chromium referee on a workstation — [budgets.md](docs/verification/budgets.md); the
budgets' stated mid-range laptop and Firefox measurements have not been run.

## Building and running

    cargo test -p fieldsmith-verify           # verification cases (V1–V10)
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
documents export the graded part, `--solid` the body alone.

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
measured, not assumed, in V7). One material per part. No STEP or B-rep import, no topology
optimisation, no modal, thermal or CFD analysis, no multi-material assembly (3MF import
reads single-object files; multi-material labelling is not built), no g-code
generation, no mobile layout. One load case at a time in the setup lens; envelopes exist in
the solver but the UI shows one case or the envelope, not both side by side.

Successor to the MorphoForge experiment. Built on [`fenics-rs`](../fenics-rs).

License: AGPL-3.0-or-later.
