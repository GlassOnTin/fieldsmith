// V40 — loud rendering failures (VISION §7 item 1): a failed shader compile, a document
// whose field has no surface inside its own bounds, and a results mesh whose build throws
// must each surface a persistent error state the lens can show — never a silently blank
// viewport. Every failure is checked three ways: the persistent `ui.renderError` carries
// the right kind and real text, the banner is in the DOM (role=alert, data-kind), and the
// framebuffer says what it should (the shader case keeps the previous shader — non-blank;
// the surface case is blank inside the projected bounds, which is exactly why the error
// must exist; the results case keeps the field — non-blank). Recovery is asserted too:
// the next good state clears the error and the frame renders content again.
//
// The three failures are injected deliberately:
//  (shader)  a direct viewport.setField with a fragment source calling a function that
//            does not exist — the driver's compile error reaches the onShaderError hook;
//  (surface) a subtract whose cutter misses the kept box entirely: bounds = the kept box
//            (non-empty, so the invisible path does not apply), the field is empty inside
//            it — the CPU probe (1-Lipschitz grid test) is what catches this;
//  (results) a throwing getter on the solved level's surfaceOwner, so rebuild() throws
//            deterministically; the property is restored afterwards to prove recovery.
//
// Run with node tests/run-e2e.mjs --only renderfail.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

// Bounds non-empty (the kept box), field empty everywhere: the cutter contains the kept
// box entirely, so the subtract removes the whole body. (A cutter that merely misses the
// kept box leaves it solid — a surface exists and the probe correctly stays quiet; an
// intersect of disjoint bodies gives an empty AABB and takes the invisible path instead.)
const NO_SURFACE_DOC = `\
fieldsmith 1
name "Vanishing cut"
material petg
box kept size=(10, 10, 10) at=(0, 0, 5)
box cutter size=(20, 20, 20) at=(0, 0, 5) subtract
`;

// A fragment source that cannot compile: fs_field calls a function that does not exist.
// (three prepends #version 300 es itself for GLSL3, so the source stays bare.)
const BAD_FIELD = `vec4 fs_field(vec3 p) {
  return vec4(nosuchfunction(p), 0.0, 0.0, 1.0);
}`;

/** Settle loop (the gradedresults idiom): ready on the current docVersion, throws on the
 * worker's error post. */
async function awaitLadder(page, tag) {
  const t0 = Date.now();
  for (;;) {
    const s = await page.evaluate(() => ({
      status: window.fieldsmith.analysis.state.status,
      error: window.fieldsmith.analysis.state.error,
      levels: window.fieldsmith.analysis.state.levels.length,
      resultsFor: window.fieldsmith.analysis.state.resultsFor,
      dv: window.fieldsmith.store.docVersion,
    }));
    if (s.status === 'ready' && s.resultsFor === s.dv && s.levels >= 1) return;
    if (s.status === 'error' && s.resultsFor === s.dv) throw new Error(`${tag}: analysis error: ${s.error}`);
    if (Date.now() - t0 > 240000) throw new Error(`${tag}: ladder did not settle (last status ${JSON.stringify(s)})`);
    await page.waitForTimeout(250);
  }
}

export async function renderfailTest(browser, url, { screens, docsDir }) {
  const errors = [];
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const table = (names) =>
    [
      '| Check | Result | Measured |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  // Registered up front so the finaliser can flip it, and so a catch has something to
  // annotate whatever the failure point.
  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  // ---- framebuffer helpers (the section family's idiom) ----------------------------------
  /** Render and stash the RGBA readback under `name`, in-page. */
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

  /** Counts inside the projected bounds: opaque part pixels (α ≥ 200). */
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

  /** Compare two stashed captures over the projected bounds (Δ > 12/255 counts as changed). */
  const diffCaps = (a, b) =>
    page.evaluate(
      ([an, bn]) => {
        const A = window[an];
        const B = window[bn];
        if (A.W !== B.W || A.H !== B.H) return { size: [A.W, A.H, B.W, B.H] };
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
        let inBox = 0;
        let inChanged = 0;
        for (let row = A.H - 1 - yBot; row <= A.H - 1 - yTop; row++) {
          for (let col = x0; col <= x1; col++) {
            const i = (row * A.W + col) * 4;
            const d = Math.max(
              Math.abs(A.px[i] - B.px[i]),
              Math.abs(A.px[i + 1] - B.px[i + 1]),
              Math.abs(A.px[i + 2] - B.px[i + 2]),
            );
            inBox++;
            if (d > 12) inChanged++;
          }
        }
        return { inBox, inChanged };
      },
      [a, b],
    );

  /** Frame-quiet settle (the combinations idiom): interaction idle, full pixel ratio, then
   * two animation frames so pending rAF sync passes have run. */
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

  const readError = () => page.evaluate(() => window.fieldsmith.ui.renderError);
  const banner = () =>
    page.evaluate(() => {
      const el = document.querySelector('[data-testid="render-error"]');
      return el ? { kind: el.getAttribute('data-kind'), text: el.textContent } : null;
    });

  const setLens = async (lens) => {
    await page.locator(`[data-lens="${lens}"]`).click();
    await page.waitForFunction((l) => window.fieldsmith.store.lens === l, lens, { timeout: 5000 });
  };

  const loadFsm = async (text, tag) => {
    await page.evaluate(async (t) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(t);
      if ('error' in r) throw new Error(`${t.slice(0, 24)}: ${r.error.message}`);
      window.fieldsmith.store.load(r.ok);
    }, text);
    await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 60000 });
    await setLens('shape');
    void tag;
  };

  /** Wait for the persistent error state to name a kind (or clear with null). */
  const waitErrorKind = (kind) =>
    page.waitForFunction(
      (k) => (k === null ? window.fieldsmith.ui.renderError === null : window.fieldsmith.ui.renderError?.kind === k),
      kind,
      { timeout: 30000 },
    );

  // Coarse progress marker so an abort names the phase it died in.
  let step = 'boot';
  const at = (s) => {
    step = s;
  };

  try {
    at('goto');
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.waitForSelector('[data-testid="empty-state"]');

    // ===================== baseline: the good bracket renders, no error =====================
    at('baseline load');
    const solidFsm = readFileSync(resolve(here, '../../examples/bracket.fsm'), 'utf8');
    await loadFsm(solidFsm, 'solid');
    await settle();
    await stash('__good');
    const good = await frameStats('__good');
    check('baseline: the good document renders content', good.opaque >= 2000, `${good.opaque} opaque px in ${good.inBox}`);
    check('baseline: no render error is set', (await readError()) === null, JSON.stringify(await readError()));

    // ===================== (a) shader compile failure =======================================
    at('shader inject');
    await page.evaluate((src) => void window.fieldsmith.viewport.setField({ source: src, bounds: [[-10, -10, -10], [10, 10, 10]], ident: '' }), BAD_FIELD);
    await waitErrorKind('shader');
    const shaderErr = await readError();
    check(
      'shader failure: ui.renderError.kind is "shader"',
      shaderErr?.kind === 'shader',
      JSON.stringify(shaderErr?.kind),
    );
    check(
      'shader failure: the error carries the driver log',
      typeof shaderErr?.text === 'string' && shaderErr.text.length > 0 && /ERROR|error|shader/i.test(shaderErr.text),
      String(shaderErr?.text).slice(0, 160),
    );
    const shaderBanner = await banner();
    check(
      'shader failure: the banner is in the DOM (role=alert, data-kind=shader)',
      shaderBanner?.kind === 'shader',
      JSON.stringify(shaderBanner?.kind),
    );
    await settle();
    await stash('__shader');
    const shaderFrame = await frameStats('__shader');
    check(
      'shader failure: the frame is not blank (the previous shader is retained)',
      shaderFrame.opaque >= 2000,
      `${shaderFrame.opaque} opaque px in ${shaderFrame.inBox}`,
    );

    // ===================== recovery from the shader failure =================================
    at('shader recover');
    await loadFsm(solidFsm, 'solid-recover');
    await waitErrorKind(null);
    await settle();
    await stash('__recover');
    const recoverFrame = await frameStats('__recover');
    check('recovery: loading a good document clears the shader error', (await readError()) === null, JSON.stringify(await readError()));
    check('recovery: the frame renders content again', recoverFrame.opaque >= 2000, `${recoverFrame.opaque} opaque px`);

    // ===================== (b) no surface inside non-empty bounds ============================
    at('surface inject');
    await loadFsm(NO_SURFACE_DOC, 'vanish');
    await waitErrorKind('surface');
    const surfErr = await readError();
    check('surface failure: ui.renderError.kind is "surface"', surfErr?.kind === 'surface', JSON.stringify(surfErr?.kind));
    const surfBanner = await banner();
    check('surface failure: the banner is in the DOM (data-kind=surface)', surfBanner?.kind === 'surface', JSON.stringify(surfBanner?.kind));
    await settle();
    await stash('__surface');
    const surfFrame = await frameStats('__surface');
    // Blank bar: ≤ 8 px. The origin axis triad sits at (-2, -2, 0) — three overlapping
    // 0.7-α lines accumulate past α ≥ 200 where they cross, and the screen-space
    // projected-bounds rectangle can include that corner. No part pixels: the march
    // converges on nothing, which is the silent case the error exists for.
    check(
      'surface failure: the viewport shows no part inside the bounds (the error is what remains)',
      surfFrame.opaque <= 8,
      `${surfFrame.opaque} opaque px in ${surfFrame.inBox} (axis-triad crossings allowed)`,
    );

    // ===================== recovery from the surface failure ================================
    at('surface recover');
    await loadFsm(solidFsm, 'solid-recover2');
    await waitErrorKind(null);
    await settle();
    await stash('__recover2');
    const recover2 = await frameStats('__recover2');
    check('recovery: a document with a surface clears the surface error', (await readError()) === null, JSON.stringify(await readError()));
    check('recovery: the frame renders content again (second load)', recover2.opaque >= 2000, `${recover2.opaque} opaque px`);

    // ===================== (c) results mesh build throws ====================================
    at('results solve');
    await setLens('results');
    await awaitLadder(page, 'results');
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });
    await settle();
    await stash('__res0');
    const res0 = await frameStats('__res0');
    check('results baseline: the solved lens renders content', res0.opaque >= 2000, `${res0.opaque} opaque px`);
    check('results baseline: no render error is set', (await readError()) === null, JSON.stringify(await readError()));

    at('results inject');
    // A throwing getter on the level's surfaceOwner: rebuild() reads it per facet, so the
    // build throws deterministically with a recognisable message. configurable: true so the
    // recovery step can restore the original array.
    await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.state.levels.at(-1);
      window.__origOwner = lvl.surfaceOwner;
      Object.defineProperty(lvl, 'surfaceOwner', {
        configurable: true,
        get() {
          throw new Error('injected build failure');
        },
      });
    });
    await page.evaluate(() => window.fieldsmith.ui.set('resultsScale', 7));
    await waitErrorKind('results');
    const resErr = await readError();
    check('results failure: ui.renderError.kind is "results"', resErr?.kind === 'results', JSON.stringify(resErr?.kind));
    check(
      'results failure: the error names the build failure',
      /injected build failure/.test(String(resErr?.text)),
      String(resErr?.text).slice(0, 160),
    );
    const resBanner = await banner();
    check('results failure: the banner is in the DOM (data-kind=results)', resBanner?.kind === 'results', JSON.stringify(resBanner?.kind));
    await settle();
    await stash('__res1');
    const res1 = await frameStats('__res1');
    // The failed build emptied the result group, so the analysed mesh is gone from the
    // frame: the capture must differ from the healthy one and carry far fewer opaque
    // pixels. (The lens hides the field quad — showField false — so whatever remains is
    // floor and axis lines, not part content.) The banner is what carries the failure.
    const resDiff = await diffCaps('__res0', '__res1');
    check(
      'results failure: the analysed mesh disappears from the frame',
      resDiff.inChanged > 0 && res1.opaque <= res0.opaque - 1000,
      `${res0.opaque} → ${res1.opaque} opaque px, ${resDiff.inChanged}/${resDiff.inBox} px changed`,
    );
    const buildLogged = errors.filter((e) => e.includes('results mesh build failed'));
    check(
      'results failure: the throw is also logged to the console exactly once',
      buildLogged.length === 1,
      `${buildLogged.length} matching console errors`,
    );

    // ===================== recovery from the results failure ================================
    at('results recover');
    await page.evaluate(() => {
      const lvl = window.fieldsmith.analysis.state.levels.at(-1);
      Object.defineProperty(lvl, 'surfaceOwner', { value: window.__origOwner, configurable: true, writable: true });
    });
    await page.evaluate(() => window.fieldsmith.ui.set('resultsScale', 5));
    await waitErrorKind(null);
    await settle();
    await stash('__res2');
    const res2 = await frameStats('__res2');
    check('recovery: a successful rebuild clears the results error', (await readError()) === null, JSON.stringify(await readError()));
    check('recovery: the results mesh renders again', res2.opaque >= 2000, `${res2.opaque} opaque px`);

    // ---- report ---------------------------------------------------------------------------
    // The one console error the family itself provokes (the deliberate results-build throw)
    // is accounted for by its own check above; everything else must stay silent.
    const unexpected = errors.filter((e) => !e.includes('results mesh build failed'));
    check('no unexpected console errors', unexpected.length === 0, unexpected.join(' | ').slice(0, 400));
    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);

    const names = checks.map((c) => c.name);
    const lines = [
      '# V40 — loud rendering failures: shader, no-surface and results-build errors surface, never blank silently',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
      '',
      'Three rendering failures are injected deliberately and each must produce a',
      'persistent error state (`ui.renderError`), a visible banner in the DOM',
      '(`[data-testid="render-error"]`, role=alert, data-kind), and the framebuffer',
      'behaviour the design promises:',
      '',
      '- **Shader**: a direct `viewport.setField` with a fragment source calling a',
      '  nonexistent function. `compileAsync` resolves even when the program is dead',
      '  (verified on this headless stack), so the check is the program wrapper’s',
      '  first-use link diagnostics plus the `debug.onShaderError` hook; the driver’s',
      '  compile log lands in the error text. The previous shader stays on screen — the',
      '  frame remains non-blank while the banner shows.',
      '- **No surface**: a subtract whose cutter contains the kept box entirely. The',
      '  bounds are the kept box (non-empty, so the invisible path does not apply) and',
      '  the field is empty inside it; the CPU probe (a 1-Lipschitz grid test: every node',
      '  reading d > h·√3/2 proves no surface exists) sets the error. No part pixels',
      '  remain inside the projected bounds — exactly the silent case the error exists',
      '  for (the blank bar allows ≤ 8 px for the origin axis-triad line crossings).',
      '- **Results build**: a throwing getter on the solved level’s `surfaceOwner`',
      '  makes `rebuild()` throw deterministically; the catch reports it persistently and',
      '  logs it once to the console. The lens hides the field quad, so the frame is blank',
      '  inside the bounds — the banner is what remains.',
      '',
      'Recovery is asserted after each failure: the next good state (a document load, or',
      'the property restored and the mode signature changed) clears the error and the',
      'frame renders content again. Captures are full-canvas RGBA readbacks of live',
      'frames (`vp.render()` then `gl.readPixels`), the section family’s idiom; opacity',
      'counting is α ≥ 200 inside the projected bounds.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The shader failure is injected through `viewport.setField` directly (a document',
      '  cannot carry a broken GLSL source); the production path that reaches the same',
      '  detection is the document’s generated source.',
      '- The no-surface probe is advisory: a wasm-side failure in the probe is swallowed',
      '  (it must not break the view), so the probe itself is not failure-tested here.',
      '- The results failure is injected on the level object in-page; a real build throw',
      '  from worker data would land in the same catch.',
      '',
      `Console errors: **${errors.length}** (one expected: the deliberate results-build throw)${unexpected.length ? '\n' + unexpected.map((e) => `  - ${e}`).join('\n') : ''}`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/renderfail.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['renderfail: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'V40.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = `at ${step}: ${String(e).split('\n')[0]}`;
    console.error(`renderfail: run aborted at ${step}: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}
