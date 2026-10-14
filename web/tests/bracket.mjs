// The bracket test (GOAL.md), driven through the real UI in a headless browser.
//
// Modeling, setup and results all go through what a person would use: buttons, menus,
// the inspector and the keyboard. Hole-wall regions are chosen by projected, verified
// clicks (each candidate is checked with pickAt before the click, so a wrong pick is
// impossible). The run records per-step timings and screenshots and fails on any
// console error, and measures edit→coarse / edit→refined against the GOAL budgets.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

async function setNum(page, testId, value) {
  const el = page.locator(`[data-testid="${testId}"]`);
  await el.click();
  await el.fill(String(value));
  await el.press('Enter');
}

async function setVec(page, testId, v) {
  for (const [i, a] of ['x', 'y', 'z'].entries()) await setNum(page, `${testId}-${a}`, v[i]);
}

async function rename(page, name) {
  const el = page.locator('[data-testid="name"]');
  await el.click();
  await el.fill(name);
  await el.press('Enter');
}

async function addShape(page, type) {
  await page.locator('[data-action="add-menu"]').click();
  await page.locator(`[data-testid="add-${type}"]`).click();
}

async function setOp(page, op) {
  await page.locator(`[data-testid="op"] button[data-value="${op}"]`).click();
}

export async function bracketTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const steps = [];
  let editAt = null;
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const t0 = Date.now();
  let last = t0;
  const step = async (name, fn) => {
    await fn();
    await page.waitForTimeout(150);
    const now = Date.now();
    const file = `bracket-${String(steps.length + 1).padStart(2, '0')}.png`;
    await page.screenshot({ path: resolve(screens, file) });
    steps.push({ name, ms: now - last, file });
    last = now;
  };

  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('[data-testid="empty-state"]');

  await step('Empty document', async () => {});
  await step('Base plate 60 × 40 × 8', async () => {
    await page.locator('[data-testid="empty-add-box"]').click();
    await rename(page, 'base');
    await setVec(page, 'size', [60, 40, 8]);
    await setVec(page, 'position', [30, 20, 4]);
  });
  await step('Upright plate with 8 mm fillet', async () => {
    await addShape(page, 'box');
    await rename(page, 'upright');
    await setVec(page, 'size', [60, 8, 40]);
    await setVec(page, 'position', [30, 4, 20]);
    await setNum(page, 'blend', 8);
  });
  for (const [name, x] of [
    ['hole1', 15],
    ['hole2', 45],
  ]) {
    await step(`Base hole ${name} ⌀6.5`, async () => {
      await addShape(page, 'cylinder');
      await rename(page, name);
      await setOp(page, 'subtract');
      await setNum(page, 'diameter', 6.5);
      await setNum(page, 'height', 20);
      await setVec(page, 'position', [x, 26, 4]);
    });
  }
  await step('Upright hole ⌀8.5', async () => {
    await addShape(page, 'cylinder');
    await rename(page, 'hole3');
    await setOp(page, 'subtract');
    await setNum(page, 'diameter', 8.5);
    await setNum(page, 'height', 20);
    await setVec(page, 'rotation', [90, 0, 0]);
    await setVec(page, 'position', [30, 4, 28]);
  });

  // The modelled shapes must equal the reference document's shapes exactly.
  const reference = readFileSync(resolve(here, '../../examples/bracket.fsm'), 'utf8');
  const cmp = await page.evaluate(async (ref) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(ref);
    // Compare in canonical form (round-trip through the Rust serialiser).
    const text = w.printFsm(window.fieldsmith.store.doc);
    const got = w.parseFsm(text).ok.root;
    return { equal: JSON.stringify(got) === JSON.stringify(r.ok.root), got: text };
  }, reference);

  // ---- study setup ---------------------------------------------------------------------

  await step('Setup lens', async () => {
    await page.locator('[data-lens="setup"]').click();
    await page.waitForSelector('[data-testid="add-support"]', { timeout: 5000 });
    // A person would orbit the view to look down at the base plate from the +y side,
    // which is the only side from which the base-hole walls are visible (the upright
    // plate blocks them from the default camera). Pick the +y,+z direction.
    await page.evaluate(() => {
      window.fieldsmith.view.view([0, 1, 1]);
      window.fieldsmith.view.fit(false);
    });
    // With no support or load in the study yet, the status line shows the neutral setup
    // hint (typed 'setup', not the red 'analysis failed').
    await page.waitForSelector('[data-testid="setup-block"]', { timeout: 5000 });
    if (await page.locator('[data-testid="badge-error"]').count()) {
      throw new Error('an incomplete study rendered the red analysis-failed badge');
    }
  });

  // Candidates for the visible hole walls, derived from the +y,+z camera above (rays
  // travel about (0,-0.71,-0.71)): a vertical hole's wall is sighted around azimuth
  // 270° on the hole's -y side at z ≈ 3..5 (rays clear the z=8 mouth when 4.75 - z ≤
  // 3.25, so z ≥ 1.5), and the upright_hole's wall around 270° in the xz plane at
  // y ≈ 1.5..4.5. Each candidate is verified with pickAt before the click, so unseen
  // points are skipped and a wrong pick cannot write the document — the test fails
  // loudly instead.
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

  await step('Region: base-hole walls', async () => {
    await clickSurface('hole1', 'side', vertWall(15, 26));
    await clickSurface('hole2', 'side', vertWall(45, 26));
    await rename(page, 'base_holes');
  });
  await step('Region: upright-hole wall', async () => {
    await page.keyboard.press('Escape');
    await clickSurface('hole3', 'side', endWall(30, 28));
    await rename(page, 'upright_hole');
  });
  await step('Support on the base_holes', async () => {
    await page.locator('[data-testid="region-base_holes"]').click();
    await page.locator('[data-testid="add-support"]').click();
  });
  await step('Load on the upright_hole', async () => {
    await page.locator('[data-testid="region-upright_hole"]').click();
    await page.locator('[data-testid="add-load"]').click();
    // 400 N pointing down and 15° outward: down -z = 400·cos15°, outward -y = 400·sin15°.
    await setVec(page, 'force', [0, -103.53, -386.37]);
    editAt = await page.evaluate(() => performance.now());
  });

  // ---- solve ---------------------------------------------------------------------------

  const runHas = async (fn, timeoutMs) => {
    try {
      await page.waitForFunction(fn, null, { timeout: timeoutMs, polling: 50 });
      return true;
    } catch {
      return false;
    }
  };

  const coarseSeen = await runHas(() => window.fieldsmith.analysis.state.levels.length >= 1, 20000);
  await step('Coarse result streamed', async () => {});
  await runHas(
    () => ['ready', 'error'].includes(window.fieldsmith.analysis.state.status),
    60000,
  );
  await step('Refined solve finished', async () => {});

  const res = await page.evaluate(() => {
    const F = window.fieldsmith;
    const a = F.analysis.state;
    const lvl = a.levels[a.levels.length - 1];
    return {
      status: a.status,
      error: a.error,
      skippedFine: a.skippedFine,
      levelArrived: a.levelArrived,
      startedMs: a.startedMs,
      labels: a.levels.map((l) => l.label),
      levels: a.levels.map((l) => ({
        label: l.label,
        p: l.meta.provenance,
        peak: Math.max(0, ...l.meta.cases.map((c) => c.max_vm_elem)),
        defl: Math.max(0, ...l.meta.cases.map((c) => c.max_displacement)),
      })),
      regions: F.store.doc.regions.map((r) => ({ id: r.id, selectors: r.selectors })),
      supports: F.store.doc.supports.map((s) => ({ id: s.id, region: s.region, fixed: s.fixed })),
      loads: F.store.doc.loads.map((l) => ({ id: l.id, region: l.region, case: l.case, kind: l.kind })),
      material: F.store.doc.material,
    };
  });

  const fine = res.levels[res.levels.length - 1];
  const load = res.loads[0];
  const vm = fine ? fine.peak : NaN;
  const defl = fine ? fine.defl : NaN;
  const edit = editAt ?? NaN;
  const coarseMs = res.levelArrived[0] - edit;
  const refinedMs = (res.levelArrived[1] ?? NaN) - edit;

  check('document material is PETG', res.material === 'petg', res.material);
  check('two regions were created', res.regions.length === 2, JSON.stringify(res.regions.map((r) => r.id)));
  const sels = (name) => res.regions.find((r) => r.id === name)?.selectors ?? [];
  const faces = (name) => sels(name).map((s) => [s.shape, s.face]).sort();
  check(
    'base_holes region covers hole1 + hole2 walls',
    JSON.stringify(faces('base_holes')) === JSON.stringify([['hole1', 'side'], ['hole2', 'side']]),
    JSON.stringify(faces('base_holes')),
  );
  check(
    'upright_hole region covers hole3 wall',
    faces('upright_hole').length === 1 && faces('upright_hole')[0][0] === 'hole3' && faces('upright_hole')[0][1] === 'side',
    JSON.stringify(faces('upright_hole')),
  );
  check(
    'support fixes all three axes on the base_holes',
    res.supports.length === 1 && res.supports[0].region === 'base_holes' && JSON.stringify(res.supports[0].fixed) === '[true,true,true]',
    JSON.stringify(res.supports),
  );
  const mag = load && load.kind.type === 'force' ? Math.hypot(...load.kind.vector) : NaN;
  const ang = load && load.kind.type === 'force' ? (Math.atan2(Math.abs(load.kind.vector[1]), Math.abs(load.kind.vector[2])) * 180) / Math.PI : NaN;
  check(
    'force on the upright_hole, 400 N ±0.5',
    load?.kind.type === 'force' && load.region === 'upright_hole' && Math.abs(mag - 400) <= 0.5,
    `${mag.toFixed(2)} N`,
  );
  check(
    'force points down and 15° outward ±0.2° (y negative, z negative)',
    load?.kind.type === 'force' && load.kind.vector[1] < 0 && load.kind.vector[2] < 0 && Math.abs(ang - 15) <= 0.2,
    `${ang.toFixed(2)}° from vertical, y ${load?.kind.vector[1]}, z ${load?.kind.vector[2]}`,
  );
  check('solve reached ready with coarse and refined levels', coarseSeen && res.status === 'ready' && res.labels.length === 2, `${res.status}; labels ${JSON.stringify(res.labels)}; ${coarseSeen ? '' : 'no coarse in 20 s'}`);
  if (res.skippedFine) check('refined level was not skipped', false, res.skippedFine);
  check('peak von Mises 20–100 MPa', vm >= 20 && vm <= 100, `${vm.toFixed(1)} MPa`);
  check('max deflection 0.01–3 mm', defl >= 0.01 && defl <= 3, `${defl.toFixed(3)} mm`);
  check('edit→coarse ≤ 1500 ms (GOAL budget)', coarseMs >= 0 && coarseMs <= 1500, `${Math.round(coarseMs)} ms measured in-page`);
  check('edit→refined ≤ 10 s (GOAL budget)', refinedMs >= 0 && refinedMs <= 10000, `${Math.round(refinedMs)} ms measured in-page`);
  check('element order P2', fine?.p?.order === 2, `order ${fine?.p?.order}`);

  // ---- results lens ---------------------------------------------------------------------

  let panelSeen = false;
  let probe = null;
  await step('Results lens: stress field', async () => {
    await page.locator('[data-lens="results"]').click();
    await page.waitForSelector('[data-testid="results-panel"]', { timeout: 5000 });
    await page.waitForSelector('[data-testid="analysis-badge"]', { timeout: 5000 });
    await page.waitForSelector('[data-testid="legend"]', { timeout: 5000 });
    panelSeen = true;
  });
  const hintBefore = await page.locator('[data-testid="probe-hint"]').count();
  check('probe hint names the surface click before any probe', hintBefore === 1, `${hintBefore} hint elements`);
  await step('Probe the base plate', async () => {
    // A point on the top face of the base plate, away from the fillet and the holes.
    const at = await page.evaluate(() => {
      const s = window.fieldsmith.view.project([30, 20, 8]);
      return s ? { x: s.x, y: s.y } : null;
    });
    if (!at) throw new Error('could not project a probe point');
    await page.mouse.click(at.x, at.y);
    await page.waitForSelector('[data-testid="probe"]', { timeout: 5000 });
    probe = await page.evaluate(() => window.fieldsmith.ui.probe);
    check(
      'probe returns a stress in MPa on the base plate',
      !!probe && probe.unit === 'MPa' && probe.value > 0,
      probe ? `${probe.value.toFixed(2)} MPa at (${probe.point.map((v) => v.toFixed(1)).join(', ')}) mm` : 'no probe reading',
    );
  });
  const hintAfter = await page.locator('[data-testid="probe-hint"]').count();
  check('probe hint replaced by the probe reading', hintAfter === 0, `${hintAfter} hint elements after probing`);
  check('results panel, badge and legend render', panelSeen, panelSeen ? '' : 'panel, badge or legend missing after switching to the results lens');
  const badgeOk = (await page.locator('[data-testid="analysis-badge"]').count()) === 1;
  check('provenance badge shows the refined level', badgeOk, 'analysis-badge present once');
  // M13 rung records in the UI: both levels listed with a wall time, and the rung-1
  // change labelled an estimate (V17's small e2e assert).
  const rung = await page.evaluate(() => {
    const el = document.querySelector('[data-testid=rung-table]');
    return el ? el.textContent : null;
  });
  check(
    'rung table lists both levels and labels the change an estimate',
    !!rung && /rung 0/.test(rung) && /rung 1/.test(rung) && /wall \d+ ms/.test(rung) && /\(estimate\):/.test(rung),
    rung ? rung.slice(0, 240) : 'rung table missing',
  );
  // The Show seg must fit the panel: flex chips cannot shrink below their text, so a
  // fifth mode or a narrower panel would silently crop the last chip (the Vectors chip
  // read as "Ve" before the 2026-10-13 widening).
  const segFit = await page.evaluate(() => {
    const seg = document.querySelector('[data-testid="results-mode"]');
    const panel = document.querySelector('.results-panel');
    if (!seg || !panel) return { ok: false, detail: 'seg or panel missing' };
    const pb = panel.getBoundingClientRect();
    const sb = seg.getBoundingClientRect();
    const inside = [...seg.querySelectorAll('button')].every((b) => b.getBoundingClientRect().right <= pb.right + 0.5);
    return {
      ok: seg.scrollWidth <= seg.clientWidth + 0.5 && inside,
      detail: `seg ${seg.clientWidth}px holds ${seg.scrollWidth}px of chips, panel ${Math.round(pb.width)}px wide`,
    };
  });
  check('Show seg fits the results panel (no cropped chip)', segFit.ok, segFit.detail);

  // Legend range: the ramped modes' min/max are editable. Editing stores the pair against
  // this mode + rung and marks the saturation; a mode switch must fall back to auto bounds
  // rather than miscolour deflection with a stress range (the level key includes the doc
  // version, so a new solve likewise drops it). The Reset button exists to get back to the
  // auto pair without a reload or a re-solve.
  let stressAutoHi = 0;
  await step('Set the legend upper bound to 1 MPa', async () => {
    stressAutoHi = parseFloat(await page.locator('[data-testid=legend-hi]').inputValue());
    await setNum(page, 'legend-hi', '1');
    const r = await page.evaluate(() => {
      const u = window.fieldsmith.ui;
      const a = window.fieldsmith.analysis.state;
      const label = a.levels[a.levels.length - 1].label;
      return { range: u.resultsRange, label, cap: document.querySelector('[data-testid=legend] .meta')?.textContent ?? '' };
    });
    check(
      'legend range stored against this mode and rung',
      !!r.range && r.range.mode === 'stress' && r.range.lo === 0 && r.range.hi === 1 && r.range.levelKey.endsWith(`:${r.label}`),
      JSON.stringify(r.range),
    );
    check('legend caption notes the saturation', /values outside the range/.test(r.cap), r.cap);
  });
  await step('Switch to deflection: range falls back to auto bounds', async () => {
    const autoHi = await page.evaluate(() => {
      const a = window.fieldsmith.analysis.state;
      const lvl = a.levels[a.levels.length - 1];
      return Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement));
    });
    await page.locator('[data-testid="results-mode"] button', { hasText: 'Deflection' }).click();
    // Wait for the deflection legend to settle: caption switched AND the number input's
    // text re-synced to the auto bound (the input's text syncs in a post-paint effect, so
    // painting the caption alone is not proof the input shows it).
    await page.waitForFunction(
      (hi) => {
        const el = document.querySelector('[data-testid=legend-hi]');
        const cap = document.querySelector('[data-testid=legend] .meta')?.textContent ?? '';
        return /displacement magnitude/.test(cap) && !!el
          && !Number.isNaN(parseFloat(el.value))
          && Math.abs(parseFloat(el.value) - hi) <= 1e-3;
      },
      autoHi,
      { timeout: 5000 },
    );
    const r = await page.evaluate(() => ({
      range: window.fieldsmith.ui.resultsRange,
      hiInput: document.querySelector('[data-testid=legend-hi]')?.value ?? null,
      cap: document.querySelector('[data-testid=legend] .meta')?.textContent ?? '',
    }));
    check(
      'stress range kept but deflection legend shows auto bounds',
      !!r.range && r.range.mode === 'stress' && !/values outside the range/.test(r.cap)
        && !!r.hiInput && Math.abs(parseFloat(r.hiInput) - autoHi) <= 1e-3,
      `stress range kept; deflection upper bound ${r.hiInput} ≈ auto ${autoHi.toFixed(4)}`,
    );
  });
  await step('Back to stress: the stored range still governs', async () => {
    await page.locator('[data-testid="results-mode"] button', { hasText: 'Stress' }).click();
    await page.waitForFunction(
      () => {
        const u = window.fieldsmith.ui;
        const el = document.querySelector('[data-testid=legend-hi]');
        return u.resultsRange?.mode === 'stress' && !!el && parseFloat(el.value) === 1;
      },
      { timeout: 5000 },
    );
    check('stress range survives the mode round trip', true, 'upper bound restored to 1 MPa from the stored range');
  });
  await step('Reset the legend to the auto bounds', async () => {
    await page.locator('[data-testid="legend-reset"]').click();
    // Same post-paint caveat as the mode switch above: wait on the input's own value.
    await page.waitForFunction(
      (hi) => {
        const u = window.fieldsmith.ui;
        const el = document.querySelector('[data-testid=legend-hi]');
        const cap = document.querySelector('[data-testid=legend] .meta')?.textContent ?? '';
        return u.resultsRange === null && !/values outside the range/.test(cap)
          && !!el && !Number.isNaN(parseFloat(el.value))
          && Math.abs(parseFloat(el.value) - hi) <= 1e-3;
      },
      stressAutoHi,
      { timeout: 5000 },
    );
    check('legend reset returns to the auto bounds', true, `range cleared, upper bound back to ${stressAutoHi.toFixed(3)} MPa`);
  });
  await step('Reset the deform scale', async () => {
    await setNum(page, 'results-scale', '20');
    const before = await page.evaluate(() => window.fieldsmith.ui.resultsScale);
    await page.locator('[data-testid="scale-reset"]').click();
    await page.waitForFunction(
      () => {
        const u = window.fieldsmith.ui;
        const el = document.querySelector('[data-testid=results-scale]');
        return u.resultsScale === 5 && !!el && el.value === '5';
      },
      { timeout: 5000 },
    );
    check('deform scale reset returns to ×5', true, `was ×${before}, back to ×5`);
  });

  // ---- fill lens ------------------------------------------------------------------------

  const fillRun = async (timeoutMs = 30000) => {
    // The run launches on a 150 ms debounce after the edit; waiting on bare 'ready' would
    // resolve on the pre-edit run, so require the finished run to be for the current document.
    await page.waitForFunction(
      () => {
        const a = window.fieldsmith.analysis.state;
        return ['ready', 'error'].includes(a.status) && a.resultsFor === window.fieldsmith.store.docVersion;
      },
      null,
      { timeout: timeoutMs, polling: 100 },
    );
    return page.evaluate(() => {
      const a = window.fieldsmith.analysis.state;
      const lvl = a.levels[a.levels.length - 1];
      return {
        status: a.status,
        labels: a.levels.map((l) => l.label),
        fi: lvl?.meta?.fill ?? null,
        solid: lvl?.meta?.solid_cases?.[0] ?? null,
        g: lvl?.meta?.cases?.[0] ?? null,
      };
    });
  };

  await step('Fill lens: add lattice fill', async () => {
    await page.locator('[data-lens="fill"]').click();
    await page.waitForSelector('[data-testid="fill-panel"]', { timeout: 5000 });
    await page.waitForSelector('[data-testid="add-fill"]', { timeout: 5000 });
    await page.locator('[data-testid="add-fill"]').click();
    await page.waitForSelector('[data-testid="fill-family"]', { timeout: 5000 });
    const chips = await page.locator('[data-testid="fill-protect"] button').count();
    check('fill panel shows one protect chip per shape', chips === 5, `${chips} chips`);
    const st = await fillRun();
    check('graded ladder ran without error', st.status === 'ready', st.status);
    const lvl = await page.evaluate(() => {
      const a = window.fieldsmith.analysis.state;
      const last = a.levels[a.levels.length - 1];
      const fi = last?.meta?.fill ?? null;
      const solid = last?.meta?.solid_cases?.[0] ?? null;
      const g = last?.meta?.cases?.[0] ?? null;
      return { labels: a.levels.map((l) => l.label), fi, solid, g, lens: window.fieldsmith.store.lens };
    });
    check('both levels carried fill results', lvl.labels.length === 2 && !!lvl.fi, JSON.stringify(lvl.labels));
    check(
      'fill mass 20–40 g on the bracket',
      lvl.fi && lvl.fi.mass_g >= 20 && lvl.fi.mass_g <= 40,
      lvl.fi ? `${lvl.fi.mass_g.toFixed(2)} g of ${lvl.fi.solid_mass_g.toFixed(2)} g solid` : 'no fill meta',
    );
    check(
      'achieved design density inside the spec band',
      lvl.fi && lvl.fi.rho_design[0] >= lvl.fi.rho_min - 0.02 && lvl.fi.rho_design[1] <= lvl.fi.rho_max + 0.02 && lvl.fi.rho_design[1] >= lvl.fi.rho_min,
      lvl.fi ? `${lvl.fi.rho_design[0].toFixed(3)}–${lvl.fi.rho_design[1].toFixed(3)} of [${lvl.fi.rho_min}, ${lvl.fi.rho_max}]` : 'no fill meta',
    );
    const rd = lvl.fi && lvl.solid && lvl.g ? lvl.g.max_displacement / lvl.solid.max_displacement : NaN;
    check('lattice deflects more than solid (1.0–1.6× for −33 % mass)', rd > 1.0 && rd < 1.6, `${rd.toFixed(3)}× deflection`);
    const rs = lvl.fi && lvl.solid && lvl.g ? lvl.g.max_vm_elem / lvl.solid.max_vm_elem : NaN;
    check('graded peak stress within 0.9–1.5× of solid', rs > 0.9 && rs < 1.5, `${rs.toFixed(3)}× peak vm`);
  });

  await step('Fill lens: badge and results rows', async () => {
    const rows = await page.evaluate(() => ({
      summary: document.querySelector('[data-testid=fill-summary]') ? document.querySelector('[data-testid=fill-summary]').textContent : '',
      badge: document.querySelector('[data-testid=fill-badge]') ? document.querySelector('[data-testid=fill-badge]').textContent : '',
      dupBadge: document.querySelectorAll('[data-testid=analysis-badge]').length,
    }));
    check('fill lens badge shows mass, peak and deflection', /mass \d/g.test(rows.badge) && /peak/.test(rows.badge), rows.badge || 'missing');
    check('no duplicate analysis badge in the fill lens', rows.dupBadge === 0, `${rows.dupBadge} analysis badges`);
    check('fill summary lists mass and per-case deltas', /Mass/.test(rows.summary) && /vs solid/.test(rows.summary), rows.summary.slice(0, 80));
  });

  await step('Fill lens: ρ band cross-clamp and undo/redo', async () => {
    await setNum(page, 'fill-rho-max', 0.1);
    const clamped = await page.evaluate(() => {
      const f = window.fieldsmith.store.doc.fill;
      return [f.rho_min, f.rho_max];
    });
    check('ρ max below ρ min clamps up to ρ min', JSON.stringify(clamped) === '[0.2,0.2]', JSON.stringify(clamped));
    await page.locator('[data-testid="undo"]').click();
    const undone = await page.evaluate(() => [window.fieldsmith.store.doc.fill.rho_min, window.fieldsmith.store.doc.fill.rho_max]);
    check('undo restores the ρ band', JSON.stringify(undone) === '[0.2,0.8]', JSON.stringify(undone));
    await page.locator('[data-testid="redo"]').click();
    const redone = await page.evaluate(() => [window.fieldsmith.store.doc.fill.rho_min, window.fieldsmith.store.doc.fill.rho_max]);
    check('redo restores the clamped band', JSON.stringify(redone) === '[0.2,0.2]', JSON.stringify(redone));
    await page.locator('[data-testid="undo"]').click(); // leave the default band in place
  });

  await step('Results lens: the temperature mode on a thermal doc', async () => {
    // The graded re-solve is mechanical only (its cases are the mechanical cases; no
    // thermal field exists at that mesh), so the lattice fill is dropped in the same
    // transaction and every level then carries the nodal field. A single Dirichlet
    // region on an all-insulated domain would give a uniform field, so the two hole
    // regions take the two ends of the gradient, as in examples/bracket-thermal.fsm.
    await page.evaluate(() => {
      const F = window.fieldsmith;
      F.store.transact('thermal render', (d) => {
        d.fill = null;
        d.solve.t_ref = 20;
        d.temperatures.push({ id: 'hot', region: 'base_holes', celsius: 80, case: 'Thermal 1' });
        d.temperatures.push({ id: 'cold', region: 'upright_hole', celsius: 20, case: 'Thermal 1' });
      });
    });
    const solved = await runHas(
      () => {
        const a = window.fieldsmith.analysis.state;
        return a.status === 'ready' && a.resultsFor === window.fieldsmith.store.docVersion && a.levels.length >= 1;
      },
      70000,
    );
    await page.locator('[data-lens="results"]').click();
    await page.locator('[data-testid="results-mode"] button[data-value="temperature"]').click();
    await page.waitForTimeout(300);
    const out = await page.evaluate(() => {
      const F = window.fieldsmith;
      const lvl = F.analysis.state.levels[F.analysis.state.levels.length - 1];
      const i = lvl.meta.cases.findIndex((c) => c.name === 'Thermal 1');
      const t1 = i >= 0 ? (lvl.temperature[i] ?? null) : null;
      let lo = Infinity, hi = -Infinity;
      if (t1) for (const v of t1) { if (v < lo) lo = v; if (v > hi) hi = v; }
      return {
        temps: F.store.doc.temperatures.length,
        tref: F.store.doc.solve.t_ref,
        i,
        count: lvl.meta.cases.length,
        othersNull: lvl.temperature.every((t, k) => t === null || k === i),
        lo: t1 ? lo : NaN,
        hi: t1 ? hi : NaN,
        mode: F.ui.resultsMode,
        captionEnvelope: document.body.innerText.includes('per-node max across thermal cases, nodal (°C)'),
      };
    });
    const range = isNaN(out.lo) ? 'none' : `${out.lo.toFixed(1)}–${out.hi.toFixed(1)} °C`;
    check(
      'thermal doc solved: two temperature statements, nodal °C on the thermal case only',
      solved && out.temps === 2 && out.tref === 20 && out.mode === 'temperature' && out.i >= 0 && out.othersNull,
      `temps ${out.temps}, tref ${out.tref}, cases ${out.count}, Thermal 1 at index ${out.i}, mechanical slots null ${out.othersNull}`,
    );
    check(
      'nodal field keeps the Dirichlet bounds with a real gradient',
      Math.abs(out.lo - 20) < 0.01 && Math.abs(out.hi - 80) < 0.01 && out.hi - out.lo > 20,
      `nodal range ${range}`,
    );
    check('the legend names the envelope read in °C', out.captionEnvelope, 'caption on screen');
    await page.locator('[data-testid="results-case"] button', { hasText: 'Thermal 1' }).click();
    const after = await page.evaluate(() => {
      const F = window.fieldsmith;
      return { ci: F.ui.resultsCase, caption: document.body.innerText.includes('temperature, nodal (°C)') };
    });
    check(
      'thermal case selectable, caption narrows to the nodal read',
      after.ci === out.i && after.caption,
      `resultsCase ${after.ci}, caption 'temperature, nodal (°C)' shown ${after.caption}`,
    );
  });

  // ---- verdict ---------------------------------------------------------------------------

  const total = Date.now() - t0;
  const ok = cmp.equal && errors.length === 0 && checks.every((c) => c.pass);
  const fmtLevels = () =>
    res.levels
      .map(
        (l) =>
          `| ${l.label} | ${l.p.elements} | ${l.p.dofs} | ${l.p.order} | ${l.p.iterations} | ${l.p.mesh_ms} | ${l.p.solve_ms} | ${l.p.total_ms} | ${l.peak.toFixed(2)} MPa | ${l.defl.toFixed(3)} mm |`,
      )
      .join('\n');
  const fmtChecks = () =>
    checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**FAIL**'} | ${c.detail} |`).join('\n');
  const lines = [
    '# Bracket test — headless UI run',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}** (scripted; the human hallway test is separate — see docs/hallway-test.md)`,
    '',
    '| Step | Time | Screenshot |',
    '| :-- | --: | :-- |',
    ...steps.map((s) => `| ${s.name} | ${(s.ms / 1000).toFixed(2)} s | ${s.file} |`),
    '',
    `Total scripted time: ${(total / 1000).toFixed(1)} s.`,
    '',
    `- Modelled shapes equal \`examples/bracket.fsm\`: **${cmp.equal ? 'yes' : 'no'}**`,
    `- Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
    '',
    '## Checks',
    '',
    '| Check | Result | Detail |',
    '| :-- | :-- | :-- |',
    fmtChecks(),
    ...(res.error ? ['', `Analysis error: ${res.error}`] : []),
    ...(res.skippedFine ? ['', `Refined level skipped: ${res.skippedFine}`] : []),
    '',
    '## Ladder provenance',
    '',
    '| Level | Elements | DOFs | Order | Iterations | Mesh ms | Solve ms | Total ms | Peak vm | Max deflection |',
    '| :-- | --: | --: | :-- | --: | --: | --: | --: | --: | --: |',
    fmtLevels(),
    '',
    `- Edit → coarse: measured on the page clock from the last edit commit to levelArrived[0]: ${Number.isFinite(coarseMs) ? Math.round(coarseMs) + ' ms' : 'not captured'} of the 1500 ms budget; launch fired ${Number.isFinite(coarseMs) ? Math.round(res.startedMs - edit) + ' ms' : '?'} after the edit (150 ms of that is the edit debounce).`,
    `- Edit → refined: ${Number.isFinite(refinedMs) ? Math.round(refinedMs) + ' ms of the 10000 ms budget' : 'not captured'}.`,
    '- Solve is cold-started at every level (PCG from zero displacement; no warm start is carried between refinement levels), so the level-to-level deltas are of cold solves.',
    '- Timings measure this machine (software WebGL, JS shell): treat them as wall-clock evidence, not a performance guarantee.',
    '',
    '## Document produced',
    '',
    '```text',
    cmp.got.trim(),
    '```',
    '',
    '---',
    '*Generated by `npm run test:e2e` — do not edit by hand.*',
    '',
  ];
  writeFileSync(resolve(docsDir, 'bracket-e2e.md'), lines.join('\n'));
  await page.close();
  return ok;
}