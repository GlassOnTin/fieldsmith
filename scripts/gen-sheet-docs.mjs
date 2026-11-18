// Generate markdown mirrors for literate `.fsm` sheets that lack one, and emit docs/README
// index rows for them. The mirror is rendered faithfully from the sheet's literate header
// (the source of truth): provenance, @check table, the starter .fsm body, @step walkthrough,
// @theory manual, and @mcp automation. Pending sheets ( @verification_status: pending ) get
// an honest "Pending verification" header, never a PASS claim.
//
// This closes the documentation flywheel: a contributed .fsm with a literate header can be
// documented by running this script, and web/tests/sheetdocs.mjs gates that every shipped
// sheet is indexed and mirrored with honest provenance.
//
// Run: node scripts/gen-sheet-docs.mjs            (generate missing mirrors + print index rows)
//      node scripts/gen-sheet-docs.mjs --all      (regenerate every mirror)
import { readFileSync, writeFileSync, readdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..');
const all = process.argv.includes('--all');
const onlyArg = process.argv.find((a) => a.startsWith('--only='));
const onlyIds = new Set(onlyArg ? onlyArg.slice(7).split(',') : []);

const CAT = {
  analytic: { dir: 'docs/analytic-tests', example: 'examples/analytic', label: 'Analytic Verification Test' },
  tutorial: { dir: 'docs/tutorials', example: 'examples/tutorials', label: 'Guided Tutorial' },
  'case-study': { dir: 'docs/case-studies', example: 'examples/case-studies', label: 'Multiphysics Case Study' },
};

// Parse a literate header into ordered directives (multi-line continuations joined) + body.
function parseLiterate(raw) {
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  const start = lines.findIndex((l) => l.trim() === '# === FIELDSMITH LITERATE CASE ===');
  const end = lines.findIndex((l) => l.trim() === '# === END LITERATE HEADER ===');
  const directives = [];
  let cur = null;
  if (start >= 0 && end > start) {
    for (let i = start + 1; i < end; i++) {
      const line = lines[i];
      const m = line.match(/^#\s*@([a-z_]+):\s?(.*)$/);
      if (m) { cur = { tag: m[1], value: m[2] }; directives.push(cur); continue; }
      const cont = line.match(/^#\s{2,}(.*)$/);
      if (cont && cur) { cur.value += (cur.value ? '\n' : '') + cont[1]; continue; }
      if (line.trim() === '# ---') { cur = null; }
    }
  }
  const body = end >= 0 ? lines.slice(end + 1).join('\n').trim() : raw.trim();
  return { directives, body };
}

const get = (d, tag) => d.find((x) => x.tag === tag)?.value?.trim() ?? '';
const getAll = (d, tag) => d.filter((x) => x.tag === tag).map((x) => x.value.trim());

function renderMirror(sheet, mirrorPath) {
  const { directives: d, body } = sheet;
  const id = get(d, 'id');
  const title = get(d, 'title');
  const category = get(d, 'category');
  const level = get(d, 'level') || 'Intermediate';
  const pending = get(d, 'verification_status') === 'pending';
  const badge = pending ? 'Pending verification (no shipped report yet)' : (get(d, 'verification_badge') || 'Pending verification');
  const starter = get(d, 'starter_fsm_name') || `${id.toLowerCase()}.fsm`;
  const refs = get(d, 'verification_refs').split(',').map((s) => s.trim()).filter(Boolean);
  const reports = refs.filter((r) => /^docs\/verification\/V\d+.*\.md$/.test(r));
  const reportCell = reports.length ? reports.map((r) => `\`${r}\``).join(', ') : '—';
  const summary = get(d, 'summary') || get(d, 'subtitle');

  const out = [];
  out.push(`# ${id} — ${title}`, '');
  out.push(`> **Category**: ${CAT[category]?.label ?? category} · **Level**: ${level} · **Verification**: ${badge}`);
  out.push(`> **Repository Path**: \`${mirrorPath}\` · **Starter Model**: \`${starter}\` · **Verification Reports**: ${reportCell}`, '');
  if (summary) out.push(summary, '');

  // @check table.
  const checks = getAll(d, 'check').map((c) => c.split('||').map((s) => s.trim()));
  if (checks.length) {
    out.push('## Closed-Form Analytic Reference vs Fieldsmith P2 FEA', '');
    out.push('| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |');
    out.push('| :--- | :--- | ---: | ---: | ---: | :--- |');
    const esc = (s) => s.replace(/\|/g, '\\|');
    for (const c of checks) {
      const [q = '', formula = '', ref = '', meas = '', err = '', tol = ''] = c;
      out.push(`| ${esc(q)} | \`${esc(formula)}\` | **${esc(ref)}** | **${esc(meas)}** | ${esc(err)} | ${esc(tol)} |`);
    }
    out.push('');
  }

  // Starter model.
  out.push('## 1. Starter `.fsm` Model', '');
  out.push('Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):', '');
  out.push('```fsm', body, '```', '');

  // Steps: @step (title || lens) followed by its body and @expected.
  const steps = [];
  for (let i = 0; i < d.length; i++) {
    if (d[i].tag !== 'step') continue;
    // The @step value is "title || lens" on the first line; the body lines follow as
    // indented continuations joined into the same value.
    const valParts = d[i].value.split('\n');
    const [stitleRaw, lens] = valParts[0].split('||').map((s) => s.trim());
    const stitle = stitleRaw.replace(/^\d+\.\s*/, '');
    const sbody = valParts.slice(1).join('\n').trim();
    let expected = '';
    for (let j = i + 1; j < d.length && d[j].tag !== 'step'; j++) {
      if (d[j].tag === 'expected') expected = d[j].value.trim();
      else if (d[j].tag === 'theory_title') break;
    }
    steps.push({ stitle, lens, sbody, expected });
  }
  if (steps.length) {
    out.push('## 2. Step-by-Step Guided Walkthrough', '');
    steps.forEach((s, i) => {
      out.push(`### ${i + 1}. ${s.stitle}${s.lens ? ` *(Lens: \`${s.lens.toUpperCase()}\`)*` : ''}`, '');
      if (s.sbody) out.push(s.sbody, '');
      if (s.expected) out.push(`- **Expected Check**: ${s.expected}`, '');
    });
  }

  // Theory manual.
  const theoryTitle = get(d, 'theory_title');
  if (theoryTitle) {
    out.push(`## 3. Theoretical Manual — ${theoryTitle}`, '');
    const theorySummary = get(d, 'theory_summary');
    if (theorySummary) out.push(theorySummary, '');
    for (let i = 0; i < d.length; i++) {
      if (d[i].tag !== 'theory_section') continue;
      out.push(`### ${d[i].value.trim()}`, '');
      for (let j = i + 1; j < d.length && d[j].tag !== 'theory_section'; j++) {
        if (d[j].tag === 'theory_eq') out.push('```text', d[j].value.trim(), '```', '');
        else if (d[j].tag === 'theory_body') out.push(d[j].value.trim(), '');
        else if (d[j].tag === 'mcp') break;
      }
    }
  }

  // MCP automation.
  const mcps = getAll(d, 'mcp').map((m) => m.split('||').map((s) => s.trim()));
  if (mcps.length) {
    out.push('## 4. Reproducible Automation (MCP / REST API & CLI)', '');
    for (const [mtitle, tool, args] of mcps) {
      out.push(`### ${mtitle || tool}`, '');
      if (tool) {
        let argsObj = {};
        try { argsObj = JSON.parse(args || '{}'); } catch { argsObj = { raw: args }; }
        out.push('```json', JSON.stringify({ tool, arguments: argsObj }, null, 2), '```', '');
      }
    }
  }

  out.push('---', `*Generated from \`${get(d, 'fsm_path') || sheet.rel}\` by \`node scripts/gen-sheet-docs.mjs\`.*`, '');
  return out.join('\n');
}

// Index row per category, matching the docs/README table columns.
function indexRow(sheet, mirrorRel) {
  const { directives: d } = sheet;
  const bt = '`';
  const id = get(d, 'id');
  const title = get(d, 'title');
  const category = get(d, 'category');
  const level = get(d, 'level') || 'Intermediate';
  const pending = get(d, 'verification_status') === 'pending';
  const badge = pending ? 'Pending verification' : (get(d, 'verification_badge') || '—');
  const refs = get(d, 'verification_refs').split(',').map((s) => s.trim()).filter((r) => /^docs\/verification\/V\d+.*\.md$/.test(r));
  const reportLinks = refs.length ? refs.map((r) => {
    const stem = r.match(/V\d+[^/]*?\.md$/)[0];
    return `[${bt}${stem.replace(/\.md$/, '')}${bt}](verification/${stem})`;
  }).join(', ') : '—';
  const fsmRel = get(d, 'fsm_path') || sheet.rel;
  const fsmLink = `[${bt}${fsmRel.split('/').pop()}${bt}](../${fsmRel})`;
  const docLink = `[${id} — ${title}](${mirrorRel.replace(/^docs\//, '')})`;
  const checks = getAll(d, 'check').map((c) => c.split('||').map((s) => s.trim()));
  const measSummary = checks.length ? checks.map((c) => c[3] || c[2]).join('; ') : '—';
  if (category === 'analytic') {
    return `| **${id}** | ${docLink} | ${badge} | ${measSummary} | ${reportLinks} |`;
  }
  if (category === 'tutorial') {
    return `| **${id}** | ${docLink} | ${level} | ${fsmLink} | ${reportLinks} |`;
  }
  return `| **${id}** | ${docLink} | ${get(d, 'subtitle') || badge} | ${fsmLink} | ${reportLinks} |`;
}

const rows = { analytic: [], tutorial: [], 'case-study': [] };
let generated = 0;
for (const [category, cfg] of Object.entries(CAT)) {
  const exAbs = resolve(repo, cfg.example);
  if (!existsSync(exAbs)) continue;
  for (const f of readdirSync(exAbs).filter((f) => f.endsWith('.fsm')).sort()) {
    const rel = join(cfg.example, f);
    const raw = readFileSync(resolve(repo, rel), 'utf8');
    const sheet = { rel, ...parseLiterate(raw) };
    const id = get(sheet.directives, 'id');
    if (!id) continue;
    if (onlyIds.size && !onlyIds.has(id)) continue;
    const mirrorFile = f.replace(/\.fsm$/, '.md');
    const mirrorRel = join(cfg.dir, mirrorFile);
    const mirrorAbs = resolve(repo, mirrorRel);
    if (all || !existsSync(mirrorAbs)) {
      writeFileSync(mirrorAbs, renderMirror(sheet, mirrorRel));
      generated++;
      console.log(`generated ${mirrorRel}`);
    }
    rows[category].push({ id, row: indexRow(sheet, mirrorRel) });
  }
}

console.log('\n===== INDEX ROWS (append to the matching docs/README.md table) =====');
for (const [category, list] of Object.entries(rows)) {
  console.log(`\n--- ${category} ---`);
  for (const { row } of list.sort((a, b) => parseInt(a.id.slice(1)) - parseInt(b.id.slice(1)))) console.log(row);
}
console.log(`\n${generated} mirror(s) generated.`);
