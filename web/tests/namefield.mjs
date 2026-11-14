// Entity-rename commit lifecycle (web UI): NameField is the editable id in the selected
// shape's inspector header (data-testid="name"). Renaming an entity rewrites every reference
// to it, so a stray commit is costly. This suite drives the real field (no FE solve) and gates
// the keyboard affordances a user relies on:
//   - Enter commits ONCE (one undo step reverts) and the selection follows the new id;
//   - blur commits ONCE;
//   - Escape CANCELS: the id is unchanged, the document is NOT mutated, no undo entry, the
//     input reverts and the selection stays put;
//   - Escape after a prior commit reverts to the CURRENT id (the focus session re-arms);
//   - a whitespace-only edit reverted by Escape leaves the document untouched.
//
// The defect this gates was observed before the fix: Escape does setText(id) then blurs, but
// the blur fires synchronously — before the reverted text state lands — so onBlur->commit()
// still read the edited draft and renamed the entity (beam->beam2, docVersion+1, an undo entry
// pushed, selection dragged to beam2). Enter and blur were already single-commit. A cancel flag
// now makes Escape's trailing blur skip the commit. Same race and same fix as NumberInput
// (web/tests/numinput.mjs); NameField blurs rather than unmounts, so it uses the cancel-ref form.
//
// Run with node tests/run-e2e.mjs --only namefield.
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

/** Select the beam so its shape inspector (the NameField rename input) renders. */
async function selectName(page) {
  await page.evaluate(() => window.fieldsmith.store.select({ kind: 'node', id: 'beam' }));
  await page.waitForSelector('[data-testid="name"]', { timeout: 10000 });
}

/** Self-contained in-page reader: the rename-field state the assertions check. */
function readNameState() {
  const s = window.fieldsmith.store;
  const d = s.doc;
  const ids = d.root.map((x) => x.id);
  const sel = s.selection;
  const nameInp = document.querySelector('[data-testid="name"]');
  const toastEl = document.querySelector('[data-testid="toast"]');
  return {
    v: s.docVersion,
    hasBeam: ids.includes('beam'),
    hasBeam2: ids.includes('beam2'),
    hasBeam3: ids.includes('beam3'),
    canUndo: s.canUndo(),
    undoLabel: s.undoLabel?.() ?? '',
    selId: sel ? sel.id : null,
    selKind: sel ? sel.kind : null,
    nameInput: nameInp?.value ?? null,
    toastVisible: !!toastEl,
    toastText: toastEl?.textContent ?? '',
  };
}

export async function namefieldTest(browser, url, { docsDir }) {
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
  const name = () => page.locator('[data-testid="name"]');
  const undo = async () => {
    await page.evaluate(() => window.fieldsmith.store.undo());
    await settle();
  };

  try {
    // ---- A. Escape cancels a rename (the defect) -------------------------------------------
    await loadDoc(page, url);
    await selectName(page);
    let before = await page.evaluate(readNameState);
    await name().click();
    await name().fill('beam2');
    await name().press('Escape');
    await settle();
    let st = await page.evaluate(readNameState);
    check('Escape rename: node id unchanged', st.hasBeam && !st.hasBeam2, `beam=${st.hasBeam} beam2=${st.hasBeam2}`);
    check('Escape rename: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);
    check('Escape rename: no undo history added', st.canUndo === false, `canUndo=${st.canUndo} (${st.undoLabel})`);
    check('Escape rename: input reverted to beam', st.nameInput === 'beam', `input=${st.nameInput}`);
    check('Escape rename: selection stays beam', st.selId === 'beam', `sel=${st.selKind}:${st.selId}`);
    check('Escape rename: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- B. Enter commits a rename once (regression guard) ---------------------------------
    await loadDoc(page, url);
    await selectName(page);
    before = await page.evaluate(readNameState);
    await name().click();
    await name().fill('beam2');
    await name().press('Enter');
    await settle();
    st = await page.evaluate(readNameState);
    check('Enter rename: node renamed to beam2', st.hasBeam2 && !st.hasBeam, `beam=${st.hasBeam} beam2=${st.hasBeam2}`);
    check('Enter rename: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    check('Enter rename: undo label is the rename', st.undoLabel.includes('Rename beam to beam2'), `label=${st.undoLabel}`);
    check('Enter rename: selection follows to beam2', st.selId === 'beam2', `sel=${st.selKind}:${st.selId}`);
    await undo();
    st = await page.evaluate(readNameState);
    check('Enter rename: one undo restores beam', st.hasBeam && !st.hasBeam2 && st.canUndo === false, `beam=${st.hasBeam} canUndo=${st.canUndo}`);

    // ---- C. Blur commits a rename once (regression guard) ----------------------------------
    await loadDoc(page, url);
    await selectName(page);
    before = await page.evaluate(readNameState);
    await name().click();
    await name().fill('beam2');
    await name().press('Tab'); // blur by moving focus to the next field
    await settle();
    st = await page.evaluate(readNameState);
    check('Blur rename: node renamed to beam2', st.hasBeam2 && !st.hasBeam, `beam=${st.hasBeam} beam2=${st.hasBeam2}`);
    check('Blur rename: single history entry', st.v - before.v === 1, `docVersion delta ${st.v - before.v}`);
    await undo();
    st = await page.evaluate(readNameState);
    check('Blur rename: one undo restores beam', st.hasBeam && !st.hasBeam2, `beam=${st.hasBeam} beam2=${st.hasBeam2}`);

    // ---- D. Escape reverts to the CURRENT id after a prior commit (session re-arm) ---------
    await loadDoc(page, url);
    await selectName(page);
    await name().click();
    await name().fill('beam2');
    await name().press('Enter'); // commits beam -> beam2
    await settle();
    const afterEnter = await page.evaluate(readNameState);
    await name().click();
    await name().fill('beam3');
    await name().press('Escape'); // must revert to beam2, not beam3 and not beam
    await settle();
    st = await page.evaluate(readNameState);
    check('Escape after commit: keeps committed beam2', st.hasBeam2 && !st.hasBeam3, `beam2=${st.hasBeam2} beam3=${st.hasBeam3}`);
    check('Escape after commit: no extra history', st.v === afterEnter.v, `docVersion ${afterEnter.v}->${st.v}`);
    check('Escape after commit: input shows beam2', st.nameInput === 'beam2', `input=${st.nameInput}`);

    // ---- E. Escape on a whitespace-only edit reverts (guard) -------------------------------
    await loadDoc(page, url);
    await selectName(page);
    before = await page.evaluate(readNameState);
    await name().click();
    await name().fill('   ');
    await name().press('Escape');
    await settle();
    st = await page.evaluate(readNameState);
    check('Escape whitespace: input reverted to beam', st.nameInput === 'beam', `input=${st.nameInput}`);
    check('Escape whitespace: document not mutated', st.v === before.v, `docVersion ${before.v}->${st.v}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Entity-rename commit lifecycle (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      '`NameField` is the editable id in the selected shape\'s inspector header. A rename',
      'rewrites every reference to the entity, so a stray commit is costly. This suite drives',
      'the real field on a selected box with no FE solve. The field commits on blur. Enter',
      'blurs (one commit); blur commits once. Escape reverts the text then blurs, but that blur',
      'fires synchronously before the reverted text state lands, so it used to commit the edited',
      'draft and rename the entity; a cancel flag now makes Escape\'s trailing blur skip the',
      'commit. This is the same race and the same cancel-ref fix as `NumberInput`',
      '(numinput-e2e.md); `NameField` blurs rather than unmounts.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Drives the one `NameField` render site (the selected shape\'s id); this is a document',
      '  edit, so no FE solve is run and no field result is asserted.',
      '- Reference rewriting on a successful rename is `renameId`\'s job and is not re-asserted',
      '  here; the gate checks the id, docVersion, undo step and selection follow-through.',
      '- Enter/blur single-commit is asserted via docVersion delta 1 + single-undo restore,',
      '  not by counting onChange calls.',
      '- The toast is a single replaceable slot (`ui.notify` overwrites); checks assert presence.',
      '- A rename that collides with an existing id (the `renameId` error path) is not exercised.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/namefield.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['namefield: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'namefield-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`namefield: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}