// V16 — the interior pass and part colours, driven through the real UI in a headless browser.
//
// (a) `fs_core` GPU vs Rust `removed_skin`: the numerical case, on verify.html, mirroring the
//     V5 harness — 10 000 seeded points, max |Δd| ≤ 1e-4 (the tolerance bounds codegen
//     mistakes, both sides compute f32/f64 like V5).
// (b) Semi-transparent view: the filled bracket at 35 % body opacity — the pixels inside the
//     model's bounds that differ from the opaque render, plus the spread of shade across
//     them (struts bright, voids dark), from live framebuffer reads.
// (c) Part colour: a colour set through the Inspector reaches a projected face's pixel and
//     the tree row's swatch.
// (d) Opacity is session-only: it resets to 100 % on reload.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

export async function interiorTest(browser, url, { screens, docsDir }) {
  const errors = [];
  const collectErrors = (page) => {
    page.on('console', (m) => {
      if (m.type() === 'error') errors.push(m.text());
    });
    page.on('pageerror', (e) => errors.push(String(e)));
  };

  // ---- (a) numerics ------------------------------------------------------------------------
  const verifyPage = await browser.newPage();
  collectErrors(verifyPage);
  await verifyPage.goto(`${url}verify.html`);
  await verifyPage.waitForFunction(() => window.__verify, null, { timeout: 120000 });
  const num = (await verifyPage.evaluate(() => window.__verify)).V16;
  await verifyPage.close();
  const numTol = 1e-4;
  const numericsOk = !num.error && num.maxAbs <= numTol;
  console.log(
    `V16 interior numerics ${numericsOk ? 'PASS' : 'FAIL'}: max |Δd| = ${num.maxAbs ?? num.error}`,
  );

  // ---- (b)(c)(d) the app page ----------------------------------------------------------------
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  collectErrors(page);
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const filledFsm = readFileSync(resolve(here, '../../examples/bracket-filled.fsm'), 'utf8');

  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('[data-testid="empty-state"]');

  // Load the filled document the way automation loads any .fsm text: parse with the Rust loader
  // and hand the document to the store — the same path the Open… flow takes.
  await page.evaluate(async (text) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(text);
    if ('error' in r) throw new Error(`bracket-filled.fsm: ${r.error.message}`);
    window.fieldsmith.store.load(r.ok);
  }, filledFsm);
  await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 30000 });
  // The shape lens never launches the ladder (analysis `wanted()` is setup/results/fill), so
  // the captures below run on the preview fill's uniform-ρmax grid and no graded arrival can
  // swap the texture under them. Assert the shader actually carries the interior pass.
  const carries = await page.evaluate(async () => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const sh = w.bodyShader(window.fieldsmith.store.doc);
    return { core: sh.core, grid: !!sh.grid, alpha: window.fieldsmith.ui.bodyAlpha };
  });
  check('a filled shape lens binds a core shader with a parameter grid', carries.core && carries.grid && carries.alpha === 1, JSON.stringify(carries));

  /** Render the current state and stash the RGBA readback under `name`, in-page (the arrays
   * are ~7 MB — comparisons run in the page, only stats cross the wire). */
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

  /** RGBA of the pixel nearest a world point in the stashed capture `cap`. */
  const pixelAt = (cap, world) =>
    page.evaluate(
      ([capName, p]) => {
        const vp = window.fieldsmith.viewport;
        const s = vp.project(p);
        const c = vp.renderer.domElement.getBoundingClientRect();
        const dpr = window.devicePixelRatio;
        const cap = window[capName];
        const dx = Math.min(cap.W - 1, Math.max(0, Math.round((s.x - c.left) * dpr)));
        const dy = Math.min(cap.H - 1, Math.max(0, Math.round((s.y - c.top) * dpr)));
        const row = cap.H - 1 - dy; // readPixels rows run bottom-up
        const i = (row * cap.W + dx) * 4;
        return Array.from(cap.px.slice(i, i + 4));
      },
      [cap, world],
    );

  // The slider is an <input type="range">, which playwright's fill() does not drive; dispatch
  // the input event the way a real drag does (Preact's onInput listens for it).
  const setAlpha = (value) =>
    page.evaluate((v) => {
      const el = document.querySelector('[data-testid="opacity-slider"]');
      el.value = String(v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      return el.value;
    }, value);

  await stash('__capO');
  const prevOpaque = await page.evaluate(() => window.fieldsmith.ui.compileMs);
  await setAlpha(0.35);
  await page.waitForFunction(
    (prev) => window.fieldsmith.ui.compileMs !== prev,
    prevOpaque,
    { timeout: 30000 },
  );
  await stash('__capT');

  const vis = await page.evaluate(async () => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const sh = w.bodyShader(window.fieldsmith.store.doc);
    if (!sh.bounds) throw new Error('no bounds');
    const rect = sh.bounds.flatMap((v) => v); // [x0,y0,z0,x1,y1,z1]
    const A = window.__capO;
    const B = window.__capT;
    const vp = window.fieldsmith.viewport;
    const c = vp.renderer.domElement.getBoundingClientRect();
    const dpr = window.devicePixelRatio;
    const corners = [
      [rect[0], rect[1], rect[2]], [rect[3], rect[1], rect[2]], [rect[0], rect[4], rect[2]], [rect[3], rect[4], rect[2]],
      [rect[0], rect[1], rect[5]], [rect[3], rect[1], rect[5]], [rect[0], rect[4], rect[5]], [rect[3], rect[4], rect[5]],
    ].map((p) => vp.project(p));
    const x0 = Math.max(0, Math.round((Math.min(...corners.map((p) => p.x)) - c.left) * dpr));
    const x1 = Math.min(A.W - 1, Math.round((Math.max(...corners.map((p) => p.x)) - c.left) * dpr));
    const yTopD = Math.max(0, Math.round((Math.min(...corners.map((p) => p.y)) - c.top) * dpr));
    const yBotD = Math.min(A.H - 1, Math.round((Math.max(...corners.map((p) => p.y)) - c.top) * dpr));
    // GL rows run bottom-up.
    const rowA = A.H - 1 - yBotD;
    const rowB = A.H - 1 - yTopD;
    let inBox = 0;
    let changed = 0;
    let minLum = Infinity;
    let maxLum = 0;
    let sum = 0;
    let sum2 = 0;
    for (let row = rowA; row <= rowB; row++) {
      for (let col = x0; col <= x1; col++) {
        const i = (row * A.W + col) * 4;
        inBox++;
        const d = Math.max(
          Math.abs(A.px[i] - B.px[i]),
          Math.abs(A.px[i + 1] - B.px[i + 1]),
          Math.abs(A.px[i + 2] - B.px[i + 2]),
        );
        if (d > 12) {
          changed++;
          const lum = (0.2126 * B.px[i] + 0.7152 * B.px[i + 1] + 0.0722 * B.px[i + 2]) / 255;
          sum += lum;
          sum2 += lum * lum;
          if (lum < minLum) minLum = lum;
          if (lum > maxLum) maxLum = lum;
        }
      }
    }
    const n = Math.max(1, changed);
    return { inBox, changed, frac: changed / inBox, mean: sum / n, std: Math.sqrt(Math.max(0, sum2 / n - (sum / n) ** 2)), minLum, maxLum, rectPx: [x1 - x0 + 1, rowB - rowA + 1] };
  });
  const visBound = 0.08; // stated bound: ≥ 8 % of the projected-bounds pixels differ
  const visStdBound = 0.15; // stated bound: shade spread among changed pixels (0–1 luminance)
  check(
    `semi-transparent view shows the fill: ${visBound * 100} %+ of the in-bounds pixels differ from opaque`,
    !Number.isNaN(vis.frac) && vis.frac >= visBound,
    `${(vis.frac * 100).toFixed(2)} % of ${vis.inBox} in-bounds px differ by > 12/255 on some channel (${vis.changed} px, rect ${vis.rectPx.join('×')})`,
  );
  check(
    `shading spreads from struts to voids (luminance σ ≥ ${visStdBound})`,
    vis.std >= visStdBound,
    `σ ${vis.std.toFixed(3)}, mean ${(vis.mean).toFixed(3)}, range ${vis.minLum.toFixed(3)}–${vis.maxLum.toFixed(3)}`,
  );

  // ---- (c) part colour ----------------------------------------------------------------------
  // Selecting a node tints its faces (the shader's highlight blend), so the pixel checks run
  // after the selection is cleared — they read the plain leaf colour. Back at full opacity
  // first, so the pixel is the body colour itself, not a 35 % blend over dark voids.
  const alphaPrev = await page.evaluate(() => window.fieldsmith.ui.compileMs);
  await setAlpha(1);
  await page.waitForFunction((p) => window.fieldsmith.ui.compileMs !== p, alphaPrev, { timeout: 30000 });
  await stash('__cap0');

  const prevCompile = await page.evaluate(() => window.fieldsmith.ui.compileMs);
  await page.locator('.row[data-node="base"]').click();
  await page.locator('[data-testid="node-color"] [data-color="#21BB4C"]').click();
  const baseHint = await page.evaluate(() => document.querySelector('[data-testid="color-hint"]')?.textContent ?? '');
  await page.keyboard.press('Escape');
  await page.waitForFunction((prev) => window.fieldsmith.ui.compileMs !== prev, prevCompile, { timeout: 30000 });
  check('Inspector hint names the shape colour after selecting base', baseHint.includes('#21BB4C'), baseHint);
  await stash('__capG');
  { // #21BB4C: green dominant — the leaf colour reaches the framebuffer. The sample sits on the
    // base's top face right of the upright, which otherwise occludes the plate's centre.
    const [before, after] = await Promise.all([
      pixelAt('__cap0', [54, 32, 8]),
      pixelAt('__capG', [54, 32, 8]),
    ]);
    const changedHue = after[1] > Math.max(after[0], after[2]) + 15;
    check(
      'base renders #21BB4C (green-dominant pixel on its top face)',
      changedHue,
      `pixel rgb(${before.join(', ')}) → rgb(${after.join(', ')}) at (54, 32, 8), green channel must dominate`,
    );
  }
  check('tree row shows the colour dot on base', (await page.locator('.row[data-node="base"] .swatch').count()) === 1);

  const prevCompile2 = await page.evaluate(() => window.fieldsmith.ui.compileMs);
  await page.locator('.row[data-node="upright"]').click();
  await page.locator('[data-testid="node-color"] [data-color="#B5432A"]').click();
  await page.keyboard.press('Escape');
  await page.waitForFunction((prev) => window.fieldsmith.ui.compileMs !== prev, prevCompile2, { timeout: 30000 });
  await stash('__capR');
  { // #B5432A: red dominant; colourless body before this — a real change.
    const [before, after] = await Promise.all([
      pixelAt('__capG', [30, 0, 16]),
      pixelAt('__capR', [30, 0, 16]),
    ]);
    const changedHue = after[0] > Math.max(after[1], after[2]) + 15;
    check(
      'upright renders #B5432A (red-dominant pixel on its front face)',
      changedHue,
      `pixel rgb(${before.join(', ')}) → rgb(${after.join(', ')}) at (30, 0, 16), red channel must dominate`,
    );
  }
  check('tree row shows the colour dot on upright', (await page.locator('.row[data-node="upright"] .swatch').count()) === 1);
  // The wiring is checked at the uniforms: leafN covers both coloured leaves and the slot
  // values are the palette hexes through the sRGB→linear transfer (THREE.Color's setStyle).
  const leafWire = await page.evaluate(() => {
    const m = window.fieldsmith.viewport.material;
    const lin = (hex) =>
      [1, 3, 5].map((i) => {
        const v = parseInt(hex.slice(i, i + 2), 16) / 255;
        return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
    const got = m.uniforms.u_leafCol.value.slice(0, 2).map((v) => [v.x, v.y, v.z]);
    const want = [lin('#21BB4C'), lin('#B5432A')];
    return {
      n: m.uniforms.u_leafN.value,
      close: got.every((g, i) => g.every((x, a) => Math.abs(x - want[i][a]) < 0.005)),
      got,
    };
  });
  check('two leaf colours ride the shader uniforms', leafWire.n === 2 && leafWire.close, `leafN ${leafWire.n}; slots ${JSON.stringify(leafWire.got)}`);

  // The colour round-trips through the document text.
  const canon = await page.evaluate(async () => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    return w.printFsm(window.fieldsmith.store.doc);
  });
  const colourLine = canon.split('\n').find((l) => l.includes('color')).trim() ?? '';
  check('.fsm text carries the colour statements', /color "#21BB4C" on base/.test(canon) && /color "#B5432A" on upright/.test(canon), colourLine);

  // ---- (d) opacity resets on reload -----------------------------------------------------------
  await page.reload();
  await page.waitForFunction(() => window.fieldsmith?.store?.doc?.root?.length > 0, null, { timeout: 30000 });
  const afterReload = await page.evaluate(() => ({
    alpha: window.fieldsmith.ui.bodyAlpha,
    slider: document.querySelector('[data-testid="opacity-slider"]')?.value ?? null,
    shown: document.querySelector('[data-testid="opacity-value"]')?.textContent ?? null,
  }));
  check(
    'opacity resets to 100 % on reload (session-only)',
    afterReload.alpha === 1 && afterReload.slider === '1' && afterReload.shown === '100%',
    JSON.stringify(afterReload),
  );

  await page.screenshot({ path: resolve(screens, 'interior-01.png') });
  await page.close();

  // ---- report ---------------------------------------------------------------------------------
  const ok = numericsOk && checks.every((c) => c.pass) && errors.length === 0;
  const lines = [
    '# V16 — Interior (core) field and part colours',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    '## Numerics: shader `fs_core` vs Rust `removed_skin`',
    '',
    `**Result: ${numericsOk ? 'PASS' : 'FAIL'}** (max |Δd| ≤ ${numTol} over ${num.points} points)`,
    '',
    '| Quantity | Reference | Measured | Tolerance | |',
    '| :-- | --: | --: | --: | :-- |',
  ];
  if (num.error) {
    lines.push(`| max \\|d_GPU − d_Rust\\| | 0 mm | error: ${num.error} | ${numTol} mm | **fail** |`);
  } else {
    lines.push(`| max \\|d_GPU − d_Rust\\| over ${num.points} points | 0 mm | ${num.maxAbs.toExponential(3)} mm | ${numTol} mm | ${numericsOk ? 'pass' : '**fail**'} |`);
    lines.push('', '## Detail', '');
    lines.push(`- Renderer: ${num.renderer}`);
    lines.push(`- Parameter grid: ${num.grid.dims.join(' × ')} nodes, spacing ${num.grid.spacing} mm (the preview fill's uniform-ρmax grid).`);
    lines.push(`- Sampling: ${num.spacingNote}, seeded (mulberry32, seed 16).`);
    lines.push(`- RMS difference: ${num.rms.toExponential(3)} mm; max within 2 mm of a surface: ${num.maxAbsNear.toExponential(3)} mm.`);
    lines.push(`- Largest gradient direction difference: ${num.maxGradAngleDeg.toExponential(3)}°.`);
    lines.push(`- Worst point (${num.worst.p.map((v) => v.toFixed(3)).join(', ')}) mm: GPU ${num.worst.gpu} mm, Rust ${num.worst.rust} mm.`);
  }
  lines.push(
    '',
    'The GPU computes in 32-bit floats and the Rust evaluator in 64-bit, so the tolerance bounds codegen mistakes, not float precision — matching V5.',
    '',
    '## Semi-transparent view and part colour (app checks)',
    '',
    '| Check | Result | Measured |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail} |`),
    '',
    'Method: the filled bracket is loaded through the real store API; captures are full-canvas RGBA readbacks of live frames',
    '(`vp.render()` then `gl.readPixels`, `preserveDrawingBuffer` retains content). The changed-fraction bounds the pixels inside the',
    'projected bounds that the semi-transparent render changes relative to the opaque one; the luminance spread says the visible',
    'interior covers both lattice struts and voids, not a flat wash.',
    '',
    `Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
    '',
    `Screenshot: web/test-results/interior-01.png.`,
    '',
    'Operator checks (not CI), run 2026-10-13 on the referee desktop: the CLI-exported coloured 3MF (two add-only boxes,',
    'labels pla/petg, colours #21BB4C/#B5432A) carries both <base displaycolor="#…"/> entries, verified in the XML; opened in',
    'desktop PrusaSlicer 2.9.6 (flatpak), the object loads and separates into two named volumes whose 3D view renders. Measured',
    'with a screen capture: in that single-material profile PrusaSlicer paints every volume the active filament colour and',
    'ignores the 3MF displaycolor, so the per-part colours are not seen in-slicer there — the attribute is present and',
    'spec-conformant; 3.0 alpha in Downloads is a Windows build, untested on Linux.',
    '',
    '---',
    '*Generated by `npm run test:e2e` (web/tests/interior.mjs) — do not edit by hand.*',
    '',
  );
  writeFileSync(resolve(docsDir, 'V16.md'), lines.join('\n'));
  return ok;
}