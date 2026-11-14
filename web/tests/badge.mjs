// Verification-badge honesty (web UI): a sheet whose verification report is NOT shipped
// must not advertise a green "Verified · PASS" chip. The literate header carries
// `# @verification_status: pending` for those sheets; the Tutorials drawer renders them
// with an amber `--warn` "verification pending" chip and no green check, while a genuinely
// verified sheet keeps its green `--ok` ✓ chip. The data-layer honesty (badge text, refs
// resolving on disk) is gated separately by web/tests/registry.mjs; this suite gates the
// rendered surface so the UI cannot quietly re-green a pending sheet.
//
// Samples A15 + T18 (pending) and A1 (verified, V1.md & V2.md shipped) across both view
// modes (contents list + detailed cards) under both light and dark colour schemes, reading
// chip colour from getComputedStyle against live --warn / --ok probes, and fails on any
// console error.
//
// Run with node tests/run-e2e.mjs --only badge.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const PENDING = ['A15', 'T18'];
const VERIFIED = ['A1'];

// Self-contained in-page reader (arg-last): collect the verification chips inside each
// contents card, plus the resolved --warn / --ok probe colours (hex→rgb normalised).
function readSurface(ids) {
  const probe = (token) => {
    const el = document.createElement('span');
    el.style.color = `var(${token})`;
    document.body.appendChild(el);
    const c = getComputedStyle(el).color;
    el.remove();
    return c;
  };
  const warnRgb = probe('--warn');
  const okRgb = probe('--ok');
  const out = {};
  for (const id of ids) {
    const card = document.querySelector(`[data-testid="contents-card-${id}"]`);
    const chips = [];
    if (card) {
      for (const span of card.querySelectorAll('span')) {
        const text = (span.textContent || '').trim();
        if (text === 'verification pending' || text.startsWith('✓')) {
          chips.push({ text, color: getComputedStyle(span).color, title: span.getAttribute('title') || '' });
        }
      }
    }
    out[id] = { present: !!card, chips };
  }
  return { warnRgb, okRgb, cards: out };
}

async function openDrawer(page) {
  await page.evaluate(() => {
    const { ui } = window.fieldsmith;
    ui.set('tutorialsOpen', true);
    ui.set('tutorialMinimised', false);
    ui.set('tutorialContentsOpen', true);
  });
  await page.waitForSelector('[data-testid="contents-card-A15"]', { state: 'visible', timeout: 30000 });
}

export async function badgeTest(browser, url, { docsDir }) {
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

  const page = await browser.newPage();
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto(url);
      await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });
      await openDrawer(page);

      for (const view of ['contents', 'cards']) {
        if (view === 'cards') {
          await page.locator('button:has-text("Detailed Cards")').first().click();
          await page.waitForTimeout(250);
        }
        const { warnRgb, okRgb, cards } = await page.evaluate(readSurface, [...PENDING, ...VERIFIED]);
        const tag = `${scheme}/${view}`;

        for (const id of PENDING) {
          const c = cards[id];
          check(`${tag}: ${id} card present`, c.present);
          const pending = c.chips.filter((x) => x.text === 'verification pending');
          const verified = c.chips.filter((x) => x.text.startsWith('✓'));
          check(`${tag}: ${id} shows a pending chip`, pending.length === 1, JSON.stringify(c.chips));
          check(`${tag}: ${id} shows NO green ✓ chip`, verified.length === 0, JSON.stringify(verified));
          if (pending.length === 1) {
            check(`${tag}: ${id} pending chip is amber (--warn)`, pending[0].color === warnRgb, `${pending[0].color} (want ${warnRgb})`);
            check(`${tag}: ${id} pending chip title flags pending`, /pending/i.test(pending[0].title), pending[0].title);
          }
        }

        for (const id of VERIFIED) {
          const c = cards[id];
          check(`${tag}: ${id} card present`, c.present);
          const pending = c.chips.filter((x) => x.text === 'verification pending');
          const verified = c.chips.filter((x) => x.text.startsWith('✓'));
          check(`${tag}: ${id} shows NO pending chip`, pending.length === 0, JSON.stringify(pending));
          // List collapses extra refs into one "✓ V1+1" chip; cards map every ref.
          const expected = view === 'cards' ? 2 : 1;
          check(`${tag}: ${id} shows ${expected} green ✓ chip(s)`, verified.length === expected, JSON.stringify(verified));
          for (const v of verified) {
            check(`${tag}: ${id} ✓ chip is green (--ok)`, v.color === okRgb, `${v.color} (want ${okRgb})`);
            check(`${tag}: ${id} ✓ chip title claims verification`, /verif/i.test(v.title), v.title);
          }
        }
      }
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Verification-badge honesty (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'A sheet whose verification report is not shipped must not advertise a green',
      '"Verified · PASS" chip. The literate header marks those sheets',
      '`# @verification_status: pending`; the Tutorials drawer renders them with an amber',
      '`--warn` "verification pending" chip and no green check, while a genuinely verified',
      'sheet keeps its green `--ok` ✓ chip. This gates the rendered surface in both view modes',
      '(contents list + detailed cards) under both light and dark, reading chip colour from',
      '`getComputedStyle` against live `--warn` / `--ok` probes. The data-layer honesty (badge',
      'text and refs resolving on disk) is gated separately by `web/tests/registry.mjs`.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Samples A15 + T18 (pending) and A1 (verified) only; the other three pending sheets',
      '  (A16, A18, A25) share the same render path but are not individually asserted here.',
      '- Exercises the contents-list and detailed-card chips. The analytic-reference num span',
      '  and the markdown "Linked verification artifacts" column are not driven by this suite;',
      '  their underlying status/refs are covered by the registry gate.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/badge.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['badge: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'badge-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`badge: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}