// Native-control accent-color integrity: every native checkbox/radio/range must paint its
// fill/thumb with the app accent (--accent), not the browser's `accent-color: auto` default
// (a UA blue that matches neither theme). The stylesheet already set accent-color on six scoped
// selectors (.check, .opacity-panel, .clip-panel, .sys-param-card, .sys-3d-layers,
// .sys-vp-mini-slider), but native controls appear in ~15 files; everything outside those six
// scopes fell back to `auto`. This gate holds the universal rule that replaces the six ad-hoc
// declarations: one inherited `accent-color: var(--accent)` on :root.
//
// The probe synthesizes one representative element per selector — three UNSCOPED controls (the
// gap) and the six previously-scoped wrappers (a regression guard for the consolidation) — and
// reads getComputedStyle(el).accentColor. It passes only if the computed value equals the
// resolved --accent rgb(); `auto` fails, so the gate is red before the fix and green after.
// Checked in both light and dark, plus the real native controls rendered in the Theory lens
// (one tab click, no body) — a previously-scoped control that guards the consolidation.
//
// Run with node tests/run-e2e.mjs --only nativeaccent.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// One representative element per native-control selector. `parent` wraps the descendant
// selectors that previously carried their own accent-color declaration; the three unscoped
// entries are the gap the universal :root rule must close.
const CONTROLS = [
  { label: 'input[type=checkbox] (unscoped)', type: 'checkbox' },
  { label: 'input[type=radio] (unscoped)', type: 'radio' },
  { label: 'input[type=range] (unscoped)', type: 'range' },
  { label: '.check input', type: 'checkbox', parent: 'check' },
  { label: '.opacity-panel input', type: 'range', parent: 'opacity-panel' },
  { label: '.clip-panel input', type: 'range', parent: 'clip-panel' },
  { label: '.sys-param-card input[type=range]', type: 'range', parent: 'sys-param-card' },
  { label: '.sys-3d-layers input[type=checkbox]', type: 'checkbox', parent: 'sys-3d-layers' },
  { label: '.sys-vp-mini-slider input[type=range]', type: 'range', parent: 'sys-vp-mini-slider' },
];

/** In-page probe: resolve --accent to rgb(), then read each control's computed accent-color.
 * All data arrives as the argument — a function passed to page.evaluate is serialized into the
 * page, so module-level closures are not available there. */
function probeNativeAccent({ controls }) {
  const host = document.createElement('div');
  host.style.position = 'absolute';
  host.style.left = '-9999px';
  host.style.top = '0';
  document.body.appendChild(host);

  const accentEl = document.createElement('div');
  accentEl.style.color = 'var(--accent)';
  host.appendChild(accentEl);
  const accent = getComputedStyle(accentEl).color;

  const readings = controls.map((spec) => {
    let mount = host;
    if (spec.parent) {
      const wrap = document.createElement('div');
      wrap.className = spec.parent;
      host.appendChild(wrap);
      mount = wrap;
    }
    const el = document.createElement('input');
    el.setAttribute('type', spec.type);
    mount.appendChild(el);
    const accentColor = getComputedStyle(el).accentColor;
    el.remove();
    if (spec.parent) mount.remove();
    return { label: spec.label, accentColor };
  });
  host.remove();
  return { accent, readings };
}

export async function nativeaccentTest(browser, url, { docsDir }) {
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
      const { accent, readings } = await page.evaluate(probeNativeAccent, { controls: CONTROLS });
      for (const r of readings) {
        check(
          `${scheme}: ${r.label} paints with the theme accent`,
          r.accentColor === accent,
          `accent-color ${r.accentColor} (accent ${accent})`,
        );
      }
    }

    // Real native controls rendered in the live DOM, not just a synthetic probe. The empty
    // default screen renders none (OpacityPanel needs a body, setup checkboxes a selected
    // support), so open the Theory lens — one tab click, no body — which renders the standby
    // toggle. It was .check-scoped, so this guards that removing the scoped declaration still
    // paints a real control through the inherited :root rule.
    await page.emulateMedia({ colorScheme: 'light' });
    await page.click('button[data-lens="theory"]');
    await page.waitForSelector('[data-testid="theory-toggle-standby"]', { timeout: 15000 });
    const live = await page.evaluate(() => {
      const els = [...document.querySelectorAll('input[type="checkbox"],input[type="radio"],input[type="range"]')].filter(
        (e) => !e.disabled && e.getClientRects().length > 0,
      );
      const accentEl = document.createElement('div');
      accentEl.style.color = 'var(--accent)';
      document.body.appendChild(accentEl);
      const accent = getComputedStyle(accentEl).color;
      accentEl.remove();
      const readings = els.map((e) => ({
        sel: e.tagName.toLowerCase() + `[type=${e.type}]` + (e.className ? `.${e.className}` : ''),
        accentColor: getComputedStyle(e).accentColor,
      }));
      return { accent, count: readings.length, allMatch: readings.every((r) => r.accentColor === accent), readings };
    });
    check(
      'live native controls paint with the theme accent',
      live.count > 0 && live.allMatch,
      `${live.count} rendered; accent ${live.accent}; ` +
        (live.readings.slice(0, 5).map((r) => `${r.sel}=${r.accentColor}`).join(', ') || 'none'),
    );

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Native-control accent-color integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Every native checkbox/radio/range paints its fill/thumb with the app accent (`--accent`),',
      'not the browser `accent-color: auto` default (a UA blue matching neither theme). One',
      'inherited `accent-color: var(--accent)` on `:root` replaces six ad-hoc scoped declarations',
      'and reaches every workbench. The probe reads each control\'s computed accent-color and',
      'passes only if it equals the resolved accent rgb(); `auto` fails. Checked in both light and',
      'dark, plus the native controls rendered in the Theory lens (one tab click, no body).',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The probe tests the stylesheet rule per control selector, not every rendered instance.',
      '- Asserted by computed `accent-color`, not a screenshot of the painted fill/thumb.',
      '- The live check covers the Theory-lens controls (a previously-scoped toggle), not an unscoped one.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/nativeaccent.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['nativeaccent: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'nativeaccent-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`nativeaccent: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}