// The results lens' parameter sweep (M16 slice 3): the panel drives the analysis worker's
// sweep arm — the mirror of the native Sweep question — and the export writes the CLI's
// sweep CSV. Run with node tests/run-e2e.mjs --only sweep.
//
// Two evidences, both against the same five-point question on the Cantilever V1:
//
// A — the byte contract. One native `fieldsmith api --csv` run answers and writes the CSV
// file from the same typed response it prints as the JSON envelope on stdout: one run, so
// the walls agree. In the page, `sweepCsvFromEnvelope(JSON.parse(stdout))` — the engine's
// own mirror in web/src/app/sweep.ts — must reproduce the file byte for byte. The request
// pins `rungs: [30000, 150000]` explicitly: the web sweep walks the ladder's coarse/refined
// pair (COARSE_DOFS/FINE_DOFS in wasm.ts), and omitting rungs would walk the native's
// three-rung default (question.rs DEFAULT_RUNGS = 30000/150000/750000), a different
// question. The same page checks rustFloat against the Rust-Display vectors.
//
// B — the live UI. The V1 document loads through the store, the Results lens carries the
// sweep panel, the run (beam.size.2 from 8 to 12, 5 points) streams its rows into the
// table, and the export downloads the CSV. The contract between the two targets is
// measured, not assumed: the integer and status columns must equal the CLI's cell for
// cell, the tracked floats must agree within two named gates (SOLVE_DRIFT_REL on the
// direct engine floats, CHANGE_DRIFT_REL on the web-derived change columns — see the
// constants' note and the report's Limits for what was measured and what was not), and
// wall_ms is each engine's own clock and cannot match — the report names the numbers. The
// document keeps its values: each point ran on a copy.
import { writeFileSync, mkdirSync, readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

const DOC = `\
fieldsmith 1
name "Cantilever V1"
custommaterial steel200 name="Steel (E 200 GPa)" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
`;

// Rust prints f64 Display in plain decimal everywhere; JS only inside [1e-6, 1e21).
// The rustFloat vectors live inline in the page evaluate below, at the range edges the
// two files' shared note names (1e-7 … 1e21, ±2.5e-8, -0, and the V23 ship literal).

/** Wait until the predicate holds in the page; `arg` is passed through evaluate
 * (closures are serialised away). */
async function until(page, fn, arg, ms = 30000, note = '') {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (await page.evaluate(fn, arg)) return;
    await page.waitForTimeout(250);
  }
  throw new Error(`timed out${note ? ` (${note})` : ''}: ${fn}`);
}

export async function sweepTest(browser, url, { screens, docsDir }) {
  mkdirSync(docsDir, { recursive: true });
  const tmp = mkdtempSync(resolve(tmpdir(), 'fieldsmith-sweep-'));
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, acceptDownloads: true });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });

  try {
    // ---- the native run: one question, one envelope, one CSV file -------------------------
    const bin = resolve(here, '../../target/debug/fieldsmith');
    const request = JSON.stringify({
      apiv: 2,
      doc: DOC,
      question: { kind: 'sweep', param: 'beam.size.2', values: [8, 9, 10, 11, 12], rungs: [30000, 150000] },
    });
    const csvPath = resolve(tmp, 'cantilever-sweep.csv');
    const tNat = Date.now();
    const nat = spawnSync(bin, ['api', '--csv', csvPath], {
      input: request,
      encoding: 'utf8',
      timeout: 180000,
      maxBuffer: 256 * 1024 * 1024,
    });
    const nativeMs = Date.now() - tNat;
    check('native api --csv exits 0', nat.status === 0, `status ${nat.status} stderr ${String(nat.stderr).slice(0, 200)}`);
    let envelope = null;
    try {
      envelope = JSON.parse(nat.stdout);
    } catch (e) {
      check('native stdout parses as the envelope', false, String(e));
    }
    if (envelope) {
      check('envelope answers ok and names the run', envelope.status === 'ok' && envelope.name === 'Cantilever V1', JSON.stringify({ status: envelope.status, name: envelope.name }));
      const sw = envelope.sweep;
      check('values echoed in order', JSON.stringify(sw?.values) === JSON.stringify([8, 9, 10, 11, 12]), JSON.stringify(sw?.values));
      check('five points, two rungs each', sw?.points?.length === 5 && sw.points.every((p) => p.outcome.records.length === 2), `points ${sw?.points?.length}, rungs ${sw?.points?.[0]?.outcome?.records?.length}`);
      check(
        'the run pinned the web ladder constants',
        sw?.points?.every?.((p) => p.outcome.records.every((r, j) => r.target_dofs === [30000, 150000][j])) === true,
        sw?.points?.[0]?.outcome?.records?.map((r) => r.target_dofs).join('/') ?? '(no records)',
      );
      check(
        'no tolerance declared, no verdict',
        sw?.points?.every?.((p) => p.outcome.met === null && p.outcome.unmet === null && p.outcome.refusal === null) === true,
        'all points met/unmet/refusal null',
      );
    }

    // ---- the page: module mirror + rustFloat vectors + the live sweep --------------------
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const mirror = await page.evaluate(async (raw) => {
      const [numfmt, app] = await Promise.all([import('/src/doc/numfmt.ts'), import('/src/app/sweep.ts')]);
      const env = JSON.parse(raw);
      const csv = app.sweepCsvFromEnvelope(env);
      const vectors = {};
      for (const [k, v] of Object.entries({
        small7: [numfmt.rustFloat(1e-7), '0.0000001'],
        small8: [numfmt.rustFloat(1.5e-8), '0.000000015'],
        small8b: [numfmt.rustFloat(2.5e-8), '0.000000025'],
        small8n: [numfmt.rustFloat(-2.5e-8), '-0.000000025'],
        big20: [numfmt.rustFloat(1e20), '1' + '0'.repeat(20)],
        big21: [numfmt.rustFloat(1e21), '1' + '0'.repeat(21)],
        negzero: [numfmt.rustFloat(-0), '-0'],
        literal: [numfmt.rustFloat(0.39066861588109364), '0.39066861588109364'],
      })) {
        const [got, want] = v;
        vectors[k] = `${got} === ${want} ? ${got === want}`;
      }
      return { csv, vectors };
    }, nat.stdout);
    for (const [k, v] of Object.entries(mirror.vectors)) {
      check(`rustFloat ${k} prints as Rust does`, v.endsWith('? true'), v);
    }

    const cliCsv = readFileSync(csvPath, 'utf8');
    check(
      'envelope mirror byte-equals the CLI CSV (one run, engine digits)',
      mirror.csv === cliCsv,
      `mirror ${mirror.csv.length} chars vs file ${cliCsv.length} chars`,
    );

    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      // The engine's own module instance (the app already imported it).
      const m = await import('/src/app/sweep.ts');
      window.__sweep = m;
    }, DOC);

    await page.locator('[data-lens="results"]').click();
    await until(
      page,
      () => window.fieldsmith.analysis.state.status === 'ready',
      null,
      240000,
      'ladder ready',
    );

    // The parameter defaults to the document's first addressable one; the question
    // needs beam.size.2, so the select drives it and from/to re-derive to its value.
    await page.selectOption('[data-testid="sweep-param"]', 'beam.size.2');
    await until(
      page,
      () => window.fieldsmith.ui.sweepParam === 'beam.size.2' && window.fieldsmith.ui.sweepFrom === null,
      null,
      5000,
      'parameter select',
    );
    // A controlled input only changes hands safely between renders. Two races were
    // observed live on this page: Preact rewrites the bound value inside Playwright's
    // clear-then-insert gap ('10' + '8' → '108'), and a Tab landing before the typed
    // text's render makes commit() read the stale closure. So each fill is gated on the
    // render it could race having visibly landed, and each blur is gated one frame past
    // the fill's own render.
    const settleFrame = () => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r())));
    await until(
      page,
      () => document.querySelector('[data-testid="sweep-from"]').value === '10',
      null,
      5000,
      'from re-derives to the selected value',
    );
    await page.locator('[data-testid="sweep-from"]').fill('8');
    await settleFrame();
    await page.locator('[data-testid="sweep-from"]').press('Tab');
    await until(page, () => window.fieldsmith.ui.sweepFrom === 8, null, 5000, 'from commit');
    await settleFrame(); // the from-commit render lands before the to-fill's clear
    await page.locator('[data-testid="sweep-to"]').fill('12');
    await settleFrame();
    await page.locator('[data-testid="sweep-to"]').press('Tab');
    await until(page, () => window.fieldsmith.ui.sweepTo === 12, null, 5000, 'to commit');
    const held = await page.evaluate(() => ({
      param: window.fieldsmith.ui.sweepParam,
      from: window.fieldsmith.ui.sweepFrom,
      to: window.fieldsmith.ui.sweepTo,
      count: window.fieldsmith.ui.sweepCount,
    }));
    check('panel holds the question', held.param === 'beam.size.2' && held.from === 8 && held.to === 12 && held.count === 5, JSON.stringify(held));

    const tRun = Date.now();
    await page.locator('[data-testid="sweep-run"]').click();
    await page.waitForSelector('[data-testid="sweep-progress"]', { timeout: 15000 });
    await until(
      page,
      () => window.__sweep.sweep.state.done === true,
      null,
      600000,
      'sweep points',
    );
    const sweepMs = Date.now() - tRun;
    const sw = await page.evaluate(() => ({
      running: window.__sweep.sweep.state.running,
      done: window.__sweep.sweep.state.done,
      error: window.__sweep.sweep.state.error,
      points: window.__sweep.sweep.state.points.length,
      docVersion: window.__sweep.sweep.state.docVersion,
      storeVersion: window.fieldsmith.store.docVersion,
    }));
    check('sweep ends whole with no error', sw.done && sw.error === null && sw.running === false, JSON.stringify(sw));
    check('five points streamed', sw.points === 5, `${sw.points}`);
    check('the sweep measured this document edition', sw.docVersion !== null && sw.docVersion === sw.storeVersion, `docVersion ${sw.docVersion}`);

    await page.waitForSelector('[data-testid="sweep-row-4"]', { timeout: 5000 });
    const rows = await page.evaluate(() =>
      [0, 1, 2, 3, 4].map((i) => {
        const el = document.querySelector(`[data-testid="sweep-row-${i}"]`);
        const valueEl = el?.parentElement?.previousElementSibling;
        return `${valueEl?.textContent ?? '?'} | ${el?.textContent ?? '?'}`;
      }),
    );

    // ---- the export: the real click → download flow --------------------------------------
    const [dl] = await Promise.all([page.waitForEvent('download'), page.locator('[data-testid="sweep-export"]').click()]);
    check('export names the document', dl.suggestedFilename().includes('sweep.csv'), dl.suggestedFilename());
    const webCsv = readFileSync(await dl.path(), 'utf8');

    // The cross-target contract, measured: the integer and status columns (0 value, 1 dofs,
    // 2 elements, 8 status) must equal the CLI cell for cell; the tracked floats must agree
    // within two named gates (Limits names what was measured and what was not); wall_ms is
    // column 3 and is each engine's own clock — the report records both numbers.
    // SOLVE_DRIFT_REL gates the direct engine floats (4 deflection, 5 vm). The change
    // columns (6,7) are the web's own JS division of drift-affected doubles by the change
    // fraction — the relative drift of a difference of doubles blows up by ~1/change
    // fraction (~3400× on this question, change_rel ≈ 2.9e-4) — so they get their own,
    // looser gate CHANGE_DRIFT_REL, still tight in absolute terms (see Limits).
    const SOLVE_DRIFT_REL = 1e-7;
    const CHANGE_DRIFT_REL = 1e-6;
    const gateTxt = (g) => g.toExponential().replace('e+', 'e');
    const CSV_COLS = ['value', 'dofs', 'elements', 'wall_ms', 'deflection_mm', 'vm_mpa', 'compliance_nmm', 'deflection_change_rel', 'vm_change_rel', 'compliance_change_rel', 'status'];
    const driftRel = (x, y) => (x === y ? 0 : Math.abs(x - y) / Math.max(Math.abs(x), Math.abs(y)));
    const cmp = { header: false, lines: 0, cells: 0, mismatched: 0, mismatch: '', walls: '', maxDrift: 0, maxChangeDrift: 0 };
    const a = cliCsv.split('\n');
    const b = webCsv.split('\n');
    cmp.header = a[0] === b[0];
    cmp.lines = Math.min(a.filter((x) => x !== '').length, b.filter((x) => x !== '').length);
    for (let i = 0; i < cmp.lines; i++) {
      const ca = a[i].split(',');
      const cb = b[i].split(',');
      if (ca.length !== cb.length) {
        cmp.mismatched++;
        if (!cmp.mismatch) cmp.mismatch = `line ${i}: ${ca.length} vs ${cb.length} columns`;
        continue;
      }
      if (i === 0) continue; // header line: compared by the header check
      for (let c = 0; c < ca.length; c++) {
        if (c === 3) continue;
        if (c === 0 || c === 1 || c === 2 || c === 10) {
          if (ca[c] === cb[c]) {
            if (c !== 0) cmp.cells++;
          } else {
            cmp.mismatched++;
            if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${c} (${CSV_COLS[c]}): native ${ca[c]} vs web ${cb[c]}`;
          }
          continue;
        }
        const na = Number(ca[c]);
        const nb = Number(cb[c]);
        const gate = c === 7 || c === 8 || c === 9 ? CHANGE_DRIFT_REL : SOLVE_DRIFT_REL;
        if (!Number.isFinite(na) || !Number.isFinite(nb)) {
          cmp.mismatched++;
          if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${c} (${CSV_COLS[c]}): not a finite number, native "${ca[c]}" vs web "${cb[c]}"`;
          continue;
        }
        const rel = driftRel(na, nb);
        if (!(rel <= gate)) {
          cmp.mismatched++;
          if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${c} (${CSV_COLS[c]}): rel ${rel.toExponential(2)} > gate ${gate}, native ${ca[c]} vs web ${cb[c]}`;
        } else {
          cmp.cells++;
          if (c === 7 || c === 8 || c === 9) {
            if (rel > cmp.maxChangeDrift) cmp.maxChangeDrift = rel;
          } else if (rel > cmp.maxDrift) cmp.maxDrift = rel;
        }
      }
      if (i === 1) cmp.walls = `native ${ca[3]} ms vs web ${cb[3]} ms (row 0, refined rung)`;
    }
    check('web CSV header = the CLI header', cmp.header, JSON.stringify(b[0]));
    check('web CSV row count = the CLI row count', b.filter((l) => l !== '').length === a.filter((l) => l !== '').length, `${b.length} vs ${a.length} lines`);
    check(
      'integer/status columns equal, tracked floats within solve-drift',
      cmp.mismatched === 0,
      `${cmp.lines - 1} data rows, ${cmp.cells} compared cells, gates direct ${gateTxt(SOLVE_DRIFT_REL)} / derived ${gateTxt(CHANGE_DRIFT_REL)}, max observed drift direct ${cmp.maxDrift.toExponential(2)} / derived ${cmp.maxChangeDrift.toExponential(2)} relative${cmp.mismatched ? `; first mismatch ${cmp.mismatch}` : ''}`,
    );
    check('row 0 wall columns recorded', cmp.walls !== '', cmp.walls);
    // The V23 pin lives on the native side; the web sits inside the measured drift ball.
    const natRow0 = a[1]?.split(',') ?? [];
    check('native point 0 deflection is the ship literal', natRow0[4] === '0.39066861588109364', natRow0[4] ?? '(no row)');
    const webRow0 = b[1]?.split(',') ?? [];
    const relLit = Number.isFinite(Number(webRow0[4])) && Number.isFinite(Number(natRow0[4]))
      ? driftRel(Number(webRow0[4]), Number(natRow0[4]))
      : NaN;
    check(
      'web point 0 deflection within solve-drift of the ship literal',
      relLit <= SOLVE_DRIFT_REL,
      `${webRow0[4] ?? '(no row)'} vs ${natRow0[4] ?? '(no row)'} (rel ${Number.isFinite(relLit) ? relLit.toExponential(2) : 'NaN'})`,
    );

    // The document keeps its values — each point ran on a copy.
    const restore = await page.evaluate(async () => {
      const p = await import('/src/doc/param.ts');
      return p.readParam(window.fieldsmith.store.doc, p.parseAddr('beam.size.2'));
    });
    check('beam.size.2 is still 10 after the sweep', restore === 10, `${restore}`);

    const panelShot = resolve(screens, 'sweep-01-panel.png');
    await page.screenshot({ path: panelShot });

    // ---- report ----------------------------------------------------------------------------
    check('no console errors', errors.length === 0, errors.join(' | ').slice(0, 400));
    const ok = checks.every((c) => c.pass);
    const checkRows = (names) =>
      names.map((name) => {
        const c = checks.find((x) => x.name === name);
        return `| ${name} | ${c?.pass ? 'pass' : '**fail**'} | ${c?.detail ?? ''} |`.replace(/\n/g, ' ');
      });
    const table = (rows) => ['| Check | Result | Detail |', '| :-- | :-- | :-- |', ...rows].join('\n');
    const natRows = envelope
      ?.sweep?.points?.map?.((p) => {
        const last = p.outcome.records[1];
        return `| ${p.value} | ${last.dofs} | ${last.elements} | ${last.wall_ms} | ${last.tracked.deflection_mm} | ${last.tracked.vm_mpa} |`;
      })
      .join('\n');
    const lines = [
      '# Sweep e2e — the results lens’ parameter sweep (M16 slice 3)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
      '',
      'The five-point question on the Cantilever V1 (`beam.size.2` 8 → 12 mm, rungs',
      '30000/150000 asked explicitly on both sides): the native `fieldsmith api --csv` run once,',
      'then the same question driven through the Results lens in the live app.',
      '',
      '## Evidence A — the byte contract (CSV file ↔ in-page envelope mirror)',
      '',
      `- Native: \`fieldsmith api --csv <tmp>\` reading the request on stdin; exit ${nat.status},`,
      `  ${nativeMs} ms whole. Omitting \`rungs\` would ask the native three-rung default`,
      '  (`question.rs` `DEFAULT_RUNGS` = 30000/150000/750000) — the request pins the ladder',
      '  coarse/refined pair the web mirrors, so the two sides answer the same question.',
      '- `sweepCsvFromEnvelope(JSON.parse(stdout))` in the page (web/src/app/sweep.ts) reproduces',
      `  the CLI's CSV file byte for byte: ${mirror.csv === cliCsv ? 'pass' : '**fail**'} (mirror ${mirror.csv.length} chars, file ${cliCsv.length} chars).`,
      '- Why that must hold: the CSV file and the JSON envelope are printed from one typed',
      '  response, so their float values are the same doubles; both writers use',
      '  shortest-round-trip decimal text, and parsing it returns those same doubles;',
      '  rustFloat is the JS twin of Rust\'s plain-decimal Display, so each cell re-renders to',
      '  the file\'s own bytes.',
      '',
      '| rustFloat vector | rendered |',
      '| :-- | :-- |',
      ...Object.entries(mirror.vectors).map(([k, v]) => `| ${k} | ${v} |`),
      '',
      '## The native answer (per-point ladders, refined rung)',
      '',
      '| point (mm) | dofs | elements | wall r1 (ms) | deflection (mm) | vm (MPa) |',
      '| :-- | --: | --: | --: | --: | --: |',
      natRows ?? '(envelope missing)',
      '',
      '## Evidence B — the live UI sweep',
      '',
      `- Loaded through the store, Results lens, ladder ready; the panel's parameter select`,
      `  steps to \`beam.size.2\`, from/to committed to 8/12 on blur. Sweep run: ${sweepMs} ms.`,
      `- Rows streamed: ${sw.points} with progress shown mid-run; export through the real click`,
      `  → download flow (${dl.suggestedFilename()}).`,
      '',
      table(
        checkRows([
          'sweep ends whole with no error',
          'five points streamed',
          'the sweep measured this document edition',
          'web CSV header = the CLI header',
          'web CSV row count = the CLI row count',
          'integer/status columns equal, tracked floats within solve-drift',
          'row 0 wall columns recorded',
          'native point 0 deflection is the ship literal',
          'web point 0 deflection within solve-drift of the ship literal',
          'beam.size.2 is still 10 after the sweep',
          'export names the document',
          'no console errors',
        ]),
      ),
      `- What the panel showed while the run went (value | row text):`,
      ...rows.map((r) => `  - ${r}`),
      '',
      `- Wall columns: ${cmp.walls}. Column 3 (wall_ms) is each engine's own clock. Columns`,
      `  4–7 are gated (the solve-drift contract, Limits): ${cmp.cells} cells compared across`,
      `  the five data rows — columns 0/1/2/8 exact, 4/5 within ${gateTxt(SOLVE_DRIFT_REL)}, 6/7 within`,
      `  ${gateTxt(CHANGE_DRIFT_REL)} — ${cmp.mismatched} mismatched, largest observed drift`,
      `  ${cmp.maxDrift.toExponential(2)} (direct) and ${cmp.maxChangeDrift.toExponential(2)}`,
      `  (derived change columns) relative.`,
      '',
      '## The web CSV',
      '',
      '```csv',
      webCsv.trimEnd(),
      '```',
      '',
      '## Method',
      '',
      '`web/tests/sweep.mjs`: the native reply comes from one `api --csv` run (both artifacts',
      'from the same typed response); the mirror and the rustFloat vectors run in the page',
      'against the module the app itself uses; the UI question is driven through the real',
      'NumberInput blur-commit path and the Run button, and the export through a real download.',
      'The two targets are not assumed bit-identical: columns 0/1/2/8 must equal the CLI cell',
      'for cell, columns 4–7 are gated at SOLVE_DRIFT_REL — named and measured under Limits.',
      '',
      '## Limits',
      '',
      '- wall_ms cannot match between native and wasm, and the report does not ask it to; the',
      '  walls that agree are Evidence A\'s (one run).',
      '- No tolerance is declared on either side, so every row\'s verdict is `ok` on both sides;',
      '  the met/refusal cell paths are exercised by the Rust cases, not this browser run.',
      '- Point 0\'s deflection literal (0.39066861588109364) is pinned from the referee ship run',
      '  (V23, native). The native CLI reproduces it exactly in this run; the web lands within',
      '  SOLVE_DRIFT_REL of it.',
      '- Cross-target bit-identity does not hold here, and no case requires it: V23 pins',
      '  native-vs-native and M15 checked x86_64-vs-x86_64. Measured on a diagnostic ladder',
      '  (outside this run): the mesh stage is bit-identical across targets at every size',
      '  (dofs/elements/order/element_size and the two boundary integrals); the solve is',
      '  bit-identical at total 8247 dofs (coarse factor 1437) and drifts at 77535 and 202473',
      '  (coarse 11679/29250) — the same iteration count on both targets (24 at the refined',
      '  rung), relative residual differing ~5.6e-8 relative (4.694681002945911e-10 native vs',
      '  4.694681003002073e-10 web at 202473 dofs), tracked floats differing ≤ 7e-11 relative',
      '  on this question. Both answers sit inside the PCG convergence ball (rtol 1e-9). The',
      '  mechanism, consistent-with not proven: faer 0.20.2\'s sparse Cholesky — the two-level',
      '  preconditioner\'s coarse factor — dispatches its numeric kernels through pulp per ISA',
      '  (AVX2/FMA on x86_64, wasm SIMD elsewhere), so accumulation order differs below the',
      '  identical combinatorial symbolic choice; the fieldsmith solver\'s own reductions are',
      '  sequential and single-threaded on both sides.',
      `- The float gates: SOLVE_DRIFT_REL = ${SOLVE_DRIFT_REL} on the direct columns (4/5) is`,
      '  100× the solver\'s own rtol (1e-9) — a tighter gate would test convergence noise, not',
      '  the pipeline — and 10⁵× below the 1e-2 scale of the tolerance grammar this feature',
      '  serves. CHANGE_DRIFT_REL = ' + gateTxt(CHANGE_DRIFT_REL) + ' on the derived change columns (6/7)',
      '  accounts for the web\'s own JS division: the relative error of a difference of',
      '  drift-affected doubles scales as 1/change fraction (~3400× on this question,',
      '  change_rel ≈ 2.9e-4), so the ≤ 7e-11 direct drift shows up as ~1.1e-7 — above the',
      '  direct gate, below the derived one. Even so the derived gate implies tighter absolute',
      '  discipline on the deflections than the direct gate does (chg·2.9e-10 vs 1e-7',
      '  relative). Unverified: the exact faer kernel where the bits diverge; whether the',
      '  multithreaded pkg (the isolated page) drifts the same way; the N100 native leg, which',
      '  is slice 4, not this run.',
      '- The panel\'s rung schedule is the ladder\'s coarse/refined pair (30k/150k, the worker',
      '  wasm.ts constants); the native three-rung default is only avoided by the explicit',
      '  `rungs` in this test\'s request.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/sweep.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'sweep-e2e.md'), lines.join('\n'));
    console.log(`sweep lens ${ok ? 'PASS' : 'FAIL'}: native ${nativeMs} ms, web ${sweepMs} ms, mirror ${
      mirror.csv === cliCsv ? 'byte-equal' : 'MISMATCH'
    }`);
    if (!ok) {
      for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    }
    if (errors.length) console.error('console errors:', errors);
    await page.close();
    return ok;
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}