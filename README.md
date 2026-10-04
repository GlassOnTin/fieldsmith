# Fieldsmith

Shape a part, load it, and grow its interior to fit the load — in the browser, with no server.
Fieldsmith is an implicit-field CAD and analysis tool: a document is a small text programme,
its geometry is an SDF-like field, and every measurement (volume, stress, printed mass) comes
from integrating that field, not from a boundary representation.

**Status:** pre-release. The model → solve → fill → export flow works locally and is verified
by the cases listed in [docs/verification/](docs/verification/). The performance budgets in
[GOAL.md](GOAL.md) are not all met yet; see [docs/verification/V8.md](docs/verification/V8.md)
for the export budget's current numbers.

## Building and running

    cargo test -p fieldsmith-verify           # verification cases (V1–V9)
    npm --prefix web install                  # once
    bash scripts/build-wasm.sh                # build the WebAssembly analysis
    npm --prefix web -- run build             # production build into web/dist

For development, `bash scripts/build-wasm.sh && npm --prefix web -- run dev` serves the app
locally. Or work headless with the CLI:

    cargo run -p fieldsmith-cli -- volume examples/bracket.fsm
    cargo run -p fieldsmith-cli -- solve examples/bracket.fsm --report
    cargo run -p fieldsmith-cli -- export examples/bracket-filled.fsm --3mf bracket.3mf --stl bracket.stl

The CLI exports binary STL and 3MF (millimetres, part name and material recorded); fill
documents export the graded part, `--solid` the body alone.

## What is verified, and what is not

`docs/verification/` carries a report per verification case: volumes against closed-form
answers (V4), a plate-with-hole against theory (V3), the cantilever against theory (V2), and
the graded lattice's stiffness against a resolved reference mesh (V7). Limits that are not
covered get stated in those reports rather than glossed: the export mesh is uniform (there is
no adaptive spacing yet) and the graded fill's surface carries measured non-manifold edges
where TPMS sheets cross or pinch inside one cell (V8, "Non-manifold edges"); slicers take
the solid as the union of the closed shell.

## Limitations

Linear-elastic, small strain, isotropic material only; homogenisation of the fill is
isotropic (the actual TPMS sheets are not, and the error against a resolved reference is
measured, not assumed, in V7). One material per part. No STEP or B-rep import, no topology
optimisation, no modal, thermal or CFD analysis, no multi-material assembly, no g-code
generation, no mobile layout. One load case at a time in the setup lens; envelopes exist in
the solver but the UI shows one case or the envelope, not both side by side.

Successor to the MorphoForge experiment. Built on [`fenics-rs`](../fenics-rs).

License: AGPL-3.0-or-later.
