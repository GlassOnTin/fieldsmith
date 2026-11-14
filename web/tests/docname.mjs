// Document-name commit lifecycle (web UI): the doc-name input in the top toolbar
// (class "doc-name", aria-label "Document name"). Unlike the entity-rename NameField,
// this field is always mounted and renames the whole document. This suite drives the
// real field with character-by-character typing (pressSequentially, not fill) so it can
// tell a per-keystroke commit model from a commit-on-blur one, and gates the keyboard
// affordances a user relies on:
//   - typing does NOT commit until Enter or blur (one undo step, not one per character);
//   - Enter commits ONCE;
//   - blur commits ONCE;
//   - Escape CANCELS: the name is unchanged, the document is NOT mutated, no undo entry,
//     and the input reverts to the current name;
//   - Escape after a prior commit reverts to the CURRENT name (the focus session re-arms);
//   - clearing the field commits the "Untitled" fallback once;
//   - undo re-syncs the visible input to the restored name.
//
// Scenario A records a mid-typing snapshot (before any blur) in its check detail so the
// report shows the observed commit model, not an assumed one. The app uses plain Preact
// (onChange is the native change event, which fires on blur for a text input), so the
// defect under test is expected to be Escape being a no-op that leaves the draft to
// commit on a later blur, plus a controlled value that diverges from the DOM while typing.
//
// Run with node tests/run-e2e.mjs --only docname.
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

/** Self-contained in-page reader: the doc-name field state the assertions check. */
function readDocNameState() {
  const s = window.fieldsmith.store;
  const inp = document.querySelector('input.doc-name');
  const toastEl = document.querySelector('[data-testid="toast"]');
  return {
    v: s.docVersion,
    name: s.doc.name,
    canUndo: s.canUndo(),
    undoLabel: s.undoLabel?.() ?? '',
    input: inp?.value ?? null,
    toastVisible: !!toastEl,
    toastText: toastEl?.textContent ?? '',
  };
}

export async function docnameTest(browser, url, { docsDir }) {
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
  const field = () => page.locator('input.doc-name');
  const undo = async () => {
    await page.evaluate(() => window.fieldsmith.store.undo());
    await settle();
  };

  try {
    // ---- A. Escape cancels a rename; mid-typing snapshot records the commit model --------
    await loadDoc(page, url);
    let before = await page.evaluate(readDocNameState);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket');
    await settle();
    const mid = await page.evaluate(readDocNameState); // BEFORE any blur: did typing commit?
    await field().press('Escape');
    await field().press('Tab'); // force the blur the current Escape skips
    await settle();
    let st = await page.evaluate(readDocNameState);
    check('Escape: name unchanged', st.name === 'Cantilever V1', `name=${st.name}`);
    check(
      'Escape: document not mutated',
      st.v === before.v,
      `docVersion ${before.v}->${st.v} (midType v=${mid.v} name=${mid.name} input=${mid.input})`,
    );
    check('Escape: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape: input reverted', st.input === 'Cantilever V1', `input=${st.input}`);
    check('Escape: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- B. Enter commits once (regression guard) ----------------------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readDocNameState);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket');
    await field().press('Enter');
    await settle();
    st = await page.evaluate(readDocNameState);
    check('Enter: name committed', st.name === 'Bracket', `name=${st.name}`);
    check('Enter: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    check('Enter: undo label is the rename', st.undoLabel.includes('Rename document'), `label=${st.undoLabel}`);
    await undo();
    st = await page.evaluate(readDocNameState);
    check('Enter: one undo restores', st.name === 'Cantilever V1' && st.canUndo === false, `name=${st.name} canUndo=${st.canUndo}`);

    // ---- C. Blur commits once (regression guard) -----------------------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readDocNameState);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket');
    await field().press('Tab');
    await settle();
    st = await page.evaluate(readDocNameState);
    check('Blur: name committed', st.name === 'Bracket', `name=${st.name}`);
    check('Blur: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readDocNameState);
    check('Blur: one undo restores', st.name === 'Cantilever V1', `name=${st.name}`);

    // ---- D. Escape reverts to the CURRENT name after a prior commit (session re-arm) ------
    await loadDoc(page, url);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket');
    await field().press('Enter'); // commits Cantilever V1 -> Bracket
    await settle();
    const afterEnter = await page.evaluate(readDocNameState);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket2');
    await field().press('Escape');
    await field().press('Tab');
    await settle();
    st = await page.evaluate(readDocNameState);
    check('Escape after commit: keeps committed Bracket', st.name === 'Bracket', `name=${st.name}`);
    check('Escape after commit: no extra history', st.v === afterEnter.v, `docVersion ${afterEnter.v}->${st.v}`);
    check('Escape after commit: input shows Bracket', st.input === 'Bracket', `input=${st.input}`);

    // ---- E. Clearing the field commits the Untitled fallback once -------------------------
    await loadDoc(page, url);
    before = await page.evaluate(readDocNameState);
    await field().click();
    await field().press('Control+A');
    await field().press('Backspace');
    await field().press('Enter');
    await settle();
    st = await page.evaluate(readDocNameState);
    check('Clear: name falls back to Untitled', st.name === 'Untitled', `name=${st.name}`);
    check('Clear: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);

    // ---- F. Undo re-syncs the visible input to the restored name --------------------------
    await loadDoc(page, url);
    await field().click();
    await field().press('Control+A');
    await field().pressSequentially('Bracket');
    await field().press('Enter');
    await settle();
    await undo();
    st = await page.evaluate(readDocNameState);
    check('Undo: input re-synced', st.input === 'Cantilever V1', `input=${st.input}`);
    check('Undo: name restored', st.name === 'Cantilever V1', `name=${st.name}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Document-name commit lifecycle (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The `doc-name` input in the top toolbar renames the whole document. This suite drives',
      'the real field with character-by-character typing (`pressSequentially`) and no FE solve,',
      'so it can distinguish a per-keystroke commit from a commit-on-blur one. Typing must not',
      'commit until Enter or blur (one undo step, not one per character); Enter and blur each',
      'commit once; Escape cancels and reverts the input without mutating the document; Escape',
      'after a commit reverts to the current name; clearing the field commits the `Untitled`',
      'fallback once; and undo re-syncs the visible input. Scenario A records a mid-typing',
      'snapshot before any blur so the report shows the observed commit model.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Drives the one `doc-name` render site in the toolbar; this is a document edit, so no',
      '  FE solve is run and no field result is asserted.',
      '- Enter/blur single-commit is asserted via docVersion delta 1 + single-undo restore,',
      '  not by counting onInput/onChange calls.',
      '- The toast is a single replaceable slot (`ui.notify` overwrites); checks assert presence.',
      '- The mid-typing snapshot assumes a quiet page (no unrelated re-render between keystrokes);',
      '  a re-render mid-typing would snap the controlled value back, which is itself the defect.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/docname.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['docname: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'docname-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`docname: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}