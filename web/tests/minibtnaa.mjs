// Mini-button label legibility: the reusable `.mini-btn` compact control carries interactive
// labels (11px, below the WCAG large-text threshold) so its resting text colour must meet AA
// (>= 4.5:1) against every panel it sits on. The control's fill is `background: none`, so the
// effective background is whatever panel is behind it. Its resting colour was `--text-3`, the
// muted tertiary token, which measures ~3.3:1 on the light panels and ~3.2-3.6:1 on the dark
// panels — below AA in both themes, on every surface that uses the class (results, fill, make,
// systems, antenna, and the calculator cards). The fix moves the resting colour to `--text-2`
// (the secondary token, AA on every panel in both themes); hover already rises to `--text`.
//
// The probe places a real `button.mini-btn` on each themed fill (--panel, --panel-2, --bg,
// --viewport), composites the ancestor background chain, and computes the WCAG contrast ratio of
// the resting label. It passes only if every fill meets AA in light and dark, if the hover label
// (--text on the --panel-2 hover fill) stays AA, and if a live rendered control meets AA. The live
// anchor drives to the Make lens, which shows an in-panel mini-btn (the minimize toggle) even with
// an empty model; the heavier outside-panel surfaces (systems swirl HUD, antenna bench) need a
// loaded model/bench, so the synthetic per-fill probe carries those backgrounds deterministically.
//
// Run with node tests/run-e2e.mjs --only minibtnaa.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** In-page probe: resting `.mini-btn` label contrast on each themed fill, plus the hover token
 * contrast. Self-contained — a function passed to page.evaluate is serialized into the page, so
 * module closures are unavailable. */
function probeMiniBtnAA() {
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
  const r2 = (x) => Math.round(x * 100) / 100;
  const rgb = (c) => `rgb(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)})`;
  const tok = (name) => {
    const d = document.createElement('div');
    d.style.color = `var(${name})`;
    document.body.appendChild(d);
    const c = getComputedStyle(d).color;
    d.remove();
    return c;
  };

  // A real .mini-btn on each themed fill the control can sit on (its own fill is transparent).
  const fills = ['--panel', '--panel-2', '--bg', '--viewport'];
  const host = document.createElement('div');
  host.style.cssText = 'position:absolute;left:-99999px;top:0;';
  document.body.appendChild(host);
  const resting = [];
  for (const f of fills) {
    const wrap = document.createElement('div');
    wrap.style.background = `var(${f})`;
    const btn = document.createElement('button');
    btn.className = 'mini-btn';
    btn.textContent = 'Label';
    wrap.appendChild(btn);
    host.appendChild(wrap);
    const cs = getComputedStyle(btn);
    const bg = effBg(btn);
    resting.push({ fill: f, color: cs.color, bg: rgb(bg), ratio: r2(contrast(parse(cs.color), bg)) });
  }
  host.remove();

  // Hover rises to --text on the --panel-2 hover fill; guard that it stays AA.
  const hoverRatio = r2(contrast(parse(tok('--text')), parse(tok('--panel-2'))));
  return {
    resting, hoverRatio,
    tokens: { text2: tok('--text-2'), text3: tok('--text-3'), text: tok('--text') },
  };
}

/** In-page live probe: contrast of every visible rendered .mini-btn label on the Make lens. */
function probeLiveMiniBtnAA() {
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
  const r2 = (x) => Math.round(x * 100) / 100;
  const rgb = (c) => `rgb(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)})`;
  const visible = (e) => e.getClientRects().length > 0;
  const els = [...document.querySelectorAll('.mini-btn')].filter(visible);
  const meas = els.map((e) => {
    const cs = getComputedStyle(e);
    const bg = effBg(e);
    return { text: (e.textContent || '').trim().slice(0, 16), color: cs.color, bg: rgb(bg), ratio: r2(contrast(parse(cs.color), bg)) };
  });
  return {
    total: els.length,
    min: meas.length ? Math.min(...meas.map((m) => m.ratio)) : 0,
    fails: meas.filter((m) => m.ratio < 4.5),
    sample: meas.slice(0, 6),
  };
}

export async function minibtnaaTest(browser, url, { docsDir }) {
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

    // Drive to the Make lens once: it renders an in-panel mini-btn even with an empty model.
    await page.emulateMedia({ colorScheme: 'light' });
    await page.click('button[data-lens=make]');
    await page.waitForSelector('[data-testid=make-panel]', { timeout: 15000 });

    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.waitForTimeout(150);
      const r = await page.evaluate(probeMiniBtnAA);
      const restDesc = r.resting.map((x) => `${x.fill} ${x.ratio}:1`).join(', ');
      const restFails = r.resting.filter((x) => x.ratio < 4.5);
      check(`${scheme}: resting .mini-btn label meets AA on every panel fill (>= 4.5:1)`,
        restFails.length === 0,
        `${restDesc}${restFails.length ? ' — below AA: ' + restFails.map((x) => `${x.fill} ${x.ratio}:1 (${x.color} on ${x.bg})`).join('; ') : ''} (text-2 ${r.tokens.text2}, text-3 ${r.tokens.text3})`);
      check(`${scheme}: hover label stays AA (--text on --panel-2)`,
        r.hoverRatio >= 4.5,
        `${r.hoverRatio}:1 (text ${r.tokens.text})`);

      const live = await page.evaluate(probeLiveMiniBtnAA);
      const liveDesc = live.fails.map((f) => `"${f.text}" ${f.ratio}:1 (${f.color} on ${f.bg})`).join('; ');
      check(`${scheme}: live Make-lens mini-btn labels meet AA`,
        live.total > 0 && live.fails.length === 0,
        `${live.total} visible; min ${live.min}:1; ${live.fails.length} below AA${liveDesc ? ': ' + liveDesc : ''}`);
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Mini-button label legibility (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The reusable `.mini-btn` compact control carries 11px interactive labels, below the WCAG',
      'large-text threshold, so its resting colour must meet AA (>= 4.5:1) on every panel it sits',
      'on (its fill is `background: none`). The resting colour was `--text-3`, the muted tertiary',
      'token (~3.3:1 light, ~3.2-3.6:1 dark on the panels) — below AA in both themes on every surface',
      'that uses the class. Moving the resting colour to `--text-2` (secondary, AA on every panel in',
      'both themes) fixes it; hover already rises to `--text`. The probe places a real button on each',
      'themed fill, composites the ancestor background, and computes the WCAG ratio; it passes only',
      'if every fill meets AA in light and dark, the hover label stays AA, and a live Make-lens',
      'control meets AA.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Asserted by computed colour + composited-background contrast, not a painted screenshot.',
      '- The live anchor is the Make lens minimize toggle (in-panel, renders with an empty model); the heavier outside-panel surfaces (systems swirl HUD, antenna bench) need a loaded model/bench, so the synthetic per-fill probe carries those backgrounds.',
      '- AA normal-text threshold (4.5:1) applied; the 11px label is below the large-text size/weight threshold.',
      '- Evaluated in Chromium 142.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/minibtnaa.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['minibtnaa: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'minibtnaa-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`minibtnaa: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}