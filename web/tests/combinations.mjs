// V39 — feature-combination matrix (VISION §7 item 1): every overlay control against every
// document kind and lens, driven through the real UI, asserted on framebuffer pixels — a
// state variable can change correctly while the render stays dark, so every state change is
// followed by a non-blank frame assertion and every control by a visible pixel delta.
//
// Document kinds: the shipped analytic solid (examples/bracket.fsm), the filled variant
// (examples/bracket-filled.fsm), a mesh leaf imported through the palette (the box.glb
// fixture with patch-clicked support and load), and a combined modes+thermal cantilever
// (`solve modes=3 size=8 tref=20`) that carries the mode picker, the case picker and a real
// temperature field in one document.
//
// Controls: section clip (toggle/slider/axis/flip), body opacity (absent by design in the
// results lens — asserted as control absence), the five result modes, the case picker, the
// mode-shape animation (picker/phase/play), the click probe, the sweep, and the profile
// sketch feeding the viewport. store.load replaces the document but not the ui display
// knobs, so the matrix resets them explicitly between kinds.
//
// Run with node tests/run-e2e.mjs --only combinations.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

// The shipped bracket plus a third thermal case and three modes: one document that
// exercises the mode picker, the case picker and the temperature ramp together. The
// coarse rung (size=8) carries the evidence; the browser's coarse element size stays far
// below 8 mm, so the refined rung is skipped (the modes family established this).
const COMBINED_DOC = `\
fieldsmith 1
name "Combined modes + thermal bar"
custommaterial steel200 name="Steel (E 200 GPa)" e=200000 nu=0.3 yield=250 density=7.85 a=1.2e-5
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
region hot = beam.nx
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
temperature hot hot temp=80 case="Thermal 1"
solve modes=3 size=8 tref=20
`;

// The sketch fixture (the profilesketch family's DOC_A): a straight pad whose edge click
// inserts a point — here only as a field-edit that must repaint the viewport.
const SKETCH_DOC = `\
fieldsmith 1
name "Sketch pad"
material petg
extrude pad h=4 profile=[(0, 0), (20, 0), (20, 10), (0, 10)]
`;

/** Settle loop (gradedresults.mjs's awaitLadder): resolves when the ladder is ready on the
 * current docVersion; throws on the worker's error post. */
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

export async function combinationsTest(browser, url, { screens, docsDir }) {
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
        const rowA = A.H - 1 - yBot;
        const rowB = A.H - 1 - yTop;
        let maxAll = 0;
        let inBox = 0;
        let inChanged = 0;
        let maxIn = 0;
        for (let row = rowA; row <= rowB; row++) {
          for (let col = x0; col <= x1; col++) {
            const i = (row * A.W + col) * 4;
            const d = Math.max(
              Math.abs(A.px[i] - B.px[i]),
              Math.abs(A.px[i + 1] - B.px[i + 1]),
              Math.abs(A.px[i + 2] - B.px[i + 2]),
            );
            inBox++;
            if (d > 12) {
              inChanged++;
              if (d > maxIn) maxIn = d;
            }
          }
        }
        for (let i = 0; i < A.W * A.H * 4; i += 4) {
          const d = Math.max(
            Math.abs(A.px[i] - B.px[i]),
            Math.abs(A.px[i + 1] - B.px[i + 1]),
            Math.abs(A.px[i + 2] - B.px[i + 2]),
          );
          if (d > maxAll) maxAll = d;
        }
        return { inBox, inChanged, frac: inChanged / Math.max(1, inBox), maxIn, maxAll };
      },
      [a, b],
    );

  /** Counts inside the projected bounds: opaque part pixels (α ≥ 200) and tinted pixels
   * (channel spread > 12 — a ramp colour, not grey). */
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
        let tinted = 0;
        for (let row = A.H - 1 - yBot; row <= A.H - 1 - yTop; row++) {
          for (let col = x0; col <= x1; col++) {
            const i = (row * A.W + col) * 4;
            if (A.px[i + 3] >= 200) opaque++;
            const spread = Math.max(
              Math.abs(A.px[i] - A.px[i + 1]),
              Math.abs(A.px[i + 1] - A.px[i + 2]),
              Math.abs(A.px[i] - A.px[i + 2]),
            );
            if (spread > 12) tinted++;
          }
        }
        return { opaque, tinted, inBox: (x1 - x0 + 1) * (yBot - yTop + 1) };
      },
      cap,
    );

  /** Colour census of the analysed result mesh (distinct quantised colours, off-grey). */
  const colorCensus = () =>
    page.evaluate(() => {
      const c = window.fieldsmith.results?.colors?.() ?? null;
      if (!c) return { distinct: 0, offGrey: false, n: 0 };
      const seen = new Set();
      let offGrey = false;
      for (let i = 0; i < c.length; i += 3) {
        seen.add(`${Math.round(c[i] * 128)},${Math.round(c[i + 1] * 128)},${Math.round(c[i + 2] * 128)}`);
        if (Math.abs(c[i] - 0.55) > 0.01 || Math.abs(c[i + 1] - 0.55) > 0.01 || Math.abs(c[i + 2] - 0.55) > 0.01) offGrey = true;
      }
      return { distinct: seen.size, offGrey, n: c.length / 3 };
    });

  const setSlider = (testId, value) =>
    page.evaluate(
      ([tid, v]) => {
        const el = document.querySelector(`[data-testid="${tid}"]`);
        el.value = String(v);
        el.dispatchEvent(new Event('input', { bubbles: true }));
        return el.value;
      },
      [testId, value],
    );

  /** store.load replaces the document but not the ui display knobs — the matrix resets
   * them explicitly so each kind starts from the shipped defaults. */
  const resetUi = () =>
    page.evaluate(() => {
      const ui = window.fieldsmith.ui;
      ui.set('clip', { on: false, axis: 0, dir: 1, t: 0 });
      ui.set('bodyAlpha', 1);
      ui.set('resultsMode', 'stress');
      ui.set('resultsCase', -1);
      ui.set('resultsScale', 5);
      ui.set('resultsRange', null);
      ui.set('resultsModeShape', -1);
      ui.set('resultsModePhase', 0);
      ui.set('resultsModePlay', false);
      ui.set('sweepParam', null);
      ui.set('sweepFrom', null);
      ui.set('sweepTo', null);
      ui.set('sweepCount', 5);
      ui.set('makeOverhangOn', false);
      ui.set('makeSliceOn', false);
      ui.set('probe', null);
    });

  const setLens = async (lens) => {
    await page.locator(`[data-lens="${lens}"]`).click();
    await page.waitForFunction((l) => window.fieldsmith.store.lens === l, lens, { timeout: 5000 });
  };

  const loadFsm = async (text, tag) => {
    await resetUi();
    await page.evaluate(async (t) => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      const r = w.parseFsm(t);
      if ('error' in r) throw new Error(`${t.slice(0, 24)}: ${r.error.message}`);
      window.fieldsmith.store.load(r.ok);
    }, text);
    await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 60000 });
    // The previous kind may have left the lens elsewhere (store.load does not touch it):
    // drive the tab back to shape rather than wait for it.
    await setLens('shape');
    void tag;
  };

  /** One lens visit: switch, render, assert the frame is not blank. */
  const lensVisit = async (lens, minOpaque) => {
    await setLens(lens);
    await page.waitForTimeout(350);
    const cap = `__lens_${lens}`;
    await stash(cap);
    const s = await frameStats(cap);
    check(
      `lens ${lens}: the frame renders content (not a blank viewport)`,
      s.opaque >= minOpaque,
      `${s.opaque} opaque px in ${s.inBox} in-bounds px`,
    );
    return s;
  };

  /** Clip off: click the toggle only when it is currently on (the slider alone never turns off). */
  const waitClipOff = async () => {
    const on = await page.evaluate(() => window.fieldsmith.ui.clip.on);
    if (on) await page.locator('[data-testid="clip-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.viewport.material?.uniforms.u_clipOn.value !== 1, null, { timeout: 5000 });
  };

  /** The section control against the current frame: toggle-on cuts visibly, axis and flip
   * each change the frame, off round-trips bit-identically. Every capture first waits the
   * frame quiet — interaction idle and the full pixel ratio restored (render() halves it
   * while interacting, viewport.ts:896, so a capture taken mid-ease differs in size from
   * its baseline) — then two animation frames: the ui state lands synchronously on the
   * click, but the analysed mesh's clippingPlanes ride the next rAF sync pass (ViewportHost
   * schedule → ext.sync), and the damped orbit controls keep writing camera matrices until
   * they settle — a capture before that races a camera still easing (the solid-results
   * 0 px toggle-on and Δ226 off round-trip, the mesh-shape Δ47). */
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
  const clipCell = async (prefix) => {
    await waitClipOff();
    await settle();
    await stash(`${prefix}__off`);
    await page.locator('[data-testid="clip-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.clip.on === true && window.fieldsmith.ui.clip.t > 0, null, { timeout: 5000 });
    await settle();
    await stash(`${prefix}__on`);
    const dOn = await diffCaps(`${prefix}__off`, `${prefix}__on`);
    check(`${prefix}: clip toggle-on visibly changes the frame`, dOn.inChanged > 0 && dOn.maxIn > 0, `${dOn.inChanged}/${dOn.inBox} px changed, max Δ ${dOn.maxIn}/255`);
    const dirBefore = await page.evaluate(() => window.fieldsmith.ui.clip.dir);
    await page.locator('[data-testid="clip-flip"]').click();
    await page.waitForFunction((d) => window.fieldsmith.ui.clip.dir === -d, dirBefore, { timeout: 5000 });
    await settle();
    await stash(`${prefix}__flip`);
    const dFlip = await diffCaps(`${prefix}__on`, `${prefix}__flip`);
    check(`${prefix}: clip flip changes the frame`, dFlip.inChanged >= 50, `${dFlip.inChanged} px changed`);
    // Axis switch: click a button that actually differs from the current axis. The
    // default view auto-picks z on toggle-on (camera-relative), so a fixed clip-axis-z
    // click was a no-op state write and the frame never changed (the solid-results
    // 0 px axis check). Click the other axis and wait for the state to move.
    const axisBefore = await page.evaluate(() => window.fieldsmith.ui.clip.axis);
    const targetAxis = axisBefore === 0 ? 'y' : 'x';
    await page.locator(`[data-testid="clip-axis-${targetAxis}"]`).click();
    await page.waitForFunction((a) => window.fieldsmith.ui.clip.axis !== a, axisBefore, { timeout: 5000 });
    await settle();
    await stash(`${prefix}__axis`);
    const dAx = await diffCaps(`${prefix}__flip`, `${prefix}__axis`);
    check(`${prefix}: clip axis switch changes the frame`, dAx.inChanged >= 50, `${dAx.inChanged} px changed`);
    await waitClipOff();
    await settle();
    await stash(`${prefix}__off2`);
    const dRt = await diffCaps(`${prefix}__off`, `${prefix}__off2`);
    check(`${prefix}: clip off round-trips bit-identically`, dRt.maxAll === 0, `whole-frame max Δ ${dRt.maxAll}`);
  };

  /** The opacity slider against the current frame (present lenses only). Stashes its own
   * baseline: the caller's frame is opaque here by construction (resetUi + alpha restored). */
  const opacityCell = async (prefix) => {
    await stash(`${prefix}__off`);
    const prev = await page.evaluate(() => window.fieldsmith.ui.compileMs);
    await setSlider('opacity-slider', 0.35);
    await page.waitForFunction((p) => window.fieldsmith.ui.compileMs !== p, prev, { timeout: 30000 });
    await stash(`${prefix}__a35`);
    const d = await diffCaps(`${prefix}__off`, `${prefix}__a35`);
    check(`${prefix}: opacity 0.35 visibly changes the frame`, d.inChanged > 0 && d.maxIn > 0, `${d.inChanged}/${d.inBox} px changed, max Δ ${d.maxIn}/255`);
    await setSlider('opacity-slider', 1);
    await page.waitForFunction(() => window.fieldsmith.ui.bodyAlpha === 1, null, { timeout: 10000 });
  };

  const rename = async (name) => {
    const el = page.locator('[data-testid="name"]');
    await el.click();
    await el.fill(name);
    await el.press('Enter');
  };

  const palette = async (text) => {
    await page.keyboard.press('Control+k');
    await page.locator('[data-testid="palette-input"]').fill(text);
    await page.keyboard.press('Enter');
  };

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

    // ===================== A — analytic solid (examples/bracket.fsm) ======================
    at('A load solid');
    const solidFsm = readFileSync(resolve(here, '../../examples/bracket.fsm'), 'utf8');
    await loadFsm(solidFsm, 'solid');
    await page.waitForSelector('[data-testid="clip-panel"]');

    // Every lens renders content on the solid kind.
    await lensVisit('shape', 2000);
    await lensVisit('setup', 2000);
    await lensVisit('fill', 2000);
    await lensVisit('make', 2000);

    // Overhang overlay × Make lens (V45 / V39): toggling the overhang highlight (> 45° from +Z)
    // tints the downward-facing bolt-hole ceilings on the bracket from the exact field gradient ∇f(p).
    await settle();
    await stash('__makeOvOff');
    await page.locator('[data-testid="make-overhang-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.makeOverhangOn === true, null, { timeout: 5000 });
    await settle();
    await stash('__makeOvOn');
    const dOvOn = await diffCaps('__makeOvOff', '__makeOvOn');
    check(
      'solid × make: overhang toggle-on visibly tints downward-facing overhang pixels',
      dOvOn.inChanged >= 20 && dOvOn.maxIn > 50,
      `${dOvOn.inChanged}/${dOvOn.inBox} px changed, max Δ ${dOvOn.maxIn}/255`,
    );
    await page.locator('[data-testid="make-overhang-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.makeOverhangOn === false, null, { timeout: 5000 });
    await settle();
    await stash('__makeOvOff2');
    const dOvRt = await diffCaps('__makeOvOff', '__makeOvOff2');
    check(
      'solid × make: overhang toggle-off round-trips bit-identically',
      dOvRt.maxAll === 0,
      `whole-frame max Δ ${dOvRt.maxAll}`,
    );

    // Direct field slice overlay × Make lens (V49 / V39): toggling the layer preview on
    // without building a 3D mesh slices the implicit field directly and renders the Z contour.
    await page.locator('[data-testid="make-slice-toggle"]').click();
    await page.waitForFunction(
      () => window.fieldsmith.ui.makeSliceOn === true && (window.fieldsmith.analysis.state.slice?.segs?.length ?? 0) > 0,
      null,
      { timeout: 15000 },
    );
    await settle();
    await stash('__makeSlOn');
    const dSlOn = await diffCaps('__makeOvOff', '__makeSlOn');
    check(
      'solid × make: direct field slice toggle-on visibly draws contour pixels without mesh export',
      dSlOn.inChanged >= 5 && dSlOn.maxIn > 20,
      `${dSlOn.inChanged}/${dSlOn.inBox} px changed, max Δ ${dSlOn.maxIn}/255`,
    );
    await page.locator('[data-testid="make-slice-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.makeSliceOn === false, null, { timeout: 5000 });
    await settle();
    await stash('__makeSlOff2');
    const dSlRt = await diffCaps('__makeOvOff', '__makeSlOff2');
    check(
      'solid × make: direct field slice toggle-off round-trips bit-identically',
      dSlRt.maxAll === 0,
      `whole-frame max Δ ${dSlRt.maxAll}`,
    );

    // Section × solid (shape lens).
    await setLens('shape');
    await clipCell('solid-shape');

    // Opacity × solid.
    await opacityCell('solid-shape');

    // Results lens: the analysed mesh renders (opaque overlay) and is ramp-coloured.
    await setLens('results');
    await awaitLadder(page, 'solid');
    await page.waitForSelector('[data-testid="results-panel"]', { timeout: 10000 });
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });
    await stash('__solidRes');
    const solidResStats = await frameStats('__solidRes');
    const solidCensus = await colorCensus();
    check(
      'solid × results: the analysed mesh renders non-blank and ramp-coloured',
      solidResStats.opaque >= 500 && solidCensus.distinct >= 2 && solidCensus.offGrey,
      `${solidResStats.opaque} opaque px; ${solidCensus.distinct} distinct colours, off-grey ${solidCensus.offGrey}`,
    );

    // Opacity × results: the control is absent by design (no field shader to thin).
    const opacityInResults = await page.evaluate(() => !!document.querySelector('[data-testid="opacity-slider"]'));
    check('solid × results: the opacity control is absent (no field shader to make transparent)', opacityInResults === false, `opacity-slider present: ${opacityInResults}`);

    // Mode chips: stress → deflection changes the mesh colours and the frame.
    await page.locator('[data-testid="results-mode"] button[data-value="deflection"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'deflection', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    await stash('__solidDefl');
    const dMode = await diffCaps('__solidRes', '__solidDefl');
    check('solid × results: the mode chip stress → deflection changes the frame', dMode.inChanged > 0, `${dMode.inChanged} px changed`);

    // Peel mode chip: inter-layer peel utilization along the build axis, with orientation summary.
    await page.locator('[data-testid="results-mode"] button[data-value="peel"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'peel', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    await stash('__solidPeel');
    const dPeel = await diffCaps('__solidRes', '__solidPeel');
    const peelUi = await page.evaluate(() => ({
      caption: document.querySelector('[data-testid="legend"] .meta')?.textContent ?? '',
      summary: document.querySelector('[data-testid="results-peel-summary"]')?.textContent ?? '',
    }));
    check(
      'solid × results: peel mode colours inter-layer peel utilization and reports best build orientation',
      dPeel.inChanged > 0 && peelUi.caption.includes('inter-layer peel utilization') && peelUi.summary.includes('best ('),
      `${dPeel.inChanged} px vs stress, summary "${peelUi.summary}"`,
    );

    // Vectors: grey base + white arrows over the deformed shape.
    await page.locator('[data-testid="results-mode"] button[data-value="vectors"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'vectors', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const arrowCount = await page.evaluate(() => {
      let n = 0;
      const walk = (o) => {
        for (const c of o.children) {
          if (c.type === 'ArrowHelper') n++;
          walk(c);
        }
      };
      walk(window.fieldsmith.viewport.overlays);
      return n;
    });
    await stash('__solidVec');
    const vecStats = await frameStats('__solidVec');
    check(
      'solid × results: vectors renders the base mesh plus arrow helpers',
      arrowCount >= 5 && vecStats.opaque >= 500,
      `${arrowCount} arrow helpers, ${vecStats.opaque} opaque px`,
    );

    // Temperature on a run with no temperature statement: the named no-data state.
    await page.locator('[data-testid="results-mode"] button[data-value="temperature"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'temperature', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const tempNoField = await page.evaluate(() => ({
      caption: document.querySelector('[data-testid="legend"] .meta')?.textContent ?? '',
      census: (() => {
        const c = window.fieldsmith.results.colors();
        let offGrey = false;
        for (let i = 0; i < c.length; i += 3) {
          if (Math.abs(c[i] - 0.55) > 0.01 || Math.abs(c[i + 1] - 0.55) > 0.01 || Math.abs(c[i + 2] - 0.55) > 0.01) offGrey = true;
        }
        return offGrey;
      })(),
    }));
    check(
      'solid × results: temperature without a field says so and renders the grey base (not blank)',
      tempNoField.caption.includes('no temperature field') && tempNoField.census === false,
      `caption "${tempNoField.caption.slice(0, 60)}…", off-grey ${tempNoField.census}`,
    );

    // Click probe on the analysed mesh.
    await page.locator('[data-testid="results-mode"] button[data-value="stress"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'stress', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const center = await page.evaluate(() => {
      const F = window.fieldsmith;
      const s = F.view.project([30, 20, 4]);
      return s ? { x: s.x, y: s.y } : null;
    });
    if (center) await page.mouse.click(center.x, center.y);
    await page.waitForTimeout(300);
    const probe = await page.evaluate(() => document.querySelector('[data-testid="probe"]')?.textContent ?? '');
    check('solid × results: clicking the mesh writes a probe readout', probe.length > 0, `probe "${probe.slice(0, 60)}"`);

    // Section × results mesh (the overlay rides the plane).
    await clipCell('solid-results');

    // Sweep on the solid kind: two points, the document keeps its values (the sweep
    // family's driving idiom: selectOption on the picker, fill + Tab on the NumberInputs,
    // which commit on blur).
    const before = await page.evaluate(() => JSON.stringify(window.fieldsmith.store.doc));
    await page.selectOption('[data-testid="sweep-param"]', 'base.size.0');
    await page.locator('[data-testid="sweep-from"]').fill('50');
    await page.locator('[data-testid="sweep-from"]').press('Tab');
    await page.locator('[data-testid="sweep-to"]').fill('70');
    await page.locator('[data-testid="sweep-to"]').press('Tab');
    await page.locator('[data-testid="sweep-count"]').fill('2');
    await page.locator('[data-testid="sweep-count"]').press('Tab');
    await page.locator('[data-testid="sweep-run"]').click();
    const sweepDone = await page
      .waitForFunction(() => document.querySelectorAll('[data-testid^="sweep-row-"]').length >= 2, null, { timeout: 240000 })
      .then(() => true)
      .catch(() => false);
    const after = await page.evaluate(() => JSON.stringify(window.fieldsmith.store.doc));
    check('solid × sweep: two points run and the live document keeps its values', sweepDone && before === after, `rows ${sweepDone ? '≥ 2' : 'never reached 2'}, doc unchanged ${before === after}`);

    await page.screenshot({ path: resolve(screens, 'combinations-01-solid.png') });

    // ===================== B — filled (examples/bracket-filled.fsm) ======================
    at('B load filled');
    const filledFsm = readFileSync(resolve(here, '../../examples/bracket-filled.fsm'), 'utf8');
    await loadFsm(filledFsm, 'filled');

    await lensVisit('shape', 2000);
    await lensVisit('fill', 2000);

    // Opacity × filled: the see-through fill is this control's reason to exist.
    await setLens('shape');
    await settle();
    await stash('__filledOff');
    await opacityCell('filled-shape');

    // Section × filled at 35 % opacity: the open cut exposes the lattice (the section
    // family's σ ≥ 0.10 finding, re-asserted as a combination).
    await page.locator('[data-testid="clip-toggle"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.clip.on === true, null, { timeout: 5000 });
    await setSlider('clip-slider', 0.5);
    await page.waitForFunction(() => Math.abs(window.fieldsmith.ui.clip.t - 0.5) < 1e-9, null, { timeout: 5000 });
    await settle();
    await stash('__filledCut');
    const dCut = await diffCaps('__filledOff', '__filledCut');
    check('filled × clip × opacity: the cut frame changes the translucent frame', dCut.inChanged > 0, `${dCut.inChanged} px changed`);
    await waitClipOff();
    await setSlider('opacity-slider', 1);
    await page.waitForFunction(() => window.fieldsmith.ui.bodyAlpha === 1, null, { timeout: 10000 });

    // Fill lattice rotation × opacity (V44 / V39): rotating the lattice in the Fill lens
    // changes the interior lattice shader (`fs_core`) and visibly changes the viewport pixels.
    await setLens('fill');
    await setSlider('opacity-slider', 0.35);
    await page.waitForFunction(() => Math.abs(window.fieldsmith.ui.bodyAlpha - 0.35) < 1e-6, null, { timeout: 5000 });
    await settle();
    await stash('__fillRot0');
    const prevFillMs = await page.evaluate(() => window.fieldsmith.ui.compileMs);
    await page.locator('[data-testid="fill-rotate-z"]').fill('45');
    await page.locator('[data-testid="fill-rotate-z"]').press('Enter');
    await page.waitForFunction((p) => window.fieldsmith.ui.compileMs !== p, prevFillMs, { timeout: 30000 });
    await settle();
    await stash('__fillRot45');
    const dFillRot = await diffCaps('__fillRot0', '__fillRot45');
    check(
      'filled × fill-rotate-z: rotating lattice by 45° visibly changes the translucent frame',
      dFillRot.inChanged >= 50 && dFillRot.maxIn > 12,
      `${dFillRot.inChanged}/${dFillRot.inBox} px changed, max Δ ${dFillRot.maxIn}/255`,
    );
    await page.locator('[data-testid="fill-rotate-z"]').fill('0');
    await page.locator('[data-testid="fill-rotate-z"]').press('Enter');
    await setSlider('opacity-slider', 1);
    await page.waitForFunction(() => window.fieldsmith.ui.bodyAlpha === 1, null, { timeout: 10000 });

    // Results on the filled kind.
    await setLens('results');
    await awaitLadder(page, 'filled');
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });
    await stash('__filledRes');
    const filledStats = await frameStats('__filledRes');
    check('filled × results: the analysed mesh renders non-blank', filledStats.opaque >= 500, `${filledStats.opaque} opaque px`);

    await page.screenshot({ path: resolve(screens, 'combinations-02-filled.png') });

    // ===================== C — mesh leaf (box.glb through the palette) ====================
    at('C import mesh');
    await resetUi();
    await page.keyboard.press('Control+k');
    await page.locator('[data-testid="palette-input"]').fill('New document');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.length === 0, null, { timeout: 10000 });
    const fixture = readFileSync(new URL('./fixtures/box.glb', import.meta.url));
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), palette('Import mesh')]);
    await chooser.setFiles({ name: 'box.glb', mimeType: 'model/gltf-binary', buffer: fixture });
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.some((n) => n.kind.type === 'mesh'), null, { timeout: 30000 });
    const meshId = await page.evaluate(() => window.fieldsmith.store.doc.root[0].id);

    // Setup: patch-clicked support and load (the mesh family's verified flow).
    await setLens('setup');
    await page.waitForSelector('[data-testid="add-support"]', { timeout: 5000 });
    const aim = async (dir) => {
      await page.evaluate((d) => {
        window.fieldsmith.view.view(d);
        window.fieldsmith.view.fit(false);
      }, dir);
    };
    const clickPatch = async (node, plane) => {
      const hit = await page.evaluate(
        ([sh, at]) => {
          const F = window.fieldsmith;
          for (const q of [[-6, -6, at], [6, -6, at], [6, 6, at], [-6, 6, at], [0, 0, at]]) {
            const s = F.view.project(q);
            if (!s) continue;
            const h = F.pickAt(s.x, s.y);
            if (h && h.shape === sh && h.patch != null && Math.abs(h.point[2] - at) < 0.5) return { patch: h.patch, x: s.x, y: s.y };
          }
          return null;
        },
        [node, plane],
      );
      if (!hit) throw new Error(`no visible patch of ${node} on the z = ${plane} plane to click`);
      await page.mouse.click(hit.x, hit.y);
      return hit;
    };
    await aim([0, 0, -1]);
    await clickPatch(meshId, -10);
    await rename('bottom');
    await page.keyboard.press('Escape');
    await aim([0, 0, 1]);
    await clickPatch(meshId, 10);
    await rename('top');
    await page.locator('[data-testid="region-bottom"]').click();
    await page.locator('[data-testid="add-support"]').click();
    await page.locator('[data-testid="region-top"]').click();
    await page.locator('[data-testid="add-load"]').click();

    await lensVisit('shape', 500);
    await lensVisit('setup', 500);

    await setLens('shape');
    await clipCell('mesh-shape');
    await opacityCell('mesh-shape');

    await setLens('results');
    await awaitLadder(page, 'mesh');
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });
    await stash('__meshRes');
    const meshStats = await frameStats('__meshRes');
    const meshCensus = await colorCensus();
    check(
      'mesh leaf × results: the analysed mesh renders non-blank and ramp-coloured',
      meshStats.opaque >= 200 && meshCensus.distinct >= 2 && meshCensus.offGrey,
      `${meshStats.opaque} opaque px; ${meshCensus.distinct} distinct colours, off-grey ${meshCensus.offGrey}`,
    );

    await page.screenshot({ path: resolve(screens, 'combinations-03-mesh.png') });

    // ===================== D — combined modes + thermal cantilever =======================
    at('D load combined');
    await loadFsm(COMBINED_DOC, 'combined');
    await setLens('results');
    await awaitLadder(page, 'combined');
    await page.waitForSelector('[data-testid="results-panel"]', { timeout: 10000 });
    await page.waitForFunction(() => window.fieldsmith.results?.colors?.()?.length > 0, null, { timeout: 15000 });

    // Case picker: Envelope vs the mechanical case vs the thermal case.
    await page.waitForSelector('[data-testid="results-case"]', { timeout: 10000 });
    await stash('__combEnv');
    await page.locator('[data-testid="results-case"] button', { hasText: 'Load 1' }).first().click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsCase === 0, null, { timeout: 5000 });
    await page.waitForTimeout(350);
    await stash('__combLoad');
    const dCase = await diffCaps('__combEnv', '__combLoad');
    check('combined × case picker: Envelope → Load 1 changes the frame', dCase.inChanged > 0, `${dCase.inChanged} px changed`);

    // Temperature mode with a real field: the named temperature caption, ramp colours.
    // The thermal slot is the Thermal 1 case (tempField reads ts[ci]; with Load 1 selected
    // the field is null and the mode greys out — the graded family clicks Thermal 1 first).
    await page.locator('[data-testid="results-case"] button', { hasText: 'Thermal 1' }).first().click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsCase === 1, null, { timeout: 5000 });
    await page.locator('[data-testid="results-mode"] button[data-value="temperature"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'temperature', null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const tempOn = await page.evaluate(() => ({
      caption: document.querySelector('[data-testid="legend"] .meta')?.textContent ?? '',
      offGrey: (() => {
        const c = window.fieldsmith.results.colors();
        for (let i = 0; i < c.length; i += 3) {
          if (Math.abs(c[i] - 0.55) > 0.01 || Math.abs(c[i + 1] - 0.55) > 0.01 || Math.abs(c[i + 2] - 0.55) > 0.01) return true;
        }
        return false;
      })(),
    }));
    await stash('__combTemp');
    const tempStats = await frameStats('__combTemp');
    check(
      'combined × temperature: the thermal field ramps (caption names temperature, colours off-grey)',
      tempOn.caption.includes('temperature') && tempOn.offGrey && tempStats.opaque >= 200,
      `caption "${tempOn.caption.slice(0, 60)}…", off-grey ${tempOn.offGrey}, ${tempStats.opaque} opaque px`,
    );

    // Mode-shape animation on the same document: the picker exists beside the thermal case.
    await page.locator('[data-testid="results-mode"] button[data-value="deflection"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsMode === 'deflection', null, { timeout: 5000 });
    await page.waitForSelector('[data-testid="modes-shape"]', { timeout: 10000 });
    await page.evaluate(() => {
      window.__statPos = Float64Array.from(window.fieldsmith.results.positions());
    });
    await page.locator('[data-testid="modes-shape"] button[data-value="0"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsModeShape === 0, null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const shapeMoved = await page.evaluate(
      () => {
        const p = window.fieldsmith.results.positions();
        const s = window.__statPos;
        if (!p || p.length !== s.length) return { moved: false, n: p?.length ?? 0 };
        let max = 0;
        for (let i = 0; i < p.length; i++) max = Math.max(max, Math.abs(p[i] - s[i]));
        return { moved: max > 1e-9, max };
      },
    );
    await stash('__combMode');
    const dModeAnim = await diffCaps('__combTemp', '__combMode');
    check(
      'combined × modes: selecting mode 1 deforms the mesh off the static positions',
      shapeMoved.moved && dModeAnim.inChanged > 0,
      `max |Δpos| ${shapeMoved.max?.toExponential?.(2) ?? shapeMoved.max} m, ${dModeAnim.inChanged} px changed`,
    );

    // Phase change moves the deformed mesh.
    const phase = page.locator('[data-testid="modes-phase"]');
    await phase.fill('90');
    await phase.press('Tab');
    await page.waitForFunction(() => Math.abs(window.fieldsmith.ui.resultsModePhase - Math.PI / 2) < 1e-6, null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const phaseMoved = await page.evaluate(
      () => {
        const p = window.fieldsmith.results.positions();
        const s = window.__statPos;
        let max = 0;
        for (let i = 0; i < p.length; i++) max = Math.max(max, Math.abs(p[i] - s[i]));
        return max;
      },
    );
    check('combined × modes: phase 90° keeps the mesh off the static positions', phaseMoved > 1e-9, `max |Δpos| ${phaseMoved.toExponential(2)} m`);

    // Play advances the phase on its own.
    await page.locator('[data-testid="modes-play"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsModePlay === true, null, { timeout: 5000 });
    const playAdvanced = await page
      .waitForFunction(() => window.fieldsmith.ui.resultsModePhase !== Math.PI / 2, null, { timeout: 5000 })
      .then(() => true)
      .catch(() => false);
    await page.locator('[data-testid="modes-play"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsModePlay === false, null, { timeout: 5000 });
    check('combined × modes: play advances the phase and pause stops it', playAdvanced, `phase advanced while playing: ${playAdvanced}`);

    // Off restores the static positions byte for byte.
    await page.locator('[data-testid="modes-shape"] button[data-value="-1"]').click();
    await page.waitForFunction(() => window.fieldsmith.ui.resultsModeShape === -1, null, { timeout: 5000 });
    await page.waitForTimeout(350);
    const backStatic = await page.evaluate(
      () => {
        const p = window.fieldsmith.results.positions();
        const s = window.__statPos;
        if (!p || p.length !== s.length) return false;
        for (let i = 0; i < p.length; i++) if (p[i] !== s[i]) return false;
        return true;
      },
    );
    check('combined × modes: Off restores the static mesh byte for byte', backStatic, 'positions compared float-exact');

    // Section × the animated-capable results mesh (picker now Off, static mesh shown).
    await clipCell('combined-results');

    await page.screenshot({ path: resolve(screens, 'combinations-04-combined.png') });

    // ===================== E — sketch edit repaints the viewport =========================
    at('E load sketch');
    await loadFsm(SKETCH_DOC, 'sketch');
    await page.locator('.row[data-node="pad"]').click();
    await page.waitForSelector('[data-testid="profile-sketch"]', { timeout: 10000 });
    await page.waitForTimeout(350);
    await stash('__sketch0');
    const box = await page.locator('[data-testid="profile-sketch"]').boundingBox();
    // The fit constants are pinned behaviour of Sketch.tsx (the profilesketch family
    // establishes them): W 240, H 180, PAD 20 over the (0,0)-(20,10) profile.
    const s = Math.min((240 - 40) / 20, (180 - 40) / 10);
    const ox = box.x + (box.width - 20 * s) / 2;
    const oy = box.y + (box.height + 10 * s) / 2;
    await page.mouse.click(ox + 10 * s, oy + 0.5 * s); // mid bottom edge → insert (10, ~0)
    const inserted = await page
      .waitForFunction(
        () => {
          const n = window.fieldsmith.store.doc.root.find((x) => x.id === 'pad');
          return n && n.kind.profile.length === 5;
        },
        null,
        { timeout: 5000 },
      )
      .then(() => true)
      .catch(() => false);
    await page.waitForTimeout(500);
    await stash('__sketch1');
    const dSketch = await diffCaps('__sketch0', '__sketch1');
    check(
      'sketch × viewport: an inserted profile point repaints the 3D frame',
      inserted && dSketch.inChanged > 0,
      `profile points ${inserted ? 5 : 'unchanged'}, ${dSketch.inChanged} px changed, max Δ ${dSketch.maxIn}/255`,
    );

    // ---- report ---------------------------------------------------------------------------
    check('no console errors', errors.length === 0, errors.join(' | ').slice(0, 400));
    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);

    const names = checks.map((c) => c.name);
    const lines = [
      '# V39 — feature-combination matrix: overlays × documents × lenses',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
      '',
      'Every overlay control is driven through its real UI widget against every document kind',
      '(the shipped analytic bracket, the filled bracket, an imported mesh leaf, and a combined',
      'modes+thermal cantilever) and every lens, and every state change is followed by a',
      'framebuffer assertion: the frame is non-blank (opaque part pixels inside the projected',
      'bounds) and the control moved pixels (RGB Δ > 12/255). State variables are read only to',
      'gate the waits — the verdicts are pixels, because a state variable can change correctly',
      'while the render stays dark.',
      '',
      'Captures are full-canvas RGBA readbacks of live frames (`vp.render()` then',
      '`gl.readPixels`, `preserveDrawingBuffer` retains content), the section family’s idiom.',
      'The analysed mesh is additionally censused at the vertex-colour level',
      '(`results.colors()` distinct quantised colours, off the 0.55 no-data grey) and its',
      'positions compared float-exact for the mode-shape Off round-trip.',
      '',
      'store.load replaces the document but not the ui display knobs, so the matrix resets',
      'clip, opacity, mode, case, scale, range, shape/phase/play, sweep and probe between',
      'document kinds; each kind therefore starts from the shipped defaults.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- The opacity × results cell asserts the control’s absence (the results lens has no',
      '  field shader to thin); the see-through behaviour itself is the M12 family’s.',
      '- The temperature-without-field cell asserts the named grey state; the thermal ramp is',
      '  asserted on the combined document, where the field exists.',
      '- Sweep points run on the rung ladder (coarse + refined) on the analysis worker; the',
      '  cell asserts rows appear and the live document keeps its values, not the rung values.',
      '- The mesh-leaf kind is the 20 mm box fixture with one support and one force load; its',
      '  import and patch-pick mechanics are the mesh family’s, re-used here only to reach a',
      '  solved mesh document.',
      '',
      `Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
      '',
      'Screenshots: web/test-results/combinations-0{1..4}-*.png.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/combinations.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(
        ['combinations: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'),
      );
    } else {
      writeFileSync(resolve(docsDir, 'V39.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = `at ${step}: ${String(e).split('\n')[0]}`;
    console.error(`combinations: run aborted at ${step}: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}
