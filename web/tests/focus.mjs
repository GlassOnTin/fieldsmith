// Keyboard-focus integrity: every interactive control must show a single designed focus ring
// when reached by keyboard, consistent with the accent border the text inputs already show on
// :focus. Before this gate the button/control vocabulary (.btn, .mini, .seg/.chips/.lenses/.menu
// buttons, palette rows, lens tabs) had :hover but no :focus-visible, so keyboard users got the
// browser's default `outline: auto` ring — inconsistent across browsers and invisible under some
// resets — instead of the app's accent ring.
//
// The probe synthesizes one representative element per control selector (so the CSS rule is
// tested for every class regardless of which lens is currently rendered), focuses it under
// keyboard modality, and reads getComputedStyle. A control passes only if it shows a SOLID
// outline >= 2px in the theme's --accent colour (or an accent box-shadow). The UA default
// (`outline: auto`, non-accent) fails, so the gate is red before the fix and green after.
//
// Run with node tests/run-e2e.mjs --only focus.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// One representative element per control selector. `parent` wraps descendant selectors
// (.seg button etc.); `attrs` makes non-button elements focusable.
const SPECS = [
  { label: 'button', tag: 'button' },
  { label: '.btn', tag: 'button', cls: 'btn' },
  { label: '.btn.icon', tag: 'button', cls: 'btn icon' },
  { label: '.mini', tag: 'button', cls: 'mini' },
  { label: '.seg button', tag: 'button', parent: 'seg' },
  { label: '.chips button', tag: 'button', parent: 'chips' },
  { label: '.lenses button', tag: 'button', parent: 'lenses' },
  { label: '.menu button', tag: 'button', parent: 'menu' },
  { label: '.palette li', tag: 'li', parent: 'palette', attrs: { tabindex: '0' } },
  { label: '[role="tab"]', tag: 'button', attrs: { role: 'tab' } },
  { label: 'a[href]', tag: 'a', attrs: { href: '#' } },
];

/** In-page probe: resolve --accent to rgb(), then for each control spec read the computed
 * focus ring under keyboard modality. Returns the accent colour and per-control readings. */
function probeFocus(specs) {
  const host = document.createElement('div');
  host.style.position = 'absolute';
  host.style.left = '-9999px';
  host.style.top = '0';
  document.body.appendChild(host);

  // Resolve the theme accent through a probe so it normalises hex -> rgb().
  const accentEl = document.createElement('div');
  accentEl.style.color = 'var(--accent)';
  host.appendChild(accentEl);
  const accent = getComputedStyle(accentEl).color;

  const readings = specs.map((spec) => {
    let mount = host;
    if (spec.parent) {
      const wrap = document.createElement(spec.parent === 'palette' ? 'ul' : 'div');
      wrap.className = spec.parent;
      host.appendChild(wrap);
      mount = wrap;
    }
    const el = document.createElement(spec.tag);
    if (spec.cls) el.className = spec.cls;
    if (spec.attrs) for (const [k, v] of Object.entries(spec.attrs)) el.setAttribute(k, v);
    el.textContent = 'x';
    mount.appendChild(el);
    el.focus();
    const cs = getComputedStyle(el);
    const r = {
      label: spec.label,
      focusVisible: el.matches(':focus-visible'),
      isActive: document.activeElement === el,
      outlineStyle: cs.outlineStyle,
      outlineWidth: cs.outlineWidth,
      outlineColor: cs.outlineColor,
      boxShadow: cs.boxShadow,
    };
    el.remove();
    if (spec.parent) mount.remove();
    return r;
  });
  host.remove();
  return { accent, readings };
}

/** A designed ring: solid outline >= 2px in the accent colour, or an accent box-shadow. */
function hasAccentRing(r, accent) {
  const w = parseFloat(r.outlineWidth) || 0;
  const outline = r.outlineStyle === 'solid' && w >= 2 && r.outlineColor === accent;
  const shadow = r.boxShadow && r.boxShadow !== 'none' && r.boxShadow.includes(accent);
  return outline || shadow;
}

export async function focusTest(browser, url, { docsDir }) {
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
      const { accent, readings } = await page.evaluate(probeFocus, SPECS);
      for (const r of readings) {
        const pass = r.focusVisible && hasAccentRing(r, accent);
        check(
          `${scheme}: ${r.label} shows an accent focus ring`,
          pass,
          `outline ${r.outlineStyle} ${r.outlineWidth} ${r.outlineColor}; shadow ${r.boxShadow}; ` +
            `:focus-visible=${r.focusVisible} (accent ${accent})`,
        );
      }
    }

    // Compose the rule with a real live control (not just a synthetic probe).
    await page.emulateMedia({ colorScheme: 'light' });
    await page.keyboard.press('Tab');
    const live = await page.evaluate(() => {
      // First enabled, rendered button (the first .btn may be a disabled Undo/Redo).
      const el = [...document.querySelectorAll('button, .btn')].find(
        (b) => !b.disabled && b.getClientRects().length > 0,
      );
      if (!el) return null;
      el.focus();
      const cs = getComputedStyle(el);
      const accentEl = document.createElement('div');
      accentEl.style.color = 'var(--accent)';
      document.body.appendChild(accentEl);
      const accent = getComputedStyle(accentEl).color;
      accentEl.remove();
      return {
        tag: el.tagName.toLowerCase(),
        cls: el.className,
        focusVisible: el.matches(':focus-visible'),
        outlineStyle: cs.outlineStyle,
        outlineWidth: cs.outlineWidth,
        outlineColor: cs.outlineColor,
        boxShadow: cs.boxShadow,
        accent,
      };
    });
    check(
      'live first button shows an accent focus ring',
      !!live && live.focusVisible && hasAccentRing(live, live.accent),
      live
        ? `${live.tag}.${live.cls}: outline ${live.outlineStyle} ${live.outlineWidth} ${live.outlineColor}; :focus-visible=${live.focusVisible}`
        : 'no button found in the live DOM',
    );

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Keyboard-focus integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Every interactive control shows one designed focus ring when reached by keyboard, matching',
      'the accent border the text inputs already show on `:focus`. The probe synthesizes a',
      'representative element per control selector, focuses it under keyboard modality, and reads',
      '`getComputedStyle`. A control passes only with a solid outline >= 2px in the theme `--accent`',
      'colour (or an accent box-shadow); the browser default `outline: auto` fails. Checked in both',
      'light and dark, plus one real live button.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Text inputs (`input`, `select`, the `.fsm` textarea) indicate focus with an accent border',
      '  and `outline: none`; they are intentionally outside this outline-based gate.',
      '- The probe tests the stylesheet rule per control class, not every rendered instance.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/focus.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['focus: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'focus-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`focus: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}