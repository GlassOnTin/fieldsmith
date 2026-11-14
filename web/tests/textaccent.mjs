// Text-accent integrity: text selection and input placeholders must speak the theme palette,
// not the browser defaults. The stylesheet had NO ::selection or ::placeholder rule anywhere,
// so (observed on HeadlessChrome/142):
//   * ::selection background computed `rgba(0, 0, 0, 0)` (unset -> the UA painted highlight,
//     not the theme accent) and its text colour just inherited the element's own text;
//   * ::placeholder was a fixed UA grey `rgb(117, 117, 117)` in BOTH themes, ignoring the
//     palette and contrasting ~3.65:1 on the dark panel (below WCAG AA 4.5).
// The fix: `::selection { background: var(--accent); color: var(--panel); }` (panel is near-white
// in light / near-black in dark, the right contrast inversion against the accent in each theme)
// and `::placeholder { color: var(--text-2); opacity: 1; }` (AA-legible secondary text). Both
// pseudo-element rules have an implied universal subject, so one declaration reaches every element.
//
// The probe synthesizes a real text div and a real placeholder input in the live document, reads
// getComputedStyle(el, '::selection' / '::placeholder'), and passes only if the colours equal the
// resolved theme tokens AND the text/background contrast meets AA (>= 4.5). `auto`/UA-grey fails,
// so the gate is red before the fix and green after. Checked in both light and dark, plus the
// selection of the text elements rendered in the live app.
//
// Run with node tests/run-e2e.mjs --only textaccent.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** In-page probe: resolve the theme tokens to rgb(), read the selection/placeholder pseudo-element
 * colours on a real div and input, and compute WCAG contrast ratios. Self-contained — a function
 * passed to page.evaluate is serialized into the page, so module-level closures are unavailable. */
function probeTextAccent() {
  const tok = (name) => {
    const d = document.createElement('div');
    d.style.color = `var(${name})`;
    document.body.appendChild(d);
    const c = getComputedStyle(d).color;
    d.remove();
    return c;
  };
  const rgb = (s) => s.match(/[\d.]+/g).slice(0, 3).map(Number);
  const lum = (s) => {
    const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    const [r, g, b] = rgb(s);
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const contrast = (a, b) => {
    const l1 = lum(a), l2 = lum(b);
    return Math.round(((Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)) * 100) / 100;
  };

  const accent = tok('--accent'), panel = tok('--panel'), text2 = tok('--text-2');
  const host = document.createElement('div');
  host.style.position = 'absolute'; host.style.left = '-9999px'; host.style.top = '0';
  document.body.appendChild(host);
  const div = document.createElement('div');
  div.textContent = 'selectable text';
  host.appendChild(div);
  const input = document.createElement('input');
  input.type = 'text';
  input.placeholder = 'hint text';
  host.appendChild(input);

  const selDiv = getComputedStyle(div, '::selection');
  const selInput = getComputedStyle(input, '::selection');
  const ph = getComputedStyle(input, '::placeholder');
  const out = {
    accent, panel, text2,
    selDivBg: selDiv.backgroundColor, selDivColor: selDiv.color,
    selInputBg: selInput.backgroundColor, selInputColor: selInput.color,
    phColor: ph.color, phOpacity: ph.opacity,
    selContrast: contrast(selDiv.backgroundColor, selDiv.color),
    phContrast: contrast(ph.color, panel),
  };
  host.remove();
  return out;
}

export async function textaccentTest(browser, url, { docsDir }) {
  const errors = [];
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const table = (names) =>
    [
      '| Check | Result | Measured |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''}`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      const r = await page.evaluate(probeTextAccent);
      check(`${scheme}: text selection paints the accent`,
        r.selDivBg === r.accent && r.selDivColor === r.panel,
        `bg ${r.selDivBg}; color ${r.selDivColor} (accent ${r.accent}, panel ${r.panel})`);
      check(`${scheme}: input selection paints the accent`,
        r.selInputBg === r.accent && r.selInputColor === r.panel,
        `bg ${r.selInputBg}; color ${r.selInputColor}`);
      check(`${scheme}: selection text meets AA contrast`,
        r.selContrast >= 4.5, `${r.selContrast}:1 (>= 4.5)`);
      check(`${scheme}: placeholder uses secondary text`,
        r.phColor === r.text2, `color ${r.phColor}; opacity ${r.phOpacity} (text-2 ${r.text2})`);
      check(`${scheme}: placeholder meets AA contrast`,
        r.phContrast >= 4.5, `${r.phContrast}:1 on panel (>= 4.5)`);
    }

    // Real text rendered in the live app: every visible leaf text element's selection must paint
    // the accent (non-vacuous — the default screen renders ~122 such elements). Placeholder inputs
    // are covered opportunistically (the default screen renders 0; the synthetic probe covers them).
    await page.emulateMedia({ colorScheme: 'light' });
    const live = await page.evaluate(() => {
      const tok = (name) => {
        const d = document.createElement('div');
        d.style.color = `var(${name})`;
        document.body.appendChild(d);
        const c = getComputedStyle(d).color;
        d.remove();
        return c;
      };
      const accent = tok('--accent'), text2 = tok('--text-2');
      const visible = (e) => e.getClientRects().length > 0;
      const textEls = [...document.querySelectorAll('*')].filter(
        (e) => visible(e) && e.children.length === 0 && e.textContent.trim());
      const selThemed = textEls.every((e) => getComputedStyle(e, '::selection').backgroundColor === accent);
      const inputs = [...document.querySelectorAll('input,textarea')].filter((e) => visible(e) && e.placeholder);
      const phThemed = inputs.every((e) => getComputedStyle(e, '::placeholder').color === text2);
      return { accent, textCount: textEls.length, selThemed, inputCount: inputs.length, phThemed };
    });
    check('live rendered text uses the accent selection',
      live.textCount > 0 && live.selThemed && live.phThemed,
      `${live.textCount} text els, selection themed ${live.selThemed}; ${live.inputCount} placeholder inputs themed ${live.phThemed} (accent ${live.accent})`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Text-accent integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Text selection and input placeholders speak the theme palette, not the browser defaults.',
      '`::selection { background: var(--accent); color: var(--panel); }` paints the accent with a',
      'contrast-inverted text colour (panel is near-white in light, near-black in dark);',
      '`::placeholder { color: var(--text-2); opacity: 1; }` uses AA-legible secondary text. Before',
      'the fix the selection background was unset (UA highlight) and the placeholder was a fixed UA',
      'grey rgb(117, 117, 117) in both themes (~3.65:1 on the dark panel, below AA). The probe reads',
      'the computed pseudo-element colours on a real div and input and passes only if they equal the',
      'resolved tokens and meet WCAG AA contrast (>= 4.5). Checked in light and dark, plus the',
      'selection of the text elements rendered live.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Asserted by computed pseudo-element colour and a computed contrast ratio, not a painted screenshot.',
      '- The live check anchors on rendered text selection (~122 elements); the default screen renders 0 placeholder inputs, so placeholders are covered by the synthetic real-input probe.',
      '- Contrast is WCAG relative-luminance math on the resolved rgb(), evaluated in Chromium 142; Firefox/Safari placeholder opacity defaults differ.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/textaccent.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['textaccent: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'textaccent-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`textaccent: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}