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