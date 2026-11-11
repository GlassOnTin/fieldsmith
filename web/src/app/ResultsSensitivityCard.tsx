// General 3D Parameter Sensitivity (∂C/∂p, ∂m/∂p, ∂σ_vM/∂p, ∂‖u‖/∂p, ∂T_max/∂p) &
// Unified .fsm `assert` Contract Authoring + Verifier Card for the Results lens.
// Works on ANY .fsm document via `computeFsmSensitivities`, `verifyFsmContracts`,
// and `DocCommand` (`upsert_assert` / `delete_assert`) in `fieldsmith-wasm`.

import { useState } from 'preact/hooks';
import { store } from '../doc/store';
import {
  computeFsmSensitivities,
  optimiseFsmDocument,
  verifyFsmContracts,
  type OptObjective,
  type OptimiseBundle,
  type SensitivityReport,
  type VerificationReport,
} from '../doc/wasm';
import { ui, useApp } from './ui';

const OPT_OBJECTIVES: { value: OptObjective; label: string }[] = [
  { value: 'mass_g', label: 'Min Mass (g)' },
  { value: 'compliance', label: 'Min Compliance (N·mm)' },
  { value: 'max_vm_elem', label: 'Min Stress σ_vM (MPa)' },
  { value: 'max_displacement', label: 'Min Deflection ‖u‖ (mm)' },
  { value: 'max_temp', label: 'Min Temp T_max (°C)' },
  { value: 'max_mode_1_hz', label: 'Max Mode f₁ (Hz)' },
];

const CONTRACT_METRICS: { value: string; label: string; defaultUnit: string; defaultTarget: string }[] = [
  { value: 'max_vm_elem', label: 'Peak von Mises σ_vM (max_vm_elem)', defaultUnit: 'MPa', defaultTarget: '<= 180' },
  { value: 'min_safety_factor', label: 'Safety factor (min_safety_factor)', defaultUnit: '', defaultTarget: '>= 1.5' },
  { value: 'max_displacement', label: 'Peak deflection ‖u‖ (max_displacement)', defaultUnit: 'mm', defaultTarget: '<= 1.0' },
  { value: 'compliance', label: 'Strain compliance C (compliance)', defaultUnit: 'mJ', defaultTarget: '<= 500' },
  { value: 'max_temp', label: 'Peak temperature T_max (max_temp)', defaultUnit: '°C', defaultTarget: '<= 120' },
  { value: 'min_temp', label: 'Minimum temperature T_min (min_temp)', defaultUnit: '°C', defaultTarget: '>= 15' },
  { value: 'mass_g', label: 'Part mass (mass_g)', defaultUnit: 'g', defaultTarget: '<= 250' },
  { value: 'max_layer_peel', label: 'Inter-layer peel (max_layer_peel)', defaultUnit: '', defaultTarget: '<= 1.0' },
  { value: 'mode_1', label: 'Fundamental mode f₁ (mode_1)', defaultUnit: 'Hz', defaultTarget: '>= 100' },
  { value: 'mode_2', label: 'Second mode f₂ (mode_2)', defaultUnit: 'Hz', defaultTarget: '>= 150' },
  { value: 'mode_split_12_pct', label: 'Doublet split |f₂−f₁|/f̄ (mode_split_12_pct)', defaultUnit: '%', defaultTarget: '<= 2.0' },
  { value: 'graded.yield_utilization', label: 'Lattice yield util U_y (graded.yield_utilization)', defaultUnit: '', defaultTarget: '<= 0.85' },
  { value: 'graded.buckling_utilization', label: 'Lattice buckling util U_b (graded.buckling_utilization)', defaultUnit: '', defaultTarget: '<= 0.85' },
  { value: 'graded.deflection', label: 'Graded deflection ‖u‖ (graded.deflection)', defaultUnit: 'mm', defaultTarget: '<= 0.50' },
  { value: 'mass_saved_pct', label: 'Lattice mass saved (mass_saved_pct)', defaultUnit: '%', defaultTarget: '>= 25' },
];

export function ResultsSensitivityCard() {
  useApp();
  const doc = store.doc;
  const letCount = doc.lets?.length ?? 0;
  const sysDesignCount = doc.system_block?.designs?.length ?? 0;
  const fillParamCount = doc.fill != null ? 4 : 0;
  const paramCount = letCount + sysDesignCount + fillParamCount;
  const hasParams = paramCount > 0;
  const asserts = doc.asserts ?? [];
  const hasAsserts = asserts.length > 0;

  const [sensReport, setSensReport] = useState<SensitivityReport | null>(null);
  const [contractReport, setContractReport] = useState<VerificationReport | null>(null);
  const [optBundle, setOptBundle] = useState<OptimiseBundle | null>(null);
  const [optObjective, setOptObjective] = useState<OptObjective>('mass_g');
  const [busy, setBusy] = useState<'sens' | 'verify' | 'opt' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const [addOpen, setAddOpen] = useState(false);
  const [label, setLabel] = useState('Peak stress margin');
  const [metric, setMetric] = useState('max_vm_elem');
  const [target, setTarget] = useState('<= 180');
  const [tolerance, setTolerance] = useState('');
  const [unit, setUnit] = useState('MPa');

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

  const runOptimiser = () => {
    setBusy('opt');
    setErr(null);
    window.setTimeout(() => {
      try {
        const res = optimiseFsmDocument(store.doc, {
          objective: optObjective,
          max_iters: 6,
          dofs: 3500,
        });
        setOptBundle(res);
        setContractReport(res.report.final_verification);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErr(msg);
        ui.notify(`Optimisation error: ${msg}`, 'error');
      } finally {
        setBusy(null);
      }
    }, 10);
  };

  const applyOptimisedDesign = () => {
    if (!optBundle) return;
    store.applyExternalDoc(optBundle.doc, 'Apply 3D Adjoint Optimisation');
    setSensReport(null);
    setOptBundle(null);
    ui.notify('Applied optimised .fsm parameters (Ctrl+Z to undo)');
  };

  const onMetricChange = (nextMetric: string) => {
    setMetric(nextMetric);
    const preset = CONTRACT_METRICS.find((m) => m.value === nextMetric);
    if (preset) {
      setUnit(preset.defaultUnit);
      setTarget(preset.defaultTarget);
    }
  };

  const saveContract = () => {
    const trimmedMetric = metric.trim();
    const trimmedTarget = target.trim();
    if (!trimmedMetric || !trimmedTarget) {
      setErr('Contract metric and target are required.');
      return;
    }
    setErr(null);
    store.dispatch('add engineering contract', {
      cmd: 'upsert_assert',
      assertion: {
        label: label.trim() || trimmedMetric,
        metric: trimmedMetric,
        target: trimmedTarget,
        tolerance: tolerance.trim(),
        unit: unit.trim(),
      },
    });
    setContractReport(null);
    setAddOpen(false);
  };

  const removeContract = (index: number) => {
    store.dispatch('delete engineering contract', {
      cmd: 'delete_assert',
      index,
    });
    setContractReport(null);
  };

  const hasThermalSens = Boolean(
    sensReport?.parameters.some((p) => p.cases.some((c) => c.d_max_temp != null)),
  );
  const hasModalSens = Boolean(
    sensReport?.parameters.some((p) => (p.d_modes_hz?.length ?? 0) > 0),
  );
  const hasGradedSens = Boolean(
    sensReport?.parameters.some((p) =>
      p.cases.some((c) => c.d_yield_utilization != null || c.d_buckling_utilization != null),
    ),
  );

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
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', alignItems: 'center' }}>
          {hasParams && (
            <>
              <button
                type="button"
                class="mini-btn"
                data-testid="results-compute-sensitivities-btn"
                disabled={busy !== null}
                onClick={runSensitivities}
                title="Evaluate fixed-topology 3D P2 boundary-velocity adjoint sensitivities ∂C/∂p, ∂m/∂p, ∂σ_vM/∂p, ∂‖u‖/∂p, ∂T_max/∂p"
              >
                {busy === 'sens' ? 'Computing ∂/∂p…' : `Compute ∂/∂p (${paramCount})`}
              </button>
              <select
                class="select"
                data-testid="optimise-objective-select"
                value={optObjective}
                disabled={busy !== null}
                onChange={(e) => setOptObjective((e.target as HTMLSelectElement).value as OptObjective)}
                style={{ fontSize: '9.5px', padding: '1px 4px', height: '20px' }}
                title="Objective minimised by the 3D contract-constrained adjoint optimiser"
              >
                {OPT_OBJECTIVES.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
              <button
                type="button"
                class="mini-btn"
                data-testid="results-run-optimise-btn"
                disabled={busy !== null}
                onClick={runOptimiser}
                style={{ borderColor: 'rgba(16, 185, 129, 0.5)', color: '#6ee7b7', fontWeight: 700 }}
                title="Run trust-region SQP 3D + 0D/1D adjoint optimiser subject to all .fsm assert contracts"
              >
                {busy === 'opt' ? 'Optimising…' : 'Optimise'}
              </button>
            </>
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
              {busy === 'verify' ? 'Verifying…' : `Verify Asserts (${asserts.length})`}
            </button>
          )}
          <button
            type="button"
            class="mini-btn"
            data-testid="results-toggle-add-contract-btn"
            onClick={() => setAddOpen((v) => !v)}
            title="Author a persistent .fsm engineering contract (assert ...)"
          >
            {addOpen ? 'Cancel' : '+ Add Contract'}
          </button>
        </div>
      </div>

      {addOpen && (
        <div
          data-testid="results-add-contract-form"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(110px, 1fr))',
            gap: '4px',
            padding: '6px',
            borderRadius: '4px',
            background: 'var(--bg-2)',
            border: '1px solid var(--line)',
            alignItems: 'end',
          }}
        >
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-3)' }}>
            Label
            <input
              type="text"
              class="input"
              data-testid="contract-label-input"
              value={label}
              onInput={(e) => setLabel((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-3)' }}>
            Metric
            <select
              class="select"
              data-testid="contract-metric-select"
              value={metric}
              onChange={(e) => onMetricChange((e.target as HTMLSelectElement).value)}
              style={{ fontSize: '10px', padding: '2px 4px' }}
            >
              {CONTRACT_METRICS.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-3)' }}>
            Target
            <input
              type="text"
              class="input"
              data-testid="contract-target-input"
              placeholder="<= 180"
              value={target}
              onInput={(e) => setTarget((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', fontFamily: 'var(--mono)' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-3)' }}>
            Unit / Tol
            <div style={{ display: 'flex', gap: '3px' }}>
              <input
                type="text"
                class="input"
                data-testid="contract-unit-input"
                placeholder="unit"
                value={unit}
                onInput={(e) => setUnit((e.target as HTMLInputElement).value)}
                style={{ width: '50%', fontSize: '10px', padding: '2px 4px', fontFamily: 'var(--mono)' }}
              />
              <input
                type="text"
                class="input"
                data-testid="contract-tol-input"
                placeholder="tol"
                value={tolerance}
                onInput={(e) => setTolerance((e.target as HTMLInputElement).value)}
                style={{ width: '50%', fontSize: '10px', padding: '2px 4px', fontFamily: 'var(--mono)' }}
              />
            </div>
          </label>
          <button
            type="button"
            class="mini-btn"
            data-testid="contract-save-btn"
            onClick={saveContract}
            style={{ height: '22px', fontWeight: 700, color: '#6ee7b7' }}
          >
            Save Contract
          </button>
        </div>
      )}

      {hasAsserts && !contractReport && (
        <div data-testid="results-authored-contracts-list" style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          {asserts.map((a, i) => (
            <div
              key={`${a.metric}-${i}`}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: '6px',
                fontFamily: 'var(--mono)',
                fontSize: '9.5px',
                color: 'var(--text-2)',
              }}
            >
              <span>
                • {a.label || a.metric}: {a.metric} {a.target} {a.unit}
              </span>
              <button
                type="button"
                class="mini-btn"
                data-testid={`delete-contract-${i}`}
                onClick={() => removeContract(i)}
                title="Remove contract from .fsm document"
                style={{ padding: '0 4px', lineHeight: 1.2 }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      {err && (
        <div
          data-testid="results-card-error"
          style={{ color: 'var(--danger)', fontFamily: 'var(--mono)', fontSize: '10px' }}
        >
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
                alignItems: 'center',
                gap: '6px',
                fontFamily: 'var(--mono)',
                fontSize: '9.5px',
                color: r.passed ? 'var(--text-2)' : 'var(--danger)',
              }}
            >
              <span>
                {r.passed ? '✓' : '✗'} {r.label || r.metric} ({r.target})
              </span>
              <span style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
                <span>
                  {r.value.toPrecision(4)} {r.unit} ({r.error_or_margin})
                </span>
                {i < asserts.length && (
                  <button
                    type="button"
                    class="mini-btn"
                    data-testid={`delete-contract-${i}`}
                    onClick={() => removeContract(i)}
                    title="Remove contract from .fsm document"
                    style={{ padding: '0 4px', lineHeight: 1.2 }}
                  >
                    ×
                  </button>
                )}
              </span>
            </div>
          ))}
        </div>
      )}

      {optBundle && (
        <div
          data-testid="results-optimisation-output"
          style={{
            padding: '6px 8px',
            borderRadius: '5px',
            background: 'rgba(16, 185, 129, 0.10)',
            border: '1px solid rgba(16, 185, 129, 0.4)',
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
            fontFamily: 'var(--mono)',
            fontSize: '9.5px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <span style={{ fontWeight: 700, color: optBundle.report.final_verification.all_passed ? '#4ade80' : '#fbbf24' }}>
              {optBundle.report.final_verification.all_passed ? '✓' : '⚠'} {optBundle.report.objective}:{' '}
              {optBundle.report.initial_objective.toPrecision(4)} → {optBundle.report.final_objective.toPrecision(4)}{' '}
              {optBundle.report.objective_unit} ({optBundle.report.objective_change_pct >= 0 ? '+' : ''}
              {optBundle.report.objective_change_pct.toFixed(1)}%) · {optBundle.report.iterations} iters ({optBundle.report.solves_run} solves)
            </span>
            <button
              type="button"
              class="mini-btn"
              data-testid="results-apply-optimised-btn"
              onClick={applyOptimisedDesign}
              style={{ fontWeight: 700, color: '#6ee7b7', borderColor: 'rgba(16, 185, 129, 0.5)' }}
            >
              Apply to 3D Model
            </button>
          </div>
          <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap', color: 'var(--text-2)' }}>
            {optBundle.report.parameters.map((p) => (
              <span key={p.param}>
                {p.param}: {p.initial.toPrecision(4)} → <strong>{p.final_value.toPrecision(4)}</strong>
              </span>
            ))}
          </div>
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
                <th style={{ textAlign: 'left', padding: '2px 4px' }}>param p</th>
                <th style={{ padding: '2px 4px' }} title="Logarithmic compliance elasticity (p/C)·∂C/∂p">e_C</th>
                <th style={{ padding: '2px 4px' }}>∂m/∂p (g)</th>
                <th style={{ padding: '2px 4px' }}>∂σ_vM/∂p</th>
                <th style={{ padding: '2px 4px' }}>∂‖u‖/∂p</th>
                {hasGradedSens && <th style={{ padding: '2px 4px' }}>∂U_y/∂p</th>}
                {hasGradedSens && <th style={{ padding: '2px 4px' }}>∂U_b/∂p</th>}
                {hasThermalSens && <th style={{ padding: '2px 4px' }}>∂T_max/∂p (°C)</th>}
                {hasModalSens && <th style={{ padding: '2px 4px' }}>∂f₁/∂p (Hz)</th>}
              </tr>
            </thead>
            <tbody>
              {sensReport.parameters.map((p) => {
                const c0 = p.cases[0];
                const ct = p.cases.find((c) => c.d_max_temp != null);
                const df1 = p.d_modes_hz?.[0];
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
                    {hasGradedSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {c0?.d_yield_utilization != null ? c0.d_yield_utilization.toFixed(3) : '—'}
                      </td>
                    )}
                    {hasGradedSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {c0?.d_buckling_utilization != null ? c0.d_buckling_utilization.toFixed(3) : '—'}
                      </td>
                    )}
                    {hasThermalSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {ct?.d_max_temp != null ? ct.d_max_temp.toFixed(2) : '—'}
                      </td>
                    )}
                    {hasModalSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {df1 != null ? df1.toFixed(2) : '—'}
                      </td>
                    )}
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
