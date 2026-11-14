// Setup-lens authoring text fields (web UI): the three commit-on-blur text inputs in the
// load inspector — TextInputRow (data-testid="load-case"), SpatialFnField ("pressure-fn")
// and FnField ("load-fn"). Each keeps a `dirty` draft and commits on blur; Enter blurs.
// None had an Escape branch, so Escape was swallowed (unconditional stopPropagation) and
// the draft survived to commit on the NEXT blur. This suite drives the real fields (no FE
// solve) and gates the keyboard affordances a user relies on:
//   - Escape CANCELS: the draft is reverted, the document is NOT mutated, no undo entry,
//     the input reverts; a following Tab (which forces the blur Escape skipped) must NOT
//     commit the discarded draft;
//   - Enter commits ONCE (one undo step reverts);
//   - blur (Tab) commits ONCE;
//   - Escape after a prior commit reverts to the CURRENT value (the focus session re-arms).
//
// The defect this gates was observed before the fix: with no Escape handler, fill+Escape
// left the draft in place and the trailing Tab blurred into commit(), writing the draft
// (load case "Load 1"->"caseA", docVersion+1, an undo entry pushed). The fix is the same
// cancel-ref idiom as NumberInput (numinput.mjs) and NameField (namefield.mjs): Escape sets
// a synchronous cancel flag, reverts the text and blurs; the blur honours the flag once and
// skips the commit. These fields blur and stay mounted, so they use the cancel-ref form.
//
// Run with node tests/run-e2e.mjs --only setupfields.
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
load press tip pressure=1
load dist tip fn="0.01" dir=(0, 0, -1)
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

/** Select a load (setup lens) so its inspector renders, and wait for the field under test. */
async function selectLoad(page, id, testId) {
  await page.evaluate((lid) => {
    window.fieldsmith.store.setLens('setup');
    window.fieldsmith.store.select({ kind: 'load', id: lid });
  }, id);
  await page.waitForSelector(`[data-testid="${testId}"]`, { timeout: 10000 });
}

/** Self-contained in-page reader: the three loads' document state plus the field under test. */
function readState(testId) {
  const s = window.fieldsmith.store;
  const d = s.doc;
  const find = (id) => d.loads.find((x) => x.id === id);
  const push = find('push');
  const press = find('press');
  const dist = find('dist');
  const sel = s.selection;
  const inp = document.querySelector(`[data-testid="${testId}"]`);
  const toastEl = document.querySelector('[data-testid="toast"]');
  return {
    v: s.docVersion,
    canUndo: s.canUndo(),
    undoLabel: s.undoLabel?.() ?? '',
    pushCase: push ? push.case ?? '' : null,
    pressExpr: press && press.kind.type === 'pressure' ? press.kind.expr ?? '' : null,
    distExpr: dist && dist.kind.type === 'distributed' ? dist.kind.expr ?? '' : null,
    selId: sel ? sel.id : null,
    selKind: sel ? sel.kind : null,
    inputValue: inp ? inp.value : null,
    toastVisible: !!toastEl,
    toastText: toastEl ? toastEl.textContent : '',
  };
}

export async function setupfieldsTest(browser, url, { docsDir }) {
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
  const field = (testId) => page.locator(`[data-testid="${testId}"]`);
  const undo = async () => {
    await page.evaluate(() => window.fieldsmith.store.undo());
    await settle();
  };

  try {
    // ---- A. TextInputRow / load-case: Escape cancels (the defect) --------------------------
    await loadDoc(page, url);
    await selectLoad(page, 'push', 'load-case');
    let before = await page.evaluate(readState, 'load-case');
    await field('load-case').click();
    await field('load-case').fill('caseA');
    await field('load-case').press('Escape');
    await field('load-case').press('Tab'); // force the blur Escape skipped
    await settle();
    let st = await page.evaluate(readState, 'load-case');
    check('Escape case: load case unchanged', st.pushCase === 'Load 1', `case=${st.pushCase}`);
    check('Escape case: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape case: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape case: input reverted', st.inputValue === 'Load 1', `input=${st.inputValue}`);
    check('Escape case: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- B. TextInputRow / load-case: Enter commits once (regression guard) ----------------
    await loadDoc(page, url);
    await selectLoad(page, 'push', 'load-case');
    before = await page.evaluate(readState, 'load-case');
    await field('load-case').click();
    await field('load-case').fill('caseA');
    await field('load-case').press('Enter');
    await settle();
    st = await page.evaluate(readState, 'load-case');
    check('Enter case: case committed', st.pushCase === 'caseA', `case=${st.pushCase}`);
    check('Enter case: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    check('Enter case: undo label is the case edit', st.undoLabel.includes('Set load case'), `label=${st.undoLabel}`);
    await undo();
    st = await page.evaluate(readState, 'load-case');
    check('Enter case: one undo restores', st.pushCase === 'Load 1' && st.canUndo === false, `case=${st.pushCase} canUndo=${st.canUndo}`);

    // ---- C. TextInputRow / load-case: blur commits once (regression guard) -----------------
    await loadDoc(page, url);
    await selectLoad(page, 'push', 'load-case');
    before = await page.evaluate(readState, 'load-case');
    await field('load-case').click();
    await field('load-case').fill('caseA');
    await field('load-case').press('Tab');
    await settle();
    st = await page.evaluate(readState, 'load-case');
    check('Blur case: case committed', st.pushCase === 'caseA', `case=${st.pushCase}`);
    check('Blur case: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readState, 'load-case');
    check('Blur case: one undo restores', st.pushCase === 'Load 1', `case=${st.pushCase}`);

    // ---- D. TextInputRow / load-case: Escape after a commit re-arms ------------------------
    await loadDoc(page, url);
    await selectLoad(page, 'push', 'load-case');
    await field('load-case').click();
    await field('load-case').fill('caseA');
    await field('load-case').press('Enter'); // commits Load 1 -> caseA
    await settle();
    const afterEnter = await page.evaluate(readState, 'load-case');
    await field('load-case').click();
    await field('load-case').fill('caseB');
    await field('load-case').press('Escape'); // must revert to caseA, not caseB and not Load 1
    await field('load-case').press('Tab');
    await settle();
    st = await page.evaluate(readState, 'load-case');
    check('Escape after commit: keeps committed caseA', st.pushCase === 'caseA', `case=${st.pushCase}`);
    check('Escape after commit: no extra history', st.v === afterEnter.v, `docVersion ${afterEnter.v}->${st.v}`);
    check('Escape after commit: input shows caseA', st.inputValue === 'caseA', `input=${st.inputValue}`);

    // ---- E. SpatialFnField / pressure-fn: Escape cancels (the defect) ----------------------
    await loadDoc(page, url);
    await selectLoad(page, 'press', 'pressure-fn');
    before = await page.evaluate(readState, 'pressure-fn');
    await field('pressure-fn').click();
    await field('pressure-fn').fill('0.5*(20 - z)');
    await field('pressure-fn').press('Escape');
    await field('pressure-fn').press('Tab');
    await settle();
    st = await page.evaluate(readState, 'pressure-fn');
    check('Escape profile: pressure expr unchanged', st.pressExpr === '', `expr=${st.pressExpr}`);
    check('Escape profile: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape profile: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape profile: input reverted', st.inputValue === '', `input=${st.inputValue}`);

    // ---- F. SpatialFnField / pressure-fn: Enter commits once (regression guard) ------------
    await loadDoc(page, url);
    await selectLoad(page, 'press', 'pressure-fn');
    before = await page.evaluate(readState, 'pressure-fn');
    await field('pressure-fn').click();
    await field('pressure-fn').fill('0.5*(20 - z)');
    await field('pressure-fn').press('Enter');
    await settle();
    st = await page.evaluate(readState, 'pressure-fn');
    check('Enter profile: expr committed', st.pressExpr === '0.5*(20 - z)', `expr=${st.pressExpr}`);
    check('Enter profile: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readState, 'pressure-fn');
    check('Enter profile: one undo restores', st.pressExpr === '', `expr=${st.pressExpr}`);

    // ---- G. FnField / load-fn: Escape cancels (the defect) ---------------------------------
    await loadDoc(page, url);
    await selectLoad(page, 'dist', 'load-fn');
    before = await page.evaluate(readState, 'load-fn');
    await field('load-fn').click();
    await field('load-fn').fill('0.02');
    await field('load-fn').press('Escape');
    await field('load-fn').press('Tab');
    await settle();
    st = await page.evaluate(readState, 'load-fn');
    check('Escape fn: distributed expr unchanged', st.distExpr === '0.01', `expr=${st.distExpr}`);
    check('Escape fn: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape fn: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape fn: input reverted', st.inputValue === '0.01', `input=${st.inputValue}`);

    // ---- H. FnField / load-fn: Enter commits once (regression guard) -----------------------
    await loadDoc(page, url);
    await selectLoad(page, 'dist', 'load-fn');
    before = await page.evaluate(readState, 'load-fn');
    await field('load-fn').click();
    await field('load-fn').fill('0.02');
    await field('load-fn').press('Enter');
    await settle();
    st = await page.evaluate(readState, 'load-fn');
    check('Enter fn: expr committed', st.distExpr === '0.02', `expr=${st.distExpr}`);
    check('Enter fn: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    check('Enter fn: undo label is the fn edit', st.undoLabel.includes('Set load fn'), `label=${st.undoLabel}`);
    await undo();
    st = await page.evaluate(readState, 'load-fn');
    check('Enter fn: one undo restores', st.distExpr === '0.01', `expr=${st.distExpr}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Setup-lens authoring text-field commit lifecycle (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The load inspector has three commit-on-blur text inputs: `TextInputRow` (the load',
      'case, `load-case`), `SpatialFnField` (the pressure profile, `pressure-fn`) and `FnField`',
      '(the distributed load fn, `load-fn`). Each keeps a `dirty` draft and commits on blur;',
      'Enter blurs. None had an Escape branch, so Escape was swallowed and the draft survived',
      'to commit on the next blur. This suite drives the real fields on three selected loads',
      'with no FE solve. Escape now sets a synchronous cancel flag, reverts the text and blurs;',
      'the blur honours the flag once and skips the commit. A following Tab forces the blur',
      'Escape skipped, so a discarded draft cannot leak into the document. This is the same',
      'cancel-ref fix as `NumberInput` (numinput-e2e.md) and `NameField` (namefield-e2e.md);',
      'these fields blur and stay mounted.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Drives one render site per component (load-case on a force load, pressure-fn on a',
      '  pressure load, load-fn on a distributed load). `TextInputRow` also renders the',
      '  contact case and the pressure/preload case rows; those share the component but are',
      '  not separately driven here.',
      '- These are document edits, so no FE solve is run and no field result is asserted.',
      '- Enter/blur single-commit is asserted via docVersion delta 1 + single-undo restore,',
      '  not by counting onInput calls.',
      '- The toast is a single replaceable slot (`ui.notify` overwrites); checks assert presence.',
      '- The spatial-fn / fn probe error path (an invalid expression) is not exercised; the',
      '  gate uses valid expressions so the commit path, not the probe, is under test.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/setupfields.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['setupfields: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'setupfields-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`setupfields: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}