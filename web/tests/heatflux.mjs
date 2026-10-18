// The `heatflux` statement (VISION §7 item 3, the Neumann boundary): a band of the
// surface receives q per unit area into the conduction solve, read against the solver's
// pinned solid conductivity k = 1 W/(mm·K), so q is in W/mm² (1 kW/m² = 1e-3). The
// fixture is the 40 × 10 × 10 convection bar with a 100 °C Dirichlet patch on x = 0 and
// a flux band on the +x end face: the steady field is the same 1D line, run the other
// way through the ends
//
//   T(x) = 100 + q/k·x = 100 + 2·x   (q = 2, k = 1)
//
// with the band reading 180 °C. That is the exact anchor the fieldsmith-solve and
// fieldsmith-core tests pin (heatflux_bar_matches_the_analytic_profile /
// heatflux_band_drives_the_thermal_case), so the browser run lands on the native
// numbers. Four fixtures:
//
//   A — .fsm-authored: the statement with a let-driven q parses into the store, solves to
//       the line, and the .fsm round-trip re-prints the consumer expression verbatim
//       (q=2 * qv); a rename follows the recorded consumer.
//   B — Setup-lens authored: add-heatflux on the selected region (q 1e-3, case
//       "Thermal 1" defaults), q through the inspector's number input; renaming both
//       regions and the flux id (through the shipped name field) rewrites every
//       reference and re-solves to the same line; another .fsm round-trip.
//   C — flux on the fixed face only: a heat-flux band contributes no matrix entry and its
//       band rows are Dirichlet-lifted to zero, so the solve must be bit-identical to the
//       same bar with no flux statement at all — every temperature value and node equal.
//   D — a flux-only study: no fixed temperature, no convection, q alone does not fix the
//       field's level, so the ladder refuses with the solver's named gate.
//
// Run with node tests/run-e2e.mjs --only heatflux.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const BAR = (extra) => `\
fieldsmith 1
name "Heat-flux bar"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hot = beam.nx
region cool = beam.px
support clamp hot fixed=xyz
temperature fixer hot temp=100 case="Thermal 1"
${extra}\
`;

// A: q reads a let (the consumer record matters below); C's flux variant puts the band on
// the Dirichlet face itself — every band node is fixed there, so the flux must not move
// the field.
const DOC_A = BAR(`let qv = 1\nheatflux lead cool q=2 * qv case="Thermal 1"\nsolve size=8 tref=20\n`);
const DOC_B = BAR(`solve size=8 tref=20\n`);
const DOC_C = BAR(`heatflux intofixed hot q=2 case="Thermal 1"\nsolve size=8 tref=20\n`);
const DOC_D = `\
fieldsmith 1
name "Flux-only"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hold = beam.nx
region band = beam.px
support clamp hold fixed=xyz
heatflux lead band q=2 case="Thermal 1"
solve size=8
`;

/** Nodal read of the thermal case 'Thermal 1': line residuals (T − (100 + 2·x) at each
 * node's own x, from the same buffers), the line's measured slope from the hottest node,
 * and the band's anchor value. `note` carries what was missing when the run produced no
 * nodal field. */
function fieldRead() {
  const lvl = window.fieldsmith.analysis.latest;
  const out = { nodeCount: (lvl.nodes?.length ?? 0) / 3, slope: null, bandC: null, maxDev: null, note: '' };
  const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
  if (ci < 0) return { ...out, note: `no 'Thermal 1' case (cases: ${lvl.meta.cases.map((c) => c.name).join(', ')})` };
  const t = lvl.temperature[ci];
  if (!t) return { ...out, note: 'the thermal case carries no nodal field' };
  let max = -Infinity, xAtMax = NaN, maxDev = 0;
  for (let n = 0; n < out.nodeCount; n++) {
    const x = lvl.nodes[3 * n], v = t[n];
    if (v > max) { max = v; xAtMax = x; }
    const dev = Math.abs(v - (100 + 2 * x));
    if (dev > maxDev) maxDev = dev;
  }
  return { ...out, slope: (max - 100) / xAtMax, bandC: max, maxDev };
}

/** Settle loop: resolves when the ladder is ready (or 'done'-with-skipped) on the current
 * docVersion; throws on the worker's error post. Runs on the caller's page. */
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

export async function heatfluxTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `heatflux-${name}.png`);
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

    // ---- A — .fsm authored, let-driven q ------------------------------------------------
    const docA = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      return JSON.parse(JSON.stringify(window.fieldsmith.store.doc.heat_fluxes));
    }, DOC_A);
    check(
      'A: the statement parses into a stored heat flux with its let evaluated into q',
      docA.length === 1 && docA[0].id === 'lead' && docA[0].region === 'cool' && Math.abs(docA[0].q - 2) < 1e-12 && docA[0].case === 'Thermal 1',
      JSON.stringify(docA),
    );
    await page.locator('[data-lens="results"]').click();
    await awaitLadder(page, 'A');

    const fieldA = await page.evaluate(fieldRead);
    check(
      'A: the band drives the bar onto the Neumann line (T = 100 + 2x, band 180 °C)',
      fieldA.maxDev !== null && fieldA.nodeCount > 0 && fieldA.maxDev < 1e-3 && Math.abs(fieldA.slope - 2) < 1e-4 && Math.abs(fieldA.bandC - 180) < 1e-3,
      `${fieldA.nodeCount} nodes, max |Δ| ${typeof fieldA.maxDev === 'number' ? fieldA.maxDev.toExponential(2) : fieldA.note}, slope ${fieldA.slope?.toFixed?.(6) ?? fieldA.slope}, band ${fieldA.bandC?.toFixed?.(3) ?? fieldA.bandC} °C; a fixed-temperature-at-band model would read 20 °C and slope −5`,
    );
    // Consumer record: q reads let qv, so one `consumed` entry targets the heat flux.
    const consumedA = await page.evaluate(() => (window.fieldsmith.store.doc.consumed ?? []).filter((c) => c.target === 'lead'));
    check(
      'A: the let-driven q left a consumer record',
      consumedA.length === 1 && consumedA[0].attr === 'q' && consumedA[0].comp === null && consumedA[0].expr === '2 * qv',
      JSON.stringify(consumedA),
    );
    await page.screenshot({ path: shot('01-fsm-authored-field') });

    const renamedA = await page.evaluate(async () => {
      const ops = await import('/src/doc/ops.ts');
      let err = null;
      window.fieldsmith.store.transact('Rename lead to inflow', (d) => {
        err = ops.renameId(d, 'lead', 'inflow');
      });
      return {
        err,
        id: window.fieldsmith.store.doc.heat_fluxes[0]?.id ?? null,
        target: (window.fieldsmith.store.doc.consumed ?? []).find((c) => c.expr === '2 * qv')?.target ?? null,
      };
    });
    check(
      'A: renaming the heat flux follows its consumer record',
      renamedA.err === null && renamedA.id === 'inflow' && renamedA.target === 'inflow',
      JSON.stringify(renamedA),
    );

    const roundA = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      return {
        line: (text.split('\n').find((l) => l.startsWith('heatflux ')) ?? '').trim(),
        flux: r.ok ? JSON.parse(JSON.stringify(r.ok.heat_fluxes)) : null,
      };
    });
    check(
      'A: the .fsm round-trip re-prints the let-driven q and re-parses it',
      roundA.line === 'heatflux inflow cool q=2 * qv case="Thermal 1"'
        && roundA.flux?.length === 1 && roundA.flux[0].id === 'inflow' && Math.abs(roundA.flux[0].q - 2) < 1e-12 && roundA.flux[0].case === 'Thermal 1',
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
    await page.locator('[data-testid="add-heatflux"]').click();
    const addedB = await page.evaluate(() => JSON.parse(JSON.stringify(window.fieldsmith.store.doc.heat_fluxes)));
    check(
      'B: add-heatflux stamps the defaults on the selected region',
      addedB.length === 1 && addedB[0].id === 'heatflux1' && addedB[0].region === 'cool' && Math.abs(addedB[0].q - 1e-3) < 1e-15 && addedB[0].case === 'Thermal 1',
      JSON.stringify(addedB),
    );
    // The inspector came up with the new item selected (addAnalysis selects it).
    const numEl = page.locator('[data-testid="heatflux-q"]');
    await numEl.click();
    await numEl.fill('2');
    await numEl.press('Enter');
    await awaitLadder(page, 'B');
    const fieldB1 = await page.evaluate(fieldRead);
    check(
      'B: the inspector-set q drives the same Neumann line',
      fieldB1.maxDev !== null && fieldB1.nodeCount > 0 && fieldB1.maxDev < 1e-3 && Math.abs(fieldB1.slope - 2) < 1e-4 && Math.abs(fieldB1.bandC - 180) < 1e-3,
      `${fieldB1.nodeCount} nodes, max |Δ| ${typeof fieldB1.maxDev === 'number' ? fieldB1.maxDev.toExponential(2) : fieldB1.note}, slope ${fieldB1.slope?.toFixed?.(6) ?? fieldB1.slope}, band ${fieldB1.bandC?.toFixed?.(3) ?? fieldB1.bandC} °C`,
    );

    // Rename the two regions and the flux id through the shipped name field; every
    // reference moves with it — the flux's region, the temperature's region and the
    // statement's own id.
    const renameWith = async (to) => {
      const el = page.locator('[data-testid="name"]');
      await el.click();
      await el.fill(to);
      await el.press('Enter');
      await page.waitForTimeout(100);
    };
    await page.locator('[data-testid="region-cool"]').click();
    await renameWith('coldface');
    await page.locator('[data-testid="region-hot"]').click();
    await renameWith('hotplate');
    await page.locator('[data-testid="heatflux-heatflux1"]').click();
    await renameWith('inflow');
    await awaitLadder(page, 'B-renames');
    const afterB = await page.evaluate(() => ({
      flux: JSON.parse(JSON.stringify(window.fieldsmith.store.doc.heat_fluxes[0])),
      tempRegion: window.fieldsmith.store.doc.temperatures[0]?.region ?? null,
      field: null,
    }));
    afterB.field = await page.evaluate(fieldRead);
    check(
      'B: renames rewrite the region references and the statement id, and re-solve to the same line',
      afterB.flux.id === 'inflow' && afterB.flux.region === 'coldface' && afterB.tempRegion === 'hotplate'
        && afterB.field.maxDev !== null && afterB.field.nodeCount > 0 && afterB.field.maxDev < 1e-3 && Math.abs(afterB.field.slope - 2) < 1e-4,
      `flux ${JSON.stringify(afterB.flux)}, temperature region ${afterB.tempRegion}; field ${afterB.field.nodeCount} nodes, max |Δ| ${typeof afterB.field.maxDev === 'number' ? afterB.field.maxDev.toExponential(2) : afterB.field.note}, slope ${afterB.field.slope?.toFixed?.(6) ?? afterB.field.slope}`,
    );
    await page.locator('[data-lens="results"]').click();
    await page.screenshot({ path: shot('02-ui-authored-renamed') });

    const roundB = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      return {
        line: (text.split('\n').find((l) => l.startsWith('heatflux ')) ?? '').trim(),
        flux: r.ok ? JSON.parse(JSON.stringify(r.ok.heat_fluxes)) : null,
      };
    });
    check(
      'B: the .fsm round-trip re-prints the plain-number flux and re-parses it',
      roundB.line.startsWith('heatflux inflow coldface q=') && roundB.line.includes('case="Thermal 1"')
        && roundB.flux?.length === 1 && roundB.flux[0].id === 'inflow' && roundB.flux[0].region === 'coldface' && Math.abs(roundB.flux[0].q - 2) < 1e-15,
      JSON.stringify(roundB),
    );

    // ---- C — flux on the fixed face only: bit-identical to no flux at all ------------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_B);
    await awaitLadder(page, 'C-baseline');
    const baseC = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
      return { nodes: Array.from(lvl.nodes), t: Array.from(lvl.temperature[ci] ?? []) };
    });
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_C);
    await awaitLadder(page, 'C-flux');
    const fluxC = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const ci = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
      return { nodes: Array.from(lvl.nodes), t: Array.from(lvl.temperature[ci] ?? []) };
    });
    await page.screenshot({ path: shot('03-fixed-face-flux') });
    const nodesDiff = baseC.nodes.filter((v, i) => v !== fluxC.nodes[i]).length;
    const tDiff = baseC.t.filter((v, i) => v !== fluxC.t[i]).length;
    check(
      'C: a heat flux on the fixed face changes the solve by nothing — every value identical',
      baseC.t.length > 0 && fluxC.t.length === baseC.t.length && nodesDiff === 0 && tDiff === 0,
      `${baseC.t.length} nodes; node-array differences ${nodesDiff}, temperature differences ${tDiff} (the flux has no matrix entry and its band rows are lifted, so the run is expected to be bit-identical)`,
    );

    // ---- D — a flux-only study: the named gate -------------------------------------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_D);
    {
      const t0 = Date.now();
      for (;;) {
        const s = await page.evaluate(() => ({
          status: window.fieldsmith.analysis.state.status,
          error: window.fieldsmith.analysis.state.error,
          resultsFor: window.fieldsmith.analysis.state.resultsFor,
          dv: window.fieldsmith.store.docVersion,
        }));
        if (s.status === 'error' && s.resultsFor === s.dv && s.error.includes("a heat flux q alone does not fix the field's level")) break;
        if (s.status === 'ready' && s.resultsFor === s.dv) throw new Error(`D: the flux-only study unexpectedly solved (status ready, ${s.error ?? 'no error'})`);
        if (Date.now() - t0 > 120000) throw new Error(`D: the gate never arrived (last status ${JSON.stringify(s)})`);
        await page.waitForTimeout(250);
      }
      const msg = await page.evaluate(() => window.fieldsmith.analysis.state.error);
      await page.screenshot({ path: shot('04-flux-only-refusal') });
      check(
        'D: a flux-only study refuses with the solver’s named gate text',
        msg.includes("a heat flux q alone does not fix the field's level") && msg.includes('place at least one temperature region or a convection'),
        msg,
      );
    }

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V30 — Heat flux (the `heatflux` statement)',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'A band of the surface receives q per unit area into the conduction solve — the Neumann',
      'boundary. The solver pins the solid conductivity at k = 1 W/(mm·K), so q reads in W/mm²',
      '(1 kW/m² = 1e-3 here; positive is into the solid). The weak form contributes only to the',
      'right-hand side: the band rows take q·∫φ dΓ, which a uniform q lands on the three midsides',
      'at A/3 each and conserves q·A. The fixtures below are the same 40 × 10 × 10 bar as the',
      'Robin family: a 100 °C Dirichlet patch on x = 0, the flux band on the +x end face. The',
      'exact steady field is the 1D line T(x) = 100 + q/k·x; at q = 2 that is T = 100 + 2·x with',
      'the band at 180 °C — the identical anchor the fieldsmith-solve and fieldsmith-core tests',
      'pin, so the browser numbers are the native numbers.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      '## A — .fsm-authored statement with a let-driven q',
      '',
      table([
        'A: the statement parses into a stored heat flux with its let evaluated into q',
        'A: the band drives the bar onto the Neumann line (T = 100 + 2x, band 180 °C)',
        'A: the let-driven q left a consumer record',
        'A: renaming the heat flux follows its consumer record',
        'A: the .fsm round-trip re-prints the let-driven q and re-parses it',
      ]),
      '',
      '## B — authored in the Setup lens, then renamed',
      '',
      'The same bar without the statement: add-heatflux stamps its defaults',
      '(q 1e-3, case "Thermal 1") on the selected region, then q is typed at the',
      'inspector’s number input. Both regions and the flux id are then renamed through',
      'the inspector’s name field — every reference follows.',
      '',
      table([
        'B: add-heatflux stamps the defaults on the selected region',
        'B: the inspector-set q drives the same Neumann line',
        'B: renames rewrite the region references and the statement id, and re-solve to the same line',
        'B: the .fsm round-trip re-prints the plain-number flux and re-parses it',
      ]),
      '',
      '## C — flux on the fixed face only',
      '',
      'The band is moved onto the Dirichlet face. The statement contributes no matrix entry,',
      'and its band rows are lifted to zero with the right-hand side, so the solve is expected',
      'to be bit-identical to the same bar with no flux statement — the comparison is exact',
      'element equality, not a tolerance.',
      '',
      '```fsm',
      DOC_C.trimEnd(),
      '```',
      '',
      table([
        'C: a heat flux on the fixed face changes the solve by nothing — every value identical',
      ]),
      '',
      '## D — flux-only study',
      '',
      'The temperature patch is removed; only the flux remains. q contributes nothing to the',
      'matrix, so the Laplacian’s constant mode stays and the study has no steady solution:',
      'the ladder refuses with the solver’s named gate rather than a bare code.',
      '',
      '```fsm',
      DOC_D.trimEnd(),
      '```',
      '',
      table([
        'D: a flux-only study refuses with the solver’s named gate text',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/heatflux.mjs`, driven by `node tests/run-e2e.mjs --only heatflux`. The',
      'ladder runs in the page against the same fieldsmith-solve solver the CLI uses. The',
      'field evidence is nodal: P2 nodes carry T − (100 + 2·x) at each node’s own x. The',
      'renames and the q edit drive the shipped inspector inputs (`name`, NumberInput Enter',
      'commit), not direct store calls; the round-trips run the shipped .fsm printer/parser.',
      '',
      '## Limits',
      '',
      '- q is per the pinned unit conductivity; the UI title carries the W/(m²·K) conversion',
      '  (1 kW/m² = 1e-3 in these units). The settled study’s mechanical slots are null — no',
      '  loads.',
      '- Outward flux (negative q) is valid but not exercised in the browser fixtures; the',
      '  fieldsmith-doc round-trip test covers a q=-0.5 statement.',
      '- Sweep parameterisation over q and .fsmz sidecar round-trips of heat fluxes are',
      '  not asserted here; the fixtures keep a single, static statement.',
      '- The solvability gate behind D is the solver’s own; the native variants live in',
      '  `flux_bands_refuse_a_level_free_study` (fieldsmith-solve). The browser fixture',
      '  asserts the named text, not the internal error shape.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/heatflux.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V30.md'), lines.join('\n'));
    console.log(`heatflux ${done ? 'PASS' : 'FAIL'}`);
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