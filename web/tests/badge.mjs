// Verification-badge honesty (web UI): a sheet whose verification report is NOT shipped
// must not advertise a green "Verified · PASS" chip. The literate header carries
// `# @verification_status: pending` for those sheets; the Tutorials drawer renders them
// with an amber `--warn` "verification pending" chip and no green check, while a genuinely
// verified sheet keeps its green `--ok` ✓ chip. The data-layer honesty (badge text, refs
// resolving on disk) is gated separately by web/tests/registry.mjs; this suite gates the
// rendered surface so the UI cannot quietly re-green a pending sheet.
//
// Samples all five pending sheets (A15, A16, A18, A25, T18) and A1 (verified, V1.md & V2.md
// shipped) across both view modes (contents list + detailed cards) under both light and dark
// colour schemes, reading chip colour from getComputedStyle against live --warn / --ok
// probes. It then drives the sheet-detail analytic-reference num span for the same six
// sheets and asserts the pending state uses the same amber --warn convention there (5S
// standardise: one pending state, one colour), while a verified sheet stays neutral
// --text-2. Fails on any console error.
//
// Run with node tests/run-e2e.mjs --only badge.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const PENDING = ['A15', 'A16', 'A18', 'A25', 'T18'];
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

// Self-contained in-page reader (no args): the analytic-reference status span in the
// sheet-detail user column, plus resolved --warn / --text-2 probes. The span is the first
// span.num inside the analytic-checks card head (.tut-step-top); the @check table below is
// a <table class="num">, not a span, so it is not matched.
function readNumSpan() {
  const probe = (token) => {
    const el = document.createElement('span');
    el.style.color = `var(${token})`;
    document.body.appendChild(el);
    const c = getComputedStyle(el).color;
    el.remove();
    return c;
  };
  const card = document.querySelector('[data-testid="tutorial-analytic-checks-card"]');
  const span = card ? card.querySelector('.tut-step-top span.num') : null;
  return {
    warnRgb: probe('--warn'),
    text2Rgb: probe('--text-2'),
    present: !!span,
    text: span ? (span.textContent || '').trim() : '',
    color: span ? getComputedStyle(span).color : '',
    title: span ? (span.getAttribute('title') || '') : '',
  };
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

      // Analytic-reference num span (sheet-detail user column): the same pending state must
      // use the same amber --warn convention as the list/card chips (5S standardise). The
      // span renders from the static parsed @check header, so no FE solve / model load is
      // needed. Hide the detail first so the card detaches, then select the sheet — this
      // avoids reading a stale card left over from the previous id.
      for (const id of [...PENDING, ...VERIFIED]) {
        await page.evaluate(() => { window.fieldsmith.ui.set('tutorialContentsOpen', true); });
        await page.waitForSelector('[data-testid="tutorial-analytic-checks-card"]', { state: 'detached', timeout: 30000 }).catch(() => {});
        await page.evaluate((sid) => {
          const { ui } = window.fieldsmith;
          ui.set('tutorialsOpen', true);
          ui.set('tutorialMinimised', false);
          ui.set('tutorialSheetId', sid);
          ui.set('tutorialContentsOpen', false);
        }, id);
        await page.waitForSelector('[data-testid="tutorial-analytic-checks-card"]', { state: 'visible', timeout: 30000 });
        const ns = await page.evaluate(readNumSpan);
        const tag = `${scheme}/numspan`;
        check(`${tag}: ${id} analytic num span present`, ns.present);
        if (PENDING.includes(id)) {
          check(`${tag}: ${id} num span reads pending`, ns.text === 'verification pending', ns.text);
          check(`${tag}: ${id} num span is amber (--warn)`, ns.color === ns.warnRgb, `${ns.color} (want ${ns.warnRgb})`);
          check(`${tag}: ${id} num span title flags pending`, /pending/i.test(ns.title), ns.title);
        } else {
          check(`${tag}: ${id} num span is NOT pending`, ns.text !== 'verification pending', ns.text);
          check(`${tag}: ${id} num span shows refs`, /V\d/.test(ns.text), ns.text);
          check(`${tag}: ${id} num span is neutral (--text-2)`, ns.color === ns.text2Rgb, `${ns.color} (want ${ns.text2Rgb})`);
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
      '`getComputedStyle` against live `--warn` / `--ok` probes, and drives the sheet-detail',
      'analytic-reference num span for the same sheets so the pending state uses one amber',
      'convention everywhere (5S standardise). The data-layer honesty (badge text and refs',
      'resolving on disk) is gated separately by `web/tests/registry.mjs`.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Covers all five pending sheets (A15, A16, A18, A25, T18) and A1 (verified) on the',
      '  contents-list chips, detailed-card chips, and the analytic-reference num span.',
      '- The markdown "Linked verification artifacts" column is not driven by this suite;',
      '  its underlying status/refs are covered by the registry gate.',
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