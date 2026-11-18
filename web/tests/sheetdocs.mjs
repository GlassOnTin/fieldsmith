// Sheet-library documentation integrity (sustain leg of 5S, VISION §3/§6).
//
// The self-verifying benchmark library is the project's compounding asset: every analytic
// test, tutorial, and case study is a standalone literate `.fsm` under examples/, auto-
// discovered by the app (import.meta.glob) and parse-checked in CI. The repository documents
// that library two ways: docs/README.md indexes each sheet in a table row, and a markdown
// mirror under docs/{analytic-tests,tutorials,case-studies}/ carries its theory, checks, and
// provenance. Two failure modes drift as the library grows:
//   * coverage — a shipped sheet with no index entry (the front door presents a subset as the
//     whole library);
//   * honesty — a mirror whose "Verified by V#" badge names a different report than its
//     "Verification Reports:" link, or an unshipped report (the same false-provenance class
//     registry.mjs / docsclaims.mjs gate for the UI badges and the front door).
//
// This Node-only gate asserts:
//   1. every shipped sheet (@id in examples/{analytic,tutorials,case-studies}/*.fsm) is indexed
//      in docs/README.md;
//   2. every markdown mirror corresponds to a shipped sheet (no orphan mirrors);
//   3. every mirror "Verification Reports:" path resolves to a shipped report;
//   4. every mirror "Verified by V#" claim is shipped;
//   5. every mirror "Verified by V#" claim agrees with its "Verification Reports:" link;
//   6. every indexed sheet has a markdown mirror (docs/README promises a standalone doc for
//      each sheet it lists).
//
// Prose accuracy of a mirror's theory vs the solver is out of scope (REVIEW.md tiers); this
// gate covers coverage and mechanical provenance only.
//
// Run with: node web/tests/sheetdocs.mjs   (wired into scripts/ci.sh).
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const docsDir = resolve(repo, 'docs/verification');

const checks = [];
const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
const read = (rel) => readFileSync(resolve(repo, rel), 'utf8');

// Shipped verification reports.
const reportStems = readdirSync(docsDir)
  .filter((f) => /^V\d+.*\.md$/.test(f))
  .map((f) => f.replace(/\.md$/, ''));
const shippedNums = new Set(reportStems.map((s) => parseInt(s.match(/^V(\d+)/)[1], 10)));

// Shipped sheets: @id from each example header.
const sheetDirs = ['examples/analytic', 'examples/tutorials', 'examples/case-studies'];
const sheets = [];
for (const dir of sheetDirs) {
  const abs = resolve(repo, dir);
  if (!existsSync(abs)) continue;
  for (const f of readdirSync(abs).filter((f) => f.endsWith('.fsm')).sort()) {
    const m = read(join(dir, f)).match(/^#\s*@id:\s*(\S+)/m);
    if (m) sheets.push({ id: m[1], rel: join(dir, f) });
  }
}
const sheetIds = new Set(sheets.map((s) => s.id));

// Markdown mirrors.
const mirrorDirs = ['docs/analytic-tests', 'docs/tutorials', 'docs/case-studies'];
const mirrors = [];
for (const dir of mirrorDirs) {
  const abs = resolve(repo, dir);
  if (!existsSync(abs)) continue;
  for (const f of readdirSync(abs).filter((f) => /^[AT]\d+.*\.md$/.test(f)).sort()) {
    mirrors.push({ id: f.match(/^([AT]\d+)/)[1], rel: join(dir, f), src: read(join(dir, f)) });
  }
}
const mirrorIds = new Set(mirrors.map((m) => m.id));

// Indexed sheet IDs: docs/README.md table rows "| **A1** | ...".
const indexedIds = new Set(
  [...read('docs/README.md').matchAll(/^\|\s*\*\*([AT]\d+)\*\*\s*\|/gm)].map((m) => m[1]),
);

try {
  // 1. Coverage: every shipped sheet is indexed.
  const unindexed = sheets.filter((s) => !indexedIds.has(s.id)).map((s) => s.id);
  check('every shipped sheet is indexed in docs/README.md',
    unindexed.length === 0,
    `${sheets.length} sheets, ${indexedIds.size} indexed, ${unindexed.length} missing${unindexed.length ? ': ' + unindexed.join(', ') : ''}`);

  // 2. No orphan mirrors.
  const orphan = mirrors.filter((m) => !sheetIds.has(m.id)).map((m) => m.id);
  check('every markdown mirror corresponds to a shipped sheet',
    orphan.length === 0,
    `${mirrors.length} mirrors, ${orphan.length} orphan${orphan.length ? ': ' + orphan.join(', ') : ''}`);

  // 3/4/5. Mirror provenance: Verification Reports resolve; Verified by V# shipped + agrees.
  const deadReport = [];
  const unshippedClaim = [];
  const mismatch = [];
  let claimMirrors = 0;
  for (const m of mirrors) {
    const repField = m.src.match(/\*\*Verification Reports\*\*:\s*([^\n]*)/);
    const linked = repField
      ? [...repField[1].matchAll(/docs\/verification\/(V\d+[^`\s]*?)\.md/g)].map((x) => x[1])
      : [];
    for (const stem of linked) if (!reportStems.includes(stem)) deadReport.push(`${m.id}→${stem}`);
    const claim = m.src.match(/Verified by\s+([^·\n]*)/);
    if (!claim) continue;
    claimMirrors++;
    const claimed = [...claim[1].matchAll(/V(\d+)/g)].map((x) => parseInt(x[1], 10));
    const linkedNums = new Set(linked.map((s) => parseInt(s.match(/^V(\d+)/)[1], 10)));
    for (const n of claimed) {
      if (!shippedNums.has(n)) unshippedClaim.push(`${m.id}:V${n}`);
      if (!linkedNums.has(n)) mismatch.push(`${m.id} claims V${n}, reports ${linked.join('/') || 'none'}`);
    }
  }
  check('every mirror "Verification Reports" path resolves',
    deadReport.length === 0,
    `${deadReport.length} dead${deadReport.length ? ': ' + deadReport.join(', ') : ''}`);
  check('every mirror "Verified by V#" claim is shipped',
    unshippedClaim.length === 0,
    `${claimMirrors} claiming mirrors, ${unshippedClaim.length} unshipped${unshippedClaim.length ? ': ' + unshippedClaim.join(', ') : ''}`);
  check('every mirror "Verified by V#" agrees with its Verification Reports',
    mismatch.length === 0,
    `${mismatch.length} mismatched${mismatch.length ? ': ' + mismatch.join('; ') : ''}`);

  // 6. Every indexed sheet has a mirror (docs/README promises a standalone doc per listed sheet).
  const noMirror = [...indexedIds].filter((id) => !mirrorIds.has(id));
  check('every indexed sheet has a markdown mirror',
    noMirror.length === 0,
    `${indexedIds.size} indexed, ${noMirror.length} without a mirror${noMirror.length ? ': ' + noMirror.join(', ') : ''}`);

  check('sheet-docs scan completed', true,
    `${sheets.length} sheets, ${mirrors.length} mirrors, ${indexedIds.size} indexed, ${claimMirrors} claiming mirrors`);

  const ok = checks.every((c) => c.pass);
  const table = [
    '| Check | Result | Measured |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail}`.replace(/\n/g, ' ')),
  ].join('\n');
  const lines = [
    '# Sheet-library documentation integrity (docs)',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    'The benchmark library ships as literate `.fsm` sheets under examples/, auto-discovered by',
    'the app and parse-checked in CI. docs/README.md indexes each sheet and a markdown mirror',
    'under docs/{analytic-tests,tutorials,case-studies}/ carries its theory and provenance. This',
    'Node-only gate asserts coverage (every shipped sheet is indexed; every indexed sheet has a',
    'mirror; no orphan mirrors) and mechanical provenance honesty (every mirror "Verification',
    'Reports:" path resolves; every "Verified by V#" claim is shipped and agrees with its',
    'reported link). It is the sheet-documentation counterpart of registry.mjs (UI badges) and',
    'docsclaims.mjs (front-door claims).',
    '',
    '## Checks',
    '',
    table,
    '',
    '## Limits',
    '',
    '- Prose accuracy of a mirror\'s theory against the solver is out of scope; that stays with the REVIEW.md tiers. Coverage and mechanical provenance are checked.',
    '- Sheet IDs are read from the example header @id; mirror IDs from the filename prefix. A sheet whose header @id disagrees with its filename is not cross-checked here.',
    '- The "Verified by" claim is parsed up to the first middot or newline; a verification phrase formatted differently would need the parser updated.',
    '',
    '---',
    '*Generated by `node web/tests/sheetdocs.mjs` — do not edit by hand.*',
    '',
  ].join('\n');
  if (!ok) {
    const bad = checks.filter((c) => !c.pass);
    console.error(['sheetdocs: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
  } else {
    writeFileSync(resolve(docsDir, 'sheetdocs-e2e.md'), lines);
    console.log(`sheetdocs: PASS — ${sheets.length} sheets indexed and mirrored, ${claimMirrors} claiming mirrors honest`);
  }
  process.exit(ok ? 0 : 1);
} catch (e) {
  console.error(`sheetdocs: run aborted: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
}
