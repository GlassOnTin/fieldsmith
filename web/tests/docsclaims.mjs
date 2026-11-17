// Repository claim-surface integrity (sustain leg of 5S, VISION §3/§6).
//
// The UI's verification badges are gated by registry.mjs / badge.mjs: a sheet may only
// advertise a verification that is shipped. The repository's prose surfaces make the same
// kind of claim — docs/README.md rows say "Verified by V57" and link a report, README.md
// and VISION.md cite V# numbers and script paths, and range claims ("V1–V55") describe the
// suite's coverage. A claim that names an unshipped report, links a different report than
// it names, cites a script that does not exist, or states a stale range is a false claim on
// the project's front door, in either direction: over-claiming verification and under-
// claiming shipped capability both mislead the reader.
//
// This Node-only gate walks docs/README.md, README.md, and VISION.md and asserts:
//   1. every relative markdown link in docs/README.md resolves on disk;
//   2. every V# named in a "Verified by …" claim is linked in the same table row;
//   3. every V# named in a "Verified by …" claim has a shipped report;
//   4. every shipped V# report has a live emitter (a quoted Report id or a filename
//      literal in crates/fieldsmith-verify/tests/, web/tests/, or scripts/);
//   5. every v<n>_*.rs verification test has its shipped V<n>.md report;
//   6. every V# cited in README.md / VISION.md has a shipped report;
//   7. every scripts/… path cited in README.md / VISION.md exists;
//   8. every "V1–V<n>" suite-range claim ends at the largest shipped report number.
//
// Prose semantics (does a Limitations paragraph match the shipped solver set?) are not
// regex-checkable and stay with the REVIEW.md tiers; this gate covers the mechanical
// claim surfaces only.
//
// Run with: node web/tests/docsclaims.mjs   (wired into scripts/ci.sh).
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const docsDir = resolve(repo, 'docs/verification');

const checks = [];
const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });

const read = (rel) => readFileSync(resolve(repo, rel), 'utf8');
const docsReadme = read('docs/README.md');
const readme = read('README.md');
const vision = read('VISION.md');

// --- Shipped report set: docs/verification/V*.md, keyed by stem (V22-MCP keeps its suffix). ---
const reportStems = readdirSync(docsDir)
  .filter((f) => /^V\d+.*\.md$/.test(f))
  .map((f) => f.replace(/\.md$/, ''))
  .sort();
const reportNums = reportStems.map((s) => parseInt(s.match(/^V(\d+)/)[1], 10));
const maxReport = Math.max(...reportNums);
const shipped = new Set(reportStems);
const shippedNums = new Set(reportNums);

// --- Emitter sources: where a report can be written from. ---
const emitterDirs = ['crates/fieldsmith-verify/tests', 'web/tests', 'scripts'];
const emitterSrcs = [];
for (const dir of emitterDirs) {
  const abs = resolve(repo, dir);
  if (!existsSync(abs)) continue;
  for (const f of readdirSync(abs)) {
    if (!/\.(rs|mjs|sh|py)$/.test(f)) continue;
    emitterSrcs.push({ rel: join(dir, f), src: readFileSync(join(abs, f), 'utf8') });
  }
}

try {
  // Invariant 1: every relative markdown link in docs/README.md resolves. Links are
  // relative to docs/ (the file's own directory); anchors and external URLs are skipped.
  const deadLinks = [];
  let totalLinks = 0;
  for (const m of docsReadme.matchAll(/\]\(([^)\s]+)\)/g)) {
    const target = m[1];
    if (/^(https?:|mailto:|#)/.test(target)) continue;
    totalLinks++;
    const path = target.split('#')[0];
    if (!existsSync(resolve(repo, 'docs', path))) deadLinks.push(target);
  }
  check('every relative link in docs/README.md resolves',
    deadLinks.length === 0,
    `${totalLinks} links, ${deadLinks.length} dead${deadLinks.length ? ': ' + deadLinks.join(', ') : ''}`);

  // Invariants 2 + 3: per table row, the V# numbers named in a "Verified by …" claim must
  // be linked in the same row and must have shipped reports. A row that claims V57 but
  // links V53 is a false provenance claim even though the link itself resolves.
  const claimLinkBad = [];
  const claimUnshipped = [];
  let claimRows = 0;
  for (const line of docsReadme.split('\n')) {
    if (!line.startsWith('|')) continue;
    const claim = line.match(/Verified by ([^·|]*)/);
    if (!claim) continue;
    claimRows++;
    const claimed = [...claim[1].matchAll(/V(\d+)/g)].map((m) => parseInt(m[1], 10));
    const linked = new Set([...line.matchAll(/\(verification\/(V\d+)[^)]*\.md\)/g)].map((m) => m[1]));
    for (const n of claimed) {
      if (!linked.has(`V${n}`)) claimLinkBad.push(`V${n} claimed, not linked in row`);
      if (!shippedNums.has(n)) claimUnshipped.push(`V${n}`);
    }
  }
  check('every "Verified by" V# is linked in its own row',
    claimLinkBad.length === 0,
    `${claimRows} claiming rows, ${claimLinkBad.length} mismatched${claimLinkBad.length ? ': ' + claimLinkBad.join('; ') : ''}`);
  check('every "Verified by" V# has a shipped report',
    claimUnshipped.length === 0,
    `${claimUnshipped.length} unshipped${claimUnshipped.length ? ': ' + [...new Set(claimUnshipped)].join(', ') : ''}`);

  // Invariant 4: every shipped report has a live emitter — a quoted Report id ("V56")
  // in a Rust test, or a filename literal ('V26.md', V24.md) in a browser gate or script.
  const orphanReports = [];
  for (const stem of reportStems) {
    const live = emitterSrcs.some(({ src }) =>
      src.includes(`"${stem}"`) || src.includes(`'${stem}'`) || src.includes(`${stem}.md`));
    if (!live) orphanReports.push(stem);
  }
  check('every shipped V# report has a live emitter',
    orphanReports.length === 0,
    `${reportStems.length} reports, ${orphanReports.length} without an emitter${orphanReports.length ? ': ' + orphanReports.join(', ') : ''}`);

  // Invariant 5: every v<n>_*.rs verification test ships its V<n>.md report. The suite's
  // convention is one report per case; a test that asserts without emitting leaves a
  // "Verified by V<n>" claim with nothing to point at. (v1_v2_cantilever.rs covers V1 and
  // V2; the leading number is the one checked. adaptive_* probes are not V cases.)
  const testFiles = readdirSync(resolve(repo, 'crates/fieldsmith-verify/tests'))
    .filter((f) => /^v\d+.*\.rs$/.test(f));
  const unreported = [];
  for (const f of testFiles) {
    const n = parseInt(f.match(/^v(\d+)/)[1], 10);
    if (!shippedNums.has(n)) unreported.push(`${f}→V${n}.md`);
  }
  check('every v<n> verification test ships its V<n> report',
    unreported.length === 0,
    `${testFiles.length} test files, ${unreported.length} without a report${unreported.length ? ': ' + unreported.join(', ') : ''}`);

  // Invariant 6: every bare V# citation in README.md / VISION.md has a shipped report.
  const citedBad = [];
  for (const [name, src] of [['README.md', readme], ['VISION.md', vision]]) {
    for (const m of src.matchAll(/\bV(\d+)\b/g)) {
      const n = parseInt(m[1], 10);
      if (!shippedNums.has(n)) citedBad.push(`${name}:V${n}`);
    }
  }
  check('every V# cited in README.md / VISION.md has a shipped report',
    citedBad.length === 0,
    `${citedBad.length} unshipped citations${citedBad.length ? ': ' + [...new Set(citedBad)].join(', ') : ''}`);

  // Invariant 7: every scripts/… path cited in README.md / VISION.md exists.
  const deadScripts = [];
  let scriptCites = 0;
  for (const [name, src] of [['README.md', readme], ['VISION.md', vision]]) {
    for (const m of src.matchAll(/scripts\/[A-Za-z0-9._-]+\.(?:sh|py|mjs|rs)/g)) {
      scriptCites++;
      if (!existsSync(resolve(repo, m[0]))) deadScripts.push(`${name}:${m[0]}`);
    }
  }
  check('every scripts/ path cited in README.md / VISION.md exists',
    deadScripts.length === 0,
    `${scriptCites} citations, ${deadScripts.length} dead${deadScripts.length ? ': ' + deadScripts.join(', ') : ''}`);

  // Invariant 8: every "V1–V<n>" suite-range claim ends at the largest shipped report.
  // Ranges starting at V1 describe the suite's coverage; historical ranges (V26–V43)
  // start elsewhere and are not checked. Backticked forms (`V1` – `V60`) are included.
  const rangeBad = [];
  let ranges = 0;
  for (const [name, src] of [['README.md', readme], ['VISION.md', vision], ['docs/README.md', docsReadme]]) {
    for (const m of src.matchAll(/V1(?:\.md)?`?\s*[–-]\s*`?V(\d+)(?:\.md)?/g)) {
      ranges++;
      const n = parseInt(m[1], 10);
      if (n !== maxReport) rangeBad.push(`${name}:V1–V${n} (max shipped V${maxReport})`);
    }
  }
  check('every V1–V<n> suite-range claim matches the shipped suite',
    rangeBad.length === 0,
    `${ranges} range claims, ${rangeBad.length} stale${rangeBad.length ? ': ' + rangeBad.join('; ') : ''}`);

  check('docs-claim scan completed', true,
    `${reportStems.length} shipped reports (max V${maxReport}), ${emitterSrcs.length} emitter sources, ${claimRows} claiming rows`);

  const ok = checks.every((c) => c.pass);
  const table = [
    '| Check | Result | Measured |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail}`.replace(/\n/g, ' ')),
  ].join('\n');
  const lines = [
    '# Repository claim-surface integrity (docs)',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    'The repository\'s prose surfaces make verification claims the same way the UI\'s badges',
    'do: docs/README.md rows say "Verified by V#" and link a report, README.md and VISION.md',
    'cite V# numbers and script paths, and "V1–V<n>" ranges describe the suite\'s coverage.',
    'This Node-only gate asserts those claims are mechanical truth: every relative link in',
    'docs/README.md resolves; every V# named in a "Verified by" claim is linked in the same',
    'row and has a shipped report; every shipped report has a live emitter (a quoted Report',
    'id or filename literal in the verify tests, browser gates, or scripts); every v<n>',
    'verification test ships its V<n> report; every V# cited in README.md / VISION.md is',
    'shipped; every cited scripts/ path exists; and every V1–V<n> range ends at the largest',
    'shipped report number.',
    '',
    '## Checks',
    '',
    table,
    '',
    '## Limits',
    '',
    '- Prose semantics are out of scope: whether a Limitations paragraph matches the shipped solver set, or a capability description is accurate, stays with the REVIEW.md tiers. Under-claiming shipped capability is caught by review, not by this gate.',
    '- Emitter liveness is a literal search (quoted Report id or filename) in the emitter directories; a report written through a computed id would not be found.',
    '- The test→report check uses the leading v<n> number of the test filename; multi-case files (v1_v2_cantilever.rs) are checked on their first number only.',
    '- Range checks cover V1-started ranges only; historical ranges starting at other numbers are not coverage claims.',
    '',
    '---',
    '*Generated by `node web/tests/docsclaims.mjs` — do not edit by hand.*',
    '',
  ].join('\n');
  if (!ok) {
    const bad = checks.filter((c) => !c.pass);
    console.error(['docsclaims: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
  } else {
    writeFileSync(resolve(docsDir, 'docsclaims-e2e.md'), lines);
    console.log(`docsclaims: PASS — ${reportStems.length} reports, ${claimRows} claiming rows, ${ranges} range claims, ${scriptCites} script citations all honest`);
  }
  process.exit(ok ? 0 : 1);
} catch (e) {
  console.error(`docsclaims: run aborted: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
}
