# Verification-badge honesty (web UI)

**Result: PASS**

A sheet whose verification report is not shipped must not advertise a green
"Verified · PASS" chip. The literate header marks those sheets
`# @verification_status: pending`; the Tutorials drawer renders them with an amber
`--warn` "verification pending" chip and no green check, while a genuinely verified
sheet keeps its green `--ok` ✓ chip. This gates the rendered surface in both view modes
(contents list + detailed cards) under both light and dark, reading chip colour from
`getComputedStyle` against live `--warn` / `--ok` probes. The data-layer honesty (badge
text and refs resolving on disk) is gated separately by `web/tests/registry.mjs`.

## Checks

| Check | Result | Measured |
| :-- | :-- | :-- |
| e2e completed | pass |  |
| light/contents: A15 card present | pass |  |
| light/contents: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: A15 shows NO green ✓ chip | pass | [] |
| light/contents: A15 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/contents: T18 card present | pass |  |
| light/contents: T18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: T18 shows NO green ✓ chip | pass | [] |
| light/contents: T18 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: T18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/contents: A1 card present | pass |  |
| light/contents: A1 shows NO pending chip | pass | [] |
| light/contents: A1 shows 1 green ✓ chip(s) | pass | [{"text":"✓ V1+1","color":"rgb(39, 103, 73)","title":"Verified: docs/verification/V1.md, docs/verification/V2.md"}] |
| light/contents: A1 ✓ chip is green (--ok) | pass | rgb(39, 103, 73) (want rgb(39, 103, 73)) |
| light/contents: A1 ✓ chip title claims verification | pass | Verified: docs/verification/V1.md, docs/verification/V2.md |
| light/cards: A15 card present | pass |  |
| light/cards: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/cards: A15 shows NO green ✓ chip | pass | [] |
| light/cards: A15 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/cards: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/cards: T18 card present | pass |  |
| light/cards: T18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/cards: T18 shows NO green ✓ chip | pass | [] |
| light/cards: T18 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/cards: T18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/cards: A1 card present | pass |  |
| light/cards: A1 shows NO pending chip | pass | [] |
| light/cards: A1 shows 2 green ✓ chip(s) | pass | [{"text":"✓ V1","color":"rgb(39, 103, 73)","title":"Automated Verification Report: docs/verification/V1.md"},{"text":"✓ V2","color":"rgb(39, 103, 73)","title":"Automated Verification Report: docs/verification/V2.md"}] |
| light/cards: A1 ✓ chip is green (--ok) | pass | rgb(39, 103, 73) (want rgb(39, 103, 73)) |
| light/cards: A1 ✓ chip title claims verification | pass | Automated Verification Report: docs/verification/V1.md |
| light/cards: A1 ✓ chip is green (--ok) | pass | rgb(39, 103, 73) (want rgb(39, 103, 73)) |
| light/cards: A1 ✓ chip title claims verification | pass | Automated Verification Report: docs/verification/V1.md |
| dark/contents: A15 card present | pass |  |
| dark/contents: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: A15 shows NO green ✓ chip | pass | [] |
| dark/contents: A15 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/contents: T18 card present | pass |  |
| dark/contents: T18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: T18 shows NO green ✓ chip | pass | [] |
| dark/contents: T18 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: T18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/contents: A1 card present | pass |  |
| dark/contents: A1 shows NO pending chip | pass | [] |
| dark/contents: A1 shows 1 green ✓ chip(s) | pass | [{"text":"✓ V1+1","color":"rgb(92, 185, 138)","title":"Verified: docs/verification/V1.md, docs/verification/V2.md"}] |
| dark/contents: A1 ✓ chip is green (--ok) | pass | rgb(92, 185, 138) (want rgb(92, 185, 138)) |
| dark/contents: A1 ✓ chip title claims verification | pass | Verified: docs/verification/V1.md, docs/verification/V2.md |
| dark/cards: A15 card present | pass |  |
| dark/cards: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/cards: A15 shows NO green ✓ chip | pass | [] |
| dark/cards: A15 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/cards: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/cards: T18 card present | pass |  |
| dark/cards: T18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/cards: T18 shows NO green ✓ chip | pass | [] |
| dark/cards: T18 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/cards: T18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/cards: A1 card present | pass |  |
| dark/cards: A1 shows NO pending chip | pass | [] |
| dark/cards: A1 shows 2 green ✓ chip(s) | pass | [{"text":"✓ V1","color":"rgb(92, 185, 138)","title":"Automated Verification Report: docs/verification/V1.md"},{"text":"✓ V2","color":"rgb(92, 185, 138)","title":"Automated Verification Report: docs/verification/V2.md"}] |
| dark/cards: A1 ✓ chip is green (--ok) | pass | rgb(92, 185, 138) (want rgb(92, 185, 138)) |
| dark/cards: A1 ✓ chip title claims verification | pass | Automated Verification Report: docs/verification/V1.md |
| dark/cards: A1 ✓ chip is green (--ok) | pass | rgb(92, 185, 138) (want rgb(92, 185, 138)) |
| dark/cards: A1 ✓ chip title claims verification | pass | Automated Verification Report: docs/verification/V1.md |
| no console errors | pass | 0 |

## Limits

- Samples A15 + T18 (pending) and A1 (verified) only; the other three pending sheets
  (A16, A18, A25) share the same render path but are not individually asserted here.
- Exercises the contents-list and detailed-card chips. The analytic-reference num span
  and the markdown "Linked verification artifacts" column are not driven by this suite;
  their underlying status/refs are covered by the registry gate.

Console errors: **0**

---
*Generated by `npm run test:e2e` (web/tests/badge.mjs) — do not edit by hand.*
