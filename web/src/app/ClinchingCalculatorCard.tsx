// Standalone 1D Lee–Kim / Breda Sheet-Metal Clinching Reference Calculator Card
// paired with 3D .fsm J2 Secant Elasto-Plasticity toggles.
// Extracted from results.tsx so domain-specific 1D closed-form curves stay quarantined
// from the general 3D continuum FEA results view.

import { store } from '../doc/store';
import {
  analyzeClinchingProcessWindow,
  extractClinchingSpecFromDoc,
} from '../doc/clinching';
import { isDocJ2Plastic } from '../doc/continuum';
import type { LevelResult } from '../doc/wasm';
import { fmt } from './fields';
import { ui, useApp, type ResultMode } from './ui';

export function ClinchingCalculatorCard({
  lvl,
  mode,
  peakEpsP,
}: {
  lvl: LevelResult;
  mode: ResultMode;
  peakEpsP: number;
}) {
  useApp();
  const spec = extractClinchingSpecFromDoc(store.doc);
  const sweepRes = analyzeClinchingProcessWindow(spec);
  const cur = sweepRes.current;
  const isNlActive = isDocJ2Plastic(store.doc);
  const peakVm = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem));

  const svgW = 320;
  const svgH = 112;
  const padL = 34;
  const padR = 10;
  const padT = 12;
  const padB = 22;
  const plotW = svgW - padL - padR;
  const plotH = svgH - padT - padB;
  const xLo = 0.35;
  const xHi = 1.6;
  const maxF = Math.max(
    2200,
    ...sweepRes.curve.map((p) => Math.max(p.neckFracture1dN, p.buttonPullout1dN)),
  );
  const sx = (x: number) => padL + ((x - xLo) / (xHi - xLo)) * plotW;
  const sy = (f: number) => padT + plotH - Math.min(1, Math.max(0, f / maxF)) * plotH;
  const neckPts = sweepRes.curve.map((p) => `${sx(p.xMm).toFixed(1)},${sy(p.neckFracture1dN).toFixed(1)}`).join(' ');
  const pullPts = sweepRes.curve.map((p) => `${sx(p.xMm).toFixed(1)},${sy(p.buttonPullout1dN).toFixed(1)}`).join(' ');
  const joint1dPts = sweepRes.curve.map((p) => `${sx(p.xMm).toFixed(1)},${sy(p.jointStrength1dN).toFixed(1)}`).join(' ');
  const joint3dPts = sweepRes.curve.map((p) => `${sx(p.xMm).toFixed(1)},${sy(p.jointStrength3dNonlinearN).toFixed(1)}`).join(' ');

  return (
    <div
      data-testid="results-clinching-explorer"
      style={{
        margin: '6px 0',
        padding: '8px 10px',
        background: 'rgba(139, 92, 246, 0.08)',
        border: '1px solid rgba(167, 139, 250, 0.38)',
        borderRadius: '6px',
        display: 'flex',
        flexDirection: 'column',
        gap: '6px',
        fontSize: '10.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
          <span style={{ fontWeight: 700, color: '#ddd6fe', fontSize: '11px' }}>
            🔩 Sheet-Metal Clinching Explorer
          </span>
          <span
            class="sys-badge"
            title="1D Lee–Kim / Breda closed-form reference calculator compared against live 3D .fsm J₂ continuum FEA"
            style={{ fontSize: '9px', padding: '1px 5px' }}
          >
            1D REFERENCE CALCULATOR vs 3D .fsm J₂ FEA
          </span>
        </div>
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
          <button
            type="button"
            class="mini-btn"
            data-testid="clinch-toggle-nonlinear"
            onClick={() =>
              store.dispatch(
                isNlActive ? 'Switch to Linear Hookean Solver' : 'Switch to 3D J2 Secant Plasticity Solver',
                { cmd: 'apply_clinching', request: { j2_plasticity: !isNlActive } },
              )
            }
            style={
              isNlActive
                ? { background: '#7c3aed', borderColor: '#a78bfa', color: '#fff', fontWeight: 700, fontSize: '9.5px' }
                : { fontSize: '9.5px' }
            }
            title="Toggle solve.plasticity between 3D Nonlinear J2 Secant Elasto-Plasticity and Linear Hookean Elasticity"
          >
            {isNlActive ? '⚡ 3D J₂ Secant Solver: ON' : 'Linear Hookean (E₀)'}
          </button>
          <button
            type="button"
            class="mini-btn"
            data-testid="clinch-show-plastic-strain"
            onClick={() => ui.set('resultsMode', 'plastic_strain')}
            style={
              mode === 'plastic_strain'
                ? { background: '#0284c7', borderColor: '#38bdf8', color: '#fff', fontWeight: 700, fontSize: '9.5px' }
                : { fontSize: '9.5px' }
            }
          >
            🧬 View ε_p ({fmt(peakEpsP, 2)}%)
          </button>
        </div>
      </div>

      <div
        style={{
          background: 'rgba(15, 23, 42, 0.72)',
          border: '1px solid rgba(148, 163, 184, 0.2)',
          borderRadius: '5px',
          padding: '4px',
        }}
      >
        <svg viewBox={`0 0 ${svgW} ${svgH}`} style={{ width: '100%', height: 'auto', display: 'block' }}>
          <rect
            x={sx(sweepRes.windowRangeMm[0])}
            y={padT}
            width={Math.max(2, sx(sweepRes.windowRangeMm[1]) - sx(sweepRes.windowRangeMm[0]))}
            height={plotH}
            fill="rgba(16, 185, 129, 0.12)"
          />
          <line x1={padL} y1={padT + plotH} x2={svgW - padR} y2={padT + plotH} stroke="#475569" stroke-width="1" />
          <line x1={padL} y1={padT} x2={padL} y2={padT + plotH} stroke="#475569" stroke-width="1" />
          <polyline fill="none" stroke="#f59e0b" stroke-width="1.2" stroke-dasharray="3,2" points={neckPts} />
          <polyline fill="none" stroke="#38bdf8" stroke-width="1.2" stroke-dasharray="3,2" points={pullPts} />
          <polyline fill="none" stroke="#10b981" stroke-width="2" points={joint1dPts} />
          <polyline fill="none" stroke="#c084fc" stroke-width="1.8" points={joint3dPts} />
          <line
            x1={sx(cur.xMm)}
            y1={padT}
            x2={sx(cur.xMm)}
            y2={padT + plotH}
            stroke="#f43f5e"
            stroke-width="1.5"
          />
          <circle cx={sx(cur.xMm)} cy={sy(cur.jointStrength1dN)} r="3.2" fill="#10b981" stroke="#0f172a" stroke-width="1" />
          <circle cx={sx(cur.xMm)} cy={sy(cur.jointStrength3dNonlinearN)} r="3.2" fill="#c084fc" stroke="#0f172a" stroke-width="1" />
          <text x={padL + 3} y={padT + 9} fill="#f59e0b" font-size="7.5">← Over-Clinched (Neck Fracture)</text>
          <text x={svgW - padR - 3} y={padT + 9} fill="#38bdf8" font-size="7.5" text-anchor="end">Under-Clinched (Button Pull-Out) →</text>
          <text x={sx(sweepRes.optimalXMm)} y={svgH - 4} fill="#6ee7b7" font-size="8" text-anchor="middle">
            Optimal X* = {fmt(sweepRes.optimalXMm, 2)} mm
          </text>
          <text x={4} y={padT + 8} fill="#94a3b8" font-size="7.5">{fmt(maxF / 1000, 1)}kN</text>
          <text x={4} y={padT + plotH} fill="#94a3b8" font-size="7.5">0</text>
        </svg>
        <div style={{ display: 'flex', justifyContent: 'space-between', gap: '4px', flexWrap: 'wrap', fontSize: '9.5px', color: '#cbd5e1', paddingTop: '2px' }}>
          <span><strong style={{ color: '#10b981' }}>━</strong> 1D Lee–Kim Analytic: <strong>{fmt(cur.jointStrength1dN, 0)} N</strong></span>
          <span><strong style={{ color: '#c084fc' }}>━</strong> Neuber–Hencky Notch Analytic: <strong>{fmt(cur.jointStrength3dNonlinearN, 0)} N</strong></span>
          <span>
            Mode:{' '}
            <strong style={{ color: cur.failureMode === 'optimal_transition' ? '#4ade80' : cur.failureMode === 'neck_fracture' ? '#fbbf24' : '#38bdf8' }}>
              {cur.failureMode === 'optimal_transition' ? '✓ Optimal Window (X*)' : cur.failureMode === 'neck_fracture' ? '⚠ Neck Fracture' : '⚠ Button Pull-Out'}
            </strong>
          </span>
        </div>
      </div>

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
        <span style={{ color: '#cbd5e1', fontSize: '10px' }}>
          Bottom Coin <strong>X = {fmt(cur.xMm, 2)} mm</strong>:
        </span>
        <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
          {[
            { x: 0.5, label: '0.50 mm (Over-Clinched)' },
            { x: 0.85, label: '0.85 mm (Nominal)' },
            { x: 1.25, label: '1.25 mm (Under-Clinched)' },
          ].map((pr) => (
            <button
              key={pr.x}
              type="button"
              class="mini-btn"
              data-testid={`clinch-preset-x-${pr.x.toFixed(2)}`}
              onClick={() =>
                store.dispatch(`Set Clinch Bottom Thickness X = ${pr.x} mm`, {
                  cmd: 'apply_clinching',
                  request: { bottom_thickness_x_mm: pr.x },
                })
              }
              style={
                Math.abs(cur.xMm - pr.x) < 0.04
                  ? { background: 'rgba(16, 185, 129, 0.22)', borderColor: '#34d399', color: '#ecfdf5', fontWeight: 700, fontSize: '9.5px' }
                  : { fontSize: '9.5px' }
              }
            >
              {pr.label}
            </button>
          ))}
          <button
            type="button"
            class="mini-btn"
            data-testid="clinch-dial-optimal-x"
            onClick={() =>
              store.dispatch('Dial Optimal Clinch Coin Thickness X*', {
                cmd: 'apply_clinching',
                request: { dial_optimal_x: true },
              })
            }
            style={
              Math.abs(cur.xMm - sweepRes.optimalXMm) < 0.02
                ? { background: 'rgba(16, 185, 129, 0.3)', borderColor: '#10b981', color: '#ecfdf5', fontWeight: 700, fontSize: '9.5px' }
                : { fontSize: '9.5px' }
            }
            title={`Solve hat-curve process window and set X = X* (${fmt(sweepRes.optimalXMm, 2)} mm)`}
          >
            ✨ Dial X* ({fmt(sweepRes.optimalXMm, 2)} mm)
          </button>
        </div>
      </div>

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
        <span style={{ color: '#cbd5e1', fontSize: '10px' }}>
          Beyond-1D Loading Mode (<strong>θ = {fmt(spec.loadAngleDeg, 0)}°</strong>):
        </span>
        <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
          {[
            { deg: 0, label: '0° Cross-Tension' },
            { deg: 45, label: '45° Mixed Peel' },
            { deg: 90, label: '90° Lap-Shear' },
          ].map((ld) => (
            <button
              key={ld.deg}
              type="button"
              class="mini-btn"
              data-testid={`clinch-angle-${ld.deg}`}
              onClick={() =>
                store.dispatch(`Set Clinch Service Load Angle = ${ld.deg}°`, {
                  cmd: 'apply_clinching',
                  request: { load_angle_deg: ld.deg },
                })
              }
              style={
                Math.abs(spec.loadAngleDeg - ld.deg) < 2
                  ? { background: 'rgba(139, 92, 246, 0.28)', borderColor: '#a78bfa', color: '#f5f3ff', fontWeight: 700, fontSize: '9.5px' }
                  : { fontSize: '9.5px' }
              }
            >
              {ld.label}
            </button>
          ))}
        </div>
      </div>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: '3px 8px',
          padding: '5px 7px',
          background: 'rgba(15, 23, 42, 0.55)',
          borderRadius: '4px',
          fontSize: '10px',
        }}
      >
        <span style={{ color: '#94a3b8' }}>Forming force F_punch(X)</span>
        <span class="num" style={{ textAlign: 'right', color: '#e2e8f0' }}>
          {fmt(cur.formingForceKn, 2)} kN (ε̄_coin = {fmt(cur.coinStrain, 2)})
        </span>
        <span style={{ color: '#94a3b8' }}>Neck t_n(X) &amp; Undercut t_u^eff</span>
        <span class="num" style={{ textAlign: 'right', color: '#e2e8f0' }}>
          t_n = {fmt(cur.neckThicknessMm, 3)} mm · t_u = {fmt(cur.undercutEffMm, 3)} mm (−{fmt(cur.springbackLossMm, 3)} springback)
        </span>
        <span style={{ color: '#94a3b8' }}>Notch K_t (Lin) → K_σ (J₂ NL)</span>
        <span class="num" style={{ textAlign: 'right', color: '#c4b5fd' }}>
          K_t = {fmt(cur.elasticKt, 2)} → K_σ = {fmt(cur.plasticKsigma, 2)} (n = {fmt(spec.hardeningExponentN, 2)})
        </span>
        <span style={{ color: '#94a3b8' }}>3D Peak σ_vM &amp; Plastic ε_p</span>
        <span class="num" style={{ textAlign: 'right', color: '#6ee7b7' }}>
          {fmt(peakVm, 1)} MPa · ε_p = {fmt(peakEpsP, 2)}% ({lvl.meta.provenance.iterations} PCG iters)
        </span>
      </div>
    </div>
  );
}
