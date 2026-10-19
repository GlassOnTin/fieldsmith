// The fill re-solve's strength-utilization factor, surfaced in the Results lens
// (VISION §7 item 2, "buckling/yield indicators for thin TPMS walls" — the yield half):
// `Graded.yield_utilization` is the max over NON-VOID elements of the element von Mises
// divided by the fraction-scaled yield (the Gibson–Ashby open-cell strength law,
// `strength_extended` in fill.rs — literature-cited constants, not fitted). This family
// checks the whole plumbing: the raw wasm meta's per-case factors, a re-computation from
// the raw getters, the index alignment the worker maintains when thermal slots are
// appended (mechanical values, then null per thermal slot), and the summary row's
// readout.
//
// The native anchor is fieldsmith-core's `graded_fill_on_a_mesh_document_is_measured`
// (fill_mesh.rs), which pins the factor's ordering — every non-void element has φ ≤ 1
// so its law yield ≤ the solid yield, and the factor therefore dominates
// peak_vm/solid_yield — plus the no-yield contract (absent without a yield-stating
// material). This e2e pins the bridge, not the law.
//
// Run with node tests/run-e2e.mjs --only gradedyield.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const DOC_A = `\
fieldsmith 1
name "Graded yield bar"
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

// The strength law the engine applies to the fraction (constants mirrored from
// fill.rs — STRENGTH_COEF / STRENGTH_EXP / the [RHO_LO, RHO_HI] band with its
// extensions). A test-side copy by design: the native unit gate owns the law.
const STRENGTH_COEF = 0.3;
const STRENGTH_EXP = 1.5;
const RHO_LO = 0.18;
const RHO_HI = 0.4;

function strengthExtended(y, phi) {
  if (phi <= 0) return 0;
  if (phi < RHO_LO) return y * STRENGTH_COEF * Math.pow(RHO_LO, STRENGTH_EXP) * (phi / RHO_LO);
  if (phi <= RHO_HI) return y * STRENGTH_COEF * Math.pow(phi, STRENGTH_EXP);
  const sHi = y * STRENGTH_COEF * Math.pow(RHO_HI, STRENGTH_EXP);
  return sHi + ((phi - RHO_HI) / (1 - RHO_HI)) * (y - sHi);
}

/** Max over non-void elements of von Mises over the fraction-scaled yield
 * (CLASS_VOID = 2, fill.rs). The factor's engine-side definition. */
function utilizationFactor(vm, rho, cls, y) {
  let worst = 0;
  for (let e = 0; e < vm.length; e++) {
    if (cls[e] === 2) continue;
    const f = vm[e] / strengthExtended(y, rho[e]);
    if (f > worst) worst = f;
  }
  return worst;
}

/** The raw wasm handle's inputs and reported factor (plain data through serialization:
 * the vm/rho/class arrays come back full-length for the Node-side re-computation). */
async function readHandle(page, fsm) {
  return page.evaluate(async (fsmSource) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(fsmSource);
    if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
    const doc = JSON.parse(JSON.stringify(r.ok));
    const graded = w.analyseGraded(doc, { h: 8 });
    try {
      const meta = JSON.parse(graded.metaJson());
      return {
        yieldStrength: meta.yield_strength,
        reported: meta.yield_utilization ?? null,
        caseNames: meta.cases.map((c) => c.name),
        vm0: Array.from(graded.vmElem(0)),
        rho: Array.from(graded.rhoElem()),
        cls: Array.from(graded.class()),
      };
    } finally {
      graded.free();
    }
  }, fsm);
}

export async function gradedyieldTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `gradedyield-${name}.png`);
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

    // ---- A — the raw wasm meta and a re-computation from the raw getters ----------------
    const h = await readHandle(page, DOC_A);
    await page.screenshot({ path: shot('01-wasm-handle') });

    const want = utilizationFactor(h.vm0, h.rho, h.cls, h.yieldStrength);
    const got = h.reported?.[0];
    const rel = Math.abs(got - want) / Math.max(1e-12, want);
    check(
      'A: the wasm meta’s utilization factor matches a re-computation from the raw getters',
      h.reported !== null && h.reported.length === h.caseNames.length && got > 0 && rel < 1e-9,
      `reported ${got} vs re-computed ${want} (rel ${rel.toExponential(2)}), ` +
        `cases [${h.caseNames.join(', ')}], ${h.vm0.length} elements, ${h.rho.length} fractions`,
    );

    // ---- B — the packed level: alignment through the worker's thermal merge -------------
    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('results');
    }, DOC_A);
    await awaitLadder(page, 'B');

    const lv = await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.latest;
      const row = document.querySelector('[data-testid="results-summary"]')?.textContent ?? '';
      const at = (name) => {
        const m = row.match(new RegExp(`${name}[^·]*(?:·[^\\n]*)*?· yield ×(\\d+(?:\\.\\d+)?)`));
        return m ? parseFloat(m[1]) : null;
      };
      return {
        names: lvl.meta.cases.map((c) => c.name),
        ut: lvl.meta.yield_utilization ?? null,
        loadRow: at('Load 1'),
        // The thermal row is the summary's last row, so everything after its name is
        // exactly its remaining text: a yield segment must be absent there.
        thermalRowHasYield: /· yield/.test(row.split('Thermal 1')[1] ?? ''),
      };
    });
    await page.screenshot({ path: shot('02-results-summary') });

    const packed = lv.ut;
    // The packed level runs the ladder's own resolution (the fixture's rung), so the
    // factor's VALUE is not compared across the two runs — the packing is value-preserving
    // by construction and the row readout (next check) binds it to the same level.
    check(
      'B: the packed level aligns the factors with the merged case list (mechanical value, then null per thermal slot)',
      packed !== null && packed.length === lv.names.length && packed[0] > 0 && packed[1] === null,
      `packed [${packed?.map((x) => (x === null ? 'null' : x.toFixed(4))).join(', ')}] for cases [${lv.names.join(', ')}]`,
    );

    check(
      'B: the summary row reads the factor next to the case maxima',
      lv.loadRow !== null && Math.abs(lv.loadRow - Math.round(packed[0] * 100) / 100) < 5e-3,
      `row shows yield ×${lv.loadRow}, packed ${packed[0]?.toFixed(4)} (shown at 2 decimals)`,
    );

    check(
      'B: the thermal slot carries no factor in its row',
      lv.thermalRowHasYield === false,
      `the 'Thermal 1' row text ${lv.thermalRowHasYield ? 'shows' : 'lacks'} a yield segment`,
    );

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V37 — The Results lens reads the fill\'s yield-utilization factor',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'The yield half of §7 item 2\'s indicator bullet: with a yield-stating material,',
      'the graded re-solve reports one strength-utilization factor per mechanical case —',
      'the max over non-void elements of the element von Mises over the fraction-scaled',
      'yield (`strength_extended`, the Gibson–Ashby open-cell law 0.3·σ_ys·φ^1.5 with the',
      'band extensions; literature-cited constants, not fitted). The wasm meta carries',
      'the array, the worker keeps it index-aligned with the merged case slots (thermal',
      'slots append `null`), and the summary row reads `· yield ×<factor>` next to the',
      'case maxima. The argmax element is the loaded wall, so values above 1 mark the',
      'over-yield regime at the current load.',
      '',
      'The native anchors are fieldsmith-core\'s `graded_fill_on_a_mesh_document_is_measured`',
      '(the factor dominates peak_vm/solid_yield exactly, φ ≤ 1 ⇒ law yield ≤ solid yield)',
      'and the no-yield contract (entries absent). This family pins the bridge: the raw',
      'meta against a Node-side re-computation from the raw getters (`vmElem(0)`,',
      '`rhoElem()`, `class()`), the packed level\'s alignment through the worker merge,',
      'and the row readout.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      table([
        'A: the wasm meta’s utilization factor matches a re-computation from the raw getters',
        'B: the packed level aligns the factors with the merged case list (mechanical value, then null per thermal slot)',
        'B: the summary row reads the factor next to the case maxima',
        'B: the thermal slot carries no factor in its row',
        'no console errors',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/gradedyield.mjs`, driven by `node tests/run-e2e.mjs --only gradedyield`.',
      'Part A calls the wasm handle directly (parse + `analyseGraded` at h = 8) and',
      're-computes the factor in Node from the raw field arrays with the mirrored law',
      'constants; part B drives the real ladder (parse → load → Results lens) and asserts',
      'the packed meta and the rendered summary row.',
      '',
      '## Limits',
      '',
      '- The factor\'s magnitudes are fixture- and law-dependent; this family asserts the',
      '  plumbing (alignment, re-computation, display), not a design claim about the',
      '  fixture. The law constants\' provenance (literature-cited Gibson–Ashby) is',
      '  stated in fill.rs at the definition.',
      '- Only petg (yield 47 MPa) is exercised: the absent-factor contract is the native',
      '  unit gate\'s job (`fill_mesh.rs`), not the e2e\'s.',
      '- The thermal slot\'s `null` is the worker\'s own appends (one per thermal case);',
      '  more thermal cases than the fixture\'s single one are untested here.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/gradedyield.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V37.md'), lines.join('\n'));
    console.log(`gradedyield ${done ? 'PASS' : 'FAIL'}`);
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