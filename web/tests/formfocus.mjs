// Form-control keyboard-focus integrity: the text-entry and native controls that focus.mjs
// explicitly left out of scope must also show a designed accent focus indicator, not the
// browser's near-black `outline: auto` ring (invisible on the dark panel — the same defect the
// buttons had before e645412).
//
// Two focus languages are already in the stylesheet and this gate holds both:
//   * text-entry surfaces (text input, select, the palette search, the two source editors)
//     show an accent EDGE on :focus — a border-colour change, or an inset accent edge for the
//     full-bleed editors that have no border;
//   * native sliders/toggles (range/checkbox/radio) show the accent RING on :focus-visible,
//     matching the button vocabulary.
//
// The probe synthesizes one representative element per selector, reads getComputedStyle at REST
// and again under keyboard-modality FOCUS, and passes only if a designed accent indicator appears
// on focus (solid >=2px accent outline, accent box-shadow, or a border side that became accent)
// AND was absent at rest. The UA default (`outline: auto`, non-accent) fails, so the gate is red
// before the fix and green after. Checked in both light and dark, plus one real live control.
//
// Run with node tests/run-e2e.mjs --only formfocus.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// One representative element per form-control selector. `parent` wraps descendant selectors
// (.palette input); `style` reproduces the inline resets the real editors carry so the probe
// matches reality (the fsm editor's inline border:none/outline:none in particular).
const SPECS = [
  { label: 'input.text-input', tag: 'input', cls: 'text-input', attrs: { type: 'text' } },
  { label: 'select.select', tag: 'select', cls: 'select' },
  { label: '.palette input', tag: 'input', parent: 'palette', attrs: { type: 'text' } },
  { label: '.fsm-source (editor)', tag: 'textarea', cls: 'fsm-source', style: 'border:none;outline:none;background:var(--panel)' },
  { label: '.decl-source (editor)', tag: 'textarea', cls: 'decl-source', style: 'border:1px solid var(--line);background:var(--bg-1)' },
  { label: 'input[type=range]', tag: 'input', attrs: { type: 'range' } },
  { label: 'input[type=checkbox]', tag: 'input', attrs: { type: 'checkbox' } },
  { label: 'input[type=radio]', tag: 'input', attrs: { type: 'radio' } },
];

const SIDES = ['Top', 'Right', 'Bottom', 'Left'];

/** In-page probe: resolve --accent to rgb(), then for each spec read the computed style at rest
 * and under keyboard-modality focus. Returns the accent colour and per-control readings. */
function probeFormFocus({ specs, sides }) {
  const host = document.createElement('div');
  host.style.position = 'absolute';
  host.style.left = '-9999px';
  host.style.top = '0';
  document.body.appendChild(host);

  const accentEl = document.createElement('div');
  accentEl.style.color = 'var(--accent)';
  host.appendChild(accentEl);
  const accent = getComputedStyle(accentEl).color;

  const read = (el) => {
    const cs = getComputedStyle(el);
    const o = { outlineStyle: cs.outlineStyle, outlineWidth: cs.outlineWidth, outlineColor: cs.outlineColor, boxShadow: cs.boxShadow };
    for (const s of sides) o['border' + s + 'Color'] = cs['border' + s + 'Color'];
    return o;
  };

  const readings = specs.map((spec) => {
    let mount = host;
    if (spec.parent) {
      const wrap = document.createElement('div');
      wrap.className = spec.parent;
      host.appendChild(wrap);
      mount = wrap;
    }
    const el = document.createElement(spec.tag);
    if (spec.cls) el.className = spec.cls;
    if (spec.attrs) for (const [k, v] of Object.entries(spec.attrs)) el.setAttribute(k, v);
    if (spec.style) el.setAttribute('style', spec.style);
    if (spec.tag === 'select') {
      const opt = document.createElement('option');
      opt.textContent = 'x';
      el.appendChild(opt);
    }
    mount.appendChild(el);
    const rest = read(el);
    el.focus();
    const focused = read(el);
    const r = { label: spec.label, focusVisible: el.matches(':focus-visible'), isActive: document.activeElement === el, rest, focused };
    el.remove();
    if (spec.parent) mount.remove();
    return r;
  });
  host.remove();
  return { accent, readings };
}

/** Any accent indicator present in a single reading (used to assert the rest state is clean). */
function accentIndicator(r, accent) {
  const w = parseFloat(r.outlineWidth) || 0;
  const outline = r.outlineStyle === 'solid' && w >= 2 && r.outlineColor === accent;
  const shadow = r.boxShadow && r.boxShadow !== 'none' && r.boxShadow.includes(accent);
  const border = SIDES.some((s) => r['border' + s + 'Color'] === accent);
  return outline || shadow || border;
}

/** A designed focus indicator: accent ring or inner edge on focus, or a border side that became
 * accent on focus (the text-entry border-change language). */
function hasDesignedFocus(focused, rest, accent) {
  const w = parseFloat(focused.outlineWidth) || 0;
  const outline = focused.outlineStyle === 'solid' && w >= 2 && focused.outlineColor === accent;
  const shadow = focused.boxShadow && focused.boxShadow !== 'none' && focused.boxShadow.includes(accent);
  const borderChanged = SIDES.some((s) => focused['border' + s + 'Color'] === accent && rest['border' + s + 'Color'] !== accent);
  return outline || shadow || borderChanged;
}

const fmt = (r) =>
  `outline ${r.outlineStyle} ${r.outlineWidth} ${r.outlineColor}; shadow ${r.boxShadow}; ` +
  `borders ${SIDES.map((s) => r['border' + s + 'Color']).join('/')}`;

export async function formfocusTest(browser, url, { docsDir }) {
  const errors = [];
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const table = (names) =>
    [
      '| Check | Result | Measured |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      // Establish keyboard modality so :focus-visible matches the programmatic probe focus.
      await page.keyboard.press('Tab');
      const { accent, readings } = await page.evaluate(probeFormFocus, { specs: SPECS, sides: SIDES });
      for (const r of readings) {
        const pass = r.focusVisible && hasDesignedFocus(r.focused, r.rest, accent) && !accentIndicator(r.rest, accent);
        check(
          `${scheme}: ${r.label} shows a designed accent focus indicator`,
          pass,
          `focused ${fmt(r.focused)}; rest ${fmt(r.rest)}; :focus-visible=${r.focusVisible} (accent ${accent})`,
        );
      }
    }

    // Compose the rule with a real live control (not just a synthetic probe).
    await page.emulateMedia({ colorScheme: 'light' });
    await page.keyboard.press('Tab');
    const live = await page.evaluate((sides) => {
      const el = [...document.querySelectorAll('input[type="range"],input[type="checkbox"],input[type="radio"],input.text-input,select.select')].find(
        (b) => !b.disabled && b.getClientRects().length > 0,
      );
      if (!el) return null;
      const read = (e) => {
        const cs = getComputedStyle(e);
        const o = { outlineStyle: cs.outlineStyle, outlineWidth: cs.outlineWidth, outlineColor: cs.outlineColor, boxShadow: cs.boxShadow };
        for (const s of sides) o['border' + s + 'Color'] = cs['border' + s + 'Color'];
        return o;
      };
      const rest = read(el);
      el.focus();
      const focused = read(el);
      const accentEl = document.createElement('div');
      accentEl.style.color = 'var(--accent)';
      document.body.appendChild(accentEl);
      const accent = getComputedStyle(accentEl).color;
      accentEl.remove();
      const sel = el.tagName.toLowerCase() + (el.type ? `[type=${el.type}]` : '') + (el.className ? `.${el.className}` : '');
      return { sel, focusVisible: el.matches(':focus-visible'), rest, focused, accent };
    }, SIDES);
    check(
      'live form control shows a designed accent focus indicator',
      !!live && live.focusVisible && hasDesignedFocus(live.focused, live.rest, live.accent) && !accentIndicator(live.rest, live.accent),
      live ? `${live.sel}: focused ${fmt(live.focused)}; :focus-visible=${live.focusVisible}` : 'no live form control found in the DOM',
    );

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Form-control keyboard-focus integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The text-entry and native controls outside the button-focused `focus.mjs` gate also show a',
      'designed accent focus indicator, not the browser default `outline: auto` ring (near-black,',
      'invisible on the dark panel). Text-entry surfaces (text input, select, palette search, the',
      'two source editors) show an accent edge on `:focus`; native range/checkbox/radio show the',
      'accent ring on `:focus-visible`. The probe reads each control at rest and under keyboard',
      'focus and passes only if an accent indicator appears on focus and was absent at rest.',
      'Checked in both light and dark, plus one real live control.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The probe tests the stylesheet rule per control selector, not every rendered instance.',
      '- Asserted by computed style (outline / box-shadow / border colour), not a screenshot.',
      '- `.numbox` indicates focus through its parent `:focus-within` border, a different mechanism',
      '  outside this per-control probe.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/formfocus.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['formfocus: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'formfocus-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`formfocus: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}