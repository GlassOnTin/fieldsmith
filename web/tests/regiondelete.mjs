// Region-delete reference cascade (doc model + UI): deleting a region must remove
// EVERY dependent, so the document still parses and validates afterwards. The
// headline defect this suite gates: the two region-delete paths in
// web/src/app/setup.tsx — deleteAnalysis('region', …) and removeFace's
// last-selector branch — cascade-deleted the six analysis-statement types
// (supports/contacts/loads/temperatures/convections/heat_fluxes), pruned
// fill.protect and consumers, but missed two other sites that name a region id:
//   - NodeKind.extend.region  (fsm.rs: "extend <id> <region> dir=… len=…"), and
//   - HxSpec.inlet / HxSpec.outlet  (fsm.rs: "hx inlet=<region> outlet=<region> …").
// Deleting such a region left a dangling reference the Rust validator rejects
// ("Extend '…' refers to missing region '…'" model.rs:2021; "hx inlet refers to
// missing region '…'" model.rs:1551). This is the delete-side twin of the rename
// blind spot fixed in 7812d95 (rename.mjs).
//
// The primary oracle is the real exported deleteAnalysis('region', id) — the exact
// function the inspector and tree delete buttons call — driven against the store.
// Two UI scenarios click the real "delete-region" and "Remove this face" buttons to
// prove both wirings (removeFace is private, reachable only through the DOM). The
// decisive oracles are the direct reference-field check, the validate() new-error
// delta (after vs baseline, so pre-existing notes don't mask the defect), and a
// printFsm→parseFsm round-trip. Regression guards cover the already-working
// statement cascade and a no-dependents no-op. No FE solve.
//
// Run with node tests/run-e2e.mjs --only regiondelete.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Real A22 geometry/fill/hx (examples/analytic/A22-…fsm:59-71) plus a protect list
// naming in_port and a second support on out_port, so deleting in_port leaves the
// document supported (no confounding "no support" error in the validate delta).
const HX_FSM = `fieldsmith 1
name "A22 hx delete probe"
material alsi10mg
box core size=(60, 20, 20) at=(30, 10, 10)
region in_port = core.nx
region out_port = core.px
support anchor in_port fixed=xyz
support hold out_port fixed=z
load internal_p out_port pressure=-0.5 case="ProofPressure"
fill gyroid sheet cell=5.0 skin=0.0 rho=(0.25, 0.25) wall=0.1 protect=(in_port)
hx inlet=in_port outlet=out_port mdot=0.08 tin=85 fluid=water arrangement=counterflow cold_tin=20 pout=100
`;

// extend's second positional token is a region name (fsm.rs:1185). skirtface is a
// two-selector union (skirt.nz + b.px) so removing the extend prunes one selector
// and leaves a still-valid region; top is unrelated and must survive. A box parent
// is not mesh-valid, so the extend case is asserted by the direct node/selector
// fields + the validate new-error delta (the dangling-reference error is new).
const EXTEND_FSM = `fieldsmith 1
name "Extend delete probe"
box b size=(10, 10, 10) at=(5, 5, 5)
region bottom = b.nz
extend skirt bottom dir=(0, 0, -1) len=5
region skirtface = skirt.nz + b.px
region top = b.pz
`;

// shape→region→statement chain plus a region with no dependents (free) for the
// no-op safety case.
const CHAIN_FSM = `fieldsmith 1
name "Chain delete probe"
material alsi10mg
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
region free = beam.pz
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
`;

export async function regiondeleteTest(browser, url, { docsDir }) {
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

  // Load a fixture into the real store, run deleteAnalysis('region', id), and read
  // the resulting document + validate delta + round-trip. Self-contained reader.
  const deleteViaAnalysis = (fsm, id) =>
    page.evaluate(async ([src, rid]) => {
      const w = await import('/src/doc/wasm.ts');
      const { deleteAnalysis } = await import('/src/app/setup.tsx');
      await w.loadWasm();
      const r = w.parseFsm(src);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      window.fieldsmith.store.load(r.ok);
      const baseline = w.validate(window.fieldsmith.store.doc);
      deleteAnalysis('region', rid);
      // transact() structuredClones and REPLACES store.doc, so re-read after the call.
      const doc = window.fieldsmith.store.doc;
      const after = w.validate(doc);
      const findExt = (nodes) => {
        for (const n of nodes) {
          if (n.kind?.type === 'extend') return n;
          if (n.kind?.type === 'group') { const f = findExt(n.kind.children); if (f) return f; }
        }
        return null;
      };
      const printed = w.printFsm(doc);
      const re = w.parseFsm(printed);
      return {
        regions: doc.regions.map((x) => x.id),
        supports: doc.supports.map((s) => ({ id: s.id, region: s.region })),
        loads: doc.loads.map((l) => ({ id: l.id, region: l.region })),
        protect: doc.fill ? doc.fill.protect.slice() : null,
        hx: doc.hx ? { inlet: doc.hx.inlet, outlet: doc.hx.outlet } : null,
        extend: findExt(doc.root)?.id ?? null,
        extendRegion: findExt(doc.root)?.kind?.region ?? null,
        skirtface: doc.regions.find((x) => x.id === 'skirtface')?.selectors.map((s) => s.shape) ?? null,
        baselineCount: baseline.length,
        newErrors: after.filter((e) => !baseline.includes(e)),
        printedExtend: printed.split('\n').find((l) => l.startsWith('extend')) ?? '',
        roundTripOk: !!re.ok,
        roundTripHx: re.ok?.hx ? { inlet: re.ok.hx.inlet, outlet: re.ok.hx.outlet } : null,
        roundTripExtend: re.ok ? (findExt(re.ok.root)?.id ?? null) : null,
        roundTripError: re.ok ? null : (re.error?.message ?? 'reparse failed'),
      };
    }, [fsm, id]);

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    // ---- A. hx region delete: the hx spec must go when its inlet region goes ------
    const hx = await deleteViaAnalysis(HX_FSM, 'in_port');
    check('hx: fixture parses', !hx.parseError, hx.parseError ?? 'ok');
    check('hx: region removed', !hx.regions.includes('in_port') && hx.regions.includes('out_port'), `regions=[${hx.regions}]`);
    check('hx: support cascade', !hx.supports.some((s) => s.id === 'anchor') && hx.supports.some((s) => s.id === 'hold'), `supports=[${hx.supports.map((s) => s.id)}]`);
    check('hx: unrelated load kept', hx.loads.some((l) => l.id === 'internal_p' && l.region === 'out_port'), `loads=[${hx.loads.map((l) => l.id)}]`);
    check('hx: fill.protect pruned', Array.isArray(hx.protect) && !hx.protect.includes('in_port'), `protect=[${hx.protect}]`);
    check('hx: hx spec removed', hx.hx === null, `hx=${JSON.stringify(hx.hx)}`);
    check('hx: no new validate errors', (hx.newErrors ?? []).length === 0, (hx.newErrors ?? []).join(' | ') || `baseline ${hx.baselineCount} errors, 0 new`);
    check('hx: round-trip parses', hx.roundTripOk === true, hx.roundTripError ?? 'ok');
    check('hx: round-trip hx absent', hx.roundTripOk === true && hx.roundTripHx === null, `reparse hx=${JSON.stringify(hx.roundTripHx)}`);

    // ---- B. extend region delete: the extend node + its downstream selectors go ----
    const ext = await deleteViaAnalysis(EXTEND_FSM, 'bottom');
    check('extend: fixture parses', !ext.parseError, ext.parseError ?? 'ok');
    check('extend: region removed', !ext.regions.includes('bottom'), `regions=[${ext.regions}]`);
    check('extend: node removed', ext.extend === null, `extend=${ext.extend} region=${ext.extendRegion}`);
    check('extend: downstream selector pruned', Array.isArray(ext.skirtface) && !ext.skirtface.includes('skirt') && ext.skirtface.includes('b'), `skirtface=[${ext.skirtface}]`);
    check('extend: unrelated region kept', ext.regions.includes('top'), `regions=[${ext.regions}]`);
    check('extend: no new validate errors', (ext.newErrors ?? []).length === 0, (ext.newErrors ?? []).join(' | ') || `baseline ${ext.baselineCount} errors, 0 new`);
    check('extend: round-trip no extend line', ext.roundTripOk === true && ext.roundTripExtend === null && ext.printedExtend === '', ext.roundTripError ?? `printed="${ext.printedExtend}" reparse extend=${ext.roundTripExtend}`);

    // ---- C. statement cascade (regression: already-working path) -------------------
    const chain = await deleteViaAnalysis(CHAIN_FSM, 'tip');
    check('chain: region removed', !chain.regions.includes('tip') && chain.regions.includes('root'), `regions=[${chain.regions}]`);
    check('chain: load cascade', !chain.loads.some((l) => l.id === 'push'), `loads=[${chain.loads.map((l) => l.id)}]`);
    check('chain: unrelated support kept', chain.supports.some((s) => s.id === 'clamp' && s.region === 'root'), `supports=[${chain.supports.map((s) => s.id)}]`);
    check('chain: no new validate errors + round-trip', (chain.newErrors ?? []).length === 0 && chain.roundTripOk === true, (chain.newErrors ?? []).join(' | ') || (chain.roundTripError ?? 'ok'));

    // ---- D. no-dependents delete is a clean no-op beyond the region itself ---------
    const noop = await deleteViaAnalysis(CHAIN_FSM, 'free');
    check('noop: only free removed', !noop.regions.includes('free') && noop.regions.includes('root') && noop.regions.includes('tip'), `regions=[${noop.regions}]`);
    check('noop: statements untouched', noop.supports.some((s) => s.id === 'clamp') && noop.loads.some((l) => l.id === 'push'), `supports=[${noop.supports.map((s) => s.id)}] loads=[${noop.loads.map((l) => l.id)}]`);
    check('noop: no new validate errors + round-trip', (noop.newErrors ?? []).length === 0 && noop.roundTripOk === true, (noop.newErrors ?? []).join(' | ') || (noop.roundTripError ?? 'ok'));

    // ---- E. UI wiring: the "Remove this face" button (removeFace last-selector) ----
    await page.evaluate(async (src) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(src);
      if (!r.ok) throw new Error('parse failed: ' + (r.error?.message ?? ''));
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('setup');
      window.fieldsmith.store.select({ kind: 'region', id: 'bottom' });
    }, EXTEND_FSM);
    await page.waitForSelector('[data-inspector="region"]', { timeout: 60000 });
    await page.click('button[title="Remove this face"]');
    await page.waitForTimeout(500);
    const faceUi = await page.evaluate(() => {
      const doc = window.fieldsmith.store.doc;
      const findExt = (nodes) => {
        for (const n of nodes) {
          if (n.kind?.type === 'extend') return n;
          if (n.kind?.type === 'group') { const f = findExt(n.kind.children); if (f) return f; }
        }
        return null;
      };
      return {
        bottomGone: !doc.regions.some((x) => x.id === 'bottom'),
        extend: findExt(doc.root)?.id ?? null,
        skirtface: doc.regions.find((x) => x.id === 'skirtface')?.selectors.map((s) => s.shape) ?? null,
      };
    });
    check('ui removeFace: region removed', faceUi.bottomGone === true, `bottomGone=${faceUi.bottomGone}`);
    check('ui removeFace: extend cascade', faceUi.extend === null, `extend=${faceUi.extend}`);
    check('ui removeFace: downstream pruned', Array.isArray(faceUi.skirtface) && !faceUi.skirtface.includes('skirt'), `skirtface=[${faceUi.skirtface}]`);

    // ---- F. UI wiring: the inspector "delete-region" button (hx fixture) -----------
    await page.evaluate(async (src) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(src);
      if (!r.ok) throw new Error('parse failed: ' + (r.error?.message ?? ''));
      window.fieldsmith.store.load(r.ok);
      window.fieldsmith.store.setLens('setup');
      window.fieldsmith.store.select({ kind: 'region', id: 'in_port' });
    }, HX_FSM);
    await page.waitForSelector('[data-testid="delete-region"]', { timeout: 60000 });
    await page.click('[data-testid="delete-region"]');
    await page.waitForTimeout(500);
    const delUi = await page.evaluate(() => {
      const doc = window.fieldsmith.store.doc;
      return {
        inPortGone: !doc.regions.some((x) => x.id === 'in_port'),
        hx: doc.hx ? { inlet: doc.hx.inlet, outlet: doc.hx.outlet } : null,
        anchorGone: !doc.supports.some((s) => s.id === 'anchor'),
      };
    });
    check('ui delete-region: region removed', delUi.inPortGone === true, `inPortGone=${delUi.inPortGone}`);
    check('ui delete-region: hx cascade', delUi.hx === null && delUi.anchorGone === true, `hx=${JSON.stringify(delUi.hx)} anchorGone=${delUi.anchorGone}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Region-delete reference cascade (doc model + UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Deleting a region must remove every dependent so the document still parses and',
      'validates. The two region-delete paths in setup.tsx (`deleteAnalysis(\'region\', …)`',
      "and `removeFace`'s last-selector branch) cascade-deleted the six analysis-statement",
      'types, pruned `fill.protect` and consumers, but missed two other sites that name a',
      "region id: an `extend` node's `region` and the heat-exchanger `hx inlet`/`outlet`.",
      'Deleting such a region left a dangling reference the Rust validator rejects',
      "(`Extend '…' refers to missing region`, `hx inlet refers to missing region`). This is",
      'the delete-side twin of the rename blind spot fixed in 7812d95. The primary oracle is',
      'the real exported `deleteAnalysis` driven against the store; two UI scenarios click',
      'the real `delete-region` and `Remove this face` buttons to prove both wirings. The',
      'decisive oracles are the direct reference-field check, the validate() new-error delta',
      '(after vs baseline), and a printFsm→parseFsm round-trip. No FE solve.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The hx case uses the real A22 geometry/fill/hx plus a protect list and a second',
      '  support; the extend case uses a constructed box-parent doc, which parses and',
      '  round-trips but is not mesh-valid. The extend cascade is asserted by the direct',
      '  node/selector fields and the validate new-error delta (the dangling-reference error',
      '  is new relative to baseline), not by a clean baseline.',
      '- removeNode prunes region selectors, not whole regions: removing an extend can leave',
      '  a region whose selector list shrinks (skirtface keeps b.px). A region left with zero',
      '  selectors by that path is existing removeNode behaviour, not introduced here.',
      '- No FE solve is run; this is a document-model edit, so no field result is asserted.',
      '- The two UI scenarios drive the inspector delete button and the per-face remove',
      '  button; the tree-row delete button calls the same deleteAnalysis, not re-driven.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/regiondelete.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['regiondelete: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'regiondelete-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`regiondelete: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}