// Start from a sketch on the empty-state page (UI flow).
//
// Many models begin as a 2D section, so the "Start with a shape" empty state offers a
// "Start from a sketch" button alongside Add a box / Cylinder / Import mesh. It calls the
// already-imported openSketchWorkbenchModal(), which sets ui.sketchOpen and lazy-mounts the
// SketchWorkbench modal. The workbench opens on its default preset (flanged_mounting_bracket)
// with an additive boss_extrude on the xy plane, so "Build 3D Feature in Document" works on an
// empty document with no host shape: applyTo3dDocument dispatches apply_sketch_feature through
// the shared Rust reducer, persists the sketch + a sketch_features binding, creates the 3D
// node(s), selects the first created node and closes the modal. store.dispatch records exactly
// one undo entry, so a single Undo returns to the empty document, clears the now-dangling
// selection (fixSelection) and brings the empty state back.
//
// The headline defect this suite gates: the empty state had no sketch entry point at all
// (empty-start-sketch absent), so a user starting from a 2D section had to first add a throwaway
// shape or find the HUD launcher. The oracle is the real UI: click the button, read ui.sketchOpen
// and the workbench backdrop, click apply, read store.doc / store.selection, click the toolbar
// Undo, and assert the round trip back to empty. No FE solve.
//
// Run with node tests/run-e2e.mjs --only sketchstart.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

export async function sketchstartTest(browser, url, { docsDir }) {
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

  // Self-contained reader: doc shape + sketch artifacts + selection + ui.sketchOpen + undo arm.
  // selectionExists mirrors store.fixSelection's node existence check so selection-follow is
  // asserted against a real node, not just a non-null selection.
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
      };
    });

  try {
    // Proven empty-state setup (shape.mjs): clear autosave so a prior doc is not restored,
    // reload, and wait for the empty-state card. window.fieldsmith is assembled in main().
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForSelector('[data-testid="empty-state"]', { timeout: 120000 });

    // ---- A. the empty state is the shape lens with an empty document -------------------
    const s0 = await readState();
    check('empty: shape lens with empty doc', s0.rootLen === 0 && s0.lens === 'shape', `root=${s0.rootLen} lens=${s0.lens}`);
    check('empty: empty-state card visible', await page.locator('[data-testid="empty-state"]').isVisible(), 'visible');

    // ---- B. THE feature: a Start from a sketch entry point exists (RED until added) -----
    const btnCount = await page.locator('[data-testid="empty-start-sketch"]').count();
    check('empty: Start from a sketch button present', btnCount === 1, `count=${btnCount}`);

    // Guard the interactive flow on the button existing so a RED run is a clean FAIL report
    // (button absent) rather than an aborted click. GREEN drives the whole round trip.
    if (btnCount === 1) {
      // ---- C. click opens the sketch workbench modal; doc is still empty ---------------
      await page.locator('[data-testid="empty-start-sketch"]').click();
      await page.waitForSelector('[data-testid="parametric-sketch-workbench"]', { state: 'visible', timeout: 30000 });
      await page.waitForSelector('[data-testid="sketch-workbench-modal"]', { state: 'visible', timeout: 30000 });
      const s1 = await readState();
      check('open: ui.sketchOpen true', s1.sketchOpen === true, `sketchOpen=${s1.sketchOpen}`);
      check('open: workbench visible', await page.locator('[data-testid="parametric-sketch-workbench"]').isVisible(), 'visible');
      check('open: doc still empty before apply', s1.rootLen === 0, `root=${s1.rootLen}`);

      // ---- D. apply the default sketch feature: 3D node(s) + sketch + binding -----------
      await page.locator('[data-testid="apply-sketch-feature-btn"]').click();
      await page.waitForFunction(() => window.fieldsmith.store.doc.root.length >= 1, null, { timeout: 30000 });
      const s2 = await readState();
      check('apply: root grew from empty', s2.rootLen >= 1, `root=${s2.rootLen}`);
      check('apply: sketch persisted', s2.sketches >= 1, `sketches=${s2.sketches}`);
      check('apply: sketch feature binding created', s2.features >= 1, `features=${s2.features}`);
      check('apply: selection follows created node', !!s2.selection && s2.selection.kind === 'node', `selection=${JSON.stringify(s2.selection)}`);
      check('apply: selection points at a real node', s2.selectionExists === true, `selectionExists=${s2.selectionExists}`);

      // ---- E. apply closes the modal and arms exactly one undo -------------------------
      await page.waitForSelector('[data-testid="sketch-workbench-modal"]', { state: 'hidden', timeout: 30000 });
      const s3 = await readState();
      check('apply: modal closed', s3.sketchOpen === false, `sketchOpen=${s3.sketchOpen}`);
      check('apply: undo armed', s3.canUndo === true, `canUndo=${s3.canUndo}`);

      // ---- F. one Undo returns to empty: doc, selection and the empty state ------------
      await page.locator('[data-testid="undo"]').click();
      await page.waitForFunction(() => window.fieldsmith.store.doc.root.length === 0, null, { timeout: 30000 });
      await page.waitForSelector('[data-testid="empty-state"]', { state: 'visible', timeout: 30000 });
      const s4 = await readState();
      check('undo: doc empty again', s4.rootLen === 0, `root=${s4.rootLen}`);
      check('undo: selection cleared', s4.selection === null, `selection=${JSON.stringify(s4.selection)}`);
      check('undo: empty-state returns', await page.locator('[data-testid="empty-state"]').isVisible(), 'visible');
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Start from a sketch on the empty-state page (UI flow)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Many models begin as a 2D section, so the "Start with a shape" empty state offers a',
      '"Start from a sketch" button (`empty-start-sketch`) alongside Add a box / Cylinder /',
      'Import mesh. It calls the already-imported `openSketchWorkbenchModal()`, which sets',
      '`ui.sketchOpen` and lazy-mounts the SketchWorkbench modal. The workbench opens on its',
      'default preset (`flanged_mounting_bracket`) with an additive `boss_extrude` on the xy',
      'plane, so "Build 3D Feature in Document" (`apply-sketch-feature-btn`) works on an empty',
      'document with no host shape: `applyTo3dDocument` dispatches `apply_sketch_feature`',
      'through the shared Rust reducer, persists the sketch + a `sketch_features` binding,',
      'creates the 3D node(s), selects the first created node and closes the modal.',
      '`store.dispatch` records exactly one undo entry, so a single toolbar Undo returns to the',
      'empty document, clears the now-dangling selection (`fixSelection`) and brings the empty',
      'state back. The oracle is the real UI round trip; no FE solve.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The default preset is additive (`boss_extrude`), so no host shape is required; a',
      '  subtractive operation on an empty document is a separate path, not gated here.',
      '- The created-node count for the preset is not pinned (asserted `root >= 1`); the',
      '  binding and selection-follow are the oracles, not an exact node tally.',
      '- No FE solve is run; this is a document-model edit driven through the UI, so no field',
      '  result is asserted.',
      '- The HUD sketch launcher (`hud-open-sketch-workbench`) is unchanged and not re-driven',
      '  here; this suite gates only the new empty-state entry point.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/sketchstart.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['sketchstart: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'sketchstart-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`sketchstart: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}