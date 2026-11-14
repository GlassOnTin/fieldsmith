// Selection pruning after a cascade delete (doc model): deleting an analysis item
// must drop a selection whose target the delete removed. The headline defect this
// suite gates: deleteAnalysis in web/src/app/setup.tsx cleared a stale selection
// with a hand-rolled existence check that ended in a `: true` fallback for the node
// kind, so a selected `extend` node that deleteRegion cascade-removes (via removeNode)
// was never cleared — store.selection dangled, pointing at a node no longer in the
// document. The Inspector masks it (findNode returns nothing → DocumentInspector
// fallback, no crash), but the selection state is wrong. This is exposed directly by
// the region-delete cascade shipped in 35f576b (regiondelete.mjs): that fix made the
// delete remove the extend node, which turned the latent node-selection blind spot
// into a reachable dangling selection.
//
// The root cause is drift: deleteAnalysis's manual block is a copy of the store's
// canonical fixSelection() (store.ts) minus the node case. The fix exposes a public
// Store.pruneSelection() that reuses fixSelection() and emits on change, and replaces
// the manual block with it — so the delete path cannot drift from undo/redo again.
// transact() is deliberately NOT changed: Tree.tsx RenameInput reads store.selection
// AFTER its transact and re-selects the new id, so calling fixSelection inside transact
// would clear the selection mid-rename and break selection-follow.
//
// The primary oracle is the real exported deleteAnalysis driven against the store,
// asserting store.selection after the call. Regression guards cover the already-working
// analysis-kind clears (region/support/load), an unrelated node that must survive, and
// an unrelated support that must survive. No UI, no FE solve.
//
// Run with node tests/run-e2e.mjs --only selprune.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// extend's second positional token is a region name (fsm.rs:1185). The extend node id
// is `skirt`; deleting region `bottom` cascade-removes it. Box `b` is unrelated and must
// survive. A box parent is not mesh-valid, but selection pruning is a pure doc-model
// concern, so validate() is not the oracle here — store.selection is.
const EXTEND_FSM = `fieldsmith 1
name "Extend selection probe"
box b size=(10, 10, 10) at=(5, 5, 5)
region bottom = b.nz
extend skirt bottom dir=(0, 0, -1) len=5
region skirtface = skirt.nz + b.px
region top = b.pz
`;

// shape→region→statement chain: clamp names root, push names tip, free has no dependents.
const CHAIN_FSM = `fieldsmith 1
name "Chain selection probe"
material alsi10mg
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
region free = beam.pz
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
`;

export async function selpruneTest(browser, url, { docsDir }) {
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

  // Load a fixture, select `sel`, run the real deleteAnalysis(kind, id), and read the
  // selection before/after plus existence facts. Self-contained reader; re-reads
  // store.doc AFTER the call because transact() structuredClones and REPLACES it.
  const selectThenDelete = (fsm, sel, kind, id) =>
    page.evaluate(async ([src, selection, k, rid]) => {
      const w = await import('/src/doc/wasm.ts');
      const { deleteAnalysis } = await import('/src/app/setup.tsx');
      await w.loadWasm();
      const r = w.parseFsm(src);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const store = window.fieldsmith.store;
      store.load(r.ok);
      const findExt = (nodes) => {
        for (const n of nodes) {
          if (n.kind?.type === 'extend') return n;
          if (n.kind?.type === 'group') { const f = findExt(n.kind.children); if (f) return f; }
        }
        return null;
      };
      const extendId = findExt(r.ok.root)?.id ?? null;
      store.select(selection);
      const before = store.selection;
      deleteAnalysis(k, rid);
      const doc = store.doc;
      const after = store.selection;
      return {
        extendId,
        before,
        after,
        extendGone: findExt(doc.root) === null,
        nodeBExists: JSON.stringify(doc.root).includes('"id":"b"'),
        regions: doc.regions.map((x) => x.id),
        supports: doc.supports.map((s) => s.id),
        loads: doc.loads.map((l) => l.id),
      };
    }, [fsm, sel, kind, id]);

  const isNull = (s) => s === null;
  const isSel = (s, kind, id) => !!s && s.kind === kind && s.id === id;

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    // ---- A. THE defect: a selected extend node cascade-removed by region delete -----
    const a = await selectThenDelete(EXTEND_FSM, { kind: 'node', id: 'skirt' }, 'region', 'bottom');
    check('extend: fixture parses', !a.parseError, a.parseError ?? 'ok');
    check('extend: node id is skirt (sanity)', a.extendId === 'skirt', `extendId=${a.extendId}`);
    check('extend: selected before delete', isSel(a.before, 'node', 'skirt'), `before=${JSON.stringify(a.before)}`);
    check('extend: node cascade-removed', a.extendGone === true && !a.regions.includes('bottom'), `extendGone=${a.extendGone} regions=[${a.regions}]`);
    check('extend: dangling node selection cleared', isNull(a.after), `after=${JSON.stringify(a.after)}`);

    // ---- B. regression: an unrelated node selection must survive the same delete ----
    const b = await selectThenDelete(EXTEND_FSM, { kind: 'node', id: 'b' }, 'region', 'bottom');
    check('unrelated node: box b survives', b.nodeBExists === true, `nodeBExists=${b.nodeBExists}`);
    check('unrelated node: selection kept', isSel(b.after, 'node', 'b'), `after=${JSON.stringify(b.after)}`);

    // ---- C. regression: a selected region clears when it is deleted -----------------
    const c = await selectThenDelete(CHAIN_FSM, { kind: 'region', id: 'tip' }, 'region', 'tip');
    check('region: removed', !c.regions.includes('tip'), `regions=[${c.regions}]`);
    check('region: selection cleared', isNull(c.after), `after=${JSON.stringify(c.after)}`);

    // ---- D. regression: a selected support clears when its region is cascade-deleted -
    const d = await selectThenDelete(CHAIN_FSM, { kind: 'support', id: 'clamp' }, 'region', 'root');
    check('support cascade: clamp removed', !d.supports.includes('clamp'), `supports=[${d.supports}]`);
    check('support cascade: selection cleared', isNull(d.after), `after=${JSON.stringify(d.after)}`);

    // ---- E. regression: a selected load clears on a direct load delete --------------
    const e = await selectThenDelete(CHAIN_FSM, { kind: 'load', id: 'push' }, 'load', 'push');
    check('load direct: push removed', !e.loads.includes('push'), `loads=[${e.loads}]`);
    check('load direct: selection cleared', isNull(e.after), `after=${JSON.stringify(e.after)}`);

    // ---- F. regression: an unrelated support survives a different region's delete ---
    const f = await selectThenDelete(CHAIN_FSM, { kind: 'support', id: 'clamp' }, 'region', 'tip');
    check('unrelated support: clamp kept', f.supports.includes('clamp'), `supports=[${f.supports}]`);
    check('unrelated support: selection kept', isSel(f.after, 'support', 'clamp'), `after=${JSON.stringify(f.after)}`);

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Selection pruning after a cascade delete (doc model)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'Deleting an analysis item must drop a selection whose target the delete removed.',
      "`deleteAnalysis` in setup.tsx cleared a stale selection with a hand-rolled existence",
      'check that ended in a `: true` fallback for the node kind, so a selected `extend`',
      'node that `deleteRegion` cascade-removes (via `removeNode`) was never cleared —',
      '`store.selection` dangled. The Inspector masks it (`findNode` returns nothing →',
      'DocumentInspector fallback, no crash), but the selection state is wrong. Exposed by',
      'the region-delete cascade shipped in 35f576b. The root cause is drift: the manual',
      "block is a copy of the store's canonical `fixSelection()` minus the node case. The",
      'fix exposes a public `Store.pruneSelection()` that reuses `fixSelection()` and emits',
      'on change, and replaces the manual block with it. `transact()` is deliberately not',
      'changed: `RenameInput` reads `store.selection` after its transact and re-selects the',
      'new id, so pruning inside transact would break rename selection-follow. The oracle',
      'is the real exported `deleteAnalysis` driven against the store; no UI, no FE solve.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The extend fixture is a box parent (parses, not mesh-valid); selection pruning is a',
      '  pure doc-model concern, so `store.selection` is the oracle, not `validate()`.',
      '- No FE solve is run; this is a document-model edit, so no field result is asserted.',
      '- Rename selection-follow is not re-driven here: `transact()` and `RenameInput` are',
      '  untouched by this fix, and the full-suite `rename.mjs` (30 checks) stays green as the',
      '  regression guard for the rename operation.',
      '- The tree-row node delete (Tree.tsx `removeNodeAndCompanion`) has its own manual',
      '  selection clear and is a separate path, not gated here.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/selprune.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['selprune: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'selprune-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`selprune: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}