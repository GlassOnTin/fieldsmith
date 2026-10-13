// Budget probe for GOAL.md's browser budgets on examples/bracket-filled.fsm, 1080p:
//   edit → coarse stress visible ≤ 1.5 s; edit → refined (P2) ≤ 10 s;
//   main-thread long tasks during a solve: none > 50 ms;
//   preview frame rate ≥ 60 fps; cold load, compressed ≤ 3 MB.
// Passes: headless (what CI sees — its GPU-process frame pipeline has a known
// main-thread-stall artifact around presented frames) and, if a desktop's Xwayland
// socket is reachable, headed on the real display: the setting GOAL's budget table
// names, and the referee for the long-task and fps rows. Writes
// docs/verification/budgets.md with every number. Exits non-zero on a headed failure.
// The export budget (3MF of the graded bracket at 0.1 mm ≤ 15 s) is measured through the
// Make lens in the last section of each pass: mesh time from the export meta, write time
// from the download click. The native mesh+3MF cross-check comes from adaptive_probe.rs.
// A mesh-doc section (M6) then imports the shipped GLB fixture through the real file
// chooser and runs the same Make-lens export on the imported part, so the mesh-doc
// pipeline has one timing row per pass too; budget-scale mesh numbers are in V10.md.
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { gzipSync, inflateRawSync } from 'node:zlib';
import { cpus, totalmem, platform } from 'node:os';
import { resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..', '..');
/* CLI: --browser chromium|firefox (default chromium), --out FILE (default the referee
 * doc). A laptop/Firefox pass aims them at its own file, e.g. budgets-laptop.md; the
 * default run regenerates docs/verification/budgets.md unchanged. */
const argv = process.argv.slice(2);
const opt = (name, dflt) => {
  const i = argv.indexOf(name);
  return i >= 0 && i + 1 < argv.length ? argv[i + 1] : dflt;
};
const browserName = opt('--browser', 'chromium');
const outPathArg = opt('--out', null);
const outPath = resolve(ROOT, outPathArg ?? 'docs/verification/budgets.md');
const { execSync } = await import('node:child_process');
execSync('npm run build', { cwd: resolve(ROOT, 'web'), stdio: 'inherit' });
const server = await createServer({ root: resolve(ROOT, 'web'), logLevel: 'error', server: { port: 0, strictPort: false } });
await server.listen();
const url = server.resolvedUrls.local[0];
const docText = readFileSync(resolve(ROOT, 'examples/bracket-filled.fsm'), 'utf8');

// A desktop session's Xwayland auth cookie → a headed reference pass is possible.
// The cookie file's suffix rotates across desktop sessions; glob for the current one.
const cookie = (function () {
  const dir = `/run/user/${process.getuid()}`;
  const found = readdirSync(dir).find((f) => f.startsWith('xauth_'));
  return found ? resolve(dir, found) : null;
})();
const xauth = cookie !== null && existsSync(cookie);
const modes = ['headless', ...(xauth ? ['headed'] : [])];


/* Cold load: the compressed transfer the production build sends. Every byte of dist/*
 * with standard gzip static hosting — the honest stand-in for Content-Encoding. */
const distDir = resolve(ROOT, 'web/dist');
const dist = (function list(dir) {
  return [...readdirSync(dir, { withFileTypes: true }).flatMap((d) => {
    const p = resolve(dir, d.name);
    return d.isDirectory() ? list(p) : [p];
  })];
})(distDir).filter((p) => !p.endsWith('.map'));
// dist carries two wasm builds (threaded and single-thread); a page fetches exactly one of
// them, so the smaller is left out — the count is the threaded page's, the heavier variant.
const gz = dist.map((p) => ({ p, n: gzipSync(readFileSync(p)).length }));
const wasms = gz.filter((f) => f.p.endsWith('.wasm')).sort((a, b) => a.n - b.n);
const skipped = wasms.length > 1 ? wasms[0] : null;
const coldKb = Math.round(gz.reduce((a, f) => a + (f === skipped ? 0 : f.n), 0) / 1024);
console.log(`cold load: ${coldKb} kB gzip over ${dist.length - (skipped ? 1 : 0)} files` +
  (skipped ? ` (the other wasm variant, ${Math.round(skipped.n / 1024)} kB, is never fetched with it)` : ''));

/** The machine the numbers belong to: budget timings are this host's facts, not a laptop's. */
const machine = `${cpus()[0].model.trim()}, ${cpus().length} hardware threads, ` +
  `${Math.round(totalmem() / 2 ** 30)} GiB RAM, ${platform()}`;

/** Model entry of a self-produced 3MF package: EOCD → central directory → inflate. The
 * triangle count walks the inflated buffer with indexOf at 0.1 mm scale (a full-length UTF-8
 * decode would exceed V8's string limit at this size). */
function threeMfCheck(bytes) {
  const i = bytes.lastIndexOf(Buffer.from('PK\x05\x06'));
  const count = bytes.readUInt16LE(i + 10);
  let p = bytes.readUInt32LE(i + 16);
  const names = [];
  let tris = 0;
  for (let k = 0; k < count; k++) {
    const csize = bytes.readUInt32LE(p + 20);
    const usize = bytes.readUInt32LE(p + 24);
    const nlen = bytes.readUInt16LE(p + 28);
    const elen = bytes.readUInt16LE(p + 30);
    const clen = bytes.readUInt16LE(p + 32);
    const name = bytes.slice(p + 46, p + 46 + nlen).toString();
    const lho = bytes.readUInt32LE(p + 42);
    const noff = lho + 30 + bytes.readUInt16LE(lho + 26) + bytes.readUInt16LE(lho + 28);
    names.push(name);
    if (name.endsWith('.model')) {
      const body = inflateRawSync(bytes.subarray(noff, noff + csize));
      if (body.length !== usize) throw new Error(`${name}: inflated ${body.length} ≠ ${usize}`);
      let at = 0;
      for (;;) {
        const j = body.indexOf('<triangle ', at);
        if (j < 0) break;
        tris++;
        at = j + 1;
      }
    }
    p += 46 + nlen + elen + clen;
  }
  return { names, tris };
}

const rows = [];
const notes = [];
let headedOk = modes.includes('headed');

for (const mode of modes) {
  let browser = null;
  try {
    if (browserName === 'firefox') {
      const { firefox } = await import('playwright');
      browser = await firefox.launch({
        headless: mode !== 'headed',
        env: mode === 'headed' && cookie ? { ...process.env, DISPLAY: ':0', XAUTHORITY: cookie } : process.env,
      });
    } else if (mode === 'headed') {
      browser = await chromium.launch({
        headless: false,
        channel: 'chrome',
        // The page viewport stays 1920×1080; the window args only keep the run fully on
        // screen (the referee's right monitor), instead of an undecorated window floating
        // off the top where the desktop session maps Playwright's default.
        args: ['--use-angle=vulkan', '--no-sandbox', '--window-position=2280,300', '--window-size=1920,1100'],
        env: { ...process.env, DISPLAY: ':0', XAUTHORITY: cookie },
      });
    } else {
      browser = await chromium.launch({ headless: true, args: ['--use-angle=vulkan', '--no-sandbox'] });
    }
  } catch {
    try {
      browser = await chromium.launch({ headless: mode !== 'headed', channel: 'chrome' });
      if (mode === 'headed') browser = null;
    } catch {
      /* nothing */
    }
  }
  if (!browser) {
    notes.push(`${mode} pass: browser unavailable, skipped`);
    if (mode === 'headed') headedOk = false;
    continue;
  }
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  const consoleErrors = [];
  page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
  await page.goto(url);

  // Cold load was measured above against the production build; this pass exercises the
  // dev-server app for the interaction budgets. The reload clears the cache-biased state.
  await page.waitForSelector('[data-testid="empty-state"]', { timeout: 30000 });
  // Long-task observer + 1 ms interval probe (records any main-thread absence > 25 ms).
  await page.evaluate(() => {
    const w = window;
    w.__lt = [];
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) w.__lt.push([Math.round(e.startTime), Math.round(e.duration)]);
    }).observe({ type: 'longtask', buffered: false });
    w.__gaps = [];
    let prev = performance.now();
    setInterval(() => {
      const now = performance.now();
      if (now - prev - 1 > 25) w.__gaps.push([Math.round(prev), Math.round(now - prev)]);
      prev = now;
    }, 1);
    w.__budgetFlush = (t0) => ({
      lt: w.__lt.filter((t) => t[0] >= t0 - 200).map((t) => [Math.round(t[0] - t0), t[1]]),
      gaps: w.__gaps.filter((g) => g[0] >= t0 - 200).map((g) => [Math.round(g[0] - t0), g[1]]),
    });
  });

  // Load the filled bracket through the same path the examples menu uses; the ladder
  // runs because the setup lens wants an analysis.
  await page.evaluate(async (text) => {
    const g = await import('/src/pkg/fieldsmith_wasm.js');
    await g.default();
    window.fieldsmith.store.setLens('setup');
    window.fieldsmith.store.load(JSON.parse(g.parse_fsm(text)).ok);
  }, docText);
  const tLoad = Date.now();
  await page.waitForFunction(
    () => window.fieldsmith.analysis.state.status === 'ready' &&
      window.fieldsmith.analysis.state.resultsFor === window.fieldsmith.store.docVersion &&
      window.fieldsmith.analysis.state.levels.length > 0,
    null,
    { timeout: 240000 },
  );
  const setupSolveMs = Date.now() - tLoad;
  await page.waitForTimeout(2000);

  // One undoable edit restarts the ladder, like every inspector edit.
  const timing = await page.evaluate(() => new Promise((res) => {
    const F = window.fieldsmith;
    const t0 = performance.now();
    window.__editT0 = t0;
    let coarse = null;
    const un = F.analysis.subscribe(() => {
      const s = F.analysis.state;
      const at = performance.now() - t0;
      if (s.status === 'running' && s.levels.length >= 1 && coarse === null) coarse = at;
      if (s.status === 'ready' && s.resultsFor !== null) {
        un();
        res({ coarse, ready: at });
      } else if (s.status === 'error') {
        un();
        res({ error: s.error, coarse });
      }
    });
    F.store.transact('Budget probe edit', (d) => { d.name = 'Budget probe'; });
  }));
  if (timing.error) {
    rows.push({ mode, label: 'edit ladder', ok: false, detail: `error: ${timing.error}` });
    await browser.close();
    continue;
  }

  // Long tasks and blocked windows over the edit + full solve.
  await page.waitForTimeout(1000);
  const ltInfo = await page.evaluate(() => window.__budgetFlush(window.__editT0));
  const worstLt = ltInfo.lt.length ? Math.max(...ltInfo.lt.map((e) => e[1])) : 0;

  // Presented frame rate: render continuously, as during an orbit, and count real renders.
  // A browser/environment with no WebGL context never attaches a viewport (ViewportHost's
  // effect throws before `hook.viewport = vp`); record the row unmeasurable, don't kill the run.
  await page.evaluate(() => window.fieldsmith.store.setLens('fill'));
  await page.waitForTimeout(400);
  let cad = null;
  try {
    cad = await page.evaluate(() => new Promise((res) => {
    const vp = window.fieldsmith.viewport;
    const R = vp.renderer;
    const cpu = [];
    const orig = R.render.bind(R);
    R.render = (...a) => {
      const t = performance.now();
      orig(...a);
      cpu.push(performance.now() - t);
    };
    let calls = 0;
    const t0 = performance.now();
    let prev = 0;
    const deltas = [];
    const tick = () => {
      const now = performance.now();
      const el = now - t0;
      if (el < 2000) {
        if (prev) deltas.push(now - prev);
        prev = now;
        calls++;
        vp.invalidate();
        requestAnimationFrame(tick);
      } else {
        cpu.sort((a, b) => a - b);
        deltas.sort((a, b) => a - b);
        res({ calls, wall: el, cpuMed: cpu[calls >> 1], deltaP95: deltas[Math.floor(deltas.length * 0.95)] });
      }
    };
    requestAnimationFrame(tick);
  }));
  } catch {}
  const fps = cad ? +(cad.calls / (cad.wall / 1000)).toFixed(1) : null;

  const check = (label, ok, detail) => rows.push({ mode, label, ok, detail });
  check('cold load (compressed) ≤ 3 MB', coldKb <= 3 * 1024, `${coldKb} kB`);
  check('edit → coarse ≤ 1500 ms', timing.coarse !== null && timing.coarse <= 1500,
    timing.coarse === null ? 'no coarse level observed' : `${Math.round(timing.coarse)} ms`);
  check('edit → refined ≤ 10000 ms', timing.ready <= 10000, `${Math.round(timing.ready)} ms`);
  check('main-thread long tasks > 50 ms: none', worstLt <= 50,
    ltInfo.lt.length ? `longtasks ${JSON.stringify(ltInfo.lt)}; timer gaps ${JSON.stringify(ltInfo.gaps)}` : 'none; no timer gaps');
  // A 60 Hz vsync display presents at most ~59.7 fps across a 2 s window; hold the
  // cadence at ≥ 59 with a healthy frame-arrival p95 instead of demanding > 60.
  if (cad) {
    check('presented preview ≥ 59 fps at 1080p (60 Hz vsync; fill lens)',
      fps >= 59 && cad.deltaP95 <= 17.5,
      `${fps} fps, frame-arrival p95 ${cad.deltaP95?.toFixed(1)} ms, render CPU med ${cad.cpuMed?.toFixed(2)} ms`);
  } else {
    check('presented preview ≥ 59 fps at 1080p (60 Hz vsync; fill lens)', false,
      'unmeasurable: no WebGL context in this browser/environment; the viewport never attached');
  }

  // The semi-transparent preview row (M12): the same presented cadence with the body at
  // 35 % opacity — the interior pass mounted underneath a transparent body. The flip is an
  // async setField (program-cache rebuild); wait for its compileMs to change before timing.
  let cad35 = null;
  try {
    const prev = await page.evaluate(() => window.fieldsmith.ui.compileMs);
    // The slider is an <input type="range">, which fill() does not drive; dispatch the
    // input event directly the way a drag does.
    await page.evaluate((v) => {
      const el = document.querySelector('[data-testid="opacity-slider"]');
      el.value = String(v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, 0.35);
    await page.waitForFunction(
      (p) => window.fieldsmith.ui.compileMs !== p && window.fieldsmith.ui.compileMs > 0,
      prev,
      { timeout: 30000 },
    );
    await page.waitForTimeout(300);
    cad35 = await page.evaluate(() => new Promise((res) => {
    const vp = window.fieldsmith.viewport;
    const R = vp.renderer;
    const cpu = [];
    const orig = R.render.bind(R);
    R.render = (...a) => {
      const t = performance.now();
      orig(...a);
      cpu.push(performance.now() - t);
    };
    let calls = 0;
    const t0 = performance.now();
    let prev2 = 0;
    const deltas = [];
    const tick = () => {
      const now = performance.now();
      const el = now - t0;
      if (el < 2000) {
        if (prev2) deltas.push(now - prev2);
        prev2 = now;
        calls++;
        vp.invalidate();
        requestAnimationFrame(tick);
      } else {
        cpu.sort((a, b) => a - b);
        deltas.sort((a, b) => a - b);
        res({ calls, wall: el, cpuMed: cpu[calls >> 1], deltaP95: deltas[Math.floor(deltas.length * 0.95)] });
      }
    };
    requestAnimationFrame(tick);
  }));
  } catch (e) {
    notes.push(`semi-transparent row: probe error ${String(e).split('\n')[0]}`);
  }
  const fps35 = cad35 ? +(cad35.calls / (cad35.wall / 1000)).toFixed(1) : null;
  if (cad35) {
    check('presented preview ≥ 59 fps at 1080p, body at 35 % (see-through fill)',
      fps35 >= 59 && cad35.deltaP95 <= 17.5,
      `${fps35} fps, frame-arrival p95 ${cad35.deltaP95?.toFixed(1)} ms, render CPU med ${cad35.cpuMed?.toFixed(2)} ms`);
  } else {
    check('presented preview ≥ 59 fps at 1080p, body at 35 % (see-through fill)', false,
      'unmeasurable: no WebGL context in this browser/environment; the viewport never attached');
  }
  try {
    await page.evaluate(() => {
      const el = document.querySelector('[data-testid="opacity-slider"]');
      el.value = '1';
      el.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await page.waitForTimeout(200);
  } catch {
    /* the export section below does not depend on the body alpha */
  }

  // ---- Export budget: 3MF of the graded bracket at 0.1 mm, through the real Make UI ----
  // GOAL's box is mesh + write; the click→meta wall also carries the health scans (not in
  // the budget), and those wall times are in the detail. An abort (OOM at this spacing)
  // lands on the exportError state and fails the row instead of killing the probe.
  try {
    await page.evaluate(() => {
      window.fieldsmith.store.setLens('make');
      // The export-done effect otherwise requests a layer slice in the worker, which
      // would queue ahead of the bytes round-trip and bloat the timed write.
      window.fieldsmith.ui.set('makeSliceOn', false);
    });
    await page.locator('[data-testid="make-h"] button[data-value="0.1"]').click();
    const tExport = Date.now();
    await page.locator('[data-testid="make-export"]').click();
    await page.waitForFunction(() => {
      const s = window.fieldsmith.analysis.state;
      return s.export instanceof Object || s.exportError !== null;
    }, null, { timeout: 600000, polling: 250 });
    const wallS = (Date.now() - tExport) / 1000;
    const es = await page.evaluate(() => {
      const s = window.fieldsmith.analysis.state;
      return { export: s.export, exportError: s.exportError };
    });
    if (es.exportError) {
      check('export 3MF at h 0.1: mesh + write ≤ 15 s', false, `export error: ${es.exportError}`);
    } else {
      const meta = es.export.meta;
      const t0 = Date.now();
      const [dl] = await Promise.all([
        page.waitForEvent('download', { timeout: 120000 }),
        page.locator('[data-testid="make-download-3mf"]').click(),
      ]);
      const bytes = readFileSync(await dl.path());
      const writeS = (Date.now() - t0) / 1000;
      const pkg = threeMfCheck(bytes);
      const budgetS = (meta.mesh_ms + writeS * 1000) / 1000;
      check('export 3MF at h 0.1: mesh + write ≤ 15 s',
        budgetS <= 15 && pkg.tris === meta.tris,
        `${budgetS.toFixed(1)} s (mesh ${(meta.mesh_ms / 1000).toFixed(1)}, write ${writeS.toFixed(1)}; ` +
        `click→meta ${wallS.toFixed(0)} s incl. scan ${(meta.scan_ms / 1000).toFixed(0)} s; ` +
        `${(bytes.length / 1048576).toFixed(1)} MB, ${pkg.tris} tris vs ${meta.tris})`);
    }
  } catch (e) {
    check('export 3MF at h 0.1: mesh + write ≤ 15 s', false, `probe error: ${e}`);
  }

  // ---- Mesh-doc rows (M6): import pipeline, then the same Make-lens 3MF -----------------
  // The import palette opens, the file chooser settles and the mesh node lands in the
  // document — the wall carries read, weld, segment and the worker's voxelize. The new
  // document clears the bracket export (a superseded version is nulled on sync), so the
  // export wait sees only the fresh object.
  const palette = async (text) => {
    await page.keyboard.press('Control+k');
    await page.locator('[data-testid="palette-input"]').fill(text);
    await page.keyboard.press('Enter');
  };
  try {
    await palette('New document');
    await page.waitForFunction(() => window.fieldsmith.store.doc.root.length === 0, null, { timeout: 10000 });
    await page.evaluate(() => window.fieldsmith.store.setLens('make'));
    const fixture = readFileSync(new URL('./fixtures/box.glb', import.meta.url)); // Node Buffer
    const tImport = Date.now();
    const [chooser] = await Promise.all([page.waitForEvent('filechooser'), palette('Import mesh')]);
    await chooser.setFiles({ name: 'box.glb', mimeType: 'model/gltf-binary', buffer: fixture });
    await page.waitForFunction(
      () => window.fieldsmith.store.doc.root.some((n) => n.kind.type === 'mesh'),
      null,
      { timeout: 60000 },
    );
    const importMs = Date.now() - tImport;
    await page.waitForTimeout(400);
    check('mesh doc import (box.glb fixture) ≤ 1 s', importMs <= 1000,
      `${importMs} ms (file chooser → node in the document; weld + segment + voxelize in the worker)`);

    const tExport = Date.now();
    await page.locator('[data-testid="make-h"] button[data-value="0.1"]').click();
    await page.locator('[data-testid="make-export"]').click();
    await page.waitForFunction(() => {
      const s = window.fieldsmith.analysis.state;
      return s.export instanceof Object || s.exportError !== null;
    }, null, { timeout: 600000, polling: 250 });
    const wallS = (Date.now() - tExport) / 1000;
    const es = await page.evaluate(() => {
      const s = window.fieldsmith.analysis.state;
      return { export: s.export, exportError: s.exportError };
    });
    if (es.exportError) {
      check('mesh doc export 3MF at h 0.1: mesh + write ≤ 15 s', false, `export error: ${es.exportError}`);
    } else {
      const meta = es.export.meta;
      const t0 = Date.now();
      const [dl] = await Promise.all([
        page.waitForEvent('download', { timeout: 120000 }),
        page.locator('[data-testid="make-download-3mf"]').click(),
      ]);
      const bytes = readFileSync(await dl.path());
      const writeS = (Date.now() - t0) / 1000;
      const pkg = threeMfCheck(bytes);
      const budgetS = (meta.mesh_ms + writeS * 1000) / 1000;
      check('mesh doc export 3MF at h 0.1: mesh + write ≤ 15 s',
        budgetS <= 15 && pkg.tris === meta.tris,
        `${budgetS.toFixed(1)} s (mesh ${(meta.mesh_ms / 1000).toFixed(1)}, write ${writeS.toFixed(1)}; ` +
        `${(bytes.length / 1048576).toFixed(1)} MB, ${pkg.tris} tris vs ${meta.tris})`);
    }
  } catch (e) {
    check('mesh doc import (box.glb fixture) ≤ 1 s', false, `probe error: ${String(e).split('\n')[0]}`);
    check('mesh doc export 3MF at h 0.1: mesh + write ≤ 15 s', false, 'probe error (see the import row)');
  }
  const iso = await page.evaluate(() => ({ coi: crossOriginIsolated, hc: navigator.hardwareConcurrency }));
  notes.push(`${mode}: wasm ${iso.coi ? `threaded (cross-origin isolated, pool of ${iso.hc})` : 'single thread (page not cross-origin isolated)'}`);
  notes.push(`${mode}: setup solve ${setupSolveMs} ms; console errors ${consoleErrors.length}${consoleErrors.length ? `: ${consoleErrors.join(' | ')}` : ''}`);
  await browser.close();
}

/* ---- regenerate docs/verification/budgets.md ------------------------------------- */
await server.close();
const today = new Date().toISOString().slice(0, 10);
const md = [
  '# Performance budgets — `bracket-filled.fsm`, 1080p viewport',
  '',
  `**Generated by** \`web/tests/probe-budgets.mjs${argv.length ? ' ' + argv.join(' ') : ''}\`, run \`${today}\`.`,
  '',
  'The referee is the headed pass (GOAL.md names a real browser on a real display).',
  'The headless pass records what CI sees; headless Chromium holds the main thread on',
  'presented-frame back-pressure inside its GPU-process pipeline for ~100-250 ms after',
  'an edit even though the app JS costs ~0.5 ms per frame — see the note below.',
  '',
  '| Mode | Check | Result | Detail |',
  '| :-- | :-- | :-- | :-- |',
];
for (const r of rows) md.push(`| ${r.mode} | ${r.label} | ${r.ok ? 'pass' : 'FAIL'} | ${r.detail} |`);
md.push('');
for (const n of notes) md.push(`- ${n}`);
md.push('');
if (browserName === 'firefox') {
  md.push('Firefox note: the browser emits no `longtask` entries, so the long-task row');
  md.push('here reads "none" from that observer; the 25 ms timer-gap probe is what');
  md.push('records Firefox main-thread stalls.');
  md.push('');
}
md.push('Headless caveat, measured on the referee workstation (SwiftShader and ANGLE/Vulkan');
md.push('on an RTX 5090): headless Chromium reports one 123-236 ms long task per edit; a trace ties it to a');
md.push('single GPU-process `WebGL` command-buffer flush of 127-183 ms, with the renderer');
md.push('main thread waiting inside one umbrella rAF task. Hiding the raymarch quad, hiding');
md.push('overlays, forced GC and warm shader caches make no difference; the app JS costs');
md.push('0.2-0.6 ms per frame. The headed run on the same machine records no long tasks and');
md.push('a 59.9-60.0 fps presented cadence. The headless number is therefore a harness');
md.push('artifact, and the headed pass is the budget referee.');
md.push('');
md.push('Export budget (3MF of the graded bracket at 0.1 mm feature resolution, ≤ 15 s): the');
md.push('"export 3MF at h 0.1" row measures it through the real Make UI in Chrome — mesh time');
md.push('from the export meta, write time from the timed download click. The click→meta wall');
md.push('also carries the export health report (field-volume reference and defect scan), which');
md.push('the budget row does not include; both wall and report times are in the detail.');
const exportRows = rows.filter((r) => r.label.startsWith('export 3MF'));
md.push(exportRows.length && exportRows.every((r) => r.ok)
  ? `This run: the row passes in every pass above (${exportRows.map((r) => r.mode).join(', ')}).`
  : `This run: the row FAILS in ${exportRows.filter((r) => !r.ok).map((r) => r.mode).join(', ') || 'every pass (no row recorded)'}.`);
md.push('Defect classes and their classification: `docs/verification/V8.md`, section');
md.push('"Adaptive dual contouring (the browser export path)".');
md.push('');
md.push('Mesh-doc rows (M6): "mesh doc import" times the shipped 12-triangle GLB fixture');
md.push('through the real file palette — chooser settle, weld, segment and the worker');
md.push('voxelise, up to the mesh node landing in the document; "mesh doc export" runs the');
md.push('same Make-lens 3MF measurement as the bracket row on that imported part. The');
md.push('fixture is not budget-scale: the 87k-triangle Meshy STL the milestone names is');
md.push('measured natively in `docs/verification/V10.md`.');
md.push('');
md.push(`Machine: ${machine}. Every timing above is this machine's.` +
  (outPathArg ? '' : ` GOAL's mid-range laptop is not measured here.`));
if (xauth && !rows.filter((r) => r.mode === 'headed').every((r) => r.ok)) headedOk = false;
writeFileSync(outPath, md.join('\n') + '\n');

for (const r of rows) console.log(`${r.ok ? 'PASS' : 'FAIL'} — [${r.mode}] ${r.label}: ${r.detail}`);
if (!modes.includes('headed')) {
  console.log(`headed pass unavailable: ${outPath} records the headless numbers only`);
  process.exit(1);
}
process.exit(headedOk ? 0 : 1);