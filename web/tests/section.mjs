// V26 — section-plane clip: a plane sweeps through the raymarched body (and the analysed
// result mesh) in real time, inspected through live framebuffer reads.
//
// (a) No-op at rest: toggling on with the slider parked at 0 routes the rest plane inert
//     (u_clipOn = 0 — the plane coincides with the field bound face, so the renderer takes
//     the null clip rather than a plane that can shave the march interval by an ulp),
//     rendering the intact part bit-identically (no pixel anywhere changes); on → off
//     round-trips bit-identically too.
// (b) The cut is monotonic in t: changed-pixel counts grow strictly across 0.25 → 0.5 → 0.75,
//     and the mid cut changes ≥ 8 % of the in-bounds pixels.
// (c) Material truly leaves: opaque frame, pixels that were solid part in the intact frame
//     and are cleared background/floor in the cut frame (removed silhouette) ≥ 100 px.
// (d) The open cut exposes the interior: at 35 % body opacity the cut frame's changed pixels
//     carry the same strut/void luminance spread the transparency leg measures (σ ≥ 0.10).
// (e) Flip and axis switch: flip preserves the plane's world position (plane position read
//     as D · Nₐ so the dir sign convention cancels) and changes the frame; an axis switch
//     changes the normal and the frame.
// (f) Uniform wiring: u_clipOn / u_clipN / u_clipD carry the ui clip state each step, and
//     the plane position advances linearly in t.
// (g) The results lens' analysed mesh rides the same plane: overlay materials carry
//     clippingPlanes matching the ui state while on, none while off, with a frame delta.
// (h) Clip state is session-only: it resets on reload.
// There is no Rust cross-implementation to compare against — the clip is GLSL geometry, so
// verification is framebuffer- and uniform-level only.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

export async function sectionTest(browser, url, { screens, docsDir }) {
  const errors = [];
  const collectErrors = (page) => {
    page.on('console', (m) => {
      if (m.type() === 'error') errors.push(m.text());
    });
    page.on('pageerror', (e) => errors.push(String(e)));
  };

  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const filledFsm = readFileSync(resolve(here, '../../examples/bracket-filled.fsm'), 'utf8');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  collectErrors(page);

  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('[data-testid="empty-state"]');

  // Load the filled document through the store, the same path the Open… flow takes.
  await page.evaluate(async (text) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(text);
    if ('error' in r) throw new Error(`bracket-filled.fsm: ${r.error.message}`);
    window.fieldsmith.store.load(r.ok);
  }, filledFsm);
  await page.waitForFunction(() => window.fieldsmith.ui.compileMs > 0, null, { timeout: 30000 });
  await page.waitForSelector('[data-testid="clip-panel"]');

  /** Render the current state and stash the RGBA readback under `name`, in-page. */
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

  /** Compare two stashed captures, over the whole frame and inside the projected bounds. */
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
        let removed = 0;
        let sum = 0;
        let sum2 = 0;
        let nLum = 0;
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
              const lum = (0.2126 * B.px[i] + 0.7152 * B.px[i + 1] + 0.0722 * B.px[i + 2]) / 255;
              sum += lum;
              sum2 += lum * lum;
              nLum++;
            }
            // Part pixel (opaque body, alpha 255) that the cut frame cleared: floor lines
            // write alpha ≤ 0.12·255, background writes 0.
            if (A.px[i + 3] >= 200 && B.px[i + 3] <= 40) removed++;
          }
        }
        // Whole-frame max (outside the projected rect too — catches silhouette spill).
        for (let i = 0; i < A.W * A.H * 4; i += 4) {
          const d = Math.max(
            Math.abs(A.px[i] - B.px[i]),
            Math.abs(A.px[i + 1] - B.px[i + 1]),
            Math.abs(A.px[i + 2] - B.px[i + 2]),
          );
          if (d > maxAll) maxAll = d;
        }
        const mean = sum / Math.max(1, nLum);
        return {
          inBox,
          inChanged,
          frac: inChanged / Math.max(1, inBox),
          maxIn,
          maxAll,
          removed,
          lumStd: Math.sqrt(Math.max(0, sum2 / nLum - mean * mean)),
          lumMean: mean,
        };
      },
      [a, b],
    );

  /** RGBA of the pixel nearest a world point in capture `cap`. */
  const pixelAt = (cap, world) =>
    page.evaluate(
      ([capName, p]) => {
        const vp = window.fieldsmith.viewport;
        const s = vp.project(p);
        const c = vp.renderer.domElement.getBoundingClientRect();
        const dpr = window.devicePixelRatio;
        const cp = window[capName];
        const dx = Math.min(cp.W - 1, Math.max(0, Math.round((s.x - c.left) * dpr)));
        const dy = Math.min(cp.H - 1, Math.max(0, Math.round((s.y - c.top) * dpr)));
        const row = cp.H - 1 - dy; // readPixels rows run bottom-up
        const i = (row * cp.W + dx) * 4;
        return Array.from(cp.px.slice(i, i + 4));
      },
      [cap, world],
    );

  const setClip = (value) =>
    page.evaluate((v) => {
      const el = document.querySelector('[data-testid="clip-slider"]');
      el.value = String(v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
      return el.value;
    }, value);

  const uiClip = () => page.evaluate(() => ({ ...window.fieldsmith.ui.clip }));
  const uniforms = () =>
    page.evaluate(() => {
      const u = window.fieldsmith.viewport.material?.uniforms ?? {};
      return {
        on: u.u_clipOn?.value,
        n: u.u_clipN ? [u.u_clipN.value.x, u.u_clipN.value.y, u.u_clipN.value.z] : null,
        d: u.u_clipD?.value,
      };
    });

  // Overlay walk, in-page once so waitForFunction can poll it.
  await page.evaluate(() => {
    window.__walkOverlays = () => {
      const vp = window.fieldsmith.viewport;
      let meshMats = 0;
      let withPlane = 0;
      let planes = null;
      const walk = (o) => {
        for (const c of o.children) {
          if (c.material) {
            const mats = Array.isArray(c.material) ? c.material : [c.material];
            for (const m of mats) {
              meshMats++;
              if (m.clippingPlanes && m.clippingPlanes.length) {
                withPlane++;
                planes = m.clippingPlanes.map((p) => [p.normal.x, p.normal.y, p.normal.z, p.constant]);
              }
            }
          }
          walk(c);
        }
      };
      walk(vp.overlays);
      return { meshMats, withPlane, planes };
    };
  });
  const overlayWalk = () => page.evaluate(() => window.__walkOverlays());

  /** Expected plane from the ui state and the field bounds — the contract the uniforms must
   * match. dir=+1 keeps p_a ≥ pos with D = pos; dir=−1 keeps p_a ≤ pos with N = −e_a and
   * D = −pos (both sides keep dot(p, N) − D ≥ 0). */
  const expectedPlane = () =>
    page.evaluate(() => {
      const c = window.fieldsmith.ui.clip;
      const b = window.fieldsmith.viewport.bounds;
      const a = c.axis;
      const lo = b[0][a];
      const hi = b[1][a];
      const span = hi - lo;
      const n = [0, 0, 0];
      n[a] = c.dir;
      // dir=+1 keeps the +axis half (plane sweeps up from lo); dir=−1 keeps the −axis half
      // (plane sweeps down from hi). t = 0 restores the intact part in both directions.
      const pos = c.dir === 1 ? lo + c.t * span : hi - c.t * span;
      return { on: c.on, n, d: c.dir === 1 ? pos : -pos, pos, lo, hi, span, t: c.t };
    });

  const assertPlane = (uni, want) => {
    const near = (x, y, eps = 1e-6) =>
      x !== null && y !== null && Math.abs(x[0] - y[0]) < eps && Math.abs(x[1] - y[1]) < eps && Math.abs(x[2] - y[2]) < eps;
    if (want.on && want.t <= 0) {
      // Rest position: the plane sits on the field bound face, so the viewport routes it
      // inert (u_clipOn = 0) rather than risk an ulp-level interval shave at the face.
      check('rest plane routes inert (u_clipOn=0 while ui.clip.on)', want.on && uni.on === 0, `ui.on ${want.on}, u_clipOn ${uni.on}`);
    } else if (want.on) {
      check(
        `clip on rides u_clipOn=1 → u_clipN=${want.n.map((v) => v.toFixed(0))} u_clipD=${want.d.toFixed(4)}`,
        uni.on === 1 && near(uni.n, want.n) && Math.abs(uni.d - want.d) < 1e-6,
        `got on ${uni.on}, n ${JSON.stringify(uni.n)}, d ${uni.d}`,
      );
    } else {
      check('clip off rides u_clipOn=0', uni.on === 0, `got ${uni.on}`);
    }
  };

  // ---- (a) engaging cut is visible; the t = 0 rest state keeps the intact part ------------
  await stash('__capOff');
  await page.locator('[data-testid="clip-toggle"]').click();
  await page.waitForFunction(() => window.fieldsmith.ui.clip.t > 0, null, { timeout: 5000 });
  const t0 = await uiClip();
  const plane0 = await expectedPlane();
  const uni0 = await uniforms();
  assertPlane(uni0, plane0);
  check('toggle-on auto-picks a direction (dir ±1)', t0.dir === 1 || t0.dir === -1, JSON.stringify(t0));
  check(
    'toggle-on lands mid-part (t = 0.5, a visible cut, not the no-cut rest state)',
    Math.abs(t0.t - 0.5) < 1e-9,
    JSON.stringify(t0),
  );

  await stash('__capOn05');
  const dOn = await diffCaps('__capOff', '__capOn05');
  check(
    'the engaged section visibly cuts part of the frame',
    dOn.inChanged > 0 && dOn.maxIn > 0,
    `in-bounds max Δ ${dOn.maxIn}/255, changed ${dOn.inChanged}/${dOn.inBox}`,
  );

  // The t = 0 rest state: reachable via the slider, keeps the intact part bit-exactly.
  await setClip('0');
  await page.waitForFunction(() => Math.abs(window.fieldsmith.ui.clip.t) < 1e-9, null, { timeout: 5000 });
  await stash('__capOn0');
  const d0 = await diffCaps('__capOff', '__capOn0');
  check(
    'clip on at t = 0 keeps the intact part (whole-frame max Δ = 0)',
    d0.maxAll === 0,
    `in-bounds max Δ ${d0.maxIn}/255, whole-frame max Δ ${d0.maxAll}/255, changed ${d0.inChanged}/${d0.inBox}`,
  );

  await waitClipOff(page);
  await stash('__capOff2');
  const dRt = await diffCaps('__capOff', '__capOff2');
  check(
    'clip on → off round-trips to a bit-identical frame (whole-frame max Δ = 0)',
    dRt.maxAll === 0,
    `whole-frame max Δ ${dRt.maxAll}`,
  );
  await page.locator('[data-testid="clip-toggle"]').click();
  await page.waitForFunction(() => window.fieldsmith.ui.clip.on === true, null, { timeout: 5000 });

  // ---- (b) monotonic cut (axis x, default dir from the toggle) ----------------------------
  const counts = {};
  for (const t of ['0.25', '0.5', '0.75']) {
    await setClip(t);
    await page.waitForFunction((v) => Math.abs(window.fieldsmith.ui.clip.t - v) < 1e-9, Number(t), { timeout: 5000 });
    await stash(`__capT${t}`);
    const uni = await uniforms();
    const want = await expectedPlane();
    assertPlane(uni, want);
    counts[t] = await diffCaps('__capOff', `__capT${t}`);
  }
  const fracBound = 0.08; // stated bound: ≥ 8 % of the projected-bounds pixels differ
  check(
    `mid cut changes ≥ ${fracBound * 100} % of the in-bounds pixels`,
    counts['0.5'].frac >= fracBound,
    `${(counts['0.5'].frac * 100).toFixed(2)} % of ${counts['0.5'].inBox} px (counts ${counts['0.25'].inChanged}/${counts['0.5'].inChanged}/${counts['0.75'].inChanged})`,
  );
  check(
    'changed-pixel count grows strictly 0.25 → 0.5 → 0.75',
    counts['0.25'].inChanged < counts['0.5'].inChanged && counts['0.5'].inChanged < counts['0.75'].inChanged,
    `${counts['0.25'].inChanged} < ${counts['0.5'].inChanged} < ${counts['0.75'].inChanged}`,
  );

  // ---- (c) material actually leaves -------------------------------------------------------
  check(
    'removed silhouette ≥ 100 px at t = 0.75 (part cleared to background/floor)',
    counts['0.75'].removed >= 100,
    `${counts['0.75'].removed} px cleared`,
  );

  // ---- (d) the open cut exposes the lattice ------------------------------------------------
  // 35 % body opacity with no clip: baseline. Then the z-cut opens the upright and the
  // changed pixels must show the strut/void spread, not a flat wash.
  await waitClipOff(page);
  const alphaPrev = await page.evaluate(() => window.fieldsmith.ui.compileMs);
  await page.evaluate((v) => {
    const el = document.querySelector('[data-testid="opacity-slider"]');
    el.value = String(v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }, 0.35);
  await page.waitForFunction((p) => window.fieldsmith.ui.compileMs !== p, alphaPrev, { timeout: 30000 });
  await stash('__capA35');
  await page.locator('[data-testid="clip-axis-z"]').click();
  await page.locator('[data-testid="clip-toggle"]').click();
  await page.waitForFunction(() => window.fieldsmith.ui.clip.on === true, null, { timeout: 5000 });
  const zOn0 = await uniforms();
  const zWant0 = await expectedPlane();
  assertPlane(zOn0, zWant0);
  await setClip('0.5');
  await page.waitForFunction(() => Math.abs(window.fieldsmith.ui.clip.t - 0.5) < 1e-9, null, { timeout: 5000 });
  await stash('__capA35Cut');
  const d035 = await diffCaps('__capA35', '__capA35Cut');
  const stdBound = 0.1;
  check(
    `open cut exposes the lattice interior (changed-pixel luminance σ ≥ ${stdBound})`,
    d035.inChanged > 0 && d035.lumStd >= stdBound,
    `${d035.inChanged}/${d035.inBox} px changed (Δ>12), σ ${d035.lumStd.toFixed(3)}, mean ${d035.lumMean.toFixed(3)}`,
  );

  // The cap face must show material where the intact frame showed part: a projected point
  // on the mid-plane cross-section (upright centre) renders — the translucent film alone
  // gives ~0.35·255 ≈ 89 alpha, so ≥ 60 separates it from cleared background/floor.
  const want2 = await expectedPlane();
  const capProbe = await pixelAt('__capA35Cut', [30, 4, want2.pos]);
  check(
    'cap face pixel at the mid-plane is rendered, not a hole',
    capProbe[3] >= 60,
    `RGBA ${capProbe.join(',')} at (30, 4, ${want2.pos})`,
  );

  // ---- (e) flip preserves the plane position ------------------------------------------------
  const uniA = await uniforms();
  const preDir = (await uiClip()).dir;
  await page.locator('[data-testid="clip-flip"]').click();
  await page.waitForFunction((d) => window.fieldsmith.ui.clip.dir === -d, preDir, { timeout: 5000 });
  const uniB = await uniforms();
  // Plane world position on the axis: D · Nₐ (+1 → D, −1 → −D) — sign convention cancels.
  const posA = uniA.d * uniA.n[[0, 1, 2].reduce((acc, ai, i) => (Math.abs(uniA.n[i]) === 1 ? ai : acc), 0)];
  const posB = uniB.d * uniB.n[[0, 1, 2].reduce((acc, ai, i) => (Math.abs(uniB.n[i]) === 1 ? ai : acc), 0)];
  check(
    'flip keeps the plane at the same world position',
    Math.abs(posA - posB) < 1e-6,
    `pos before ${posA?.toFixed(4)} mm, after ${posB?.toFixed(4)} mm (n ${JSON.stringify(uniA.n)} → ${JSON.stringify(uniB.n)})`,
  );
  await stash('__capFlip');
  const dFlip = await diffCaps('__capA35Cut', '__capFlip');
  check('flip changes the rendered frame', dFlip.inChanged >= 50, `${dFlip.inChanged} px changed in bounds`);

  // Flip back for the axis-switch leg (same plane position, restored kept half).
  await page.locator('[data-testid="clip-flip"]').click();
  await page.waitForFunction((d) => window.fieldsmith.ui.clip.dir === -d, -preDir, { timeout: 5000 });
  const uni3 = await uniforms();
  const want3 = await expectedPlane();
  assertPlane(uni3, want3);

  // ---- (f) axis switch ---------------------------------------------------------------------
  await page.locator('[data-testid="clip-axis-x"]').click();
  await page.waitForFunction(() => window.fieldsmith.ui.clip.axis === 0, null, { timeout: 5000 });
  const uniX = await uniforms();
  const wantX = await expectedPlane();
  assertPlane(uniX, wantX);
  await stash('__capAX');
  const dAx = await diffCaps('__capA35Cut', '__capAX');
  check('axis switch changes the rendered frame', dAx.inChanged >= 50, `${dAx.inChanged} px changed in bounds`);

  // ---- (g) results mesh rides the same plane ------------------------------------------------
  // The clip is on (axis x, dir −1, t = 0.5). Switching to the results lens launches the
  // ladder (wanted() includes it); its mesh overlay then picks the live clip plane up.
  // Expected three.js plane: normal −e_x, constant −D = pos = hi − 0.5·span.
  await page.locator('[data-lens="results"]').click();
  await page.waitForFunction(() => window.fieldsmith.analysis.state.levels.length >= 1, null, { timeout: 60000 });
  await page.waitForSelector('[data-testid="results-panel"]', { timeout: 10000 });
  await page.waitForFunction(() => window.__walkOverlays().withPlane > 0, null, { timeout: 15000 });
  await stash('__capResOn');
  const resOn = await overlayWalk();
  const wantG = await expectedPlane();
  const p0 = resOn.planes?.[0] ?? null;
  check(
    `results mesh materials carry the clip plane (−e_x, constant ${(-wantG.d).toFixed(2)}) while on`,
    resOn.withPlane > 0 && p0 !== null && Math.abs(p0[0] + 1) < 1e-3 && Math.abs(p0[3] - (-wantG.d)) < 1e-3,
    `${resOn.withPlane}/${resOn.meshMats} materials carry ${JSON.stringify(resOn.planes)}`,
  );
  await waitClipOff(page);
  await page.waitForFunction(() => window.__walkOverlays().withPlane === 0, null, { timeout: 15000 });
  await stash('__capResOff');
  const resOff = await overlayWalk();
  check(
    'results mesh materials drop the clip plane when off',
    resOff.meshMats > 0 && resOff.withPlane === 0,
    `${resOff.withPlane}/${resOff.meshMats} materials carry a plane`,
  );
  const dRes = await diffCaps('__capResOn', '__capResOff');
  check('results frame changes with the clip', dRes.inChanged >= 50, `${dRes.inChanged} px changed in bounds`);
  check(
    'the analysed overlay has a colour mesh to clip',
    resOn.meshMats > 0,
    `${resOn.meshMats} overlay materials`,
  );

  // ---- (h) session-only --------------------------------------------------------------------
  await page.reload();
  await page.waitForFunction(() => window.fieldsmith?.store?.doc?.root?.length > 0, null, { timeout: 30000 });
  const afterReload = await page.evaluate(() => ({
    on: window.fieldsmith.ui.clip.on,
    t: window.fieldsmith.ui.clip.t,
    alpha: window.fieldsmith.ui.bodyAlpha,
  }));
  check(
    'clip resets on reload (session-only)',
    afterReload.on === false && afterReload.t === 0,
    `on ${afterReload.on}, t ${afterReload.t}, alpha ${afterReload.alpha}`,
  );

  await page.screenshot({ path: resolve(screens, 'section-01.png') });
  await page.close();

  // ---- report ---------------------------------------------------------------------------------
  const ok = checks.every((c) => c.pass) && errors.length === 0;
  const lines = [
    '# V26 — Section-plane clip',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    '## Frame-level checks',
    '',
    '| Check | Result | Measured |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail} |`),
    '',
    'Method: the filled bracket is loaded through the real store API; captures are full-canvas RGBA readbacks of live',
    'frames (`vp.render()` then `gl.readPixels`, `preserveDrawingBuffer` retains content). Diff statistics count pixels',
    'inside the projected bounds whose RGB differs by > 12/255; alpha thresholds separate opaque part pixels (α ≥ 200)',
    'from cleared background/floor (α ≤ 40). Uniform assertions read `u_clipOn`/`u_clipN`/`u_clipD` off the live material',
    'and compare against the plane derived from the ui state and the field bounds. The (d) leg measures the luminance',
    'spread of the changed region — a single-pixel colour probe cannot discriminate the cap from an intact face at the',
    'same normal, so interior exposure is asserted statistically; the cap is additionally probed for opacity (no hole).',
    '',
    'Plane convention: keep dot(p, N) − D ≥ 0. dir=+1 keeps the +axis half (N = +e_axis, D = lo + t·span); dir=−1 keeps',
    'the −axis half (N = −e_axis, D = −(hi − t·span)). t = 0 restores the intact part in both directions; the',
    'camera-relative direction is picked automatically on toggle so the cut opens toward the viewer. At t = 0 the',
    'viewport routes the plane inert (u_clipOn = 0): the plane coincides with a field bound face, and a plane on the face',
    'can shave the march interval by an ulp (plane crossing vs box exit come from different arithmetic) and flicker the',
    'silhouette edges — the null clip is the only bit-identical rest encoding.',
    '',
    `Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
    '',
    'Screenshot: web/test-results/section-01.png.',
    '',
    'Operator checks (not CI): drag the section slider on a filled document with the body at 35 % opacity and confirm the',
    'cap face renders the lattice in place (visual inspection in a headed run).',
    '',
    '---',
    '*Generated by `npm run test:e2e` (web/tests/section.mjs) — do not edit by hand.*',
    '',
  ];
  writeFileSync(resolve(docsDir, 'V26.md'), lines.join('\n'));
  return ok;
}

/** Clip off: click the toggle only when it is currently on (the slider alone never turns off). */
async function waitClipOff(page) {
  const on = await page.evaluate(() => window.fieldsmith.ui.clip.on);
  if (on) {
    await page.locator('[data-testid="clip-toggle"]').click();
  }
  await page.waitForFunction(() => window.fieldsmith.viewport.material?.uniforms.u_clipOn.value !== 1, null, { timeout: 5000 });
}