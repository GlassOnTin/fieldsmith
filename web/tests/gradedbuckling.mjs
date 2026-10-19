// The fill re-solve's buckling-utilization factor, surfaced in the Results lens
// (VISION §7 item 2, "buckling/yield indicators for thin TPMS walls" — the buckling
// half): `Graded.buckling_utilization` is the max over LATTICE elements of the element's
// compressive principal magnitude over the elastic-buckling law's stress (the
// Gibson–Ashby open-cell collapse law 0.05·E_s·φ², banded and clamped at the band top —
// literature-cited constants, not fitted; a linear-elastic solve cannot measure
// buckling, so the geometric-stiffness eigenproblem remains the rigorous route owed).
// This family checks the plumbing: the raw wasm meta's per-case factors against a
// Node-side re-computation from the raw getters, the worker's thermal-slot alignment,
// and the summary row's readout.
//
// The native anchors are fieldsmith-solve's `principal_reads_the_known_spectra`
// (the closed-form principal readout) and fieldsmith-core's
// `graded_fill_on_a_mesh_document_is_measured` (the factor present, and yield binding
// first at these densities). This e2e pins the bridge, not the law.
//
// Run with node tests/run-e2e.mjs --only gradedbuckling.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const DOC_A = `\
fieldsmith 1
name "Graded buckling bar"
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

// The buckling law the engine applies (constants mirrored from fill.rs — BUCKLING_COEF /
// BUCKLING_EXP, the band floor RHO_LO = 0.18 and the band-top clamp at RHO_HI = 0.40).
// A test-side copy by design: the native unit gate owns the law.
const BUCKLING_COEF = 0.05;
const RHO_LO = 0.18;
const RHO_HI = 0.4;

function bucklingExtended(e, phi) {
  if (phi <= 0) return 0;
  if (phi < RHO_LO) return e * BUCKLING_COEF * Math.pow(RHO_LO, 2.0) * (phi / RHO_LO);
  return e * BUCKLING_COEF * Math.pow(Math.min(phi, RHO_HI), 2.0);
}

/** Max over LATTICE elements (class === 1) of the compressive magnitude over the law's
 * stress — the factor's engine-side definition. */
function bucklingFactor(smin, rho, cls, eYoung) {
  let worst = 0;
  for (let el = 0; el < smin.length; el++) {
    if (cls[el] !== 1) continue;
    const sc = -smin[el];
    if (sc <= 0) continue;
    const f = sc / bucklingExtended(eYoung, rho[el]);
    if (f > worst) worst = f;
  }
  return worst;
}

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
        reported: meta.buckling_utilization ?? null,
        caseNames: meta.cases.map((c) => c.name),
        smin0: Array.from(graded.sigmaMinElem(0)),
        rho: Array.from(graded.rhoElem()),
        cls: Array.from(graded.class()),
      };
    } finally {
      graded.free();
    }
  }, fsm);
}

export async function gradedbucklingTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `gradedbuckling-${name}.png`);
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

    const want = bucklingFactor(h.smin0, h.rho, h.cls, 1500.0);
    const got = h.reported?.[0];
    const rel = Math.abs(got - want) / Math.max(1e-12, want);
    check(
      'A: the wasm meta’s buckling factor matches a re-computation from the raw getters',
      h.reported !== null && h.reported.length === h.caseNames.length && got > 0 && rel < 1e-9,
      `reported ${got} vs re-computed ${want} (rel ${rel.toExponential(2)}), ` +
        `cases [${h.caseNames.join(', ')}], ${h.smin0.length} elements`,
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
        const m = row.match(new RegExp(`${name}[^·]*(?:·[^\\n]*)*?· buckling ×(\\d+(?:\\.\\d+)?)`));
        return m ? parseFloat(m[1]) : null;
      };
      return {
        names: lvl.meta.cases.map((c) => c.name),
        bt: lvl.meta.buckling_utilization ?? null,
        loadRow: at('Load 1'),
        thermalRowHasBuckling: /· buckling/.test(row.split('Thermal 1')[1] ?? ''),
      };
    });
    await page.screenshot({ path: shot('02-results-summary') });

    const packed = lv.bt;
    check(
      'B: the packed level aligns the factors with the merged case list (mechanical value, then null per thermal slot)',
      packed !== null && packed.length === lv.names.length && packed[0] > 0 && packed[1] === null,
      `packed [${packed?.map((x) => (x === null ? 'null' : x.toFixed(4))).join(', ')}] for cases [${lv.names.join(', ')}]`,
    );

    check(
      'B: the summary row reads the buckling factor next to the case maxima',
      lv.loadRow !== null && Math.abs(lv.loadRow - Math.round(packed[0] * 100) / 100) < 5e-3,
      `row shows buckling ×${lv.loadRow}, packed ${packed[0]?.toFixed(4)} (shown at 2 decimals)`,
    );

    check(
      'B: the thermal slot carries no buckling factor in its row',
      lv.thermalRowHasBuckling === false,
      `the 'Thermal 1' row text ${lv.thermalRowHasBuckling ? 'shows' : 'lacks'} a buckling segment`,
    );

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V38 — The Results lens reads the fill\'s buckling-utilization factor',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'The buckling half of §7 item 2\'s indicator bullet: the graded re-solve reports',
      'one buckling-utilization factor per mechanical case — the max over LATTICE',
      'elements of the element\'s compressive principal magnitude over the buckling',
      'law\'s stress (`buckling_extended`, the Gibson–Ashby open-cell collapse law',
      '0.05·E_s·φ² with the band ramp and the band-top clamp; literature-cited',
      'constants, not fitted). Tension-only elements contribute nothing; lattice',
      'elements only (the solid skin\'s check is the yield factor\'s). The wasm meta',
      'carries the array, the worker keeps it index-aligned with the merged case slots',
      '(thermal slots append `null`), and the summary row reads `· buckling ×<factor>`.',
      '',
      'The native anchors are fieldsmith-solve\'s `principal_reads_the_known_spectra`',
      '(the closed-form principal readout, identities pinned) and fieldsmith-core\'s',
      '`graded_fill_on_a_mesh_document_is_measured` (the factor present; yield binding',
      'first at these densities). This family pins the bridge: the raw meta against a',
      'Node-side re-computation from the raw getters (`sigmaMinElem(0)`, `rhoElem()`,',
      '`class()`), the packed level\'s alignment, and the row readout.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      table([
        'A: the wasm meta’s buckling factor matches a re-computation from the raw getters',
        'B: the packed level aligns the factors with the merged case list (mechanical value, then null per thermal slot)',
        'B: the summary row reads the buckling factor next to the case maxima',
        'B: the thermal slot carries no buckling factor in its row',
        'no console errors',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/gradedbuckling.mjs`, driven by `node tests/run-e2e.mjs --only',
      'gradedbuckling`. Part A calls the wasm handle directly (parse + `analyseGraded`',
      'at h = 8) and re-computes the factor in Node from the raw field arrays with the',
      'mirrored law constants; part B drives the real ladder (parse → load → Results',
      'lens) and asserts the packed meta and the rendered summary row.',
      '',
      '## Limits',
      '',
      '- The factor\'s magnitude is fixture- and law-dependent; this family asserts the',
      '  plumbing, not a design claim. The constants\' provenance (literature-cited',
      '  Gibson–Ashby open-cell collapse) is stated in fill.rs at the definition.',
      '- The law applies only to lattice elements in and below the measured band; above',
      '  the band the denominator clamps at the band-top value — conservative in the',
      '  factor (it reads high), the opposite of skipping those elements.',
      '- A linear-elastic solve cannot measure buckling: these factors are the',
      '  literature-law ESTIMATE. The rigorous route — the geometric-stiffness',
      '  eigenproblem on the solved stress field — is tracked on the roadmap.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/gradedbuckling.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V38.md'), lines.join('\n'));
    console.log(`gradedbuckling ${done ? 'PASS' : 'FAIL'}`);
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
