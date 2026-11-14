# Verification-badge honesty (web UI)

**Result: PASS**

A sheet whose verification report is not shipped must not advertise a green
"Verified · PASS" chip. The literate header marks those sheets
`# @verification_status: pending`; the Tutorials drawer renders them with an amber
`--warn` "verification pending" chip and no green check, while a genuinely verified
sheet keeps its green `--ok` ✓ chip. This gates the rendered surface in both view modes
(contents list + detailed cards) under both light and dark, reading chip colour from
`getComputedStyle` against live `--warn` / `--ok` probes, and drives the sheet-detail
analytic-reference num span and the markdown "Linked verification artifacts" column for
the same sheets so the pending state uses one amber convention everywhere (5S
standardise). The data-layer honesty (badge text and refs resolving on disk) is gated
separately by `web/tests/registry.mjs`.

## Checks

| Check | Result | Measured |
| :-- | :-- | :-- |
| e2e completed | pass |  |
| light/contents: A15 card present | pass |  |
| light/contents: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: A15 shows NO green ✓ chip | pass | [] |
| light/contents: A15 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/contents: A16 card present | pass |  |
| light/contents: A16 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: A16 shows NO green ✓ chip | pass | [] |
| light/contents: A16 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: A16 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/contents: A18 card present | pass |  |
| light/contents: A18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: A18 shows NO green ✓ chip | pass | [] |
| light/contents: A18 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: A18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/contents: A25 card present | pass |  |
| light/contents: A25 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/contents: A25 shows NO green ✓ chip | pass | [] |
| light/contents: A25 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/contents: A25 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
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
| light/cards: A16 card present | pass |  |
| light/cards: A16 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/cards: A16 shows NO green ✓ chip | pass | [] |
| light/cards: A16 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/cards: A16 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/cards: A18 card present | pass |  |
| light/cards: A18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/cards: A18 shows NO green ✓ chip | pass | [] |
| light/cards: A18 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/cards: A18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/cards: A25 card present | pass |  |
| light/cards: A25 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(183, 121, 31)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| light/cards: A25 shows NO green ✓ chip | pass | [] |
| light/cards: A25 pending chip is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/cards: A25 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
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
| light/numspan: A15 analytic num span present | pass |  |
| light/numspan: A15 num span reads pending | pass | verification pending |
| light/numspan: A15 num span is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/numspan: A15 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/numspan: A16 analytic num span present | pass |  |
| light/numspan: A16 num span reads pending | pass | verification pending |
| light/numspan: A16 num span is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/numspan: A16 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/numspan: A18 analytic num span present | pass |  |
| light/numspan: A18 num span reads pending | pass | verification pending |
| light/numspan: A18 num span is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/numspan: A18 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/numspan: A25 analytic num span present | pass |  |
| light/numspan: A25 num span reads pending | pass | verification pending |
| light/numspan: A25 num span is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/numspan: A25 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/numspan: T18 analytic num span present | pass |  |
| light/numspan: T18 num span reads pending | pass | verification pending |
| light/numspan: T18 num span is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/numspan: T18 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/numspan: A1 analytic num span present | pass |  |
| light/numspan: A1 num span is NOT pending | pass | V1 · V2 |
| light/numspan: A1 num span shows refs | pass | V1 · V2 |
| light/numspan: A1 num span is neutral (--text-2) | pass | rgb(93, 93, 88) (want rgb(93, 93, 88)) |
| light/markdown: A15 markdown column present | pass |  |
| light/markdown: A15 artifacts line listed | pass |  |
| light/markdown: A15 shows a pending status pill | pass |  |
| light/markdown: A15 status pill reads pending | pass | verification pending |
| light/markdown: A15 status pill is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/markdown: A15 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/markdown: A16 markdown column present | pass |  |
| light/markdown: A16 artifacts line listed | pass |  |
| light/markdown: A16 shows a pending status pill | pass |  |
| light/markdown: A16 status pill reads pending | pass | verification pending |
| light/markdown: A16 status pill is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/markdown: A16 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/markdown: A18 markdown column present | pass |  |
| light/markdown: A18 artifacts line listed | pass |  |
| light/markdown: A18 shows a pending status pill | pass |  |
| light/markdown: A18 status pill reads pending | pass | verification pending |
| light/markdown: A18 status pill is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/markdown: A18 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/markdown: A25 markdown column present | pass |  |
| light/markdown: A25 artifacts line listed | pass |  |
| light/markdown: A25 shows a pending status pill | pass |  |
| light/markdown: A25 status pill reads pending | pass | verification pending |
| light/markdown: A25 status pill is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/markdown: A25 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/markdown: T18 markdown column present | pass |  |
| light/markdown: T18 artifacts line listed | pass |  |
| light/markdown: T18 shows a pending status pill | pass |  |
| light/markdown: T18 status pill reads pending | pass | verification pending |
| light/markdown: T18 status pill is amber (--warn) | pass | rgb(183, 121, 31) (want rgb(183, 121, 31)) |
| light/markdown: T18 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| light/markdown: A1 markdown column present | pass |  |
| light/markdown: A1 artifacts line listed | pass |  |
| light/markdown: A1 shows NO pending status pill | pass |  |
| dark/contents: A15 card present | pass |  |
| dark/contents: A15 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: A15 shows NO green ✓ chip | pass | [] |
| dark/contents: A15 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: A15 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/contents: A16 card present | pass |  |
| dark/contents: A16 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: A16 shows NO green ✓ chip | pass | [] |
| dark/contents: A16 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: A16 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/contents: A18 card present | pass |  |
| dark/contents: A18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: A18 shows NO green ✓ chip | pass | [] |
| dark/contents: A18 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: A18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/contents: A25 card present | pass |  |
| dark/contents: A25 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/contents: A25 shows NO green ✓ chip | pass | [] |
| dark/contents: A25 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/contents: A25 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
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
| dark/cards: A16 card present | pass |  |
| dark/cards: A16 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/cards: A16 shows NO green ✓ chip | pass | [] |
| dark/cards: A16 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/cards: A16 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/cards: A18 card present | pass |  |
| dark/cards: A18 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/cards: A18 shows NO green ✓ chip | pass | [] |
| dark/cards: A18 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/cards: A18 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/cards: A25 card present | pass |  |
| dark/cards: A25 shows a pending chip | pass | [{"text":"verification pending","color":"rgb(217, 164, 65)","title":"Verification pending — no report shipped yet; benchmark source linked"}] |
| dark/cards: A25 shows NO green ✓ chip | pass | [] |
| dark/cards: A25 pending chip is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/cards: A25 pending chip title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
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
| dark/numspan: A15 analytic num span present | pass |  |
| dark/numspan: A15 num span reads pending | pass | verification pending |
| dark/numspan: A15 num span is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/numspan: A15 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/numspan: A16 analytic num span present | pass |  |
| dark/numspan: A16 num span reads pending | pass | verification pending |
| dark/numspan: A16 num span is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/numspan: A16 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/numspan: A18 analytic num span present | pass |  |
| dark/numspan: A18 num span reads pending | pass | verification pending |
| dark/numspan: A18 num span is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/numspan: A18 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/numspan: A25 analytic num span present | pass |  |
| dark/numspan: A25 num span reads pending | pass | verification pending |
| dark/numspan: A25 num span is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/numspan: A25 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/numspan: T18 analytic num span present | pass |  |
| dark/numspan: T18 num span reads pending | pass | verification pending |
| dark/numspan: T18 num span is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/numspan: T18 num span title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/numspan: A1 analytic num span present | pass |  |
| dark/numspan: A1 num span is NOT pending | pass | V1 · V2 |
| dark/numspan: A1 num span shows refs | pass | V1 · V2 |
| dark/numspan: A1 num span is neutral (--text-2) | pass | rgb(165, 166, 162) (want rgb(165, 166, 162)) |
| dark/markdown: A15 markdown column present | pass |  |
| dark/markdown: A15 artifacts line listed | pass |  |
| dark/markdown: A15 shows a pending status pill | pass |  |
| dark/markdown: A15 status pill reads pending | pass | verification pending |
| dark/markdown: A15 status pill is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/markdown: A15 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/markdown: A16 markdown column present | pass |  |
| dark/markdown: A16 artifacts line listed | pass |  |
| dark/markdown: A16 shows a pending status pill | pass |  |
| dark/markdown: A16 status pill reads pending | pass | verification pending |
| dark/markdown: A16 status pill is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/markdown: A16 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/markdown: A18 markdown column present | pass |  |
| dark/markdown: A18 artifacts line listed | pass |  |
| dark/markdown: A18 shows a pending status pill | pass |  |
| dark/markdown: A18 status pill reads pending | pass | verification pending |
| dark/markdown: A18 status pill is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/markdown: A18 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/markdown: A25 markdown column present | pass |  |
| dark/markdown: A25 artifacts line listed | pass |  |
| dark/markdown: A25 shows a pending status pill | pass |  |
| dark/markdown: A25 status pill reads pending | pass | verification pending |
| dark/markdown: A25 status pill is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/markdown: A25 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/markdown: T18 markdown column present | pass |  |
| dark/markdown: T18 artifacts line listed | pass |  |
| dark/markdown: T18 shows a pending status pill | pass |  |
| dark/markdown: T18 status pill reads pending | pass | verification pending |
| dark/markdown: T18 status pill is amber (--warn) | pass | rgb(217, 164, 65) (want rgb(217, 164, 65)) |
| dark/markdown: T18 status pill title flags pending | pass | Verification pending — no report shipped yet; benchmark source linked |
| dark/markdown: A1 markdown column present | pass |  |
| dark/markdown: A1 artifacts line listed | pass |  |
| dark/markdown: A1 shows NO pending status pill | pass |  |
| no console errors | pass | 0 |

## Limits

- Covers all five pending sheets (A15, A16, A18, A25, T18) and A1 (verified) on the
  contents-list chips, detailed-card chips, the analytic-reference num span, and the
  markdown "Linked verification artifacts" column.
- Topical correctness of refs (does the cited V# actually cover the sheet) is out of
  scope; the registry gate checks that refs resolve on disk, not that they are on-topic.

Console errors: **0**

---
*Generated by `npm run test:e2e` (web/tests/badge.mjs) — do not edit by hand.*
