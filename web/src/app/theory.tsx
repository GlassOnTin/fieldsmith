// Theory (6) Lens & Applied Mathematics Report Generator:
// Dynamically inspects the live Document at any design stage (Shape 1, Setup 2, Results 3, Fill 4, Make 5)
// to document the active continuum subdomains, boundary value problems, automatically activated
// governing field equations (strong + weak variational forms), and solved verification metrics.

import { useState } from 'preact/hooks';
import { store } from '../doc/store';
import {
  collectVolumeContinuumNodes,
  detectCoupledPhysicsRegime,
  evaluateDocumentFieldEquations,
  generateAppliedMathMarkdownReport,
  synthesiseVariationalBvpSummary,
  type DesignStageId,
} from '../doc/continuum';
import {
  evaluateLiterateAssertOnLevel,
  findMatchingLiterateSheet,
} from '../doc/literate-fsm';
import { viewOverlays } from './App';
import { analysis } from './analysis';
import { fmt } from './fields';
import { ui, useApp } from './ui';

type StageFilter = 'all' | DesignStageId;

const STAGE_FILTERS: { id: StageFilter; label: string }[] = [
  { id: 'all', label: 'All Stages (1–5)' },
  { id: 'shape', label: '1. Shape (CSG & Continuum)' },
  { id: 'setup', label: '2. Setup (BCs & Active PDEs)' },
  { id: 'results', label: '3. Results (P2 FEM & Bounds)' },
  { id: 'fill', label: '4. Fill (TPMS & Level-Set)' },
  { id: 'make', label: '5. Make (Peel & Slicing)' },
];

function downloadBlob(filename: string, content: string, mime: string) {
  const blob = new Blob([content], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

function slugifyName(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'fieldsmith-model'
  );
}

function TheoryLensView() {
  useApp();
  const [stageFilter, setStageFilter] = useState<StageFilter>('all');
  const [showStandby, setShowStandby] = useState<boolean>(false);
  const [showMarkdownPreview, setShowMarkdownPreview] = useState<boolean>(false);
  const [copiedFlag, setCopiedFlag] = useState<boolean>(false);

  if (store.lens !== 'theory') return null;

  const doc = store.doc;
  const vols = collectVolumeContinuumNodes(doc);
  const allEqs = evaluateDocumentFieldEquations(doc);
  const activeCount = allEqs.filter((e) => e.active).length;
  const regime = detectCoupledPhysicsRegime(doc);

  const lvl = analysis.launchedFor === store.docVersion ? analysis.latest : null;
  const firstCase = lvl?.meta.cases[0];
  const solvedSummary = lvl
    ? {
        dofs: lvl.meta.provenance.dofs,
        hMm: lvl.meta.provenance.element_size,
        maxVmMpa: firstCase?.max_von_mises,
        maxDispMm: firstCase?.max_displacement,
        maxTempC:
          lvl.temperature?.[0] && lvl.temperature[0].length > 0
            ? Math.max(...Array.from(lvl.temperature[0]))
            : undefined,
        massG: lvl.meta.mass_g,
      }
    : undefined;

  const bvp = synthesiseVariationalBvpSummary(doc, solvedSummary);
  const litSheet = findMatchingLiterateSheet(doc.name);
  const liveAsserts =
    litSheet && lvl
      ? (litSheet.asserts ?? []).map((a) => evaluateLiterateAssertOnLevel(a, lvl))
      : [];

  const filteredEqs = allEqs.filter((eq) => {
    if (stageFilter !== 'all' && eq.stage !== stageFilter) return false;
    if (!showStandby && !eq.active) return false;
    return true;
  });

  const baseMdReport = generateAppliedMathMarkdownReport(
    doc,
    stageFilter === 'all' ? 'all stages' : stageFilter,
    solvedSummary,
    showStandby,
  );
  const litMdLines: string[] = [];
  if (litSheet) {
    litMdLines.push('');
    litMdLines.push(`## 5. Closed-Form Analytical Reference & Verification (${litSheet.id}: ${litSheet.title})`);
    litMdLines.push(`**Chapter**: ${litSheet.theoryChapter.chapterTitle} (\`${litSheet.verificationBadge}\`)`);
    litMdLines.push('');
    litMdLines.push(litSheet.theoryChapter.summary);
    litMdLines.push('');
    for (const sec of litSheet.theoryChapter.sections) {
      litMdLines.push(`### ${sec.heading}`);
      litMdLines.push('```text');
      litMdLines.push(sec.equations.trim());
      litMdLines.push('```');
      litMdLines.push(sec.body);
      litMdLines.push('');
    }
    if (litSheet.analyticChecks && litSheet.analyticChecks.length > 0) {
      litMdLines.push('### Closed-Form Reference vs Fieldsmith P2 FEA');
      litMdLines.push('| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |');
      litMdLines.push('|---|---|---|---|---|---|');
      for (const ck of litSheet.analyticChecks) {
        litMdLines.push(
          `| ${ck.quantity} | \`${ck.formula}\` | **${ck.analyticRef}** | **${ck.measuredFea}** | ${ck.relError} | ${ck.tolerance} |`,
        );
      }
      litMdLines.push('');
    }
    if (liveAsserts.length > 0) {
      litMdLines.push('### Live `# @assert` Verification Contracts');
      for (const ev of liveAsserts) {
        const u = ev.assert.unit ? ` ${ev.assert.unit}` : '';
        litMdLines.push(
          `- **${ev.passed ? 'PASS' : 'FAIL'}** · ${ev.assert.label} (\`${ev.assert.metric}\`): live \`${ev.liveValue != null ? fmt(ev.liveValue, 4) : 'N/A'}${u}\` vs target \`${ev.assert.target}${u}\` (${ev.errorOrMargin})`,
        );
      }
      litMdLines.push('');
    }
  }
  const mdReport = baseMdReport + litMdLines.join('\n');

  const handleCopyMd = () => {
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(mdReport).catch(() => {});
    }
    setCopiedFlag(true);
    ui.notify('Applied Mathematics Markdown report copied to clipboard.', 'info');
    setTimeout(() => setCopiedFlag(false), 2500);
  };

  const handleDownloadMd = () => {
    const fname = `${slugifyName(doc.name)}-applied-mathematics.md`;
    downloadBlob(fname, mdReport, 'text/markdown;charset=utf-8');
    ui.notify(`Downloaded ${fname}`, 'info');
  };

  const handleDownloadHtml = () => {
    const fname = `${slugifyName(doc.name)}-applied-mathematics.html`;
    const escapedMd = mdReport
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
    const html = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>Applied Mathematics Specification — ${doc.name}</title>
<style>
  body { font-family: system-ui, -apple-system, sans-serif; background: #0f172a; color: #e2e8f0; max-width: 980px; margin: 32px auto; padding: 0 24px; line-height: 1.55; }
  h1, h2, h3 { color: #38bdf8; }
  pre { background: #020617; border: 1px solid #1e293b; border-radius: 8px; padding: 16px; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 13px; color: #f8fafc; }
</style>
</head>
<body>
  <h1>Applied Mathematics &amp; Active Field Equations Specification</h1>
  <p>Generated by Fieldsmith Theory (6) for model <strong>${doc.name}</strong>.</p>
  <pre>${escapedMd}</pre>
</body>
</html>`;
    downloadBlob(fname, html, 'text/html;charset=utf-8');
    ui.notify(`Downloaded ${fname}`, 'info');
  };

  return (
    <div
      data-testid="theory-view-page"
      style={{
        position: 'absolute',
        inset: 0,
        zIndex: 30,
        background: 'color-mix(in srgb, var(--bg) 96%, transparent)',
        backdropFilter: 'blur(10px)',
        color: 'var(--text)',
        overflowY: 'auto',
        padding: '76px 18px 48px',
        display: 'flex',
        flexDirection: 'column',
        gap: '16px',
      }}
    >
      {/* Header & Report Generator Bar */}
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          gap: '12px',
          borderBottom: '1px solid var(--line-strong)',
          paddingBottom: '14px',
        }}
      >
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '10px', flexWrap: 'wrap' }}>
            <span
              style={{
                fontSize: '11px',
                fontWeight: 700,
                padding: '2px 8px',
                borderRadius: '999px',
                background: 'var(--accent-soft)',
                color: 'var(--accent)',
                border: '1px solid color-mix(in srgb, var(--accent) 45%, var(--line-strong))',
                fontFamily: 'var(--mono)',
              }}
            >
              THEORY &amp; APPLIED MATHEMATICS SPEC · LENS 6
            </span>
            <span
              data-testid="theory-active-count-badge"
              style={{
                fontSize: '11px',
                fontWeight: 700,
                padding: '2px 8px',
                borderRadius: '999px',
                background: 'color-mix(in srgb, var(--ok) 15%, var(--panel))',
                color: 'var(--ok)',
                border: '1px solid color-mix(in srgb, var(--ok) 45%, var(--line-strong))',
              }}
            >
              {activeCount} / {allEqs.length} Governing PDEs Active
            </span>
            {regime && (
              <span
                data-testid="theory-coupled-regime-badge"
                style={{
                  fontSize: '11px',
                  fontWeight: 700,
                  padding: '2px 8px',
                  borderRadius: '999px',
                  background: 'color-mix(in srgb, var(--warn) 16%, var(--panel))',
                  color: 'var(--warn)',
                  border: '1px solid color-mix(in srgb, var(--warn) 45%, var(--line-strong))',
                }}
              >
                Coupled Regime: {regime}
              </span>
            )}
          </div>
          <h2 style={{ margin: '6px 0 3px', fontSize: '18px', fontWeight: 700, color: 'var(--text)' }}>
            {doc.name} — Live Mathematical Formulation
          </h2>
          <div style={{ fontSize: '12px', color: 'var(--text-2)' }}>
            Governing continuum laws, boundary operators, and finite-element / finite-volume discretizations automatically activated by your <strong>Shape (1)</strong> volume materials and <strong>Setup (2)</strong> boundary conditions.
          </div>
        </div>

        <div
          data-testid="theory-report-toolbar"
          style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', alignItems: 'center' }}
        >
          <button
            type="button"
            class="btn small"
            data-testid="theory-close-btn"
            title="Close Theory view and return to 3D viewport (1)"
            onClick={() => store.setLens('shape')}
          >
            ✕ Close Theory (3D View)
          </button>
          <button
            type="button"
            class={`btn small ${showMarkdownPreview ? 'primary' : ''}`}
            data-testid="theory-preview-md"
            onClick={() => setShowMarkdownPreview((v) => !v)}
          >
            {showMarkdownPreview ? 'Structured View' : 'Markdown Report Preview'}
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="theory-copy-md"
            onClick={handleCopyMd}
          >
            {copiedFlag ? '✓ Copied .md' : 'Copy Report (.md)'}
          </button>
          <button
            type="button"
            class="btn small primary"
            data-testid="theory-export-md"
            onClick={handleDownloadMd}
          >
            Export .md
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="theory-export-html"
            onClick={handleDownloadHtml}
          >
            Export .html
          </button>
        </div>
      </div>

      {/* Design Stage Filter Bar */}
      <div
        data-testid="theory-stage-filter"
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '8px',
        }}
      >
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px' }}>
          {STAGE_FILTERS.map((sf) => (
            <button
              key={sf.id}
              type="button"
              class={`btn small ${stageFilter === sf.id ? 'primary' : ''}`}
              data-testid={`theory-filter-${sf.id}`}
              onClick={() => setStageFilter(sf.id)}
            >
              {sf.label}
            </button>
          ))}
        </div>
        <label class="check" style={{ fontSize: '11.5px', color: 'var(--text-2)', cursor: 'pointer' }}>
          <input
            type="checkbox"
            checked={showStandby}
            data-testid="theory-toggle-standby"
            onChange={(e) => setShowStandby((e.target as HTMLInputElement).checked)}
          />
          <span>Show standby equations (with activation instructions)</span>
        </label>
      </div>

      {showMarkdownPreview ? (
        <pre
          data-testid="theory-markdown-preview"
          style={{
            background: 'var(--panel)',
            border: '1px solid var(--line-strong)',
            borderRadius: '8px',
            padding: '16px',
            fontFamily: 'var(--mono)',
            fontSize: '12px',
            lineHeight: 1.55,
            color: 'var(--text)',
            whiteSpace: 'pre-wrap',
            margin: 0,
          }}
        >
          {mdReport}
        </pre>
      ) : (
        <>
          {/* Section 0: Synthesised Variational Boundary-Value Problem & Sobolev Spaces */}
          <div
            class="card"
            data-testid="theory-variational-statement"
            style={{
              padding: '12px 14px',
              background: 'var(--panel)',
              border: '1.5px solid color-mix(in srgb, var(--accent) 50%, var(--line-strong))',
              borderRadius: '8px',
              margin: 0,
              display: 'flex',
              flexDirection: 'column',
              gap: '7px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
              <span style={{ fontSize: '13px', fontWeight: 700, color: 'var(--accent)' }}>
                0. Synthesised Variational Boundary-Value Problem &amp; Sobolev Function Spaces
              </span>
              <span style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                Conforming P₂ Lagrange Finite Elements · Keast Degree-2 Quadrature
              </span>
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: '8px' }}>
              <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', borderRadius: '6px', padding: '7px 10px' }}>
                <div style={{ fontSize: '10px', fontWeight: 700, color: 'var(--text-2)', marginBottom: '3px' }}>
                  COMPUTATIONAL DOMAIN &amp; BOUNDARY PARTITION
                </div>
                <div style={{ fontFamily: 'var(--mono)', fontSize: '11px', color: 'var(--text)', wordBreak: 'break-word' }}>
                  {bvp.domainLine}
                </div>
              </div>
              <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', borderRadius: '6px', padding: '7px 10px' }}>
                <div style={{ fontSize: '10px', fontWeight: 700, color: 'var(--text-2)', marginBottom: '3px' }}>
                  SOBOLEV TRIAL &amp; TEST SPACES
                </div>
                <div style={{ fontFamily: 'var(--mono)', fontSize: '11px', color: 'var(--text)', wordBreak: 'break-word' }}>
                  {bvp.sobolevSpaceLine}
                </div>
              </div>
            </div>
            <div
              style={{
                background: 'var(--panel-2)',
                border: '1px solid var(--line-strong)',
                borderLeft: '3px solid var(--accent)',
                borderRadius: '6px',
                padding: '8px 10px',
              }}
            >
              <div style={{ fontSize: '10px', fontWeight: 700, color: 'var(--accent)', marginBottom: '3px' }}>
                COUPLED WEAK VARIATIONAL FORMULATION
              </div>
              <div style={{ fontFamily: 'var(--mono)', fontSize: '11.5px', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                {bvp.weakStatementLine}
              </div>
              <div style={{ fontFamily: 'var(--mono)', fontSize: '10.5px', color: 'var(--text-2)', marginTop: '4px' }}>
                Discrete Operator: {bvp.discreteAlgebraicLine}
              </div>
            </div>
          </div>

          {/* Section 1: Shape (1) Subdomain Continuum & Constitutive Table */}
          {(stageFilter === 'all' || stageFilter === 'shape') && (
            <div
              class="card"
              data-testid="theory-subdomains-table"
              style={{
                padding: '12px 14px',
                background: 'var(--panel)',
                border: '1px solid var(--line-strong)',
                borderRadius: '8px',
                margin: 0,
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                <span style={{ fontSize: '13px', fontWeight: 700, color: 'var(--accent)' }}>
                  1. Subdomain Continuum &amp; Constitutive Law Assignment (Shape 1)
                </span>
                <button
                  type="button"
                  class="btn ghost small"
                  onClick={() => store.setLens('shape')}
                  style={{ fontSize: '11px' }}
                >
                  Edit Materials in Shape (1) →
                </button>
              </div>
              <div style={{ overflowX: 'auto' }}>
                <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '11.5px' }}>
                  <thead>
                    <tr style={{ borderBottom: '1px solid var(--line-strong)', color: 'var(--text-2)', textAlign: 'left' }}>
                      <th style={{ padding: '5px 8px' }}>Volume Node</th>
                      <th style={{ padding: '5px 8px' }}>CSG Op</th>
                      <th style={{ padding: '5px 8px' }}>Assigned Material</th>
                      <th style={{ padding: '5px 8px' }}>Continuum Phase</th>
                      <th style={{ padding: '5px 8px' }}>Governing Constitutive Law</th>
                    </tr>
                  </thead>
                  <tbody>
                    {vols.map((v) => (
                      <tr
                        key={v.node.id}
                        style={{ borderBottom: '1px solid var(--line)' }}
                      >
                        <td style={{ padding: '6px 8px', fontFamily: 'var(--mono)', fontWeight: 600 }}>
                          <button
                            type="button"
                            class="btn ghost small"
                            style={{ padding: '1px 6px', fontFamily: 'var(--mono)', fontSize: '11px' }}
                            onClick={() => {
                              store.select({ kind: 'node', id: v.node.id });
                              store.setLens('shape');
                            }}
                          >
                            {v.node.id}
                          </button>
                        </td>
                        <td style={{ padding: '6px 8px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                          {v.node.op}
                        </td>
                        <td style={{ padding: '6px 8px', color: 'var(--text)' }}>
                          {v.spec.name} <code style={{ color: 'var(--text-3)' }}>({v.matId})</code>
                        </td>
                        <td style={{ padding: '6px 8px' }}>
                          <span
                            style={{
                              fontSize: '10.5px',
                              fontWeight: 700,
                              padding: '2px 7px',
                              borderRadius: '5px',
                              background: `color-mix(in srgb, ${v.spec.badgeColor} 14%, var(--panel-2))`,
                              color: `color-mix(in srgb, ${v.spec.badgeColor} 68%, var(--text))`,
                              border: `1px solid color-mix(in srgb, ${v.spec.badgeColor} 40%, var(--line-strong))`,
                            }}
                          >
                            {v.spec.phaseLabel}
                          </span>
                        </td>
                        <td
                          style={{
                            padding: '6px 8px',
                            fontFamily: 'var(--mono)',
                            fontSize: '11px',
                            color: 'var(--text)',
                          }}
                        >
                          {v.spec.constitutiveEquation}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Section 2: Setup (2) Boundary Value Problem & Coupling Table */}
          {(stageFilter === 'all' || stageFilter === 'setup') && (
            <div
              class="card"
              data-testid="theory-bcs-table"
              style={{
                padding: '12px 14px',
                background: 'var(--panel)',
                border: '1px solid var(--line-strong)',
                borderRadius: '8px',
                margin: 0,
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
                <span style={{ fontSize: '13px', fontWeight: 700, color: 'var(--accent)' }}>
                  2. Boundary Value Problem (Γ_D, Γ_N, Γ_R) &amp; Multiphysics Couplings (Setup 2)
                </span>
                <button
                  type="button"
                  class="btn ghost small"
                  onClick={() => store.setLens('setup')}
                  style={{ fontSize: '11px' }}
                >
                  Edit Boundary Conditions in Setup (2) →
                </button>
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))', gap: '8px' }}>
                <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', padding: '8px 10px', borderRadius: '6px' }}>
                  <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--ok)', marginBottom: '4px' }}>
                    Dirichlet Supports (Γ_D: u = 0)
                  </div>
                  {doc.supports.length === 0 ? (
                    <div style={{ fontSize: '11px', color: 'var(--text-3)' }}>None defined</div>
                  ) : (
                    doc.supports.map((s) => (
                      <div key={s.id} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                        • {s.id} on {s.region}: u_[{['x', 'y', 'z'].filter((_, i) => s.fixed[i]).join('')}] = 0
                      </div>
                    ))
                  )}
                </div>

                <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', padding: '8px 10px', borderRadius: '6px' }}>
                  <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--danger)', marginBottom: '4px' }}>
                    Neumann Mechanical Loads (Γ_N: σ·n = t̂)
                  </div>
                  {doc.loads.length === 0 ? (
                    <div style={{ fontSize: '11px', color: 'var(--text-3)' }}>
                      {doc.temperatures.length + doc.convections.length + doc.heat_fluxes.length > 0
                        ? 'Thermal-driven (zero external mechanical traction)'
                        : 'None defined'}
                    </div>
                  ) : (
                    doc.loads.map((l) => (
                      <div key={l.id} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                        • {l.id} on {l.region}:{' '}
                        {l.kind.type === 'force'
                          ? `F = (${l.kind.vector.join(', ')}) N`
                          : l.kind.type === 'pressure'
                            ? `p = ${l.kind.mpa} MPa`
                            : l.kind.type === 'preload'
                              ? `F_preload = ${l.kind.newtons} N`
                              : l.kind.type === 'distributed'
                                ? `p(x,y,z) = ${l.kind.expr}`
                                : (l.kind as { type: string }).type}
                      </div>
                    ))
                  )}
                </div>

                <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', padding: '8px 10px', borderRadius: '6px' }}>
                  <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--warn)', marginBottom: '4px' }}>
                    Thermal Boundary Operators (Γ_T, Γ_R, Γ_q)
                  </div>
                  {doc.temperatures.length + doc.convections.length + doc.heat_fluxes.length === 0 ? (
                    <div style={{ fontSize: '11px', color: 'var(--text-3)' }}>Isothermal (T = T_ref)</div>
                  ) : (
                    <>
                      {doc.temperatures.map((t) => (
                        <div key={t.id} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                          • {t.id} ({t.region}): T = {t.celsius} °C
                        </div>
                      ))}
                      {doc.convections.map((c) => (
                        <div key={c.id} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                          • {c.id} ({c.region}): −k∇T·n = {c.h}(T − {c.t_inf} °C)
                        </div>
                      ))}
                      {doc.heat_fluxes.map((q) => (
                        <div key={q.id} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                          • {q.id} ({q.region}): q_n = {q.q} W/mm²
                        </div>
                      ))}
                    </>
                  )}
                </div>

                <div style={{ background: 'var(--panel-2)', border: '1px solid var(--line)', padding: '8px 10px', borderRadius: '6px' }}>
                  <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--accent)', marginBottom: '4px' }}>
                    Coupled PDE / Design Parameters (let)
                  </div>
                  {(doc.lets?.length ?? 0) === 0 ? (
                    <div style={{ fontSize: '11px', color: 'var(--text-3)' }}>Default regime constants active</div>
                  ) : (
                    doc.lets!.map((l) => (
                      <div key={l.name} style={{ fontSize: '11px', fontFamily: 'var(--mono)', color: 'var(--text)' }}>
                        • {l.name} = {l.value}
                      </div>
                    ))
                  )}
                </div>
              </div>
            </div>
          )}

          {/* Section 3: Governing Field Equations Cards */}
          <div data-testid="theory-equations-list" style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
            <div style={{ fontSize: '13px', fontWeight: 700, color: 'var(--accent)' }}>
              3. Governing Field Equations &amp; Variational Discretizations ({filteredEqs.length} shown)
            </div>
            {filteredEqs.map((eq) => (
              <div
                key={eq.id}
                data-testid={`theory-eq-${eq.id}`}
                style={{
                  padding: '12px 14px',
                  borderRadius: '8px',
                  background: 'var(--panel)',
                  border: eq.active
                    ? '1.5px solid color-mix(in srgb, var(--accent) 55%, var(--line-strong))'
                    : '1px solid var(--line)',
                  opacity: eq.active ? 1 : 0.72,
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    gap: '8px',
                    flexWrap: 'wrap',
                    marginBottom: '6px',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <span
                      style={{
                        fontSize: '10px',
                        fontWeight: 700,
                        padding: '2px 6px',
                        borderRadius: '4px',
                        background: 'var(--panel-2)',
                        border: '1px solid var(--line)',
                        color: 'var(--text-2)',
                        fontFamily: 'var(--mono)',
                      }}
                    >
                      STAGE {eq.stageNumber} · {eq.stage.toUpperCase()}
                    </span>
                    <span style={{ fontSize: '13.5px', fontWeight: 700, color: 'var(--text)' }}>
                      {eq.title}
                    </span>
                  </div>
                  <span
                    style={{
                      fontSize: '10.5px',
                      fontWeight: 700,
                      padding: '2px 8px',
                      borderRadius: '999px',
                      background: eq.active
                        ? 'color-mix(in srgb, var(--ok) 15%, var(--panel-2))'
                        : 'var(--panel-2)',
                      color: eq.active ? 'var(--ok)' : 'var(--text-3)',
                      border: eq.active
                        ? '1px solid color-mix(in srgb, var(--ok) 45%, var(--line-strong))'
                        : '1px solid var(--line)',
                    }}
                  >
                    {eq.active ? '● ACTIVE IN SOLVE' : '○ STANDBY'}
                  </span>
                </div>

                <div style={{ fontSize: '11.5px', color: 'var(--text-2)', marginBottom: '8px' }}>
                  {eq.triggerSummary}
                </div>

                <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: '8px', marginBottom: '8px' }}>
                  <div
                    style={{
                      background: 'var(--panel-2)',
                      border: '1px solid var(--line-strong)',
                      borderLeft: '3px solid var(--accent)',
                      borderRadius: '6px',
                      padding: '8px 10px',
                    }}
                  >
                    <div style={{ fontSize: '10px', fontWeight: 700, color: 'var(--accent)', marginBottom: '4px' }}>
                      STRONG DIFFERENTIAL FORM
                    </div>
                    <div style={{ fontFamily: 'var(--mono)', fontSize: '11.5px', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                      {eq.strongForm}
                    </div>
                  </div>
                  <div
                    style={{
                      background: 'var(--panel-2)',
                      border: '1px solid var(--line-strong)',
                      borderLeft: '3px solid color-mix(in srgb, #a855f7 68%, var(--text))',
                      borderRadius: '6px',
                      padding: '8px 10px',
                    }}
                  >
                    <div style={{ fontSize: '10px', fontWeight: 700, color: 'color-mix(in srgb, #a855f7 68%, var(--text))', marginBottom: '4px' }}>
                      WEAK VARIATIONAL / DISCRETE FORM
                    </div>
                    <div style={{ fontFamily: 'var(--mono)', fontSize: '11.5px', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                      {eq.weakOrDiscreteForm}
                    </div>
                  </div>
                </div>

                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                  {eq.liveParameters.map((lp, idx) => (
                    <span
                      key={idx}
                      style={{
                        fontSize: '11px',
                        fontFamily: 'var(--mono)',
                        padding: '3px 8px',
                        borderRadius: '4px',
                        background: 'var(--panel-2)',
                        border: '1px solid var(--line)',
                        color: 'var(--text-2)',
                      }}
                    >
                      {lp}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* Section 4: Solved Discretization Readout & Closed-Form Literate .fsm Verification */}
          {(solvedSummary || litSheet) && (
            <div
              class="card"
              data-testid="theory-literate-verification"
              style={{
                padding: '12px 14px',
                background: 'var(--panel)',
                border: '1px solid var(--line-strong)',
                borderRadius: '8px',
                margin: 0,
                display: 'flex',
                flexDirection: 'column',
                gap: '10px',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
                <span style={{ fontSize: '13px', fontWeight: 700, color: 'var(--accent)' }}>
                  4. Closed-Form Analytical Reference, Solved Discretization &amp; Verification Contracts
                </span>
                {litSheet && (
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="theory-open-sheet-btn"
                    onClick={() => {
                      ui.set('tutorialSheetId', litSheet.id);
                      ui.set('tutorialMinimised', false);
                      ui.set('tutorialsOpen', true);
                    }}
                    style={{ fontSize: '11px' }}
                  >
                    Open Sheet {litSheet.id} ({litSheet.verificationBadge}) →
                  </button>
                )}
              </div>

              {solvedSummary && (
                <div
                  data-testid="theory-solved-summary"
                  style={{
                    display: 'grid',
                    gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))',
                    gap: '8px',
                    background: 'var(--panel-2)',
                    border: '1px solid var(--line)',
                    borderRadius: '6px',
                    padding: '8px 10px',
                    fontFamily: 'var(--mono)',
                    fontSize: '11px',
                  }}
                >
                  <div>
                    Active P₂ DOFs: <strong>{solvedSummary.dofs?.toLocaleString() ?? '—'}</strong> (h = {solvedSummary.hMm?.toFixed(2) ?? '—'} mm)
                  </div>
                  {solvedSummary.maxVmMpa != null && (
                    <div>
                      Peak σ_vM: <strong>{solvedSummary.maxVmMpa.toFixed(2)} MPa</strong>
                    </div>
                  )}
                  {solvedSummary.maxDispMm != null && (
                    <div>
                      Peak ‖u_h‖_∞: <strong>{solvedSummary.maxDispMm.toFixed(4)} mm</strong>
                    </div>
                  )}
                  {solvedSummary.maxTempC != null && (
                    <div>
                      Peak T_max: <strong>{solvedSummary.maxTempC.toFixed(1)} °C</strong>
                    </div>
                  )}
                  {solvedSummary.massG != null && (
                    <div>
                      Mass: <strong>{solvedSummary.massG.toFixed(2)} g</strong>
                    </div>
                  )}
                </div>
              )}

              {litSheet && (
                <>
                  <div style={{ fontSize: '12px', color: 'var(--text-2)', lineHeight: 1.5 }}>
                    <strong style={{ color: 'var(--text)' }}>{litSheet.theoryChapter.chapterTitle}:</strong>{' '}
                    {litSheet.theoryChapter.summary}
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: '8px' }}>
                    {litSheet.theoryChapter.sections.map((sec, idx) => (
                      <div
                        key={idx}
                        style={{
                          background: 'var(--panel-2)',
                          border: '1px solid var(--line)',
                          borderLeft: '3px solid var(--ok)',
                          borderRadius: '6px',
                          padding: '8px 10px',
                          display: 'flex',
                          flexDirection: 'column',
                          gap: '5px',
                        }}
                      >
                        <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--ok)' }}>
                          {sec.heading}
                        </div>
                        <pre
                          style={{
                            margin: 0,
                            padding: '6px 8px',
                            background: 'var(--panel)',
                            border: '1px solid var(--line)',
                            borderRadius: '4px',
                            fontFamily: 'var(--mono)',
                            fontSize: '11px',
                            color: 'var(--text)',
                            whiteSpace: 'pre-wrap',
                            wordBreak: 'break-word',
                          }}
                        >
                          {sec.equations.trim()}
                        </pre>
                        <div style={{ fontSize: '11px', color: 'var(--text-2)', lineHeight: 1.45 }}>
                          {sec.body}
                        </div>
                      </div>
                    ))}
                  </div>

                  {litSheet.analyticChecks && litSheet.analyticChecks.length > 0 && (
                    <div style={{ overflowX: 'auto' }}>
                      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '11.5px' }}>
                        <thead>
                          <tr style={{ borderBottom: '1px solid var(--line-strong)', color: 'var(--text-2)', textAlign: 'left' }}>
                            <th style={{ padding: '5px 8px' }}>Quantity</th>
                            <th style={{ padding: '5px 8px' }}>Closed-Form Formula</th>
                            <th style={{ padding: '5px 8px' }}>Analytic Reference</th>
                            <th style={{ padding: '5px 8px' }}>Fieldsmith P₂ FEA</th>
                            <th style={{ padding: '5px 8px' }}>Rel. Error</th>
                            <th style={{ padding: '5px 8px' }}>Tolerance</th>
                          </tr>
                        </thead>
                        <tbody>
                          {litSheet.analyticChecks.map((ck, idx) => (
                            <tr key={idx} style={{ borderBottom: '1px solid var(--line)' }}>
                              <td style={{ padding: '5px 8px', fontWeight: 600 }}>{ck.quantity}</td>
                              <td style={{ padding: '5px 8px', fontFamily: 'var(--mono)', fontSize: '11px' }}>{ck.formula}</td>
                              <td style={{ padding: '5px 8px', fontFamily: 'var(--mono)' }}>{ck.analyticRef}</td>
                              <td style={{ padding: '5px 8px', fontFamily: 'var(--mono)', fontWeight: 600 }}>{ck.measuredFea}</td>
                              <td style={{ padding: '5px 8px', fontFamily: 'var(--mono)', color: 'var(--ok)' }}>{ck.relError}</td>
                              <td style={{ padding: '5px 8px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>{ck.tolerance}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}

                  {liveAsserts.length > 0 && (
                    <div
                      data-testid="theory-live-asserts"
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: '4px',
                        padding: '8px 10px',
                        background: 'var(--panel-2)',
                        border: '1px solid var(--line-strong)',
                        borderRadius: '6px',
                      }}
                    >
                      <div style={{ fontSize: '11px', fontWeight: 700, color: 'var(--ok)' }}>
                        Executable `# @assert` Verification Contracts ({liveAsserts.filter((a) => a.passed).length} / {liveAsserts.length} PASS)
                      </div>
                      {liveAsserts.map((ev, idx) => {
                        const u = ev.assert.unit ? ` ${ev.assert.unit}` : '';
                        return (
                          <div
                            key={idx}
                            style={{
                              display: 'flex',
                              justifyContent: 'space-between',
                              alignItems: 'center',
                              gap: '8px',
                              flexWrap: 'wrap',
                              fontSize: '11px',
                              fontFamily: 'var(--mono)',
                            }}
                          >
                            <span>
                              {ev.assert.label} <code style={{ color: 'var(--text-3)' }}>({ev.assert.metric})</code>
                            </span>
                            <span style={{ color: ev.passed ? 'var(--ok)' : 'var(--danger)', fontWeight: 700 }}>
                              live {ev.liveValue != null ? fmt(ev.liveValue, 4) : 'N/A'}
                              {u} vs {ev.assert.target}
                              {u} ({ev.errorOrMargin}) · {ev.passed ? 'PASS' : 'FAIL'}
                            </span>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}

viewOverlays.push(TheoryLensView);
