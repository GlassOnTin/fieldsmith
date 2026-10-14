# Fieldsmith — Goal (v0.5)

**Open.** Drafted 2026-10-14 from the owner's direction in the v0.4-close conversation:
the five leftover threads of the close, worked "each as a goal". The owner's standing
criterion, stated the same day: this is a long-term project, and the quality bar is
outcomes "better than all the rest put together". v0.4 closed at `295e80e` — every box
ticked, the ladder green through V23, tagged `v0.4.0`. Milestones are declared; nothing
measured yet. Each milestone enters execution through a reviewed plan, as previous
goals did; design detail lands there, not here. If a criterion turns out to be wrong,
change this file in a reviewed commit; do not quietly skip it.

The v0.4 ranking question stays the ranking question: does the change make a leading
commercial tool's capability available to everyone, freely — licence and hardware class
both count as "freely". Two of these milestones pass it (the repo's own home, the
ARM datapoint); three are foundations the owner ordered alongside it: two documented
findings that were closed out instead of fixed, and one measurement that was never
explained. Quality of foundations is the point of this goal, per the owner's bar.

---

## The goal in one sentence

> Anyone can clone Fieldsmith, build it from the clone alone, and get the measured
> answer; the measured answer exists on an architecture other than x86_64, on a machine
> the user owns; and the three known imperfections carried out of v0.3/v0.4 — the
> quarter-junction mesh finding, the graded-fill slicer-manifold boundary, the
> unexplained N100 Chrome wall move — are each closed with evidence, whatever the
> evidence turns out to say.

---

## The reference task

The acceptance test for v0.5, provenance stated as in the previous goals:

1. `git clone` the repo from its remote on a fresh path; `cargo build`; run the pinned
   golden api question; the answer is byte-stable at the pinned version (V24).
2. On the phone the user already measured (SM8750, aarch64, OnePlus 13): the reference
   ladder and the V23 five-point sweep through `fieldsmith api`. Walls, points/hour and
   tracked fields measured; the tracked fields compared against the x86_64 literals
   field by field — divergence measured, never promised (V25).
3. V3's quarter model runs the same 5-row mesh study the half-plate passes, at the
   half-plate's tolerances — or the mesher mechanism is named from its own output.
4. The graded-fill reference document goes through headless PrusaSlicer at skin 1.2
   with `manifold=yes` measured — or the manifold boundary is located on a skin/cell
   grid and documented with the probe artifacts.
5. The N100 Chrome split closes: both sides of the M13 move reproduced in one controlled
   run set, so the "not closed" line in the M13 ledger is replaced by a named cause or
   explicit falsifications.

**Pass:** V24 passes in CI; V25 is measured at its milestone with rows in
`data/costs/costs.csv`; V3 and the slicer doc carry the new legs with tables; the N100
appendix names its outcome.

---

## New acceptance criteria

### A. The repo's own home (M17 — evidence: docs/verification/V24.md)
- [x] Annotated tag `v0.4.0` at the v0.4 close `295e80e`, honesty body in the v0.2.0
      tag's format. (Done at drafting: the tag names what v0.4 measured and what it
      did not verify.)
- [ ] The build stops depending on an invisible sibling: fenics-rs becomes part of the
      clone (working assumption: git submodule at its own rev, workspace paths repointed
      in-repo; decided at the plan). Done when a fresh clone alone builds and
      `fieldsmith api` answers the pinned golden question byte-stably.
- [ ] A remote exists: private GitHub repos under the active gh account
      (GlassOnTin — unilateral call; Haven sits under the GlassHaven name, transfer or
      flip to public remains the owner's). A credentials scan of the tree (tokens, keys,
      `.env`-type files, absolute paths that should not travel) runs before the first
      push and its findings are stated, whatever they are.
- [ ] `main` and the tags are pushed; a second fresh clone verifies; V24 passes.

### B. The architecture datapoint (M18 — evidence: docs/verification/V25.md)
- [ ] `fieldsmith api` runs on a machine the user owns that is not x86_64: the OnePlus
      13 (SM8750, aarch64) — hosted through Haven's Linux guest (proot) or termux, the
      binary cross-built on the referee or built on-device; the route is chosen at plan
      with the working assumption first (proot guest, statically linked aarch64 build
      shipped to the phone).
- [ ] The reference-tolerance ladder and the V23 five-point sweep are measured there:
      per-leg walls, points/hour, tracked fields. The phone throttles and the guest has
      no GPU — walls are recorded with their band, and the browser rows stay separate
      from the api rows as on the N100.
- [ ] Tracked fields are compared with the referee's literals field by field; the case
      states, per field, equal-or-not with both numbers. The README's divergence-scope
      line updates from "these hosts" when the datapoint lands.
- [ ] Rows enter `data/costs/costs.csv` with provenance, hand-seeded as the other rows;
      V18's embedded constants grow with the file in the same commit (the M16 pitfall:
      the tie-in test's row count and constants must be extended, never weakened).
- [ ] V25 is measured and recorded — a milestone case like the N100 legs, not a CI
      case; its portable assertions (schema, row shape) assert in CI.

### C. The quarter junction (M19 — evidence: docs/verification/V3.md, extended)
- [ ] The mechanism is diagnosed from the mesher's own output before any fix: re-run
      the quarter model, observe where the cut face meets the hole's tangent, name the
      defect (element quality, node duplication, boundary-condition placement) with
      numbers from the mesh, not a theory paragraph.
- [ ] A fix is attempted and tested: the quarter model passes the 5-row mesh study at
      the half-plate's recorded tolerances, tables in V3.md measured per row.
- [ ] If a fix does not land, this file changes in a reviewed commit recording why and
      what the standing bound is — but the attempted fix and its evidence must exist
      before the box may change. A documented-deferral line can no longer tick or
      replace this box; that is the delta this milestone is buying.

### D. Graded fill reaches the slicer (M20 — evidence: docs/verification/slicer.md, extended)
- [ ] The manifold boundary is located: headless PrusaSlicer 2.9.6 (the v0.3 tooling,
      re-derivable from the flathub AppImage), the graded-fill reference document, a
      grid over skin (0 / 0.6 / 1.2 mm) and cell size; `manifold` verdict per cell; the
      flip point identified from the grid.
- [ ] The cause is found in fieldsmith's export path (what makes mesh-leaf graded fill
      non-manifold to Prusa at skin 1.2) and either fixed there or named geometrically
      from the probe artifacts — with the fix attempt evidenced, as in C.
- [ ] Done when PrusaSlicer reports `manifold=yes` at skin 1.2 for the graded reference
      document, measured — or the reviewed finding closes the box with the boundary
      documented and the attempt's artifacts filed.

### E. The N100 Chrome anomaly, closed (M21 — evidence: an appendix section in docs/verification/budgets.md)
- [ ] Hypotheses enumerated from the M13 record first (the old binary's actual bytes vs
      that session's environment state; SwiftShader state; browser profile/cache;
      scheduler or thermal context that morning; Firefox already held its walls), then
      one factorial experiment: both binaries × the controlled dimensions that the
      hypotheses make discriminating.
- [ ] Both sides reproduce in the run set; the cause is named with numbers, replacing
      the M13 "split ... not closed" ledger line.
- [ ] If nothing separates: a reviewed finding states which hypotheses the experiment
      falsified. "Not closed" is not an acceptable close state for this milestone — it
      may only end named or falsified.

### F. Carried
- [ ] CI runs the whole ladder: V1–V23 unchanged, V24 as it lands, V25's portable
      assertions as they land; `budgets.md` regenerated on the release legs. The two
      owner verdicts still pending from v0.4 (the N100 leg-time row; whether a refusal
      at the cap counts for the tolerance row) stay recorded until the owner rules —
      they are surfaced explicitly at this goal's close.
- [ ] The README carries the v0.4 honesty lines and gains no new claims without
      measured evidence.

Unilateral calls made when drafting (cheap to reverse via a reviewed commit if the
owner disagrees): the private GitHub account is GlassOnTin (the active gh auth), not
the GlassHaven name Haven uses; the repos start private — the VISION §6
"contributed to the repository itself" story becomes literal on the day the owner flips
them public, which is the owner's publishing decision; submodule is the working
assumption for fenics-rs (vendoring and crates.io named for later inside the plan);
the ARM sweep target below is declared from the phone's web-wall evidence before any
api wall exists; C and D's boxes deliberately require an attempted fix with evidence
before a finding may close them.

---

## Verification cases (must pass where they apply)

Cases V1–V23 are the carried ladder and run in CI unchanged (definitions live in
`docs/verification/` and git history). The new cases; each tolerance is a declared
target until first measured, then stated in the case and its report:

| ID | Case | Reference | Tolerance |
| :-- | :-- | :-- | :-- |
| V24 | Fresh-clone contract: `git clone` from the remote → build with nothing else present → the pinned golden api request → byte-stable response; `data/costs/costs.csv` intact in the clone | V22 golden at its pinned apiv/engine | Byte-stable at the pinned version; clone-to-answer wall recorded; passes in CI |
| V25 | The architecture datapoint: reference-tolerance ladder and the 5-point sweep through the api verb on the SM8750 phone, through its Linux guest | The referee's x86_64 rows (V1/V23 tables, V18 constants) | Tracked fields stated equal-or-not per field with both numbers; sweep ≥ 100 points/hour (declared from coarse 1270 ms / refined 7523 ms web walls; revisable in a reviewed commit when first measured); walls recorded with their band |
| V3x | V3 extended: the quarter model joins the 5-row mesh study already passing on the half-plate | The half-plate's 5-row study | The half-plate's recorded tolerances unchanged; full quarter tables in V3.md |
| Sx | slicer.md extended: the manifold grid over skin and cell size, verdict per cell | PrusaSlicer's own `manifold` verdict | `manifold=yes` at skin 1.2 for the graded reference (the deliverable), or the boundary documented with the grid |

V3x and Sx extend existing cases rather than spawning new ones: the docs they extend
already carry the context, and a separate ID would fork it.

---

## Performance budgets

Declared targets below; each row is measured as its milestone lands, and a miss is
answered in a reviewed commit, never a quiet skip.

| Metric | Budget |
| :-- | :-- |
| Fresh clone to a passing api answer | ≤ 5 min on the referee (declared; measured by V24's run) |
| Sweep throughput on the SM8750 (aarch64) | ≥ 100 points/hour (declared, from the phone's web walls; the phone api walls do not exist yet — a first-measure miss triggers the reviewed-commit revision, not a silent band) |
| V3x quarter leg | The half-plate study's recorded tolerances, unchanged |
| Slicer manifold boundary | `manifold=yes` at skin 1.2 for the graded reference, measured; or the boundary cell's parameters documented |
| N100 Chrome split | Both sides of the move reproduce inside the wall bands already recorded; the cause named |

---

## Build order

| # | Milestone | Done when |
| :-- | :-- | :-- |
| M17 | **The repo's own home** — tag (done), fenics-rs in the clone, private remote, credentials scan, push | V24 green; a fresh clone builds and answers; `main` and the tags on the remote |
| M18 | **The architecture datapoint** — the api verb on the phone | V25 measured; rows in `data/costs/costs.csv`; README scope line updated |
| M19 | **The quarter junction** | V3's quarter tables green at half-plate tolerances, or the mechanism named with the mesher's own numbers |
| M20 | **Graded fill reaches the slicer** | `manifold=yes` at skin 1.2 measured, or the boundary documented from the grid |
| M21 | **N100 Chrome, closed** | The factorial run set executes; the cause named or the falsifications stated |

---

## Explicitly out of scope for v0.5

Making the repo public and hosted CI runners (GitHub Actions) — the ladder stays on
`scripts/ci.sh`, and turning a repo public is the owner's publishing act, not mine ·
publishing fenics-rs to crates.io (the submodule carries it; standalone publication is
its own decision) · the termux route unless Haven's guest path fails · new analysis
flows, CAD-parity asks (grouping, sketch, section planes), multi-parameter sweep grids,
topology optimisation (carried from v0.3/v0.4) · any Fieldsmith-hosted service: relay,
accounts, metering, a submit endpoint (the standing rule) · a `v0.5.0` tag on anything
but this goal's close.

---

## Definition of done

v0.5 is done when every box in sections A–F is ticked, V1–V24 pass in CI with V25's
portable assertions green alongside, the budget table is measured, `data/costs/costs.csv`
holds the ARM rows with provenance, the V3 and slicer docs carry their extended tables,
and the three close-out lines carried out of v0.3/v0.4 (V3 deferral, M10's no-route
finding, M13's unclosed split) are each replaced by evidence — and not before. If a
criterion turns out to be wrong, change this file in a reviewed commit; do not quietly
skip it. Each milestone's design detail lands in a reviewed plan before code, as in
previous goals.