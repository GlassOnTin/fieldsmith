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
- [x] One-way coupled solve: steady linear conduction on the existing tet mesh;
      temperature regions carry fixed values placed on labelled faces; the temperature
      field then enters the elasticity solve as thermal strain from a stated reference
      temperature. Both fields render in the Results lens, temperature with a legend and
      units.
- [x] A temperature region is placed like a load (pick a face, set the value); `.fsm`
      round-trips it losslessly with named validation errors.
- [x] Single isotropic material for the coupled solve; per-volume material laws are named
      for later, not attempted here.
- [x] V19 and V20 pass (three analytic cases below).
- [x] The thermal solve leg gets budget rows on the referee and on the N100 class
      (below).

Design (plan commit 2026-10-13): `temperature` statements carry fixed values on
labelled regions like loads do (`temp=<°C>`, case default "Thermal 1"), and `solve
tref=` states the reference temperature. Dirichlet on every P2 node of the region's
facets; conflicting values on shared nodes are a named setup error. Steady conduction
on the existing tet mesh with one material is conductivity-independent (∇·(k∇T)=0
with constant k is Laplace), so no conductivity property enters v0.4 and unlabelled
faces are insulated by the natural boundary condition; README names this scope. The
material gains a single thermal-expansion coefficient (sourced; same honesty pattern
as the ν entries). Each thermal case is a conduction solve (scalar two-level PCG
beside the elastic one) plus an elastic solve driven by the thermal load —
f = (3λ+2μ)·α·∫ΔT·∇φ dV, degree-3-exact quadrature; stress recovery subtracts
(3λ+2μ)·α·ΔT·I. Thermal cases run as their own results beside the mechanical ones;
a case name claimed by both a load and a temperature is a named setup error (mixed
cases named for later). V19(a) uses three single-component supports on the faces
through the origin: the exact free-expansion solution u = αΔT·(x,y,z) satisfies
them and lies in the P2 space, so the discrete stress must sit at roundoff.
V19(b) probed mid-length (Saint-Venant end effects); V20 takes the Lamé-class
generalized-plane-strain cylinder solution with the free-end force balance.

### C. The connection — API, MCP, and the worker you own (M15 — evidence: docs/verification/V21.md, V22.md)
- [x] A versioned, headless JSON API over the verified crates: document and question in
      — tolerance mode, budget mode, sweep mode — result out with provenance, rung
      records and measurements. A contract case pins the schema; version bumps are named
      changes, not silent ones.
- [x] An MCP server over that API, running as a local process on the user's machine:
      typed tools to open and inspect a document, place supports, loads and temperatures,
      solve to a declaration, export. Nothing hosted; no accounts.
- [x] `fieldsmith worker` (working name) — the worker on machines the user owns:
      enrols once by token to a session host started on a machine the user owns; connects
      outbound only; runs solve jobs with the same verified crates; artifacts stay in the
      directory the operator states. The session host binds only where the user says
      (default loopback). Reaching a worker across networks is the user's own path
      (Tailscale/WireGuard/SSH class); the browser-attached session host is named for
      later, and this milestone's sessions are the CLI/API ones.
- [x] Verified, not promised: session traffic captured during a full run and scanned for
      plaintext model or field bytes; the worker's artifact directory scanned afterwards;
      the same `.fsmz` re-solved locally and on the worker with the divergence measured
      and reported. Cross-machine bit-identity is not promised; the divergence numbers
      are stated in the case (V21).
- [x] Remote rungs enter the M13 measures; the worker's cost is the user-stated rate
      times measured time, shown per rung.
- [x] V21 (privacy evidence) and V22 (API/MCP contract) pass (below).

Design (plan commit 2026-10-14). Three slices, one shared verified core:

1. **The API** — new crate `fieldsmith-api`. It owns two things: a JSON envelope
   (request: `apiv` schema version, `doc` as `.fsm` text or `docz` as base64 `.fsmz`
   bytes, `question` = ladder/solve/modes with the same tolerance/budget fields the CLI
   parses; response: `apiv`, `engine` (the `store::engine_identity()` string), `status`
   = ok / refusal / error, the per-rung records and their change measurements, the cost
   fit and projections, and named errors), and the declared-question runner moved out of
   the CLI's `solve_ladder` so there is one verified loop: records stream through a sink
   trait; the CLI keeps its prose prints as the sink implementation. The one-shot verb
   `fieldsmith api` reads a request on stdin and writes the response on stdout — no
   server process to keep alive, and byte-stable golden cases are just files. Errors of
   processing (bad request, setup error) are a JSON `error` status with exit code 2;
   ok and refusal both exit 0 (a refusal is an answer).
2. **The MCP server** — small binary over stdio, hand-rolled JSON-RPC (MCP is
   initialize/tools-list/tools-call; a server library and a tokio runtime would outweight
   the surface). It holds one document open at a time via the API crate's functions:
   tools for inspecting, placing supports/loads/temperatures, solving to a declaration
   (same runner, so tools return the same result the API does for the same document —
   that is V22's second column), exporting. Local process, nothing hosted, no accounts.
3. **The worker** — `fieldsmith session` starts the host: binds where the user says
   (default `127.0.0.1`), prints an enrolment token once (the host's Noise static key +
   a pre-shared key, base64). `fieldsmith worker enrol T <host:port>` stores the token;
   `fieldsmith worker` then connects outbound, Noise_XXpsk3_25519_ChaChaPoly_SHA256
   (via the `snow` crate — hand-rolled crypto is off the table). Jobs are the API
   request (document inside a `.fsmz` built at send time); results come back as the API
   response records. The worker writes artifacts only under the operator-stated
   directory and prints nothing about geometry or fields; the host shows per-rung walls
   from the worker's own records and computes cost = user-stated rate × measured time.

V21's capture: tcpdump on loopback across a full worker run, scanned for the document's
known plaintext patterns (shape names, coordinates, load vectors, field bytes); the
expected match count is zero — handshake frames and one message-length prefix are
accounted for. Worker-side scan: everything outside the stated artifact dir (temp, /tmp,
stdout) carries no plaintext document/field bytes; the artifact dir holds what the
operator asked for, and the listing is recorded. Divergence: the same `.fsmz` solved
locally and through the worker, peak von Mises and max deflection compared, numbers
stated.

### D. Parameter sweep (M16 — evidence: docs/verification/V23.md)
- [x] Sweep any numeric document parameter over a stated range and point count; each
      point solves to its declaration (tolerance or the ladder default); the headless
      runner emits a table (JSON and CSV) with per-point measured records.
- [x] The Results lens shows the sweep table — parameter, tracked outputs, per-point box
      and time — and exports the same CSV.
- [x] V23 passes (below).

Design (plan commit 2026-10-14). Three slices, one shared grammar:

1. **The parameter** — a numeric document parameter is addressed as `<id>.<attr>[.<k>]`:
   shape attributes by the document model's own names (`box`: `size`/`round`,
   `cylinder`: `radius`/`height`/`round`, `sphere`: `radius`, `capsule`: `radius`/
   `length`, `torus`: `major`/`minor`, `extrude`: `height`; `size`, `at` and `rotate`
   take a 0–2 component index), loads (`force` components, `mpa`, `total`) and
   temperatures (`celsius`) by their statement ids. The address is resolved against the
   parsed document when the request arrives; an unknown id or a non-numeric attribute
   is a request error naming what failed to resolve. Applying a value is an edit on the
   parsed model, per point. Extrude profile vertices, region/support selector numbers
   and the solve settings are outside the sweep grammar (stated in the report).
2. **The sweep question** — a `sweep` variant in the API's question grammar: `param`
   (the address), `values` (the stated range and point count), and the per-point
   declaration the D box names — `rungs` and `tolerance`, defaulting exactly like the
   ladder; a per-point budget is outside this grammar (a point runs to its tolerance or
   to the schedule's cap). Each point copies the document, applies its value, and
   solves through the same declared-question runner, so every point's box (rung, DOFs,
   elements, change) and its time are measured, not reconstructed. The envelope takes
   its first version bump — `apiv` 1 → 2 — named in schema.rs, and V22's golden is
   regenerated at the pinned new version; an `apiv: 1` request is refused with a line
   naming the change.
3. **The headless table + the Results lens** — `fieldsmith api` gains `--csv <file>` for
   sweep questions: one row per point (`value,dofs,elements,wall_ms,deflection_mm,
   vm_mpa,deflection_change_rel,vm_change_rel,status`), the last rung's measured
   numbers. The Results lens gains a sweep section: a parameter picker over the
   document's addressable parameters, from/to/point-count inputs, a run that drives the
   app's ladder machinery per point on a document clone (restoring the user's value
   when it finishes), a table of value, tracked outputs, per-point box and time, and an
   export button building the same column set — the CSV the runner emits and the CSV
   the lens exports are the same file to the byte on the same run. Lens evidence: a
   headless probe driving the sweep (report beside the other `-e2e` docs).
4. **The cases** — V23 runs headless through the API grammar: the V1 cantilever
   (`box beam size=(100, 10, 10)`, 100 N down at the tip) sweeping `beam.size.2` over
   five values of [8, 12] mm at rungs 30000,150000; the per-point measured deflection
   ratios against the beam-theory relation δ ∝ 1/h³, first run measured and the case
   tolerance pinned from it, per-point boxes asserted present. The throughput row is
   the same declaration style: a 5-point bracket sweep at the reference 1 % deflection
   through the api verb, on the referee and on the N100 (ssh, as M15 measured), points
   per hour into budgets.md and rows in costs.csv.

### E. Carried
- [x] CI runs the whole ladder: V1–V16 unchanged, V17–V23 as they land; `budgets.md`
      regenerated per milestone on the referee. (The 2026-10-14 close run: all nine
      stages green per /tmp logs — clippy, typecheck, workspace V1–V23, wasm, the
      headless browser suite, V16 isolated, V6, V7, V14.)
- [x] README limitations gain the honesty lines: rung error estimates are estimates from
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
The M16 sweep rows are unmeasured until M16's close.

Measured 2026-10-14 at M14 close:
- Thermal-leg row: bracket-thermal (mount 80 °C, pin 20 °C, tref 20), third run of three, wall = mesh + static solve + thermal (the banner's disjoint components). Referee refined thermal leg 1245 ms vs the v0.3 refined static row 1164.928266 ms = 1.07×; N100 refined thermal 2723 ms vs 2642.862383 ms = 1.03× — both inside the ≤ 2× target. The N100 binary is the referee-built baseline-x86_64 shipped over ssh; its mesh+static part reproduces the 2026-10-13 row within 0.4 %, so the thermal run rides the measured engine behaviour. Components and rows in data/costs/costs.csv.

Measured 2026-10-14 at M15 close:
- Worker round-trip row: `fieldsmith session` on the referee served seven jobs of the
  same ladder (bracket, rungs 30000,150000) to `fieldsmith worker` on the N100 over
  LAN, each job a fresh outbound TCP + Noise_XXpsk3 handshake. The first job after the
  worker's spawn measured a 10.0 s host wall: that wall contains spawn, enrolment and
  first connect, not the per-job mechanism (an earlier unscripted pairing measured
  82.9 s the same way). The six setup-free jobs measured 3.1–3.2 s host walls against
  billed rung totals of 3.140–3.169 s from the worker's own records — the mechanism
  overhead is bounded by the host print's 0.1 s granularity to ≤ ~100 ms per job
  (≤ ~3 % of the wall), inside the ≤ 20 % target. The N100 local comparator, same day,
  third run of three: 558.747089 / 2617.657443 ms, within 1.8 %/1.0 % of the
  2026-10-13 rows. Rows and sample spans in data/costs/costs.csv.
- Cross-machine divergence (C4): the same request through the referee's api verb and
  through the N100 worker returned bit-identical tracked values on these two x86_64
  hosts, same engine string — rung 0 deflection 2.330609765727498 mm / von Mises
  41.97761660839724 MPa, rung 1 1.6762950333172848 mm / 47.02818468117891 MPa — with
  identical rung-1 change values; divergence appears only in wall time, at the ~2.2×
  machine ratio. That is a statement about these hosts, not about bit-identity in
  general; the README limitation carries the scope.
- C5 cost leg: the session host printed each rung's cost as rate × measured time (the
  operator's 12/h: rung 0 0.6 s → 0.0019, rung 1 2.6 s → 0.0087), and the worker's
  records entered the cost store as platform-"worker" rows — remote rungs are part of
  the measure set (data/costs/costs.csv).
- C1 status: the api's ladder (tolerance/budget), solve and modes questions are
  measured and byte-pinned by V22; the sweep-mode clause waits for M16, where
  schema.rs names the sweep question as the envelope's first version bump.

Measured 2026-10-14 at M16 close:
- Sweep-throughput row: the V23 cantilever sweep through `fieldsmith api` — five points
  on beam.size.2 over 8..12 mm, rungs 30000,150000, tolerance deflection:1%, one
  process per machine, costs store off (rows seeded by hand; the api verb never writes
  the store). Referee: 6.071 s process wall → 2965 points/hour, engine sums 6.065 s
  (declared ≥ 6). The regenerated budgets.md carries the probe's own fresh measure of
  the same question — 2950 points/hour at 6.10 s — agreeing within 0.5 % of the direct
  run. N100: the referee-built binary shipped over ssh, 13.660 s process wall →
  1318 points/hour, engine sums 13.643 s (declared ≥ 2). Every point met its
  declaration at rung 1 (2 rungs bought) on both machines. Twenty rows in
  data/costs/costs.csv (per point per rung per machine; rung-0 change cells empty by
  the schema rule, each rung-1 row carrying that point's change values). V18's
  tie-in test grew with the file: all twenty rows now pin walls, per-point achieved
  dofs and change cells against embedded constants (dofs shared across the machines
  per the identity datapoint). The N100
  number lives in the store and this ledger — that leg is a native measurement, not a
  browser one, so the laptop budget doc does not carry it.
- Cross-target identity (C4): the N100 sweep envelope is bit-identical to the
  referee's in every tracked field — per-point records (dofs, elements, element sizes,
  tracked deflection and von Mises, the change columns), the met blocks and
  field_leaves — same engine string; divergence is wall time only, ~2.25× on this run
  (rung walls 2.19–2.45×). The third x86_64↔x86_64 datapoint (after M15's
  worker-vs-api pair and M14's thermal static part). Scope as measured: these hosts,
  this question.
- C1 status closed: the sweep-mode clause shipped with the apiv 1→2 bump named in
  schema.rs (M16 slice 1), V22's golden regenerated at the pinned new version, and
  V23 pinning the per-point boxes; the api takes document and question in all three
  modes with provenance out. C1 ticks at this close.

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