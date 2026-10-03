// One-off probe for the GOAL.md B/C claims the bracket script does not cover:
//   B: blended-face region pick, per-axis support, pressure load, second load case +
//      envelope, custom material.
// The study is built through the UI exactly as in bracket.mjs (shapes, then the setup
// lens). Phase results print as PASS/FAIL lines with evidence; a summary block at the end
// mirrors it. Custom materials have no UI to add them, so that phase uses the store API
// the inspector itself reads — the probe reports it that way rather than implying a UI text.
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: new URL('..', import.meta.url).pathname, logLevel: 'error', server: { port: 0, strictPort: false } });
await server.listen();
const url = server.resolvedUrls.local[0];
let browser;
try {
  browser = await chromium.launch({ args: ['--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
} catch {
  browser = await chromium.launch({ args: ['--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'], channel: 'chrome' });
}
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const consoleErrors = [];
page.on('console', (m) => {
  if (m.type() === 'error') consoleErrors.push(m.text());
});
await page.goto(url);
await page.evaluate(() => localStorage.clear());
await page.reload();
await page.waitForSelector('[data-testid="empty-state"]', { timeout: 30000 });

const results = [];
const check = (label, ok, detail) => {
  results.push({ label, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} — ${label}: ${detail}`);
};
const step = async (_label, fn) => fn();

async function setNum(testId, value) {
  const el = page.locator(`[data-testid="${testId}"]`);
  await el.click();
  await el.fill(String(value));
  await el.press('Enter');
}
async function setVec(testId, v) {
  for (const [i, a] of ['x', 'y', 'z'].entries()) await setNum(`${testId}-${a}`, v[i]);
}
async function renameWith(name) {
  const el = page.locator('[data-testid="name"]');
  await el.click();
  await el.fill(name);
  await el.press('Enter');
}
async function addShape(type) {
  await page.locator('[data-action="add-menu"]').click();
  await page.locator(`[data-testid="add-${type}"]`).click();
}
async function setOp(op) {
  await page.locator(`[data-testid="op"] button[data-value="${op}"]`).click();
}

/** Wait for a fresh solved run: ready, ≥1 level, for the CURRENT docVersion. */
async function waitReady(label, timeout = 25000) {
  const t0 = Date.now();
  for (;;) {
    const s = await page.evaluate(() => {
      const a = window.fieldsmith.analysis.state;
      return { status: a.status, error: a.error, levels: a.levels.length, resultsFor: a.resultsFor, dv: window.fieldsmith.store.docVersion };
    });
    if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) return s;
    if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`${label}: analysis error: ${s.error}`);
    if (Date.now() - t0 > timeout) throw new Error(`${label}: timed out (last status ${s.status})`);
    await page.waitForTimeout(200);
  }
}

// ---- shapes (the bracket, via the UI) ---------------------------------------------------

await step('base', async () => {
  await page.locator('[data-testid="empty-add-box"]').click();
  await renameWith('base');
  await setVec('size', [60, 40, 8]);
  await setVec('position', [30, 20, 4]);
});
await step('upright', async () => {
  await addShape('box');
  await renameWith('upright');
  await setVec('size', [60, 8, 40]);
  await setVec('position', [30, 4, 20]);
  await setNum('blend', 8);
});
for (const [name, x] of [['hole1', 15], ['hole2', 45]]) {
  await step(name, async () => {
    await addShape('cylinder');
    await renameWith(name);
    await setOp('subtract');
    await setNum('diameter', 6.5);
    await setNum('height', 20);
    await setVec('position', [x, 26, 4]);
  });
}
await step('hole3', async () => {
  await addShape('cylinder');
  await renameWith('hole3');
  await setOp('subtract');
  await setNum('diameter', 8.5);
  await setNum('height', 20);
  await setVec('rotation', [90, 0, 0]);
  await setVec('position', [30, 4, 28]);
});
await step('setup lens', async () => {
  await page.locator('[data-lens="setup"]').click();
  await page.waitForSelector('[data-testid="add-support"]', { timeout: 5000 });
  await page.evaluate(() => {
    window.fieldsmith.view.view([0, 1, 1]);
    window.fieldsmith.view.fit(false);
  });
});

const deg = (d) => (d * Math.PI) / 180;
const vertWall = (cx, cy) => {
  const c = [];
  for (const z of [3, 4, 5]) for (let a = 250; a <= 290; a += 4) c.push([cx + 3.25 * Math.cos(deg(a)), cy + 3.25 * Math.sin(deg(a)), z]);
  return c;
};
const endWall = (cx, cz) => {
  const c = [];
  for (const y of [1.5, 3, 4.5]) for (let a = 250; a <= 290; a += 4) c.push([cx + 4.25 * Math.cos(deg(a)), y, cz + 4.25 * Math.sin(deg(a))]);
  return c;
};
const clickSurface = async (shape, face, cands) => {
  const p = await page.evaluate(([sh, fc, cs]) => {
    const F = window.fieldsmith;
    for (const q of cs) {
      const s = F.view.project(q);
      if (!s) continue;
      const h = F.pickAt(s.x, s.y);
      if (h && h.shape === sh && h.face === fc) return { x: s.x, y: s.y };
    }
    return null;
  }, [shape, face, cands]);
  if (!p) throw new Error(`no visible ${face} face of ${shape} to click`);
  await page.mouse.click(p.x, p.y);
};

// ---- phase 1: a blended face is pickable as a region ------------------------------------
// The upright is a fully rounded slab (60 × 8 × 40, blend 8): no flat faces on its
// silhouette faces — the y=8 surface is a blend arc around (y 0, z 32). Candidates sit
// just outside that arc, x mid-plate away from the end rounds.
{
  const cands = [];
  for (const x of [16, 30, 46]) {
    for (const phi of [10, 30, 45, 60]) {
      const r = 8.4;
      cands.push([x, r * Math.cos(deg(phi)), 32 + r * Math.sin(deg(phi))]);
    }
  }
  const hit = await page.evaluate((cs) => {
    const F = window.fieldsmith;
    for (const q of cs) {
      const s = F.view.project(q);
      if (!s) continue;
      const h = F.pickAt(s.x, s.y);
      if (h && h.shape === 'upright') return { face: h.face, point: h.point };
    }
    return null;
  }, cands);
  if (!hit || hit.face === null) throw new Error('no blend face of the upright pickable from the [0,1,1] camera');
  const p = await page.evaluate((pt) => {
    const F = window.fieldsmith;
    const s = F.view.project(pt);
    const h = F.pickAt(s.x, s.y);
    return h && h.shape === 'upright' ? { x: s.x, y: s.y } : null;
  }, hit.point);
  if (!p) throw new Error('blend point not clickable');
  await page.mouse.click(p.x, p.y);
  await renameWith('shoulder');
  const region = await page.evaluate(() => {
    const d = window.fieldsmith.store.doc;
    return d.regions.find((r) => r.id === 'shoulder') ?? null;
  });
  const ok = !!region && region.selectors.length >= 1 && region.selectors.every((s) => s.shape === 'upright' && s.face === hit.face);
  check('blended face picked as a region', ok, `selector ${JSON.stringify(region?.selectors)} (face code '${hit.face}')`);
  await page.keyboard.press('Escape');
}

// ---- phase 2: per-axis support ----------------------------------------------------------

await step('regions: base holes + pin hole', async () => {
  await clickSurface('hole1', 'side', vertWall(15, 26));
  await clickSurface('hole2', 'side', vertWall(45, 26));
  await renameWith('base_holes');
  await page.keyboard.press('Escape');
  await clickSurface('hole3', 'side', endWall(30, 28));
  await renameWith('upright_hole');
});
await step('supports: fixed-y off on the holes, y-only hold on the shoulder', async () => {
  // Freeing y at the holes leaves one free rigid mode (y translation); the solver refuses
  // that study outright ("stiffness matrix is not positive definite") rather than solve a
  // singular system — so the per-axis study adds a y-only hold on the shoulder region.
  await page.locator('[data-testid="region-base_holes"]').click();
  await page.locator('[data-testid="add-support"]').click();
  await page.locator('[data-testid="fixed-y"]').uncheck();
  await page.keyboard.press('Escape');
  await page.locator('[data-testid="region-shoulder"]').click();
  await page.locator('[data-testid="add-support"]').click();
  await page.locator('[data-testid="fixed-x"]').uncheck();
  await page.locator('[data-testid="fixed-z"]').uncheck();
});
await step('load, pure -z', async () => {
  await page.locator('[data-testid="region-upright_hole"]').click();
  await page.locator('[data-testid="add-load"]').click();
  await renameWith('pull');
  // Pure -z: with y un-fixed the only free rigid mode is +y translation, which a purely
  // vertical load cannot excite.
  await setVec('force', [0, 0, -400]);
});
{
  const s = await waitReady('per-axis support');
  const doc = await page.evaluate(() => {
    const F = window.fieldsmith;
    const a = F.analysis.state;
    const lvl = a.levels[a.levels.length - 1];
    return {
      supports: F.store.doc.supports.map((x) => ({ id: x.id, fixed: x.fixed })),
      peak: lvl.meta.cases[0].max_vm_elem,
      defl: lvl.meta.cases[0].max_displacement,
    };
  });
  const fixed = doc.supports.map((x) => x.fixed);
  const ok =
    s.levels >= 2 &&
    JSON.stringify(fixed) === '[[true,false,true],[false,true,false]]' &&
    isFinite(doc.peak) && doc.peak > 0;
  check('per-axis supports (X,Z on holes; Y only on shoulder) solve', ok, `fixed ${JSON.stringify(fixed)}, levels ${s.levels}, peak ${doc.peak.toFixed(1)} MPa`);
}

// ---- phase 3: pressure load -------------------------------------------------------------

{
  await page.locator('[data-testid="load-kind"] button[data-value="pressure"]').click();
  await setNum('pressure', 0.5);
  const s = await waitReady('pressure load');
  const doc = await page.evaluate(() => {
    const a = window.fieldsmith.analysis.state;
    const lvl = a.levels[a.levels.length - 1];
    return { kind: window.fieldsmith.store.doc.loads[0].kind, cases: lvl.meta.cases.length, peak: lvl.meta.cases[0].max_vm_elem };
  });
  const ok = doc.kind.type === 'pressure' && doc.kind.mpa === 0.5 && doc.cases === 1 && isFinite(doc.peak) && doc.peak > 0;
  check('pressure load (0.5 MPa) solves end-to-end', ok, `${JSON.stringify(doc.kind)}, peak ${doc.peak.toFixed(2)} MPa`);
}

// ---- phase 4: second load case + envelope ------------------------------------------------

{
  await page.locator('[data-testid="region-upright_hole"]').click();
  await page.locator('[data-testid="add-load"]').click();
  await renameWith('pull2');
  // load-case is a text input; the case name is free text (the .fsm parser allows spaces).
  const el = page.locator('[data-testid="load-case"]');
  await el.click();
  await el.fill('Load 2');
  await el.press('Enter');
  await setVec('force', [0, 0, 150]); // opposite sign: the envelope must differ from case 0
  await waitReady('second load case');
}
{
  // Switch to results and drive the case segment control.
  await page.locator('[data-lens="results"]').click();
  await page.waitForSelector('[data-testid="results-panel"]', { timeout: 5000 });
  const meta = await page.evaluate(() => {
    const a = window.fieldsmith.analysis.state;
    const lvl = a.levels[a.levels.length - 1];
    return {
      names: lvl.meta.cases.map((c) => c.name),
      peaks: lvl.meta.cases.map((c) => c.max_vm_elem),
      envelopePeak: Math.max(0, ...lvl.vmElemEnvelope),
    };
  });
  const seg = page.locator('[data-testid="results-case"]');
  const segPresent = await seg.count() > 0;
  let switched = '';
  if (segPresent) {
    const states = [];
    for (const v of ['-1', '0', '1']) {
      await seg.locator(`button[data-value="${v}"]`).click();
      const c = await page.evaluate(() => window.fieldsmith.ui.resultsCase);
      states.push(c);
    }
    switched = `resultsCase after clicks: ${JSON.stringify(states)}`;
  }
  const ok = meta.names.length === 2 && meta.names[1] === 'Load 2' && meta.peaks[0] !== meta.peaks[1] && segPresent && switched.endsWith('[-1,0,1]');
  check('second load case + envelope switch', ok, `cases ${JSON.stringify(meta.names)}, peaks ${meta.peaks.map((p) => p.toFixed(1)).join(' / ')} MPa, ${switched}`);
  // leave the envelope on
  if (segPresent) await seg.locator('button[data-value="-1"]').click();
}

// ---- phase 4b: all result modes + scalable deformed shape -------------------------------

{
  const out = [];
  for (const m of ['stress', 'safety', 'deflection', 'vectors']) {
    await page.locator(`[data-testid="results-mode"] button[data-value="${m}"]`).click();
    await page.waitForTimeout(400);
    const state = await page.evaluate(() => window.fieldsmith.ui.resultsMode);
    out.push(`${m}=${state === m ? 'ok' : 'unset'}`);
  }
  // The Deform × input scales the drawn deformation.
  const before = await page.evaluate(() => window.fieldsmith.ui.resultsScale);
  await setNum('results-scale', 12);
  const after = await page.evaluate(() => window.fieldsmith.ui.resultsScale);
  const errors = consoleErrors.length;
  const ok = errors === 0 && out.every((s) => s.endsWith('=ok')) && after === 12 && before !== 12;
  check(
    'all four result modes + deformed-scale input switch without console errors',
    ok,
    `${out.join(', ')}; deform × ${before} → ${after}; console errors ${errors}`,
  );
  await setNum('results-scale', before);
}

// ---- phase 5: custom material -------------------------------------------------------------

{
  // Custom materials have no editor UI: they enter the doc through the store API (or the
  // .fsm text) and are then driven through the real material <select>, which shows them.
  await page.locator('[data-lens="setup"]').click();
  await page.evaluate(() => {
    const F = window.fieldsmith;
    F.store.transact('probe custom material', (d) => {
      d.custom_materials.push({
        id: 'probe_mat', name: 'Probe alloy', e: 0.6, nu: 0.36, yield_strength: 21, density: 1.05, min_wall: 0.5,
        source: 'probe material, invented for this test only',
      });
    });
  });
  await page.keyboard.press('Escape'); // clear the selection so the DocumentInspector shows
  // A closed <select>'s options are never 'visible' to Playwright, so wait for attachment.
  await page.waitForSelector('[data-testid="material"] option[value="probe_mat"]', { timeout: 3000, state: 'attached' });
  await page.selectOption('[data-testid="material"]', 'probe_mat');
  const s = await waitReady('custom material');
  const out = await page.evaluate(() => {
    const F = window.fieldsmith;
    const lvl = F.analysis.state.levels[F.analysis.state.levels.length - 1];
    const ids = [...document.querySelectorAll('[data-testid="material"] option')].map((o) => o.value);
    return { material: F.store.doc.material, yield: lvl.meta.yield_strength, ids, sourceShown: document.body.innerText.includes('probe material, invented for this test only') };
  });
  const lib = ['pla', 'petg', 'abs', 'pa12', 'alsi10mg', 'ti64', 'ss316l'];
  const ok =
    out.material === 'probe_mat' && Math.abs(out.yield - 21) < 1e-9 && lib.every((m) => out.ids.includes(m)) && out.ids.includes('probe_mat');
  check('custom material drives the solver, listed in the picker', ok, `doc.material '${out.material}', meta yield ${out.yield} MPa, picker ${out.ids.length} options`);
  check('the cited source string is user-visible', out.sourceShown, out.sourceShown ? "the inspector renders the material's source text" : 'source text not found in the page');
}

// ---- summary ------------------------------------------------------------------------------

const failed = results.filter((r) => !r.ok);
console.log('---');
for (const r of results) console.log(`${r.ok ? 'PASS' : 'FAIL'} ${r.label} — ${r.detail}`);
console.log(`console errors during probe: ${consoleErrors.length}${consoleErrors.length ? ': ' + JSON.stringify(consoleErrors) : ''}`);
console.log(failed.length ? `PROBE: ${failed.length} claim(s) FAILED` : 'PROBE: all claims pass');

await browser.close();
await server.close();