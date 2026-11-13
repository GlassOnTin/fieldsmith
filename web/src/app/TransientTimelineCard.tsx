import { useEffect, useState } from 'preact/hooks';
import { store } from '../doc/store';
import type { LevelResult, TransientCaseReport } from '../doc/wasm';
import { ui, useApp } from './ui';

function fmt(v: number, digits = 2): string {
  if (!Number.isFinite(v)) return '—';
  return v.toFixed(digits);
}

export function TransientTimelineCard(props: { lvl: LevelResult | null; ci: number }) {
  useApp();
  const { lvl, ci } = props;
  const doc = store.doc;
  const hasThermalBc =
    doc.temperatures.length > 0 ||
    doc.convections.length > 0 ||
    doc.heat_fluxes.length > 0;
  const reports = lvl?.meta.transient ?? [];

  let report: TransientCaseReport | null = null;
  if (reports.length > 0) {
    if (ci >= 0 && lvl && ci < lvl.meta.cases.length) {
      const cname = lvl.meta.cases[ci].name;
      report = reports.find((r) => r.case_name === cname) ?? reports[0];
    } else {
      report = reports[0];
    }
  }

  const docTEnd = doc.solve.t_end ?? null;
  const docSteps = doc.solve.steps ?? null;
  const docTInit = doc.solve.t_init ?? doc.solve.t_ref ?? 20;

  const [tEndDraft, setTEndDraft] = useState<string>(() => String(docTEnd ?? 5));
  const [stepsDraft, setStepsDraft] = useState<string>(() => String(docSteps ?? 16));
  const [tInitDraft, setTInitDraft] = useState<string>(() => String(docTInit));

  useEffect(() => {
    if (docTEnd != null) setTEndDraft(String(docTEnd));
    if (docSteps != null) setStepsDraft(String(docSteps));
    if (doc.solve.t_init != null) setTInitDraft(String(doc.solve.t_init));
    else if (doc.solve.t_ref != null) setTInitDraft(String(doc.solve.t_ref));
  }, [docTEnd, docSteps, doc.solve.t_init, doc.solve.t_ref]);

  if (!hasThermalBc && !report) return null;

  const applyTransientSettings = () => {
    const tEnd = Number.parseFloat(tEndDraft);
    const steps = Math.round(Number.parseFloat(stepsDraft));
    const tInit = Number.parseFloat(tInitDraft);
    if (!(tEnd > 0) || !(steps >= 1 && steps <= 500) || !Number.isFinite(tInit)) {
      ui.notify('t_end must be > 0, steps in 1..500, and t_init finite', 'error');
      return;
    }
    ui.set('resultsTransientStep', -1);
    store.dispatch(`Transient solve (t_end=${tEnd}s, steps=${steps})`, {
      cmd: 'set_solve',
      solve: {
        ...doc.solve,
        t_ref: doc.solve.t_ref ?? tInit,
        t_end: tEnd,
        steps,
        t_init: tInit,
      },
    });
  };

  const clearTransientSettings = () => {
    ui.set('resultsTransientStep', -1);
    store.dispatch('Return to steady-state thermal solve', {
      cmd: 'set_solve',
      solve: {
        ...doc.solve,
        t_end: null,
        steps: null,
        t_init: null,
      },
    });
  };

  const timeline = report?.timeline ?? [];
  const maxStepIdx = Math.max(0, timeline.length - 1);
  const activeStep =
    report && timeline.length > 0
      ? ui.resultsTransientStep >= 0
        ? Math.min(maxStepIdx, ui.resultsTransientStep)
        : Math.min(maxStepIdx, report.step_at_peak_vm)
      : 0;
  const activeSummary = timeline[activeStep] ?? null;

  // Dual-axis SVG chart geometry
  const W = 324;
  const H = 96;
  const padL = 34;
  const padR = 34;
  const padT = 10;
  const padB = 18;
  const pw = W - padL - padR;
  const ph = H - padT - padB;

  let vmMaxChart = 1;
  let tMinChart = 0;
  let tMaxChart = 100;
  if (timeline.length > 0 && report) {
    vmMaxChart = Math.max(
      1e-6,
      report.peak_vm_elem,
      report.steady_limit_vm_elem ?? 0,
      ...timeline.map((s) => s.max_vm_elem),
    );
    tMinChart = Math.min(...timeline.map((s) => s.min_temp_c));
    tMaxChart = Math.max(...timeline.map((s) => s.max_temp_c));
    if (!(tMaxChart > tMinChart + 1e-6)) {
      tMinChart -= 1;
      tMaxChart += 1;
    }
  }

  const xOf = (idx: number) =>
    padL + (maxStepIdx > 0 ? (idx / maxStepIdx) * pw : 0);
  const yVm = (vm: number) =>
    padT + ph - Math.max(0, Math.min(1, vm / vmMaxChart)) * ph;
  const yTemp = (tc: number) =>
    padT + ph - Math.max(0, Math.min(1, (tc - tMinChart) / (tMaxChart - tMinChart))) * ph;

  const vmPolyline = timeline
    .map((s, idx) => `${xOf(idx).toFixed(1)},${yVm(s.max_vm_elem).toFixed(1)}`)
    .join(' ');
  const tMaxPolyline = timeline
    .map((s, idx) => `${xOf(idx).toFixed(1)},${yTemp(s.max_temp_c).toFixed(1)}`)
    .join(' ');
  const tMinPolyline = timeline
    .map((s, idx) => `${xOf(idx).toFixed(1)},${yTemp(s.min_temp_c).toFixed(1)}`)
    .join(' ');

  return (
    <div
      data-testid="transient-thermal-panel"
      style={{
        margin: '6px 0',
        padding: '8px 9px',
        background: 'rgba(15, 23, 42, 0.72)',
        border: '1px solid rgba(251, 191, 36, 0.35)',
        borderRadius: '6px',
        display: 'flex',
        flexDirection: 'column',
        gap: '6px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}>
        <span style={{ fontSize: '11px', fontWeight: 700, color: '#fde68a' }}>
          ⏱ 3D P2 BDF2 Transient Conduction & Thermal Shock
        </span>
        {docTEnd != null && (
          <button
            type="button"
            class="mini-btn"
            data-testid="transient-steady-btn"
            onClick={clearTransientSettings}
            title="Clear solve t_end and return to steady-state conduction (t → ∞)"
          >
            Steady (t → ∞)
          </button>
        )}
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: '5px', flexWrap: 'wrap', fontSize: '10.5px' }}>
        <label style={{ display: 'flex', alignItems: 'center', gap: '3px', color: '#cbd5e1' }}>
          <span>t_end (s)</span>
          <input
            type="number"
            step="any"
            min="0.01"
            data-testid="transient-tend-input"
            value={tEndDraft}
            onInput={(e) => setTEndDraft((e.target as HTMLInputElement).value)}
            style={{ width: '52px', padding: '2px 4px', fontSize: '10.5px', background: '#0f172a', color: '#f8fafc', border: '1px solid #334155', borderRadius: '3px' }}
          />
        </label>
        <label style={{ display: 'flex', alignItems: 'center', gap: '3px', color: '#cbd5e1' }}>
          <span>steps</span>
          <input
            type="number"
            step="1"
            min="1"
            max="500"
            data-testid="transient-steps-input"
            value={stepsDraft}
            onInput={(e) => setStepsDraft((e.target as HTMLInputElement).value)}
            style={{ width: '44px', padding: '2px 4px', fontSize: '10.5px', background: '#0f172a', color: '#f8fafc', border: '1px solid #334155', borderRadius: '3px' }}
          />
        </label>
        <label style={{ display: 'flex', alignItems: 'center', gap: '3px', color: '#cbd5e1' }}>
          <span>T₀ (°C)</span>
          <input
            type="number"
            step="any"
            data-testid="transient-tinit-input"
            value={tInitDraft}
            onInput={(e) => setTInitDraft((e.target as HTMLInputElement).value)}
            style={{ width: '52px', padding: '2px 4px', fontSize: '10.5px', background: '#0f172a', color: '#f8fafc', border: '1px solid #334155', borderRadius: '3px' }}
          />
        </label>
        <button
          type="button"
          class="mini-btn"
          data-testid="transient-enable-btn"
          onClick={applyTransientSettings}
          style={{ marginLeft: 'auto', borderColor: 'rgba(251, 191, 36, 0.5)', color: '#fde68a' }}
        >
          {docTEnd != null ? 'Update BDF2' : 'Run Transient'}
        </button>
      </div>

      {report && timeline.length > 0 && (
        <div
          data-testid="transient-timeline-card"
          style={{ display: 'flex', flexDirection: 'column', gap: '5px' }}
        >
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
              gap: '4px',
              fontSize: '10px',
            }}
          >
            <div style={{ padding: '4px 5px', background: 'rgba(251, 191, 36, 0.09)', border: '1px solid rgba(251, 191, 36, 0.28)', borderRadius: '4px' }}>
              <div style={{ color: '#fcd34d', fontWeight: 600 }}>Peak Shock σ_vM(t*)</div>
              <div class="num" data-testid="transient-peak-vm" style={{ color: '#fef3c7', fontWeight: 700 }}>
                {fmt(report.peak_vm_elem, 1)} MPa @ {fmt(report.t_at_peak_vm_s, 2)} s
              </div>
            </div>
            <div style={{ padding: '4px 5px', background: 'rgba(56, 189, 248, 0.08)', border: '1px solid rgba(56, 189, 248, 0.25)', borderRadius: '4px' }}>
              <div style={{ color: '#7dd3fc', fontWeight: 600 }}>Steady Limit (t → ∞)</div>
              <div class="num" data-testid="transient-steady-vm" style={{ color: '#e0f2fe', fontWeight: 700 }}>
                {report.steady_limit_vm_elem != null
                  ? `${fmt(report.steady_limit_vm_elem, 2)} MPa${
                      report.steady_limit_vm_elem > 0.05 && report.shock_amplification != null
                        ? ` (×${fmt(report.shock_amplification, 2)})`
                        : report.peak_vm_elem > 1
                          ? ' (pure shock)'
                          : ''
                    }`
                  : 'Unanchored flux'}
              </div>
            </div>
            <div style={{ padding: '4px 5px', background: 'rgba(148, 163, 184, 0.08)', border: '1px solid rgba(148, 163, 184, 0.25)', borderRadius: '4px' }}>
              <div style={{ color: '#cbd5e1', fontWeight: 600 }}>Diffusivity α_th</div>
              <div class="num" style={{ color: '#f1f5f9', fontWeight: 700 }}>
                {fmt(report.diffusivity_mm2_s, 2)} mm²/s · Δt {fmt(report.dt_s, 3)} s
              </div>
            </div>
          </div>

          <svg
            viewBox={`0 0 ${W} ${H}`}
            style={{ width: '100%', height: '94px', background: 'rgba(2, 6, 23, 0.65)', borderRadius: '4px', border: '1px solid rgba(51, 65, 85, 0.6)', cursor: 'pointer' }}
            onClick={(e) => {
              const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
              const frac = (e.clientX - rect.left - (padL / W) * rect.width) / ((pw / W) * rect.width);
              const idx = Math.max(0, Math.min(maxStepIdx, Math.round(frac * maxStepIdx)));
              ui.set('resultsTransientStep', idx);
            }}
          >
            {report.steady_limit_vm_elem != null && (
              <line
                x1={padL}
                x2={padL + pw}
                y1={yVm(report.steady_limit_vm_elem)}
                y2={yVm(report.steady_limit_vm_elem)}
                stroke="rgba(148, 163, 184, 0.45)"
                stroke-dasharray="3,3"
                stroke-width="1"
              />
            )}
            <polyline fill="none" stroke="#38bdf8" stroke-width="1.3" stroke-dasharray="2,2" points={tMinPolyline} />
            <polyline fill="none" stroke="#fb7185" stroke-width="1.3" stroke-dasharray="2,2" points={tMaxPolyline} />
            <polyline fill="none" stroke="#fbbf24" stroke-width="2" points={vmPolyline} />
            <circle
              cx={xOf(Math.min(maxStepIdx, report.step_at_peak_vm))}
              cy={yVm(report.peak_vm_elem)}
              r="3.2"
              fill="#fbbf24"
              stroke="#0f172a"
              stroke-width="1"
            />
            <line
              x1={xOf(activeStep)}
              x2={xOf(activeStep)}
              y1={padT}
              y2={padT + ph}
              stroke="#f8fafc"
              stroke-width="1.2"
              stroke-dasharray="2,2"
            />
            <text x={padL - 3} y={padT + 7} text-anchor="end" fill="#fbbf24" font-size="8.5">
              {fmt(vmMaxChart, 0)}
            </text>
            <text x={padL - 3} y={padT + ph} text-anchor="end" fill="#94a3b8" font-size="8.5">
              0 MPa
            </text>
            <text x={padL + pw + 3} y={padT + 7} text-anchor="start" fill="#fb7185" font-size="8.5">
              {fmt(tMaxChart, 0)}°
            </text>
            <text x={padL + pw + 3} y={padT + ph} text-anchor="start" fill="#38bdf8" font-size="8.5">
              {fmt(tMinChart, 0)}°
            </text>
            <text x={padL} y={H - 4} text-anchor="start" fill="#94a3b8" font-size="8.5">
              t = 0
            </text>
            <text x={padL + pw} y={H - 4} text-anchor="end" fill="#94a3b8" font-size="8.5">
              {fmt(report.t_end_s, 2)} s
            </text>
          </svg>

          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <input
              type="range"
              min={0}
              max={maxStepIdx}
              step={1}
              value={activeStep}
              data-testid="transient-step-slider"
              onInput={(e) => {
                const next = Number.parseInt((e.target as HTMLInputElement).value, 10);
                ui.set('resultsTransientStep', Number.isFinite(next) ? next : -1);
              }}
              style={{ flex: 1 }}
            />
            <button
              type="button"
              class="mini-btn"
              data-testid="transient-jump-peak"
              onClick={() => ui.set('resultsTransientStep', -1)}
              title="Snap 3D viewport to Peak Thermal-Shock state (t*)"
              style={{
                borderColor: ui.resultsTransientStep < 0 ? '#fbbf24' : undefined,
                color: ui.resultsTransientStep < 0 ? '#fde68a' : undefined,
              }}
            >
              ⚡ Peak (t*)
            </button>
          </div>

          {activeSummary && (
            <div
              class="num"
              data-testid="transient-active-frame-readout"
              style={{ fontSize: '10px', color: '#e2e8f0', display: 'flex', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}
            >
              <span>
                Step {activeSummary.step}/{report.steps_count} ({activeSummary.step === 0 ? 'IC' : activeSummary.step === 1 ? 'BDF1' : 'BDF2'}) · t = {fmt(activeSummary.time_s, 3)} s
              </span>
              <span>
                σ_vM {fmt(activeSummary.max_vm_elem, 2)} MPa · T {fmt(activeSummary.min_temp_c, 1)}…{fmt(activeSummary.max_temp_c, 1)} °C
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
