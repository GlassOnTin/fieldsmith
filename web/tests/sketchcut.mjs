// A subtractive sketch feature on an empty document is rejected, not silently staged.
//
// The empty-state "Start from a sketch" entry point (sketchstart.mjs) opens the workbench on
// an additive boss_extrude, which is valid with no host. The subtractive operations
// (pocket_cut, revolve_cut) remove material from an existing solid, so on an empty document
// there is nothing to cut from. Before the guard, apply_sketch_feature_to_document staged a
// lone Op::Subtract node and reported success ("Applied pocket_cut ... (1 node)"), yet the
// document had no solid: exportSolid later threw "document has no solid". The user saw a
// success toast and an empty part.
//
// The fix is a structural guard in the shared Rust reducer (fieldsmith-doc): a subtractive
// request with no non-suppressed Add node in doc.root returns Err before any mutation. The
// error propagates through apply_doc_command -> the wasm applyDocCommand binding (a JS throw)
// -> store.dispatch, which throws before it commits the document or pushes an undo entry, so
// the doc stays empty and the workbench's catch shows an actionable error toast with the modal
// still open. One guard covers the UI, MCP and WASM paths (no engine island).
//
// The oracle is the real UI: open the workbench from the empty state, switch to pocket_cut and
// apply, read ui.toast / store.doc / store.selection / canUndo, repeat for revolve_cut, then
// switch back to the additive boss_extrude and apply to prove the guard is operation-specific
// and leaves the workbench usable. No FE solve.
//
// Run with node tests/run-e2e.mjs --only sketchcut.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

export async function sketchcutTest(browser, url, { docsDir }) {
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

  // Self-contained reader: doc shape + sketch artifacts + selection + ui.sketchOpen + undo arm
  // + the live toast ({text, kind}). selectionExists mirrors store.fixSelection's node check.
  const readState = () =>
    page.evaluate(() => {
      const f = window.fieldsmith;
      const d = f.store.doc;
      const sel = f.store.selection;
      const selectionExists =
        sel && sel.kind === 'node' ? JSON.stringify(d.root).includes(`"id":"${sel.id}"`) : null;
      return {
        rootLen: d.root.length,
        sketches: (d.sketches ?? []).length,
        features: (d.sketch_features ?? []).length,
        selection: sel,
        selectionExists,
        lens: f.store.lens,
        sketchOpen: !!f.ui.sketchOpen,
        canUndo: f.store.canUndo(),
        toast: f.ui.toast ? { text: f.ui.toast.text, kind: f.ui.toast.kind } : null,
      };
    });

  // Apply the current operation and wait for the error toast naming that operation. Waiting on
  // the operation text (not just kind==='error') avoids reading a lingering prior toast.
  const applyExpectingRejection = async (opName) => {
    await page.locator('[data-testid="apply-sketch-feature-btn"]').click();
    await page.waitForFunction(
      (name) => {
        const t = window.fieldsmith.ui.toast;
        return !!t && t.kind === 'error' && t.text.includes(name);
      },
      opName,
      { timeout: 30000 },
    );
  };

  try {
    // Proven empty-state setup (shape.mjs / sketchstart.mjs): clear autosave so a prior doc is
    // not restored, reload, wait for the empty-state card.
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForSelector('[data-testid="empty-state"]', { timeout: 120000 });

    // ---- A. empty shape lens -------------------------------------------------------------
    const s0 = await readState();
    check('empty: shape lens with empty doc', s0.rootLen === 0 && s0.lens === 'shape', `root=${s0.rootLen} lens=${s0.lens}`);

    // ---- B. open the workbench from the empty state --------------------------------------
    await page.locator('[data-testid="empty-start-sketch"]').click();
    await page.waitForSelector('[data-testid="parametric-sketch-workbench"]', { state: 'visible', timeout: 30000 });
    await page.waitForSelector('[data-testid="sketch-workbench-modal"]', { state: 'visible', timeout: 30000 });
    const s1 = await readState();
    check('open: workbench visible, doc still empty', s1.sketchOpen === true && s1.rootLen === 0, `sketchOpen=${s1.sketchOpen} root=${s1.rootLen}`);

    // ---- C. pocket_cut on empty is rejected with an actionable message -------------------
    await page.locator('[data-testid="sketch-op-card-pocket_cut"]').click();
    check('pocket: depth input shown', await page.locator('[data-testid="sketch-pocket-depth"]').isVisible(), 'visible');
    await applyExpectingRejection('pocket cut');
    const s2 = await readState();
    const pocketToastDom = await page.locator('[data-testid="toast"]').textContent().catch(() => '');
    check('pocket: error toast names the missing solid', s2.toast?.kind === 'error' && /solid/.test(s2.toast.text), `toast=${JSON.stringify(s2.toast)}`);
    check('pocket: toast visible in the DOM', /solid/.test(pocketToastDom ?? ''), `dom=${(pocketToastDom ?? '').trim()}`);
    check('pocket: doc stays empty (no dangling subtract node)', s2.rootLen === 0, `root=${s2.rootLen}`);
    check('pocket: no sketch persisted', s2.sketches === 0, `sketches=${s2.sketches}`);
    check('pocket: no feature binding persisted', s2.features === 0, `features=${s2.features}`);
    check('pocket: modal stays open', s2.sketchOpen === true, `sketchOpen=${s2.sketchOpen}`);
    check('pocket: no undo entry armed', s2.canUndo === false, `canUndo=${s2.canUndo}`);
    check('pocket: selection stays null', s2.selection === null, `selection=${JSON.stringify(s2.selection)}`);

    // ---- D. revolve_cut on empty is rejected too -----------------------------------------
    await page.locator('[data-testid="sketch-op-card-revolve_cut"]').click();
    check('revolve: angle input shown', await page.locator('[data-testid="sketch-revolve-angle"]').isVisible(), 'visible');
    await applyExpectingRejection('revolve cut');
    const s3 = await readState();
    check('revolve: error toast names the missing solid', s3.toast?.kind === 'error' && /solid/.test(s3.toast.text), `toast=${JSON.stringify(s3.toast)}`);
    check('revolve: doc stays empty', s3.rootLen === 0, `root=${s3.rootLen}`);
    check('revolve: modal stays open', s3.sketchOpen === true, `sketchOpen=${s3.sketchOpen}`);
    check('revolve: no undo entry armed', s3.canUndo === false, `canUndo=${s3.canUndo}`);

    // ---- E. the additive boss still applies: the guard is operation-specific -------------
    await page.locator('[data-testid="sketch-op-card-boss_extrude"]').click();
    await page.locator('[data-testid="apply-sketch-feature-btn"]').click();
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.length >= 1, null, { timeout: 30000 });
    await page.waitForSelector('[data-testid="sketch-workbench-modal"]', { state: 'hidden', timeout: 30000 });
    const s4 = await readState();
    check('boss: root grew from empty', s4.rootLen >= 1, `root=${s4.rootLen}`);
    check('boss: sketch persisted', s4.sketches >= 1, `sketches=${s4.sketches}`);
    check('boss: feature binding created', s4.features >= 1, `features=${s4.features}`);
    check('boss: modal closed', s4.sketchOpen === false, `sketchOpen=${s4.sketchOpen}`);
    check('boss: undo armed', s4.canUndo === true, `canUndo=${s4.canUndo}`);
    check('boss: selection follows a real node', s4.selectionExists === true, `selectionExists=${s4.selectionExists}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors (rejection is a handled error)', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Subtractive sketch feature on an empty document (UI flow)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'A pocket cut or groove (revolve) cut removes material from an existing solid. On an',
      'empty document there is nothing to cut from, so the shared Rust reducer',
      '(`apply_sketch_feature_to_document`) rejects a subtractive request that has no',
      'non-suppressed `Add` node, before any mutation. The error travels through',
      '`apply_doc_command` and the wasm `applyDocCommand` binding (a JS throw) to',
      '`store.dispatch`, which throws before committing the document or pushing an undo entry.',
      'The workbench catch shows an actionable error toast and the modal stays open, so the',
      'document stays empty rather than reporting success over a part with no solid. The same',
      'guard covers the UI, MCP and WASM paths. Switching back to the additive `boss_extrude`',
      'and applying still works, so the guard is operation-specific and leaves the workbench',
      'usable. The oracle is the real UI round trip; no FE solve.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The subtractive-on-a-host case (a pocket cut against an existing `Add` solid) is',
      '  gated by the Rust unit tests and v58, not re-driven through the UI here.',
      '- The created-node count for the additive preset is not pinned (asserted `root >= 1`);',
      '  the binding, selection-follow and modal-close are the oracles.',
      '- No FE solve is run; this is a document-model edit driven through the UI.',
      '- The toast is read from `ui.toast` and the `[data-testid="toast"]` node; its 3.2 s',
      '  auto-dismiss is not exercised.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/sketchcut.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['sketchcut: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'sketchcut-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`sketchcut: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}