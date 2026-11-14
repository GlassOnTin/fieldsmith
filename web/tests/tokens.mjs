// Design-token integrity: the web app references a CSS custom-property vocabulary that
// styles.css must actually define. An undefined `var(--x)` with no fallback is invalid at
// computed-value time — `color` silently inherits and `background` falls back to transparent
// — so cards render see-through and muted/error text loses its emphasis. This suite gates
// two things:
//   (static)  every `var(--name)` reference in web/src that has NO inline fallback resolves
//             to a name declared in styles.css (light or dark block);
//   (runtime) the tokens that were historically undefined render the intended palette colour
//             in BOTH light and dark, read from getComputedStyle on live probe elements.
//
// The runtime probe parents every element under a sentinel colour (rgb(1, 2, 3)) so an
// undefined colour token — which inherits — is distinguishable from a defined one, and reads
// the reference tokens through the same probe so both sides normalise to rgb().
//
// Run with node tests/run-e2e.mjs --only tokens.
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = resolve(here, '../src');

// alias token -> the palette token it must resolve to.
const COLOR_MAP = { '--text-1': '--text', '--fg': '--text', '--fg-dim': '--text-2', '--err': '--danger' };
const BG_MAP = { '--bg-1': '--panel', '--bg-2': '--panel-2', '--bg-3': '--panel-2' };
const SENTINEL = 'rgb(1, 2, 3)';
const TRANSPARENT = 'rgba(0, 0, 0, 0)';

/** Static guard: collect declared token names from styles.css and every no-fallback
 * `var(--name)` reference across src; return the references that resolve to nothing. */
function undefinedTokenRefs() {
  const css = readFileSync(resolve(srcDir, 'styles.css'), 'utf8');
  const defined = new Set();
  for (const m of css.matchAll(/(--[a-z0-9-]+)\s*:/g)) defined.add(m[1]);

  const files = readdirSync(srcDir, { recursive: true })
    .filter((f) => /\.(ts|tsx|css)$/.test(f))
    .map((f) => join(srcDir, f));
  const missing = new Map(); // name -> [files]
  for (const file of files) {
    const text = readFileSync(file, 'utf8');
    // capture the token name and the delimiter that follows it: ',' = has a fallback (safe),
    // ')' = no fallback (must be defined). Handles nested var() and color-mix(...) wrappers.
    for (const m of text.matchAll(/var\(\s*(--[a-z0-9-]+)\s*([,)])/g)) {
      const [, name, delim] = m;
      if (delim === ')' && !defined.has(name)) {
        const rel = file.slice(srcDir.length + 1);
        if (!missing.has(name)) missing.set(name, []);
        if (!missing.get(name).includes(rel)) missing.get(name).push(rel);
      }
    }
  }
  return [...missing.entries()].map(([name, where]) => ({ name, where }));
}

/** In-page probe: read computed colour/background for the alias tokens and their references
 * under a sentinel-coloured parent, so undefined (inheriting) tokens are detectable. */
function probeTokens() {
  const colorNames = [
    '--text-1', '--fg', '--fg-dim', '--err', '--text', '--text-2', '--danger',
  ];
  const bgNames = ['--bg-1', '--bg-2', '--bg-3', '--panel', '--panel-2'];
  const parent = document.createElement('div');
  parent.style.color = 'rgb(1, 2, 3)';
  parent.style.position = 'absolute';
  parent.style.left = '-9999px';
  parent.style.top = '0';
  document.body.appendChild(parent);
  const read = (tok, prop) => {
    const el = document.createElement('div');
    el.style[prop] = `var(${tok})`;
    parent.appendChild(el);
    return getComputedStyle(el)[prop === 'color' ? 'color' : 'backgroundColor'];
  };
  const colors = {};
  const bgs = {};
  for (const n of colorNames) colors[n] = read(n, 'color');
  for (const n of bgNames) bgs[n] = read(n, 'background');
  parent.remove();
  return { colors, bgs };
}

export async function tokensTest(browser, url, { docsDir }) {
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

  check('e2e completed', false, '');

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));

  try {
    // ---- static guard -------------------------------------------------------------------
    const missing = undefinedTokenRefs();
    check(
      'no undefined design-token references (no-fallback var() all declared)',
      missing.length === 0,
      missing.length === 0
        ? 'every no-fallback var(--name) in web/src is declared in styles.css'
        : missing.map((m) => `${m.name} (${m.where.join(', ')})`).join('; '),
    );

    // ---- runtime probe, light then dark -------------------------------------------------
    await page.goto(url);
    await page.waitForFunction(() => window.fieldsmith, null, { timeout: 120000 });

    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      const { colors, bgs } = await page.evaluate(probeTokens);
      for (const [tok, ref] of Object.entries(COLOR_MAP)) {
        const got = colors[tok];
        const want = colors[ref];
        const pass = got === want && got !== SENTINEL;
        check(
          `${scheme}: ${tok} resolves to ${ref}`,
          pass,
          `${got} (want ${want}${got === SENTINEL ? '; inherited sentinel = undefined' : ''})`,
        );
      }
      for (const [tok, ref] of Object.entries(BG_MAP)) {
        const got = bgs[tok];
        const want = bgs[ref];
        const pass = got === want && got !== TRANSPARENT;
        check(
          `${scheme}: ${tok} resolves to ${ref}`,
          pass,
          `${got} (want ${want}${got === TRANSPARENT ? '; transparent = undefined' : ''})`,
        );
      }
    }

    const unexpected = errors.filter((e) => !/favicon/i.test(e));
    check('no console errors', unexpected.length === 0, unexpected.join(' | ') || '0');

    const done = checks.find((c) => c.name === 'e2e completed');
    if (done.detail === '') done.pass = true;
    const ok = checks.every((c) => c.pass);
    const names = checks.map((c) => c.name);
    const lines = [
      '# Design-token integrity (web UI)',
      '',
      `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
      '',
      'The app references a CSS custom-property vocabulary that `styles.css` must define. An',
      'undefined `var(--x)` with no fallback is invalid at computed-value time: `color` inherits',
      'and `background` becomes transparent, so cards render see-through and muted/error text',
      'loses emphasis. This gates (1) a static scan — every no-fallback `var(--name)` in `web/src`',
      'is declared in `styles.css` — and (2) a live `getComputedStyle` probe of the historically',
      'undefined tokens in both light and dark, parented under a sentinel colour so an undefined',
      '(inheriting) token is distinguishable from a defined one.',
      '',
      '## Checks',
      '',
      table(names),
      '',
      '## Limits',
      '',
      '- Tokens referenced with an inline fallback (e.g. `var(--bad, #ef4444)`) are treated as',
      '  safe by the static guard and are not colour-checked; their fallback renders.',
      '- The runtime probe checks the alias→palette mapping, not the aesthetic choice of palette.',
      '',
      `Console errors: **${unexpected.length}**`,
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/tokens.mjs) — do not edit by hand.*',
      '',
    ].join('\n');
    if (!ok) {
      const bad = checks.filter((c) => !c.pass);
      console.error(['tokens: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    } else {
      writeFileSync(resolve(docsDir, 'tokens-e2e.md'), lines);
    }
    return ok;
  } catch (e) {
    const done = checks.find((c) => c.name === 'e2e completed');
    done.detail = String(e).split('\n')[0];
    console.error(`tokens: run aborted: ${e && e.stack ? e.stack : e}`);
    return false;
  } finally {
    await page.close();
  }
}