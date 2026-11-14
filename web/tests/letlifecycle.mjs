// Let-parameter lifecycle (web UI): the rename/delete affordances in the Inspector params
// panel are document edits the user reaches for constantly, yet had no end-to-end coverage.
// This suite drives the REAL panel (no FE solve) and gates the behaviours a user relies on:
//   - renaming via Enter commits once, propagates to consumers, and shows NO error toast;
//   - renaming via blur commits cleanly;
//   - Escape CANCELS the edit (no rename, no document mutation, no toast);
//   - invalid / reserved / colliding names are rejected with an error and leave the doc intact;
//   - deleting a parameter unbinds dependent consumers (keeping their last number) and freezes
//     derived lets to their last value;
//   - a renamed document survives print -> parse, and undo restores the original name.
//
// Two defects this gates were observed before the fix: the rename input commits on BOTH Enter
// and the blur that fires when it unmounts, so Enter ran a second stale rename (spurious
// "already in use" toast on a rename that had worked) and Escape's unmount-blur committed the
// edited draft instead of cancelling. A single disposition token now gates one commit-or-cancel.
//
// Run with node tests/run-e2e.mjs --only letlifecycle.
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

/** Self-contained in-page reader: the let lifecycle state the assertions check. */
function readLetState() {
  const s = window.fieldsmith.store;
  const d = s.doc;
  const toastEl = document.querySelector('[data-testid="toast"]');
  const beam = d.root.find((x) => x.id === 'beam');
  const half = (d.lets ?? []).find((l) => l.name === 'half');
  const consumed = {};
  for (const c of d.consumed ?? []) {
    const key = `${c.target}.${c.attr}${c.comp === null || c.comp === undefined ? '' : '.' + c.comp}`;
    consumed[key] = c.expr;
  }
  return {
    names: (d.lets ?? []).map((l) => l.name),
    docVersion: s.docVersion,
    toastVisible: !!toastEl,
    toastText: toastEl?.textContent ?? '',
    consumed,
    beamSize: beam?.kind?.size ?? null,
    halfExpr: half?.expr ?? null,
    halfValue: half?.value ?? null,
  };
}

/** Self-contained in-page reader: print the live doc and re-parse it (round-trip). */
async function readRoundTrip() {
  const w = await import('/src/doc/wasm.ts');
  const s = window.fieldsmith.store;
  const text = w.printFsm(s.doc);
  const r = w.parseFsm(text);
  if (!r.ok) return { ok: false, error: r.error?.message ?? 'parse failed', names: [], consumed: {} };
  const names = (r.ok.lets ?? []).map((l) => l.name);
  const consumed = {};
  for (const c of r.ok.consumed ?? []) {
    const key = `${c.target}.${c.attr}${c.comp === null || c.comp === undefined ? '' : '.' + c.comp}`;
    consumed[key] = c.expr;
  }
  return { ok: true, names, consumed };
}

async function openRename(page, name) {
  await page.locator(`[data-testid="rename-let-${name}"]`).click();
  await page.waitForSelector(`[data-testid="let-rename-input-${name}"]`, { timeout: 5000 });
  await page.locator(`[data-testid="let-rename-input-${name}"]`).fill('thickness');
}

export async function letlifecycleTest(browser, url, { docsDir }) {
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

  const settle = () => page.waitForTimeout(600);
  const sizeIs = (s, want) => Array.isArray(s) && s.length === 3 && s.every((v, i) => Math.abs(v - want[i]) < 1e-9);

  try {
    // ---- A. Enter-rename: commits once, propagates, NO spurious toast ----------------------
    await loadDoc(page, url);
    await openRename(page, 'thick');
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    let st = await page.evaluate(readLetState);
    check('Enter rename: parameter renamed thick->thickness', st.names.includes('thickness') && !st.names.includes('thick'), st.names.join(','));
    check('Enter rename: consumer beam.size.2 propagated', st.consumed['beam.size.2'] === 'thickness', `beam.size.2=${st.consumed['beam.size.2']}`);
    check('Enter rename: geometry preserved [100,10,10]', sizeIs(st.beamSize, [100, 10, 10]), JSON.stringify(st.beamSize));
    check('Enter rename: no spurious error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- B. Blur-rename: clean baseline ----------------------------------------------------
    await loadDoc(page, url);
    await openRename(page, 'thick');
    await page.locator('.insp-heading').click(); // blur by clicking elsewhere
    await settle();
    st = await page.evaluate(readLetState);
    check('Blur rename: parameter renamed', st.names.includes('thickness') && !st.names.includes('thick'), st.names.join(','));
    check('Blur rename: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- C. Escape-cancel: no rename, no mutation, no toast --------------------------------
    await loadDoc(page, url);
    const beforeEscape = await page.evaluate(readLetState);
    await openRename(page, 'thick');
    await page.locator('[data-testid="let-rename-input-thick"]').press('Escape');
    await settle();
    st = await page.evaluate(readLetState);
    check('Escape cancels: parameter name unchanged', st.names.includes('thick') && !st.names.includes('thickness'), st.names.join(','));
    check('Escape cancels: document not mutated', st.docVersion === beforeEscape.docVersion, `docVersion ${beforeEscape.docVersion}->${st.docVersion}`);
    check('Escape cancels: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- D. Collision guard: legitimate error preserved, doc intact ------------------------
    await loadDoc(page, url);
    await page.locator('[data-testid="rename-let-thick"]').click();
    await page.waitForSelector('[data-testid="let-rename-input-thick"]', { timeout: 5000 });
    await page.locator('[data-testid="let-rename-input-thick"]').fill('span'); // already exists
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    st = await page.evaluate(readLetState);
    check('Collision rejected: name unchanged', st.names.includes('thick') && st.names.includes('span'), st.names.join(','));
    check('Collision rejected: error toast shown', st.toastVisible && /already in use/i.test(st.toastText), st.toastText || 'no toast');

    // ---- E. Invalid-name guard -------------------------------------------------------------
    await loadDoc(page, url);
    await page.locator('[data-testid="rename-let-thick"]').click();
    await page.waitForSelector('[data-testid="let-rename-input-thick"]', { timeout: 5000 });
    await page.locator('[data-testid="let-rename-input-thick"]').fill('bad name'); // space => not an ident
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    st = await page.evaluate(readLetState);
    check('Invalid name rejected: name unchanged', st.names.includes('thick'), st.names.join(','));
    check('Invalid name rejected: error toast shown', st.toastVisible && /must start with a letter/i.test(st.toastText), st.toastText || 'no toast');

    // ---- F. Reserved-name guard ------------------------------------------------------------
    await loadDoc(page, url);
    await page.locator('[data-testid="rename-let-thick"]').click();
    await page.waitForSelector('[data-testid="let-rename-input-thick"]', { timeout: 5000 });
    await page.locator('[data-testid="let-rename-input-thick"]').fill('pi'); // reserved math ident
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    st = await page.evaluate(readLetState);
    check('Reserved name rejected: name unchanged', st.names.includes('thick'), st.names.join(','));
    check('Reserved name rejected: error toast shown', st.toastVisible && /reserved math identifier/i.test(st.toastText), st.toastText || 'no toast');

    // ---- G. Delete: unbind dependent consumer, freeze derived let --------------------------
    await loadDoc(page, url);
    await page.locator('[data-testid="delete-let-height"]').click();
    await settle();
    st = await page.evaluate(readLetState);
    check('Delete: parameter removed', !st.names.includes('height'), st.names.join(','));
    check('Delete: derived let half frozen to last value', st.halfExpr !== null && !/height/.test(st.halfExpr) && st.halfValue === 5, `half=${st.halfExpr} (${st.halfValue})`);
    check('Delete: dependent consumer beam.size.1 unbound', !('beam.size.1' in st.consumed), `beam.size.1=${st.consumed['beam.size.1'] ?? '(absent)'}`);
    check('Delete: geometry number kept [100,10,10]', sizeIs(st.beamSize, [100, 10, 10]), JSON.stringify(st.beamSize));
    check('Delete: no error toast', !st.toastVisible, st.toastVisible ? st.toastText : 'no toast');

    // ---- H. Round-trip: a renamed document survives print -> parse -------------------------
    await loadDoc(page, url);
    await openRename(page, 'thick');
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    const rt = await page.evaluate(readRoundTrip);
    check('Round-trip: print->parse preserves renamed let', rt.ok && rt.names.includes('thickness') && !rt.names.includes('thick'), rt.ok ? rt.names.join(',') : `parse failed: ${rt.error}`);
    check('Round-trip: print->parse preserves renamed consumer', rt.ok && rt.consumed['beam.size.2'] === 'thickness', rt.ok ? `beam.size.2=${rt.consumed['beam.size.2']}` : `parse failed: ${rt.error}`);

    // ---- I. Undo: restores the original name ----------------------------------------------
    await loadDoc(page, url);
    await openRename(page, 'thick');
    await page.locator('[data-testid="let-rename-input-thick"]').press('Enter');
    await settle();
    await page.evaluate(() => window.fieldsmith.store.undo());
    await settle();
    st = await page.evaluate(readLetState);
    check('Undo: restores original parameter name', st.names.includes('thick') && !st.names.includes('thickness'), st.names.join(','));

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Let-parameter lifecycle (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The Inspector params panel lets a user rename and delete `let` parameters. Rename and',
      'delete are document edits: a rename must propagate to every consumer expression and',
      'derived let, a delete must unbind dependent consumers (keeping their last evaluated',
      'number) and freeze derived lets. This suite drives the real panel — no FE solve — and',
      'gates the keyboard affordances in particular. The rename input commits on blur, and both',
      'Enter and Escape unmount it (firing blur too); a single disposition token gates exactly',
      'one commit-or-cancel so Enter commits once without a spurious error toast and Escape',
      'cancels without mutating the document.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Drives the real Inspector params panel; rename/delete are document edits, so no FE',
      '  solve is run and no field result is asserted.',
      '- The toast is a single replaceable slot (`ui.notify` overwrites), so the legitimate-error',
      '  checks assert presence + message, not a stack count.',
      '- Uncertainty propagation on rename/delete is not exercised: PARAM_DOC declares no',
      '  uncertainties. The derived-let freeze is checked via `half = height / 2`; deeper',
      '  multi-level chains are not exhaustively covered.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/letlifecycle.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['letlifecycle: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'letlifecycle-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`letlifecycle: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}