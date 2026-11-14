// Scrollbar palette integrity: every scroll region must paint its scrollbar from the app
// palette (--line-strong thumb on a transparent track, thin), not the browser's
// `scrollbar-color: auto` / `scrollbar-width: auto` default (a UA grey that speaks neither the
// light nor the dark panel/line vocabulary). The stylesheet had NO scrollbar rule anywhere, so
// all 14 overflow regions fell back to `auto`. This gate holds the universal rule that closes
// the gap: `scrollbar-color: var(--line-strong) transparent` on :root (it inherits) plus
// `scrollbar-width: thin` on * — Chromium does NOT inherit scrollbar-width (observed on 142:
// :root computes thin while body and children compute auto). Together they reach every descendant.
//
// The probe synthesizes one scrollable element per representative container selector — a bare
// unscoped div (the universal case) plus the real scroll-region classes — and reads
// getComputedStyle(el).scrollbarColor / .scrollbarWidth. It passes only if the thumb equals the
// resolved --line-strong rgb() and the width is `thin`; `auto` fails, so the gate is red before
// the fix and green after. Checked in both light and dark, plus the scrollable containers
// rendered in the live DOM.
//
// Run with node tests/run-e2e.mjs --only scrollbar.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// One representative scrollable element per real overflow region. `parent` wraps the descendant
// selector (.palette ul); `tag` defaults to div. The bare unscoped entry is the universal case.
const SCROLLERS = [
  { label: 'scrollable div (unscoped)' },
  { label: '.panel-body', cls: 'panel-body' },
  { label: '.palette ul', tag: 'ul', parent: 'palette' },
  { label: '.results-panel', cls: 'results-panel' },
  { label: '.sys-col', cls: 'sys-col' },
  { label: '.tut-col-body', cls: 'tut-col-body' },
];

/** In-page probe: resolve --line-strong to rgb(), then read each scroll region's computed
 * scrollbar-color and scrollbar-width. All data arrives as the argument — a function passed to
 * page.evaluate is serialized into the page, so module-level closures are not available there. */
function probeScrollbar({ scrollers }) {
  const host = document.createElement('div');
  host.style.position = 'absolute';
  host.style.left = '-9999px';
  host.style.top = '0';
  document.body.appendChild(host);

  const thumbEl = document.createElement('div');
  thumbEl.style.color = 'var(--line-strong)';
  host.appendChild(thumbEl);
  const thumb = getComputedStyle(thumbEl).color;

  const readings = scrollers.map((spec) => {
    let mount = host;
    if (spec.parent) {
      const wrap = document.createElement('div');
      wrap.className = spec.parent;
      host.appendChild(wrap);
      mount = wrap;
    }
    const el = document.createElement(spec.tag || 'div');
    if (spec.cls) el.className = spec.cls;
    el.style.overflow = 'auto';
    el.style.height = '80px';
    const tall = document.createElement('div');
    tall.style.height = '400px';
    el.appendChild(tall);
    mount.appendChild(el);
    const cs = getComputedStyle(el);
    const scrollbarColor = cs.scrollbarColor;
    const scrollbarWidth = cs.scrollbarWidth;
    el.remove();
    if (spec.parent) mount.remove();
    return { label: spec.label, scrollbarColor, scrollbarWidth };
  });
  host.remove();
  return { thumb, readings };
}

export async function scrollbarTest(browser, url, { docsDir }) {
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
      const { thumb, readings } = await page.evaluate(probeScrollbar, { scrollers: SCROLLERS });
      for (const r of readings) {
        const themed = r.scrollbarColor !== 'auto' && r.scrollbarColor.startsWith(thumb);
        const thin = r.scrollbarWidth === 'thin';
        check(
          `${scheme}: ${r.label} scrolls with the theme palette`,
          themed && thin,
          `scrollbar-color ${r.scrollbarColor}; width ${r.scrollbarWidth} (thumb ${thumb})`,
        );
      }
    }

    // Real scrollable containers rendered in the live DOM, not just a synthetic probe. Read
    // every visible element whose computed overflow is auto/scroll and confirm its scrollbar
    // speaks the palette. The default screen renders the .panel-body scroll region.
    await page.emulateMedia({ colorScheme: 'light' });
    const live = await page.evaluate(() => {
      const thumbEl = document.createElement('div');
      thumbEl.style.color = 'var(--line-strong)';
      document.body.appendChild(thumbEl);
      const thumb = getComputedStyle(thumbEl).color;
      thumbEl.remove();
      const els = [...document.querySelectorAll('*')].filter((e) => {
        if (e.getClientRects().length === 0) return false;
        const cs = getComputedStyle(e);
        return /(auto|scroll)/.test(cs.overflowY) || /(auto|scroll)/.test(cs.overflowX);
      });
      const readings = els.map((e) => ({
        sel: e.tagName.toLowerCase() + (e.className && typeof e.className === 'string' ? `.${e.className.trim().split(/\s+/).join('.')}` : ''),
        scrollbarColor: getComputedStyle(e).scrollbarColor,
      }));
      return {
        thumb,
        count: readings.length,
        allThemed: readings.every((r) => r.scrollbarColor !== 'auto' && r.scrollbarColor.startsWith(thumb)),
        readings,
      };
    });
    check(
      'live scroll regions use the theme palette',
      live.count > 0 && live.allThemed,
      `${live.count} rendered; thumb ${live.thumb}; ` +
        (live.readings.slice(0, 5).map((r) => `${r.sel}=${r.scrollbarColor}`).join(', ') || 'none'),
    );

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Scrollbar palette integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Every scroll region paints its scrollbar from the app palette (`--line-strong` thumb on a',
      'transparent track, thin), not the browser `scrollbar-color: auto` / `scrollbar-width: auto`',
      'default (a UA grey matching neither theme). `scrollbar-color` inherits from one `:root` rule;',
      '`scrollbar-width: thin` is applied to every element via `*` because Chromium does not inherit',
      'it (observed on 142). var(--line-strong) resolves lazily against the dark block. The probe reads',
      'each region\'s computed scrollbar-color and -width and passes only if the thumb equals the',
      'resolved --line-strong rgb() and the width is `thin`; `auto` fails. Checked in both light and',
      'dark, plus the scrollable containers rendered live.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The probe tests the stylesheet rule per container selector, not every rendered instance.',
      '- Asserted by computed `scrollbar-color`/`scrollbar-width`, not a screenshot of the painted scrollbar.',
      '- The live check reads the default-screen scroll regions; workbench-specific regions are covered synthetically.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/scrollbar.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['scrollbar: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'scrollbar-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`scrollbar: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}