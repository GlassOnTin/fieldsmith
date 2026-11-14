// Mini-button cohesion: the reusable `.mini-btn` compact control must speak the theme palette
// wherever it is used, not only inside the Results panel. The class is used 70+ times across
// 11 components, but its only rules were scoped `.results-panel .mini-btn` / `:hover`, so outside
// that one panel the class was inert and (observed on HeadlessChrome/142):
//   * <button class="mini-btn"> fell back to the UA button — bg rgb(239,239,239) light /
//     rgb(107,107,107) dark, border `2px outset`, height 24.19px, font 13px, cursor default;
//   * <a class="mini-btn"> fell back to the UA link — colour rgb(0,0,238) light /
//     rgb(158,158,255) dark (UA blue), no border, height auto, font 13px.
// Inside the panel both were themed (colour --text-2, border 1px solid --line, height 22px,
// font 11px, cursor pointer). The fix drops the `.results-panel ` ancestor prefix so the control
// is self-contained everywhere (the same shape as the native-accent fix). The global reset at
// `button,input,select,textarea { font: inherit; color: inherit }` sets font+colour only, so it
// never supplied the box; a global `.mini-btn` (0,1,0) outranks it and no other rule targets the
// class, so the in-panel rendering is unchanged.
//
// The probe synthesizes a real button and a real anchor with class `mini-btn` both inside a
// `.results-panel` div and inside a plain div, reads getComputedStyle, and passes only if the box
// equals the resolved theme tokens. The UA bevel / UA blue fails, so the gate is red before the
// fix and green after. Checked in light and dark. A live guard drives to the Make lens (which
// renders an in-panel mini-btn even with an empty model) and asserts every rendered mini-btn is
// themed. The outside-panel surfaces (systems swirl HUD, antenna bench) are gated behind a loaded
// model/bench, so the synthetic outside-panel probe carries that case deterministically.
//
// Run with node tests/run-e2e.mjs --only minibtn.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** In-page probe: resolve the theme tokens to rgb(), build a real button+anchor with class
 * `mini-btn` inside and outside a `.results-panel`, and read their computed box. Self-contained —
 * a function passed to page.evaluate is serialized into the page, so module closures are unavailable. */
function probeMiniBtn() {
  const tok = (name) => {
    const d = document.createElement('div');
    d.style.color = `var(${name})`;
    document.body.appendChild(d);
    const c = getComputedStyle(d).color;
    d.remove();
    return c;
  };
  const host = document.createElement('div');
  host.style.cssText = 'position:absolute;left:-99999px;top:0;';
  const panel = document.createElement('div');
  panel.className = 'results-panel';
  const outside = document.createElement('div');
  host.appendChild(panel);
  host.appendChild(outside);
  document.body.appendChild(host);
  const mk = (parent, tag) => {
    const e = document.createElement(tag);
    e.className = 'mini-btn';
    e.textContent = 'x';
    if (tag === 'a') e.href = '#';
    parent.appendChild(e);
    return e;
  };
  const read = (e) => {
    const c = getComputedStyle(e);
    return {
      color: c.color,
      bg: c.backgroundColor,
      border: `${c.borderTopWidth} ${c.borderTopStyle} ${c.borderTopColor}`,
      height: c.height,
      fontSize: c.fontSize,
      cursor: c.cursor,
    };
  };
  const out = {
    tokens: { text2: tok('--text-2'), line: tok('--line') },
    btnOut: read(mk(outside, 'button')),
    ancOut: read(mk(outside, 'a')),
    btnIn: read(mk(panel, 'button')),
  };
  host.remove();
  return out;
}

export async function minibtnTest(browser, url, { docsDir }) {
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

  // The themed box every `.mini-btn` must compute, wherever it lives: secondary text colour,
  // transparent fill, a 1px solid hairline border, 22px tall, 11px type, pointer cursor.
  const themedBox = (r, t) =>
    r.color === t.text2 &&
    r.bg === 'rgba(0, 0, 0, 0)' &&
    r.border === `1px solid ${t.line}` &&
    r.height === '22px' &&
    r.fontSize === '11px' &&
    r.cursor === 'pointer';
  const fmtBox = (r) =>
    `color ${r.color}; bg ${r.bg}; border ${r.border}; h ${r.height}; font ${r.fontSize}; cursor ${r.cursor}`;

  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      const r = await page.evaluate(probeMiniBtn);
      check(`${scheme}: button.mini-btn outside the panel is themed`,
        themedBox(r.btnOut, r.tokens),
        `${fmtBox(r.btnOut)} (want text-2 ${r.tokens.text2}, line ${r.tokens.line})`);
      check(`${scheme}: a.mini-btn outside the panel is themed`,
        themedBox(r.ancOut, r.tokens),
        `${fmtBox(r.ancOut)} (want text-2 ${r.tokens.text2}, line ${r.tokens.line})`);
      check(`${scheme}: button.mini-btn inside the panel stays themed`,
        themedBox(r.btnIn, r.tokens),
        `${fmtBox(r.btnIn)} (regression guard)`);
    }

    // Live rendered controls: the Make lens shows an in-panel mini-btn (the minimize toggle) even
    // with an empty model, so this anchors on a real control without needing a solve. Every visible
    // mini-btn must compute the themed box. The outside-panel surfaces need a loaded model/bench,
    // so they are covered by the synthetic probe above rather than relied on here.
    await page.emulateMedia({ colorScheme: 'light' });
    await page.click('button[data-lens=make]');
    await page.waitForSelector('[data-testid=make-panel]', { timeout: 15000 });
    const live = await page.evaluate(() => {
      const tok = (name) => {
        const d = document.createElement('div');
        d.style.color = `var(${name})`;
        document.body.appendChild(d);
        const c = getComputedStyle(d).color;
        d.remove();
        return c;
      };
      const text2 = tok('--text-2'), line = tok('--line');
      const visible = (e) => e.getClientRects().length > 0;
      const els = [...document.querySelectorAll('.mini-btn')].filter(visible);
      const themed = (e) => {
        const c = getComputedStyle(e);
        return c.color === text2 && c.backgroundColor === 'rgba(0, 0, 0, 0)' &&
          `${c.borderTopWidth} ${c.borderTopStyle} ${c.borderTopColor}` === `1px solid ${line}` &&
          c.height === '22px' && c.fontSize === '11px';
      };
      const inside = els.filter((e) => e.closest('.results-panel')).length;
      const unthemed = els.filter((e) => !themed(e)).map(
        (e) => `${e.tagName.toLowerCase()} "${(e.textContent || '').trim().slice(0, 16)}" ${getComputedStyle(e).color}`);
      return { text2, total: els.length, inside, outside: els.length - inside, allThemed: els.every(themed), unthemed };
    });
    check('live: rendered mini-btn controls are themed',
      live.total > 0 && live.allThemed,
      `${live.total} visible (${live.inside} in-panel, ${live.outside} outside); unthemed: ${live.unthemed.join(', ') || 'none'} (text-2 ${live.text2})`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Mini-button cohesion (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The reusable `.mini-btn` compact control speaks the theme palette wherever it is used.',
      'Its rules were scoped `.results-panel .mini-btn`, so outside that one panel the class was',
      'inert: a button fell back to the UA bevel (bg rgb(239,239,239) light / rgb(107,107,107) dark,',
      'border 2px outset, height 24.19px, font 13px, cursor default) and an anchor to UA blue',
      '(rgb(0,0,238) light / rgb(158,158,255) dark, no border, height auto). Dropping the',
      '`.results-panel ` prefix makes the control self-contained: colour --text-2, transparent fill,',
      '1px solid --line border, 22px tall, 11px type, pointer cursor — in every component. The probe',
      'reads the computed box on a real button and anchor placed inside and outside a `.results-panel`',
      'and passes only if it equals the resolved tokens, in light and dark.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Asserted by computed box (colour, fill, border, height, font, cursor), not a painted screenshot.',
      '- The live guard anchors on the Make lens minimize toggle (in-panel, renders with an empty model); the outside-panel surfaces (systems swirl HUD, antenna bench) need a loaded model/bench, so the synthetic outside-panel button+anchor probe carries that case.',
      '- Evaluated in Chromium 142; the UA bevel/blue fallback values are browser-specific, but the themed-box assertion is palette-driven.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/minibtn.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['minibtn: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'minibtn-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`minibtn: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}