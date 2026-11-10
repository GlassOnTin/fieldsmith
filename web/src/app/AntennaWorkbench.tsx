import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { store } from '../doc/store';
import { exportDroneTouchstoneS2pWasm, parseFsm, solveDroneAntennaVnaWasm } from '../doc/wasm';
import { viewApi } from './commands';
import { ui, useApp } from './ui';
import {
  DRONE_ANTENNA_TOPOLOGIES,
  analyzeDroneAntenna,
  buildDeadCatDroneFsmText,
  defaultOptimisedDroneAntennaSpec,
  extractDroneAntennaSpecFromDoc,
  formatTouchstoneS2p,
  optimiseDroneAntenna,
  parseTouchstoneVna,
  suggestLNetworkMatching,
  unoptimisedTuckedDroneAntennaSpec,
  type DroneAntennaAnalysisReport,
  type DroneAntennaDesignSpec,
  type DroneAntennaTopologyId,
  type PolarLobePoint,
  type VnaSamplePoint,
} from '../doc/antenna-vna';

let antennaModalOpenState = false;
let antennaModalSeq = 0;
let initialPresetMode: 'from_doc' | 'optimised' | 'unoptimised' = 'from_doc';

export function openAntennaWorkbenchModal(
  presetMode: 'from_doc' | 'optimised' | 'unoptimised' = 'from_doc',
) {
  initialPresetMode = presetMode;
  antennaModalSeq++;
  antennaModalOpenState = true;
  ui.emit();
}

export function closeAntennaWorkbenchModal() {
  antennaModalOpenState = false;
  ui.emit();
}

/** Sync a DroneAntennaDesignSpec into the active Fieldsmith Document as a 4" DeadCat Drone + Gemini Antenna assembly. */
export function syncDeadCatDroneToDocument(
  spec: DroneAntennaDesignSpec,
  report?: DroneAntennaAnalysisReport,
  fitCamera = false,
): boolean {
  const rep = report ?? analyzeDroneAntenna(spec);
  const fsmText = buildDeadCatDroneFsmText(spec, rep);
  const parsed = parseFsm(fsmText);
  if ('error' in parsed) {
    ui.notify(
      `Drone .fsm build error (line ${parsed.error.line}): ${parsed.error.message}`,
      'error',
      6000,
    );
    return false;
  }
  store.load(parsed.ok);
  if (fitCamera) {
    setTimeout(() => viewApi.fit(false), 40);
  }
  return true;
}

/** Render a 50 Ω Smith Chart SVG with constant-r circles, constant-x arcs, VSWR 2:1 qualification circle,
 *  and dual-band Γ(f) impedance loci (820–980 MHz and 2.30–2.58 GHz). */
function SmithChartSvg({
  lowSweep,
  highSweep,
  targetLowGhz,
  targetHighGhz,
  benchSweep,
}: {
  lowSweep: VnaSamplePoint[];
  highSweep: VnaSamplePoint[];
  targetLowGhz: number;
  targetHighGhz: number;
  benchSweep?: VnaSamplePoint[];
}) {
  const cx = 145;
  const cy = 140;
  const R = 112;

  const gammaToXY = (re: number, im: number): [number, number] => [
    cx + re * R,
    cy - im * R,
  ];

  const rCircles = [0.2, 0.5, 1.0, 2.0, 5.0];
  const xValues = [0.25, 0.5, 1.0, 2.0, -0.25, -0.5, -1.0, -2.0];

  const buildReactanceArcPath = (xNorm: number): string => {
    const pts: string[] = [];
    const rSteps = 48;
    for (let i = 0; i <= rSteps; i++) {
      // Sweep r from 0 to 12 logarithmically
      const t = i / rSteps;
      const rNorm = t === 1 ? 14.0 : (0.12 * t) / Math.max(0.01, 1.0 - 0.92 * t) * 8.0;
      const denom = (rNorm + 1.0) * (rNorm + 1.0) + xNorm * xNorm;
      const gRe = (rNorm * rNorm + xNorm * xNorm - 1.0) / denom;
      const gIm = (2.0 * xNorm) / denom;
      const [px, py] = gammaToXY(gRe, gIm);
      pts.push(`${i === 0 ? 'M' : 'L'}${px.toFixed(1)},${py.toFixed(1)}`);
    }
    return pts.join(' ');
  };

  const buildSweepPath = (pts: VnaSamplePoint[]): string =>
    pts
      .map((p, idx) => {
        const [px, py] = gammaToXY(p.gamma_re, p.gamma_im);
        return `${idx === 0 ? 'M' : 'L'}${px.toFixed(1)},${py.toFixed(1)}`;
      })
      .join(' ');

  const lowTarget =
    lowSweep.reduce((best, p) =>
      Math.abs(p.freq_ghz - targetLowGhz) < Math.abs(best.freq_ghz - targetLowGhz)
        ? p
        : best,
    ) ?? lowSweep[0];

  const highTarget =
    highSweep.reduce((best, p) =>
      Math.abs(p.freq_ghz - targetHighGhz) < Math.abs(best.freq_ghz - targetHighGhz)
        ? p
        : best,
    ) ?? highSweep[0];

  const [lowTx, lowTy] = gammaToXY(lowTarget.gamma_re, lowTarget.gamma_im);
  const [highTx, highTy] = gammaToXY(highTarget.gamma_re, highTarget.gamma_im);

  return (
    <svg
      viewBox="0 0 290 295"
      data-testid="antenna-smith-chart-svg"
      style={{
        width: '100%',
        maxWidth: '320px',
        height: 'auto',
        background: '#090D16',
        border: '1px solid var(--line)',
        borderRadius: '6px',
        display: 'block',
      }}
    >
      <defs>
        <clipPath id="smith-unit-clip">
          <circle cx={cx} cy={cy} r={R} />
        </clipPath>
      </defs>

      {/* Outer unit circle |Γ| = 1.0 */}
      <circle
        cx={cx}
        cy={cy}
        r={R}
        fill="#0D1322"
        stroke="#334155"
        stroke-width="1.4"
      />

      {/* Constant resistance circles r = R / Z0 */}
      <g clip-path="url(#smith-unit-clip)">
        {rCircles.map((r) => {
          const cRe = r / (r + 1.0);
          const cRad = 1.0 / (r + 1.0);
          const [ccx, ccy] = gammaToXY(cRe, 0.0);
          return (
            <circle
              key={`r-${r}`}
              cx={ccx}
              cy={ccy}
              r={cRad * R}
              fill="none"
              stroke={r === 1.0 ? '#475569' : '#1E293B'}
              stroke-width={r === 1.0 ? '1.1' : '0.8'}
              stroke-dasharray={r === 1.0 ? undefined : '2 2'}
            />
          );
        })}

        {/* Constant reactance arcs x = X / Z0 */}
        {xValues.map((x) => (
          <path
            key={`x-${x}`}
            d={buildReactanceArcPath(x)}
            fill="none"
            stroke="#1E293B"
            stroke-width="0.8"
          />
        ))}

        {/* Real axis (X = 0) */}
        <line
          x1={cx - R}
          y1={cy}
          x2={cx + R}
          y2={cy}
          stroke="#334155"
          stroke-width="1"
        />
      </g>

      {/* VSWR = 2:1 qualification circle (|Γ| = 1/3, S₁₁ = -9.54 dB) */}
      <circle
        cx={cx}
        cy={cy}
        r={R / 3.0}
        fill="rgba(16, 185, 129, 0.07)"
        stroke="#10B981"
        stroke-width="1.2"
        stroke-dasharray="4 3"
      />
      <text
        x={cx}
        y={cy - R / 3.0 - 4}
        text-anchor="middle"
        fill="#10B981"
        font-size="8.5"
        font-family="var(--mono)"
      >
        VSWR 2:1 (S₁₁ ≤ -10 dB)
      </text>

      {/* Axis labels */}
      <text x={cx - R + 4} y={cy - 4} fill="#64748B" font-size="8" font-family="var(--mono)">
        0 Ω
      </text>
      <text x={cx + 3} y={cy + 10} fill="#94A3B8" font-size="8" font-family="var(--mono)">
        50 Ω
      </text>
      <text x={cx + R - 20} y={cy - 4} fill="#64748B" font-size="8" font-family="var(--mono)">
        ∞ Ω
      </text>
      <text x={cx} y={cy - R + 11} text-anchor="middle" fill="#64748B" font-size="8" font-family="var(--mono)">
        +j50 Ω (Inductive)
      </text>
      <text x={cx} y={cy + R - 5} text-anchor="middle" fill="#64748B" font-size="8" font-family="var(--mono)">
        -j50 Ω (Capacitive)
      </text>

      {/* Low-Band Locus (820–980 MHz) */}
      <path
        d={buildSweepPath(lowSweep)}
        fill="none"
        stroke="#F59E0B"
        stroke-width="2.2"
        stroke-linecap="round"
      />

      {/* High-Band Locus (2.30–2.58 GHz) */}
      <path
        d={buildSweepPath(highSweep)}
        fill="none"
        stroke="#38BDF8"
        stroke-width="2.2"
        stroke-linecap="round"
      />

      {/* Imported Bench Touchstone (.s1p / .s2p) Overlay Locus */}
      {benchSweep && benchSweep.length > 0 && (
        <>
          <path
            d={buildSweepPath(benchSweep.filter((p) => p.freq_ghz < 1.5))}
            fill="none"
            stroke="#34D399"
            stroke-width="2.0"
            stroke-dasharray="4 2"
            stroke-linecap="round"
          />
          <path
            d={buildSweepPath(benchSweep.filter((p) => p.freq_ghz >= 1.5))}
            fill="none"
            stroke="#A855F7"
            stroke-width="2.0"
            stroke-dasharray="4 2"
            stroke-linecap="round"
          />
        </>
      )}

      {/* 50 Ω Bullseye */}
      <circle cx={cx} cy={cy} r="3" fill="#10B981" />

      {/* Target frequency markers */}
      <circle
        cx={lowTx}
        cy={lowTy}
        r="5"
        fill="#F59E0B"
        stroke="#090D16"
        stroke-width="1.5"
      />
      <text
        x={Math.min(248, Math.max(32, lowTx + 7))}
        y={Math.min(245, Math.max(20, lowTy - 5))}
        fill="#FDE047"
        font-size="8.5"
        font-weight="700"
        font-family="var(--mono)"
      >
        {Math.round(targetLowGhz * 1000)} MHz
      </text>

      <circle
        cx={highTx}
        cy={highTy}
        r="5"
        fill="#38BDF8"
        stroke="#090D16"
        stroke-width="1.5"
      />
      <text
        x={Math.min(248, Math.max(32, highTx + 7))}
        y={Math.min(245, Math.max(20, highTy + 11))}
        fill="#7DD3FC"
        font-size="8.5"
        font-weight="700"
        font-family="var(--mono)"
      >
        {targetHighGhz.toFixed(2)} GHz
      </text>

      {/* Legend footer */}
      <g transform="translate(10, 266)">
        <circle cx="6" cy="5" r="4" fill="#F59E0B" />
        <text x="14" y="8" fill="#E2E8F0" font-size="8.5" font-family="var(--mono)">
          915 MHz: {lowTarget.z_real_ohm.toFixed(1)}
          {lowTarget.z_imag_ohm >= 0 ? `+j${lowTarget.z_imag_ohm.toFixed(1)}` : `-j${Math.abs(lowTarget.z_imag_ohm).toFixed(1)}`} Ω (|Γ|={lowTarget.gamma_mag.toFixed(2)})
        </text>
        <circle cx="6" cy="18" r="4" fill="#38BDF8" />
        <text x="14" y="21" fill="#E2E8F0" font-size="8.5" font-family="var(--mono)">
          2.44 GHz: {highTarget.z_real_ohm.toFixed(1)}
          {highTarget.z_imag_ohm >= 0 ? `+j${highTarget.z_imag_ohm.toFixed(1)}` : `-j${Math.abs(highTarget.z_imag_ohm).toFixed(1)}`} Ω (|Γ|={highTarget.gamma_mag.toFixed(2)})
        </text>
      </g>
    </svg>
  );
}

/** Dual-Band VNA Return Loss (`S₁₁`) & Mutual Coupling (`S₂₁`) Frequency Sweep Plot. */
function VnaSweepSvg({
  lowSweep,
  highSweep,
  targetLowGhz,
  targetHighGhz,
  benchSweep,
}: {
  lowSweep: VnaSamplePoint[];
  highSweep: VnaSamplePoint[];
  targetLowGhz: number;
  targetHighGhz: number;
  benchSweep?: VnaSamplePoint[];
}) {
  const W = 360;
  const H = 225;
  const padL = 34;
  const padR = 10;
  const padT = 22;
  const padB = 34;
  const plotW = (W - padL - padR - 14) / 2;
  const plotH = H - padT - padB;

  const dbMin = -35.0;
  const dbMax = 0.0;
  const dbToY = (db: number): number => {
    const clamped = Math.min(dbMax, Math.max(dbMin, db));
    return padT + ((dbMax - clamped) / (dbMax - dbMin)) * plotH;
  };

  const buildBandCurve = (
    pts: VnaSamplePoint[],
    x0: number,
    fMin: number,
    fMax: number,
    field: 's11_db' | 's21_db',
  ): string =>
    pts
      .map((p, i) => {
        const x = x0 + ((p.freq_ghz - fMin) / (fMax - fMin)) * plotW;
        const y = dbToY(p[field]);
        return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' ');

  const xLow0 = padL;
  const xHigh0 = padL + plotW + 14;
  const yMinus10 = dbToY(-10.0);
  const yMinus15 = dbToY(-15.0);

  const xTargetLow = xLow0 + ((targetLowGhz - 0.82) / (0.98 - 0.82)) * plotW;
  const xTargetHigh = xHigh0 + ((targetHighGhz - 2.3) / (2.58 - 2.3)) * plotW;

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      data-testid="antenna-vna-plot-svg"
      style={{
        width: '100%',
        height: 'auto',
        background: '#090D16',
        border: '1px solid var(--line)',
        borderRadius: '6px',
        display: 'block',
      }}
    >
      {/* Left panel: 820–980 MHz */}
      <rect
        x={xLow0}
        y={padT}
        width={plotW}
        height={plotH}
        fill="#0D1322"
        stroke="#1E293B"
      />
      {/* Right panel: 2.30–2.58 GHz */}
      <rect
        x={xHigh0}
        y={padT}
        width={plotW}
        height={plotH}
        fill="#0D1322"
        stroke="#1E293B"
      />

      {/* dB horizontal grid lines */}
      {[-5, -10, -15, -20, -25, -30].map((db) => {
        const y = dbToY(db);
        return (
          <g key={`grid-${db}`}>
            <line
              x1={xLow0}
              y1={y}
              x2={xLow0 + plotW}
              y2={y}
              stroke={db === -10 ? '#10B981' : '#1E293B'}
              stroke-width={db === -10 ? '1.0' : '0.7'}
              stroke-dasharray={db === -10 ? '3 2' : undefined}
            />
            <line
              x1={xHigh0}
              y1={y}
              x2={xHigh0 + plotW}
              y2={y}
              stroke={db === -10 ? '#10B981' : '#1E293B'}
              stroke-width={db === -10 ? '1.0' : '0.7'}
              stroke-dasharray={db === -10 ? '3 2' : undefined}
            />
            <text
              x={padL - 4}
              y={y + 3}
              text-anchor="end"
              fill={db === -10 ? '#10B981' : '#64748B'}
              font-size="8"
              font-family="var(--mono)"
            >
              {db}
            </text>
          </g>
        );
      })}

      {/* Target frequency vertical cursor lines */}
      <line
        x1={xTargetLow}
        y1={padT}
        x2={xTargetLow}
        y2={padT + plotH}
        stroke="#F59E0B"
        stroke-width="0.9"
        stroke-dasharray="2 2"
      />
      <line
        x1={xTargetHigh}
        y1={padT}
        x2={xTargetHigh}
        y2={padT + plotH}
        stroke="#38BDF8"
        stroke-width="0.9"
        stroke-dasharray="2 2"
      />

      {/* S₂₁ isolation curves (dashed purple) */}
      <path
        d={buildBandCurve(lowSweep, xLow0, 0.82, 0.98, 's21_db')}
        fill="none"
        stroke="#A855F7"
        stroke-width="1.4"
        stroke-dasharray="3 2"
      />
      <path
        d={buildBandCurve(highSweep, xHigh0, 2.3, 2.58, 's21_db')}
        fill="none"
        stroke="#A855F7"
        stroke-width="1.4"
        stroke-dasharray="3 2"
      />

      {/* S₁₁ return loss curves (solid amber & cyan) */}
      <path
        d={buildBandCurve(lowSweep, xLow0, 0.82, 0.98, 's11_db')}
        fill="none"
        stroke="#F59E0B"
        stroke-width="2.2"
      />
      <path
        d={buildBandCurve(highSweep, xHigh0, 2.3, 2.58, 's11_db')}
        fill="none"
        stroke="#38BDF8"
        stroke-width="2.2"
      />
      {/* Optional imported bench VNA Touchstone (.s1p / .s2p) return loss overlay */}
      {benchSweep && benchSweep.length > 1 && (
        <>
          <path
            d={buildBandCurve(
              benchSweep.filter((p) => p.freq_ghz >= 0.82 && p.freq_ghz <= 0.98),
              xLow0,
              0.82,
              0.98,
              's11_db',
            )}
            fill="none"
            stroke="#34D399"
            stroke-width="1.9"
            stroke-dasharray="4 2"
          />
          <path
            d={buildBandCurve(
              benchSweep.filter((p) => p.freq_ghz >= 2.3 && p.freq_ghz <= 2.58),
              xHigh0,
              2.3,
              2.58,
              's11_db',
            )}
            fill="none"
            stroke="#34D399"
            stroke-width="1.9"
            stroke-dasharray="4 2"
          />
        </>
      )}

      {/* Panel headers */}
      <text
        x={xLow0 + plotW / 2}
        y={14}
        text-anchor="middle"
        fill="#FDE047"
        font-size="9"
        font-weight="700"
        font-family="var(--mono)"
      >
        Sub-GHz Band (820–980 MHz)
      </text>
      <text
        x={xHigh0 + plotW / 2}
        y={14}
        text-anchor="middle"
        fill="#7DD3FC"
        font-size="9"
        font-weight="700"
        font-family="var(--mono)"
      >
        2.4 GHz Band (2.30–2.58 GHz)
      </text>

      {/* Threshold tag */}
      <text
        x={xLow0 + 4}
        y={yMinus10 - 3}
        fill="#10B981"
        font-size="7.5"
        font-family="var(--mono)"
      >
        -10 dB (VSWR 1.92)
      </text>
      <text
        x={xHigh0 + 4}
        y={yMinus15 - 3}
        fill="#A855F7"
        font-size="7.5"
        font-family="var(--mono)"
      >
        S₂₁ ≤ -15 dB
      </text>

      {/* X-axis ticks */}
      <text x={xLow0} y={padT + plotH + 11} fill="#64748B" font-size="8" font-family="var(--mono)">
        820
      </text>
      <text
        x={xTargetLow}
        y={padT + plotH + 11}
        text-anchor="middle"
        fill="#F59E0B"
        font-size="8"
        font-family="var(--mono)"
      >
        915
      </text>
      <text
        x={xLow0 + plotW}
        y={padT + plotH + 11}
        text-anchor="end"
        fill="#64748B"
        font-size="8"
        font-family="var(--mono)"
      >
        980 MHz
      </text>

      <text x={xHigh0} y={padT + plotH + 11} fill="#64748B" font-size="8" font-family="var(--mono)">
        2.30
      </text>
      <text
        x={xTargetHigh}
        y={padT + plotH + 11}
        text-anchor="middle"
        fill="#38BDF8"
        font-size="8"
        font-family="var(--mono)"
      >
        2.44
      </text>
      <text
        x={xHigh0 + plotW}
        y={padT + plotH + 11}
        text-anchor="end"
        fill="#64748B"
        font-size="8"
        font-family="var(--mono)"
      >
        2.58 GHz
      </text>

      {/* Legend */}
      <g transform={`translate(${padL}, ${H - 10})`}>
        <line x1="0" y1="-3" x2="14" y2="-3" stroke="#F59E0B" stroke-width="2.2" />
        <text x="18" y="0" fill="#E2E8F0" font-size="8" font-family="var(--mono)">
          S₁₁ (900M)
        </text>
        <line x1="82" y1="-3" x2="96" y2="-3" stroke="#38BDF8" stroke-width="2.2" />
        <text x="100" y="0" fill="#E2E8F0" font-size="8" font-family="var(--mono)">
          S₁₁ (2.4G)
        </text>
        <line
          x1="162"
          y1="-3"
          x2="176"
          y2="-3"
          stroke="#A855F7"
          stroke-width="1.5"
          stroke-dasharray="3 2"
        />
        <text x="180" y="0" fill="#E2E8F0" font-size="8" font-family="var(--mono)">
          S₂₁ Diversity Isolation (dB)
        </text>
      </g>
    </svg>
  );
}

/** Polar Far-Field Radiation Pattern Diagram (`Port 1`, `Port 2`, and `Gemini Diversity Envelope`)
 *  with the 4-inch DeadCat drone silhouette at the origin. */
function PolarLobeSvg({
  cutSamples,
  cutPlane,
  bandLabel,
  p10FloorDbi,
  peakGainDbi,
}: {
  cutSamples: PolarLobePoint[];
  cutPlane: 'yaw' | 'pitch' | 'roll';
  bandLabel: string;
  p10FloorDbi: number;
  peakGainDbi: number;
}) {
  const cx = 145;
  const cy = 142;
  const R = 106;
  const minDb = -20.0;
  const maxDb = 6.0;

  const dbToRadius = (db: number): number => {
    const clamped = Math.min(maxDb, Math.max(minDb, db));
    return ((clamped - minDb) / (maxDb - minDb)) * R;
  };

  // Convert angle_deg so that +90° (Nose +Y) points UP on the polar plot for Yaw cut
  const polarToXY = (angleDeg: number, gainDb: number): [number, number] => {
    const rad = (angleDeg * Math.PI) / 180.0;
    const r = dbToRadius(gainDb);
    return [cx + r * Math.cos(rad), cy - r * Math.sin(rad)];
  };

  const buildPolarPath = (
    field: 'port1_gain_dbi' | 'port2_gain_dbi' | 'gemini_combined_gain_dbi',
  ): string => {
    if (!cutSamples.length) return '';
    const pts = cutSamples.map((s, idx) => {
      const [x, y] = polarToXY(s.angle_deg, s[field]);
      return `${idx === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`;
    });
    return `${pts.join(' ')} Z`;
  };

  const ringsDb = [-15, -10, -5, 0, 5];

  return (
    <svg
      viewBox="0 0 290 295"
      data-testid="antenna-polar-lobe-svg"
      style={{
        width: '100%',
        maxWidth: '320px',
        height: 'auto',
        background: '#090D16',
        border: '1px solid var(--line)',
        borderRadius: '6px',
        display: 'block',
      }}
    >
      {/* Concentric dBi gain rings */}
      {ringsDb.map((db) => {
        const r = dbToRadius(db);
        return (
          <g key={`ring-${db}`}>
            <circle
              cx={cx}
              cy={cy}
              r={r}
              fill="none"
              stroke={db === 0 ? '#475569' : '#1E293B'}
              stroke-width={db === 0 ? '1.1' : '0.8'}
              stroke-dasharray={db === 0 ? undefined : '2 2'}
            />
            <text
              x={cx + 3}
              y={cy - r + 8}
              fill="#64748B"
              font-size="7.5"
              font-family="var(--mono)"
            >
              {db > 0 ? `+${db}` : db} dBi
            </text>
          </g>
        );
      })}

      {/* p10 Blind-Spot Floor Circle */}
      <circle
        cx={cx}
        cy={cy}
        r={dbToRadius(p10FloorDbi)}
        fill="none"
        stroke="#F43F5E"
        stroke-width="1.0"
        stroke-dasharray="3 3"
      />

      {/* Spokes every 45° */}
      {[0, 45, 90, 135, 180, 225, 270, 315].map((ang) => {
        const rad = (ang * Math.PI) / 180.0;
        return (
          <line
            key={`spoke-${ang}`}
            x1={cx}
            y1={cy}
            x2={cx + R * Math.cos(rad)}
            y2={cy - R * Math.sin(rad)}
            stroke="#1E293B"
            stroke-width="0.8"
          />
        );
      })}

      {/* Subtle DeadCat Drone Silhouette at origin */}
      <g opacity="0.45">
        <line x1={cx - 16} y1={cy - 10} x2={cx + 16} y2={cy - 10} stroke="#94A3B8" stroke-width="2" />
        <line x1={cx - 14} y1={cy + 12} x2={cx + 14} y2={cy + 12} stroke="#94A3B8" stroke-width="2" />
        <rect x={cx - 4} y={cy - 12} width="8" height="24" rx="2" fill="#334155" />
      </g>

      {/* Port 1 Lobe (Amber dashed) */}
      <path
        d={buildPolarPath('port1_gain_dbi')}
        fill="none"
        stroke="#F59E0B"
        stroke-width="1.3"
        stroke-dasharray="3 2"
      />

      {/* Port 2 Lobe (Purple dashed) */}
      <path
        d={buildPolarPath('port2_gain_dbi')}
        fill="none"
        stroke="#A855F7"
        stroke-width="1.3"
        stroke-dasharray="3 2"
      />

      {/* Gemini Diversity Combined Lobe (Emerald solid + fill) */}
      <path
        d={buildPolarPath('gemini_combined_gain_dbi')}
        fill="rgba(16, 185, 129, 0.16)"
        stroke="#10B981"
        stroke-width="2.2"
      />

      {/* Cardinal direction labels */}
      <text
        x={cx}
        y={18}
        text-anchor="middle"
        fill="#E2E8F0"
        font-size="8.5"
        font-weight="700"
        font-family="var(--mono)"
      >
        {cutPlane === 'yaw'
          ? 'NOSE +Y (90° — Cam/Battery Shadow)'
          : cutPlane === 'pitch'
            ? 'ZENITH +Z (90°)'
            : 'ZENITH +Z (90°)'}
      </text>
      <text
        x={cx}
        y={cy + R + 13}
        text-anchor="middle"
        fill="#38BDF8"
        font-size="8.5"
        font-weight="700"
        font-family="var(--mono)"
      >
        {cutPlane === 'yaw'
          ? 'TAIL -Y (270° — Rear V-Pod)'
          : cutPlane === 'pitch'
            ? 'NADIR -Z (270°)'
            : 'NADIR -Z (270°)'}
      </text>
      <text
        x={cx + R + 5}
        y={cy + 3}
        fill="#94A3B8"
        font-size="8"
        font-family="var(--mono)"
      >
        {cutPlane === 'pitch' ? '+Y' : '+X'}
      </text>
      <text
        x={cx - R - 5}
        y={cy + 3}
        text-anchor="end"
        fill="#94A3B8"
        font-size="8"
        font-family="var(--mono)"
      >
        {cutPlane === 'pitch' ? '-Y' : '-X'}
      </text>

      {/* Legend footer */}
      <g transform="translate(10, 268)">
        <line x1="0" y1="3" x2="12" y2="3" stroke="#10B981" stroke-width="2.2" />
        <text x="16" y="6" fill="#E2E8F0" font-size="8" font-family="var(--mono)">
          Gemini Combined (Peak {peakGainDbi >= 0 ? `+${peakGainDbi.toFixed(1)}` : peakGainDbi.toFixed(1)} dBi)
        </text>
        <line x1="0" y1="16" x2="12" y2="16" stroke="#F59E0B" stroke-width="1.4" stroke-dasharray="3 2" />
        <text x="16" y="19" fill="#94A3B8" font-size="8" font-family="var(--mono)">
          Port 1
        </text>
        <line x1="62" y1="16" x2="74" y2="16" stroke="#A855F7" stroke-width="1.4" stroke-dasharray="3 2" />
        <text x="78" y="19" fill="#94A3B8" font-size="8" font-family="var(--mono)">
          Port 2
        </text>
        <line x1="124" y1="16" x2="136" y2="16" stroke="#F43F5E" stroke-width="1.2" stroke-dasharray="2 2" />
        <text x="140" y="19" fill="#FDA4AF" font-size="8" font-family="var(--mono)">
          p10 Floor: {p10FloorDbi.toFixed(1)} dBi ({bandLabel})
        </text>
      </g>
    </svg>
  );
}

export function AntennaWorkbenchModal() {
  useApp();

  const [spec, setSpec] = useState<DroneAntennaDesignSpec>(() =>
    defaultOptimisedDroneAntennaSpec(),
  );
  const [cutPlane, setCutPlane] = useState<'yaw' | 'pitch' | 'roll'>('yaw');
  const [lobeBand, setLobeBand] = useState<'low_900' | 'high_2400'>('high_2400');
  const [autoSync3d, setAutoSync3d] = useState<boolean>(true);
  const [benchSweep, setBenchSweep] = useState<VnaSamplePoint[]>([]);
  const [benchFileName, setBenchFileName] = useState<string>('');
  const touchstoneInputRef = useRef<HTMLInputElement | null>(null);
  const cadSyncTimerRef = useRef<number>(0);
  const pendingSpecRef = useRef<DroneAntennaDesignSpec | null>(null);
  const [lastOptDelta, setLastOptDelta] = useState<{
    beforeS11Low: number;
    afterS11Low: number;
    beforeS11High: number;
    afterS11High: number;
    beforeEffLow: number;
    afterEffLow: number;
    beforeFloor: number;
    afterFloor: number;
  } | null>(null);

  useEffect(() => {
    if (!antennaModalOpenState) return;
    if (initialPresetMode === 'unoptimised') {
      setSpec(unoptimisedTuckedDroneAntennaSpec());
      setLastOptDelta(null);
    } else if (initialPresetMode === 'optimised') {
      setSpec(defaultOptimisedDroneAntennaSpec());
      setLastOptDelta(null);
    } else {
      const fromDoc = extractDroneAntennaSpecFromDoc(store.doc);
      if (fromDoc) {
        setSpec(fromDoc);
      } else {
        setSpec(defaultOptimisedDroneAntennaSpec());
      }
    }
  }, [antennaModalSeq]);

  const report = useMemo(() => analyzeDroneAntenna(spec), [spec]);

  if (!antennaModalOpenState) return null;

  const flushPendingCadSync = () => {
    if (cadSyncTimerRef.current) {
      window.clearTimeout(cadSyncTimerRef.current);
      cadSyncTimerRef.current = 0;
    }
    if (pendingSpecRef.current) {
      const s = pendingSpecRef.current;
      pendingSpecRef.current = null;
      syncDeadCatDroneToDocument(s, analyzeDroneAntenna(s), false);
    }
  };

  const handleCloseModal = () => {
    flushPendingCadSync();
    closeAntennaWorkbenchModal();
  };

  useEffect(() => {
    if (!antennaModalOpenState) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        handleCloseModal();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [antennaModalOpenState]);

  const updateSpec = (
    patch: Partial<DroneAntennaDesignSpec>,
    syncCad = autoSync3d,
    immediate = false,
  ) => {
    const next: DroneAntennaDesignSpec = {
      ...spec,
      ...patch,
      overlay_band: patch.overlay_band ?? lobeBand,
    };
    setSpec(next);
    if (syncCad) {
      if (immediate) {
        if (cadSyncTimerRef.current) {
          window.clearTimeout(cadSyncTimerRef.current);
          cadSyncTimerRef.current = 0;
        }
        pendingSpecRef.current = null;
        syncDeadCatDroneToDocument(next, analyzeDroneAntenna(next), false);
      } else {
        // Debounce 36-leaf CSG + WebGL2 fragment shader recompilation during continuous
        // touch/mouse slider drags so VNA / Smith Chart / Polar Lobe plots run at 60-120 fps.
        pendingSpecRef.current = next;
        if (cadSyncTimerRef.current) {
          window.clearTimeout(cadSyncTimerRef.current);
        }
        cadSyncTimerRef.current = window.setTimeout(() => {
          cadSyncTimerRef.current = 0;
          const s = pendingSpecRef.current;
          pendingSpecRef.current = null;
          if (s) {
            syncDeadCatDroneToDocument(s, analyzeDroneAntenna(s), false);
          }
        }, 160);
      }
    }
  };

  const handleLoadUnoptimised = () => {
    const bad = {
      ...unoptimisedTuckedDroneAntennaSpec(),
      overlay_3d_lobe: spec.overlay_3d_lobe ?? true,
      overlay_band: lobeBand,
    };
    setSpec(bad);
    setLastOptDelta(null);
    syncDeadCatDroneToDocument(bad, analyzeDroneAntenna(bad), true);
    ui.notify(
      'Loaded unoptimised tucked build (d_cfrp = 6.5 mm, detuned TPU boot, 38° skew) — severe CFRP eddy loss & VSWR mismatch',
      'info',
      5000,
    );
  };

  const handleLoadOptimised = () => {
    const opt = {
      ...defaultOptimisedDroneAntennaSpec(),
      overlay_3d_lobe: spec.overlay_3d_lobe ?? true,
      overlay_band: lobeBand,
    };
    setSpec(opt);
    setLastOptDelta(null);
    syncDeadCatDroneToDocument(opt, analyzeDroneAntenna(opt), true);
    ui.notify(
      'Loaded optimised 4" DeadCat Gemini V-Tail build (24 mm CFRP standoff, 86° orthogonal skew, L-network matched)',
      'info',
      5000,
    );
  };

  const handleRunAutoOptimiser = () => {
    const res = optimiseDroneAntenna(spec, 8);
    const nextSpec: DroneAntennaDesignSpec = {
      ...res.optimisedSpec,
      overlay_3d_lobe: spec.overlay_3d_lobe ?? true,
      overlay_band: lobeBand,
    };
    setSpec(nextSpec);
    setLastOptDelta({
      beforeS11Low: res.initialReport.low_band_summary.s11_at_target_db,
      afterS11Low: res.optimisedReport.low_band_summary.s11_at_target_db,
      beforeS11High: res.initialReport.high_band_summary.s11_at_target_db,
      afterS11High: res.optimisedReport.high_band_summary.s11_at_target_db,
      beforeEffLow: res.initialReport.low_band_summary.total_efficiency_pct,
      afterEffLow: res.optimisedReport.low_band_summary.total_efficiency_pct,
      beforeFloor: Math.min(
        res.initialReport.low_band_summary.p10_blind_spot_floor_dbi,
        res.initialReport.high_band_summary.p10_blind_spot_floor_dbi,
      ),
      afterFloor: Math.min(
        res.optimisedReport.low_band_summary.p10_blind_spot_floor_dbi,
        res.optimisedReport.high_band_summary.p10_blind_spot_floor_dbi,
      ),
    });
    syncDeadCatDroneToDocument(nextSpec, res.optimisedReport, true);
    ui.notify(
      `Auto-Optimised Gemini Antenna: 915 MHz S₁₁ = ${res.optimisedReport.low_band_summary.s11_at_target_db.toFixed(1)} dB, 2.44 GHz S₁₁ = ${res.optimisedReport.high_band_summary.s11_at_target_db.toFixed(1)} dB, η = ${res.optimisedReport.high_band_summary.total_efficiency_pct.toFixed(1)}%`,
      'info',
      6000,
    );
  };

  const handleApplySuggestedMatchOnly = () => {
    const [lNh, cPf] = suggestLNetworkMatching(spec);
    updateSpec({ match_series_nh: lNh, match_shunt_pf: cPf }, true);
    ui.notify(`Applied conjugate L-network match: L_ser = ${lNh.toFixed(2)} nH, C_sh = ${cPf.toFixed(2)} pF`);
  };

  const handleExportTouchstoneS2p = () => {
    const s2pText = formatTouchstoneS2p(report);
    const blob = new Blob([s2pText], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `fieldsmith-deadcat-gemini-${spec.topology}.s2p`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    ui.notify(`Exported Touchstone 2-port VNA sweep (${a.download})`, 'info', 4000);
  };

  const handleImportTouchstoneFile = async (file: File) => {
    try {
      const text = await file.text();
      const pts = parseTouchstoneVna(text);
      if (pts.length === 0) {
        ui.notify(`Touchstone file ${file.name} contained no valid VNA frequency rows`, 'error', 5000);
        return;
      }
      setBenchSweep(pts);
      setBenchFileName(file.name);
      ui.notify(`Overlaid ${pts.length} bench VNA points from ${file.name} on Smith Chart & VNA Plot`, 'info', 5000);
    } catch (e) {
      ui.notify(`Failed to import Touchstone file: ${e instanceof Error ? e.message : String(e)}`, 'error', 5000);
    }
  };

  const handleLoadSampleBenchVna = () => {
    const benchSpec: DroneAntennaDesignSpec = {
      ...spec,
      cfrp_standoff_mm: 11.5,
      elevation_above_plate_mm: 6.0,
      match_series_nh: 0.0,
      match_shunt_pf: 0.0,
    };
    let s2pText: string;
    try {
      solveDroneAntennaVnaWasm<DroneAntennaAnalysisReport>(benchSpec);
      s2pText = exportDroneTouchstoneS2pWasm(benchSpec);
    } catch {
      s2pText = formatTouchstoneS2p(analyzeDroneAntenna(benchSpec));
    }
    const pts = parseTouchstoneVna(s2pText);
    setBenchSweep(pts);
    setBenchFileName('sample-nanovna-bench.s2p');
    ui.notify(`Loaded sample NanoVNA bench trace (${pts.length} pts) onto Smith Chart & VNA Plot`, 'info', 4000);
  };

  const activeBandSummary =
    lobeBand === 'low_900' ? report.low_band_summary : report.high_band_summary;
  const activeCutSamples =
    cutPlane === 'yaw'
      ? activeBandSummary.yaw_horizon_cut
      : cutPlane === 'pitch'
        ? activeBandSummary.pitch_elevation_cut
        : activeBandSummary.roll_bank_cut;

  const lowSum = report.low_band_summary;
  const highSum = report.high_band_summary;

  return (
    <div
      class="modal-backdrop"
      data-testid="antenna-workbench-modal"
      onClick={(e) => {
        if (e.target === e.currentTarget) handleCloseModal();
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(5, 8, 15, 0.78)',
        backdropFilter: 'blur(4px)',
        zIndex: 1200,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '12px',
      }}
    >
      <div
        class="modal-card"
        style={{
          width: 'min(1260px, 98vw)',
          maxHeight: '94vh',
          background: '#0F172A',
          color: '#F1F5F9',
          border: '1px solid #334155',
          borderRadius: '10px',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
          boxShadow: '0 24px 60px rgba(0,0,0,0.65)',
        }}
      >
        {/* Modal Header */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '10px',
            padding: '10px 14px',
            borderBottom: '1px solid #334155',
            background: '#1E293B',
            flexWrap: 'wrap',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: '10px', flexWrap: 'wrap' }}>
            <span style={{ fontWeight: 700, fontSize: '14px', color: '#F8FAFC' }}>
              📡 4&quot; DeadCat FPV Drone &amp; Gemini Dual-Band (900 MHz + 2.4 GHz) Antenna VNA / Smith / 3D Lobe Workbench
            </span>
            <span
              data-testid="antenna-qualification-badge"
              style={{
                fontSize: '11px',
                fontWeight: 700,
                fontFamily: 'var(--mono)',
                padding: '2px 8px',
                borderRadius: '999px',
                background: report.all_targets_met
                  ? 'rgba(16, 185, 129, 0.18)'
                  : 'rgba(244, 63, 94, 0.2)',
                color: report.all_targets_met ? '#34D399' : '#FDA4AF',
                border: `1px solid ${report.all_targets_met ? '#10B981' : '#F43F5E'}`,
              }}
            >
              {report.all_targets_met
                ? 'PASS · DUAL-BAND 50 Ω MATCHED & CFRP DECOUPLED'
                : 'DETUNED / CFRP EDDY LOSS — OPTIMISATION REQUIRED'}
            </span>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <button
              class="btn small"
              data-testid="antenna-sync-cad-btn"
              onClick={() => {
                syncDeadCatDroneToDocument(spec, report, true);
                ui.notify('Synced 4" DeadCat Drone + 3D Radiation Lobe to CAD Viewport');
              }}
              style={{ background: '#0284C7', color: '#F8FAFC', borderColor: '#38BDF8' }}
              title="Build/update the 4-inch DeadCat carbon-fibre drone + motors + PCBs + Gemini antenna + 3D lobe in the CAD viewport"
            >
              🚁 Sync 4&quot; DeadCat to 3D CAD
            </button>
            <button
              class="btn small"
              data-testid="close-antenna-workbench"
              onClick={handleCloseModal}
              style={{ background: '#334155', color: '#F8FAFC', borderColor: '#475569' }}
            >
              ✕ Close
            </button>
          </div>
        </div>

        {/* Preset & Action Toolbar */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '8px',
            padding: '8px 14px',
            borderBottom: '1px solid #334155',
            background: '#0B1120',
            flexWrap: 'wrap',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <button
              class="btn small"
              data-testid="antenna-preset-unoptimised"
              onClick={handleLoadUnoptimised}
              style={{ background: 'rgba(244, 63, 94, 0.15)', borderColor: '#F43F5E', color: '#FDA4AF' }}
            >
              ⚠️ Load Tucked CFRP Build (6.5 mm Standoff, Detuned)
            </button>
            <button
              class="btn small"
              data-testid="antenna-preset-optimised"
              onClick={handleLoadOptimised}
              style={{ background: 'rgba(16, 185, 129, 0.15)', borderColor: '#10B981', color: '#6EE7B7' }}
            >
              ✅ Load Optimised V-Tail Build (24 mm Standoff, 86° Skew)
            </button>
            <button
              class="btn primary small"
              data-testid="antenna-optimise-btn"
              onClick={handleRunAutoOptimiser}
            >
              ⚡ Auto-Optimise Geometry &amp; L/C Match
            </button>
            <button
              class="btn small"
              data-testid="antenna-export-touchstone-btn"
              onClick={handleExportTouchstoneS2p}
              title="Export dual-band VNA S-parameters (S11, S21, S12, S22) as an industry-standard Touchstone .s2p file"
            >
              ⬇ Export Touchstone (.s2p)
            </button>
            <button
              class="btn small"
              data-testid="antenna-import-touchstone-btn"
              onClick={() => touchstoneInputRef.current?.click()}
              title="Import a measured bench VNA Touchstone (.s1p / .s2p) file to overlay directly on the 50 Ω Smith Chart & VNA Sweep"
            >
              📂 Overlay Bench VNA (.s1p/.s2p)
            </button>
            <button
              class="btn small"
              data-testid="antenna-load-sample-bench-btn"
              onClick={handleLoadSampleBenchVna}
              title="Load a sample NanoVNA bench trace (.s2p) with pigtail/TPU detuning to inspect sim-vs-bench Smith & VNA overlay"
            >
              🧪 Sample NanoVNA (.s2p)
            </button>
            {benchSweep.length > 0 && (
              <span
                data-testid="antenna-bench-overlay-badge"
                style={{
                  fontSize: '10.5px',
                  fontFamily: 'var(--mono)',
                  color: '#34D399',
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '4px',
                }}
              >
                ✓ Bench: {benchFileName} ({benchSweep.length} pts)
                <button
                  type="button"
                  class="mini-btn"
                  onClick={() => {
                    setBenchSweep([]);
                    setBenchFileName('');
                  }}
                >
                  ✕
                </button>
              </span>
            )}
            <input
              ref={touchstoneInputRef}
              type="file"
              accept=".s1p,.s2p,.txt"
              style={{ display: 'none' }}
              onChange={(e) => {
                const f = (e.target as HTMLInputElement).files?.[0];
                if (f) void handleImportTouchstoneFile(f);
              }}
            />
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: '10px', flexWrap: 'wrap', fontSize: '11px', color: '#E2E8F0' }}>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '5px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={!!spec.overlay_3d_lobe}
                data-testid="antenna-toggle-3d-lobe"
                onChange={(e) =>
                  updateSpec({ overlay_3d_lobe: (e.target as HTMLInputElement).checked }, true, true)
                }
              />
              <span>3D Lobe Envelope in CAD</span>
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '5px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={autoSync3d}
                onChange={(e) => setAutoSync3d((e.target as HTMLInputElement).checked)}
              />
              <span>Live CAD Sync</span>
            </label>
          </div>
        </div>

        {/* Body Content */}
        <div
          style={{
            padding: '12px 14px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '12px',
          }}
        >
          {/* KPI Summary Strip */}
          <div
            data-testid="antenna-kpi-strip"
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(175px, 1fr))',
              gap: '8px',
            }}
          >
            <div
              style={{
                padding: '8px 10px',
                background: 'rgba(30, 41, 59, 0.55)',
                border: `1px solid ${lowSum.s11_at_target_db <= -10 ? '#10B981' : '#F43F5E'}`,
                borderRadius: '6px',
              }}
            >
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                915 MHz SUB-GHZ VNA MATCH
              </div>
              <div
                data-testid="kpi-915-s11"
                style={{ fontSize: '14px', fontWeight: 700, fontFamily: 'var(--mono)', color: '#FDE047' }}
              >
                S₁₁ = {lowSum.s11_at_target_db.toFixed(1)} dB · VSWR {lowSum.vswr_at_target.toFixed(2)}
              </div>
              <div style={{ fontSize: '10.5px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                f₀ = {(lowSum.resonant_freq_ghz * 1000).toFixed(0)} MHz (Δf {lowSum.freq_detuning_mhz >= 0 ? `+${lowSum.freq_detuning_mhz.toFixed(0)}` : lowSum.freq_detuning_mhz.toFixed(0)} MHz) · BW {lowSum.bandwidth_s11_below_10db_mhz.toFixed(0)} MHz
              </div>
            </div>

            <div
              style={{
                padding: '8px 10px',
                background: 'rgba(30, 41, 59, 0.55)',
                border: `1px solid ${highSum.s11_at_target_db <= -10 ? '#10B981' : '#F43F5E'}`,
                borderRadius: '6px',
              }}
            >
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                2.44 GHZ HIGH-BAND VNA MATCH
              </div>
              <div
                data-testid="kpi-2440-s11"
                style={{ fontSize: '14px', fontWeight: 700, fontFamily: 'var(--mono)', color: '#7DD3FC' }}
              >
                S₁₁ = {highSum.s11_at_target_db.toFixed(1)} dB · VSWR {highSum.vswr_at_target.toFixed(2)}
              </div>
              <div style={{ fontSize: '10.5px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                f₀ = {(highSum.resonant_freq_ghz * 1000).toFixed(0)} MHz (Δf {highSum.freq_detuning_mhz >= 0 ? `+${highSum.freq_detuning_mhz.toFixed(0)}` : highSum.freq_detuning_mhz.toFixed(0)} MHz) · BW {highSum.bandwidth_s11_below_10db_mhz.toFixed(0)} MHz
              </div>
            </div>

            <div
              style={{
                padding: '8px 10px',
                background: 'rgba(30, 41, 59, 0.55)',
                border: '1px solid var(--line)',
                borderRadius: '6px',
              }}
            >
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                CFRP EDDY LOSS &amp; RAD EFFICIENCY
              </div>
              <div
                data-testid="kpi-efficiency"
                style={{ fontSize: '14px', fontWeight: 700, fontFamily: 'var(--mono)', color: '#34D399' }}
              >
                η₉₁₅ = {lowSum.total_efficiency_pct.toFixed(1)}% · η₂.₄G = {highSum.total_efficiency_pct.toFixed(1)}%
              </div>
              <div style={{ fontSize: '10.5px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                CFRP Eddy Loss: {lowSum.cfrp_power_loss_pct.toFixed(1)}% (915M) / {highSum.cfrp_power_loss_pct.toFixed(1)}% (2.4G)
              </div>
            </div>

            <div
              style={{
                padding: '8px 10px',
                background: 'rgba(30, 41, 59, 0.55)',
                border: '1px solid var(--line)',
                borderRadius: '6px',
              }}
            >
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                GEMINI DIVERSITY &amp; LINK RANGE (100 mW)
              </div>
              <div
                data-testid="kpi-diversity-range"
                style={{ fontSize: '14px', fontWeight: 700, fontFamily: 'var(--mono)', color: '#C084FC' }}
              >
                S₂₁ = {highSum.s21_isolation_db.toFixed(1)} dB · ECC {highSum.envelope_correlation_ecc.toFixed(3)}
              </div>
              <div style={{ fontSize: '10.5px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
                p10 Floor: {highSum.p10_blind_spot_floor_dbi.toFixed(1)} dBi · Range {lowSum.estimated_los_range_km.toFixed(1)} / {highSum.estimated_los_range_km.toFixed(1)} km
              </div>
            </div>
          </div>

          {/* Before / After Auto-Optimiser Comparison Banner */}
          {lastOptDelta && (
            <div
              data-testid="antenna-opt-delta-banner"
              style={{
                padding: '8px 12px',
                background: 'rgba(16, 185, 129, 0.12)',
                border: '1px solid #10B981',
                borderRadius: '6px',
                fontSize: '11.5px',
                fontFamily: 'var(--mono)',
                color: '#D1FAE5',
              }}
            >
              <strong>⚡ Multi-Objective Antenna Optimisation Converged:</strong>{' '}
              915 MHz S₁₁: {lastOptDelta.beforeS11Low.toFixed(1)} dB →{' '}
              <strong>{lastOptDelta.afterS11Low.toFixed(1)} dB</strong> · 2.44 GHz S₁₁:{' '}
              {lastOptDelta.beforeS11High.toFixed(1)} dB →{' '}
              <strong>{lastOptDelta.afterS11High.toFixed(1)} dB</strong> · 915 MHz Efficiency η:{' '}
              {lastOptDelta.beforeEffLow.toFixed(1)}% →{' '}
              <strong>{lastOptDelta.afterEffLow.toFixed(1)}%</strong> · p10 Blind-Spot Floor:{' '}
              {lastOptDelta.beforeFloor.toFixed(1)} dBi →{' '}
              <strong>{lastOptDelta.afterFloor.toFixed(1)} dBi</strong>
            </div>
          )}

          {/* Main 3-Column Instrument Grid: Smith Chart | VNA S11/S21 Sweep | Polar 3D Lobe Cut */}
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(290px, 1fr))',
              gap: '12px',
              alignItems: 'start',
            }}
          >
            {/* Column 1: 50 Ω Smith Chart */}
            <div
              style={{
                background: 'rgba(15, 23, 42, 0.6)',
                border: '1px solid var(--line)',
                borderRadius: '8px',
                padding: '10px',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                gap: '6px',
              }}
            >
              <div
                style={{
                  width: '100%',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  fontSize: '11.5px',
                  fontWeight: 700,
                }}
              >
                <span>1. 50 Ω Smith Chart Impedance Locus Γ(f)</span>
                <span style={{ fontSize: '10px', fontFamily: 'var(--mono)', color: 'var(--text-3)' }}>
                  Z₀ = 50 Ω
                </span>
              </div>
              <SmithChartSvg
                lowSweep={report.low_band_sweep}
                highSweep={report.high_band_sweep}
                targetLowGhz={spec.target_low_freq_ghz}
                targetHighGhz={spec.target_high_freq_ghz}
                benchSweep={benchSweep}
              />
            </div>

            {/* Column 2: Dual-Band VNA S11 & S21 Sweep */}
            <div
              style={{
                background: 'rgba(15, 23, 42, 0.6)',
                border: '1px solid var(--line)',
                borderRadius: '8px',
                padding: '10px',
                display: 'flex',
                flexDirection: 'column',
                gap: '8px',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  fontSize: '11.5px',
                  fontWeight: 700,
                }}
              >
                <span>2. Dual-Band VNA Scattering (S₁₁ &amp; S₂₁)</span>
                <span style={{ fontSize: '10px', fontFamily: 'var(--mono)', color: 'var(--text-3)' }}>
                  2-Port Gemini Xrossband
                </span>
              </div>
              <VnaSweepSvg
                lowSweep={report.low_band_sweep}
                highSweep={report.high_band_sweep}
                targetLowGhz={spec.target_low_freq_ghz}
                targetHighGhz={spec.target_high_freq_ghz}
                benchSweep={benchSweep}
              />

              {/* Engineering diagnostic notes */}
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '4px',
                  fontSize: '11px',
                  lineHeight: 1.35,
                  background: '#090D16',
                  padding: '8px',
                  borderRadius: '6px',
                  border: '1px solid #1E293B',
                }}
              >
                {report.engineering_notes.map((note, idx) => (
                  <div
                    key={idx}
                    style={{
                      color: note.includes('PASS') ? '#6EE7B7' : '#FDE68A',
                    }}
                  >
                    • {note}
                  </div>
                ))}
              </div>
            </div>

            {/* Column 3: Polar Radiation Lobe Pattern */}
            <div
              style={{
                background: 'rgba(15, 23, 42, 0.6)',
                border: '1px solid var(--line)',
                borderRadius: '8px',
                padding: '10px',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                gap: '6px',
              }}
            >
              <div
                style={{
                  width: '100%',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  gap: '6px',
                  flexWrap: 'wrap',
                }}
              >
                <span style={{ fontSize: '11.5px', fontWeight: 700 }}>
                  3. Far-Field Radiation Lobe G(θ, φ)
                </span>
                <div style={{ display: 'flex', gap: '4px' }}>
                  <select
                    value={lobeBand}
                    data-testid="antenna-lobe-band-select"
                    onChange={(e) => {
                      const b = (e.target as HTMLSelectElement).value as 'low_900' | 'high_2400';
                      setLobeBand(b);
                      updateSpec({ overlay_band: b }, autoSync3d);
                    }}
                    style={{
                      fontSize: '10.5px',
                      padding: '2px 6px',
                      background: '#1E293B',
                      color: '#F8FAFC',
                      border: '1px solid #475569',
                      borderRadius: '4px',
                    }}
                  >
                    <option value="low_900">915 MHz</option>
                    <option value="high_2400">2.44 GHz</option>
                  </select>
                  <select
                    value={cutPlane}
                    data-testid="antenna-lobe-plane-select"
                    onChange={(e) =>
                      setCutPlane((e.target as HTMLSelectElement).value as 'yaw' | 'pitch' | 'roll')
                    }
                    style={{
                      fontSize: '10.5px',
                      padding: '2px 6px',
                      background: '#1E293B',
                      color: '#F8FAFC',
                      border: '1px solid #475569',
                      borderRadius: '4px',
                    }}
                  >
                    <option value="yaw">Yaw Horizon (XY)</option>
                    <option value="pitch">Pitch Elevation (YZ)</option>
                    <option value="roll">Roll Bank (XZ)</option>
                  </select>
                </div>
              </div>

              <PolarLobeSvg
                cutSamples={activeCutSamples}
                cutPlane={cutPlane}
                bandLabel={lobeBand === 'low_900' ? '915 MHz' : '2.44 GHz'}
                p10FloorDbi={activeBandSummary.p10_blind_spot_floor_dbi}
                peakGainDbi={activeBandSummary.peak_gain_dbi}
              />
            </div>
          </div>

          {/* Parametric Geometry, CFRP Airframe & L-Network Matching Controls */}
          <div
            style={{
              background: 'rgba(15, 23, 42, 0.7)',
              border: '1px solid var(--line)',
              borderRadius: '8px',
              padding: '10px 12px',
              display: 'flex',
              flexDirection: 'column',
              gap: '10px',
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: '8px',
                flexWrap: 'wrap',
              }}
            >
              <span style={{ fontSize: '12px', fontWeight: 700 }}>
                4. Parametric Drone Airframe, TPU Pod &amp; Matching Network Synthesis
              </span>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
                <label style={{ fontSize: '11px', display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
                  <span>Mounting Topology:</span>
                  <select
                    value={spec.topology}
                    data-testid="antenna-topology-select"
                    onChange={(e) =>
                      updateSpec({
                        topology: (e.target as HTMLSelectElement).value as DroneAntennaTopologyId,
                      })
                    }
                    style={{
                      fontSize: '11px',
                      padding: '3px 8px',
                      background: '#1E293B',
                      color: '#F8FAFC',
                      border: '1px solid #475569',
                      borderRadius: '4px',
                    }}
                  >
                    {DRONE_ANTENNA_TOPOLOGIES.map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  class="btn small"
                  data-testid="antenna-apply-suggested-lc"
                  onClick={handleApplySuggestedMatchOnly}
                  style={{
                    background: '#1E293B',
                    color: '#38BDF8',
                    borderColor: '#0284C7',
                  }}
                  title="Synthesise conjugate L-network (L_ser, C_sh) for current geometry"
                >
                  Apply Suggested L/C ({report.suggested_match_series_nh.toFixed(1)} nH,{' '}
                  {report.suggested_match_shunt_pf.toFixed(2)} pF)
                </button>
              </div>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
                gap: '10px 14px',
                fontSize: '11px',
              }}
            >
              {/* Slider 1: CFRP Standoff */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>CFRP Standoff d_cfrp</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>{spec.cfrp_standoff_mm.toFixed(1)} mm</strong>
                </div>
                <input
                  type="range"
                  min="4"
                  max="36"
                  step="0.5"
                  value={spec.cfrp_standoff_mm}
                  data-testid="antenna-slider-standoff"
                  onInput={(e) =>
                    updateSpec({ cfrp_standoff_mm: parseFloat((e.target as HTMLInputElement).value) })
                  }
                />
              </label>

              {/* Slider 2: Pod Elevation */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>Top-Plate Elevation z_pod</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.elevation_above_plate_mm.toFixed(1)} mm
                  </strong>
                </div>
                <input
                  type="range"
                  min="2"
                  max="28"
                  step="0.5"
                  value={spec.elevation_above_plate_mm}
                  data-testid="antenna-slider-elevation"
                  onInput={(e) =>
                    updateSpec({
                      elevation_above_plate_mm: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 3: 900 MHz Dipole Tip-to-Tip Length */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>900 MHz Dipole Length L_900</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.low_band_arm_length_mm.toFixed(1)} mm
                  </strong>
                </div>
                <input
                  type="range"
                  min="105"
                  max="175"
                  step="0.5"
                  value={spec.low_band_arm_length_mm}
                  data-testid="antenna-slider-l900"
                  onInput={(e) =>
                    updateSpec({
                      low_band_arm_length_mm: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 4: 2.4 GHz Sleeve Length */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>2.4 GHz Sleeve Length L_2400</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.high_band_arm_length_mm.toFixed(1)} mm
                  </strong>
                </div>
                <input
                  type="range"
                  min="38"
                  max="68"
                  step="0.2"
                  value={spec.high_band_arm_length_mm}
                  data-testid="antenna-slider-l2400"
                  onInput={(e) =>
                    updateSpec({
                      high_band_arm_length_mm: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 5: Gemini Diversity V-Skew Angle */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>Gemini Diversity Skew α_skew</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.diversity_skew_deg.toFixed(0)}°
                  </strong>
                </div>
                <input
                  type="range"
                  min="25"
                  max="110"
                  step="1"
                  value={spec.diversity_skew_deg}
                  data-testid="antenna-slider-skew"
                  onInput={(e) =>
                    updateSpec({
                      diversity_skew_deg: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 6: TPU Pod Wall Thickness */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>TPU/PA12 Boot Wall t_tpu</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.dielectric_pod_wall_mm.toFixed(1)} mm (ε_r={spec.pod_eps_r.toFixed(2)})
                  </strong>
                </div>
                <input
                  type="range"
                  min="0.8"
                  max="4.5"
                  step="0.1"
                  value={spec.dielectric_pod_wall_mm}
                  data-testid="antenna-slider-tpu-wall"
                  onInput={(e) =>
                    updateSpec({
                      dielectric_pod_wall_mm: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 7: Series Match L_ser */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>Feed Match Series L_ser</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.match_series_nh.toFixed(2)} nH
                  </strong>
                </div>
                <input
                  type="range"
                  min="-8"
                  max="12"
                  step="0.1"
                  value={spec.match_series_nh}
                  data-testid="antenna-slider-l-ser"
                  onInput={(e) =>
                    updateSpec({
                      match_series_nh: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>

              {/* Slider 8: Shunt Match C_sh */}
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                  <span>Feed Match Shunt C_sh</span>
                  <strong style={{ fontFamily: 'var(--mono)' }}>
                    {spec.match_shunt_pf.toFixed(2)} pF
                  </strong>
                </div>
                <input
                  type="range"
                  min="-2"
                  max="3.5"
                  step="0.05"
                  value={spec.match_shunt_pf}
                  data-testid="antenna-slider-c-sh"
                  onInput={(e) =>
                    updateSpec({
                      match_shunt_pf: parseFloat((e.target as HTMLInputElement).value),
                    })
                  }
                />
              </label>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
