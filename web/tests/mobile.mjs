// The narrow-screen (phone) layout: below 900 px the app is one column, the tree and
// inspector become slide-in drawers over the viewport, and the bottom overlays of each lens
// must not collide. Run with node tests/run-e2e.mjs --only mobile.
import { writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

// One evaluate(): the narrow-layout facts the checks assert on.
const MEASURE = () => {
  const r = (sel) => {
    const el = document.querySelector(sel);
    if (!el) return null;
    const b = el.getBoundingClientRect();
    return { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.width), h: Math.round(b.height) };
  };
  return {
    vw: innerWidth,
    overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    view: r('.view'),
    lenses: r('.lenses'),
    toggles: r('.drawer-toggles'),
    tree: r('.tree-panel'),
    insp: r('.insp-panel'),
    opacity: r('.opacity-panel'),
    makePanel: r('[data-testid="make-panel"]'),
    scrim: r('.drawer-scrim'),
  };
};

const overlap = (a, b) =>
  a && b && a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

export async function mobileTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 412, height: 892 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const steps = [];
  const step = async (name, fn) => {
    await fn();
    const file = `mobile-${String(steps.length + 1).padStart(2, '0')}.png`;
    await page.screenshot({ path: resolve(screens, file) });
    steps.push({ name, file, ms: Date.now() - (page._t ?? Date.now()) });
    page._t = Date.now();
  };

  await page.goto(url);
  await page.waitForSelector('.app', { timeout: 15000 });
  const m = () => page.evaluate(MEASURE);

  await step('Narrow viewport: one column, no overflow', async () => {
    const s = await m();
    check('no horizontal overflow at 412 px', !s.overflowX, `scrollW was ${s.vw} px wide`);
    check('the viewport owns the full width', s.view && s.view.x === 0 && s.view.w === s.vw, JSON.stringify(s.view));
    check('drawer toggles are visible', s.toggles && s.toggles.w > 0 && s.toggles.h > 0, JSON.stringify(s.toggles));
    check('the tree is parked off-screen', s.tree && (s.tree.x + s.tree.w <= 0), JSON.stringify(s.tree));
    check('the inspector is parked off-screen', s.insp && s.insp.x >= s.vw, JSON.stringify(s.insp));
  });

  await step('Tree drawer opens over the viewport', async () => {
    await page.locator('[data-testid="toggle-tree"]').click();
    await page.waitForSelector('.tree-panel.open', { timeout: 3000 });
    // The class flips instantly; the 180 ms slide settles after. Wait for the settled box.
    await page.waitForFunction(() => {
      const t = document.querySelector('.tree-panel');
      return t && Math.round(t.getBoundingClientRect().x) === 0;
    }, { timeout: 3000 });
    const s = await m();
    check('tree drawer slides to x 0', s.tree && s.tree.x === 0, JSON.stringify(s.tree));
    check('a scrim covers the viewport', s.scrim && s.scrim.w >= s.vw, JSON.stringify(s.scrim));
  });

  await step('Tapping the scrim closes it', async () => {
    await page.locator('[data-testid="drawer-scrim"]').click({ position: { x: 380, y: 446 } });
    await page.waitForFunction(() => !document.querySelector('.tree-panel.open'), { timeout: 3000 });
    await page.waitForFunction(() => {
      const t = document.querySelector('.tree-panel');
      const b = t.getBoundingClientRect();
      return Math.round(b.x + b.width) <= 0;
    }, { timeout: 3000 });
    const s = await m();
    check('tree drawer parked again', s.tree && s.tree.x + s.tree.w <= 0, JSON.stringify(s.tree));
  });

  await step('Inspector drawer opens from the right', async () => {
    await page.locator('[data-testid="toggle-insp"]').click();
    await page.waitForSelector('.insp-panel.open', { timeout: 3000 });
    await page.waitForFunction(() => {
      const t = document.querySelector('.insp-panel');
      const b = t.getBoundingClientRect();
      return Math.round(b.x + b.width) === innerWidth;
    }, { timeout: 3000 });
    const s = await m();
    check('inspector drawer at the right edge', s.insp && s.insp.x + s.insp.w === s.vw, JSON.stringify(s.insp));
  });

  await step('Close the inspector and load an example', async () => {
    await page.locator('[data-testid="drawer-scrim"]').click({ position: { x: 40, y: 446 } });
    await page.waitForFunction(() => !document.querySelector('.insp-panel.open'), { timeout: 3000 });
    await page.locator('[data-testid^="example-"]').first().click();
    await page.waitForSelector('[role="tree"] .row', { timeout: 20000 });
  });

  await step('Make lens: opacity control clears the make panel', async () => {
    await page.locator('.lenses [data-lens="make"]').click();
    await page.waitForSelector('[data-testid="make-panel"]', { timeout: 15000 });
    const s = await m();
    check('make panel and opacity panel do not overlap', !overlap(s.makePanel, s.opacity), `make ${JSON.stringify(s.makePanel)} vs opacity ${JSON.stringify(s.opacity)}`);
    check('no horizontal overflow with panels shown', !s.overflowX);
  });

  await step('Desktop width: the old three-column layout returns', async () => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.waitForTimeout(300);
    const s = await m();
    check('drawer toggles hidden on desktop', s.toggles && s.toggles.w === 0 && s.toggles.h === 0, JSON.stringify(s.toggles));
    check('tree panel is a 240 px column again', s.tree && s.tree.x === 0 && s.tree.w === 240, JSON.stringify(s.tree));
    check('inspector panel is a 288 px column again', s.insp && s.insp.w === 288, JSON.stringify(s.insp));
  });

  await page.close();
  const ok = checks.every((c) => c.pass) && errors.length === 0;
  const lines = [
    '# Mobile e2e — the narrow-screen layout',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**  ·  ${new Date().toISOString()}`,
    '',
    'Viewport 412 × 892 (below the 900 px breakpoint): one column, slide-in drawers, no overlay collisions.',
    '',
    '## Steps',
    '',
    '| Step | Report |',
    '| :-- | :-- |',
    ...steps.map((s) => `| ${s.name} | test-results/${s.file} |`),
    '',
    '## Checks',
    '',
    '| Check | Result | Detail |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail.replace(/\|/g, '\\|')} |`),
    '',
    ...(errors.length ? ['## Console errors', '', ...errors.map((e) => `- ${e}`), ''] : []),
    '---',
    '*Generated by `npm run test:e2e` (web/tests/run-e2e.mjs) — do not edit by hand.*',
    '',
  ];
  writeFileSync(resolve(docsDir, 'mobile-e2e.md'), lines.join('\n'));
  return ok;
}