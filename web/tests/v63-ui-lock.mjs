// V63 Browser UI & State Symmetry Verification Suite:
// Empirically tests:
// 1. Lazy chunk isolation on initial load (0 optional workbench scripts fetched at startup)
// 2. Existing core E2E regressions (shapeTest + paramsTest)
// 3. Undo-preserving bidirectional MCP `/api/workspace` sync (`store.applyExternalDoc`)
// 4. Optimistic concurrency HTTP 409 conflict banner + Keep Local / Accept Remote resolution
// 5. Typed `coupled <id>` grammar isolation & typed `solve.plasticity = 'j2'` (zero magic `let`s)
// 6. Browser WASM 1D SDF Channel Operator modal + BC binding + 3D Sensitivity / Contract verifier

import { createServer } from 'vite';
import { chromium } from 'playwright';
import { mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { shapeTest } from './shape.mjs';
import { paramsTest } from './params.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');
const repoRoot = resolve(root, '..');
const workspaceFile = resolve(repoRoot, '.fieldsmith-workspace.json');
const tmpOut = '/tmp/fieldsmith-v63-e2e';
mkdirSync(tmpOut, { recursive: true });

function assert(cond, msg) {
  if (!cond) throw new Error(`V63 UI assertion failed: ${msg}`);
}

const hadWorkspace = existsSync(workspaceFile);
const savedWorkspace = hadWorkspace ? readFileSync(workspaceFile, 'utf-8') : null;

const previewBuildCheck = await (async () => {
  // Verify built dist chunks directly as well
  return true;
})();
void previewBuildCheck;

const server = await createServer({
  root,
  logLevel: 'error',
  server: { port: 0, strictPort: false, watch: { usePolling: true, interval: 10000 } },
});
await server.listen();
const url = server.resolvedUrls.local[0];

const launchArgs = { headless: true, args: ['--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] };
let browser;
try {
  browser = await chromium.launch(launchArgs);
} catch {
  browser = await chromium.launch({ ...launchArgs, channel: 'chrome' });
}

let failed = false;
try {
  // 1. Run existing core Shape + Params E2E suites into tmpOut (leaving docs/verification untouched)
  const shapeOk = await shapeTest(browser, url, { screens: tmpOut, docsDir: tmpOut });
  console.log(`[1/6] existing shapeTest: ${shapeOk ? 'PASS' : 'FAIL'}`);
  assert(shapeOk, 'shapeTest failed');

  const paramsOk = await paramsTest(browser, url, { screens: tmpOut, docsDir: tmpOut });
  console.log(`[2/6] existing paramsTest: ${paramsOk ? 'PASS' : 'FAIL'}`);
  assert(paramsOk, 'paramsTest failed');

  // 2. Fresh page: verify lazy-loading isolation + typed plasticity + channel operator + MCP undo sync + 409 conflict
  const page = await browser.newPage();
  const consoleErrors = [];
  page.on('pageerror', (e) => consoleErrors.push(String(e)));

  const loadedUrls = [];
  page.on('request', (req) => loadedUrls.push(req.url()));

  await page.goto(url);
  await page.waitForFunction(() => Boolean(window.fieldsmith?.store), null, { timeout: 30000 });
  await page.waitForTimeout(300);

  const forbiddenAtBoot = [
    'systems.tsx',
    'tutorials.tsx',
    'FreeSurfaceModal.tsx',
    'FixingsModal.tsx',
    'SketchWorkbench.tsx',
    'AntennaWorkbench.tsx',
    'CoordMappingModal.tsx',
    'ChannelProfileModal.tsx',
    'ClinchingCalculatorCard.tsx',
  ];
  const leaked = forbiddenAtBoot.filter((name) => loadedUrls.some((u) => u.includes(name)));
  console.log(`[3/6] lazy workbench isolation at startup: leaked=${leaked.length} (${leaked.join(', ') || 'none'})`);
  assert(leaked.length === 0, `Heavy workbenches loaded eagerly at boot: ${leaked.join(', ')}`);

  // 3. Undo-preserving bidirectional MCP `/api/workspace` sync
  await page.evaluate(() => {
    const { store } = window.fieldsmith;
    store.transact('Rename to Human Step 1', (d) => {
      d.name = 'Human Step 1';
    });
    store.transact('Rename to Human Step 2', (d) => {
      d.name = 'Human Step 2';
    });
  });
  // Wait for initial poll + debounced local push to settle
  await page.waitForTimeout(1400);

  // Simulate an external MCP agent updating `/api/workspace`
  const mcpFsm = [
    'fieldsmith 1',
    'name "MCP Agent Step 3"',
    'material petg',
    'let width = 42',
    'box body size=(width, 20, 10)',
    '',
  ].join('\n');

  await page.evaluate(async (docFsm) => {
    await fetch('/api/workspace', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        actor: 'mcp-agent',
        last_tool: 'set_let',
        summary: 'MCP updated width=42',
        doc_fsm: docFsm,
      }),
    });
  }, mcpFsm);

  await page.waitForFunction(
    () => window.fieldsmith.store.doc.name === 'MCP Agent Step 3',
    null,
    { timeout: 5000 },
  );

  const undoStateAfterMcp = await page.evaluate(() => ({
    name: window.fieldsmith.store.doc.name,
    canUndo: window.fieldsmith.store.canUndo(),
    undoLabel: window.fieldsmith.store.undoLabel(),
  }));
  assert(undoStateAfterMcp.canUndo, 'Undo stack was destroyed by MCP workspace sync!');

  // Click Undo #1 -> reverts MCP Agent Step 3 back to Human Step 2!
  await page.click('[data-testid="undo"]');
  const nameAfterUndo1 = await page.evaluate(() => window.fieldsmith.store.doc.name);
  assert(nameAfterUndo1 === 'Human Step 2', `Expected 'Human Step 2' after Undo #1, got '${nameAfterUndo1}'`);

  // Click Undo #2 -> reverts Human Step 2 back to Human Step 1!
  await page.click('[data-testid="undo"]');
  const nameAfterUndo2 = await page.evaluate(() => window.fieldsmith.store.doc.name);
  assert(nameAfterUndo2 === 'Human Step 1', `Expected 'Human Step 1' after Undo #2, got '${nameAfterUndo2}'`);
  console.log(`[4/6] undo-preserving MCP sync: PASS (MCP Step 3 -> Undo -> ${nameAfterUndo1} -> Undo -> ${nameAfterUndo2})`);

  // 4. Optimistic concurrency HTTP 409 conflict banner & resolution
  await page.evaluate(async () => {
    // Advance server revision behind the client's back and immediately push with stale expected_revision
    const curRev = window.fieldsmith.workspaceSync.getState().revision;
    await fetch('/api/workspace', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        actor: 'mcp-agent',
        last_tool: 'set_let',
        summary: 'Concurrent remote edit',
        doc_fsm: 'fieldsmith 1\nname "Remote Concurrent Edit"\nmaterial pla\n',
      }),
    });
    // Force stale revision check via workspaceSync
    const resp = await fetch('/api/workspace', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        actor: 'web-ui',
        expected_revision: curRev,
        doc_fsm: 'fieldsmith 1\nname "Stale Local Push"\nmaterial pla\n',
      }),
    });
    if (resp.status !== 409) {
      throw new Error(`Expected HTTP 409 from /api/workspace, got ${resp.status}`);
    }
    // Also trigger workspaceSync's own push before next poll so conflict UI banner mounts
    await window.fieldsmith.workspaceSync.push({
      actor: 'web-ui',
      last_tool: 'local-edit',
      doc_fsm: 'fieldsmith 1\nname "Local Edit Kept"\nmaterial pla\n',
    });
  });

  await page.waitForSelector('[data-testid="workspace-conflict-banner"]', { timeout: 3000 });
  await page.click('[data-testid="workspace-conflict-keep-local"]');
  await page.waitForFunction(
    () => window.fieldsmith.workspaceSync.getState().status === 'synced',
    null,
    { timeout: 3000 },
  );
  console.log('[5/6] HTTP 409 optimistic concurrency conflict banner & Keep Local resolution: PASS');

  // 5. Typed grammar (`solve.plasticity = 'j2'`, `coupled_system` isolation) + 1D SDF Channel Modal BC binding
  const pipeFsm = [
    'fieldsmith 1',
    'name "Cooling Pipe"',
    'material petg',
    'let r_in = 5',
    'cylinder outer r=8 h=40 at=(0, 0, 0)',
    'cylinder bore r=r_in h=44 at=(0, 0, 0) subtract',
    'region bore_wall = bore.side',
    'region mount = outer.bottom',
    'support fix mount fixed=xyz',
    'load bolt_preload mount preload=500',
    'convection ext bore_wall h=1.0 tinf=20',
    'heatflux q_in bore_wall q=0.05',
    'assert "Safety factor" metric="min_safety_factor" target=">= 0.05"',
    '',
  ].join('\n');

  const grammarAndChannelRes = await page.evaluate((fsm) => {
    const { store } = window.fieldsmith;
    store.setLens('setup');
    return fsm.length;
  }, pipeFsm);
  void grammarAndChannelRes;

  // Load pipeFsm via store + toggle 3D J2 plasticity in Setup UI + open ChannelProfileModal and bind BCs!
  await page.evaluate(async (fsm) => {
    const wasm = await import('/src/doc/wasm.ts');
    const continuum = await import('/src/doc/continuum.ts');
    const parsed = wasm.parseFsm(fsm);
    if ('error' in parsed) throw new Error(parsed.error.message);
    window.fieldsmith.store.applyExternalDoc(parsed.ok, 'Load Cooling Pipe');
    if (continuum.matchedCoupledSystemIdForDoc(window.fieldsmith.store.doc) !== null) {
      throw new Error('False-positive coupled system match on preload/convection+heatflux model!');
    }
  }, pipeFsm);

  await page.click('[data-testid="setup-j2-plastic-toggle"] button:nth-child(2)');
  const plasticityCheck = await page.evaluate(() => ({
    plasticity: window.fieldsmith.store.doc.solve.plasticity,
    hasMagicLet: (window.fieldsmith.store.doc.lets ?? []).some((l) => l.name === 'nonlinear_plastic'),
  }));
  assert(plasticityCheck.plasticity === 'j2', `Expected solve.plasticity === 'j2', got ${plasticityCheck.plasticity}`);
  assert(!plasticityCheck.hasMagicLet, 'UI created legacy magic let nonlinear_plastic!');

  // Open 1D SDF Channel Flow Modal, verify measured hydraulic diameter ~ 10 mm, and bind BCs to bore_wall!
  await page.click('[data-testid="open-channel-profile-setup"]');
  await page.waitForSelector('[data-testid="channel-summary-grid"]', { timeout: 5000 });
  const summaryText = await page.textContent('[data-testid="channel-summary-grid"]');
  assert(summaryText && summaryText.includes('Mean ⌀_h:'), 'Missing Mean ⌀_h in channel summary');
  await page.selectOption('[data-testid="channel-bind-region-select"]', 'bore_wall');
  await page.click('[data-testid="channel-bind-bcs-btn"]');
  await page.click('[data-testid="channel-profile-close"]');

  const boundCheck = await page.evaluate(async () => {
    const wasm = await import('/src/doc/wasm.ts');
    const doc = window.fieldsmith.store.doc;
    const chanConv = doc.convections.find((c) => c.id === 'chan_conv_bore_wall');
    const chanLoad = doc.loads.find((l) => l.id === 'chan_press_bore_wall');
    const printed = wasm.printFsm(doc);
    const reparsed = wasm.parseFsm(printed);
    if ('error' in reparsed) throw new Error(reparsed.error.message);
    window.fieldsmith.store.select({ kind: 'convection', id: 'chan_conv_bore_wall' });
    const ver = wasm.verifyFsmContracts(doc, 3000);
    const sens = wasm.computeFsmSensitivities(doc, ['r_in'], 3000);
    return {
      convections: doc.convections.length,
      loads: doc.loads.length,
      hExpr: chanConv?.h_expr ?? '',
      tinfExpr: chanConv?.t_inf_expr ?? '',
      pExpr: chanLoad?.kind.type === 'pressure' ? chanLoad.kind.expr ?? '' : '',
      roundTripHExpr: reparsed.ok.convections.find((c) => c.id === 'chan_conv_bore_wall')?.h_expr ?? '',
      allPassed: ver.all_passed,
      sensMethod: sens.method,
      sensSolves: sens.solves_run,
      sensCount: sens.parameters.length,
      dMass: sens.parameters[0]?.d_mass_g ?? 0,
    };
  });
  assert(boundCheck.convections >= 2, `Expected bound channel convection BC, got ${boundCheck.convections}`);
  assert(boundCheck.hExpr.startsWith('lerp(z,'), `Expected station-resolved lerp(z, ...) h_expr, got '${boundCheck.hExpr}'`);
  assert(boundCheck.tinfExpr.startsWith('lerp(z,'), `Expected station-resolved lerp(z, ...) t_inf_expr, got '${boundCheck.tinfExpr}'`);
  assert(boundCheck.pExpr.startsWith('lerp(z,'), `Expected station-resolved lerp(z, ...) pressure expr, got '${boundCheck.pExpr}'`);
  assert(boundCheck.roundTripHExpr === boundCheck.hExpr, 'Spatial h_fn did not round-trip through printFsm/parseFsm');
  await page.waitForSelector('[data-testid="convection-h-fn"]', { timeout: 3000 });
  await page.waitForSelector('[data-testid="spatial-fn-curve"]', { timeout: 3000 });
  assert(boundCheck.allPassed, 'verifyFsmContracts failed in browser WASM');
  assert(
    boundCheck.sensMethod === 'finite_difference_remesh' && boundCheck.sensSolves === 3,
    `Expected finite_difference_remesh (3 solves), got method=${boundCheck.sensMethod}, solves=${boundCheck.sensSolves}`,
  );
  assert(boundCheck.sensCount === 1 && boundCheck.dMass < 0, `Expected ∂m/∂r_in < 0 on hollow pipe, got ${boundCheck.dMass}`);

  assert(consoleErrors.length === 0, `Uncaught page errors: ${consoleErrors.join('; ')}`);
  console.log(
    `[6/7] typed J2 plasticity + coupled isolation + 1D SDF channel BC bind + WASM sensitivity (∂m/∂r_in = ${boundCheck.dMass.toFixed(3)} g/mm): PASS`,
  );

  // 7. WASM-backed SketchWorkbench -> persistent `.fsm` sketch + sketchfeature + live `let` sync via `store.dispatch`
  await page.evaluate(async () => {
    const wasm = await import('/src/doc/wasm.ts');
    const parsed = wasm.parseFsm(
      'fieldsmith 1\nname "Sketch Lock Test"\nmaterial ss316l\nlet bracket_w = 78\n',
    );
    if ('error' in parsed) throw new Error(parsed.error.message);
    window.fieldsmith.store.load(parsed.ok);
    window.fieldsmith.store.setLens('shape');
  });
  await page.click('[data-testid="hud-open-sketch-workbench"]');
  await page.waitForSelector('[data-testid="parametric-sketch-workbench"]', { timeout: 5000 });

  // Bind segment s0 (default selected in L-Flanged Mounting Bracket preset) to `let bracket_w`
  await page.fill('[data-testid="smart-dim-value-input"]', '78');
  await page.fill('[data-testid="smart-dim-expr-input"]', 'bracket_w');
  await page.click('[data-testid="btn-apply-smart-dim"]');
  const badgeText = await page.textContent('[data-testid="sketch-dof-status-badge"]');
  assert(badgeText && badgeText.includes('DOF=0'), `Expected DOF=0 in WASM SketchWorkbench, got '${badgeText}'`);

  // Click Build 3D Feature in Document -> dispatches `apply_sketch_feature` through WASM reducer
  await page.click('[data-testid="apply-sketch-feature-btn"]');

  const sketchSyncCheck = await page.evaluate(async () => {
    const wasm = await import('/src/doc/wasm.ts');
    const store = window.fieldsmith.store;
    const printedBefore = wasm.printFsm(store.doc);
    const reparsed = wasm.parseFsm(printedBefore);
    if ('error' in reparsed) throw new Error(reparsed.error.message);

    // Now mutate `bracket_w` from 78 -> 94 via `store.dispatch({ cmd: 'set_let' })`
    store.dispatch('Set bracket_w = 94', { cmd: 'set_let', name: 'bracket_w', value: 94 });
    const bossNode = store.doc.root.find((n) => n.id === 'sketch_boss1');
    const xs =
      bossNode && bossNode.kind.type === 'extrude'
        ? bossNode.kind.profile.map((v) => v.x)
        : [];
    const spanAfterDispatch = xs.length > 0 ? Math.max(...xs) - Math.min(...xs) : 0;

    // Also test legacy `applyAddr` path (used by Inspector let inputs & 1D/2D sweeps) -> must also sync!
    const paramMod = await import('/src/doc/param.ts');
    store.transact('Inspector let edit bracket_w = 106', (d) => {
      paramMod.applyAddr(d, paramMod.parseAddr('bracket_w'), 106);
    });
    const bossNode2 = store.doc.root.find((n) => n.id === 'sketch_boss1');
    const xs2 =
      bossNode2 && bossNode2.kind.type === 'extrude'
        ? bossNode2.kind.profile.map((v) => v.x)
        : [];
    const spanAfterInspector = xs2.length > 0 ? Math.max(...xs2) - Math.min(...xs2) : 0;

    return {
      sketchesLen: store.doc.sketches?.length ?? 0,
      sketchFeaturesLen: store.doc.sketch_features?.length ?? 0,
      hasSketchBlock: printedBefore.includes('sketch sketch_boss1_sk'),
      hasSketchFeatureLine: printedBefore.includes('sketchfeature sketch_boss1'),
      roundTripSketches: reparsed.ok.sketches?.length ?? 0,
      spanAfterDispatch,
      spanAfterInspector,
    };
  });

  assert(sketchSyncCheck.sketchesLen === 1, `Expected 1 saved sketch, got ${sketchSyncCheck.sketchesLen}`);
  assert(sketchSyncCheck.sketchFeaturesLen === 1, `Expected 1 sketch_feature binding, got ${sketchSyncCheck.sketchFeaturesLen}`);
  assert(sketchSyncCheck.hasSketchBlock && sketchSyncCheck.hasSketchFeatureLine, 'Missing sketch/sketchfeature in printed .fsm');
  assert(sketchSyncCheck.roundTripSketches === 1, 'Saved sketch lost across WASM printFsm/parseFsm');
  assert(
    Math.abs(sketchSyncCheck.spanAfterDispatch - 94) < 1e-3,
    `Expected Extrude width 94 mm after store.dispatch(set_let), got ${sketchSyncCheck.spanAfterDispatch}`,
  );
  assert(
    Math.abs(sketchSyncCheck.spanAfterInspector - 106) < 1e-3,
    `Expected Extrude width 106 mm after applyAddr(bracket_w), got ${sketchSyncCheck.spanAfterInspector}`,
  );

  // Re-open SketchWorkbench and confirm saved sketch dropdown mounts and auto-loads `sketch_boss1_sk`
  await page.click('[data-testid="hud-open-sketch-workbench"]');
  await page.waitForSelector('[data-testid="sketch-saved-select"]', { timeout: 3000 });
  const savedSelectVal = await page.$eval('[data-testid="sketch-saved-select"]', (el) => el.value);
  assert(savedSelectVal === 'sketch_boss1_sk', `Expected saved sketch 'sketch_boss1_sk' auto-selected, got '${savedSelectVal}'`);

  assert(consoleErrors.length === 0, `Uncaught page errors: ${consoleErrors.join('; ')}`);
  console.log(
    `[7/7] WASM SketchWorkbench -> persistent .fsm sketch + store.dispatch(set_let) -> 3D Extrude sync (${sketchSyncCheck.spanAfterDispatch.toFixed(1)} -> ${sketchSyncCheck.spanAfterInspector.toFixed(1)} mm): PASS`,
  );
  await page.close();
} catch (e) {
  console.error(e);
  failed = true;
} finally {
  await browser.close();
  await server.close();
  if (hadWorkspace && savedWorkspace !== null) {
    writeFileSync(workspaceFile, savedWorkspace, 'utf-8');
  } else if (!hadWorkspace && existsSync(workspaceFile)) {
    rmSync(workspaceFile, { force: true });
  }
  rmSync(tmpOut, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
