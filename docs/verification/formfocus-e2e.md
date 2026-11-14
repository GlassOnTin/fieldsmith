# Form-control keyboard-focus integrity (web UI)

**Result: PASS**

The text-entry and native controls outside the button-focused `focus.mjs` gate also show a
designed accent focus indicator, not the browser default `outline: auto` ring (near-black,
invisible on the dark panel). Text-entry surfaces (text input, select, palette search, the
two source editors) show an accent edge on `:focus`; native range/checkbox/radio show the
accent ring on `:focus-visible`. The probe reads each control at rest and under keyboard
focus and passes only if an accent indicator appears on focus and was absent at rest.
Checked in both light and dark, plus one real live control.

## Checks

| Check | Result | Measured |
| :-- | :-- | :-- |
| e2e completed | pass |  |
| light: input.text-input shows a designed accent focus indicator | pass | focused outline none 0px rgb(29, 29, 27); shadow none; borders rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: select.select shows a designed accent focus indicator | pass | focused outline none 0px rgb(29, 29, 27); shadow none; borders rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: .palette input shows a designed accent focus indicator | pass | focused outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(51, 111, 209)/rgb(29, 29, 27); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(226, 226, 222)/rgb(29, 29, 27); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: .fsm-source (editor) shows a designed accent focus indicator | pass | focused outline none 0px rgb(29, 29, 27); shadow rgb(51, 111, 209) 0px 0px 0px 2px inset; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: .decl-source (editor) shows a designed accent focus indicator | pass | focused outline none 0px rgb(29, 29, 27); shadow rgb(51, 111, 209) 0px 0px 0px 2px inset; borders rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222)/rgb(226, 226, 222); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: input[type=range] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(51, 111, 209); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: input[type=checkbox] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(51, 111, 209); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); :focus-visible=true (accent rgb(51, 111, 209)) |
| light: input[type=radio] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(51, 111, 209); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); rest outline none 0px rgb(29, 29, 27); shadow none; borders rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27)/rgb(29, 29, 27); :focus-visible=true (accent rgb(51, 111, 209)) |
| dark: input.text-input shows a designed accent focus indicator | pass | focused outline none 0px rgb(230, 230, 227); shadow none; borders rgb(107, 155, 234)/rgb(107, 155, 234)/rgb(107, 155, 234)/rgb(107, 155, 234); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: select.select shows a designed accent focus indicator | pass | focused outline none 0px rgb(230, 230, 227); shadow none; borders rgb(107, 155, 234)/rgb(107, 155, 234)/rgb(107, 155, 234)/rgb(107, 155, 234); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: .palette input shows a designed accent focus indicator | pass | focused outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(107, 155, 234)/rgb(230, 230, 227); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(46, 48, 52)/rgb(230, 230, 227); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: .fsm-source (editor) shows a designed accent focus indicator | pass | focused outline none 0px rgb(230, 230, 227); shadow rgb(107, 155, 234) 0px 0px 0px 2px inset; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: .decl-source (editor) shows a designed accent focus indicator | pass | focused outline none 0px rgb(230, 230, 227); shadow rgb(107, 155, 234) 0px 0px 0px 2px inset; borders rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52)/rgb(46, 48, 52); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: input[type=range] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(107, 155, 234); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: input[type=checkbox] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(107, 155, 234); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); :focus-visible=true (accent rgb(107, 155, 234)) |
| dark: input[type=radio] shows a designed accent focus indicator | pass | focused outline solid 2px rgb(107, 155, 234); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); rest outline none 0px rgb(230, 230, 227); shadow none; borders rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227)/rgb(230, 230, 227); :focus-visible=true (accent rgb(107, 155, 234)) |
| live form control shows a designed accent focus indicator | pass | select[type=select-one].select: focused outline none 0px rgb(29, 29, 27); shadow none; borders rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209)/rgb(51, 111, 209); :focus-visible=true |
| no console errors | pass | 0 |

## Limits

- The probe tests the stylesheet rule per control selector, not every rendered instance.
- Asserted by computed style (outline / box-shadow / border colour), not a screenshot.
- `.numbox` indicates focus through its parent `:focus-within` border, a different mechanism
  outside this per-control probe.

Console errors: **0**

---
*Generated by `npm run test:e2e` (web/tests/formfocus.mjs) — do not edit by hand.*
