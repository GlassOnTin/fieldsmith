// Literate .fsm Verification block cohesion: the provenance card in the Results lens
// (results.tsx, data-testid="results-analytic-verification") must speak the theme palette like
// its sibling cards. It was built entirely from hardcoded slate/blue inline colours while the
// panel behind it (.results-panel / .card) is `background: var(--panel)`, so in light mode the
// header (#93c5fd) and the Static Reference Notes label (#94a3b8) sit on a near-white panel far
// below WCAG AA, and the reference rows are fixed dark islands (rgba(15,23,42,…)) that never
// track the theme. This is the "shine / standardise" 5S principle (VISION §3) on the surface that
// carries every number's provenance (VISION §6).
//
// The gate loads the real A1 cantilever literate sheet (its executable starterFsm, taken from the
// in-page TUTORIAL_SHEETS so the doc name matches the sheet), solves it, opens the Results lens,
// and measures the rendered block. For every text-bearing element it composites the ancestor
// background chain over the panel and computes the WCAG contrast ratio; it passes only if every
// element meets AA (>= 4.5:1, all type here is below the large-text threshold) in BOTH light and
// dark, and if the header colour, container fill and reference-row fill each differ between the
// two themes (a hardcoded colour is identical across themes). It also reports the resolved
// semantic tokens' contrast on --panel / --panel-2 so the fix can be chosen to actually pass AA.
//
// Run with node tests/run-e2e.mjs --only veriftheme.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** In-page probe: measure the rendered verification block. Self-contained — a function passed to
 * page.evaluate is serialized into the page, so module closures are unavailable. */
function probeVerifBlock() {
  const block = document.querySelector('[data-testid="results-analytic-verification"]');
  if (!block) return { present: false };

  const parse = (s) => {
    const m = s.match(/rgba?\(([^)]+)\)/);
    if (!m) return { r: 0, g: 0, b: 0, a: 0 };
    const p = m[1].split(',').map((x) => parseFloat(x.trim()));
    return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
  };
  const over = (fg, bg) => {
    const a = fg.a + bg.a * (1 - fg.a);
    if (a === 0) return { r: 0, g: 0, b: 0, a: 0 };
    return {
      r: (fg.r * fg.a + bg.r * bg.a * (1 - fg.a)) / a,
      g: (fg.g * fg.a + bg.g * bg.a * (1 - fg.a)) / a,
      b: (fg.b * fg.a + bg.b * bg.a * (1 - fg.a)) / a,
      a,
    };
  };
  // Effective background behind an element: composite its ancestor chain's background-colour
  // alphas down over the first opaque ancestor (or white if none is opaque).
  const effBg = (el) => {
    const layers = [];
    let n = el;
    while (n && n !== document.documentElement) {
      const c = parse(getComputedStyle(n).backgroundColor);
      if (c.a > 0) layers.push(c);
      if (c.a >= 1) break;
      n = n.parentElement;
    }
    let base = layers.length ? layers[layers.length - 1] : { r: 255, g: 255, b: 255, a: 1 };
    let c = base.a >= 1 ? base : over(base, { r: 255, g: 255, b: 255, a: 1 });
    for (let i = layers.length - 2; i >= 0; i--) c = over(layers[i], c);
    return c;
  };
  const lum = ({ r, g, b }) => {
    const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const contrast = (a, b) => {
    const L1 = lum(a), L2 = lum(b);
    return (Math.max(L1, L2) + 0.05) / (Math.min(L1, L2) + 0.05);
  };
  const tok = (name) => {
    const d = document.createElement('div');
    d.style.color = `var(${name})`;
    document.body.appendChild(d);
    const c = getComputedStyle(d).color;
    d.remove();
    return c;
  };
  const rgb = (c) => `rgb(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)})`;

  const tokens = {
    accent: tok('--accent'), text: tok('--text'), text2: tok('--text-2'), text3: tok('--text-3'),
    panel: tok('--panel'), panel2: tok('--panel-2'), ok: tok('--ok'), danger: tok('--danger'),
    accentSoft: tok('--accent-soft'), line: tok('--line'),
  };
  // Token-on-panel contrasts to guide the fix (which semantic token passes AA on which fill).
  const cp = (fgName, bgName) => Math.round(contrast(parse(tokens[fgName]), parse(tokens[bgName])) * 100) / 100;
  const tokenContrasts = {
    accentOnPanel: cp('accent', 'panel'), text2OnPanel: cp('text2', 'panel'), text3OnPanel: cp('text3', 'panel'),
    okOnPanel: cp('ok', 'panel'), okOnPanel2: cp('ok', 'panel2'),
    dangerOnPanel: cp('danger', 'panel'), dangerOnPanel2: cp('danger', 'panel2'),
  };

  // Every element with its own non-whitespace text node is a text leaf to measure.
  const leaves = [];
  const walk = (el) => {
    for (const node of el.childNodes) {
      if (node.nodeType === 3 && node.textContent.trim()) {
        const cs = getComputedStyle(el);
        const fg = parse(cs.color);
        const bg = effBg(el);
        const ratio = Math.round(contrast(fg, bg) * 100) / 100;
        leaves.push({
          text: node.textContent.trim().slice(0, 28),
          color: cs.color, bg: rgb(bg), ratio,
          fontSize: cs.fontSize, fontWeight: cs.fontWeight,
        });
        break;
      }
    }
    for (const child of el.children) walk(child);
  };
  walk(block);

  const headerSpan = block.querySelector('span');
  const headerColor = headerSpan ? getComputedStyle(headerSpan).color : null;
  const containerBg = getComputedStyle(block).backgroundColor;
  // First reference-note row: a child div of the static-notes container with a background fill.
  let rowBg = null;
  for (const d of block.querySelectorAll('div')) {
    const bg = getComputedStyle(d).backgroundColor;
    if (parse(bg).a > 0 && d !== block && !d.querySelector('[data-testid="results-live-asserts"]')) {
      // prefer the static-note rows (they contain a formula .num div); fall back to any filled row
      if (d.querySelector('.num') || rowBg === null) rowBg = bg;
    }
  }
  const fails = leaves.filter((l) => l.ratio < 4.5);
  const minRatio = leaves.length ? Math.min(...leaves.map((l) => l.ratio)) : 0;
  return {
    present: true, tokens, tokenContrasts, headerColor, containerBg, rowBg,
    leafCount: leaves.length, minRatio, fails,
    sample: leaves.slice(0, 14),
  };
}

export async function verifthemeTest(browser, url, { docsDir }) {
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

  const awaitLadder = async (tag) => {
    const t0 = Date.now();
    for (;;) {
      const s = await page.evaluate(() => ({
        status: window.fieldsmith.analysis.state.status,
        error: window.fieldsmith.analysis.state.error,
        levels: window.fieldsmith.analysis.state.levels.length,
        resultsFor: window.fieldsmith.analysis.state.resultsFor,
        dv: window.fieldsmith.store.docVersion,
      }));
      if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) return;
      if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`${tag}: analysis error: ${s.error}`);
      if (Date.now() - t0 > 240000) throw new Error(`${tag}: ladder did not settle (last ${JSON.stringify(s)})`);
      await page.waitForTimeout(250);
    }
  };

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    // Load the real A1 literate sheet's executable starterFsm (so doc.name matches the sheet),
    // compile, open Results, and let the ladder settle.
    await page.evaluate(async () => {
      const lit = await import('/src/doc/literate-fsm.ts');
      const sheet = lit.TUTORIAL_SHEETS.find((s) => s.id === 'A1');
      if (!sheet) throw new Error('A1 literate sheet not found');
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(sheet.starterFsm);
      if ('error' in r) throw new Error(`A1 parse: ${r.error.message}`);
      window.fieldsmith.store.load(r.ok);
    });
    await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 60000 });
    await page.locator('[data-lens="results"]').click();
    await page.waitForFunction(() => window.fieldsmith.store.lens === 'results', null, { timeout: 5000 });
    await awaitLadder('A1');
    await page.waitForSelector('[data-testid="results-analytic-verification"]', { timeout: 15000 });

    const meas = {};
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.waitForTimeout(150);
      const r = await page.evaluate(probeVerifBlock);
      meas[scheme] = r;
      if (!r.present) { check(`${scheme}: verification block renders`, false, 'block not present'); continue; }
      const failDesc = r.fails.map((f) => `"${f.text}" ${f.ratio}:1 (${f.color} on ${f.bg})`).join('; ');
      check(`${scheme}: every verification-block text meets WCAG AA (>= 4.5:1)`,
        r.leafCount > 0 && r.fails.length === 0,
        `${r.leafCount} text els; min ${r.minRatio}:1; ${r.fails.length} below AA${failDesc ? ': ' + failDesc : ''}`);
    }

    if (meas.light?.present && meas.dark?.present) {
      check('header colour tracks the theme (light != dark)',
        meas.light.headerColor !== meas.dark.headerColor,
        `light ${meas.light.headerColor} / dark ${meas.dark.headerColor}`);
      check('container fill tracks the theme (light != dark)',
        meas.light.containerBg !== meas.dark.containerBg,
        `light ${meas.light.containerBg} / dark ${meas.dark.containerBg}`);
      check('reference-note row fill tracks the theme (light != dark)',
        meas.light.rowBg !== meas.dark.rowBg,
        `light ${meas.light.rowBg} / dark ${meas.dark.rowBg}`);
      const tc = meas.light.tokenContrasts;
      check('semantic tokens are AA-legible on the panel (fix guidance)',
        tc.accentOnPanel >= 4.5 && tc.text2OnPanel >= 4.5 && tc.okOnPanel >= 4.5 && tc.dangerOnPanel >= 4.5,
        `light: accent ${tc.accentOnPanel}, text-2 ${tc.text2OnPanel}, text-3 ${tc.text3OnPanel}, ok ${tc.okOnPanel} (panel-2 ${tc.okOnPanel2}), danger ${tc.dangerOnPanel} (panel-2 ${tc.dangerOnPanel2})`);
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Literate .fsm Verification block cohesion (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The Results-lens provenance card (`results-analytic-verification`) speaks the theme palette.',
      'It was built from hardcoded slate/blue inline colours on a `var(--panel)` panel, so in light',
      'mode the header (#93c5fd) and Static Reference Notes label (#94a3b8) fell below WCAG AA and the',
      'reference rows were fixed dark islands. The gate loads the real A1 cantilever literate sheet,',
      'solves it, opens Results, and for every text element composites the ancestor background chain',
      'and computes the WCAG contrast ratio; it passes only if every element meets AA (>= 4.5:1) in',
      'light and dark and the header / container / row fills each track the theme.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Asserted by computed colour + composited-background contrast, not a painted screenshot.',
      '- Renders the A1 sheet specifically (its @check notes and @assert contracts both populate the block); other sheets share the same markup and tokens.',
      '- AA normal-text threshold (4.5:1) applied to every element; all type in the block is below the large-text size/weight threshold.',
      '- Evaluated in Chromium 142.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/veriftheme.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['veriftheme: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'veriftheme-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`veriftheme: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}