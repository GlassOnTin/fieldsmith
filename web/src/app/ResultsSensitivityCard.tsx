// General 3D Parameter Sensitivity (∂C/∂p, ∂m/∂p, ∂σ_vM/∂p, ∂‖u‖/∂p) &
// Unified .fsm `assert` Contract Verifier Card for the Results lens.
// Works on ANY .fsm document via `computeFsmSensitivities` and `verifyFsmContracts`
// in `fieldsmith-wasm` (no shadow TS evaluators or domain-specific heuristics).

import { useState } from 'preact/hooks';
import { store } from '../doc/store';
import {
  computeFsmSensitivities,
  verifyFsmContracts,
  type SensitivityReport,
  type VerificationReport,
} from '../doc/wasm';
import { ui, useApp } from './ui';

export function ResultsSensitivityCard() {
  useApp();
  const doc = store.doc;
  const hasLets = (doc.lets?.length ?? 0) > 0;
  const hasAsserts = (doc.asserts?.length ?? 0) > 0;

  const [sensReport, setSensReport] = useState<SensitivityReport | null>(null);
  const [contractReport, setContractReport] = useState<VerificationReport | null>(null);
  const [busy, setBusy] = useState<'sens' | 'verify' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  if (!hasLets && !hasAsserts) return null;

  const runSensitivities = () => {
    setBusy('sens');
    setErr(null);
    window.setTimeout(() => {
      try {
        const rep = computeFsmSensitivities(store.doc, undefined, 4000);
        setSensReport(rep);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErr(msg);
        ui.notify(`Sensitivity error: ${msg}`, 'error');
      } finally {
        setBusy(null);
      }
    }, 10);
  };

  const runContracts = () => {
    setBusy('verify');
    setErr(null);
    window.setTimeout(() => {
      try {
        const rep = verifyFsmContracts(store.doc, 8000);
        setContractReport(rep);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErr(msg);
        ui.notify(`Contract verification error: ${msg}`, 'error');
      } finally {
        setBusy(null);
      }
    }, 10);
  };

  return (
    <div
      data-testid="results-sensitivity-contracts-card"
      style={{
        margin: '6px 0',
        padding: '7px 9px',
        background: 'rgba(16, 185, 129, 0.06)',
        border: '1px solid rgba(16, 185, 129, 0.32)',
        borderRadius: '6px',
        display: 'flex',
        flexDirection: 'column',
        gap: '6px',
        fontSize: '10.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
        <span style={{ fontWeight: 700, color: '#6ee7b7', fontSize: '11px' }}>
          ∇ 3D Continuum Sensitivities &amp; .fsm Contracts
        </span>
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
          {hasLets && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-compute-sensitivities-btn"
              disabled={busy !== null}
              onClick={runSensitivities}
              title="Evaluate 3D P2 finite-difference sensitivities ∂C/∂p, ∂m/∂p, ∂σ_vM/∂p, ∂‖u‖/∂p at locked mesh h"
            >
              {busy === 'sens' ? 'Computing ∂/∂p…' : `Compute ∂/∂p (${doc.lets?.length ?? 0} let)`}
            </button>
          )}
          {hasAsserts && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-verify-contracts-btn"
              disabled={busy !== null}
              onClick={runContracts}
              title="Verify .fsm assert / # @assert contracts via unified Rust evaluator"
            >
              {busy === 'verify' ? 'Verifying…' : `Verify Asserts (${doc.asserts?.length ?? 0})`}
            </button>
          )}
        </div>
      </div>

      {err && (
        <div style={{ color: 'var(--danger)', fontFamily: 'var(--mono)', fontSize: '10px' }}>
          {err}
        </div>
      )}

      {contractReport && (
        <div data-testid="results-contracts-output" style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <div style={{ fontFamily: 'var(--mono)', fontSize: '10px', color: contractReport.all_passed ? '#4ade80' : '#f87171' }}>
            {contractReport.all_passed ? '✓' : '✗'} {contractReport.passed}/{contractReport.total} .fsm contracts passed
          </div>
          {contractReport.outcomes.map((r, i) => (
            <div
              key={i}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                gap: '6px',
                fontFamily: 'var(--mono)',
                fontSize: '9.5px',
                color: r.passed ? 'var(--text-2)' : 'var(--danger)',
              }}
            >
              <span>
                {r.passed ? '✓' : '✗'} {r.label || r.metric} ({r.target})
              </span>
              <span>
                {r.value.toPrecision(4)} {r.unit} ({r.error_or_margin})
              </span>
            </div>
          ))}
        </div>
      )}

      {sensReport && sensReport.parameters.length > 0 && (
        <div style={{ overflowX: 'auto', display: 'flex', flexDirection: 'column', gap: '3px' }} data-testid="results-sensitivity-table">
          <div style={{ fontFamily: 'var(--mono)', fontSize: '9px', color: 'var(--text-3)' }}>
            method: {sensReport.method} · {sensReport.solves_run} solves · h={sensReport.h.toFixed(2)} mm ({sensReport.elements} P2 tets)
          </div>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontFamily: 'var(--mono)', fontSize: '9.5px' }}>
            <thead>
              <tr style={{ color: 'var(--text-3)', textAlign: 'right', borderBottom: '1px solid var(--line)' }}>
                <th style={{ textAlign: 'left', padding: '2px 4px' }}>let p</th>
                <th style={{ padding: '2px 4px' }} title="Logarithmic compliance elasticity (p/C)·∂C/∂p">e_C</th>
                <th style={{ padding: '2px 4px' }}>∂m/∂p (g)</th>
                <th style={{ padding: '2px 4px' }}>∂σ_vM/∂p</th>
                <th style={{ padding: '2px 4px' }}>∂‖u‖/∂p</th>
              </tr>
            </thead>
            <tbody>
              {sensReport.parameters.map((p) => {
                const c0 = p.cases[0];
                return (
                  <tr key={p.param} style={{ textAlign: 'right', borderTop: '1px solid rgba(148,163,184,0.12)' }}>
                    <td style={{ textAlign: 'left', padding: '2px 4px', fontWeight: 600 }}>
                      {p.param}={p.nominal}
                    </td>
                    <td style={{ padding: '2px 4px' }}>
                      {c0 ? c0.rel_compliance.toFixed(2) : '—'}
                    </td>
                    <td style={{ padding: '2px 4px' }}>{p.d_mass_g.toFixed(2)}</td>
                    <td style={{ padding: '2px 4px' }}>{c0 ? c0.d_max_vm_elem.toFixed(2) : '—'}</td>
                    <td style={{ padding: '2px 4px' }}>{c0 ? c0.d_max_displacement.toFixed(4) : '—'}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
