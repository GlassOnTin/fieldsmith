# Fieldsmith — Goal (v0.4)

**Open.** Drafted 2026-10-13 from the owner's agenda and the same-day VISION refresh
(`18754c2`; the computed-leaves-the-desk and declared-question commitments live there).
Milestones are declared, nothing measured yet. Each milestone enters execution through a
reviewed plan, as M12 did; design detail lands there, not here. If a criterion turns out
to be wrong, change this file in a reviewed commit; do not quietly skip it.

This file defines **what "done" means for v0.4**. It is deliberately concrete and testable.
The [VISION](VISION.md) says *why*; this says *what.* The previous goal, with its close-out
audit, is in git history at `0b92e1c` (v0.3: the CAD round-trip, M8–M12).

The ranking follows the owner's criterion of 2026-10-13: does the change make a leading
commercial tool's capability available to everyone, freely — licence and hardware class
both count as "freely". That is why v0.4 takes a flow rather than CAD-parity asks
(grouping, sketch, section planes), which are named for later.

---

## The goal in one sentence

> The user declares what a good answer means — a tolerance to meet or a time to spend —
> and Fieldsmith answers with the measured rungs, their cost, and the result itself; the
> first packaged engineering flow (steady heat driving thermal expansion) puts a paid
> simulation capability within reach of anyone with a browser and the repo; and when the
> desk box is too small, the analysis runs on machines the user owns, driven through an
> API agents and scripts can use.

---

## The reference task — "the declared question"

The acceptance test for v0.4, on the reference fixture ladder (provenance stated as in
v0.3):

1. Open the bracket document; confirm the carried ladder still holds (V1–V16 in CI).
2. Ask in **tolerance mode**: tip deflection to within a stated mm. The ladder refines
   until the rung-to-rung change sits inside it; the report states the per-rung error
   estimates, the time each rung took, the total, and the residual.
3. Ask in **budget mode**: a stated time. The report states the rungs that budget bought
   and the tolerance they imply, measured after the fact.
4. Switch to the **thermal flow**: mounting face held at 20 °C, working face at 80 °C.
   Solve conduction, then thermal expansion; read the thermal stress beside the
   mechanical one. The temperature field renders with a legend and units.
5. The desk box too small? Start a worker on another machine you own, enrol it by token,
   re-run the same declaration. The report states the wall time, the cost at your own
   rate, and the measured divergence from the local solve — not bit-identity.
6. Sweep one parameter over a range, headless; the table carries each point's own box,
   measured time and outputs; the Results lens shows the same table.
7. Save as `.fsmz`, reload, re-solve: bit-identical on the same machine (the v0.3
   criterion holds), and the rung records are part of what the Results view shows.

**Pass:** the machine-side legs of steps 2–7 are covered by V17–V23 plus the carried
ladder (V1–V16, still in CI); the privacy leg by the capture artifacts named in V21; the
community rows by `data/costs/` being present with provenance stated.

---

## New acceptance criteria

### A. Measured rungs and the declared question (M13 — evidence: docs/verification/V17.md, V18.md, `data/costs/`)
- [x] Every refinement rung becomes a first-class record: solve wall time, element count,
      and the error estimate taken from rung-to-rung change of the tracked quantities
      (tip deflection, peak von Mises). Records stream into the web UI and print from the
      CLI. Measured, not claimed: a headless replay asserts the records are the solver's
      own — time deltas within the run timer's resolution, estimates equal to the
      two-solve differences to floating-point roundoff.
- [x] The Results view shows the rung table with provenance — engine, element type and
      count, per-rung time and error estimate, load case — every entry with units and a
      stated source.
- [x] Declared-tolerance mode: the user states a tolerance on a tracked quantity; the
      ladder stops when the measured rung-to-rung change sits inside it, and the report
      states total time and the residual. A tolerance the stated budget cannot meet is
      answered with a named refusal (the V15 load-plane-fit precedent) — never a
      stretched answer.
- [x] Declared-budget mode: state a time budget; the report states the rungs it buys and
      the tolerance they imply, measured from the rung record after the fact.
- [x] Cost estimates carry their own error: the fit over rungs ≤ k projects rung k+1,
      and the report shows the fit residual next to the projection (for example
      "about 4.7 min, fit residual 6 %"). Cost is time in this milestone; the worker
      (M15) converts it with the user's own rate. Local rung history seeds the estimator
      on the next run (machine-specific; the store is chosen at plan time).
- [x] The community rows live in the repo: `data/costs/` carries a header stating every
      field of a row, and seed rows derived from the current referee and N100 budget
      baselines with their provenance stated. A CLI command prints the record row for
      the last run — shape-only fields: DOF, element count, field-leaf count, CPU class,
      cores, RAM, engine, wasm-or-CLI, rung, wall time, error where known; timestamps at
      month level; no geometry, no names. The command is a redaction preview: it prints
      exactly what would be contributed and writes nothing; contribution is the user
      opening a pull request. There is no submit endpoint.
- [x] V17 and V18 pass (below).

### B. First flow — steady heat into thermal expansion (M14 — evidence: docs/verification/V19.md, V20.md)
- [ ] One-way coupled solve: steady linear conduction on the existing tet mesh;
      temperature regions carry fixed values placed on labelled faces; the temperature
      field then enters the elasticity solve as thermal strain from a stated reference
      temperature. Both fields render in the Results lens, temperature with a legend and
      units.
- [ ] A temperature region is placed like a load (pick a face, set the value); `.fsm`
      round-trips it losslessly with named validation errors.
- [ ] Single isotropic material for the coupled solve; per-volume material laws are named
      for later, not attempted here.
- [ ] V19 and V20 pass (three analytic cases below).
- [ ] The thermal solve leg gets budget rows on the referee and on the N100 class
      (below).

### C. The connection — API, MCP, and the worker you own (M15 — evidence: docs/verification/V21.md, V22.md)
- [ ] A versioned, headless JSON API over the verified crates: document and question in
      — tolerance mode, budget mode, sweep mode — result out with provenance, rung
      records and measurements. A contract case pins the schema; version bumps are named
      changes, not silent ones.
- [ ] An MCP server over that API, running as a local process on the user's machine:
      typed tools to open and inspect a document, place supports, loads and temperatures,
      solve to a declaration, export. Nothing hosted; no accounts.
- [ ] `fieldsmith worker` (working name) — the worker on machines the user owns:
      enrols once by token to a session host started on a machine the user owns; connects
      outbound only; runs solve jobs with the same verified crates; artifacts stay in the
      directory the operator states. The session host binds only where the user says
      (default loopback). Reaching a worker across networks is the user's own path
      (Tailscale/WireGuard/SSH class); the browser-attached session host is named for
      later, and this milestone's sessions are the CLI/API ones.
- [ ] Verified, not promised: session traffic captured during a full run and scanned for
      plaintext model or field bytes; the worker's artifact directory scanned afterwards;
      the same `.fsmz` re-solved locally and on the worker with the divergence measured
      and reported. Cross-machine bit-identity is not promised; the divergence numbers
      are stated in the case (V21).
- [ ] Remote rungs enter the M13 measures; the worker's cost is the user-stated rate
      times measured time, shown per rung.
- [ ] V21 (privacy evidence) and V22 (API/MCP contract) pass (below).

### D. Parameter sweep (M16 — evidence: docs/verification/V23.md)
- [ ] Sweep any numeric document parameter over a stated range and point count; each
      point solves to its declaration (tolerance or the ladder default); the headless
      runner emits a table (JSON and CSV) with per-point measured records.
- [ ] The Results lens shows the sweep table — parameter, tracked outputs, per-point box
      and time — and exports the same CSV.
- [ ] V23 passes (below).

### E. Carried
- [ ] CI runs the whole ladder: V1–V16 unchanged, V17–V23 as they land; `budgets.md`
      regenerated per milestone on the referee.
- [ ] README limitations gain the honesty lines: rung error estimates are estimates from
      rung-to-rung change, not bounds; cross-machine results are not bit-identical —
      divergence is measured and reported; the thermal flow names its scope (steady,
      fixed-temperature boundaries, one material).

---

## Verification cases (must pass in CI)

Cases V1–V16 are the carried ladder and run in CI unchanged (definitions live in
`docs/verification/` and git history). The new cases; each tolerance is a declared
target until first measured, then stated in the case and its report:

| ID | Case | Reference | Tolerance |
| :-- | :-- | :-- | :-- |
| V17 | Rung-estimate honesty: the reference cantilever solved through the ladder; each rung pair's reported estimate (rung-to-rung change in tip deflection) tabulated against the known analytic error (V1's reference) | V1 analytic deflection | Both numbers reported each run; their ratio band recorded across the ladder; pass when a re-run reproduces the recorded band and the UI labels the number an estimate |
| V18 | Estimator residual: after the coarse rung, the fit predicts refined and export rung times; compared with actual on the referee and on the N100 class (headless CLI); `data/costs/` seed rows carry provenance | Predicted vs actual wall time | Each report states its residual; pass when residuals fall inside the band recorded at ship time and the seed rows hold |
| V19 | Thermal, linear: (a) unconstrained uniform ΔT — stress ≈ 0, displacement linear in L·α·ΔT; (b) long bar fixed at both ends, free laterally, uniform ΔT — uniaxial stress E·α·ΔT | Closed form | (a) stress within a stated roundoff-level bound; displacement linearity within a stated %; (b) stress ≤ 5 % of the closed form (declared target, revisable only in a reviewed commit) |
| V20 | Thermal conduction + stress: long hollow cylinder, fixed inner and outer surface temperatures, single material | Exact radial temperature profile; Lamé-class thermal stress formulas | Temperature profile within a stated tolerance; peak hoop and axial stress within a stated % across a 3-point mesh study |
| V21 | Privacy: reference task through a worker; packet capture of the session; scan of the worker's artifact directory; `.fsmz` re-solve local vs worker | Captured bytes and storage listings; divergence table | Zero matches for the model/field-content patterns the capture names, within its stated limits; divergence measured and stated; no capture or empty scan is a failure, not a pass |
| V22 | API + MCP contract: golden request/response headless for the reference document at a pinned engine version; MCP tools return the same result for the same document | Golden JSON | Byte-stable for the pinned version; version bumps named, not silent |
| V23 | Sweep correctness: a 5-point sweep of a parameter with known monotone physics (cantilever section height h: δ ∝ 1/h³) | Analytic ratio | Per-point boxes present; the ratio tracks the h³ relation within the stated tolerance |

---

## Performance budgets

Declared targets below; each row is measured as its milestone lands, and a miss is
answered in a reviewed commit, never a quiet skip. Referee = the workstation referee from
the previous goals; N100 legs are headless/CLI only, as in v0.3 (that box has no GL
driver stack for Firefox — the v0.3 finding).

| Metric | Budget |
| :-- | :-- |
| Rung-record collection adds no measurable leg time | the v0.3 coarse/refined/export rows reproduce within their stated bounds |
| Declared-tolerance ladder, reference task, referee | ≤ 25 s end to end at a 1 % deflection declaration (declared target) |
| Estimator residual, per rung | ≤ 35 %, stated band measured and recorded by V18 |
| Thermal leg (conduction + expansion), reference bracket | ≤ 2 × the v0.3 refined static row (declared target) |
| Worker round-trip overhead vs local CLI, same task | ≤ 20 % on the LAN path (declared target) |
| Sweep throughput at the reference tolerance | ≥ 6 points/hour referee, ≥ 2 points/hour N100 (declared targets) |

Measured 2026-10-13 at M13 close:
- Leg-time row: referee edit→coarse/refined 1352/6025 ms (Chrome), 1678/6494 ms (Firefox), e2e in-page 724/3005 ms — at or below the v0.3 rows, so record collection adds nothing on the referee. On the N100 the Chrome legs moved within the day (9-10 s / 23-25 s vs 4918/15064 this morning); an A/B run of the HEAD (v0.3) web code on the same wasm reproduces the slow numbers, the fresh-wasm rebuild lands the same place, and the same box's Firefox legs reproduce their morning walls — so the move is not the records and is confined to the Chrome path; the split between the old binary's bytes and that session's environment state is not closed (data/costs/costs.csv carries it). The three N100 Chrome rows now hold what the shipped binary measures. **Owner verdict pending on the N100 legs.**
- Tolerance row: the declared 1 % deflection ladder ran 10.9 s end to end on the referee and named its refusal at the 750k cap (deflection change −4.1 %) — an honest answer under the named-refusal rule, but the budget could not buy the declaration. **Owner verdict pending on whether a refusal at the cap counts for this row.**
- Residual row: 13.6 % (referee) / 18.8 % (N100), inside the ≤ 35 % target, bands recorded by V18.
The M14–M16 rows are unmeasured until their milestones.

---

## Build order

| # | Milestone | Done when |
| :-- | :-- | :-- |
| M13 | **Measured rungs + declared question + `data/costs/`** | V17, V18 pass; the rung table visible headless and in the Results lens; the dataset seeded with provenance. |
| M14 | **Thermal-structural flow** — conduction, temperature regions, thermal expansion | V19, V20 pass; temperature regions round-trip; the thermal budget row measured. |
| M15 | **API/MCP + the worker** — JSON API, MCP server, `fieldsmith worker` | V21 passes with its capture artifacts; V22 green; cost rows flow per rung. |
| M16 | **Parameter sweep** — headless runner, Results-lens table | V23 passes; the throughput row measured. |

---

## Explicitly out of scope for v0.4

Transient heat, convection or radiation boundaries · multi-material conduction (the
bi-metal strip waits for it) · thermal behaviour of the graded lattice (homogenised α) ·
nonlinear or material-law analysis · the other flows of the 2026-10-13 agenda (RF/EM,
molding, waves, material laws; each takes its own verification ladder when taken) · any
Fieldsmith-hosted service: relay, accounts, metering, a submit endpoint for cost rows ·
provider provisioner helpers (Terraform-class) · the browser-attached session host ·
bit-identical cross-machine results (divergence measured, never promised) · part
grouping, sketch and section planes (supporting cast under the ranking; named for later)
· multi-parameter sweep grids and optimisers · topology optimisation (SIMP), B-rep
editing, collaboration, drawings (carried from v0.3).

---

## Definition of done

v0.4 is done when every box in sections A–E is ticked, V1–V23 pass in CI, the budget
table is measured on the referee and on the N100 class's headless legs with the owner's
verdicts recorded where a row misses its target, `data/costs/` holds its header and seed
rows with provenance, and the V21 capture artifacts are filed in `docs/verification/` —
and not before. If a criterion turns out to be wrong, change this file in a reviewed
commit; do not quietly skip it. Each milestone's design detail lands in a reviewed plan
before code, as in previous goals.