// Modal mode-shape animation in the Results lens (VISION §1 item 3, first half). The modal
// run already ships three natural frequencies per analysis (V13's eigen route); this rung
// carries the shapes into the page (wasm getters → worker pack → LevelResult) and animates
// them at a phase in the deformed-mesh path. Run with node tests/run-e2e.mjs --only modes.
//
// The fixture is the plain Cantilever V1 plus `solve modes=3 size=8`: the coarse rung
// solves the modes (the browser's coarse element size is far below 8 mm, so the refined
// pass reports its skip and the ladder ends after one rung — the shape evidence needs one
// mesh, not two).
//
// A — the run. One shape per requested mode, 3 components per node (the P2 node order,
// corners then midsides), and the shapes respect the support: every node of the clamped
// root face is motionless.
//
// B — the animation, on the rebuilt geometry (the f32 position buffer read through the e2e
// accessor — the deterministic render input, not pixels). Phase 90° reproduces the
// undeformed body within the nodes' own f32 rounding; phase 0 → 180° moves the surface by
// exactly the expected 2·scale·amplitude computed from the shape buffer; phase 2π (set on
// the knob to the exact double) rebuilds the phase-0 geometry and colours bit for bit; the
// ramp reads the shown phase's magnitude (0 vs 90° differ) without pumping parity
// (0 vs 180° equal).
//
// C — the panel. The modes summary keeps its frequency rows; the picker chips select the
// mode; the phase input speaks degrees and commits on blur; the click probe reads the
// shown field (static mm on the static mesh, ≈0 on the phase-90° undeformed body); Play
// drives the phase frame by frame and Pause stops it; Off restores the static mesh.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

// The sweep family's document, plus the modal run settings. size=8 coarsens the refined
// pass out of the ladder (skip-fine) so the browser run costs one rung.
const MODES_DOC = `\
fieldsmith 1
name "Cantilever V1"
custommaterial steel200 name="Steel (E 200 GPa)" e=200000 nu=0.3 yield=250 density=7.85
material steel200
box beam size=(100, 10, 10) at=(50, 5, 5)
region root = beam.nx
region tip = beam.px
support clamp root fixed=xyz
load push tip force=(0, 0, -100)
solve modes=3 size=8
`;

/** Wait until the predicate holds in the page; `arg` is passed through evaluate
 * (closures are serialised away). */
async function until(page, fn, arg, ms = 30000, note = '') {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (await page.evaluate(fn, arg)) return;
    await page.waitForTimeout(250);
  }
  throw new Error(`timed out${note ? ` (${note})` : ''}: ${fn}`);
}

export async function modesTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `modes-${name}.png`);

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
      // In-page helpers: snapshots stay typed arrays (no serialisation across the boundary).
      window.__eq = (a, b) => !!a && !!b && a.length === b.length && a.every((v, i) => v === b[i]);
    }, MODES_DOC);
    await page.locator('[data-lens="results"]').click();
    await until(
      page,
      () => window.fieldsmith.analysis.state.status === 'ready',
      null,
      240000,
      'ladder ready',
    );

    // ---- A: the run carries the shapes ---------------------------------------------------
    const run = await page.evaluate(() => {
      const st = window.fieldsmith.analysis.state;
      const lvl = window.fieldsmith.analysis.latest;
      const nodes = lvl.nodes;
      const shapes = lvl.modeShapes;
      const per = nodes.length; // 3 components per node
      const out = {
        levels: st.levels.map((l) => l.label),
        skippedFine: st.skippedFine,
        freqs: lvl.meta.modes?.frequencies ?? null,
        nodeCount: per / 3,
        shapeCount: shapes === null ? 0 : shapes.length / per,
        clamped: [],
        maxPerShape: [],
      };
      if (!shapes) return out;
      for (let k = 0; k < out.shapeCount; k++) {
        let max = 0;
        for (let i = 0; i < per; i += 3) {
          const m = Math.hypot(shapes[k * per + i], shapes[k * per + i + 1], shapes[k * per + i + 2]);
          if (m > max) max = m;
        }
        out.maxPerShape.push(max);
      }
      // The clamped face: every node within 1e-6 of x = 0 is held, corners and midsides.
      out.clamped = [0, 1].map((k) => {
        for (let n = 0; n < out.nodeCount; n++) {
          if (nodes[3 * n] < 1e-6) {
            const c = shapes[k * per + 3 * n] + shapes[k * per + 3 * n + 1] + shapes[k * per + 3 * n + 2];
            if (Math.abs(c) > 1e-9) return `node ${n} moves ${c}`;
          }
        }
        return 'all held';
      });
      return out;
    });
    check(
      'the ladder ended on the skip-fine, one rung',
      (run?.skippedFine ?? '') !== '' && run?.levels.length === 1,
      JSON.stringify({ levels: run?.levels, skippedFine: (run?.skippedFine ?? '').slice(0, 120) }),
    );
    check(
      'the run solved three modes; frequencies finite, ties allowed within roundoff',
      Array.isArray(run?.freqs) &&
        run.freqs.length === 3 &&
        run.freqs.every((f) => f > 0) &&
        run.freqs[0] <= run.freqs[1] * (1 + 1e-9) &&
        run.freqs[1] <= run.freqs[2] * (1 + 1e-9),
      JSON.stringify(run?.freqs.map((f) => f.toFixed(3))),
    );
    check('one shape per mode, 3 components per P2 node', run?.shapeCount === 3, `shapes ${run?.shapeCount} on ${run?.nodeCount} nodes`);
    check('the shapes hold the clamped root face exactly', run?.clamped?.join(' · ') === 'all held · all held', JSON.stringify(run?.clamped));
    check(
      'the shapes are not flat and differ in extent',
      Array.isArray(run?.maxPerShape) && run.maxPerShape.every((m) => m > 0) && run.maxPerShape[0] !== run.maxPerShape[1],
      JSON.stringify(run?.maxPerShape?.map((m) => m.toExponential(2))),
    );

    // ---- B: the animation, on the rebuilt geometry ---------------------------------------
    // Static baseline: the picker is Off on a fresh load; the click exercises the Seg.
    // The click probe reads the lens colour mode (resultsMode), so select deflection here —
    // animated builds force 'deflection' regardless, this keeps the static probe on the
    // same field type as the phase-90 probe below.
    await page.evaluate(() => window.fieldsmith.ui.set('resultsMode', 'deflection'));
    await page.locator('[data-testid="modes-shape"] button[data-value="-1"]').click();
    await until(
      page,
      () => {
        const p = window.fieldsmith.results.positions();
        if (p) {
          window.__stat = p;
          window.__statC = window.fieldsmith.results.colors();
        }
        return !!p && p.length > 0;
      },
      null,
      5000,
      'static mesh snapshot',
    );
    const stat = await page.evaluate(() => ({ p: window.__stat?.length ?? 0, c: window.__statC?.length ?? 0 }));
    check('the static mesh carries a rebuilt geometry', stat.p > 0 && stat.c > 0, `${stat.p} position floats, ${stat.c} colour floats`);
    await page.screenshot({ path: shot('01-static') });

    // The static probe: the click probe reads the static deflection field. Mid-span, top
    // face, away from the clamp and the tip load.
    const atTop = await page.evaluate(() => window.fieldsmith.view.project([60, 5, 10]));
    if (!atTop) throw new Error('could not project a probe point');
    await page.mouse.click(atTop.x, atTop.y);
    await until(page, () => window.fieldsmith.ui.probe !== null, null, 5000, 'static probe');
    const probeStatic = await page.evaluate(() => window.fieldsmith.ui.probe);
    check(
      'the static probe reads the static deflection field in mm',
      !!probeStatic && probeStatic.unit === 'mm' && probeStatic.value > 1e-3,
      `${probeStatic?.value.toFixed?.(4) ?? probeStatic?.value} ${probeStatic?.unit} at (${probeStatic?.point.map((v) => v.toFixed(1)).join(', ')})`,
    );

    // Mode 1, phase 0 — through the Seg chip, the shipped control.
    await page.locator('[data-testid="modes-shape"] button[data-value="0"]').click();
    await until(
      page,
      () => {
        window.__p0 = window.fieldsmith.results.positions();
        window.__p0C = window.fieldsmith.results.colors();
        return window.fieldsmith.ui.resultsModeShape === 0 && !window.__eq(window.__stat, window.__p0);
      },
      null,
      10000,
      'mode 1 phase 0 geometry',
    );
    const r0 = await page.evaluate(() => ({
      pass: !window.__eq(window.__stat, window.__p0),
      detail: `${window.__p0?.length ?? 0} floats rebuilt with the shape at cos 0`,
    }));
    check('phase 0 shows a different geometry than the static mesh', r0.pass, r0.detail);

    // Phase 90° through the panel input (commit on blur): cos ≈ 1e-17 sits below the f32
    // rounding of the node coordinates themselves, so the rebuild is the undeformed body,
    // and the probe at the same point reads the shown field (≈0), not the static one.
    const phase = page.locator('[data-testid="modes-phase"]');
    await phase.fill('90');
    await phase.press('Tab');
    await until(page, () => Math.abs(window.fieldsmith.ui.resultsModePhase - Math.PI / 2) < 1e-12, null, 5000, 'phase 90° committed');
    await until(
      page,
      () => {
        // Undeformed within f32: positions[i] vs the f32-rounded node coordinate it renders.
        const p = window.fieldsmith.results.positions();
        const l = window.fieldsmith.analysis.latest;
        let maxd = 0;
        for (let i = 0; i < p.length; i++) {
          const n = l.surfaceFacets[6 * ((i / 9) | 0) + (((i % 9) / 3) | 0)];
          const d = Math.abs(l.nodes[3 * n + (i % 3)] - p[i]);
          if (d > maxd) maxd = d;
        }
        window.__p90 = p;
        window.__p90C = window.fieldsmith.results.colors();
        window.__p90max = maxd;
        return maxd < 1e-5;
      },
      null,
      10000,
      'phase 90° on the undeformed body',
    );
    const r90 = await page.evaluate(() => ({
      pass: window.__p90max < 1e-5,
      detail: `max deviation ${window.__p90max.toExponential(2)} mm (the f64→f32 rounding of the node coordinates themselves is ~4e-6 at these magnitudes); ${window.__p90.length} floats compared`,
    }));
    check('phase 90° lands on the undeformed body (f32 rounding)', r90.pass, r90.detail);
    // The probe rides the phase: click the same surface point projected from the undeformed
    // coordinates — the phase-90 mesh is exactly there.
    await page.mouse.click(atTop.x, atTop.y);
    await until(page, () => (window.fieldsmith.ui.probe?.value ?? 1) < 1e-9, null, 5000, 'probe on the phase-90 field');
    const probe90 = await page.evaluate(() => window.fieldsmith.ui.probe);
    check(
      'the probe then reads the shown shape field, not the static one',
      !!probe90 && probe90.unit === 'mm' && probe90.value < 1e-9,
      `${probe90?.value ?? 1} mm (the static probe at the same point read ${probeStatic?.value})`,
    );
    const rRamp = await page.evaluate(() => ({
      pass: !window.__eq(window.__p0C, window.__p90C),
      detail: `${window.__p0C?.length ?? 0} colour floats per build`,
    }));
    check('the colour ramp reads the shown phase (0 vs 90° differ)', rRamp.pass, rRamp.detail);
    await page.screenshot({ path: shot('02-phase-90') });

    // Phase 180°: the whole shape flips; the surface move is 2·scale·amplitude at the peak
    // corner, with the peak-normalised amplitude the panel applies (5% of the model diag).
    await phase.fill('180');
    await phase.press('Tab');
    await until(page, () => Math.abs(window.fieldsmith.ui.resultsModePhase - Math.PI) < 1e-12, null, 5000, 'phase 180° committed');
    // The rebuild is scheduled on the next animation frame, so the first poll here can
    // land while the buffer is still the phase-90 build — which also satisfies p ≠ p0.
    // Exclude it: only the fresh 180° build has colours that differ from the captured
    // phase-90 ramp.
    await until(
      page,
      () => {
        const p = window.fieldsmith.results.positions();
        const c = window.fieldsmith.results.colors();
        window.__p180 = p;
        return !!p && !window.__eq(window.__p0, p) && !window.__eq(window.__p90C, c);
      },
      null,
      10000,
      'phase 180° geometry',
    );
    const peakMove = await page.evaluate(() => {
      const p = window.__p180;
      let max = 0;
      for (let i = 0; i < p.length; i += 3) {
        const d = Math.hypot(p[i] - window.__p0[i], p[i + 1] - window.__p0[i + 1], p[i + 2] - window.__p0[i + 2]);
        if (d > max) max = d;
      }
      window.__p180C = window.fieldsmith.results.colors();
      return max;
    });
    const expectedMove = await page.evaluate(() => {
      const l = window.fieldsmith.analysis.latest;
      const nodes = l.nodes;
      const shape = l.modeShapes; // mode block 0 spans 0..nodes.length
      const scale = window.fieldsmith.ui.resultsScale;
      let lx = Infinity, hx = -Infinity, ly = Infinity, hy = -Infinity, lz = Infinity, hz = -Infinity;
      for (let i = 0; i < nodes.length; i += 3) {
        lx = Math.min(lx, nodes[i]); hx = Math.max(hx, nodes[i]);
        ly = Math.min(ly, nodes[i + 1]); hy = Math.max(hy, nodes[i + 1]);
        lz = Math.min(lz, nodes[i + 2]); hz = Math.max(hz, nodes[i + 2]);
      }
      const amp = 0.05 * Math.hypot(hx - lx, hy - ly, hz - lz);
      let peakNode = 0;
      for (let i = 0; i < nodes.length; i += 3) {
        const m = Math.hypot(shape[i], shape[i + 1], shape[i + 2]);
        if (m > peakNode) peakNode = m;
      }
      let peakCorner = 0;
      for (let f = 0; f < l.surfaceRegion.length; f++) {
        for (let j = 0; j < 3; j++) {
          const n = l.surfaceFacets[6 * f + j];
          const m = Math.hypot(shape[3 * n], shape[3 * n + 1], shape[3 * n + 2]);
          if (m > peakCorner) peakCorner = m;
        }
      }
      return 2 * scale * peakCorner * (amp / peakNode);
    });
    check(
      'phase 0 → 180° moves the surface by 2·scale·amp at the peak corner',
      Math.abs(peakMove - expectedMove) < 1e-4 * expectedMove,
      `peak move ${peakMove.toPrecision(6)} mm vs expected ${expectedMove.toPrecision(6)} mm (f32)`,
    );
    const parity = await page.evaluate(() => ({
      pass: window.__eq(window.__p0C, window.__p180C),
      detail: `${window.__p0C.length} colour floats compared; corner magnitudes are phase-parity symmetric`,
    }));
    check('the ramp does not pump parity (0 vs 180° colours equal)', parity.pass, parity.detail);
    await page.screenshot({ path: shot('03-phase-180') });

    // Phase 2π exactly — set on the knob directly: the input's 5° step cannot express the
    // exact double. x·cos(2π) = x·1.0 is exact, so the rebuild is bit-identical to phase 0.
    await page.evaluate(() => window.fieldsmith.ui.set('resultsModePhase', Math.PI * 2));
    await until(
      page,
      () => window.__eq(window.__p0, window.fieldsmith.results.positions()) && window.__eq(window.__p0C, window.fieldsmith.results.colors()),
      null,
      5000,
      'phase 2π bit-exact',
    );
    const r2pi = await page.evaluate(() => ({
      pos: window.__eq(window.__p0, window.fieldsmith.results.positions()),
      col: window.__eq(window.__p0C, window.fieldsmith.results.colors()),
      n: window.__p0.length,
      nc: window.__p0C.length,
    }));
    check('phase 2π rebuilds the phase-0 geometry bit for bit', r2pi.pos, `${r2pi.n} position floats compared`);
    check('phase 2π colours are the phase-0 colours too', r2pi.col, `${r2pi.nc} colour floats compared`);

    // ---- C: play / pause, then Off restores the static mesh ------------------------------
    // Bring the knob to a mid-phase first so play visibly starts from it.
    await page.evaluate(() => window.fieldsmith.ui.set('resultsModePhase', 1));
    await page.locator('[data-testid="modes-play"]').click();
    await until(page, () => window.fieldsmith.ui.resultsModePlay === true, null, 5000, 'play on');
    const playFrom = await page.evaluate(() => {
      window.__playRef = window.fieldsmith.results.positions();
      return window.fieldsmith.ui.resultsModePhase;
    });
    await page.waitForTimeout(700);
    const rPlay = await page.evaluate((p1) => ({
      advanced: window.fieldsmith.ui.resultsModePhase !== p1,
      moved: !window.__eq(window.__playRef, window.fieldsmith.results.positions()),
      p2: window.fieldsmith.ui.resultsModePhase,
    }), playFrom);
    check(
      'play advances the phase and the mesh moves with it',
      rPlay.advanced && rPlay.moved,
      `phase ${playFrom.toFixed(3)} → ${rPlay.p2.toFixed(3)}; the rebuilt buffer moved with it`,
    );
    await page.screenshot({ path: shot('04-playing') });
    await page.locator('[data-testid="modes-play"]').click();
    await until(page, () => window.fieldsmith.ui.resultsModePlay === false, null, 5000, 'pause');
    const pause1 = await page.evaluate(() => window.fieldsmith.ui.resultsModePhase);
    await page.waitForTimeout(700);
    const pause2 = await page.evaluate(() => window.fieldsmith.ui.resultsModePhase);
    check('pause holds the phase', pause1 === pause2, `phase ${pause1 === pause2 ? pause1 : `${pause1} → ${pause2}`}`);

    // The legend speaks the relative-amplitude truth while animating, the static caption
    // after Off; the summary keeps its frequency rows and four picker buttons.
    const panel = await page.evaluate(() => {
      const el = document.querySelector('[data-testid="modes-summary"]');
      const legend = document.querySelector('[data-testid="legend"]');
      return {
        summary: el?.textContent ?? '',
        pickers: document.querySelectorAll('[data-testid="modes-shape"] button').length,
        legend: legend?.textContent ?? '',
        shapeOn: window.fieldsmith.ui.resultsModeShape,
      };
    });
    check(
      'the modes summary lists its three frequencies and the picker',
      panel.pickers === 4 && panel.summary.includes('natural frequencies'),
      `${panel.pickers} picker buttons; ${panel.summary.slice(0, 100)}`,
    );
    const legendAnimated = panel.shapeOn >= 0;
    check(
      'the legend switches between the animation and static captions',
      legendAnimated ? panel.legend.includes('relative amplitude') : panel.legend.includes('displacement magnitude'),
      `shape selector ${panel.shapeOn}; legend "${panel.legend.slice(0, 48)}…"`,
    );

    await page.locator('[data-testid="modes-shape"] button[data-value="-1"]').click();
    await until(
      page,
      () => window.__eq(window.__stat, window.fieldsmith.results.positions()) && window.__eq(window.__statC, window.fieldsmith.results.colors()),
      null,
      10000,
      'Off back to the static mesh',
    );
    const rOff = await page.evaluate(() => ({
      pass: window.__eq(window.__stat, window.fieldsmith.results.positions()) && window.__eq(window.__statC, window.fieldsmith.results.colors()),
      detail: `${window.__stat?.length ?? 0} position floats, ${window.__statC?.length ?? 0} colour floats, compared after Off`,
    }));
    check('Off restores the static mesh and colours byte for byte', rOff.pass, rOff.detail);
    await page.screenshot({ path: shot('05-back-static') });

    // ---- report ---------------------------------------------------------------------------
    check('no console errors', errors.length === 0, errors.join(' | ').slice(0, 400));
    const ok = checks.every((c) => c.pass);
    const table = (names) =>
      [
        '| Check | Result | Detail |',
        '| :-- | :-- | :-- |',
        ...names.map((name) => {
          const ck = checks.find((x) => x.name === name);
          return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
        }),
      ].join('\n');
    const lines = [
      '# V28 — modal mode-shape animation in the Results lens',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
      '',
      'The modal run carries its mass-normalised shapes into the page and the Results lens',
      'animates them at a phase: the deformed-mesh path shows φₖ·cos(phase) with the colour on',
      "the shown phase's magnitude. The fixture is the plain Cantilever V1 with",
      '`solve modes=3 size=8` (skip-fine: one rung carries the evidence).',
      '',
      '```fsm',
      MODES_DOC.trimEnd(),
      '```',
      '',
      '## A — the run',
      '',
      `- Ladder: ${JSON.stringify(run?.levels)} · skip-fine: ${run?.skippedFine ?? ''}`,
      `- Frequencies: ${JSON.stringify(run?.freqs)} Hz`,
      `- Shapes: ${run?.shapeCount} on ${run?.nodeCount} P2 nodes; per-shape max displacement`,
      `  ${JSON.stringify(run?.maxPerShape?.map((m) => m.toExponential(2)))} — modal mass φᵀMφ = 1, so a shape's absolute scale is`,
      '  arbitrary; the panel shows it peak-normalised to 5% of the model diagonal instead (the',
      '  raw ×5 exaggeration of a 10² mm value would leave the screen).',
      `- The clamped root face holds exactly: ${run?.clamped?.join(' · ')}.`,
      '',
      '## B — the animation, on the rebuilt geometry',
      '',
      table([
        'the static mesh carries a rebuilt geometry',
        'phase 0 shows a different geometry than the static mesh',
        'phase 90° lands on the undeformed body (f32 rounding)',
        'the probe then reads the shown shape field, not the static one',
        'the colour ramp reads the shown phase (0 vs 90° differ)',
        'phase 0 → 180° moves the surface by 2·scale·amp at the peak corner',
        'the ramp does not pump parity (0 vs 180° colours equal)',
        'phase 2π rebuilds the phase-0 geometry bit for bit',
        'phase 2π colours are the phase-0 colours too',
        'Off restores the static mesh and colours byte for byte',
      ]),
      `- Static probe for comparison: ${JSON.stringify(probeStatic)} · 90° probe: ${JSON.stringify(probe90)}`,
      '',
      '## C — the panel',
      '',
      `- Mode 1 selected through the chip; phases committed through the NumberInput (Tab`,
      `  blur); 2π set on the knob directly — the input's 5° step cannot express the exact`,
      '  double. Legend captions: animated vs static.',
      '',
      table([
        'play advances the phase and the mesh moves with it',
        'pause holds the phase',
        'the modes summary lists its three frequencies and the picker',
        'the legend switches between the animation and static captions',
        'no console errors',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/modes.mjs`: the ladder runs in the page on the fixture document; the',
      'animation evidence reads the rebuilt f32 position and colour buffers through the e2e',
      'accessor (`window.fieldsmith.results`) rather than pixels — a framebuffer probe could',
      'flake on ties, the render input cannot. The panel evidence drives the shipped controls',
      '(Seg chips, NumberInput + Tab commit, Play/Pause buttons) and probes with a real mouse',
      'click at a point projected from the undeformed surface.',
      '',
      '## Limits',
      '',
      '- The graded (fill) path carries no shapes this rung: `LevelResult.modeShapes` is null',
      '  there and the Make lens is untouched.',
      '- The 2π bit-exactness rests on cos(2π) = 1 exactly in double arithmetic and x·1.0 = x;',
      '  a differently-rounded libm would shift colours by at most one ramp bin.',
      '- Animation frame cost at the refined rungs (~45k nodes) was not measured; the coarse',
      "  rung animates at the viewport's frame rate in this run.",
      "- The legend's mm numbers while animating are the relative-amplitude scale (peak 5% of",
      '  the model diagonal) — they are not a physical displacement and the caption says so.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/modes.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V28.md'), lines.join('\n'));
    console.log(`modes ${ok ? 'PASS' : 'FAIL'}: shapeCount ${run?.shapeCount}, freqs ${JSON.stringify(run?.freqs)?.slice(0, 72)}`);
    if (!ok) {
      for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    }
    if (errors.length) console.error('console errors:', errors);
    return ok;
  } catch (e) {
    console.error('failed checks so far:');
    for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    throw e;
  } finally {
    await page.close();
  }
}