// V41 — the new-user path (VISION §7 item 1): one headless pass that does what the
// README tells a first reader to do, and reports what they would hit.
//
//   node tests/newuser.mjs <url> [--report PATH] [--source NAME]
//
// The url is a dev server the caller started with the README's own command
// (`npm --prefix web -- run dev`); scripts/v41-newuser.sh does exactly that inside a
// fresh clone and points this pass at it. The pass drives the real UI — the empty-state
// example button, the lens tabs, the Make export button — and records boot, compile,
// ladder and export timings next to the checks, because for a first user the wait is
// part of what they hit. Console errors, page errors and failed requests are all
// reported. The report is written only on PASS, the e2e family convention.
//
// Unlike the run-e2e.mjs families this file is standalone (no vite import, no browser
// launch): it connects to the server the README's command started, so the gate exercises
// the documented path rather than the harness's own server.
import { writeFileSync } from 'node:fs';
import { chromium } from 'playwright';

const args = process.argv.slice(2);
const url = args[0];
if (!url || !/^https?:\/\//.test(url)) {
  console.error('usage: node tests/newuser.mjs <url> [--report PATH] [--source NAME]');
  process.exit(2);
}
const reportPath = args.includes('--report') ? args[args.indexOf('--report') + 1] : null;
// Which source the caller cloned from (the gate passes the remote URL, or the override);
// recorded in the report so the evidence names what was actually built.
const source = args.includes('--source') ? args[args.indexOf('--source') + 1] : 'the remote (GlassOnTin/fieldsmith)';

const errors = [];
const failedRequests = [];
const checks = [];
const timings = [];
const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
const time = (name, ms) => timings.push({ name, ms });
// Registered up front so the finaliser can flip it and an abort has something to annotate.
check('e2e completed', false, '');

// Coarse progress marker so an abort names the phase it died in.
let step = 'launch';
const at = (s) => {
  step = s;
};

async function main() {
  at('launch');
  // The run-e2e harness's idiom: the bundled browser first, the system Chrome if the
  // Playwright browser cache does not carry this version's headless shell.
  const launchArgs = { headless: true, args: ['--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] };
  let browser;
  try {
    browser = await chromium.launch(launchArgs);
  } catch {
    browser = await chromium.launch({ ...launchArgs, channel: 'chrome' });
  }
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('requestfailed', (req) => {
    const u = req.url();
    if (!/favicon/.test(u)) failedRequests.push(`${req.method()} ${u} — ${req.failure()?.errorText ?? 'failed'}`);
  });

  /** Frame-quiet settle, then one render and an RGBA readback stashed in-page. */
  const settle = async () => {
    await page.waitForFunction(
      () =>
        !window.fieldsmith.viewport.interacting &&
        window.fieldsmith.viewport.renderer.getPixelRatio() === window.devicePixelRatio,
      null,
      { timeout: 10000 },
    );
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  };
  const stash = (name) =>
    page.evaluate((nm) => {
      const vp = window.fieldsmith.viewport;
      vp.render();
      const c = vp.renderer.domElement;
      const gl = c.getContext('webgl2');
      const W = gl.drawingBufferWidth;
      const H = gl.drawingBufferHeight;
      const px = new Uint8Array(W * H * 4);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.readPixels(0, 0, W, H, gl.RGBA, gl.UNSIGNED_BYTE, px);
      window[nm] = { W, H, px };
      return { W, H };
    }, name);
  /** Opaque part pixels (α ≥ 200) inside the projected bounds — the section family's bar. */
  const frameStats = (cap) =>
    page.evaluate(
      (capName) => {
        const A = window[capName];
        const vp = window.fieldsmith.viewport;
        const bnd = vp.bounds;
        const c = vp.renderer.domElement.getBoundingClientRect();
        const dpr = window.devicePixelRatio;
        const corners = [];
        for (const xi of [bnd[0][0], bnd[1][0]])
          for (const yi of [bnd[0][1], bnd[1][1]])
            for (const zi of [bnd[0][2], bnd[1][2]]) corners.push(vp.project([xi, yi, zi]));
        const x0 = Math.max(0, Math.round((Math.min(...corners.map((p) => p.x)) - c.left) * dpr));
        const x1 = Math.min(A.W - 1, Math.round((Math.max(...corners.map((p) => p.x)) - c.left) * dpr));
        const yTop = Math.max(0, Math.round((Math.min(...corners.map((p) => p.y)) - c.top) * dpr));
        const yBot = Math.min(A.H - 1, Math.round((Math.max(...corners.map((p) => p.y)) - c.top) * dpr));
        let opaque = 0;
        for (let row = A.H - 1 - yBot; row <= A.H - 1 - yTop; row++) {
          for (let col = x0; col <= x1; col++) {
            const i = (row * A.W + col) * 4;
            if (A.px[i + 3] >= 200) opaque++;
          }
        }
        return { opaque, inBox: (x1 - x0 + 1) * (yBot - yTop + 1) };
      },
      cap,
    );
  const setLens = async (lens) => {
    await page.locator(`[data-lens="${lens}"]`).click();
    await page.waitForFunction((l) => window.fieldsmith.store.lens === l, lens, { timeout: 5000 });
  };
  /** One lens visit: switch, settle, assert the frame is not blank. */
  const lensVisit = async (lens, minOpaque) => {
    await setLens(lens);
    await page.waitForTimeout(350);
    await settle();
    const cap = `__lens_${lens}`;
    await stash(cap);
    const s = await frameStats(cap);
    check(`lens ${lens}: the frame renders content (not a blank viewport)`, s.opaque >= minOpaque, `${s.opaque} opaque px in ${s.inBox}`);
    return s;
  };

  try {
    at('goto');
    const t0 = Date.now();
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForSelector('[data-testid="empty-state"]', { timeout: 60000 });
    time('boot: open the URL to the empty state', Date.now() - t0);

    // The README promises the dev server's COOP/COEP headers give the threaded build.
    at('isolation');
    const iso = await page.evaluate(async () => ({
      coi: typeof crossOriginIsolated !== 'undefined' && crossOriginIsolated,
      hc: navigator.hardwareConcurrency,
    }));
    check(
      'the dev server serves the cross-origin isolation headers (the README’s threaded-build promise)',
      iso.coi,
      `crossOriginIsolated ${iso.coi}, hardwareConcurrency ${iso.hc}`,
    );

    // The wasm package loads (the app itself cannot render without it; the empty state
    // proves it, and the module reports which build was chosen).
    at('wasm');
    const wasm = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      return { threaded: w.threaded };
    });
    check('the wasm package loads', true, wasm.threaded ? 'threaded build (pkg-mt)' : 'single-thread build (pkg)');

    // The first user's first action: the example button on the empty state.
    at('example');
    const t1 = Date.now();
    await page.locator('[data-testid="example-bracket"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 60000 });
    time('example: click the example button to the compiled model', Date.now() - t1);
    const compileMs = await page.evaluate(() => window.fieldsmith.ui.compileMs);
    check('the example opens through the real UI button', compileMs > 0, `compileMs ${compileMs}`);
    check('no render error banner is showing', await page.locator('[data-testid="render-error"]').count() === 0, '');

    // Every lens, in README order of discovery: shape, setup, results, fill, make.
    at('lens shape');
    await lensVisit('shape', 2000);

    at('lens setup');
    await lensVisit('setup', 2000);

    at('lens results');
    const t2 = Date.now();
    await setLens('results');
    await page.waitForFunction(
      () => {
        const s = window.fieldsmith.analysis.state;
        return (s.status === 'ready' || s.status === 'error') && s.resultsFor === window.fieldsmith.store.docVersion;
      },
      null,
      { timeout: 240000 },
    );
    const analysisState = await page.evaluate(() => ({
      status: window.fieldsmith.analysis.state.status,
      error: window.fieldsmith.analysis.state.error,
      levels: window.fieldsmith.analysis.state.levels.length,
    }));
    time('results: the analysis ladder settles', Date.now() - t2);
    check(
      'lens results: the analysis ladder reaches ready',
      analysisState.status === 'ready' && analysisState.levels >= 1,
      `status ${analysisState.status}, ${analysisState.levels} levels${analysisState.error ? `, error ${analysisState.error}` : ''}`,
    );
    await page.waitForSelector('[data-testid="results-panel"]', { timeout: 10000 });
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });
    await settle();
    await stash('__res');
    const resStats = await frameStats('__res');
    check('lens results: the analysed mesh renders non-blank', resStats.opaque >= 2000, `${resStats.opaque} opaque px in ${resStats.inBox}`);

    at('lens fill');
    await lensVisit('fill', 2000);

    at('lens make');
    await lensVisit('make', 2000);
    await page.waitForSelector('[data-testid="make-panel"]', { timeout: 10000 });
    // The first user ends by exporting: pick the coarsest spacing first so the build
    // stays a few seconds (the default is 0.25 mm).
    await page.locator('[data-testid="make-h"] button[data-value="0.4"]').click();
    const t3 = Date.now();
    await page.locator('[data-testid="make-export"]').click();
    await page.waitForFunction(
      () => {
        const ex = window.fieldsmith.analysis.state.export;
        return ex && ex !== 'running';
      },
      null,
      { timeout: 120000 },
    );
    time('make: the export mesh builds', Date.now() - t3);
    const exportState = await page.evaluate(() => ({
      error: window.fieldsmith.analysis.state.exportError,
      meta: (() => {
        const ex = window.fieldsmith.analysis.state.export;
        return ex && ex !== 'running' ? ex.meta : null;
      })(),
    }));
    check(
      'lens make: Build export produces a mesh',
      exportState.error === null && exportState.meta !== null,
      exportState.error ? String(exportState.error).slice(0, 160) : JSON.stringify(exportState.meta?.what ?? null),
    );
    await page.waitForSelector('[data-testid="make-meta"]', { timeout: 10000 });

    // What a first user would hit: anything the console or the network says.
    check('no console errors during the first-user path', errors.length === 0, errors.join(' | ').slice(0, 400));
    check('no failed requests during the first-user path', failedRequests.length === 0, failedRequests.join(' | ').slice(0, 400));
    const done = checks.find((c) => c.name === 'e2e completed');
    if (done) done.pass = true;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    if (done) done.detail = `aborted at ${step}: ${String(e).slice(0, 300)}`;
  } finally {
    await page.close();
    await browser.close();
  }

  const ok = checks.every((c) => c.pass);
  if (!ok) {
    const bad = checks.filter((c) => !c.pass);
    console.error(['newuser: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    if (errors.length) console.error(`console errors (${errors.length}):\n` + errors.map((e) => `  - ${e}`).join('\n'));
    if (failedRequests.length) console.error(`failed requests (${failedRequests.length}):\n` + failedRequests.map((e) => `  - ${e}`).join('\n'));
  } else if (reportPath) {
    const lines = [
      '# V41 — the new-user path: the README’s own commands, driven headlessly from a fresh clone',
      '',
      `**Result: PASS**  ·  ${new Date().toISOString()}`,
      '',
      `Clone source: ${source}.`,
      '',
      'One headless pass that does what the README tells a first reader to do, run',
      'against a fresh clone built by nothing but the README’s commands (the gate is',
      '`scripts/v41-newuser.sh`): clone → `npm --prefix web install` →',
      '`bash scripts/build-wasm.sh` → `npm --prefix web -- run dev` → open the page →',
      'open the example through the empty-state button → every lens → build the export.',
      'The pass drives the real UI (the example button, the lens tabs, the export',
      'button), asserts the frame is not blank on every lens (the combinations',
      'family’s bar: α ≥ 200 inside the projected bounds), and records the timings a',
      'first user would feel next to the checks.',
      '',
      '## Checks',
      '',
      '| Check | Result | Measured |',
      '| :-- | :-- | :-- |',
      ...checks.map((c) => `| ${c.name} | pass | ${c.detail}`.replace(/\n/g, ' ')),
      '',
      '## Timings (what a first user waits)',
      '',
      '| Phase | Wall |',
      '| :-- | --: |',
      ...timings.map((t) => `| ${t.name} | ${t.ms} ms |`),
      '',
      '## Limits',
      '',
      '- The browser binary comes from the machine’s Playwright cache, not the README',
      '  path (a first user brings their own browser; the gate brings its driver).',
      '- The pass drives one example (the bracket) and the coarsest export spacing;',
      '  the other examples and finer spacings are covered by the run-e2e families.',
      '- Timings are from one run on the gate machine; they are reported, not gated.',
      '',
      `Console errors: **${errors.length}** · failed requests: **${failedRequests.length}**`,
      '',
      '---',
      '*Generated by `scripts/v41-newuser.sh` (web/tests/newuser.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    writeFileSync(reportPath, lines);
    console.log(`newuser: report written to ${reportPath}`);
  }
  console.log(`newuser test ${ok ? 'PASS' : 'FAIL'}`);
  process.exit(ok ? 0 : 1);
}

main().catch((e) => {
  console.error(`newuser: run aborted at ${step}: ${String(e)}`);
  process.exit(1);
});
