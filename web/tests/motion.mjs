// Reduced-motion integrity: when the OS asks for reduced motion, every CSS transition and
// animation in the app must collapse to a near-instant duration and stop looping. Before this
// gate the stylesheet had two transitions (the .pareto-pt radius hover and the mobile slide-in
// drawers) and no `prefers-reduced-motion` handling at all, so vestibular-sensitive users got the
// full motion regardless of their OS setting.
//
// The probe synthesizes the app's two real motion sites plus a generic inline transition+animation
// (so the universal reset is tested for arbitrary future motion, not just the known sites), and
// reads getComputedStyle under both media states. Under `no-preference` the motion must be PRESENT
// (this proves the probe actually measures duration rather than passing vacuously); under `reduce`
// it must collapse below 1ms and the loop count must drop to 1. Red before the fix, green after.
//
// Run with node tests/run-e2e.mjs --only motion.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The app's two real transitions, plus a generic transition+animation the universal reset must
// also catch. `svg` builds the element in the SVG namespace (the pareto point is a <circle>).
// `anim` marks the probe that also carries an animation, so its loop count is asserted too.
const SPECS = [
  { label: '.pareto-pt (radius hover)', svg: true, tag: 'circle', cls: 'pareto-pt' },
  { label: '.panel.tree-panel (mobile drawer)', tag: 'div', cls: 'panel tree-panel' },
  {
    label: 'generic transition+animation',
    tag: 'div',
    anim: true,
    inline: 'transition: transform 200ms ease; animation: fs-motion-probe 1s linear infinite;',
  },
];

/** In-page probe: build each motion site off-screen and read its computed durations. */
function probeMotion(specs) {
  const host = document.createElement('div');
  host.style.position = 'absolute';
  host.style.left = '-9999px';
  host.style.top = '0';
  document.body.appendChild(host);

  const readings = specs.map((spec) => {
    let el;
    if (spec.svg) {
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
      el = document.createElementNS('http://www.w3.org/2000/svg', spec.tag);
      svg.appendChild(el);
      host.appendChild(svg);
    } else {
      el = document.createElement(spec.tag);
      host.appendChild(el);
    }
    if (spec.cls) el.setAttribute('class', spec.cls);
    if (spec.inline) el.setAttribute('style', spec.inline);
    const cs = getComputedStyle(el);
    const r = {
      label: spec.label,
      transitionDuration: cs.transitionDuration,
      animationDuration: cs.animationDuration,
      animationIterationCount: cs.animationIterationCount,
    };
    if (spec.svg) el.parentNode.remove();
    else el.remove();
    return r;
  });
  host.remove();
  return readings;
}

/** Largest value of a possibly comma-separated CSS duration list, in seconds. */
const secs = (v) => Math.max(...String(v).split(',').map((s) => parseFloat(s) || 0));

export async function motionTest(browser, url, { docsDir }) {
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

  // Static guard: the stylesheet must declare the reduced-motion policy at all.
  const css = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');
  const hasBlock = /@media\s*\(prefers-reduced-motion:\s*reduce\)/.test(css);
  check('styles.css declares a prefers-reduced-motion: reduce block', hasBlock, hasBlock ? 'present' : 'absent');

  // Narrow viewport so the mobile drawer transition (inside @media max-width:900px) is active.
  const page = await browser.newPage({ viewport: { width: 800, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    for (const motion of ['no-preference', 'reduce']) {
      await page.emulateMedia({ reducedMotion: motion });
      const readings = await page.evaluate(probeMotion, SPECS);
      SPECS.forEach((spec, i) => {
        const r = readings[i];
        const td = secs(r.transitionDuration);
        const ad = secs(r.animationDuration);
        const detail =
          `transition ${r.transitionDuration}; animation ${r.animationDuration}; ` +
          `iterations ${r.animationIterationCount}`;
        if (motion === 'no-preference') {
          // Motion must be present, so the reduce check below is a real discrimination.
          const pass = td > 0 && (!spec.anim || (ad > 0 && r.animationIterationCount === 'infinite'));
          check(`no-preference: ${r.label} keeps its motion`, pass, detail);
        } else {
          const pass = td < 0.001 && (!spec.anim || (ad < 0.001 && r.animationIterationCount === '1'));
          check(`reduce: ${r.label} motion collapsed`, pass, detail);
        }
      });
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Reduced-motion integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'When the OS requests reduced motion, every CSS transition and animation collapses to a',
      'near-instant duration (0.01ms, so any end events still fire) and infinite loops stop. The',
      'probe synthesizes the app’s two real motion sites (the `.pareto-pt` radius hover and the',
      'mobile slide-in drawers) plus a generic inline transition+animation, and reads',
      '`getComputedStyle` under both media states. Under `no-preference` the motion must be present',
      '(proving the probe measures duration); under `reduce` it must fall below 1ms and the loop',
      'count must drop to 1. The universal reset uses `!important` to beat the class-level',
      '`transition` shorthands and inline styles.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The probe tests the stylesheet policy per motion site, not every rendered instance.',
      '- JS-driven `requestAnimationFrame` loops (the 3D viewport render loop, data polling) are',
      '  essential interactivity, not decorative motion, and are intentionally left running.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/motion.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['motion: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'motion-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`motion: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}