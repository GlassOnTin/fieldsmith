// In-UI profile sketching (VISION §7 item 1, the sketching half — its hierarchy half shipped
// with the model tree in M0/M1): the Extrude inspector's sketch canvas draws the closed
// profile in the node's own X-Y plane, lets a click on an edge insert a point there, and
// lets a dot be dragged. The canvas is not a decoration — it draws and hit-tests the same
// arc maths as the field (fieldsmith-field prim.rs arc_geom: centre on the left normal by
// (1−k²)/(2k)·l/2, arc mid on the right normal by k·l/2 — a positive bulge bows to the
// right of a→b), and every edit goes through store.transact / transient / commitTransient,
// the same paths the numeric table uses, so a sketched edit replays exactly like a typed one.
//
// Two fixtures:
//
//   A — a straight 20 × 10 rectangle extrusion: the four dots land at the fitted px
//       positions; a click on the bottom edge inserts a point at the clicked position
//       (after the edge the click was nearest); dragging a dot commits one undo step
//       (undo restores the vertex bit-exactly, redo reapplies); the result round-trips
//       the shipped .fsm printer and re-parses bit-equal.
//   B — the same rectangle with a 0.9 bulge on the segment leaving (20, 10): the ARC.
//       The drawn arc must bow to the RIGHT of a→b — west-pointing, so right = +y, above
//       the chord in px-space — through its mid (10, 19). A pixel scan of the canvas pins
//       the bow side, and a click beneath the bulged edge must still land on the BOTTOM
//       edge (a canvas that mirrored the arc would grab the click instead).
//
// Run with node tests/run-e2e.mjs --only profilesketch.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const DOC_A = `\
fieldsmith 1
name "Sketch pad"
material petg
extrude pad h=4 profile=[(0, 0), (20, 0), (20, 10), (0, 10)]
`;
const DOC_B = `\
fieldsmith 1
name "Sketch arc"
material petg
extrude arc h=4 profile=[(0, 0), (20, 0), (20, 10, 0.9), (0, 10)]
`;

// ---- the canvas's own geometry, replicated for click targets (Sketch.tsx) -------------
// Fit constants are pinned as behaviour by this file: changing SKETCH_W/H/PAD or the arc
// sampling moves every click here, so update both together.
const W = 240, H = 180, PAD = 20;
const ARC_SAMPLES = 32;

function sampleProfile(pts) {
  const line = [[pts[0].x, pts[0].y]];
  const ends = [0];
  const n = pts.length;
  for (let i = 0; i < n; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % n];
    if (a.bulge === 0) {
      line.push([b.x, b.y]);
      ends.push(line.length - 1);
    } else {
      const cx = b.x - a.x, cy = b.y - a.y;
      const l = Math.hypot(cx, cy);
      const d0 = cx / l, d1 = cy / l;
      const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
      const h = 0.5 * l * (1 - a.bulge * a.bulge) / (2 * a.bulge);
      const ccx = mx - d1 * h, ccy = my + d0 * h;
      const r = 0.5 * l * (1 + a.bulge * a.bulge) / (2 * Math.abs(a.bulge));
      const sag = a.bulge * l * 0.5;
      const tA = Math.atan2(a.y - ccy, a.x - ccx);
      let dT = Math.atan2(my - d0 * sag - ccy, mx + d1 * sag - ccx) - tA;
      if (dT > Math.PI) dT -= 2 * Math.PI;
      if (dT <= -Math.PI) dT += 2 * Math.PI;
      const sweep = Math.sign(dT) * 2 * Math.abs(dT);
      ends.push(line.length);
      for (let t = 1; t <= ARC_SAMPLES; t++) {
        if (t < ARC_SAMPLES) {
          const th = tA + sweep * (t / ARC_SAMPLES);
          line.push([ccx + r * Math.cos(th), ccy + r * Math.sin(th)]);
        } else line.push([b.x, b.y]);
      }
    }
  }
  return { line, ends };
}

function sketchView(pts) {
  const { line, ends } = sampleProfile(pts);
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const [x, y] of line) {
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
  }
  const spanX = Math.max(maxX - minX, 1e-9);
  const spanY = Math.max(maxY - minY, 1e-9);
  const s = Math.min((W - 2 * PAD) / spanX, (H - 2 * PAD) / spanY);
  const ox = PAD + (W - 2 * PAD - spanX * s) / 2 - minX * s;
  const oy = H - PAD - (H - 2 * PAD - spanY * s) / 2 + minY * s;
  return { s, ox, oy, line, ends };
}

const PAD_PTS = [
  { x: 0, y: 0, bulge: 0 },
  { x: 20, y: 0, bulge: 0 },
  { x: 20, y: 10, bulge: 0 },
  { x: 0, y: 10, bulge: 0 },
];
const ARC_PTS = [
  { x: 0, y: 0, bulge: 0 },
  { x: 20, y: 0, bulge: 0 },
  { x: 20, y: 10, bulge: 0.9 },
  { x: 0, y: 10, bulge: 0 },
];

/** In-page: count the canvas's ink pixels in a CSS-px rectangle. Ink is any painted
 * pixel (alpha ≥ 64) — the canvas paints on transparency. Accent = blue-dominant paint
 * (the profile stroke); dot = flat grey (the vertex dots and axes). */
async function scanRect(page, x0, y0, x1, y1) {
  return page.evaluate(
    ([x0, y0, x1, y1]) => {
      const c = document.querySelector('[data-testid="profile-sketch"]');
      const img = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
      const dpr = c.width / c.getBoundingClientRect().width;
      let accent = 0, dot = 0;
      for (let py = Math.round(y0 * dpr); py < Math.round(y1 * dpr); py++) {
        for (let px = Math.round(x0 * dpr); px < Math.round(x1 * dpr); px++) {
          const i = (py * c.width + px) * 4;
          const a = img[i + 3];
          if (a < 64) continue;
          const r = img[i], g = img[i + 1], b = img[i + 2];
          if (b >= r + 30 && b >= 100) accent++;
          else if (r > 100 && g > 100 && Math.abs(r - b) < 20 && Math.abs(r - g) < 20) dot++;
        }
      }
      return { accent, dot };
    },
    [x0, y0, x1, y1],
  );
}

/** In-page: the dome x-band scan for fixture B — accent ink rows relative to the chord
 * py, clear of the chord row (±2 px) and of the axis/bottom-edge rows (from 157 CSS px). */
async function scanBand(page, pxMidCss, bandHalf, chordPyCss) {
  return page.evaluate(
    ([pxMidCss, bandHalf, chordPyCss]) => {
      const c = document.querySelector('[data-testid="profile-sketch"]');
      const img = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
      const dpr = c.width / c.getBoundingClientRect().width;
      let above = 0, below = 0;
      const px0 = Math.round((pxMidCss - bandHalf) * dpr), px1 = Math.round((pxMidCss + bandHalf) * dpr);
      const rChord = Math.round(chordPyCss * dpr), rBottom = Math.round(157 * dpr);
      for (let py = 0; py < c.height; py++) {
        for (let px = px0; px < px1; px++) {
          const i = (py * c.width + px) * 4;
          const a = img[i + 3];
          if (a < 64) continue;
          const r = img[i], g = img[i + 1], b = img[i + 2];
          const accent = b >= r + 30 && b >= 100;
          const grey = !accent && r > 100 && g > 100 && Math.abs(r - b) < 20 && Math.abs(r - g) < 20;
          if (!accent && !grey) continue;
          if (py < rChord - 2) above++;
          else if (py > rChord + 2 && py < rBottom) below++;
        }
      }
      return { above, below };
    },
    [pxMidCss, bandHalf, chordPyCss],
  );
}

async function profileOf(page, id) {
  return page.evaluate((id) => {
    const find = (list) => {
      for (const nd of list) {
        if (nd.id === id && nd.kind.type === 'extrude') return nd.kind.profile.map((p) => ({ ...p }));
        if (nd.kind.type === 'group') {
          const f = find(nd.kind.children);
          if (f) return f;
        }
      }
      return null;
    };
    return find(window.fieldsmith.store.doc.root);
  }, id);
}

/** Poll until the profile satisfies pred (the drawn/tabled state lags the click by a frame). */
async function untilProfile(page, id, pred, tag, ms = 6000) {
  const t0 = Date.now();
  let p = null;
  for (;;) {
    p = await profileOf(page, id);
    if (p && pred(p)) return p;
    if (Date.now() - t0 > ms) throw new Error(`${tag}: profile never matched; last ${JSON.stringify(p)}`);
    await page.waitForTimeout(120);
  }
}

/** Poll until the canvas carries at least `min` ink px somewhere — the first draw after
 * selecting the row can race the first scan. */
async function untilInk(page, min) {
  const t0 = Date.now();
  for (;;) {
    const n = await scanRect(page, 0, 0, W, H);
    if (n.accent + n.dot >= min) return n;
    if (Date.now() - t0 > 6000) throw new Error(`sketch canvas never drew (last ${JSON.stringify(n)})`);
    await page.waitForTimeout(120);
  }
}

async function loadDoc(page, fsm, rowId) {
  await page.evaluate(async (fsm) => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    const r = w.parseFsm(fsm);
    if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
    window.fieldsmith.store.load(r.ok);
  }, fsm);
  await page.locator(`.row[data-node="${rowId}"]`).click();
  await untilInk(page, 150);
}

export async function profilesketchTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `profilesketch-${name}.png`);
  const table = (names) =>
    [
      '| Check | Result | Detail |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  // Fixture-fit constants are filled in as the fixtures run; the V-doc's fitted-views
  // lines fall back to a dash when the try ended early.
  let view = null;
  let viewB = null;
  let pxMid = 0;
  let pyChord = 0;
  let pyMid = 0;

  // Registered up front so the finaliser can flip it, and so a catch has something to
  // annotate whatever the failure point.
  check('e2e completed', false, '');

  try {
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    // ---- A — straight pad: draw, insert, drag, undo, round-trip ------------------------
    await loadDoc(page, DOC_A, 'pad');
    view = sketchView(PAD_PTS);
    const corners = PAD_PTS.map((p) => [view.ox + p.x * view.s, view.oy - p.y * view.s]);
    const near = await scanRect(page, 0, 0, W, H);
    const atCorners = await Promise.all(
      corners.map(([x, y]) => scanRect(page, Math.max(0, x - 4), Math.max(0, y - 4), x + 4, y + 4)),
    );
    check(
      'A: the four vertex dots land at the fitted px positions',
      atCorners.every((n) => n.accent + n.dot >= 2) && near.accent + near.dot > 150,
      `canvas ink ${near.accent + near.dot} px; per-corner ink ${atCorners.map((n) => n.accent + n.dot).join('/')} (fit s=${view.s.toFixed(3)} ox=${view.ox.toFixed(1)} oy=${view.oy.toFixed(1)})`,
    );

    // Insert: click bottom edge near model (10, 3) → px (120, 110). Distances: bottom 30 px,
    // top 70, sides 100 — the bottom edge is unique-nearest, so the point lands after
    // vertex 0. Model coords come out exact: (px−ox)/s = 100/10 = 10, 30/10 = 3.
    let box = await page.locator('[data-testid="profile-sketch"]').boundingBox();
    await page.mouse.click(box.x + 120, box.y + 110);
    const afterInsert = await untilProfile(
      page,
      'pad',
      (p) => p.length === 5 && Math.abs(p[1].x - 10) < 0.25 && Math.abs(p[1].y - 3) < 0.25 && p[0].x === 0 && p[2].x === 20 && p[2].y === 0,
      'A insert',
    );
    check(
      'A: a click on the bottom edge inserts a point at the clicked model position',
      true,
      `profile [(0,0), (${Number(afterInsert[1].x.toFixed(3))}, ${Number(afterInsert[1].y.toFixed(3))}), (20,0), (20,10), (0,10)] — inserted after the nearest edge, not the closest corner`,
    );

    // The numeric table shows the same point (row 2's x/y inputs); wait on the input
    // values, which sync after the store emit paints.
    await page.waitForFunction(
      () => {
        const tr = document.querySelector('table.profile-table tbody tr:nth-child(2)');
        const ins = tr ? tr.querySelectorAll('input') : null;
        return ins && Math.abs(parseFloat(ins[0].value) - 10) < 0.05 && Math.abs(parseFloat(ins[1].value) - 3) < 0.05;
      },
      { timeout: 5000 },
    );
    const tabled = await page.evaluate(() =>
      [...document.querySelectorAll('table.profile-table tbody tr')].map((tr) =>
        [...tr.querySelectorAll('input')].map((i) => i.value),
      ),
    );
    const r2 = tabled[1];
    check(
      'A: the numeric table shows the sketched point',
      !!r2 && Math.abs(parseFloat(r2[0]) - afterInsert[1].x) < 0.05 && Math.abs(parseFloat(r2[1]) - afterInsert[1].y) < 0.05,
      `row 2 inputs ${JSON.stringify(r2)} vs store (${afterInsert[1].x}, ${afterInsert[1].y})`,
    );

    // Drag vertex 4 — profile index 3 — from (20, 10) to (23.5, 11.5) (px 220,40 → 255,25,
    // exact under the frozen pre-drag view), then undo: one commit must restore it bit-exactly.
    await page.mouse.move(box.x + 220, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(box.x + 255, box.y + 25, { steps: 6 });
    await page.mouse.up();
    const afterDrag = await untilProfile(
      page,
      'pad',
      (p) => p.length === 5 && Math.abs(p[3].x - 23.5) < 0.25 && Math.abs(p[3].y - 11.5) < 0.25,
      'A drag',
    );
    check(
      'A: dragging a dot moves exactly that vertex',
      Math.abs(afterDrag[3].x - 23.5) < 0.25 && Math.abs(afterDrag[3].y - 11.5) < 0.25,
      `vertex 4 moved to (${Number(afterDrag[3].x.toFixed(3))}, ${Number(afterDrag[3].y.toFixed(3))}); others intact`,
    );

    await page.locator('[data-testid="undo"]').click();
    const afterUndo = await untilProfile(
      page,
      'pad',
      (p) => p.length === 5 && p[3].x === 20 && p[3].y === 10 && Math.abs(p[1].x - 10) < 0.25,
      'A undo',
    );
    check(
      'A: the drag commits one undo step — undo restores the vertex bit-exactly',
      afterUndo[3].x === 20 && afterUndo[3].y === 10 && afterUndo[1].x !== undefined,
      `vertex 4 back at (${afterUndo[3].x}, ${afterUndo[3].y}); a per-move leak would undo only the last move`,
    );

    await page.locator('[data-testid="redo"]').click();
    const afterRedo = await untilProfile(
      page,
      'pad',
      (p) => p.length === 5 && Math.abs(p[3].x - 23.5) < 0.25 && Math.abs(p[3].y - 11.5) < 0.25,
      'A redo',
    );
    check(
      'A: redo reapplies the moved vertex',
      true,
      `vertex 4 at (${Number(afterRedo[3].x.toFixed(3))}, ${Number(afterRedo[3].y.toFixed(3))})`,
    );

    // Post-sketch round-trip: the printed .fsm re-parses to the store's own numbers.
    const roundA = await page.evaluate(async (id) => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      const find = (list) => {
        for (const nd of list) {
          if (nd.id === id && nd.kind.type === 'extrude') return nd.kind.profile;
          if (nd.kind.type === 'group') {
            const f = find(nd.kind.children);
            if (f) return f;
          }
        }
        return null;
      };
      return {
        line: (text.split('\n').find((l) => l.startsWith('extrude ')) ?? '').trim(),
        back: r.ok ? find(r.ok.root) : null,
      };
    }, 'pad');
    const backA = roundA.back;
    const liveA = await profileOf(page, 'pad');
    const sameA =
      !!backA &&
      backA.length === liveA.length &&
      backA.every(
        (p, i) =>
          Math.abs(p.x - liveA[i].x) < 1e-9 && Math.abs(p.y - liveA[i].y) < 1e-9 && Math.abs(p.bulge - liveA[i].bulge) < 1e-9,
      );
    check(
      'A: the sketched profile round-trips .fsm print → parse bit-equal',
      sameA && /profile=\[\(0, 0\), \(10, 3\), \(20, 0\), \(23\.5, 11\.5\), \(0, 10\)\]/.test(roundA.line ?? ''),
      roundA.line,
    );
    await page.screenshot({ path: shot('01-pad-sketch') });

    // ---- B — the arc fixture: bow side, mid, and the discriminating click --------------
    await loadDoc(page, DOC_B, 'arc');
    viewB = sketchView(ARC_PTS);
    pxMid = viewB.ox + 10 * viewB.s;
    pyChord = viewB.oy - 10 * viewB.s;
    pyMid = viewB.oy - 19 * viewB.s;
    // Poll both scans until the arc frame is on screen (a stale earlier frame shows the
    // previous fixture's edges, which contribute nothing inside the band).
    let inBand = null, atSag = null;
    const tB = Date.now();
    for (;;) {
      inBand = await scanBand(page, pxMid, 30, pyChord);
      atSag = await scanRect(page, pxMid - 5, pyMid - 5, pxMid + 5, pyMid + 5);
      if ((inBand.above > 100 && inBand.below === 0 && atSag.accent >= 3) || Date.now() - tB > 3000) break;
      await page.waitForTimeout(120);
    }
    check(
      'B: the drawn arc bulges to the right of the point order — above its chord and nowhere below it',
      inBand !== null && inBand.above > 100 && inBand.below === 0,
      `dome-x-band ink above chord ${inBand?.above ?? 0} px, below ${inBand?.below ?? 0} px (a mirrored arc draws them swapped)`,
    );
    check(
      'B: the arc passes through its bow mid (10, 19)',
      atSag !== null && atSag.accent >= 3,
      `accent ink within 5 px of the mid px ${atSag?.accent ?? 0}`,
    );

    // The discriminating click: model (10, 5) — beneath the bulged edge. Under the correct
    // (bow-up) arc the nearest drawn thing is the BOTTOM edge (insert after vertex 0); a
    // canvas that mirrored the bow would put arc ink at y ≈ 1 and grab the click instead
    // (insert after vertex 2, profile length 5 with the new point at a different slot).
    box = await page.locator('[data-testid="profile-sketch"]').boundingBox();
    await page.mouse.click(box.x + 120, box.y + (viewB.oy - 5 * viewB.s));
    const afterArc = await untilProfile(
      page,
      'arc',
      (p) =>
        p.length === 5 &&
        Math.abs(p[1].x - 10) < 0.25 &&
        Math.abs(p[1].y - 5) < 0.25 &&
        p[2].x === 20 &&
        p[2].y === 0 &&
        Math.abs(p[3].bulge - 0.9) < 1e-9,
      'B click',
    );
    check(
      'B: a click beneath the bulged edge lands on the bottom edge, not the arc',
      afterArc.length === 5 && Math.abs(afterArc[1].x - 10) < 0.25,
      `inserted after vertex 0 at (${Number(afterArc[1].x.toFixed(3))}, ${Number(afterArc[1].y.toFixed(3))}) with the 0.9 bulge untouched on vertex 3`,
    );

    const roundB = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      const text = w.printFsm(window.fieldsmith.store.doc);
      const r = w.parseFsm(text);
      const find = (list) => {
        for (const nd of list) {
          if (nd.id === 'arc' && nd.kind.type === 'extrude') return nd.kind.profile;
          if (nd.kind.type === 'group') {
            const f = find(nd.kind.children);
            if (f) return f;
          }
        }
        return null;
      };
      return {
        line: (text.split('\n').find((l) => l.startsWith('extrude ')) ?? '').trim(),
        back: r.ok ? find(r.ok.root) : null,
      };
    });
    const liveB = await profileOf(page, 'arc');
    const sameB =
      !!roundB.back &&
      roundB.back.length === liveB.length &&
      roundB.back.every(
        (p, i) =>
          Math.abs(p.x - liveB[i].x) < 1e-9 && Math.abs(p.y - liveB[i].y) < 1e-9 && Math.abs(p.bulge - liveB[i].bulge) < 1e-9,
      );
    check(
      'B: the arc fixture round-trips .fsm with the bulge kept',
      sameB && (roundB.line ?? '').includes('(20, 10, 0.9)'),
      roundB.line,
    );
    await page.screenshot({ path: shot('02-arc-sketch') });

    check('no console errors', errors.length === 0, errors.join(' | ').slice(0, 400));
  } catch (e) {
    const c = checks.find((x) => x.name === 'e2e completed');
    if (c) c.detail = String(e);
    else checks.push({ name: 'e2e completed', pass: false, detail: String(e) });
  }
  {
    const done = checks.find((x) => x.name === 'e2e completed');
    if (done && done.detail === '') done.pass = true;
  }

  const names = [
    'A: the four vertex dots land at the fitted px positions',
    'A: a click on the bottom edge inserts a point at the clicked model position',
    'A: the numeric table shows the sketched point',
    'A: dragging a dot moves exactly that vertex',
    'A: the drag commits one undo step — undo restores the vertex bit-exactly',
    'A: redo reapplies the moved vertex',
    'A: the sketched profile round-trips .fsm print → parse bit-equal',
    'B: the drawn arc bulges to the right of the point order — above its chord and nowhere below it',
    'B: the arc passes through its bow mid (10, 19)',
    'B: a click beneath the bulged edge lands on the bottom edge, not the arc',
    'B: the arc fixture round-trips .fsm with the bulge kept',
    'no console errors',
    'e2e completed',
  ];
  const passed = names.filter((n) => checks.find((x) => x.name === n)?.pass).length;
  const ok = passed === names.length;

  const lines = [
    '# V31 — In-UI profile sketching on the Extrude inspector',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}** · ${passed}/${names.length} checks · ${new Date().toISOString()}`,
    '',
    'The Extrude inspector’s sketch canvas draws the closed profile in the node’s own',
    'X-Y plane — the same arc maths as the field (prim.rs arc_geom, positive bulge bows',
    'right of a→b) — and a click on an edge inserts a point at the clicked position',
    'through store.transact; a dragged dot moves through transient and commits one',
    'undo step through commitTransient. Two fixtures: a straight 20 × 10 rectangle and',
    'the same rectangle with a 0.9 bulge on the west-pointing segment.',
    '',
    '## Checks',
    '',
    table(names),
    '',
    '## Fitted views (the e2e replicates Sketch.tsx’s fit before clicking)',
    '',
    view && viewB
      ? `- straight pad: s = ${view.s.toFixed(4)} px/mm, origin px (${view.ox.toFixed(2)}, ${view.oy.toFixed(2)})`
      : '- straight pad: (fixture A did not run)',
    view && viewB
      ? `- arc: s = ${viewB.s.toFixed(4)} px/mm, chord py ${pyChord.toFixed(2)}, mid px (${pxMid.toFixed(2)}, ${pyMid.toFixed(2)})`
      : '- arc: (fixture B did not run)',
    '',
    '## Limits',
    '',
    '- Point deletion and bulge editing stay on the numeric table (the sketch adds and moves only).',
    '- No snapping or grid: points land where clicked, at ±1 px (≈ ±0.25 mm here).',
    '- The .fsm round-trip is asserted at the parse-back level (shortest-round-trip printing',
    '  parses back bit-equal). Field consumption of sketched profiles is covered by the',
    '  native extrude tests and the shape family, not re-asserted here.',
    '- The pixel-side bow check reads the accent colour from the canvas of the theme the',
    '  headless run starts in; thresholds accept both the light and the dark accent.',
    '',
    '*Generated by npm run test:e2e (web/tests/profilesketch.mjs) — do not edit by hand.*',
    '',
  ].join('\n');
  if (!ok) {
    const bad = checks.filter((c) => !c.pass);
    const missing = names.filter((n) => !checks.find((x) => x.name === n));
    console.error(
      [
        'profilesketch: FAIL — failed checks:',
        ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`),
        ...missing.map((n) => `  ✗ ${n}: — no check registered under this name`),
      ].join('\n'),
    );
  } else {
    writeFileSync(resolve(docsDir, 'V31.md'), lines);
  }
  return ok;
}