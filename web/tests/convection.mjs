// The `convection` statement (VISION §7 item 3, the Robin boundary): a band of the
// surface sheds heat at h·(T − t_inf) per unit area, read against the solver's pinned
// solid conductivity k = 1 W/(mm·K), so h is in W/(mm²·K). The fixture is the
// 40 × 10 × 10 bar with a 100 °C Dirichlet patch on x = 0 and the band on the +x end
// face: the steady field is the 1D line
//
//   T(x) = 100 − h·(T0 − t_inf)/(k + h·L)·x = 100 − 1.6·x   (h = 0.1, t_inf = 20)
//
// with the band reading 36 °C. That is the exact anchor the fieldsmith-core tests pin,
// so the browser run lands on the same numbers as the native solver rather than merely
// on "a solved field"; a Dirichlet-at-band model would read 20 °C and slope −2, so the
// anchors are also discriminating. Three fixtures:
//
//   A — .fsm-authored: the statement with a let-driven h parses into the store, solves to
//       the line, and the .fsm round-trip re-prints the consumer expression verbatim
//       (h=2 * hv); a rename follows the recorded consumer. The parser refuses a bare
//       let name in h= (compound-only, same rule as temp=), so the fixture drives it
//       through an expression.
//   B — Setup-lens authored: add-convection on the selected region, h through the
//       inspector's number input; renaming both regions and the convection id (through
//       the shipped name field) rewrites every reference and re-solves to the same line;
//       another .fsm round-trip.
//   C — wholly convecting, no temperature region: the worker's setup gate admits the
//       study and every node settles to t_inf.
//
// Run with node tests/run-e2e.mjs --only convection.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const BAR = (extra) => `\
fieldsmith 1
name "Convection bar"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hot = beam.nx
region cool = beam.px
support clamp hot fixed=xyz
temperature fixer hot temp=100 case="Thermal 1"
${extra}\
`;

// A: h reads a let (the consumer record matters below); C drops the temperature patch
// entirely — the run then needs the convection to lift the Laplacian's constant mode.
const DOC_A = BAR(`let hv = 0.05\nconvection band cool h=2 * hv tinf=20 case="Thermal 1"\nsolve size=8 tref=20\n`);
const DOC_B = BAR(`solve size=8 tref=20\n`);
const DOC_C = `\
fieldsmith 1
name "Settler"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hold = beam.nx
region bath = beam.px
support clamp hold fixed=xyz
convection cool bath h=0.1 tinf=20 case="Thermal 1"
solve size=8
`;

/** Nodal read of the thermal case 'Thermal 1': line residuals (T − (100 − 1.6·x) at each
 * node's own x, from the same buffers), the line's measured slope from the coldest node,
 * and the band's anchor value. `note` carries what was missing when the run produced no
 * nodal field. */
function fieldRead() {
  const lvl = window.fieldsmith.analysis.latest;
  const out = { nodeCount: (lvl.nodes?.length ?? 0) / 3, slope: null, bandC: null, maxDev: null, note: '' };
  const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
  if (ci < 0) return { ...out, note: `no 'Thermal 1' case (cases: ${lvl.meta.cases.map((c) => c.name).join(', ')})` };
  const t = lvl.temperature[ci];
  if (!t) return { ...out, note: 'the thermal case carries no nodal field' };
  let min = Infinity, xAtMin = NaN, maxDev = 0;
  for (let n = 0; n < out.nodeCount; n++) {
    const x = lvl.nodes[3 * n], v = t[n];
    if (v < min) { min = v; xAtMin = x; }
    const dev = Math.abs(v - (100 - 1.6 * x));
    if (dev > maxDev) maxDev = dev;
  }
  return { ...out, slope: (min - 100) / xAtMin, bandC: min, maxDev };
}

export async function convectionTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `convection-${name}.png`);
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

    // ---- A — .fsm authored, let-driven h ------------------------------------------------
    const docA = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      return JSON.parse(JSON.stringify(window.fieldsmith.store.doc.convections));
    }, DOC_A);
    check(
      'A: the statement parses into a stored convection with its let evaluated into h',
      docA.length === 1 && docA[0].id === 'band' && docA[0].region === 'cool' && Math.abs(docA[0].h - 0.1) < 1e-12 && docA[0].t_inf === 20 && docA[0].case === 'Thermal 1',
      JSON.stringify(docA),
    );
    await page.locator('[data-lens="results"]').click();
    let wOk = false;
    {
      const t0 = Date.now();
      for (;;) {
        const s = await page.evaluate(() => ({
          status: window.fieldsmith.analysis.state.status,
          error: window.fieldsmith.analysis.state.error,
          levels: window.fieldsmith.analysis.state.levels.length,
          resultsFor: window.fieldsmith.analysis.state.resultsFor,
          dv: window.fieldsmith.store.docVersion,
        }));
        if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) { wOk = true; break; }
        if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`A: analysis error: ${s.error}`);
        if (Date.now() - t0 > 120000) throw new Error(`A: ladder did not settle (last status ${JSON.stringify(s)})`);
        await page.waitForTimeout(250);
      }
    }
    check('A: the run settles', wOk, 'ladder ready on the loaded docVersion');

    const fieldA = await page.evaluate(fieldRead);
    check(
      'A: the band drives the bar onto the Robin line (T = 100 − 1.6x, band 36 °C)',
      fieldA.maxDev !== null && fieldA.nodeCount > 0 && fieldA.maxDev < 1e-3 && Math.abs(fieldA.slope + 1.6) < 1e-4 && Math.abs(fieldA.bandC - 36) < 1e-3,
      `${fieldA.nodeCount} nodes, max |Δ| ${typeof fieldA.maxDev === 'number' ? fieldA.maxDev.toExponential(2) : fieldA.note}, slope ${fieldA.slope?.toFixed?.(6) ?? fieldA.slope}, band ${fieldA.bandC?.toFixed?.(3) ?? fieldA.bandC} °C; a Dirichlet-at-band model would read 20 °C and slope −2`,
    );
    // Consumer record: h reads let hv, so one `consumed` entry targets the convection.
    const consumedA = await page.evaluate(() => (window.fieldsmith.store.doc.consumed ?? []).filter((c) => c.target === 'band'));
    check(
      'A: the let-driven h left a consumer record',
      consumedA.length === 1 && consumedA[0].attr === 'h' && consumedA[0].comp === null && consumedA[0].expr === '2 * hv',
      JSON.stringify(consumedA),
    );
    await page.screenshot({ path: shot('01-fsm-authored-field') });

    const renamedA = await page.evaluate(async () => {
      const ops = await import('/src/doc/ops.ts');
      let err = null;
      window.fieldsmith.store.transact('Rename band to airband', (d) => {
        err = ops.renameId(d, 'band', 'airband');
      });
      return {
        err,
        id: window.fieldsmith.store.doc.convections[0]?.id ?? null,
        target: (window.fieldsmith.store.doc.consumed ?? []).find((c) => c.expr === '2 * hv')?.target ?? null,
      };
    });
    check(
      'A: renaming the convection follows its consumer record',
      renamedA.err === null && renamedA.id === 'airband' && renamedA.target === 'airband',
      JSON.stringify(renamedA),
    );

    const roundA = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      return {
        line: (text.split('\n').find((l) => l.startsWith('convection ')) ?? '').trim(),
        conv: r.ok ? JSON.parse(JSON.stringify(r.ok.convections)) : null,
      };
    });
    check(
      'A: the .fsm round-trip re-prints the let-driven h and re-parses it',
      roundA.line === 'convection airband cool h=2 * hv tinf=20 case="Thermal 1"'
        && roundA.conv?.length === 1 && roundA.conv[0].id === 'airband' && Math.abs(roundA.conv[0].h - 0.1) < 1e-12 && roundA.conv[0].t_inf === 20,
      JSON.stringify(roundA),
    );

    // ---- B — authored in the Setup lens ---------------------------------------------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
    }, DOC_B);
    await page.locator('[data-lens="setup"]').click();
    await page.locator('[data-testid="region-cool"]').click();
    await page.locator('[data-testid="add-convection"]').click();
    const addedB = await page.evaluate(() => JSON.parse(JSON.stringify(window.fieldsmith.store.doc.convections)));
    check(
      'B: add-convection stamps the defaults on the selected region',
      addedB.length === 1 && addedB[0].id === 'convection1' && addedB[0].region === 'cool' && Math.abs(addedB[0].h - 5e-6) < 1e-12 && addedB[0].t_inf === 20 && addedB[0].case === 'Thermal 1',
      JSON.stringify(addedB),
    );
    // The inspector came up with the new item selected (addAnalysis selects it).
    const numEl = page.locator('[data-testid="convection-h"]');
    await numEl.click();
    await numEl.fill('0.1');
    await numEl.press('Enter');
    {
      const t0 = Date.now();
      for (;;) {
        const s = await page.evaluate(() => ({
          status: window.fieldsmith.analysis.state.status,
          error: window.fieldsmith.analysis.state.error,
          levels: window.fieldsmith.analysis.state.levels.length,
          resultsFor: window.fieldsmith.analysis.state.resultsFor,
          dv: window.fieldsmith.store.docVersion,
        }));
        if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) break;
        if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`B: analysis error after h set: ${s.error}`);
        if (Date.now() - t0 > 120000) throw new Error(`B: ladder did not settle after h set (last status ${JSON.stringify(s)})`);
        await page.waitForTimeout(250);
      }
    }
    const fieldB1 = await page.evaluate(fieldRead);
    check(
      'B: the inspector-set h drives the same Robin line',
      fieldB1.maxDev !== null && fieldB1.nodeCount > 0 && fieldB1.maxDev < 1e-3 && Math.abs(fieldB1.slope + 1.6) < 1e-4 && Math.abs(fieldB1.bandC - 36) < 1e-3,
      `${fieldB1.nodeCount} nodes, max |Δ| ${typeof fieldB1.maxDev === 'number' ? fieldB1.maxDev.toExponential(2) : fieldB1.note}, slope ${fieldB1.slope?.toFixed?.(6) ?? fieldB1.slope}, band ${fieldB1.bandC?.toFixed?.(3) ?? fieldB1.bandC} °C`,
    );

    // Rename the two regions and the convection id through the shipped name field; every
    // reference moves with it — the convection's region, the temperature's region (a
    // rename-rewiring gap the region arm now closes) and the statement's own id.
    const renameWith = async (to) => {
      const el = page.locator('[data-testid="name"]');
      await el.click();
      await el.fill(to);
      await el.press('Enter');
      await page.waitForTimeout(100);
    };
    await page.locator('[data-testid="region-cool"]').click();
    await renameWith('airwall');
    await page.locator('[data-testid="region-hot"]').click();
    await renameWith('hotplate');
    await page.locator('[data-testid="convection-convection1"]').click();
    await renameWith('air');
    {
      const t0 = Date.now();
      for (;;) {
        const s = await page.evaluate(() => ({
          status: window.fieldsmith.analysis.state.status,
          error: window.fieldsmith.analysis.state.error,
          levels: window.fieldsmith.analysis.state.levels.length,
          resultsFor: window.fieldsmith.analysis.state.resultsFor,
          dv: window.fieldsmith.store.docVersion,
        }));
        if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) break;
        if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`B: analysis error after renames: ${s.error}`);
        if (Date.now() - t0 > 120000) throw new Error(`B: ladder did not settle after renames (last status ${JSON.stringify(s)})`);
        await page.waitForTimeout(250);
      }
    }
    const afterB = await page.evaluate(() => ({
      conv: JSON.parse(JSON.stringify(window.fieldsmith.store.doc.convections[0])),
      tempRegion: window.fieldsmith.store.doc.temperatures[0]?.region ?? null,
      field: null,
    }));
    afterB.field = await page.evaluate(fieldRead);
    check(
      'B: renames rewrite the region references and the statement id, and re-solve to the same line',
      afterB.conv.id === 'air' && afterB.conv.region === 'airwall' && afterB.tempRegion === 'hotplate'
        && afterB.field.maxDev !== null && afterB.field.nodeCount > 0 && afterB.field.maxDev < 1e-3 && Math.abs(afterB.field.slope + 1.6) < 1e-4,
      `conv ${JSON.stringify(afterB.conv)}, temperature region ${afterB.tempRegion}; field ${afterB.field.nodeCount} nodes, max |Δ| ${typeof afterB.field.maxDev === 'number' ? afterB.field.maxDev.toExponential(2) : afterB.field.note}, slope ${afterB.field.slope?.toFixed?.(6) ?? afterB.field.slope}`,
    );
    await page.locator('[data-lens="results"]').click();
    await page.screenshot({ path: shot('02-ui-authored-renamed') });

    const roundB = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      return {
        line: (text.split('\n').find((l) => l.startsWith('convection ')) ?? '').trim(),
        conv: r.ok ? JSON.parse(JSON.stringify(r.ok.convections)) : null,
      };
    });
    check(
      'B: the .fsm round-trip re-prints the plain-number convection and re-parses it',
      roundB.line.startsWith('convection air airwall h=') && roundB.line.includes('tinf=20')
        && roundB.conv?.length === 1 && roundB.conv[0].id === 'air' && roundB.conv[0].region === 'airwall' && Math.abs(roundB.conv[0].h - 0.1) < 1e-12,
      JSON.stringify(roundB),
    );

    // ---- C — wholly convecting, no temperature region --------------------------------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_C);
    {
      const t0 = Date.now();
      for (;;) {
        const s = await page.evaluate(() => ({
          status: window.fieldsmith.analysis.state.status,
          error: window.fieldsmith.analysis.state.error,
          levels: window.fieldsmith.analysis.state.levels.length,
          resultsFor: window.fieldsmith.analysis.state.resultsFor,
          dv: window.fieldsmith.store.docVersion,
        }));
        if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) break;
        if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`C: analysis error (convection-only): ${s.error}`);
        if (Date.now() - t0 > 120000) throw new Error(`C: convection-only run did not settle (last status ${JSON.stringify(s)})`);
        await page.waitForTimeout(250);
      }
    }
    const setC = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
      const t = ci >= 0 ? lvl.temperature[ci] : null;
      if (!t || !t.length) return { nodes: 0, min: null, max: null, maxDev: null };
      let min = Infinity, max = -Infinity, maxDev = 0;
      for (const v of t) {
        if (v < min) min = v;
        if (v > max) max = v;
        const d = Math.abs(v - 20);
        if (d > maxDev) maxDev = d;
      }
      return { nodes: t.length, min, max, maxDev };
    });
    await page.screenshot({ path: shot('03-wholly-convecting') });
    check(
      'C: convection alone lifts the constant mode — the study solves and every node settles to t_inf',
      setC.maxDev !== null && setC.nodes > 0 && setC.maxDev < 1e-2,
      `${setC.nodes} nodes settle to [${setC.min?.toFixed?.(3)}, ${setC.max?.toFixed?.(3)}] °C, max |T − 20| ${typeof setC.maxDev === 'number' ? setC.maxDev.toExponential(2) : '—'} (the run itself is the setup-gate evidence: the worker admits a convection-only study)`,
    );

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V29 — Robin convection (the `convection` statement)',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'A band of the surface sheds heat at h·(T − t_inf) per unit area — the Robin boundary.',
      'The solver pins the solid conductivity at k = 1 W/(mm·K), so h reads in W/(mm²·K)',
      '(5 W/(m²·K) natural convection in air = 5e-6 here). The fixtures below are the same',
      '40 × 10 × 10 bar: a 100 °C Dirichlet patch on x = 0, the convection band on the +x end',
      'face. The exact steady field is the 1D line T(x) = 100 − h·(T0 − t_inf)/(k + h·L)·x;',
      'at h = 0.1, t_inf = 20 that is T = 100 − 1.6·x with the band at 36 °C — the identical',
      'anchor the fieldsmith-core test pins, so the browser numbers are the native numbers.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      '## A — .fsm-authored statement with a let-driven h',
      '',
      table([
        'A: the statement parses into a stored convection with its let evaluated into h',
        'A: the run settles',
        'A: the band drives the bar onto the Robin line (T = 100 − 1.6x, band 36 °C)',
        'A: the let-driven h left a consumer record',
        'A: renaming the convection follows its consumer record',
        'A: the .fsm round-trip re-prints the let-driven h and re-parses it',
      ]),
      '',
      '## B — authored in the Setup lens, then renamed',
      '',
      'The same bar without the statement: add-convection stamps its defaults',
      '(h 5e-6, t_inf 20, case "Thermal 1") on the selected region, then h is typed at the',
      'inspector’s number input. Both regions and the convection id are then renamed through',
      'the inspector’s name field — every reference follows.',
      '',
      table([
        'B: add-convection stamps the defaults on the selected region',
        'B: the inspector-set h drives the same Robin line',
        'B: renames rewrite the region references and the statement id, and re-solve to the same line',
        'B: the .fsm round-trip re-prints the plain-number convection and re-parses it',
      ]),
      '',
      '## C — wholly convecting, no fixed temperature',
      '',
      'The temperature patch is removed; only the convection remains. The worker’s setup gate',
      'admits a run on convection alone because h > 0 raises the Laplacian’s constant mode,',
      'and the steady field collapses to t_inf everywhere.',
      '',
      '```fsm',
      DOC_C.trimEnd(),
      '```',
      '',
      table([
        'C: convection alone lifts the constant mode — the study solves and every node settles to t_inf',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/convection.mjs`, driven by `node tests/run-e2e.mjs --only convection`. The',
      'ladder runs in the page against the same fieldsmith-core solver the CLI uses. The',
      'field evidence is nodal: P2 nodes carry T − (100 − 1.6·x) at each node’s own x. The',
      'renames and the h edit drive the shipped inspector inputs (`name`, NumberInput Enter',
      'commit), not direct store calls; the round-trips run the shipped .fsm printer/parser.',
      '',
      '## Limits',
      '',
      '- h is per the pinned unit conductivity; the UI title carries the W/(m²·K) conversion',
      '  (divide by 1e6 in mm-units). The settled study’s mechanical slots are null — no loads.',
      '- Sweep parameterisation over h/t_inf and .fsmz sidecar round-trips of convections are',
      '  not asserted here; the fixtures keep a single, static statement.',
      '- The wholly-insulated/wholly-negative refusal is the solver’s own solvability gate',
      '  (a wholly insulated body has no steady solution; conduction_names_the_unsolvable_case',
      '  covers it) — the browser fixtures and the settled study all carry h > 0.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/convection.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V29.md'), lines.join('\n'));
    console.log(`convection ${done ? 'PASS' : 'FAIL'}`);
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