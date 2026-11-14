// General 3D Parameter Sensitivity (∂C/∂p, ∂m/∂p, ∂σ_vM/∂p, ∂‖u‖/∂p, ∂T_max/∂p) &
// Unified .fsm `assert` Contract Authoring + Verifier Card for the Results lens.
// Works on ANY .fsm document via `computeFsmSensitivities`, `verifyFsmContracts`,
// and `DocCommand` (`upsert_assert` / `delete_assert`) in `fieldsmith-wasm`.

import { useEffect, useState } from 'preact/hooks';
import { addressableParams } from '../doc/param';
import { store } from '../doc/store';
import {
  calibrateFsmDocument,
  computeFsmSensitivities,
  computeFsmUq,
  optimiseFsmDocument,
  verifyFsmContracts,
  type CalibrateBundle,
  type OptObjective,
  type OptimiseBundle,
  type SensitivityReport,
  type UqReport,
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
  { value: 'max_layer_peel', label: 'Min Layer Peel U_peel' },
  { value: 'overhang_fraction', label: 'Min Overhang Area' },
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
  { value: 'graded.max_layer_peel', label: 'Graded layer peel (graded.max_layer_peel)', defaultUnit: '', defaultTarget: '<= 0.85' },
  { value: 'overhang_pct', label: 'Overhang area % (overhang_pct)', defaultUnit: '%', defaultTarget: '<= 15' },
  { value: 'max_overhang_deg', label: 'Peak overhang angle (max_overhang_deg)', defaultUnit: '°', defaultTarget: '<= 45' },
  { value: 'mode_1', label: 'Fundamental mode f₁ (mode_1)', defaultUnit: 'Hz', defaultTarget: '>= 100' },
  { value: 'mode_2', label: 'Second mode f₂ (mode_2)', defaultUnit: 'Hz', defaultTarget: '>= 150' },
  { value: 'mode_split_12_pct', label: 'Doublet split |f₂−f₁|/f̄ (mode_split_12_pct)', defaultUnit: '%', defaultTarget: '<= 2.0' },
  { value: 'graded.yield_utilization', label: 'Lattice yield util U_y (graded.yield_utilization)', defaultUnit: '', defaultTarget: '<= 0.85' },
  { value: 'graded.buckling_utilization', label: 'Lattice buckling util U_b (graded.buckling_utilization)', defaultUnit: '', defaultTarget: '<= 0.85' },
  { value: 'graded.deflection', label: 'Graded deflection ‖u‖ (graded.deflection)', defaultUnit: 'mm', defaultTarget: '<= 0.50' },
  { value: 'mass_saved_pct', label: 'Lattice mass saved (mass_saved_pct)', defaultUnit: '%', defaultTarget: '>= 25' },
  { value: 'p90.max_vm_elem', label: 'Robust p90 von Mises σ_vM (p90.max_vm_elem)', defaultUnit: 'MPa', defaultTarget: '<= 180' },
  { value: 'p10.min_safety_factor', label: 'Robust p10 safety factor (p10.min_safety_factor)', defaultUnit: '', defaultTarget: '>= 1.5' },
  { value: 'p90.max_layer_peel', label: 'Robust p90 layer peel (p90.max_layer_peel)', defaultUnit: '', defaultTarget: '<= 1.0' },
  { value: 'p90.max_temp', label: 'Robust p90 peak temp (p90.max_temp)', defaultUnit: '°C', defaultTarget: '<= 120' },
  { value: 'p10.mode_1', label: 'Robust p10 mode f₁ (p10.mode_1)', defaultUnit: 'Hz', defaultTarget: '>= 100' },
  { value: 'p90.graded.yield_utilization', label: 'Robust p90 lattice U_y (p90.graded.yield_utilization)', defaultUnit: '', defaultTarget: '<= 0.85' },
];

export function ResultsSensitivityCard() {
  useApp();
  const doc = store.doc;
  const letCount = doc.lets?.length ?? 0;
  const sysDesignCount = doc.system_block?.designs?.length ?? 0;
  const fillParamCount = doc.fill != null ? 4 : 0;
  const buildParamCount = doc.solve?.build != null ? 2 : 0;
  const paramCount = letCount + sysDesignCount + fillParamCount + buildParamCount;
  const hasParams = paramCount > 0;
  const uncertainties = doc.uncertainties ?? [];
  const sysEpistemicCount =
    doc.system_block?.epistemics?.filter((e) => e.sigma > 0).length ?? 0;
  const uqCount = uncertainties.length + sysEpistemicCount;
  const hasUq = uqCount > 0;
  const observations = doc.observations ?? [];
  const hasObs = observations.length > 0;
  const asserts = doc.asserts ?? [];
  const hasAsserts = asserts.length > 0;

  const [sensReport, setSensReport] = useState<SensitivityReport | null>(null);
  const [uqReport, setUqReport] = useState<UqReport | null>(null);
  const [contractReport, setContractReport] = useState<VerificationReport | null>(null);
  const [optBundle, setOptBundle] = useState<OptimiseBundle | null>(null);
  const [calBundle, setCalBundle] = useState<CalibrateBundle | null>(null);
  const [optObjective, setOptObjective] = useState<OptObjective>('mass_g');
  const [busy, setBusy] = useState<'sens' | 'uq' | 'verify' | 'opt' | 'cal' | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const [addOpen, setAddOpen] = useState(false);
  const [label, setLabel] = useState('Peak stress margin');
  const [metric, setMetric] = useState('max_vm_elem');
  const [contractCase, setContractCase] = useState('');
  const [target, setTarget] = useState('<= 180');
  const [tolerance, setTolerance] = useState('');
  const [unit, setUnit] = useState('MPa');

  const [addUqOpen, setAddUqOpen] = useState(false);
  const [uqLabel, setUqLabel] = useState('Manufacturing scatter');
  const [uqParam, setUqParam] = useState(doc.lets?.[0]?.name ?? `${doc.material}.e`);
  const [uqMode, setUqMode] = useState<'std' | 'cv'>('cv');
  const [uqVal, setUqVal] = useState('0.08');
  const [uqUnit, setUqUnit] = useState('');

  const [addObsOpen, setAddObsOpen] = useState(false);
  const [obsLabel, setObsLabel] = useState('Bench sensor');
  const [obsMetric, setObsMetric] = useState('max_displacement');
  const [obsCase, setObsCase] = useState('');
  const [obsMeasured, setObsMeasured] = useState('0.25');
  const [obsStd, setObsStd] = useState('0.01');
  const [obsUnit, setObsUnit] = useState('mm');

  useEffect(() => {
    setSensReport(null);
    setUqReport(null);
    setContractReport(null);
    setOptBundle(null);
    setCalBundle(null);
    setErr(null);
  }, [store.docVersion]);

  const docCases: string[] = [];
  const pushCase = (c?: string | null) => {
    const t = c?.trim();
    if (t && !docCases.includes(t)) docCases.push(t);
  };
  for (const l of doc.loads) pushCase(l.case);
  for (const t of doc.temperatures) pushCase(t.case);
  for (const c of doc.convections) pushCase(c.case);
  for (const q of doc.heat_fluxes) pushCase(q.case);

  const paramSuggestions: string[] = [];
  const pushSuggestion = (s: string) => {
    if (s && !paramSuggestions.includes(s)) paramSuggestions.push(s);
  };
  for (const l of doc.lets ?? []) pushSuggestion(l.name);
  if (doc.material) {
    for (const attr of ['e', 'yield', 'k', 'alpha', 'density']) {
      pushSuggestion(`${doc.material}.${attr}`);
    }
  }
  for (const cm of doc.custom_materials ?? []) {
    for (const attr of ['e', 'yield', 'k', 'alpha', 'density']) {
      pushSuggestion(`${cm.id}.${attr}`);
    }
  }
  for (const c of doc.convections) pushSuggestion(`${c.id}.h`);
  for (const q of doc.heat_fluxes) pushSuggestion(`${q.id}.q`);
  for (const ct of doc.contacts ?? []) pushSuggestion(`${ct.id}.k`);
  for (const ap of addressableParams(doc)) pushSuggestion(ap.addr);

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

  const runUq = () => {
    setBusy('uq');
    setErr(null);
    window.setTimeout(() => {
      try {
        const rep = computeFsmUq(store.doc, { dofs: 4000 });
        setUqReport(rep);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErr(msg);
        ui.notify(`UQ error: ${msg}`, 'error');
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
    setUqReport(null);
    setOptBundle(null);
    setCalBundle(null);
    ui.notify('Applied optimised .fsm parameters (Ctrl+Z to undo)');
  };

  const runCalibrate = () => {
    setBusy('cal');
    setErr(null);
    window.setTimeout(() => {
      try {
        const spec = store.doc.solve?.size != null ? undefined : { dofs: 4000 };
        const res = calibrateFsmDocument(store.doc, spec);
        setCalBundle(res);
        setContractReport(res.report.final_verification);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setErr(msg);
        ui.notify(`Calibration error: ${msg}`, 'error');
      } finally {
        setBusy(null);
      }
    }, 10);
  };

  const applyCalibratedPosterior = () => {
    if (!calBundle) return;
    store.applyExternalDoc(calBundle.doc, 'Apply Bayesian MAP Calibration');
    setSensReport(null);
    setUqReport(null);
    setOptBundle(null);
    setCalBundle(null);
    ui.notify('Applied Bayesian MAP posterior means & marginal σ to .fsm (Ctrl+Z to undo)');
  };

  const onObsMetricInput = (nextMetric: string) => {
    setObsMetric(nextMetric);
    const parts = nextMetric.split(':');
    const baseMetric = parts[0]?.trim() ?? '';
    const suffix = parts.slice(1).join(':').trim();
    if (suffix && docCases.includes(suffix)) {
      setObsCase(suffix);
    } else if (!suffix) {
      setObsCase('');
    }
    const preset = CONTRACT_METRICS.find((m) => m.value === baseMetric);
    if (preset && preset.defaultUnit) {
      setObsUnit(preset.defaultUnit);
    }
  };

  const onObsCaseChange = (nextCase: string) => {
    setObsCase(nextCase);
    const baseMetric = obsMetric.split(':')[0]?.trim() ?? '';
    if (!baseMetric) return;
    setObsMetric(nextCase ? `${baseMetric}:${nextCase}` : baseMetric);
  };

  const saveObserve = () => {
    const baseOrFull = obsMetric.trim();
    const trimmedMetric =
      obsCase && baseOrFull && !baseOrFull.includes(':') ? `${baseOrFull}:${obsCase}` : baseOrFull;
    const measuredVal = Number(obsMeasured.trim());
    const stdVal = Number(obsStd.trim());
    if (!trimmedMetric || !Number.isFinite(measuredVal) || !Number.isFinite(stdVal) || stdVal <= 0) {
      setErr('Observation metric, finite measured value, and positive std are required.');
      return;
    }
    setErr(null);
    store.dispatch('add bench observation', {
      cmd: 'upsert_observe',
      observation: {
        label: obsLabel.trim() || trimmedMetric,
        metric: trimmedMetric,
        measured: measuredVal,
        std: stdVal,
        unit: obsUnit.trim(),
      },
    });
    setCalBundle(null);
    setAddObsOpen(false);
  };

  const removeObserve = (index: number) => {
    store.dispatch('delete bench observation', {
      cmd: 'delete_observe',
      index,
    });
    setCalBundle(null);
  };

  const saveUncertainty = () => {
    const trimmedParam = uqParam.trim();
    const numVal = Number(uqVal.trim());
    if (!trimmedParam || !Number.isFinite(numVal) || numVal <= 0) {
      setErr('Uncertainty parameter and a positive std / cv value are required.');
      return;
    }
    setErr(null);
    store.dispatch('add epistemic uncertainty', {
      cmd: 'upsert_uncertainty',
      uncertainty: {
        label: uqLabel.trim() || trimmedParam,
        param: trimmedParam,
        std: uqMode === 'std' ? numVal : null,
        cv: uqMode === 'cv' ? numVal : null,
        unit: uqUnit.trim(),
      },
    });
    setUqReport(null);
    setContractReport(null);
    setAddUqOpen(false);
  };

  const removeUncertainty = (index: number) => {
    store.dispatch('delete epistemic uncertainty', {
      cmd: 'delete_uncertainty',
      index,
    });
    setUqReport(null);
    setContractReport(null);
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
    const baseMetric = metric.trim();
    const trimmedMetric =
      contractCase && baseMetric && !baseMetric.includes(':')
        ? `${baseMetric}:${contractCase}`
        : baseMetric;
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
  const hasPeelSens = Boolean(
    sensReport?.parameters.some((p) => p.cases.some((c) => c.d_max_layer_peel != null)),
  );
  const hasOverhangSens = Boolean(
    sensReport?.parameters.some((p) => p.d_overhang_fraction != null),
  );

  return (
    <div
      data-testid="results-sensitivity-contracts-card"
      style={{
        margin: '6px 0',
        padding: '7px 9px',
        background: 'color-mix(in srgb, #10b981 7%, var(--panel))',
        border: '1px solid color-mix(in srgb, #10b981 36%, var(--line))',
        borderRadius: '6px',
        display: 'flex',
        flexDirection: 'column',
        gap: '6px',
        fontSize: '10.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
        <span style={{ fontWeight: 700, color: 'color-mix(in srgb, #059669 78%, var(--text))', fontSize: '11px' }}>
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
                style={{ fontSize: '9.5px', padding: '1px 4px', height: '22px' }}
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
                style={{
                  borderColor: 'color-mix(in srgb, #10b981 55%, var(--line))',
                  color: 'color-mix(in srgb, #059669 78%, var(--text))',
                  fontWeight: 700,
                }}
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
          {hasUq && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-compute-uq-btn"
              disabled={busy !== null}
              onClick={runUq}
              title="Propagate epistemic uncertainties in O(1) FEA solves to p10 / p50 / p90 quantiles & variance shares"
            >
              {busy === 'uq' ? 'Computing UQ…' : `Compute UQ (${uqCount})`}
            </button>
          )}
          {hasObs && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-run-calibrate-btn"
              disabled={busy !== null}
              onClick={runCalibrate}
              style={{
                borderColor: 'color-mix(in srgb, #0ea5e9 55%, var(--line))',
                color: 'var(--accent)',
                fontWeight: 700,
              }}
              title="Fuse empirical bench readings (observe ...) with parameter priors via 3D adjoint Gauss–Newton Bayesian MAP calibration"
            >
              {busy === 'cal' ? 'Calibrating…' : `Calibrate MAP (${observations.length})`}
            </button>
          )}
          <button
            type="button"
            class="mini-btn"
            data-testid="results-toggle-add-observe-btn"
            onClick={() => setAddObsOpen((v) => !v)}
            title="Author an empirical bench / test-stand sensor reading (observe ...)"
          >
            {addObsOpen ? 'Cancel Obs' : '+ Add Bench Reading'}
          </button>
          <button
            type="button"
            class="mini-btn"
            data-testid="results-toggle-add-uncertainty-btn"
            onClick={() => setAddUqOpen((v) => !v)}
            title="Author an epistemic / manufacturing uncertainty (uncertainty ...)"
          >
            {addUqOpen ? 'Cancel UQ' : '+ Add Uncertainty'}
          </button>
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

      {addObsOpen && (
        <div
          data-testid="results-add-observe-form"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(100px, 1fr))',
            gap: '4px',
            padding: '6px',
            borderRadius: '4px',
            background: 'var(--panel-2)',
            border: '1px solid var(--line)',
            alignItems: 'end',
          }}
        >
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Sensor Label
            <input
              type="text"
              class="text-input"
              data-testid="observe-label-input"
              value={obsLabel}
              onInput={(e) => setObsLabel((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Metric Selector
            <input
              type="text"
              class="text-input"
              list="observe-metric-datalist"
              data-testid="observe-metric-input"
              placeholder="max_temp or max_displacement:Load 1"
              value={obsMetric}
              onInput={(e) => onObsMetricInput((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px', fontFamily: 'var(--mono)' }}
            />
            <datalist id="observe-metric-datalist">
              {CONTRACT_METRICS.filter((m) => !m.value.startsWith('p10.') && !m.value.startsWith('p90.')).map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </datalist>
          </label>
          {docCases.length > 0 && (
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
              Case
              <select
                class="select"
                data-testid="observe-case-select"
                value={obsCase}
                onChange={(e) => onObsCaseChange((e.target as HTMLSelectElement).value)}
                style={{ fontSize: '10px', padding: '2px 4px', height: '24px' }}
              >
                <option value="">Envelope / All</option>
                {docCases.map((cn) => (
                  <option key={cn} value={cn}>
                    {cn}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Measured (y)
            <input
              type="text"
              class="text-input"
              data-testid="observe-measured-input"
              value={obsMeasured}
              onInput={(e) => setObsMeasured((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px', fontFamily: 'var(--mono)' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Noise 1-σ / Unit
            <div style={{ display: 'flex', gap: '3px' }}>
              <input
                type="text"
                class="text-input"
                data-testid="observe-std-input"
                placeholder="σ"
                value={obsStd}
                onInput={(e) => setObsStd((e.target as HTMLInputElement).value)}
                style={{ width: '52%', fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
              />
              <input
                type="text"
                class="text-input"
                data-testid="observe-unit-input"
                placeholder="unit"
                value={obsUnit}
                onInput={(e) => setObsUnit((e.target as HTMLInputElement).value)}
                style={{ width: '48%', fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
              />
            </div>
          </label>
          <button
            type="button"
            class="mini-btn"
            data-testid="observe-save-btn"
            onClick={saveObserve}
            style={{ height: '24px', fontWeight: 700, color: 'var(--accent)' }}
          >
            Save Obs
          </button>
        </div>
      )}

      {observations.length > 0 && (
        <div data-testid="results-authored-observations-list" style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          {observations.map((o, i) => (
            <div
              key={`${o.metric}-${i}`}
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: '6px',
                fontFamily: 'var(--mono)',
                fontSize: '9.5px',
                color: 'var(--accent)',
              }}
            >
              <span>
                ⊙ {o.label || o.metric}: {o.metric} = {o.measured} ± {o.std} {o.unit ?? ''}
              </span>
              <button
                type="button"
                class="mini-btn"
                data-testid={`delete-observe-${i}`}
                onClick={() => removeObserve(i)}
                title="Remove bench observation from .fsm document"
                style={{ padding: '0 4px', lineHeight: 1.2 }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      {addUqOpen && (
        <div
          data-testid="results-add-uncertainty-form"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(105px, 1fr))',
            gap: '4px',
            padding: '6px',
            borderRadius: '4px',
            background: 'var(--panel-2)',
            border: '1px solid var(--line)',
            alignItems: 'end',
          }}
        >
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Label
            <input
              type="text"
              class="text-input"
              data-testid="uncertainty-label-input"
              value={uqLabel}
              onInput={(e) => setUqLabel((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Param Addr
            <input
              type="text"
              class="text-input"
              list="uncertainty-param-datalist"
              data-testid="uncertainty-param-input"
              placeholder="d or ss316l.e"
              value={uqParam}
              onInput={(e) => setUqParam((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px', fontFamily: 'var(--mono)' }}
            />
            <datalist id="uncertainty-param-datalist">
              {paramSuggestions.map((p) => (
                <option key={p} value={p} />
              ))}
            </datalist>
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Spread (1-σ)
            <div style={{ display: 'flex', gap: '3px' }}>
              <select
                class="select"
                data-testid="uncertainty-mode-select"
                value={uqMode}
                onChange={(e) => setUqMode((e.target as HTMLSelectElement).value as 'std' | 'cv')}
                style={{ width: '46%', fontSize: '10px', padding: '2px 3px', height: '24px' }}
              >
                <option value="cv">cv</option>
                <option value="std">std</option>
              </select>
              <input
                type="text"
                class="text-input"
                data-testid="uncertainty-val-input"
                value={uqVal}
                onInput={(e) => setUqVal((e.target as HTMLInputElement).value)}
                style={{ width: '54%', fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
              />
            </div>
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Unit
            <input
              type="text"
              class="text-input"
              data-testid="uncertainty-unit-input"
              placeholder="mm / MPa"
              value={uqUnit}
              onInput={(e) => setUqUnit((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
            />
          </label>
          <button
            type="button"
            class="mini-btn"
            data-testid="uncertainty-save-btn"
            onClick={saveUncertainty}
            style={{ height: '24px', fontWeight: 700, color: 'color-mix(in srgb, #059669 78%, var(--text))' }}
          >
            Save UQ
          </button>
        </div>
      )}

      {uncertainties.length > 0 && (
        <div data-testid="results-authored-uncertainties-list" style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          {uncertainties.map((u, i) => (
            <div
              key={`${u.param}-${i}`}
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
                ± {u.label || u.param}: {u.param}{' '}
                {u.std != null ? `std=${u.std}` : u.cv != null ? `cv=${u.cv}` : ''}{' '}
                {u.unit ?? ''}
              </span>
              <button
                type="button"
                class="mini-btn"
                data-testid={`delete-uncertainty-${i}`}
                onClick={() => removeUncertainty(i)}
                title="Remove epistemic uncertainty from .fsm document"
                style={{ padding: '0 4px', lineHeight: 1.2 }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      {addOpen && (
        <div
          data-testid="results-add-contract-form"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(110px, 1fr))',
            gap: '4px',
            padding: '6px',
            borderRadius: '4px',
            background: 'var(--panel-2)',
            border: '1px solid var(--line)',
            alignItems: 'end',
          }}
        >
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Label
            <input
              type="text"
              class="text-input"
              data-testid="contract-label-input"
              value={label}
              onInput={(e) => setLabel((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Metric
            <select
              class="select"
              data-testid="contract-metric-select"
              value={metric}
              onChange={(e) => onMetricChange((e.target as HTMLSelectElement).value)}
              style={{ fontSize: '10px', padding: '2px 4px', height: '24px' }}
            >
              {CONTRACT_METRICS.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
          </label>
          {docCases.length > 0 && (
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
              Case
              <select
                class="select"
                data-testid="contract-case-select"
                value={contractCase}
                onChange={(e) => setContractCase((e.target as HTMLSelectElement).value)}
                style={{ fontSize: '10px', padding: '2px 4px', height: '24px' }}
              >
                <option value="">Envelope / All</option>
                {docCases.map((cn) => (
                  <option key={cn} value={cn}>
                    {cn}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Target
            <input
              type="text"
              class="text-input"
              data-testid="contract-target-input"
              placeholder="<= 180"
              value={target}
              onInput={(e) => setTarget((e.target as HTMLInputElement).value)}
              style={{ fontSize: '10px', padding: '2px 5px', height: '24px', fontFamily: 'var(--mono)' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '9.5px', color: 'var(--text-2)' }}>
            Unit / Tol
            <div style={{ display: 'flex', gap: '3px' }}>
              <input
                type="text"
                class="text-input"
                data-testid="contract-unit-input"
                placeholder="unit"
                value={unit}
                onInput={(e) => setUnit((e.target as HTMLInputElement).value)}
                style={{ width: '50%', fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
              />
              <input
                type="text"
                class="text-input"
                data-testid="contract-tol-input"
                placeholder="tol"
                value={tolerance}
                onInput={(e) => setTolerance((e.target as HTMLInputElement).value)}
                style={{ width: '50%', fontSize: '10px', padding: '2px 4px', height: '24px', fontFamily: 'var(--mono)' }}
              />
            </div>
          </label>
          <button
            type="button"
            class="mini-btn"
            data-testid="contract-save-btn"
            onClick={saveContract}
            style={{ height: '24px', fontWeight: 700, color: 'color-mix(in srgb, #059669 78%, var(--text))' }}
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
          <div style={{ fontFamily: 'var(--mono)', fontSize: '10px', fontWeight: 600, color: contractReport.all_passed ? 'var(--ok)' : 'var(--danger)' }}>
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
            background: 'color-mix(in srgb, #10b981 9%, var(--panel))',
            border: '1px solid color-mix(in srgb, #10b981 42%, var(--line))',
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
            fontFamily: 'var(--mono)',
            fontSize: '9.5px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <span style={{ fontWeight: 700, color: optBundle.report.final_verification.all_passed ? 'var(--ok)' : 'var(--warn)' }}>
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
              style={{
                fontWeight: 700,
                color: 'color-mix(in srgb, #059669 78%, var(--text))',
                borderColor: 'color-mix(in srgb, #10b981 50%, var(--line))',
              }}
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

      {calBundle && (
        <div
          data-testid="results-calibration-output"
          style={{
            padding: '6px 8px',
            borderRadius: '5px',
            background: 'color-mix(in srgb, #0ea5e9 9%, var(--panel))',
            border: '1px solid color-mix(in srgb, #0ea5e9 45%, var(--line))',
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
            fontFamily: 'var(--mono)',
            fontSize: '9.5px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <span style={{ fontWeight: 700, color: calBundle.report.final_verification.all_passed ? 'var(--accent)' : 'var(--warn)' }}>
              {calBundle.report.final_verification.all_passed ? '✓' : '⚠'} Bayesian MAP · J_MAP:{' '}
              {calBundle.report.initial_cost.toPrecision(4)} → {calBundle.report.final_cost.toPrecision(4)} · χ²/m={' '}
              {calBundle.report.reduced_chi2.toFixed(3)} · {calBundle.report.iterations} iters ({calBundle.report.solves_run} solves)
            </span>
            <button
              type="button"
              class="mini-btn"
              data-testid="results-apply-posterior-btn"
              onClick={applyCalibratedPosterior}
              style={{
                fontWeight: 700,
                color: 'var(--accent)',
                borderColor: 'color-mix(in srgb, #0ea5e9 55%, var(--line))',
              }}
            >
              Apply Posterior to .fsm
            </button>
          </div>
          <div style={{ fontSize: '9px', color: 'var(--text-2)' }}>
            Laplace posterior Σ_post around θ_MAP (h={calBundle.report.h.toFixed(2)} mm); marginal posterior σ written to .fsm uncertainty
          </div>
          <table
            data-testid="results-calibration-params-table"
            style={{ width: '100%', borderCollapse: 'collapse', fontSize: '9.5px' }}
          >
            <thead>
              <tr style={{ color: 'var(--text-2)', textAlign: 'right', borderBottom: '1px solid var(--line)' }}>
                <th style={{ textAlign: 'left', padding: '2px 4px' }}>param</th>
                <th style={{ padding: '2px 4px' }}>prior μ ± σ</th>
                <th style={{ padding: '2px 4px' }}>posterior μ ± σ</th>
                <th style={{ padding: '2px 4px' }}>σ reduction</th>
                <th style={{ padding: '2px 4px' }}>p10 .. p90</th>
              </tr>
            </thead>
            <tbody>
              {calBundle.report.parameters.map((p) => (
                <tr key={p.param} style={{ textAlign: 'right', borderTop: '1px solid var(--line)' }}>
                  <td style={{ textAlign: 'left', padding: '2px 4px', fontWeight: 600 }}>
                    {p.param} {p.unit ? `(${p.unit})` : ''}
                  </td>
                  <td style={{ padding: '2px 4px' }}>
                    {p.prior_mean.toPrecision(4)} ± {p.prior_sigma.toPrecision(3)}
                  </td>
                  <td style={{ padding: '2px 4px', color: 'var(--accent)', fontWeight: 600 }}>
                    {p.posterior_mean.toPrecision(4)} ± {p.posterior_sigma.toPrecision(3)}
                  </td>
                  <td style={{ padding: '2px 4px', color: 'var(--ok)', fontWeight: 600 }}>
                    −{p.sigma_reduction_pct.toFixed(1)}%
                  </td>
                  <td style={{ padding: '2px 4px', color: 'var(--text-2)' }}>
                    {p.posterior_p10.toPrecision(4)} .. {p.posterior_p90.toPrecision(4)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div data-testid="results-calibration-obs-list" style={{ display: 'flex', flexDirection: 'column', gap: '2px', color: 'var(--text-2)' }}>
            {calBundle.report.observations.map((o, idx) => (
              <div key={`${o.metric}-${idx}`} style={{ display: 'flex', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}>
                <span>
                  ⊙ {o.label || o.metric} ({o.metric}): measured {o.measured.toPrecision(4)} ± {o.obs_sigma.toPrecision(3)} {o.unit ?? ''}
                </span>
                <span>
                  pred {o.prior_prediction.toPrecision(4)} → <strong>{o.posterior_prediction.toPrecision(4)}</strong> (±{o.posterior_pred_sigma.toPrecision(3)}, z={o.normalized_residual_z >= 0 ? '+' : ''}{o.normalized_residual_z.toFixed(2)})
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {uqReport && uqReport.metrics.length > 0 && (
        <div style={{ overflowX: 'auto', display: 'flex', flexDirection: 'column', gap: '3px' }} data-testid="results-uq-table">
          <div style={{ fontFamily: 'var(--mono)', fontSize: '9px', color: 'var(--text-2)' }}>
            uq: {uqReport.method} · {uqReport.inputs.length} inputs · {uqReport.solves_run} solves · h={uqReport.h.toFixed(2)} mm ({uqReport.elements} P2 tets)
          </div>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontFamily: 'var(--mono)', fontSize: '9.5px' }}>
            <thead>
              <tr style={{ color: 'var(--text-2)', textAlign: 'right', borderBottom: '1px solid var(--line)' }}>
                <th style={{ textAlign: 'left', padding: '2px 4px' }}>metric</th>
                <th style={{ padding: '2px 4px' }}>p10</th>
                <th style={{ padding: '2px 4px' }}>p50</th>
                <th style={{ padding: '2px 4px' }}>p90</th>
                <th style={{ padding: '2px 4px' }}>±σ</th>
                <th style={{ textAlign: 'left', padding: '2px 4px' }}>top variance driver</th>
              </tr>
            </thead>
            <tbody>
              {uqReport.metrics.map((m) => {
                const top = m.contributors[0];
                return (
                  <tr key={m.metric} style={{ textAlign: 'right', borderTop: '1px solid var(--line)' }}>
                    <td style={{ textAlign: 'left', padding: '2px 4px', fontWeight: 600 }}>
                      {m.metric} {m.unit ? `(${m.unit})` : ''}
                    </td>
                    <td style={{ padding: '2px 4px' }}>{m.p10.toPrecision(4)}</td>
                    <td style={{ padding: '2px 4px' }}>{m.p50.toPrecision(4)}</td>
                    <td style={{ padding: '2px 4px', color: 'var(--warn)', fontWeight: 600 }}>{m.p90.toPrecision(4)}</td>
                    <td style={{ padding: '2px 4px' }}>±{m.std_dev.toPrecision(3)}</td>
                    <td style={{ textAlign: 'left', padding: '2px 4px', color: 'var(--text-2)' }}>
                      {top && top.variance_share > 0
                        ? `${top.param} (${(top.variance_share * 100).toFixed(0)}%)`
                        : '—'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
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
                {hasPeelSens && <th style={{ padding: '2px 4px' }}>∂U_peel/∂p</th>}
                {hasOverhangSens && <th style={{ padding: '2px 4px' }}>∂Overhang/∂p</th>}
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
                    {hasPeelSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {c0?.d_max_layer_peel != null ? c0.d_max_layer_peel.toFixed(3) : '—'}
                      </td>
                    )}
                    {hasOverhangSens && (
                      <td style={{ padding: '2px 4px' }}>
                        {p.d_overhang_fraction != null ? p.d_overhang_fraction.toFixed(4) : '—'}
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
