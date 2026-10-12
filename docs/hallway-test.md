# Hallway test (GOAL v0.1)

The M5 gate: 3 people unfamiliar with the project complete the bracket test (steps 1–9 of
the reference task in `GOAL.md`) in ≤ 5 minutes each, without help. When 3 of 3 pass, tick
the bracket-test line in `GOAL.md` in a reviewed commit and tag `v0.1.0`.

## Setup

- App URL: the production bundle served with the COOP/COEP headers (threaded wasm engaged):
  - Workstation: `http://localhost:4173/`
  - From another machine on the LAN: `http://192.168.0.180:4173/`
  - Any browser that supports shared array buffers behind those headers works (Chrome, Edge,
    Firefox 113+, Safari 16.4+).
- One user at a time. Start each session from an empty document: a fresh incognito tab (or a
  fresh browser profile) is the reliable way to guarantee it.
- Do not show this file, or any documentation, before or during the session.

## Read the user this task card

> This is Fieldsmith, a design tool. It opens with an empty document. You have 5 minutes.
>
> Model this bracket: an L-bracket of two 60 × 40 × 8 mm plates at right angles, an 8 mm
> fillet on the inside corner, two ⌀ 6.5 mm holes in the base plate, one ⌀ 8.5 mm hole in
> the upright. Work in millimetres.
>
> Fix the inner faces of the two base holes. Load the upright hole with 400 N, pointing
> down and 15° outward. The material is PETG.
>
> Check the stress. Then fill the inside with a gyroid lattice: 1.2 mm solid skin, 6 mm
> cell, solid rings of 2 mm around every hole, denser where the stress is higher.
>
> Confirm the filled part is still strong enough, and export a 3MF you could send to a
> 3D printer. Tell me when you're done, or when your 5 minutes are up.

Optionally show the finished result (e.g. `web/test-results/bracket-13.png` from the latest
e2e run) so they know the shape goal. No screenshots of the UI.

## During the run

- Start the clock at page load with an empty document; stop when the 3MF file is on disk.
- Any question about how the app works, or any gesture you perform for them, is help: the
  run counts as a help-run. Answering a question the card already answers (dimension
  confirmations) is not help.
- Note console errors only in passing; the session is for the human, not the console.

## After the run

- Open the exported 3MF in a slicer (PrusaSlicer / OrcaSlicer) and check for repair
  warnings: this closes the GOAL slicer line for that user, after they leave.

## Results

| # | Date, browser, machine | Time | Steps 1–9 complete | Help | Slicer clean | Notes |
| :-- | :-- | --: | :--: | :--: | :--: | :-- |
| 1 | | | | | | |
| 2 | | | | | | |
| 3 | | | | | | |

Failing runs are product notes for the next milestone, not gate verdicts; record what they
stumbled on.

---

# Hallway test (GOAL v0.3)

The v0.3 gate: 3 people unfamiliar with the project complete the CAD round-trip (the
reference task in `GOAL.md`) in ≤ 5 minutes each, without help. When 3 of 3 pass, record
the runs here; the GOAL hallway line is ticked in a reviewed commit when the leg is done.

## Why the fixture is a 3MF

The web palette imports `.stl / .obj / .glb / .3mf` (`web/src/app/commands.ts`); STEP
import is CLI-only today (GOAL's STEP box names `CLI import`), so a first-time user cannot
drop a `.stp` on the palette. GOAL's one-sentence goal accepts "a 3MF another tool wrote",
and that is the rung the card uses; the foreign STEP leg stays machine-tested (V14).

## Fixture and provenance

`docs/hallway/bracket-fixture.3mf` — the bracket of the v0.1 card, re-authored by a
second tool so the participant receives rather than models it. Source beside it:
`docs/hallway/bracket-fixture.scad` (OpenSCAD, `$fn=64`, datum at the base corner;
L-profile polygon with the fillet arc, three hole cylinders subtracted). OpenSCAD's writer
marks its zip as zip64 even at this size, which our reader refuses by name; the shipped
file is OpenSCAD's export re-zipped with Python's `zipfile` (content unchanged). Measured
on import: one leaf, 864 triangles, 10 patches, 0 open edges, bbox (0,0,0)–(60,40,40),
volume 34 406 mm³.

## Setup

- Serve the production bundle on :4173 with the COOP/COEP headers (as v0.1: threaded wasm
  engaged); `npm run preview` in `web/`. Confirm the machine's LAN address with `ip addr`
  before printing the URL.
- Copy `bracket-fixture.3mf` to the Desktop of the machine the participant uses. One user
  at a time; each starts from an empty document in a fresh incognito tab.
- Do not show this file, or any documentation, before or during the session.

## Read the user this task card

> This is Fieldsmith, a design tool. It opens with an empty document. You have 5 minutes.
>
> A part arrived from another CAD tool as the file `Bracket` on your desktop. Bring it in:
> an L-bracket with a 60 × 40 × 8 base plate, an upright joined by an 8 mm fillet, two
> ⌀ 6.5 mm holes in the base plate, one ⌀ 8.5 mm hole through the upright. Work in
> millimetres.
>
> Fix the inner faces of the two base holes. Load the upright hole with 400 N, pointing
> down and 15° outward. The material is PETG.
>
> Check the stress. Then check how the part rings: its first natural frequencies.
>
> Then fill the inside with a gyroid lattice — 1.2 mm solid skin, 6 mm cell, denser where
> the stress is higher — and read the mass in grams, filled against solid.
>
> Confirm the filled part is still strong enough. Export a 3MF you could send to a 3D
> printer, and save the file. Tell me when you're done, or when your 5 minutes are up.

## During the run

- Clock starts at page load with an empty document; stops when their 3MF is on disk.
- Any question about how the app works, or any gesture performed for them, is help: the
  run counts as a help-run. Answering a question the card already answers (dimension
  confirmations) is not help.
- Note console errors only in passing; the session is for the human, not the console.

## After the run (operator)

- Open their exported 3MF in PrusaSlicer or OrcaSlicer and note repair warnings — as a
  product note, not a gate: v0.3's slicer-clean criterion rides the machine legs (V12,
  V14, and the reference run's solid route in `docs/verification/V14.md`). A graded
  lattice fill with prescribed skin on an *imported* (mesh-leaf) part carries a known
  warning today; see V14's Limits before deciding what it means.
- If they saved the file: reload it and re-solve like-for-like with the CLI as a courtesy
  check that their save is reproducible (machine-covered by the reference run; this is
  about *their* file).
- Record the row below.

## Results

| # | Date, browser, machine | Time | Steps complete | Help | Slicer notes | Notes |
| :-- | :-- | --: | :--: | :--: | :-- | :-- |
| 1 | | | | | | |
| 2 | | | | | | |
| 3 | | | | | | |

Failing runs are product notes for the next milestone, not gate verdicts; record what they
stumbled on.