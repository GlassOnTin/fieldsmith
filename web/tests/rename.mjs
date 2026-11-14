// Rename reference-rewriting (doc model): renameId / renameLet must update EVERY
// site that names the renamed entity, so a renamed document still parses and
// validates. The headline defect this suite gates: the region-rename branch of
// renameId (web/src/doc/ops.ts) rewired supports/contacts/loads/temperatures/
// convections/heat_fluxes, but a region id is ALSO named by
//   - NodeKind.extend.region  (fsm.rs: "extend <id> <region> dir=… len=…"), and
//   - HxSpec.inlet / HxSpec.outlet  (fsm.rs: "hx inlet=<region> outlet=<region> …").
// Neither was rewired, so renaming such a region left a dangling reference that
// the Rust validator rejects ("hx inlet refers to missing region '…'" at
// model.rs:1551; "Extend '…' refers to missing region '…'" at model.rs:2021).
//
// Everything runs in page.evaluate against the real wasm parse/print/validate and
// the real renameId/renameLet — no UI, no FE solve. The decisive oracles are the
// direct reference-field check, the validate() new-error delta (after vs baseline,
// so pre-existing notes don't mask the defect), and a printFsm→parseFsm round-trip.
// Regression guards cover the already-working shape→region→load chain, the let
// expression rewire, and the collision / invalid-ident / unknown-id error paths.
//
// Run with node tests/run-e2e.mjs --only rename.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Real A22 geometry/fill/hx (examples/analytic/A22-…fsm lines 59-71), minus the
// literate header/asserts/solve which the rename check does not need. in_port and
// out_port are regions; hx inlet=in_port outlet=out_port binds them.
const HX_FSM = `fieldsmith 1
name "A22 hx rename probe"
material alsi10mg
box core size=(60, 20, 20) at=(30, 10, 10)
region in_port = core.nx
region out_port = core.px
support anchor in_port fixed=xyz
load internal_p out_port pressure=-0.5 case="ProofPressure"
fill gyroid sheet cell=5.0 skin=0.0 rho=(0.25, 0.25) wall=0.1
hx inlet=in_port outlet=out_port mdot=0.08 tin=85 fluid=water arrangement=counterflow cold_tin=20 pout=100
`;

// extend's second positional token is a region name (fsm.rs:1185). A box parent
// is enough to PARSE and to exercise the reference rewire + round-trip; it is not
// mesh-valid, so validate() is deliberately not the oracle here (see Limits).
const EXTEND_FSM = `fieldsmith 1
name "Extend rename probe"
box b size=(10, 10, 10) at=(5, 5, 5)
region bottom = b.nz
extend skirt bottom dir=(0, 0, -1) len=5
`;

// shape→region selector→load chain, plus a dependent let for the expr rewire.
const CHAIN_FSM = `fieldsmith 1
name "Chain rename probe"
material alsi10mg
let span = 100
let half = span / 2
box beam size=(span, 10, 10) at=(span / 2, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
`;

export async function renameTest(browser, url, { docsDir }) {
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

  try {
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    // ---- A. hx region rename: inlet/outlet must follow the region (A22) --------------
    const hx = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const { renameId } = await import('/src/doc/ops.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const doc = r.ok;
      const baseline = w.validate(doc);
      const renameErr = renameId(doc, 'in_port', 'intake');
      const after = w.validate(doc);
      const printed = w.printFsm(doc);
      const re = w.parseFsm(printed);
      return {
        renameErr,
        hxInlet: doc.hx?.inlet ?? null,
        hxOutlet: doc.hx?.outlet ?? null,
        hasIntake: doc.regions.some((x) => x.id === 'intake'),
        hasInPort: doc.regions.some((x) => x.id === 'in_port'),
        supRegion: doc.supports.find((s) => s.id === 'anchor')?.region ?? null,
        baselineCount: baseline.length,
        newErrors: after.filter((e) => !baseline.includes(e)),
        roundTripOk: !!re.ok,
        roundTripInlet: re.ok?.hx?.inlet ?? null,
        roundTripError: re.ok ? null : (re.error?.message ?? 'reparse failed'),
      };
    }, HX_FSM);
    check('hx: fixture parses', !hx.parseError, hx.parseError ?? 'ok');
    check('hx: rename succeeds', hx.renameErr === null, `err=${hx.renameErr}`);
    check('hx: inlet rewired', hx.hxInlet === 'intake', `hx.inlet=${hx.hxInlet}`);
    check('hx: outlet unchanged', hx.hxOutlet === 'out_port', `hx.outlet=${hx.hxOutlet}`);
    check('hx: region renamed', hx.hasIntake && !hx.hasInPort, `intake=${hx.hasIntake} in_port=${hx.hasInPort}`);
    check('hx: support rewired', hx.supRegion === 'intake', `support.region=${hx.supRegion}`);
    check('hx: no new validate errors', (hx.newErrors ?? []).length === 0, (hx.newErrors ?? []).join(' | ') || `baseline ${hx.baselineCount} errors, 0 new`);
    check('hx: round-trip parses', hx.roundTripOk === true, hx.roundTripError ?? 'ok');
    check('hx: round-trip inlet', hx.roundTripInlet === 'intake', `reparse hx.inlet=${hx.roundTripInlet}`);

    // ---- B. extend region rename: kind.region must follow the region -----------------
    const ext = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const { renameId } = await import('/src/doc/ops.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const doc = r.ok;
      const findExt = (nodes) => {
        for (const n of nodes) {
          if (n.kind?.type === 'extend') return n;
          if (n.kind?.type === 'group') { const f = findExt(n.kind.children); if (f) return f; }
        }
        return null;
      };
      const before = findExt(doc.root)?.kind?.region ?? null;
      const renameErr = renameId(doc, 'bottom', 'floor');
      const afterRegion = findExt(doc.root)?.kind?.region ?? null;
      const printed = w.printFsm(doc);
      const re = w.parseFsm(printed);
      const reExt = re.ok ? (findExt(re.ok.root)?.kind?.region ?? null) : null;
      return {
        renameErr,
        before,
        afterRegion,
        hasFloor: doc.regions.some((x) => x.id === 'floor'),
        hasBottom: doc.regions.some((x) => x.id === 'bottom'),
        printedExtend: printed.split('\n').find((l) => l.startsWith('extend')) ?? '',
        roundTripOk: !!re.ok,
        reExt,
        roundTripError: re.ok ? null : (re.error?.message ?? 'reparse failed'),
      };
    }, EXTEND_FSM);
    check('extend: fixture parses', !ext.parseError, ext.parseError ?? 'ok');
    check('extend: rename succeeds', ext.renameErr === null, `err=${ext.renameErr}`);
    check('extend: kind.region rewired', ext.afterRegion === 'floor', `extend.region ${ext.before}->${ext.afterRegion}`);
    check('extend: region renamed', ext.hasFloor && !ext.hasBottom, `floor=${ext.hasFloor} bottom=${ext.hasBottom}`);
    check('extend: printed line names floor', /\bextend\s+skirt\s+floor\b/.test(ext.printedExtend), ext.printedExtend);
    check('extend: round-trip kind.region', ext.roundTripOk === true && ext.reExt === 'floor', ext.roundTripError ?? `reparse extend.region=${ext.reExt}`);

    // ---- C. shape→region→load chain (regression: already-working paths) --------------
    const chain = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const { renameId } = await import('/src/doc/ops.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const doc = r.ok;
      const nodeErr = renameId(doc, 'beam', 'girder');
      const rootShape = doc.regions.find((x) => x.id === 'root')?.selectors?.[0]?.shape ?? null;
      const tipShape = doc.regions.find((x) => x.id === 'tip')?.selectors?.[0]?.shape ?? null;
      const printed1 = w.printFsm(doc);
      const re1 = w.parseFsm(printed1);
      const regionErr = renameId(doc, 'tip', 'loadface');
      const loadRegion = doc.loads.find((l) => l.id === 'push')?.region ?? null;
      const supRegion = doc.supports.find((s) => s.id === 'clamp')?.region ?? null;
      const printed2 = w.printFsm(doc);
      const re2 = w.parseFsm(printed2);
      return {
        nodeErr, rootShape, tipShape,
        nodeRoundTrip: !!re1.ok,
        regionErr, loadRegion, supRegion,
        regionRoundTrip: !!re2.ok,
      };
    }, CHAIN_FSM);
    check('chain: node rename succeeds', chain.nodeErr === null, `err=${chain.nodeErr}`);
    check('chain: region selectors rewired', chain.rootShape === 'girder' && chain.tipShape === 'girder', `root.shape=${chain.rootShape} tip.shape=${chain.tipShape}`);
    check('chain: node round-trip parses', chain.nodeRoundTrip === true, 'ok');
    check('chain: region rename rewires load', chain.regionErr === null && chain.loadRegion === 'loadface', `load.region=${chain.loadRegion}`);
    check('chain: support region untouched', chain.supRegion === 'root', `support.region=${chain.supRegion}`);
    check('chain: region round-trip parses', chain.regionRoundTrip === true, 'ok');

    // ---- D. error paths leave the document unchanged (A22) ---------------------------
    const errObs = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const { renameId } = await import('/src/doc/ops.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const doc = r.ok;
      const collision = renameId(doc, 'in_port', 'out_port');
      const badIdent = renameId(doc, 'in_port', '9bad');
      const unknown = renameId(doc, 'ghost', 'x');
      return {
        collision, badIdent, unknown,
        inletStill: doc.hx?.inlet ?? null,
        inPortStill: doc.regions.some((x) => x.id === 'in_port'),
        outPortStill: doc.regions.some((x) => x.id === 'out_port'),
      };
    }, HX_FSM);
    check('error: collision rejected', typeof errObs.collision === 'string' && /already used/i.test(errObs.collision), `err=${errObs.collision}`);
    check('error: invalid ident rejected', typeof errObs.badIdent === 'string' && /Names start with/i.test(errObs.badIdent), `err=${errObs.badIdent}`);
    check('error: unknown id rejected', typeof errObs.unknown === 'string' && /Nothing is called/i.test(errObs.unknown), `err=${errObs.unknown}`);
    check('error: doc unchanged after rejects', errObs.inletStill === 'in_port' && errObs.inPortStill && errObs.outPortStill, `hx.inlet=${errObs.inletStill} in_port=${errObs.inPortStill} out_port=${errObs.outPortStill}`);

    // ---- E. let rename rewires a dependent expression (regression) -------------------
    const letObs = await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      const { renameLet } = await import('/src/doc/param.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) return { parseError: r.error?.message ?? 'parse failed' };
      const doc = r.ok;
      const renameErr = renameLet(doc, 'span', 'length');
      const halfExpr = (doc.lets ?? []).find((l) => l.name === 'half')?.expr ?? null;
      const hasLength = (doc.lets ?? []).some((l) => l.name === 'length');
      const hasSpan = (doc.lets ?? []).some((l) => l.name === 'span');
      const printed = w.printFsm(doc);
      const re = w.parseFsm(printed);
      return { renameErr, halfExpr, hasLength, hasSpan, roundTripOk: !!re.ok };
    }, CHAIN_FSM);
    check('let: rename rewires dependent expr', letObs.renameErr === null && letObs.halfExpr === 'length / 2', `half.expr=${letObs.halfExpr}`);
    check('let: slot renamed', letObs.hasLength && !letObs.hasSpan, `length=${letObs.hasLength} span=${letObs.hasSpan}`);
    check('let: round-trip parses', letObs.roundTripOk === true, 'ok');

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Rename reference-rewriting (doc model)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      '`renameId` / `renameLet` must update every site that names the renamed entity so a',
      'renamed document still parses and validates. The region-rename branch rewired the six',
      'analysis-statement region fields but missed two other region references: an `extend`',
      "node's `region` and the heat-exchanger `hx inlet`/`outlet`. Renaming such a region left",
      'a dangling reference the Rust validator rejects (`hx inlet refers to missing region`,',
      "`Extend '…' refers to missing region`). This suite drives the real wasm",
      'parse/print/validate and the real rename functions in-page (no UI, no FE solve). The',
      'decisive oracles are the direct reference-field check, the validate() new-error delta',
      '(after vs baseline), and a printFsm→parseFsm round-trip. Regression guards cover the',
      'shape→region→load chain, the let expression rewire, and the collision / invalid-ident /',
      'unknown-id error paths.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The hx case uses the real A22 geometry/fill/hx; the extend case uses a constructed',
      '  box-parent doc, which parses and round-trips but is not mesh-valid, so validate() is',
      '  not its oracle (a box parent reports "names no mesh patch" before and after alike).',
      '  The extend rewire is asserted by the direct kind.region field and the printed line.',
      '- No FE solve is run; this is a document-model edit, so no field result is asserted.',
      '- The let case asserts the dependent-expr rewire on a let-to-let reference; parametric',
      '  load/consumer expression rewiring is covered by letlifecycle.mjs, not re-driven here.',
      '- fill.protect rewiring on a node rename is implemented but not exercised (no fill with',
      '  a protect list in these fixtures).',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/rename.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['rename: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'rename-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`rename: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}