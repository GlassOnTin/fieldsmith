// Shape-lens acceptance (GOAL.md section A), driven through the real UI in a headless browser.
//
// Every edit goes through the palette, menus, inspector, tree, keyboard or the viewport gizmo.
// Geometry is checked by sampling the document's field with the Rust evaluator (WebAssembly),
// so a check passes only if the edit really changed the solid the way the UI claims.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

async function setNum(page, testId, value) {
  const el = page.locator(`[data-testid="${testId}"]`);
  await el.click();
  await el.fill(String(value));
  await el.press('Enter');
}
async function setVec(page, testId, v) {
  for (const [i, a] of ['x', 'y', 'z'].entries()) await setNum(page, `${testId}-${a}`, v[i]);
}
async function setOp(page, op) {
  await page.locator(`[data-testid="op"] button[data-value="${op}"]`).click();
}
async function palette(page, text) {
  await page.keyboard.press('Control+k');
  await page.locator('[data-testid="palette-input"]').fill(text);
  await page.keyboard.press('Enter');
}
async function selectRow(page, id) {
  await page.locator(`.row[data-node="${id}"]`).click();
}
const doc = (page) => page.evaluate(() => window.fieldsmith.store.doc);
const selected = (page) => page.evaluate(() => window.fieldsmith.store.selection?.id ?? null);
const canonical = (page) =>
  page.evaluate(async () => {
    const w = await import('/src/doc/wasm.ts');
    return w.printFsm(window.fieldsmith.store.doc);
  });
/** Field value (mm, negative inside) at each point, from the Rust evaluator. */
const field = (page, pts) =>
  page.evaluate(async (pts) => {
    const w = await import('/src/doc/wasm.ts');
    const r = w.evalBody(window.fieldsmith.store.doc, new Float64Array(pts.flat()));
    return pts.map((_, i) => r[i * 4]);
  }, pts);
function findIn(nodes, id) {
  for (const n of nodes) {
    if (n.id === id) return n;
    if (n.kind.type === 'group') {
      const r = findIn(n.kind.children, id);
      if (r) return r;
    }
  }
  return null;
}

export async function shapeTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, acceptDownloads: true });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  let shot = 0;
  const check = async (area, name, ok, detail = '') => {
    shot += 1;
    const file = `shape-${String(shot).padStart(2, '0')}.png`;
    await page.waitForTimeout(120);
    await page.screenshot({ path: resolve(screens, file) });
    checks.push({ area, name, ok: !!ok, detail, file });
  };

  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('[data-testid="empty-state"]');

  try {
  // ---- A1 primitives (palette) -----------------------------------------------------------
  const prims = [
    ['box', 'Add box', [0, 0, 10]],
    ['sphere', 'Add sphere', [40, 0, 10]],
    ['cylinder', 'Add cylinder', [80, 0, 10]],
    ['capsule', 'Add capsule', [120, 0, 10]],
    ['torus', 'Add torus', [160, 0, 4]],
    ['extrude', 'Add extruded profile', [200, 0, 0]],
  ];
  const ids = {};
  for (const [type, cmd, at] of prims) {
    await palette(page, cmd);
    ids[type] = await selected(page);
    await setVec(page, 'position', at);
  }
  {
    const d = await doc(page);
    const types = d.root.map((n) => n.kind.type).join(', ');
    // A point inside each primitive (torus: on its tube centre line; extrude: inside its profile).
    const inside = [
      [0, 0, 10],
      [40, 0, 10],
      [80, 0, 10],
      [120, 0, 18],
      [172, 0, 4],
      [200, 0, 2],
    ];
    const v = await field(page, inside);
    const torusHole = (await field(page, [[160, 0, 4]]))[0];
    await page.keyboard.press('f');
    await check(
      'A1',
      'Six primitives added from the command palette, each solid where expected',
      types === 'box, sphere, cylinder, capsule, torus, extrude' && v.every((x) => x < 0) && torusHole > 0,
      `types: ${types}; field inside each: ${v.map((x) => x.toFixed(2)).join(', ')} mm; torus hole: ${torusHole.toFixed(2)} mm`,
    );
  }

  // ---- A2 operations ---------------------------------------------------------------------
  await selectRow(page, ids.sphere);
  await setVec(page, 'position', [10, 10, 20]);
  await setNum(page, 'diameter', 16);
  await setOp(page, 'subtract');
  {
    const [cut, kept] = await field(page, [
      [10, 10, 20],
      [-8, -8, 2],
    ]);
    await check('A2', 'Subtract: sphere cuts the box corner', cut > 0 && kept < 0, `at sphere centre ${cut.toFixed(2)} mm, far corner ${kept.toFixed(2)} mm`);
  }
  await setOp(page, 'intersect');
  {
    const [inBoth, boxOnly] = await field(page, [
      [6, 6, 16],
      [-8, -8, 2],
    ]);
    await check('A2', 'Intersect: only the overlap remains', inBoth < 0 && boxOnly > 0, `overlap ${inBoth.toFixed(2)} mm, box-only point ${boxOnly.toFixed(2)} mm`);
  }
  await setOp(page, 'add');
  await selectRow(page, ids.cylinder);
  await setVec(page, 'position', [0, 0, 30]);
  await setNum(page, 'diameter', 10);
  {
    // Cylinder (⌀10, h 20, centre z 30) stands on the 20 mm box's top face (z 20): the inside
    // corner at the seam gains material only when blended.
    const probe = [[0, -5.8, 20.8]];
    const sharp = (await field(page, probe))[0];
    await setNum(page, 'blend', 4);
    const smooth = (await field(page, probe))[0];
    const cyl = findIn((await doc(page)).root, ids.cylinder);
    await check('A2', 'Blend radius adds a fillet at the seam', sharp > 0 && smooth < 0, `seam probe: sharp ${sharp.toFixed(2)} mm, blended (r ${cyl.blend}) ${smooth.toFixed(2)} mm; cylinder ${cyl.op} at (${cyl.transform.translate.join(', ')})`);
  }

  // ---- A4 transform: gizmo drag with grid and feature snap -------------------------------
  await selectRow(page, ids.capsule);
  await setVec(page, 'position', [127.3, 0, 10]);
  await selectRow(page, ids.torus);
  await page.keyboard.press('w');
  await page.waitForTimeout(200);
  /** Find the gizmo's X handle by hovering along the projected +X axis, then drag it `mm`. */
  const gizmoDragX = async (at, mm) => {
    const o = await page.evaluate((p) => window.fieldsmith.view.project(p), at);
    const ax = await page.evaluate((p) => window.fieldsmith.view.project([p[0] + 10, p[1], p[2]]), at);
    const len = Math.hypot(ax.x - o.x, ax.y - o.y);
    const u = { x: (ax.x - o.x) / len, y: (ax.y - o.y) / len };
    const pxPerMm = len / 10;
    for (let grab = 20; grab <= 200; grab += 6) {
      const s = { x: o.x + u.x * grab, y: o.y + u.y * grab };
      await page.mouse.move(s.x, s.y);
      if ((await page.evaluate(() => window.fieldsmith.view.gizmoAxis())) !== 'X') continue;
      const e = { x: s.x + u.x * mm * pxPerMm, y: s.y + u.y * mm * pxPerMm };
      await page.mouse.down();
      for (let i = 1; i <= 15; i++) await page.mouse.move(s.x + ((e.x - s.x) * i) / 15, s.y + ((e.y - s.y) * i) / 15);
      await page.mouse.up();
      return grab;
    }
    return null;
  };
  {
    const before = findIn((await doc(page)).root, ids.torus).transform.translate;
    // Drag the X arrow towards the capsule centre (x 127.3), releasing 0.6 mm short of it.
    const grab = await gizmoDragX(before, 127.3 - before[0] + 0.6);
    const after = findIn((await doc(page)).root, ids.torus).transform.translate;
    const moved = after[0] !== before[0] && after[1] === before[1] && after[2] === before[2];
    await check(
      'A4',
      'Gizmo drag along X snaps to the nearby capsule centre',
      moved && after[0] === 127.3,
      `X handle found ${grab === null ? 'nowhere' : `${grab} px from the origin`}; torus x ${before[0]} → ${after[0]} mm (y ${after[1]}, z ${after[2]}); capsule centre x 127.3`,
    );
    // Grid snap: a drag far from other centres lands on the grid.
    await gizmoDragX(after, 23.3);
    const grid = findIn((await doc(page)).root, ids.torus).transform.translate;
    const step = await page.evaluate(() => window.fieldsmith.ui.gridStep);
    const onGrid = Math.abs(grid[0] / step - Math.round(grid[0] / step)) < 1e-9;
    await check('A4', `Gizmo drag far from features snaps to the ${step} mm grid`, grid[0] !== after[0] && onGrid, `torus x ${after[0]} → ${grid[0]} mm`);
    await page.keyboard.press('Control+z');
    const undone = findIn((await doc(page)).root, ids.torus).transform.translate;
    await check('A4', 'A gizmo drag is a single undo step', undone[0] === after[0], `after Ctrl Z: x ${undone[0]} mm (expected ${after[0]})`);
  }

  // ---- A3 modifiers on a group -----------------------------------------------------------
  await selectRow(page, ids.box);
  await page.keyboard.press('Control+g');
  const groupId = await selected(page);
  {
    const g = findIn((await doc(page)).root, groupId);
    await check('A3', 'Ctrl G wraps the box in a group', g?.kind.type === 'group' && g.kind.children.some((c) => c.id === ids.box), `group ${groupId}: ${g?.kind.children?.map((c) => c.id).join(', ')}`);
  }
  // Hide the other shapes so the probes only see the group.
  for (const id of [ids.sphere, ids.cylinder, ids.capsule, ids.torus, ids.extrude]) {
    await selectRow(page, id);
    await page.keyboard.press('h');
  }
  await selectRow(page, groupId);
  const addMod = async (t) => {
    await page.locator('[data-action="add-modifier"]').click();
    await page.locator(`[data-testid="mod-${t}"]`).click();
  };
  const removeLastMod = async () => {
    await page.locator('.card[data-modifier] button[title="Remove modifier"]').last().click();
  };
  {
    await addMod('shell');
    const [centre, wall] = await field(page, [
      [0, 0, 10],
      [0, 0, 1],
    ]);
    await check('A3', 'Shell 2 mm inward hollows the box', centre > 0 && wall < 0, `centre ${centre.toFixed(2)} mm, inside the bottom wall ${wall.toFixed(2)} mm`);
    await removeLastMod();
  }
  {
    await addMod('offset');
    const [p] = await field(page, [[10.5, 0, 10]]);
    await check('A3', 'Offset 1 mm grows the box by 1 mm', Math.abs(p - -0.5) < 1e-6, `field 0.5 mm outside the original face: ${p.toFixed(4)} mm (expect −0.5)`);
    await removeLastMod();
  }
  {
    await selectRow(page, ids.box);
    await setVec(page, 'position', [20, 0, 10]);
    await selectRow(page, groupId);
    await addMod('mirror');
    const [m] = await field(page, [[-20, 0, 10]]);
    await check('A3', 'Mirror X copies the box to the other side', m < 0, `field at the mirrored centre (−20, 0, 10): ${m.toFixed(2)} mm`);
    await removeLastMod();
  }
  {
    await addMod('linear');
    const v = await field(page, [
      [40, 0, 10],
      [60, 0, 10],
      [80, 0, 10],
    ]);
    await check('A3', 'Linear pattern ×3 at 20 mm', v[0] < 0 && v[1] < 0 && v[2] > 0, `copies at x 40, 60: ${v[0].toFixed(2)}, ${v[1].toFixed(2)} mm; beyond (x 80): ${v[2].toFixed(2)} mm`);
    await removeLastMod();
  }
  {
    await addMod('circular');
    const a = (60 * Math.PI) / 180;
    const v = await field(page, [[20 * Math.cos(a), 20 * Math.sin(a), 10]]);
    await check('A3', 'Circular pattern ×6 about Z', v[0] < 0, `copy at 60°: ${v[0].toFixed(2)} mm`);
  }

  // ---- A5 tree: rename, reorder, suppress, delete ------------------------------------------
  {
    await page.locator(`.row[data-node="${ids.torus}"] .row-name`).dblclick();
    const diag = await page.evaluate(() => ({ renaming: window.fieldsmith.ui.renaming, active: document.activeElement?.tagName }));
    await page.keyboard.press('Control+a');
    await page.keyboard.type('ring');
    await page.keyboard.press('Enter');
    const d1 = await doc(page);
    const renamed = !!findIn(d1.root, 'ring') && !findIn(d1.root, ids.torus);
    const rid = renamed ? 'ring' : ids.torus;
    await page.locator(`.row[data-node="${rid}"]`).dragTo(page.locator(`.row[data-node="${groupId}"]`), { targetPosition: { x: 60, y: 3 } });
    const d2 = await doc(page);
    const first = d2.root[0]?.id;
    await selectRow(page, rid);
    const supBefore = findIn(d2.root, rid).suppressed;
    await page.locator(`.row[data-node="${rid}"] button.mini`).click();
    const supAfter = findIn((await doc(page)).root, rid).suppressed;
    await selectRow(page, ids.extrude);
    await page.keyboard.press('Delete');
    const deleted = !findIn((await doc(page)).root, ids.extrude);
    await check(
      'A5',
      'Tree: rename (double-click), reorder (drag), suppress (eye), delete (Del)',
      renamed && first === 'ring' && supBefore !== supAfter && deleted,
      `renamed ${renamed} (renaming ${diag.renaming}, focus ${diag.active}); first row after drag: ${first}; suppressed ${supBefore} → ${supAfter}; extrude deleted ${deleted}`,
    );
  }
  // ---- A5 undo / redo everything -----------------------------------------------------------
  {
    const finalText = await canonical(page);
    let undos = 0;
    await page.locator('.viewport').click({ position: { x: 5, y: 5 } }).catch(() => {});
    await page.keyboard.press('Escape');
    while (await page.evaluate(() => window.fieldsmith.store.canUndo())) {
      await page.keyboard.press('Control+z');
      undos += 1;
      if (undos > 400) break;
    }
    const emptied = (await doc(page)).root.length === 0;
    let redos = 0;
    while (await page.evaluate(() => window.fieldsmith.store.canRedo())) {
      await page.keyboard.press('Control+Shift+z');
      redos += 1;
      if (redos > 400) break;
    }
    const restored = (await canonical(page)) === finalText;
    await check('A5', 'Every edit is undoable and redoable from the keyboard', emptied && restored && undos === redos, `${undos} undos back to an empty document (${emptied}); ${redos} redos restore the final document exactly (${restored})`);
  }

  // ---- A6 .fsm round trip through Save and Open ---------------------------------------------
  {
    const before = await canonical(page);
    const [dl] = await Promise.all([page.waitForEvent('download'), page.locator('[data-testid="save"]').click()]);
    const path = await dl.path();
    const text = readFileSync(path, 'utf8');
    await palette(page, 'New document');
    const blank = (await doc(page)).root.length === 0;
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.keyboard.press('Control+o')]);
    await chooser.setFiles({ name: dl.suggestedFilename(), mimeType: 'text/plain', buffer: Buffer.from(text) });
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.length > 0);
    const after = await canonical(page);
    const readable = /^\s*box \w+/m.test(text) && /^\s*group \w+ \{/m.test(text);
    await check(
      'A6',
      'Save → New → Open restores the identical document; the file is readable text',
      before === after && blank && text === before && readable,
      `saved ${text.length} bytes as ${dl.suggestedFilename()}; identical after reopening: ${before === after}; file equals canonical print: ${text === before}; readable: ${readable}`,
    );
    writeFileSync(resolve(screens, 'shape-roundtrip.fsm'), text);
  }
  } catch (e) {
    checks.push({ area: "—", name: "Run aborted", ok: false, detail: String(e.message ?? e).split("\n")[0], file: "" });
    console.log("  shape test aborted; doc ids:", JSON.stringify((await doc(page).catch(() => ({ root: [] }))).root.map((n) => n.id)));
  }

  const ok = checks.every((c) => c.ok) && errors.length === 0;
  const lines = [
    '# Shape lens — headless UI acceptance run (GOAL.md section A)',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    '| Area | Check | Result | Measured | Screenshot |',
    '| :-- | :-- | :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.area} | ${c.name} | ${c.ok ? 'pass' : '**fail**'} | ${c.detail} | ${c.file} |`),
    '',
    `- Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
    '',
    'Geometry is verified by evaluating the document field with the Rust evaluator at probe points',
    '(negative = inside the solid), not by inspecting the UI state alone.',
    '',
    '---',
    '*Generated by `npm run test:e2e` (web/tests/shape.mjs) — do not edit by hand.*',
    '',
  ];
  writeFileSync(resolve(docsDir, 'shape-e2e.md'), lines.join('\n'));
  for (const c of checks) if (!c.ok) console.log(`  FAIL ${c.area} ${c.name}: ${c.detail}`);
  for (const e of errors) console.log(`  console error: ${e}`);
  await page.close();
  return ok;
}
