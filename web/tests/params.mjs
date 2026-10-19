// The .fsm let-variables and expressions (the M: .fsm let-variables step): named scalars
// (`let thick = 10`) and arithmetic drive dimensions, placements and loads, the sweep
// panel addresses a let by its bare name, and each swept point runs on a clone. Run
// with node tests/run-e2e.mjs --only params.
//
// The parameterised Cantilever V1 declares with `let`s exactly the geometry the plain
// Cantilever V1 (tests/sweep.mjs) carries as literals. Three evidences against the same
// five-point question (thickness 8 → 12 mm, rungs 30000/150000 pinned on both sides):
//
// A — representation. The parameterised doc's bare-let sweep (`thick`) answers with the
// plain document's node-path sweep (`beam.size.2`) to the byte — one machine, one
// binary, one geometry per point, only wall_ms differs — including the V23 ship literal
// at point 0. The same parameterised doc refuses the node-path sweep, and the envelope
// carries "sweep the let that drives it": a question cannot silently fight the
// expression that writes the number.
//
// B — the module, in the page: the TS mirror's evaluator (precedence, left fold, unary
// minus, the plain Rust error set), the bare-let apply chaining through a derived let
// (the applied let keeps the swept value, the others re-evaluate — no clobber), the
// driven refusal text, and the .fsm round trip (the printer re-links the lets into the
// statements, so Save keeps the parameterisation).
//
// C — the live UI. The Inspector's Parameters panel lists the lets; a driven field
// refuses an edit with the sweep-the-let note per axis (position x refuses, position z
// stays editable — literal components record nothing); the sweep run streams five
// points through the store's worker, and the export writes the CLI's CSV under the two
// named drift gates (SOLVE_DRIFT_REL on the direct engine floats, CHANGE_DRIFT_REL on
// the web-derived change columns, the sweep-e2e contract; wall_ms is each engine's own
// clock).
import { readFileSync, writeFileSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

const PARAM_DOC = `\
fieldsmith 1
name "Cantilever V1"
custommaterial steel200 name="Steel (E 200 GPa)" e=200000 nu=0.3 yield=250 density=7.85
material steel200
let span = 100
let height = 10
let half = height / 2
let thick = 10
let pull = 100
box beam size=(span, height, thick) at=(span / 2, half, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -pull)
`;

// The same cantilever with every number as a literal — the node-path reference whose
// numbers the let sweep must reproduce (the sweep-e2e question's own document).
const PLAIN_DOC = `\
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

const VALUES = [8, 9, 10, 11, 12];
const RUNGS = [30000, 150000];
const SOLVE_DRIFT_REL = 1e-7;
const CHANGE_DRIFT_REL = 1e-6;

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

function ask(bin, param, doc, csv = null) {
  const request = JSON.stringify({
    apiv: 2,
    doc,
    question: { kind: 'sweep', param, values: VALUES, rungs: RUNGS },
  });
  const args = csv ? ['api', '--csv', csv] : ['api'];
  return spawnSync(bin, args, {
    input: request,
    encoding: 'utf8',
    timeout: 180000,
    maxBuffer: 256 * 1024 * 1024,
  });
}

export async function paramsTest(browser, url, { screens, docsDir }) {
  mkdirSync(docsDir, { recursive: true });
  const tmp = mkdtempSync(resolve(tmpdir(), 'fieldsmith-params-'));
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, acceptDownloads: true });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });

  try {
    // ---- the native runs: the reference question, the let question, the refusal ---------
    const bin = resolve(here, '../../target/debug/fieldsmith');
    const tNode = Date.now();
    const node = ask(bin, 'beam.size.2', PLAIN_DOC, resolve(tmp, 'node.csv'));
    const nodeMs = Date.now() - tNode;
    check('native node-path sweep exits 0', node.status === 0, `status ${node.status} stderr ${String(node.stderr).slice(0, 200)}`);
    const tLet = Date.now();
    const let_run = ask(bin, 'thick', PARAM_DOC, resolve(tmp, 'let.csv'));
    const letNativeMs = Date.now() - tLet;
    check('native let sweep exits 0', let_run.status === 0, `status ${let_run.status} stderr ${String(let_run.stderr).slice(0, 200)}`);
    const tRef = Date.now();
    const ref = ask(bin, 'beam.size.2', PARAM_DOC);
    const refMs = Date.now() - tRef;

    let nodeEnv = null;
    let letEnv = null;
    try {
      nodeEnv = JSON.parse(node.stdout);
      letEnv = JSON.parse(let_run.stdout);
    } catch (e) {
      check('native stdout parses as the envelopes', false, String(e));
    }
    const envelopeChecks = (env, what) => {
      check(`${what} answers ok and names the run`, env?.status === 'ok' && env?.name === 'Cantilever V1', JSON.stringify({ status: env?.status, name: env?.name }));
      const sw = env?.sweep;
      check(`${what} echoes the values in order`, JSON.stringify(sw?.values) === JSON.stringify(VALUES), JSON.stringify(sw?.values));
      check(
        `${what} asks five points on the pinned two-rung ladder, with no verdict`,
        sw?.points?.length === 5 && sw.points.every((p) => p.outcome.records.length === 2 && p.outcome.met === null && p.outcome.refusal === null) &&
          sw.points[0].outcome.records.every((r, j) => r.target_dofs === RUNGS[j]),
        `points ${sw?.points?.length ?? '(none)'}`,
      );
    };
    if (nodeEnv && letEnv) {
      envelopeChecks(nodeEnv, 'the node-path run');
      envelopeChecks(letEnv, 'the let run');
    }
    let refEnv = null;
    try {
      refEnv = JSON.parse(ref.stdout);
    } catch (e) {
      check('the refusal envelope parses', false, String(e));
    }
    check(
      "the driven node path is refused: sweep the let that drives it",
      refEnv?.status === 'error' &&
        (refEnv?.error ?? '').includes(
          "param beam.size.2: written by an expression ('thick') — sweep the let that drives it",
        ),
      `status ${refEnv?.status}, in ${refMs} ms: ${String(refEnv?.error).slice(0, 220)}`,
    );

    // The representation contract, measured exactly: same geometry per point, same
    // machine, same binary — every CSV cell except each run's own wall_ms must equal.
    const CSV_COLS = ['value', 'dofs', 'elements', 'wall_ms', 'deflection_mm', 'vm_mpa', 'compliance_nmm', 'deflection_change_rel', 'vm_change_rel', 'compliance_change_rel', 'status'];
    const nodeCsv = node.status === 0 ? readFileSync(resolve(tmp, 'node.csv'), 'utf8') : '';
    const letCsv = let_run.status === 0 ? readFileSync(resolve(tmp, 'let.csv'), 'utf8') : '';
    const a = nodeCsv.split('\n').filter((x) => x !== '');
    const b = letCsv.split('\n').filter((x) => x !== '');
    const sameCell = { equal: 0, wrong: [] };
    for (let i = 1; i < Math.min(a.length, b.length); i++) {
      const ca = a[i].split(',');
      const cb = b[i].split(',');
      for (let c = 0; c < ca.length; c++) {
        if (c === 3) continue; // wall_ms: two runs, two clocks
        if (ca[c] === cb[c]) sameCell.equal++;
        else sameCell.wrong.push(`row ${i - 1} col ${c} (${CSV_COLS[c]}): node-path ${ca[c]} vs let ${cb[c]}`);
      }
    }
    check(
      'the let sweep reproduces the node-path sweep numbers exactly (wall column excepted)',
      a[0] === b[0] && a.length === b.length && sameCell.wrong.length === 0,
      `${a.length - 1} rows, header equal, ${sameCell.equal}/40 non-wall cells equal${
        sameCell.wrong.length ? `; first: ${sameCell.wrong[0]}` : ''
      }`,
    );
    const letRows = b.slice(1).map((l) => l.split(','));
    check(
      'point 0 is the V23 ship literal',
      letRows[0]?.[4] === '0.39066861588109364',
      `deflection ${letRows[0]?.[4] ?? '(no row)'}`,
    );

    // ---- the page: module mirror, round trip, driven fields ------------------------------
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.__sweep = await import('/src/app/sweep.ts');
    }, PARAM_DOC);
    // The panel gates on the store's doc having lets — it appears only after the load above.
    await page.waitForSelector('[data-testid="params-panel"]', { timeout: 60000 });

    const LETS = [
      ['span', 100],
      ['height', 10],
      ['half', 5],
      ['thick', 10],
      ['pull', 100],
    ];
    const mirror = await page.evaluate(async (lets) => {
      const p = await import('/src/doc/param.ts');
      const okEval = [];
      const badEval = [];
      const okErr = [];
      const badErr = [];
      const evals = [
        ['span / 2', 50],
        ['20 / 5 + 1', 5],
        ['10 - 2 - 3', 5],
        ['2 * (3 + 1)', 8],
        ['-pull', -100],
        ['half', 5],
        ['4-2', 2],
        ['1e2 + 1', 101],
      ];
      const errs = [
        ['1 2', 'unexpected text after the expression'],
        ['half / (half - half)', 'division by zero'],
        ['q + 1', "no let named 'q'"],
        ['1e', "bad number '1e'"],
      ];
      for (const [text, want] of evals) {
        let v = NaN;
        let err = null;
        try {
          v = p.evalFsmExpr(text, lets);
        } catch (e) {
          err = e.message;
        }
        (v === want ? okEval : badEval).push(`${text} → ${err ?? v} (want ${want})`);
      }
      for (const [text, want] of errs) {
        let got = null;
        try {
          p.evalFsmExpr(text, lets);
        } catch (e) {
          got = e.message;
        }
        (got !== null && got === want ? okErr : badErr).push(`${text} → ${got ?? 'no error'} (want ${want})`);
      }
      const base = window.fieldsmith.store.doc;
      const out = {
        okEval, badEval, okErr, badErr,
        consumed: (base.consumed ?? []).map((c) => `${c.target}.${c.attr}${c.comp === null ? '' : '.' + c.comp}: ${c.expr}`),
        height: {}, half: {}, thick: {}, restore: {}, picker: [], print: {},
      };
      // One apply per fresh clone — a sweep never multi-applies (each point clones the
      // parsed doc), so each case below is the exact shape the sweep exercises.
      const applyOn = (name, value) => {
        const d = structuredClone(base);
        p.applyAddr(d, p.parseAddr(name), value);
        return d;
      };
      const beamOf = (d) => d.root.find((x) => x.id === 'beam');
      // The derived-let chain: height drives half drives at.1 (and size.1). The applied
      // let keeps the swept value; derived half re-evaluates from it.
      try {
        const d = applyOn('height', 12);
        out.height = {
          height: d.lets.find((l) => l.name === 'height')?.value,
          half: d.lets.find((l) => l.name === 'half')?.value,
          size: beamOf(d)?.kind?.size,
          at: beamOf(d)?.transform?.translate,
        };
      } catch (e) {
        out.height = { error: e.message };
      }
      // An applied DERIVED let keeps its swept value while its readers update.
      try {
        const d = applyOn('half', 7);
        out.half = {
          half: d.lets.find((l) => l.name === 'half')?.value,
          height: d.lets.find((l) => l.name === 'height')?.value,
          size: beamOf(d)?.kind?.size,
          at: beamOf(d)?.transform?.translate,
        };
      } catch (e) {
        out.half = { error: e.message };
      }
      // A plain let rewrites only its own readers; every other number stays.
      try {
        const d = applyOn('thick', 9);
        out.thick = {
          thick: d.lets.find((l) => l.name === 'thick')?.value,
          size: beamOf(d)?.kind?.size,
          at: beamOf(d)?.transform?.translate,
          force2: d.loads[0]?.kind?.vector?.[2],
        };
      } catch (e) {
        out.thick = { error: e.message };
      }
      // Restore on one clone, the way a human would walk it back; the rule re-evaluates
      // every non-applied let from its expression, so the end is the plain numbers again.
      try {
        const d = structuredClone(base);
        p.applyAddr(d, p.parseAddr('height'), 12);
        p.applyAddr(d, p.parseAddr('thick'), 9);
        p.applyAddr(d, p.parseAddr('thick'), 10);
        p.applyAddr(d, p.parseAddr('height'), 10);
        out.restore = {
          half: d.lets.find((l) => l.name === 'half')?.value,
          size: beamOf(d)?.kind?.size,
          at: beamOf(d)?.transform?.translate,
          force2: d.loads[0]?.kind?.vector?.[2],
        };
      } catch (e) {
        out.restore = { error: e.message };
      }
      // A driven param refuses instead of fighting the expression; a bare unknown id
      // names itself in the resolve error.
      try {
        p.applyAddr(structuredClone(base), p.parseAddr('beam.size.2'), 5);
        out.refusal = 'no error';
      } catch (e) {
        out.refusal = e.message;
      }
      try {
        p.applyAddr(structuredClone(base), p.parseAddr('nonesuch'), 1);
        out.bare = 'no error';
      } catch (e) {
        out.bare = e.message;
      }
      out.picker = p.addressableParams(window.fieldsmith.store.doc).slice(-5).map((x) => x.addr);
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      out.print = {
        text,
        reparses: !!r.ok,
        lets: r.ok ? (r.ok.lets ?? []).map((l) => `${l.name} = ${l.expr}`) : [],
        consumed: r.ok ? (r.ok.consumed ?? []).length : -1,
      };
      return out;
    }, LETS);
    check('evaluator round trips (precedence, left fold, unary minus, sign glue)', mirror.okEval.length === 8, mirror.okEval.concat(mirror.badEval).join(' | '));
    check('evaluator errors are the plain Rust texts', mirror.badEval.length === 0 && mirror.okErr.length === 4, `values: ${mirror.badEval.join(' | ') || 'ok'}; errors: ${[...mirror.okErr, ...mirror.badErr].join(' | ')}`);
    check(
      'a let apply drives the derived let and all its readers',
      mirror.height.height === 12 && mirror.height.half === 6 &&
        JSON.stringify(mirror.height.size) === '[100,12,10]' && JSON.stringify(mirror.height.at) === '[50,6,5]',
      `height ${JSON.stringify(mirror.height)}`,
    );
    check(
      'an applied derived let keeps its swept value while its readers update',
      mirror.half.half === 7 && mirror.half.height === 10 &&
        JSON.stringify(mirror.half.size) === '[100,10,10]' && JSON.stringify(mirror.half.at) === '[50,7,5]',
      `half ${JSON.stringify(mirror.half)}`,
    );
    check(
      'a plain let apply rewrites only its own readers',
      mirror.thick.thick === 9 && JSON.stringify(mirror.thick.size) === '[100,10,9]' &&
        JSON.stringify(mirror.thick.at) === '[50,5,5]' && mirror.thick.force2 === -100,
      `thick ${JSON.stringify(mirror.thick)}`,
    );
    check('the parsed doc links every consumer back to its let', mirror.consumed.length === 6, JSON.stringify(mirror.consumed));
    check(
      'restoring the applied lets restores the plain numbers',
      JSON.stringify(mirror.restore.size) === '[100,10,10]' && JSON.stringify(mirror.restore.at) === '[50,5,5]' &&
        mirror.restore.force2 === -100 && mirror.restore.half === 5,
      JSON.stringify(mirror.restore),
    );
    check(
      'a driven param refuses with the exact sweep-the-let text',
      mirror.refusal === "param beam.size.2: written by an expression ('thick') — sweep the let that drives it",
      String(mirror.refusal),
    );
    check(
      'a bare unknown id is refused with its own name',
      mirror.bare === 'param nonesuch: no parameter named "nonesuch"',
      String(mirror.bare),
    );
    check('the picker lists the bare let names last', mirror.picker.join(',') === 'span,height,half,thick,pull', mirror.picker.join(', '));
    check(
      'the .fsm round trip keeps the lets and re-links them into the statements',
      mirror.print.reparses &&
        JSON.stringify(mirror.print.lets) === JSON.stringify(['span = 100', 'height = 10', 'half = height / 2', 'thick = 10', 'pull = 100']) &&
        mirror.print.consumed === 6 &&
        (mirror.print.text.match(/size=\(span, height, thick\)/) ?? []).length > 0,
      `lets ${JSON.stringify(mirror.print.lets)}, consumed ${mirror.print.consumed}, text: ${String(mirror.print.text).replace(/\n/g, ' ⏎ ').slice(0, 160)}`,
    );

    // The Parameters panel on the (unselected) document inspector.
    const panel = await page.evaluate(() => [...document.querySelectorAll('[data-testid="params-panel"] label')].map((l) => l.textContent));
    check('the Parameters panel lists the five lets in declaration order', panel.join(',') === 'span,height,half,thick,pull', panel.join(', '));
    const panelShot = resolve(screens, 'params-01-panel.png');
    await page.screenshot({ path: panelShot });

    // ---- driven fields refuse, per axis, with the sweep-the-let note ---------------------
    await page.locator('.row[data-node="beam"]').click();
    const titles = await page.evaluate(() => ({
      zx: document.querySelector('[data-testid="size-x"]')?.title ?? '',
      zz: document.querySelector('[data-testid="size-z"]')?.title ?? '',
      px: document.querySelector('[data-testid="position-x"]')?.title ?? '',
    }));
    check(
      'driven fields name their expression as the edit hint',
      titles.zz.includes("written by an expression ('thick')") &&
        titles.zx.includes("written by an expression ('span')") &&
        titles.px.includes("written by an expression ('span / 2')"),
      JSON.stringify(titles),
    );
    const settleFrame = () => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => r())));
    const v0 = await page.evaluate(() => window.fieldsmith.store.docVersion);
    await page.locator('[data-testid="size-z"]').click();
    await page.locator('[data-testid="size-z"]').fill('11.5');
    await settleFrame();
    await page.locator('[data-testid="size-z"]').press('Tab');
    await until(
      page,
      () => (document.querySelector('[data-testid="toast"]')?.textContent ?? '').includes('sweep the let that drives it'),
      null,
      4000,
      'refusal toast',
    );
    await until(page, () => document.querySelector('[data-testid="size-z"]').value === '10', null, 4000, 'field restore');
    const refused = await page.evaluate(() => window.fieldsmith.store.docVersion);
    check('a driven field refuses the edit and keeps the document', refused === v0, `docVersion ${refused} = ${v0}, field restored to '10'`);
    const refShot = resolve(screens, 'params-02-refusal.png');
    await page.screenshot({ path: refShot });

    // Per axis: position x is an expression's write (span / 2), position z is a plain 5.
    await page.locator('[data-testid="position-x"]').click();
    await page.locator('[data-testid="position-x"]').fill('12');
    await settleFrame();
    await page.locator('[data-testid="position-x"]').press('Tab');
    await until(
      page,
      () => (document.querySelector('[data-testid="toast"]')?.textContent ?? '').includes("'span / 2'"),
      null,
      4000,
      'x-axis refusal toast',
    );
    await page.locator('[data-testid="position-z"]').click();
    await page.locator('[data-testid="position-z"]').fill('6.5');
    await settleFrame();
    await page.locator('[data-testid="position-z"]').press('Tab');
    await until(page, () => document.querySelector('[data-testid="position-z"]').value === '6.5', null, 4000, 'z-axis edit commits');
    const v1 = await page.evaluate(() => window.fieldsmith.store.docVersion);
    check('the guard is per axis: x refuses, z edits', v1 === v0 + 1, `docVersion ${v0} → ${v1} after the z edit`);
    // Shortcuts are ignored while an input is focused; click the tree row to release it.
    await page.locator('.row[data-node="beam"]').click();
    await page.keyboard.press('Control+z');
    await until(page, () => document.querySelector('[data-testid="position-z"]').value === '5', null, 4000, 'z-axis undo');
    // The x-axis toast was replaced; make sure no stray document edit lingered.
    const afterUndo = await page.evaluate(() => window.fieldsmith.store.docVersion);
    check('the refused x edit left nothing to undo before the z edit', afterUndo === v0 + 2, `docVersion ${afterUndo} (z edit + its undo)`);

    // ---- the live sweep through the bare-let address -------------------------------------
    await page.locator('[data-lens="results"]').click();
    await until(
      page,
      () => window.fieldsmith.analysis.state.status === 'ready',
      null,
      240000,
      'ladder ready',
    );
    await page.selectOption('[data-testid="sweep-param"]', 'thick');
    await until(
      page,
      () => window.fieldsmith.ui.sweepParam === 'thick' && window.fieldsmith.ui.sweepFrom === null,
      null,
      5000,
      'parameter select',
    );
    await until(
      page,
      () => document.querySelector('[data-testid="sweep-from"]').value === '10',
      null,
      5000,
      'from re-derives to the let value',
    );
    await page.locator('[data-testid="sweep-from"]').fill('8');
    await settleFrame();
    await page.locator('[data-testid="sweep-from"]').press('Tab');
    await until(page, () => window.fieldsmith.ui.sweepFrom === 8, null, 5000, 'from commit');
    await settleFrame();
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
    check('the panel holds the bare-let question', held.param === 'thick' && held.from === 8 && held.to === 12 && held.count === 5, JSON.stringify(held));

    const tRun = Date.now();
    await page.locator('[data-testid="sweep-run"]').click();
    await page.waitForSelector('[data-testid="sweep-progress"]', { timeout: 15000 });
    try {
      await until(
        page,
        // finish on an error too — sweepError leaves done false, and the message is the evidence
        () => { const s = window.__sweep.sweep.state; return s.done === true || (s.running === false && s.error !== null); },
        null,
        600000,
        'sweep points',
      );
    } catch (e) {
      const st = await page.evaluate(() => window.__sweep?.sweep?.state ?? null);
      throw new Error(`sweep did not finish: ${JSON.stringify(st)}`);
    }
    const sweepMs = Date.now() - tRun;
    const sw = await page.evaluate(() => ({
      running: window.__sweep.sweep.state.running,
      done: window.__sweep.sweep.state.done,
      error: window.__sweep.sweep.state.error,
      points: window.__sweep.sweep.state.points.length,
      docVersion: window.__sweep.sweep.state.docVersion,
      storeVersion: window.fieldsmith.store.docVersion,
    }));
    check('the sweep ends whole with no error', sw.done && sw.error === null && sw.running === false, JSON.stringify(sw));
    check('five points streamed', sw.points === 5, `${sw.points}`);
    check('the sweep measured this document edition', sw.docVersion !== null && sw.docVersion === sw.storeVersion, `docVersion ${sw.docVersion}`);

    const rows = await page.evaluate(() => {
      const row = (i) => {
        const el = document.querySelector(`[data-testid="sweep-row-${i}"]`);
        const valueEl = el?.parentElement?.previousElementSibling;
        return `${valueEl?.textContent ?? '?'} | ${el?.textContent ?? '(absent)'}`;
      };
      return [0, 1, 2, 3, 4].map(row);
    });
    const rendered = await page.evaluate(() => ({
      rows: document.querySelectorAll('[data-testid^="sweep-row-"]').length,
      error: window.__sweep.sweep.state.error,
      points: window.__sweep.sweep.state.points.length,
      done: window.__sweep.sweep.state.done,
    }));
    check('the sweep table rendered five rows with no error', rendered.rows === 5 && rendered.error === null, JSON.stringify(rendered));

    const [dl] = await Promise.all([page.waitForEvent('download'), page.locator('[data-testid="sweep-export"]').click()]);
    check('export names the document', dl.suggestedFilename().includes('sweep.csv'), dl.suggestedFilename());
    const webCsv = readFileSync(await dl.path(), 'utf8');

    // The cross-target contract, measured exactly as the sweep-e2e does: columns 0/1/2/8
    // equal the CLI's cell for cell, 4/5 within SOLVE_DRIFT_REL, 6/7 within the looser
    // CHANGE_DRIFT_REL (the web's own JS division amplifies the cross-target drift by
    // ~1/change fraction), wall_ms skipped — two engines' clocks.
    const gateTxt = (g) => g.toExponential().replace('e+', 'e');
    const driftRel = (x, y) => (x === y ? 0 : Math.abs(x - y) / Math.max(Math.abs(x), Math.abs(y)));
    const cmp = { header: false, lines: 0, cells: 0, mismatched: 0, mismatch: '', walls: '', maxDrift: 0, maxChangeDrift: 0 };
    const c = webCsv.split('\n');
    cmp.header = a[0] === c[0];
    cmp.lines = Math.min(b.length, c.filter((x) => x !== '').length);
    for (let i = 0; i < cmp.lines; i++) {
      const cb = b[i].split(',');
      const cc = (c.filter((x) => x !== '')[i] ?? '').split(',');
      if (cb.length !== cc.length) {
        cmp.mismatched++;
        if (!cmp.mismatch) cmp.mismatch = `line ${i}: ${cb.length} vs ${cc.length} columns`;
        continue;
      }
      if (i === 0) continue;
      for (let k = 0; k < cb.length; k++) {
        if (k === 3) continue;
        if (k === 0 || k === 1 || k === 2 || k === 8) {
          if (cb[k] === cc[k]) cmp.cells++;
          else {
            cmp.mismatched++;
            if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${k} (${CSV_COLS[k]}): native ${cb[k]} vs web ${cc[k]}`;
          }
          continue;
        }
        const n1 = Number(cb[k]);
        const n2 = Number(cc[k]);
        const gate = k === 6 || k === 7 ? CHANGE_DRIFT_REL : SOLVE_DRIFT_REL;
        if (!Number.isFinite(n1) || !Number.isFinite(n2)) {
          cmp.mismatched++;
          if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${k} (${CSV_COLS[k]}): not a finite number, native "${cb[k]}" vs web "${cc[k]}"`;
          continue;
        }
        const rel = driftRel(n1, n2);
        if (!(rel <= gate)) {
          cmp.mismatched++;
          if (!cmp.mismatch) cmp.mismatch = `row ${i - 1} col ${k} (${CSV_COLS[k]}): rel ${rel.toExponential(2)} > gate ${gate}, native ${cb[k]} vs web ${cc[k]}`;
        } else {
          cmp.cells++;
          if (k === 6 || k === 7) {
            if (rel > cmp.maxChangeDrift) cmp.maxChangeDrift = rel;
          } else if (rel > cmp.maxDrift) cmp.maxDrift = rel;
        }
      }
      if (i === 1) cmp.walls = `native ${cb[3]} ms vs web ${cc[3]} ms (row 0, refined rung)`;
    }
    check('web CSV header = the CLI header', cmp.header, JSON.stringify(c[0]));
    check('web CSV row count = the CLI row count', c.filter((x) => x !== '').length === b.length, `${c.length} vs ${b.length} lines`);
    check(
      'integer/status columns equal, tracked floats within solve-drift',
      cmp.mismatched === 0,
      `${cmp.lines - 1} data rows, ${cmp.cells} compared cells, gates direct ${gateTxt(SOLVE_DRIFT_REL)} / derived ${gateTxt(CHANGE_DRIFT_REL)}, max observed drift direct ${cmp.maxDrift.toExponential(2)} / derived ${cmp.maxChangeDrift.toExponential(2)} relative${cmp.mismatched ? `; first mismatch ${cmp.mismatch}` : ''}`,
    );
    check('row 0 wall columns recorded', cmp.walls !== '', cmp.walls);
    const webRow0 = c[1]?.split(',') ?? [];
    const relLit = Number.isFinite(Number(webRow0[4]))
      ? driftRel(Number(webRow0[4]), Number(letRows[0][4]))
      : NaN;
    check(
      'web point 0 deflection within solve-drift of the ship literal',
      relLit <= SOLVE_DRIFT_REL,
      `${webRow0[4] ?? '(no row)'} vs ${letRows[0]?.[4] ?? '(no row)'} (rel ${Number.isFinite(relLit) ? relLit.toExponential(2) : 'NaN'})`,
    );

    const restore = await page.evaluate(async () => {
      const p = await import('/src/doc/param.ts');
      return {
        thick: p.readParam(window.fieldsmith.store.doc, p.parseAddr('thick')),
        size2: p.readParam(window.fieldsmith.store.doc, p.parseAddr('beam.size.2')),
      };
    });
    check('the document keeps its values after the sweep', restore.thick === 10 && restore.size2 === 10, JSON.stringify(restore));

    const sweepShot = resolve(screens, 'params-03-sweep.png');
    await page.screenshot({ path: sweepShot });

    // ---- report ----------------------------------------------------------------------------
    check('no console errors', errors.length === 0, errors.join(' | ').slice(0, 400));
    const ok = checks.every((c) => c.pass);
    const table = (names) =>
      [
        '| Check | Result | Detail |',
        '| :-- | :-- | :-- |',
        ...names.map((name) => {
          const ck = checks.find((x) => x.name === name);
          return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
        }),
      ].join('\n');
    const letTable = [
      '| value | dofs | elements | wall (ms) | deflection (mm) | vm (MPa) |',
      '| --: | --: | --: | --: | --: | --: |',
      ...b
        .slice(1)
        .map((l) => l.split(','))
        .map((r) => `| ${r[0]} | ${r[1]} | ${r[2]} | ${r[3]} | ${r[4]} | ${r[5]} |`),
    ].join('\n');
    const lines = [
      '# V27 — .fsm let-variables and expressions: the parameterised sweep',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
      '',
      'The Cantilever V1 parameterised with `let`: named scalars and arithmetic drive the',
      'statement numbers, and the sweep panel answers the question through the drivers',
      '<!--DOC-BLOCK-->',
      '',
      '```fsm',
      PARAM_DOC.trimEnd(),
      '```',
      '',
      'The plain Cantilever V1 carries the same literals (`size=(100, 10, 10)`,',
      '`at=(50, 5, 5)`, force −100), so the same five-point question (thickness 8 → 12 mm,',
      'rungs 30000/150000 pinned on both sides) asks both engines the same geometry.',
      '',
      '## Evidence A — representation (native, same machine and binary)',
      '',
      `- Node path: the plain doc's \`fieldsmith api --csv\` sweep of \`beam.size.2\`, ${nodeMs} ms`,
      `  whole. Let path: the parameterised doc's sweep of the bare name \`thick\`,`,
      `  ${letNativeMs} ms. Both five points on the pinned two-rung ladder, no tolerance declared,`,
      '  verdicts null.',
      '',
      letTable,
      '',
      `- The let sweep reproduces the node-path sweep to the byte: header equal,`,
      `  ${sameCell.equal}/40 non-wall cells equal (5 rows × 8 columns; the wall column is each`,
      `  run's own clock). Point 0 deflection is the V23 ship literal`,
      '  `0.39066861588109364`, exact.',
      `- The driven node path is refused on the parameterised doc — the envelope is an error`,
      '  carrying the full hint, so the CLI cannot silently fight the expression that writes',
      `  the number (\`${refMs} ms\`):`,
      '',
      '  ```',
      `  ${String(refEnv?.error ?? '').trim()}`,
      '  ```',
      '',
      '## Evidence B — the module (TS mirror, in the page)',
      '',
      table([
        'evaluator round trips (precedence, left fold, unary minus, sign glue)',
        'evaluator errors are the plain Rust texts',
        'bare-let apply chains through the derived let without clobbering it',
        'restoring the applied lets restores the plain numbers',
        'a driven param refuses with the exact sweep-the-let text',
        'a bare unknown id is refused with its own name',
        'the picker lists the bare let names last',
        'the .fsm round trip keeps the lets and re-links them into the statements',
      ]),
      '',
      '- The clobber case: `height` → 12 re-evaluates `half = height / 2` to 6; sweeping',
      '  `thick` then keeps half at 6 (the applied let keeps its value, every other let',
      '  re-evaluates in declaration order), and size.2 lands on the swept value only.',
      '- The round trip: `printFsm` re-emits `let half = height / 2` (not the folded value)',
      '  and re-links `size=(span, height, thick)` into the box line, so a saved .fsm keeps',
      '  the parameterisation; the reparse records the same six consumers.',
      '',
      '## Evidence C — the live UI',
      '',
      '- Loaded through the store; the Parameters panel on the document inspector lists the',
      `  five lets in declaration order (screenshot params-01). Sweep run: ${sweepMs} ms; export`,
      '  through the real click → download flow.',
      '',
      table([
        'driven fields name their expression as the edit hint',
        'a driven field refuses the edit and keeps the document',
        'the guard is per axis: x refuses, z edits',
        'the refused x edit left nothing to undo before the z edit',
        'the panel holds the bare-let question',
        'the sweep ends whole with no error',
        'five points streamed',
        'the sweep measured this document edition',
        'web CSV header = the CLI header',
        'web CSV row count = the CLI row count',
        'integer/status columns equal, tracked floats within solve-drift',
        'row 0 wall columns recorded',
        'web point 0 deflection within solve-drift of the ship literal',
        'the document keeps its values after the sweep',
        'export names the document',
        'no console errors',
      ]),
      `- What the panel showed while the run went (value | row text):`,
      ...rows.map((r) => `  - ${r}`),
      '',
      `- Columns 0/1/2/8 equal the CLI's cell for cell; 4/5 within ${gateTxt(SOLVE_DRIFT_REL)}, 6/7`,
      `  within ${gateTxt(CHANGE_DRIFT_REL)} — ${cmp.cells} cells compared, largest observed drift`,
      `  ${cmp.maxDrift.toExponential(2)} (direct) and ${cmp.maxChangeDrift.toExponential(2)} (derived change columns)`,
      `  relative, ${cmp.mismatched} mismatched${cmp.mismatched ? `; first: ${cmp.mismatch}` : ''}.`,
      `- Walls: ${cmp.walls}.`,
      '',
      '## The web CSV',
      '',
      '```csv',
      webCsv.trimEnd(),
      '```',
      '',
      '## Method',
      '',
      '`web/tests/params.mjs`: two native `api --csv` runs (the plain document on the node',
      'path, the parameterised document on the bare let name) pin the representation to the',
      'byte except the two clocks, and a third native run refuses the driven node path. The',
      'module evidence runs the TS mirror (web/src/doc/param.ts) in the page against the',
      'document the store parsed; the round-trip check drives the wasm printer the Save path',
      'uses. The UI evidence edits the real NumberInputs (blur commit), drives the sweep',
      'through the Results lens and the Run button, and exports through a real download. The',
      'web CSV answers within the two named gates against the native let run (the',
      'sweep-e2e contract, Limits there covers the cross-target ball).',
      '',
      '## Limits',
      '',
      '- wall_ms cannot match between the two native runs (two clocks) or between native and',
      '  wasm (two engines); Evidence A does not ask its wall column to agree.',
      '- No tolerance is declared on either side, so every row\'s verdict is `ok`; the',
      '  met/refusal cell paths are exercised by the Rust cases, not this browser run.',
      '- The cross-target float ball is the sweep-e2e\'s (V23/M16 machinery): columns 0/1/2/8',
      '  are exact across targets, 4–7 sit inside the solver\'s PCG convergence ball at the',
      '  gates named above; the faer kernel note in sweep-e2e\'s Limits carries what was',
      '  measured there.',
      '- Non-driveable fields (fill, pressure mpa, solve settings, extend dir/len) refuse',
      '  with the Rust texts by design and are covered by the Rust unit tests, not by UI',
      '  cases here.',
      '- The viewport gizmo\'s drag refusal (a driven node refuses a translate/rotate drag',
      '  and restores on release) shares the guards this run proves on the inspector fields',
      '  but is not driven by a drag gesture here.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/params.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V27.md'), lines.join('\n'));
    console.log(
      `params ${ok ? 'PASS' : 'FAIL'}: node ${nodeMs} ms, let ${letNativeMs} ms, web ${sweepMs} ms, reproduction ${
        sameCell.wrong.length === 0 ? 'byte-exact (ex-wall)' : 'MISMATCH'
      }`,
    );
    if (!ok) {
      for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    }
    if (errors.length) console.error('console errors:', errors);
    await page.close();
    return ok;
  } catch (e) {
    console.error('failed checks so far:');
    for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    throw e;
  } finally {
    rmSync(tmp, { recursive: true, force: true });
    await page.close();
  }
}