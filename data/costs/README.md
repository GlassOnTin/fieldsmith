# data/costs

Measured cost rows that seed the rung estimator (`crates/fieldsmith-cli/src/store.rs`:
when a fresh rung has fewer than two points to fit, the projection falls back to the
median wall of same {cpu, engine, target_dofs} rows). `costs.csv` is that historical
store, shared in the repo; every row was measured on a machine the project controls,
and each row block is prefixed with a provenance comment (date, machine, engine,
cli/wasm, source doc).

## What rows may contain

Nothing identifying. The schema (`costs.csv` header comment) allows only: month,
platform, engine, cpu, cores, ram, rung, DOF target and achieved, element count,
leaf count, wall time, and the two change estimates. No file path, no document name,
no geometry, no field results, no timestamp finer than the month. Provenance
comments — not rows — may name the shipped example fixtures (`examples/bracket.fsm`,
`bracket-filled.fsm` used by the budget probes); the row itself stays shape-only.

## Contributing

The contribution path is a PR. There is no submit endpoint and none is planned.

1. Run a ladder locally: `fieldsmith solve DOC.fsm --rungs 30000,150000,750000`
   (optional `--tolerance` / `--budget`; `--no-store` skips the personal store).
2. `fieldsmith costs` prints the last run's row(s) exactly as they should appear in
   `costs.csv`. It never writes anything, to the personal store or elsewhere.
3. Open a PR: paste those lines under a provenance comment naming the date, machine
   (cpu string as it reports itself), engine line, and which fixture produced it.
   Keep rows schema-shaped; nothing else goes in.

Wasm rows come from `web/tests/probe-budgets.mjs` runs summarised in
`docs/verification/budgets*.md`. Their wall is the edit→level wall and includes
document-UI overhead, so it is not comparable to a CLI leg's wall; that is stated in
the `costs.csv` header comment and repeated here.

The change columns are rung-to-rung estimates, not error bounds (V17). Paste them as
measured; a seed must never hold a bound dressed as a measurement.