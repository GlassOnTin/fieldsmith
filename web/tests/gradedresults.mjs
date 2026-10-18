// The Results lens reading the graded thermal field (VISION §7 item 3 leftover): the
// ladder's worker packs the fill's thermal cases into the level's case slots —
// mechanical first, thermal after, the solid path's own order — so the same picker,
// colour ramp and legend the solid level offers work on a graded level too. This family
// drives the REAL lens through the UI (unlike gradedthermal, which calls the wasm
// handle directly), pinning the composed path: worker merge → level meta → results.tsx.
//
// The native anchors are fieldsmith-core's `graded_envelope_covers_the_thermal_case`
// (the graded envelope covers the thermal contribution — the solid path's documented
// "envelope over every case" semantics) and the worker-level contract asserted directly
// here: one case list, matching u/vmElem/temperature slot lengths, thermal temperature
// present on its slot.
//
// Run with node tests/run-e2e.mjs --only gradedresults.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const DOC_A = `\
fieldsmith 1
name "Graded thermal bar"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hot = beam.nx
region cool = beam.px
support clamp hot fixed=xyz
load pull cool force=(10, 0, 0)
temperature hot hot temp=100 case="Thermal 1"
temperature cold cool temp=0
fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)
solve size=8 tref=20
`;

/** Settle loop: resolves when the ladder is ready on the current docVersion; throws on
 * the worker's error post. Runs on the caller's page. */
async function awaitLadder(page, tag) {
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
    if (Date.now() - t0 > 120000) throw new Error(`${tag}: ladder did not settle (last status ${JSON.stringify(s)})`);
    await page.waitForTimeout(250);
  }
}

/** Level meta + slots after the merge (plain data only), plus the picker's DOM options. */
function levelRead() {
  const lvl = window.fieldsmith.analysis.latest;
  const picker = Array.from(document.querySelectorAll('[data-testid="results-case"] button')).map((b) => b.textContent.trim());
  const temperature = lvl.temperature.map((t) => (t ? t.length : 0));
  return {
    caseNames: lvl.meta.cases.map((c) => c.name),
    slots: { u: lvl.u.length, vmElem: lvl.vmElem.length, temperature },
    thermalRow: lvl.meta.cases.find((c) => c.name === 'Thermal 1') ?? null,
    solid_case_names: (lvl.meta.solid_cases ?? []).map((c) => c.name),
    picker,
  };
}

/** Colour census of the rendered field (distinct quantized colours, anything off the
 * 0.55 no-data grey) plus the legend block's bound inputs and caption. Self-contained —
 * it is serialised into the page by page.evaluate, so it may reference only
 * window/document. */
function lensRead() {
  const c = window.fieldsmith.results.colors();
  let distinct = 0;
  let offGrey = false;
  if (c) {
    const seen = new Set();
    for (let i = 0; i < c.length; i += 3) {
      const key = `${Math.round(c[i] * 128)},${Math.round(c[i + 1] * 128)},${Math.round(c[i + 2] * 128)}`;
      seen.add(key);
      if (Math.abs(c[i] - 0.55) > 0.01 || Math.abs(c[i + 1] - 0.55) > 0.01 || Math.abs(c[i + 2] - 0.55) > 0.01) offGrey = true;
    }
    distinct = seen.size;
  }
  const lo = parseFloat(document.querySelector('[data-testid="legend-lo"]')?.value ?? '');
  const hi = parseFloat(document.querySelector('[data-testid="legend-hi"]')?.value ?? '');
  const caption = document.querySelector('[data-testid="legend"] .meta')?.textContent?.trim() ?? '';
  return { distinct, offGrey, lo, hi, caption };
}

export async function gradedresultsTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `gradedresults-${name}.png`);
  const table = (names) =>
    [
      '| Check | Result | Detail |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  try {
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    // ---- A — the packed graded level: case merge, picker, temperature ramp --------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_A);
    await awaitLadder(page, 'A');
    const lvlD = await page.evaluate(levelRead);
    await page.screenshot({ path: shot('01-graded-lens-picker') });

    check(
      'A: the packed graded level merges mechanical and thermal cases in one list',
      JSON.stringify(lvlD.caseNames) === JSON.stringify(['Load 1', 'Thermal 1'])
        && lvlD.slots.u === 2 && lvlD.slots.vmElem === 2
        && lvlD.slots.temperature[0] === 0 && lvlD.slots.temperature[1] > 0
        && JSON.stringify(lvlD.solid_case_names) === JSON.stringify(['Load 1', 'Thermal 1'])
        && lvlD.thermalRow?.conduction?.converged === true,
      `cases [${lvlD.caseNames.join(', ')}], slots ${JSON.stringify(lvlD.slots)}, solid_cases [${lvlD.solid_case_names.join(', ')}], conduction converged ${lvlD.thermalRow?.conduction?.converged ?? 'n/a'}`,
    );
    check(
      'A: the case picker lists Envelope, Load 1 and Thermal 1',
      JSON.stringify(lvlD.picker) === JSON.stringify(['Envelope', 'Load 1', 'Thermal 1']),
      `picker options [${lvlD.picker.join(', ')}]`,
    );

    await page.locator('[data-testid="results-case"] button', { hasText: 'Thermal 1' }).click();
    await page.locator('[data-testid="results-mode"] button[data-value="temperature"]').click();
    await page.waitForTimeout(400);
    const tempC = await page.evaluate(lensRead);
    await page.screenshot({ path: shot('02-graded-temperature') });

    check(
      'A: temperature mode on Thermal 1 renders the graded conduction field',
      tempC.distinct > 1 && tempC.offGrey,
      `${tempC.distinct} distinct colours, offGrey ${tempC.offGrey} — a null slot (uniform grey) or a single-colour field would fail this`,
    );
    check(
      'A: the temperature caption carries the nodal phrasing',
      tempC.caption.includes('temperature, nodal (°C)'),
      tempC.caption,
    );

    // The auto bounds are data-min..data-max of the shown field; also compute them from
    // the packed slot here, so the legend is compared to the field it colours.
    const field = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
      return Array.from(lvl.temperature[ci]);
    });
    const dataMin = field.length ? Math.min(...field) : NaN;
    const dataMax = field.length ? Math.max(...field) : NaN;
    check(
      'A: the temperature legend auto bounds match the field data range (0…100 °C)',
      !Number.isNaN(tempC.lo) && Math.abs(tempC.lo - dataMin) < 0.5 && Math.abs(tempC.hi - dataMax) < 0.5
        && dataMin > -1e-6 && dataMin < 1e-3 && Math.abs(dataMax - 100) < 1e-3,
      `legend ${tempC.lo}…${tempC.hi} °C, field ${dataMin.toFixed(4)}…${dataMax.toFixed(4)} °C (${field.length} nodes)`,
    );

    // ---- B — the graded thermal stress and the envelope ---------------------------------
    await page.locator('[data-testid="results-mode"] button[data-value="stress"]').click();
    await page.waitForTimeout(400);
    const stressC = await page.evaluate(lensRead);
    const summary = await page.evaluate(() => document.querySelector('[data-testid="results-summary"]')?.textContent ?? '');
    await page.screenshot({ path: shot('03-graded-stress') });

    check(
      'B: stress mode on Thermal 1 renders the graded thermal stress',
      stressC.distinct > 1 && stressC.offGrey,
      `${stressC.distinct} distinct colours, offGrey ${stressC.offGrey} — stress below the legend floor renders uniform grey`,
    );
    check(
      'B: the summary shows the thermal case with its conduction cost',
      summary.includes('Thermal 1') && /Thermal 1[^]*· conduction \d+/.test(summary),
      summary.split(/\s{2,}/)[0] ?? summary,
    );

    // Envelope coverage through the packed level — the solid path's "envelope over every
    // case" semantics, folded in fill.rs and packed to the whole ladder.
    const env = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const env = lvl.vmElemEnvelope;
      const slots = lvl.vmElem;
      let n = 0;
      let bad = 0;
      for (let e = 0; e < env.length; e += 7) {
        n++;
        const want = Math.max(slots[0][e], slots[1][e]);
        if (env[e] !== want) bad++;
      }
      return { n, bad, elems: env.length };
    });
    check(
      'B: the envelope covers the graded thermal case',
      env.n > 0 && env.bad === 0 && env.elems > 0,
      `${env.bad} of ${env.n} sampled elements differ (of ${env.elems} total; envelope must equal max(Load 1, Thermal 1) elementwise)`,
    );

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V33 — The Results lens reads the graded thermal field',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'On a fill document the ladder packs the fill\'s thermal cases into the level\'s',
      'case slots — mechanical first, thermal after, the solid path\'s own order — along',
      'with their displacement, element-stress and temperature fields. The same case',
      'picker, colour ramp and legend the solid level offers then work on a graded',
      'level, and the graded envelope covers the thermal contribution (the solid path\'s',
      '"envelope over every case" semantics, folded over `cases` ∪ `thermal_cases` in',
      'fill.rs; a load-free graded document previously read a zero envelope).',
      '',
      'The native anchors are fieldsmith-core\'s',
      '`graded_envelope_covers_the_thermal_case` (envelope is the thermal case\'s own',
      'field bit-exactly on a load-free document) and the solver tests pinned at V32.',
      'This fixture drives the real lens: the worker merge, the case picker DOM, the',
      'temperature ramp, the auto legend bounds, the stress colours and the summary row',
      'with its conduction cost.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      table([
        'A: the packed graded level merges mechanical and thermal cases in one list',
        'A: the case picker lists Envelope, Load 1 and Thermal 1',
        'A: temperature mode on Thermal 1 renders the graded conduction field',
        'A: the temperature caption carries the nodal phrasing',
        'A: the temperature legend auto bounds match the field data range (0…100 °C)',
        'B: stress mode on Thermal 1 renders the graded thermal stress',
        'B: the summary shows the thermal case with its conduction cost',
        'B: the envelope covers the graded thermal case',
        'no console errors',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/gradedresults.mjs`, driven by `node tests/run-e2e.mjs --only',
      'gradedresults`. The document loads through `parseFsm` + `store.load`, the ladder',
      'settles, and the fixture switches case and mode like a user: picker buttons by',
      'label, mode buttons by value, reading `window.fieldsmith.results.colors()` and',
      'the legend\'s bound inputs afterwards. Colours are judged by a census (distinct',
      'quantized RGB triples, off the 0.55 no-data grey) computed on the Node side.',
      '',
      '## Limits',
      '',
      '- Colour assertions are existence-style: distinct ramp colours, off the grey. The',
      '  exact ramp mapping is not asserted pointwise (the ramp maths has its own unit',
      '  coverage); what is asserted is that the graded fields reach the lens and colour',
      '  it.',
      '- The envelope equality is sampled (every 7th element) rather than exhaustive; it',
      '  is the same fold, computed once, compared to two slots, so a mismatch anywhere',
      '  in the fold source would be a systemic divergence, not a sampling artefact.',
      '- The graded conductivity read remains Voigt-style linear in φ — see V32\'s',
      '  Limits; no measured conduction law exists yet.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/gradedresults.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V33.md'), lines.join('\n'));
    console.log(`gradedresults ${done ? 'PASS' : 'FAIL'}`);
    if (!done) {
      for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    }
    if (errors.length) console.error('console errors:', errors);
    return done;
  } catch (e) {
    console.error('failed checks so far:');
    for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    throw e;
  } finally {
    await page.close();
  }
}