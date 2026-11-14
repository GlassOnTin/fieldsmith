// Sheet-provenance registry integrity (sustain leg of 5S, VISION §3/§6).
//
// Every literate sheet advertises where its repository Markdown and its `.fsm` source live
// (`# @doc_path:` / `# @fsm_path:` in the example header, mirrored by the DEFAULT_DOC_PATHS /
// DEFAULT_FSM_PATHS fallback maps in web/src/doc/literate-fsm.ts). The UI renders those paths as
// "Repository Markdown: <path>" provenance. A path that points at a file which does not exist is a
// false provenance claim: the user is told a repository file is there when it is not.
//
// This gate walks every shipped example header AND every fallback-map entry and asserts each
// declared doc_path / fsm_path resolves on disk (fs.existsSync, relative to the repo root). It also
// asserts every shipped example has a resolvable `.fsm` (header or map) and that no map key is an
// orphan (a key with no shipped example). It is a Node-only filesystem check — docs/ and examples/
// sit outside the Vite web root, so a browser probe could not read them.
//
// Dangling verification_refs (a `# @verification_refs:` or DEFAULT_VERIFICATION_REFS entry naming a
// docs/verification/V#.md that does not exist) are reported as a documented ADVISORY, not a hard
// failure: generating the missing reports is a separate verification task, and this gate must not
// fabricate them or silently understate verification. The advisory names the gap so it stays visible.
//
// Run with: node web/tests/registry.mjs   (wired into scripts/ci.sh).
import { readFileSync, writeFileSync, readdirSync, existsSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const docsDir = resolve(repo, 'docs/verification');
mkdirSync(docsDir, { recursive: true });

const checks = [];
const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
const resolves = (p) => typeof p === 'string' && p.length > 0 && existsSync(resolve(repo, p));

// --- Parse a literate `.fsm` header into its provenance directives. ---
function parseHeader(raw) {
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  const start = lines.findIndex((l) => l.trim() === '# === FIELDSMITH LITERATE CASE ===');
  const end = lines.findIndex((l) => l.trim() === '# === END LITERATE HEADER ===');
  const out = { id: null, docPath: null, fsmPath: null, verificationRefs: [] };
  if (start < 0 || end <= start) return out;
  for (let i = start + 1; i < end; i++) {
    const m = lines[i].match(/^#\s*@([a-z_]+):(.*)$/);
    if (!m) continue;
    const tag = m[1];
    const val = m[2].trim();
    if (tag === 'id') out.id = val;
    else if (tag === 'doc_path') out.docPath = val;
    else if (tag === 'fsm_path') out.fsmPath = val;
    else if (tag === 'verification_refs') out.verificationRefs = val.split(',').map((s) => s.trim()).filter(Boolean);
  }
  return out;
}

// --- Extract a flat `export const NAME: Record<...> = { ... };` literal from the TS source. ---
function extractMapBlock(src, name) {
  const re = new RegExp(`export const ${name}[^=]*=\\s*\\{([\\s\\S]*?)\\n\\};`);
  const m = src.match(re);
  if (!m) throw new Error(`map ${name} not found in literate-fsm.ts`);
  return m[1];
}
function extractStringMap(src, name) {
  const body = extractMapBlock(src, name);
  const out = {};
  const re = /^\s*([A-Za-z0-9]+):\s*'([^']*)'\s*,?\s*$/gm;
  let m;
  while ((m = re.exec(body))) out[m[1]] = m[2];
  return out;
}
function extractRefsMap(src, name) {
  const body = extractMapBlock(src, name);
  const out = {};
  const re = /^\s*([A-Za-z0-9]+):\s*\[([^\]]*)\]\s*,?\s*$/gm;
  let m;
  while ((m = re.exec(body))) {
    out[m[1]] = [...m[2].matchAll(/'([^']*)'/g)].map((x) => x[1]);
  }
  return out;
}

try {
  // Shipped example headers.
  const sheetDirs = ['examples/analytic', 'examples/tutorials', 'examples/case-studies'];
  const examples = [];
  for (const dir of sheetDirs) {
    const abs = resolve(repo, dir);
    if (!existsSync(abs)) continue;
    for (const f of readdirSync(abs).filter((f) => f.endsWith('.fsm')).sort()) {
      const rel = `${dir}/${f}`;
      const h = parseHeader(readFileSync(resolve(repo, rel), 'utf8'));
      examples.push({ rel, ...h });
    }
  }
  const exampleIds = new Set(examples.map((e) => e.id).filter(Boolean));

  // Fallback maps.
  const litSrc = readFileSync(resolve(repo, 'web/src/doc/literate-fsm.ts'), 'utf8');
  const docMap = extractStringMap(litSrc, 'DEFAULT_DOC_PATHS');
  const fsmMap = extractStringMap(litSrc, 'DEFAULT_FSM_PATHS');
  const refsMap = extractRefsMap(litSrc, 'DEFAULT_VERIFICATION_REFS');

  // Hard invariant 1: every header @fsm_path resolves.
  const deadFsmHeader = examples.filter((e) => e.fsmPath && !resolves(e.fsmPath));
  check('every example header @fsm_path resolves',
    deadFsmHeader.length === 0,
    `${examples.filter((e) => e.fsmPath).length} declared, ${deadFsmHeader.length} dead${deadFsmHeader.length ? ': ' + deadFsmHeader.map((e) => `${e.id}→${e.fsmPath}`).join(', ') : ''}`);

  // Hard invariant 2: every header @doc_path resolves.
  const deadDocHeader = examples.filter((e) => e.docPath && !resolves(e.docPath));
  check('every example header @doc_path resolves',
    deadDocHeader.length === 0,
    `${examples.filter((e) => e.docPath).length} declared, ${deadDocHeader.length} dead${deadDocHeader.length ? ': ' + deadDocHeader.map((e) => e.id).join(', ') : ''}`);

  // Hard invariant 3: every DEFAULT_FSM_PATHS value resolves.
  const deadFsmMap = Object.entries(fsmMap).filter(([, p]) => !resolves(p));
  check('every DEFAULT_FSM_PATHS value resolves',
    deadFsmMap.length === 0,
    `${Object.keys(fsmMap).length} entries, ${deadFsmMap.length} dead${deadFsmMap.length ? ': ' + deadFsmMap.map(([k, p]) => `${k}→${p}`).join(', ') : ''}`);

  // Hard invariant 4: every DEFAULT_DOC_PATHS value resolves.
  const deadDocMap = Object.entries(docMap).filter(([, p]) => !resolves(p));
  check('every DEFAULT_DOC_PATHS value resolves',
    deadDocMap.length === 0,
    `${Object.keys(docMap).length} entries, ${deadDocMap.length} dead${deadDocMap.length ? ': ' + deadDocMap.map(([k]) => k).join(', ') : ''}`);

  // Hard invariant 5: every shipped example has a resolvable .fsm via header or map.
  const noFsm = examples.filter((e) => {
    const viaHeader = e.fsmPath && resolves(e.fsmPath);
    const viaMap = e.id && fsmMap[e.id] && resolves(fsmMap[e.id]);
    return !(viaHeader || viaMap);
  });
  check('every shipped example has resolvable .fsm coverage (header or map)',
    noFsm.length === 0,
    `${examples.length} examples, ${noFsm.length} without coverage${noFsm.length ? ': ' + noFsm.map((e) => e.id ?? e.rel).join(', ') : ''}`);

  // Hard invariant 6: no orphan map keys (every key names a shipped example).
  const orphanDoc = Object.keys(docMap).filter((k) => !exampleIds.has(k));
  const orphanFsm = Object.keys(fsmMap).filter((k) => !exampleIds.has(k));
  const orphans = [...orphanDoc.map((k) => `DOC:${k}`), ...orphanFsm.map((k) => `FSM:${k}`)];
  check('no orphan fallback-map keys (every key is a shipped example)',
    orphans.length === 0,
    `${orphans.length} orphan${orphans.length ? ': ' + orphans.join(', ') : ''}`);

  // Advisory (non-failing): dangling verification_refs in headers and the fallback map.
  const deadRefsHeader = [];
  for (const e of examples) {
    for (const r of e.verificationRefs) if (!resolves(r)) deadRefsHeader.push(`${e.id}→${r}`);
  }
  const deadRefsMap = [];
  for (const [k, refs] of Object.entries(refsMap)) {
    for (const r of refs) if (!resolves(r)) deadRefsMap.push(`${k}→${r}`);
  }
  const advisory = [...deadRefsHeader, ...deadRefsMap];

  check('registry scan completed', true, `${examples.length} examples, ${Object.keys(docMap).length} doc-map / ${Object.keys(fsmMap).length} fsm-map entries`);

  const ok = checks.every((c) => c.pass);
  const table = [
    '| Check | Result | Measured |',
    '| :-- | :-- | :-- |',
    ...checks.map((c) => `| ${c.name} | ${c.pass ? 'pass' : '**fail**'} | ${c.detail}`.replace(/\n/g, ' ')),
  ].join('\n');
  const lines = [
    '# Sheet-provenance registry integrity (web UI)',
    '',
    `**Result: ${ok ? 'PASS' : 'FAIL'}**`,
    '',
    'Every literate sheet advertises a repository Markdown path (`@doc_path`) and a `.fsm` source',
    'path (`@fsm_path`), in the example header and in the DEFAULT_DOC_PATHS / DEFAULT_FSM_PATHS',
    'fallback maps. The UI renders these as "Repository Markdown: <path>" provenance, so a path that',
    'does not resolve on disk is a false claim. This Node-only gate walks every shipped example',
    'header and every fallback-map entry and asserts each declared doc_path / fsm_path resolves',
    '(fs.existsSync relative to the repo root), that every shipped example has a resolvable `.fsm`,',
    'and that no fallback-map key is an orphan. docs/ and examples/ sit outside the Vite web root, so',
    'this is a filesystem check rather than a browser probe.',
    '',
    '## Checks',
    '',
    table,
    '',
    '## Advisory — dangling verification references (not a hard failure)',
    '',
    advisory.length === 0
      ? 'None: every `@verification_refs` and DEFAULT_VERIFICATION_REFS entry resolves.'
      : [
          `${advisory.length} reference(s) name a docs/verification/V#.md that does not exist:`,
          '',
          ...advisory.map((a) => `- ${a}`),
          '',
          'These are false "Verified by V# · PASS" badge claims. Generating the missing reports is a',
          'separate verification task; this gate neither fabricates them nor silently drops the claim.',
          'Tracked here so the gap stays visible until the reports exist or the badges are corrected.',
        ].join('\n'),
    '',
    '## Limits',
    '',
    '- Asserts path resolution on disk, not that a doc\'s prose matches its sheet (content drift is out of scope).',
    '- Verification-reference resolution is advisory this revision; the hard invariants are doc_path / fsm_path resolution, .fsm coverage, and map-key hygiene.',
    '- Map literals are extracted from literate-fsm.ts by regex (flat Record literals); a reformatted map would need the extractor updated.',
    '',
    '---',
    '*Generated by `node web/tests/registry.mjs` — do not edit by hand.*',
    '',
  ].join('\n');
  if (!ok) {
    const bad = checks.filter((c) => !c.pass);
    console.error(['registry: FAIL — failed checks:', ...bad.map((c) => `  ✗ ${c.name}: ${c.detail}`)].join('\n'));
    if (advisory.length) console.error(`registry: advisory — ${advisory.length} dangling verification ref(s): ${advisory.join(', ')}`);
  } else {
    writeFileSync(resolve(docsDir, 'registry-e2e.md'), lines);
    console.log(`registry: PASS — ${examples.length} examples, ${Object.keys(docMap).length} doc-map / ${Object.keys(fsmMap).length} fsm-map entries resolve`);
    if (advisory.length) console.log(`registry: advisory — ${advisory.length} dangling verification ref(s) (see registry-e2e.md): ${advisory.join(', ')}`);
  }
  process.exit(ok ? 0 : 1);
} catch (e) {
  console.error(`registry: run aborted: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
}