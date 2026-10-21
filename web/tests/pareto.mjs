// V47 — Interactive Pareto frontier explorer in Results (M25).
//
// Run with: node web/tests/run-e2e.mjs --only pareto
//
// Drives a 2D `let`-parameter grid sweep (`w ∈ {8, 12}` × `h ∈ {8, 12}`, 4 candidates)
// on a cantilevered bracket/beam under combined load `force=(0, -30, -100)` where Z-bending
// dominates Y-bending:
//   - Point 0 `(w=8,  h=8)`:  lightest mass -> Pareto-optimal (`data-pareto="true"`)
//   - Point 1 `(w=8,  h=12)`: medium mass, aligned with dominant Z load -> Pareto-optimal (`data-pareto="true"`)
//   - Point 2 `(w=12, h=8)`:  same mass as (8, 12) but higher stress/compliance/deflection -> strictly dominated (`data-pareto="false"`)
//   - Point 3 `(w=12, h=12)`: heaviest mass, lowest stress/compliance/deflection -> Pareto-optimal (`data-pareto="true"`)
//
// Verifies:
//   1. The 2D `let` sweep completes all 4 points while leaving the live document at `(w=10, h=10)`.
//   2. `[data-testid="pareto-plot"]` renders 4 circles and a frontier polyline; `data-pareto`
//      identifies `{0, 1, 3}` as Pareto-optimal and `{2}` as dominated across `vm`, `safety`,
//      `compliance`, `peel`, and `deflection`.
//   3. Clicking frontier point 3 (`w=12, h=12`) updates both `let w = 12` and `let h = 12` in
//      one undoable `store.transact` step, updates `.fsm` output, and changes 3D viewport pixels.
//   4. Pressing `Ctrl+Z` restores `.fsm` text and 3D viewport pixels to the initial `(w=10, h=10)` state.

import { writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';

const PARETO_DOC = `\
fieldsmith 1
name "Pareto Bracket V47"
material petg
let w = 10
let h = 10
let hw = w / 2
let hh = h / 2
solve size=5

box beam size=(60, w, h) at=(30, hw, hh)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, -30, -100) case="Load 1"
`;

async function until(page, fn, arg, ms = 60000, note = '') {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (await page.evaluate(fn, arg)) return;
    await page.waitForTimeout(200);
  }
  throw new Error(`timed out${note ? ` (${note})` : ''}`);
}

export async function paretoTest(browser, url, { screens, docsDir }) {
  mkdirSync(docsDir, { recursive: true });
  mkdirSync(screens, { recursive: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 920 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });

  try {
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    await page.evaluate(async (fsm) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(fsm);
      if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
      window.fieldsmith.store.load(r.ok);
      window.__wasm = w;
      window.__sweep = await import('/src/app/sweep.ts');
      window.__param = await import('/src/doc/param.ts');
    }, PARETO_DOC);

    await page.locator('[data-lens="results"]').click();
    await until(
      page,
      () => window.fieldsmith.analysis.state.status === 'ready',
      null,
      120000,
      'initial analysis ready',
    );
    await page.waitForTimeout(350);

    // Capture initial `.fsm` text and initial WebGL canvas pixels.
    const initialSnap = await page.evaluate(() => {
      const fsm = window.__wasm.printFsm(window.fieldsmith.store.doc);
      const vp = window.fieldsmith.viewport;
      vp.render();
      const gl = vp.canvas.getContext('webgl2') || vp.canvas.getContext('webgl');
      const w = vp.canvas.width;
      const h = vp.canvas.height;
      const buf = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
      window.__capInitial = buf;
      return { fsm, w, h };
    });

    // Configure a 2D `let` sweep: w from 8 to 12 (2 steps) × h from 8 to 12 (2 steps) = 4 points.
    await page.selectOption('[data-testid="sweep-param"]', 'w');
    await until(page, () => window.fieldsmith.ui.sweepParam === 'w', null, 5000, 'select w');
    await page.evaluate(() => {
      const ui = window.fieldsmith.ui;
      ui.set('sweepParam', 'w');
      ui.set('sweepFrom', 8);
      ui.set('sweepTo', 12);
      ui.set('sweepCount', 2);
      ui.set('sweepParam2', 'h');
      ui.set('sweepFrom2', 8);
      ui.set('sweepTo2', 12);
      ui.set('sweepCount2', 2);
    });
    await page.waitForTimeout(100);

    const t0 = Date.now();
    await page.locator('[data-testid="sweep-run"]').click();
    await until(
      page,
      () => {
        const s = window.__sweep.sweep.state;
        return s.done === true || (!s.running && s.error !== null);
      },
      null,
      240000,
      '2D sweep complete',
    );
    const sweepMs = Date.now() - t0;

    const swState = await page.evaluate(() => {
      const s = window.__sweep.sweep.state;
      const doc = window.fieldsmith.store.doc;
      return {
        done: s.done,
        error: s.error,
        param: s.param,
        param2: s.param2,
        count: s.points.length,
        points: s.points.map((p) => ({
          values: p.values,
          mass_g: p.mass_g,
          layer_peel: p.layer_peel,
          vm_mpa: p.records[p.records.length - 1]?.tracked.vm_mpa ?? 0,
          defl_mm: p.records[p.records.length - 1]?.tracked.deflection_mm ?? 0,
          compl_nmm: p.records[p.records.length - 1]?.tracked.compliance_nmm ?? 0,
        })),
        liveW: doc.lets.find((l) => l.name === 'w')?.value,
        liveH: doc.lets.find((l) => l.name === 'h')?.value,
      };
    });

    check(
      '2D let grid sweep (`w × h`, 2×2 = 4 candidates) completes without error',
      swState.done && swState.error === null && swState.count === 4 && swState.param === 'w' && swState.param2 === 'h',
      `done=${swState.done}, count=${swState.count}, error=${swState.error}`,
    );
    check(
      'live document keeps initial let values `(w=10, h=10)` during sweep',
      swState.liveW === 10 && swState.liveH === 10,
      `w=${swState.liveW}, h=${swState.liveH}`,
    );

    // Check Pareto plot SVG and non-dominated frontier across metrics.
    const plotCheck = await page.evaluate(async () => {
      const plot = document.querySelector('[data-testid="pareto-plot"]');
      const line = document.querySelector('[data-testid="pareto-line"]');
      const circles = [...document.querySelectorAll('[data-testid^="pareto-pt-"]')];
      const domFlags = circles.map((c) => c.getAttribute('data-pareto') === 'true');
      const summary = document.querySelector('[data-testid="pareto-summary"]')?.textContent ?? '';

      const metrics = ['vm', 'safety', 'compliance', 'peel', 'deflection'];
      const metricFrontiers = {};
      for (const m of metrics) {
        window.fieldsmith.ui.set('paretoMetric', m);
        await new Promise((r) => setTimeout(r, 50));
        const cs = [...document.querySelectorAll('[data-testid^="pareto-pt-"]')];
        metricFrontiers[m] = cs.map((c) => c.getAttribute('data-pareto') === 'true');
      }
      window.fieldsmith.ui.set('paretoMetric', 'vm');
      await new Promise((r) => setTimeout(r, 50));
      return {
        hasPlot: !!plot,
        hasLine: !!line,
        circleCount: circles.length,
        domFlags,
        summary,
        metricFrontiers,
      };
    });

    check(
      'Pareto SVG scatter plot renders 4 candidate circles and frontier polyline',
      plotCheck.hasPlot && plotCheck.hasLine && plotCheck.circleCount === 4,
      `circles=${plotCheck.circleCount}, polyline=${plotCheck.hasLine}`,
    );
    check(
      'Pareto dominance marks (8,8), (8,12), (12,12) on frontier and (12,8) as dominated',
      JSON.stringify(plotCheck.domFlags) === JSON.stringify([true, true, false, true]),
      `data-pareto=${JSON.stringify(plotCheck.domFlags)}`,
    );
    check(
      'Pareto frontier holds across vm, safety, compliance, peel, and deflection metrics',
      Object.values(plotCheck.metricFrontiers).every(
        (flags) => JSON.stringify(flags) === JSON.stringify([true, true, false, true]),
      ),
      JSON.stringify(plotCheck.metricFrontiers),
    );

    await page.screenshot({ path: resolve(screens, 'pareto-01-frontier.png') });

    // Click Pareto point 3 (w=12, h=12) in the SVG plot.
    await page.locator('[data-testid="pareto-pt-3"]').click({ force: true });
    await until(
      page,
      () => {
        const doc = window.fieldsmith.store.doc;
        return (
          doc.lets.find((l) => l.name === 'w')?.value === 12 &&
          doc.lets.find((l) => l.name === 'h')?.value === 12 &&
          window.fieldsmith.analysis.state.status === 'ready' &&
          window.fieldsmith.analysis.state.resultsFor === window.fieldsmith.store.docVersion
        );
      },
      null,
      120000,
      'apply point 3 and re-solve',
    );
    await page.waitForTimeout(350);

    const appliedSnap = await page.evaluate(() => {
      const store = window.fieldsmith.store;
      const fsm = window.__wasm.printFsm(store.doc);
      const beam = store.doc.root.find((n) => n.id === 'beam');
      const vp = window.fieldsmith.viewport;
      vp.render();
      const gl = vp.canvas.getContext('webgl2') || vp.canvas.getContext('webgl');
      const w = vp.canvas.width;
      const h = vp.canvas.height;
      const buf = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
      const init = window.__capInitial;
      let diffPx = 0;
      for (let i = 0; i < buf.length; i += 4) {
        if (
          Math.abs(buf[i] - init[i]) +
            Math.abs(buf[i + 1] - init[i + 1]) +
            Math.abs(buf[i + 2] - init[i + 2]) >
          15
        ) {
          diffPx++;
        }
      }
      const pt3Sel = document.querySelector('[data-testid="pareto-pt-3"]')?.getAttribute('data-selected');
      return {
        fsm,
        size: beam?.kind?.size ?? null,
        at: beam?.transform?.translate ?? null,
        canUndo: store.canUndo(),
        undoLabel: store.undoLabel(),
        diffPx,
        pt3Sel,
      };
    });

    check(
      'clicking Pareto point 3 updates `.fsm` (`let w = 12`, `let h = 12`) and derived geometry (`size=(60, 12, 12)`, `at=(30, 6, 6)`)',
      appliedSnap.fsm.includes('let w = 12') &&
        appliedSnap.fsm.includes('let h = 12') &&
        JSON.stringify(appliedSnap.size) === JSON.stringify([60, 12, 12]) &&
        JSON.stringify(appliedSnap.at) === JSON.stringify([30, 6, 6]),
      `size=${JSON.stringify(appliedSnap.size)}, at=${JSON.stringify(appliedSnap.at)}, undo="${appliedSnap.undoLabel}"`,
    );
    check(
      'clicking Pareto point 3 updates the 3D viewport framebuffer and marks point 3 selected',
      appliedSnap.diffPx > 100 && appliedSnap.pt3Sel === 'true',
      `${appliedSnap.diffPx} pixels changed vs initial, data-selected=${appliedSnap.pt3Sel}`,
    );

    await page.screenshot({ path: resolve(screens, 'pareto-02-applied.png') });

    // Undo via Ctrl+Z and verify bit-identical restoration of `.fsm` and 3D viewport.
    await page.keyboard.press('Control+z');
    await until(
      page,
      () => {
        const doc = window.fieldsmith.store.doc;
        return (
          doc.lets.find((l) => l.name === 'w')?.value === 10 &&
          doc.lets.find((l) => l.name === 'h')?.value === 10 &&
          window.fieldsmith.analysis.state.status === 'ready' &&
          window.fieldsmith.analysis.state.resultsFor === window.fieldsmith.store.docVersion
        );
      },
      null,
      120000,
      'Ctrl+Z undo and re-solve',
    );
    await page.waitForTimeout(350);

    const undoneSnap = await page.evaluate(() => {
      const store = window.fieldsmith.store;
      const fsm = window.__wasm.printFsm(store.doc);
      const beam = store.doc.root.find((n) => n.id === 'beam');
      const vp = window.fieldsmith.viewport;
      vp.render();
      const gl = vp.canvas.getContext('webgl2') || vp.canvas.getContext('webgl');
      const w = vp.canvas.width;
      const h = vp.canvas.height;
      const buf = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
      const init = window.__capInitial;
      let diffPx = 0;
      for (let i = 0; i < buf.length; i += 4) {
        if (
          Math.abs(buf[i] - init[i]) +
            Math.abs(buf[i + 1] - init[i + 1]) +
            Math.abs(buf[i + 2] - init[i + 2]) >
          15
        ) {
          diffPx++;
        }
      }
      const plotStillPresent = !!document.querySelector('[data-testid="pareto-plot"]');
      return {
        fsm,
        size: beam?.kind?.size ?? null,
        at: beam?.transform?.translate ?? null,
        diffPx,
        plotStillPresent,
      };
    });

    check(
      '`Ctrl+Z` restores `.fsm` source and derived geometry bit-identically to `(w=10, h=10)`',
      undoneSnap.fsm === initialSnap.fsm &&
        JSON.stringify(undoneSnap.size) === JSON.stringify([60, 10, 10]) &&
        JSON.stringify(undoneSnap.at) === JSON.stringify([30, 5, 5]),
      `fsm equal=${undoneSnap.fsm === initialSnap.fsm}, size=${JSON.stringify(undoneSnap.size)}`,
    );
    check(
      '`Ctrl+Z` restores the 3D viewport framebuffer and preserves the Pareto plot',
      undoneSnap.diffPx === 0 && undoneSnap.plotStillPresent,
      `${undoneSnap.diffPx} pixels different from initial, plot present=${undoneSnap.plotStillPresent}`,
    );
    check('no console or page errors', errors.length === 0, errors.join(' | ').slice(0, 300));

    const ok = checks.every((c) => c.pass);
    const lines = [
      '# V47 — Interactive Pareto frontier explorer in `Results` (M25)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      '| Check | Result | Detail |',
      '| :-- | :-- | :-- |',
      ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail} |`),
      '',
      '## 2D `let` sweep candidates (`w ∈ {8, 12}` × `h ∈ {8, 12}`)',
      '',
      '| # | `(w, h)` (mm) | Mass (g) | Peak von Mises (MPa) | Max deflection (mm) | Compliance (N·mm) | Layer peel (×) | Pareto front |',
      '| --: | :-- | --: | --: | --: | --: | --: | :-- |',
      ...swState.points.map(
        (p, i) =>
          `| ${i} | \`(${p.values.w}, ${p.values.h})\` | ${p.mass_g.toFixed(2)} | ${p.vm_mpa.toFixed(2)} | ${p.defl_mm.toFixed(4)} | ${p.compl_nmm.toFixed(2)} | ${p.layer_peel.toFixed(3)} | ${plotCheck.domFlags[i] ? '**yes**' : 'no (dominated by #1)'} |`,
      ),
      '',
      '## Method',
      '',
      `- Parameterised bracket/beam (\`let w = 10\`, \`let h = 10\`, \`let hw = w / 2\`, \`let hh = h / 2\`) swept across a 2×2 \`let\` grid in ${sweepMs} ms.`,
      '- Under combined tip load `force=(0, -30, -100)`, candidates `#1 (8, 12)` and `#2 (12, 8)` have identical mass (`7.32 g`), while `#1` aligns its larger depth with the dominant `-100 N` Z load and strictly dominates `#2` across stress, deflection, compliance, safety factor, and layer-peel utilization.',
      '- Clicking frontier candidate `#3 (12, 12)` applies both `let` bindings in a single undoable transaction (`store.transact`), updates the `.fsm` text and 3D viewport, and restores bit-for-bit on `Ctrl+Z`.',
      '',
      '---',
      '*Generated by `npm run test:e2e -- --only pareto` (web/tests/pareto.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V47.md'), lines.join('\n'));
    return ok;
  } finally {
    await page.close();
  }
}
