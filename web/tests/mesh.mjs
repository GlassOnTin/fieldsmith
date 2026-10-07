// Mesh handling e2e (GOAL.md milestone M6), driven through the real UI in a headless browser.
//
// The shipped fixture web/tests/fixtures/box.glb is a 20 mm cube authored in glTF metres with
// Y-up: the import bake centres it at the origin with z spanning ±10 mm, and its face order
// puts -Y first so the weld segmentation's first-triangle rule names patch 1 the doc-space
// bottom. Patch ids are still read from live picks rather than assumed. The flow a person
// would use: palette import, patch clicks verified with pickAt (a wrong pick cannot write the
// document), regions named and loaded/supported in the setup lens, save → new → open the
// .fsmz, and both solves reach ready.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

async function rename(page, name) {
  const el = page.locator('[data-testid="name"]');
  await el.click();
  await el.fill(name);
  await el.press('Enter');
}

async function palette(page, text) {
  await page.keyboard.press('Control+k');
  await page.locator('[data-testid="palette-input"]').fill(text);
  await page.keyboard.press('Enter');
}

const doc = (page) => page.evaluate(() => window.fieldsmith.store.doc);
const canonical = (page) =>
  page.evaluate(async () => {
    const w = await import('/src/doc/wasm.ts');
    await w.loadWasm();
    return w.printFsm(window.fieldsmith.store.doc);
  });
const runHas = async (page, fn, timeoutMs) => {
  try {
    await page.waitForFunction(fn, null, { timeout: timeoutMs, polling: 50 });
    return true;
  } catch {
    return false;
  }
};

export async function meshTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 }, acceptDownloads: true });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') {
      const loc = m.location();
      const file = loc.url.replace(url, '') || loc.url;
      errors.push(`${m.text()} at ${file}:${loc.lineNumber}`);
    }
  });
  page.on('pageerror', (e) => errors.push(String(e).split('\n')[0]));
  const checks = [];
  let shot = 0;
  const check = async (area, name, ok, detail = '') => {
    shot += 1;
    const file = `mesh-${String(shot).padStart(2, '0')}.png`;
    await page.waitForTimeout(120);
    await page.screenshot({ path: resolve(screens, file) });
    checks.push({ area, name, ok: !!ok, detail, file });
  };
  /** Candidates inside the z = `plane` face (the cube spans ±10 in x and y), verified by
   * pickAt: the first visible hit that names the node with a patch id on that plane is the
   * face's patch. */
  const clickPatch = async (node, plane) => {
    const hit = await page.evaluate(
      ([sh, at]) => {
        const F = window.fieldsmith;
        for (const q of [[-6, -6, at], [6, -6, at], [6, 6, at], [-6, 6, at], [0, 0, at]]) {
          const s = F.view.project(q);
          if (!s) continue;
          const h = F.pickAt(s.x, s.y);
          if (h && h.shape === sh && h.patch != null && Math.abs(h.point[2] - at) < 0.5) return { patch: h.patch, x: s.x, y: s.y, point: h.point };
        }
        return null;
      },
      [node, plane],
    );
    if (!hit) throw new Error(`no visible patch of ${node} on the z = ${plane} plane to click`);
    await page.mouse.click(hit.x, hit.y);
    return hit;
  };
  /** View from `dir` and frame the part before picking. */
  const aim = async (dir) => {
    await page.evaluate((d) => {
      window.fieldsmith.view.view(d);
      window.fieldsmith.view.fit(false);
    }, dir);
  };

  await page.goto(url);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForSelector('[data-testid="empty-state"]');

  const docsReport = resolve(docsDir, 'mesh-e2e.md');
  try {
    // ---- import the shipped GLB -----------------------------------------------------------
    const fixture = readFileSync(new URL('./fixtures/box.glb', import.meta.url));
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), palette(page, 'Import mesh')]);
    await chooser.setFiles({ name: 'box.glb', mimeType: 'model/gltf-binary', buffer: fixture });
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.some((n) => n.kind.type === 'mesh'));
    let d = await doc(page);
    const mesh = d.root[0];
    const hashOk = /^[0-9a-f]{16}$/.test(mesh.kind.hash) && mesh.kind.type === 'mesh';
    await check(
      'import',
      'GLB fixture arrives as a mesh node named from the glTF mesh',
      mesh.id === 'refcube' && hashOk && mesh.transform.translate.join() === '0,0,0',
      `id ${mesh.id}, src ${mesh.kind.src}, units ${mesh.kind.units}, up ${mesh.kind.up}, tris ${mesh.kind.tris}, patches ${mesh.kind.patches}, open edges ${mesh.kind.open_edges}, voxel ${mesh.kind.voxel} mm, hash ${mesh.kind.hash}`,
    );
    const gotBbox = mesh.kind.bbox.map((v, i) => v - [-10, -10, -10, 10, 10, 10][i]);
    await check('import', 'the baked part is the 20 mm cube at the origin', mesh.kind.bbox.every((v, i) => Math.abs(gotBbox[i]) < 0.6), `bbox ${mesh.kind.bbox.map((v) => v.toFixed(1)).join(' ')}`);
    // evalBody packs [d, gx, gy, gz] per point: the distances are at 0, 4, 8.
    const vol = await page.evaluate(async () => {
      const w = await import('/src/doc/wasm.ts');
      await w.loadWasm();
      return w.evalBody(window.fieldsmith.store.doc, new Float64Array([0, 0, 0, 0, 0, 20, 0, 0, -20]));
    });
    await check('import', 'the field is solid inside and vacuum outside', vol[0] < 0 && vol[4] > 0 && vol[8] > 0, `inside ${vol[0].toFixed(2)} mm, above ${vol[4].toFixed(2)}, below ${vol[8].toFixed(2)}`);

    // ---- setup lens: patch clicks build regions -------------------------------------------
    await page.locator('[data-lens="setup"]').click();
    await page.waitForSelector('[data-testid="add-support"]', { timeout: 5000 });
    await page.waitForSelector('[data-testid="setup-block"]', { timeout: 5000 });

    await aim([0, 0, -1]);
    const bottom = await clickPatch(mesh.id, -10);
    await rename(page, 'bottom');
    await check('setup', 'the bottom face click names a patch of the mesh', bottom.patch >= 1, `patch ${bottom.patch} at (${bottom.point.map((v) => v.toFixed(1)).join(', ')}) mm`);

    await page.keyboard.press('Escape');
    await aim([0, 0, 1]);
    const topHit = await clickPatch(mesh.id, 10);
    if (topHit.patch === bottom.patch) {
      // Same id on the opposite plane would mean the segmentation merged the faces.
      throw new Error(`the top and bottom plane clicks landed on the same patch ${topHit.patch}`);
    }
    await rename(page, 'top');
    await check('setup', 'the top face is a different patch, kept on its own region', topHit.patch !== bottom.patch, `patch ${topHit.patch}, bottom ${bottom.patch}`);

    d = await doc(page);
    const selText = (r) => JSON.stringify(d.regions.find((x) => x.id === r)?.selectors);
    const want = (p) => JSON.stringify([{ shape: mesh.id, face: 'all', patch: p }]);
    await check(
      'setup',
      'each region carries the single .p selector of its verified click',
      d.regions.length === 2 && selText('bottom') === want(bottom.patch) && selText('top') === want(topHit.patch),
      `${d.regions.map((r) => `${r.id} ${selText(r.id)}`).join(' ; ')}`,
    );

    await page.locator('[data-testid="region-bottom"]').click();
    await page.locator('[data-testid="add-support"]').click();
    await page.locator('[data-testid="region-top"]').click();
    await page.locator('[data-testid="add-load"]').click();
    d = await doc(page);
    await check(
      'setup',
      'support on bottom, force load on top defaults down 100 N',
      d.supports.length === 1 && d.supports[0].region === 'bottom' && d.supports[0].fixed.join() === 'true,true,true' &&
        d.loads.length === 1 && d.loads[0].region === 'top' && d.loads[0].kind.type === 'force' && JSON.stringify(d.loads[0].kind.vector) === '[0,0,-100]',
      `supports ${JSON.stringify(d.supports.map((s) => s.region))}, loads ${JSON.stringify(d.loads.map((l) => [l.region, l.kind.vector]))}`,
    );

    // ---- solve ----------------------------------------------------------------------------
    const coarseSeen = await runHas(page, () => window.fieldsmith.analysis.state.levels.length >= 1, 20000);
    await runHas(page, () => ['ready', 'error'].includes(window.fieldsmith.analysis.state.status), 60000);
    const a = await page.evaluate(() => window.fieldsmith.analysis.state);
    const lvl = a.levels[a.levels.length - 1];
    const cases = lvl ? lvl.meta.cases : [];
    await check('solve', 'the mesh document solves to ready', coarseSeen && a.status === 'ready' && a.levels.length >= 1, `${a.status}${a.error ? `: ${a.error}` : ''}; levels ${JSON.stringify(a.levels.map((l) => l.label))}${a.skippedFine ? `, refined skipped: ${a.skippedFine}` : ''}`);
    await check('solve', 'the last level carries a stress and a displacement, and matches the document', a.resultsFor === (await page.evaluate(() => window.fieldsmith.store.docVersion)) && cases.some((c) => c.max_vm_elem > 0 && c.max_displacement > 0), cases.length ? cases.map((c) => `${c.max_vm_elem.toFixed(1)} MPa, ${c.max_displacement.toFixed(4)} mm`).join(' / ') : 'no cases');

    // ---- save → new → open the .fsmz ------------------------------------------------------
    const before = await canonical(page);
    const [dn] = await Promise.all([page.waitForEvent('download'), page.locator('[data-testid="save"]').click()]);
    const bytes = readFileSync(await dn.path());
    const zipMagic = bytes[0] === 0x50 && bytes[1] === 0x4b && bytes[2] === 0x03;
    await check('save', 'a mesh document saves as a .fsmz ZIP sidecar', zipMagic && dn.suggestedFilename().endsWith('.fsmz'), `${dn.suggestedFilename()}, ${bytes.length} bytes, starts ${bytes.slice(0, 4).toString('hex')}`);

    await palette(page, 'New document');
    const blank = (await doc(page)).root.length === 0;
    const [chooser2] = await Promise.all([page.waitForEvent('filechooser'), page.keyboard.press('Control+o')]);
    await chooser2.setFiles({ name: dn.suggestedFilename(), mimeType: 'application/zip', buffer: bytes });
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.some((n) => n.kind.type === 'mesh'));
    d = await doc(page);
    const reopened = d.root[0];
    const textAfter = await canonical(page);
    await check('open', 'File → Open of the .fsmz restores the same document', blank && reopened.kind.hash === mesh.kind.hash && textAfter === before, `mesh hash ${reopened.kind.hash}; canonical text identical: ${textAfter === before}`);

    const gridsBack = await page
      .waitForFunction(async ([h]) => (await import('/src/doc/meshcache.ts')).meshcache.has(h), [mesh.kind.hash], { timeout: 5000, polling: 50 })
      .then(() => true)
      .catch(() => false);
    await aim([0, 0, -1]);
    const stillPicks = await page.evaluate(
      ([pid]) => {
        const F = window.fieldsmith;
        for (const q of [[-6, -6, -10], [0, 0, -10]]) {
          const s = F.view.project(q);
          if (!s) continue;
          const h = F.pickAt(s.x, s.y);
          if (h && h.shape === 'refcube' && h.patch === pid) return true;
        }
        return false;
      },
      [bottom.patch],
    );
    await check('open', 'the mesh grid rehydrates and the same patch is still pickable', gridsBack && stillPicks, `meshcache.has(…) → ${gridsBack}; patch ${bottom.patch} pick → ${stillPicks}`);

    // ---- re-solve -------------------------------------------------------------------------
    const reRun = await runHas(
      page,
      () => {
        const s = window.fieldsmith.analysis.state;
        return s.status === 'ready' && s.levels.length >= 1 && s.resultsFor === window.fieldsmith.store.docVersion;
      },
      60000,
    );
    const afterState = await page.evaluate(() => {
      const s = window.fieldsmith.analysis.state;
      return `status ${s.status}, levels ${JSON.stringify(s.levels.map((l) => l.label))}`;
    });
    await check('open', 'the reopened document re-solves without a fresh import', reRun, afterState);

    // ---- the region's hand-built face menu (mesh-aware) ----------------------------------
    // Run last so nothing above the save/open round trip sees the edited document.
    await page.locator('[data-testid="region-bottom"]').click();
    await page.waitForSelector('[data-testid="add-face-shape"]', { timeout: 5000 });
    const faceOpts = await page.locator('[data-testid="add-face-face"] option').evaluateAll((os) => os.map((o) => o.textContent));
    const shapeOpts = await page.locator('[data-testid="add-face-shape"] option').evaluateAll((os) => os.map((o) => o.textContent));
    await check(
      'setup',
      'the face menu on a mesh shape offers its faces and patches, without `side`',
      !faceOpts.includes('Side') && /Patch 1/.test(faceOpts.join('|')) && shapeOpts.includes(mesh.id),
      `face options: ${faceOpts.join(', ')}`,
    );
    // Add a patch selector by hand that the region does not carry yet: pick the first
    // `p:<n>` option the region's selectors don't already name, add it, and read it back.
    const optionVals = await page
      .locator('[data-testid="add-face-face"] option')
      .evaluateAll((os) => os.map((o) => String(o.value)));
    const regionSel = await page.evaluate(() => window.fieldsmith.store.doc.regions.find((r) => r.id === 'bottom').selectors);
    const taken = new Set(regionSel.map((s) => s.patch ?? -1));
    const freeTag = optionVals.find((v) => v.startsWith('p:') && !taken.has(Number(v.slice(2))));
    if (!freeTag) throw new Error(`the face menu offers no patch the region ${JSON.stringify([...taken])} doesn't already carry`);
    const free = Number(freeTag.slice(2));
    await page.locator('[data-testid="add-face-face"]').selectOption(freeTag);
    await page.locator('[data-testid="add-face"]').click();
    d = await doc(page);
    const bottomNow = d.regions.find((r) => r.id === 'bottom');
    const hand = bottomNow.selectors[bottomNow.selectors.length - 1];
    await check(
      'setup',
      'a patch selector added by hand joins the region',
      JSON.stringify(hand) === JSON.stringify({ shape: mesh.id, face: 'all', patch: free }),
      `last selector of the bottom region is ${JSON.stringify(hand)}; the region carries ${bottomNow.selectors.length} faces`,
    );
  } catch (e) {
    checks.push({ area: '—', name: 'Run aborted', ok: false, detail: String(e.message ?? e).split('\n')[0], file: '' });
    console.log('  mesh test aborted:', String(e).split('\n')[0]);
  }

  const ok = checks.every((c) => c.ok) && errors.length === 0;
  const lines = [
    '# Mesh handling — headless UI acceptance run (GOAL.md milestone M6)',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    '| Area | Check | Result | Measured | Screenshot |',
    '| :-- | :-- | :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.area} | ${c.name} | ${c.ok ? 'pass' : '**fail**'} | ${c.detail} | ${c.file} |`),
    '',
    `- Console errors: **${errors.length}**${errors.length ? '\n' + errors.map((e) => `  - ${e}`).join('\n') : ''}`,
    '',
    'The fixture `web/tests/fixtures/box.glb` is a 20 mm cube authored in glTF metres, Y-up.',
    'Patch clicks are each verified with `pickAt` before the click lands, so a wrong pick aborts',
    'the run instead of writing a wrong region. Saves and opens go through the real file input.',
    '',
    '---',
    '*Generated by `npm run test:e2e` (web/tests/mesh.mjs) — do not edit by hand.*',
    '',
  ];
  writeFileSync(docsReport, lines.join('\n'));
  for (const c of checks) if (!c.ok) console.log(`  FAIL ${c.area} ${c.name}: ${c.detail}`);
  for (const e of errors) console.log(`  console error: ${e}`);
  await page.close();
  return ok;
}