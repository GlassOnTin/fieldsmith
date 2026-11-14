// Numeric-field commit lifecycle (web UI): NumberInput is the shared numeric control behind
// every dimension, load, let and profile point in the Inspector. This suite drives TWO real
// consumers of it (no FE solve) — a params-panel `let` field (literal and expression) and a
// selected box's corner-radius dimension — and gates the keyboard affordances a user relies on:
//   - Enter commits ONCE (one undo step reverts), for both a literal and a let-expression;
//   - blur commits ONCE;
//   - Escape CANCELS: the value/expression reverts, the document is NOT mutated, no undo entry;
//   - Escape after a prior commit reverts to the CURRENT value (the focus session re-arms);
//   - ArrowUp nudges by the field's step; a shape dimension commits on Enter (editability control).
//
// One defect this gates was observed before the fix: Escape does setText(fmt(value)) then blurs,
// but the blur fires synchronously — before the reverted text state lands — so onBlur->commit()
// still read the edited draft and wrote it (a literal 10->20, an expression 10->span/2). Enter and
// blur were already single-commit: a repeated Enter commit is a no-op at the store's transact
// idempotency guard. A cancel flag now makes Escape's trailing blur skip the commit.
//
// Run with node tests/run-e2e.mjs --only numinput.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const PARAM_DOC = `fieldsmith 1
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

/** Load a fresh PARAM_DOC into the running app and wait for the params panel. */
async function loadDoc(page, url) {
  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });
  await page.evaluate(async (fsm) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(fsm);
    if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
    window.fieldsmith.store.load(r.ok);
  }, PARAM_DOC);
  await page.waitForSelector('[data-testid="params-panel"]', { timeout: 60000 });
}

/** Select the beam so its shape inspector (corner-radius NumberInput) renders. */
async function selectBeam(page) {
  await page.evaluate(() => window.fieldsmith.store.select({ kind: 'node', id: 'beam' }));
  await page.waitForSelector('[data-testid="round"]', { timeout: 10000 });
}

/** Self-contained in-page reader: the numeric-field state the assertions check. */
function readNumState() {
  const s = window.fieldsmith.store;
  const d = s.doc;
  const thick = (d.lets ?? []).find((l) => l.name === 'thick');
  const beam = d.root.find((x) => x.id === 'beam');
  const toastEl = document.querySelector('[data-testid="toast"]');
  const thickInp = document.querySelector('[data-testid="let-input-thick"]');
  const roundInp = document.querySelector('[data-testid="round"]');
  return {
    v: s.docVersion,
    thickValue: thick?.value ?? null,
    thickExpr: thick?.expr ?? null,
    roundValue: beam?.kind?.round ?? null,
    canUndo: s.canUndo(),
    undoLabel: s.undoLabel?.() ?? '',
    toastVisible: !!toastEl,
    toastText: toastEl?.textContent ?? '',
    thickInput: thickInp?.value ?? null,
    roundInput: roundInp?.value ?? null,
  };
}

export async function numinputTest(browser, url, { docsDir }) {
  const errors = [];
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const table = (names) =>
    [
      '| Check | Result | Measured |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''}`.replace(/\n/g, ' ') + ' |';
      }),
    ].join('\n');

  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  const settle = () => page.waitForTimeout(500);
  const thick = () => page.locator('[data-testid="let-input-thick"]');
  const round = () => page.locator('[data-testid="round"]');
  const undo = async () => {
    await page.evaluate(() => window.fieldsmith.store.undo());
    await settle();
  };

  try {
    // ---- A. Escape cancels a LITERAL edit (the defect) -------------------------------------
    await loadDoc(page, url);
    let before = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('20');
    await thick().press('Escape');
    await settle();
    let st = await page.evaluate(readNumState);
    check('Escape literal: value reverted to 10', st.thickValue === 10, `thick=${st.thickValue}`);
    check('Escape literal: expression unchanged', st.thickExpr === '10', `expr=${st.thickExpr}`);
    check('Escape literal: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape literal: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape literal: input shows reverted value', st.thickInput === '10', `input=${st.thickInput}`);
    check('Escape literal: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- B. Escape cancels an EXPRESSION edit (the defect, expr path) ----------------------
    await loadDoc(page, url);
    before = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('= span / 2');
    await thick().press('Escape');
    await settle();
    st = await page.evaluate(readNumState);
    check('Escape expr: value reverted to 10', st.thickValue === 10, `thick=${st.thickValue}`);
    check('Escape expr: expression unchanged', st.thickExpr === '10', `expr=${st.thickExpr}`);
    check('Escape expr: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape expr: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape expr: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- C. Enter commits a LITERAL once (regression guard) --------------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('20');
    await thick().press('Enter');
    await settle();
    st = await page.evaluate(readNumState);
    check('Enter literal: value committed 20', st.thickValue === 20, `thick=${st.thickValue}`);
    check('Enter literal: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    check('Enter literal: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');
    await undo();
    st = await page.evaluate(readNumState);
    check('Enter literal: one undo restores 10', st.thickValue === 10 && st.canUndo === false, `thick=${st.thickValue} canUndo=${st.canUndo}`);

    // ---- D. Enter commits an EXPRESSION once (regression guard) ----------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('= span / 2');
    await thick().press('Enter');
    await settle();
    st = await page.evaluate(readNumState);
    check('Enter expr: value committed 50', st.thickValue === 50, `thick=${st.thickValue}`);
    check('Enter expr: expression bound', st.thickExpr === 'span / 2', `expr=${st.thickExpr}`);
    check('Enter expr: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readNumState);
    check('Enter expr: one undo restores 10', st.thickValue === 10 && st.thickExpr === '10' && st.canUndo === false, `thick=${st.thickValue} expr=${st.thickExpr}`);

    // ---- E. Blur commits a LITERAL once (regression guard) ---------------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('20');
    await page.locator('.insp-heading').click(); // blur by clicking elsewhere
    await settle();
    st = await page.evaluate(readNumState);
    check('Blur literal: value committed 20', st.thickValue === 20, `thick=${st.thickValue}`);
    check('Blur literal: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readNumState);
    check('Blur literal: one undo restores 10', st.thickValue === 10, `thick=${st.thickValue}`);

    // ---- F. Escape reverts to the CURRENT value after a prior commit (session re-arm) ------
    await loadDoc(page, url);
    await thick().click();
    await thick().fill('20');
    await thick().press('Enter'); // commits 20
    await settle();
    const afterEnter = await page.evaluate(readNumState);
    await thick().click();
    await thick().fill('30');
    await thick().press('Escape'); // must revert to 20, not 30 and not 10
    await settle();
    st = await page.evaluate(readNumState);
    check('Escape after commit: reverts to committed 20', st.thickValue === 20, `thick=${st.thickValue}`);
    check('Escape after commit: no extra history', st.v === afterEnter.v, `docVersion ${afterEnter.v}->${st.v}`);
    check('Escape after commit: input shows 20', st.thickInput === '20', `input=${st.thickInput}`);

    // ---- G. ArrowUp nudges by the field step (regression guard, untouched path) ------------
    await loadDoc(page, url);
    before = await page.evaluate(readNumState);
    await thick().click();
    await thick().press('ArrowUp'); // step 0.5 -> 10.5
    await settle();
    st = await page.evaluate(readNumState);
    check('ArrowUp: value nudged to 10.5', st.thickValue === 10.5, `thick=${st.thickValue}`);
    check('ArrowUp: document mutated', st.v > before.v, `docVersion ${before.v}->${st.v}`);

    // ---- I. Escape cancels a SHAPE-DIMENSION edit (breadth: second consumer, the defect) ---
    await loadDoc(page, url);
    await selectBeam(page);
    before = await page.evaluate(readNumState);
    await round().click();
    await round().fill('5');
    await round().press('Escape');
    await settle();
    st = await page.evaluate(readNumState);
    check('Escape shape dim: corner radius reverted to 0', st.roundValue === 0, `round=${st.roundValue}`);
    check('Escape shape dim: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape shape dim: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);

    // ---- J. Enter commits a SHAPE DIMENSION (editability control for I) --------------------
    await loadDoc(page, url);
    await selectBeam(page);
    before = await page.evaluate(readNumState);
    await round().click();
    await round().fill('5');
    await round().press('Enter');
    await settle();
    st = await page.evaluate(readNumState);
    check('Enter shape dim: corner radius committed 5', st.roundValue === 5, `round=${st.roundValue}`);
    check('Enter shape dim: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Numeric-field commit lifecycle (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      '`NumberInput` is the shared numeric control behind every dimension, load, `let` and',
      'profile point in the Inspector. This suite drives two real consumers of it — a',
      'params-panel `let` field (literal and expression) and a selected box\'s corner-radius',
      'dimension — with no FE solve. The field commits on blur. Enter commits then blurs; a',
      'repeated Enter commit is already a no-op at the store\'s transact idempotency guard, so',
      'Enter and blur each record one undo step. Escape reverts the text then blurs, but that',
      'blur fires synchronously before the reverted text state lands, so it used to commit the',
      'edited draft; a cancel flag now makes Escape\'s trailing blur skip the commit.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Drives the real `NumberInput` via two consumers (a `let` field and a shape dimension);',
      '  these are document edits, so no FE solve is run and no field result is asserted.',
      '- The fix lives in the shared component, so it applies to every numeric field; the gate',
      '  exercises two real consumers, not every consumer (loads, profile points are untested).',
      '- Enter/blur single-commit relies on the store\'s transact idempotency guard (an identical',
      '  re-commit is a no-op); the gate asserts docVersion delta 1 + single-undo restore rather',
      '  than counting onChange calls.',
      '- The toast is a single replaceable slot (`ui.notify` overwrites); checks assert presence.',
      '- Arrow-key nudge is tested at the let field\'s step (0.5); Shift/Alt multipliers and',
      '  label-scrub transient commits are not exercised here.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/numinput.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['numinput: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'numinput-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`numinput: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}