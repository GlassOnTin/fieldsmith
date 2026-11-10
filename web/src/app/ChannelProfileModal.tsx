// General 1D SDF Internal Channel Flow & Convection BC Operator Modal.
// Evaluates cross-sectional area A(s), wetted perimeter P_w(s), hydraulic diameter D_h(s),
// Darcy–Weisbach + Swamee–Jain pressure march p(s), and Gnielinski Nusselt / film coefficient
// h_conv(s) directly from ANY 3D .fsm implicit geometry, then binds Convection & Pressure BCs
// onto a selected .fsm surface region.

import { useEffect, useState } from 'preact/hooks';
import { store } from '../doc/store';
import {
  analyzeFsmChannel,
  type ChannelFlowSpec,
  type ChannelProfileReport,
} from '../doc/wasm';
import { NumberInput, Seg } from './fields';
import { ui, useApp } from './ui';

interface FluidPreset {
  id: string;
  label: string;
  density_kg_m3: number;
  viscosity_pa_s: number;
  conductivity_w_m_k: number;
  cp_j_kg_k: number;
}

const FLUID_PRESETS: FluidPreset[] = [
  {
    id: 'water_25c',
    label: 'Water (25 °C)',
    density_kg_m3: 997.0,
    viscosity_pa_s: 8.9e-4,
    conductivity_w_m_k: 0.607,
    cp_j_kg_k: 4182.0,
  },
  {
    id: 'sc_h2_cryo',
    label: 'Supercritical H₂ (15 MPa, 120 K)',
    density_kg_m3: 24.5,
    viscosity_pa_s: 6.8e-6,
    conductivity_w_m_k: 0.145,
    cp_j_kg_k: 14800.0,
  },
  {
    id: 'iso_vg46_oil',
    label: 'Hydraulic Oil ISO VG 46 (40 °C)',
    density_kg_m3: 865.0,
    viscosity_pa_s: 0.04,
    conductivity_w_m_k: 0.135,
    cp_j_kg_k: 1960.0,
  },
  {
    id: 'air_300k',
    label: 'Compressed Air (5 bar, 300 K)',
    density_kg_m3: 5.8,
    viscosity_pa_s: 1.85e-5,
    conductivity_w_m_k: 0.0263,
    cp_j_kg_k: 1007.0,
  },
];

export function ChannelProfileModal() {
  useApp();
  const isOpen = Boolean(ui.channelProfileOpen);

  const [axis, setAxis] = useState<'x' | 'y' | 'z'>('z');
  const [passageMode, setPassageMode] = useState<'auto' | 'interior_void' | 'solid_core'>('auto');
  const [nodeId, setNodeId] = useState<string>('');
  const [stations, setStations] = useState<number>(16);
  const [massFlow, setMassFlow] = useState<number>(0.15);
  const [density, setDensity] = useState<number>(997.0);
  const [viscosity, setViscosity] = useState<number>(8.9e-4);
  const [conductivity, setConductivity] = useState<number>(0.607);
  const [cp, setCp] = useState<number>(4182.0);
  const [roughness, setRoughness] = useState<number>(0.015);
  const [inletPressure, setInletPressure] = useState<number>(1.0);
  const [inletTemp, setInletTemp] = useState<number>(25.0);
  const [wallTemp, setWallTemp] = useState<number>(120.0);
  const [targetRegion, setTargetRegion] = useState<string>('');

  const [report, setReport] = useState<ChannelProfileReport | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    if (!targetRegion && store.doc.regions.length > 0) {
      setTargetRegion(store.doc.regions[0].id);
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        ui.set('channelProfileOpen', false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen]);

  const buildSpec = (): Partial<ChannelFlowSpec> => ({
    axis,
    passage_mode: passageMode,
    node_id: nodeId.trim() || null,
    stations,
    mass_flow_kg_s: massFlow,
    density_kg_m3: density,
    viscosity_pa_s: viscosity,
    conductivity_w_m_k: conductivity,
    cp_j_kg_k: cp,
    roughness_mm: roughness,
    inlet_pressure_mpa: inletPressure,
    inlet_temp_c: inletTemp,
    wall_temp_c: wallTemp,
  });

  const runSlice = () => {
    try {
      const res = analyzeFsmChannel(store.doc, buildSpec());
      setReport(res.report);
      setErr(null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setReport(null);
    }
  };

  useEffect(() => {
    if (isOpen && store.doc.root.length > 0) {
      runSlice();
    }
  }, [isOpen, store.version]);

  if (!isOpen) return null;

  const applyPreset = (id: string) => {
    const p = FLUID_PRESETS.find((f) => f.id === id);
    if (!p) return;
    setDensity(p.density_kg_m3);
    setViscosity(p.viscosity_pa_s);
    setConductivity(p.conductivity_w_m_k);
    setCp(p.cp_j_kg_k);
  };

  const bindToRegion = () => {
    if (!targetRegion) {
      ui.notify('Select or create a Region in Setup (2) first', 'error');
      return;
    }
    try {
      const res = analyzeFsmChannel(store.doc, {
        ...buildSpec(),
        bind_region: targetRegion,
        pressure_case: 'Channel Pressure',
      });
      setReport(res.report);
      setErr(null);
      if (res.doc) {
        const nextConv = res.doc.convections;
        const nextLoads = res.doc.loads;
        store.transact(`Bind 1D SDF channel BCs to ${targetRegion}`, (d) => {
          d.convections = structuredClone(nextConv);
          d.loads = structuredClone(nextLoads);
        });
        ui.notify(
          `Bound station-resolved lerp(${res.report.axis}, …) Convection (mean h = ${res.report.mean_h_conv_w_m2_k.toFixed(0)} W/(m²·K), T_∞ = ${res.report.mean_bulk_temp_c.toFixed(1)} °C) & Pressure (${res.report.mean_pressure_mpa.toFixed(3)} MPa) to ${targetRegion}`,
        );
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const st = report?.stations ?? [];
  const dhMax = Math.max(1e-6, ...st.map((x) => x.hydraulic_diameter_mm));
  const hMax = Math.max(1e-6, ...st.map((x) => x.h_conv_w_m2_k));

  return (
    <div
      class="modal-backdrop"
      data-testid="channel-profile-modal"
      onClick={(e) => {
        if (e.target === e.currentTarget) ui.set('channelProfileOpen', false);
      }}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 2000,
        background: 'rgba(5, 10, 20, 0.82)',
        backdropFilter: 'blur(4px)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '12px',
      }}
    >
      <div
        class="modal-card"
        style={{
          width: 'min(920px, 96vw)',
          maxHeight: '92vh',
          background: 'var(--bg)',
          border: '1px solid var(--line)',
          borderRadius: '10px',
          boxShadow: '0 24px 60px rgba(0,0,0,0.65)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: '8px',
            padding: '10px 14px',
            background: 'var(--panel-2)',
            borderBottom: '1px solid var(--line)',
          }}
        >
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
              <strong style={{ fontSize: '14px' }}>
                🌊 1D SDF Internal Channel Flow &amp; Convection BC Operator
              </strong>
              <span
                class="sys-badge"
                title="Slices any 3D .fsm implicit geometry for cross-section A(s), wetted perimeter P_w(s), D_h(s) = 4A/P_w, Darcy–Weisbach Δp, and Gnielinski h_conv"
              >
                GEOMETRY-DRIVEN 1D OPERATOR → 3D FEA BC
              </span>
            </div>
            <div style={{ fontSize: '11px', color: 'var(--text-3)', marginTop: '2px' }}>
              Marching Square cross-section quadrature on the live 3D SDF · Swamee–Jain friction + Gnielinski Nu → binds Convection &amp; Pressure onto any .fsm region
            </div>
          </div>
          <button
            type="button"
            class="btn small"
            data-testid="channel-profile-close"
            onClick={() => ui.set('channelProfileOpen', false)}
            style={{ fontWeight: 700 }}
          >
            ✕ Close
          </button>
        </div>

        <div
          style={{
            padding: '12px 14px',
            overflowY: 'auto',
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
            gap: '14px',
          }}
        >
          {/* Left column: Controls */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', fontSize: '12px' }}>
            <div class="field-grid">
              <label class="plain">Flow Axis</label>
              <Seg<'x' | 'y' | 'z'>
                value={axis}
                options={[
                  { value: 'x', label: 'X' },
                  { value: 'y', label: 'Y' },
                  { value: 'z', label: 'Z' },
                ]}
                onChange={(v) => setAxis(v)}
                testId="channel-axis"
              />
              <label class="plain">Passage Mode</label>
              <Seg<'auto' | 'interior_void' | 'solid_core'>
                value={passageMode}
                options={[
                  { value: 'auto', label: 'Auto' },
                  { value: 'interior_void', label: 'Void Bore' },
                  { value: 'solid_core', label: 'Fluid Core' },
                ]}
                onChange={(v) => setPassageMode(v)}
                testId="channel-passage-mode"
              />
              <label class="plain" title="Optional shape node id when slicing a specific fluid core volume">
                Fluid Core Node
              </label>
              <select
                class="select"
                value={nodeId}
                data-testid="channel-node-select"
                onChange={(e) => setNodeId((e.target as HTMLSelectElement).value)}
              >
                <option value="">(Enclosed interior bore of body)</option>
                {store.doc.root.map((n) => (
                  <option key={n.id} value={n.id}>
                    {n.id} ({n.kind.type})
                  </option>
                ))}
              </select>
              <label class="plain">Fluid Preset</label>
              <select
                class="select"
                data-testid="channel-fluid-preset"
                onChange={(e) => applyPreset((e.target as HTMLSelectElement).value)}
              >
                {FLUID_PRESETS.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.label}
                  </option>
                ))}
              </select>
              <label class="plain">Stations</label>
              <NumberInput value={stations} min={3} max={64} step={1} onChange={(v) => setStations(Math.max(3, Math.round(v)))} testId="channel-stations" />
              <label class="plain">Mass Flow ṁ (kg/s)</label>
              <NumberInput value={massFlow} min={1e-5} step={0.05} onChange={setMassFlow} testId="channel-mdot" />
              <label class="plain">Density ρ (kg/m³)</label>
              <NumberInput value={density} min={0.01} step={10} onChange={setDensity} testId="channel-rho" />
              <label class="plain">Viscosity μ (Pa·s)</label>
              <NumberInput value={viscosity} min={1e-7} step={1e-4} onChange={setViscosity} testId="channel-mu" />
              <label class="plain">Conductivity k (W/(m·K))</label>
              <NumberInput value={conductivity} min={1e-4} step={0.05} onChange={setConductivity} testId="channel-k" />
              <label class="plain">Specific Heat c_p (J/(kg·K))</label>
              <NumberInput value={cp} min={10} step={100} onChange={setCp} testId="channel-cp" />
              <label class="plain">Wall Roughness ε (mm)</label>
              <NumberInput value={roughness} min={0} step={0.005} onChange={setRoughness} testId="channel-roughness" />
              <label class="plain">Inlet Pressure p_in (MPa)</label>
              <NumberInput value={inletPressure} min={0} step={0.1} onChange={setInletPressure} testId="channel-pin" />
              <label class="plain">Inlet Temp T_in (°C)</label>
              <NumberInput value={inletTemp} step={5} onChange={setInletTemp} testId="channel-tin" />
              <label class="plain">Wall Ref Temp T_w (°C)</label>
              <NumberInput value={wallTemp} step={10} onChange={setWallTemp} testId="channel-twall" />
            </div>

            <div style={{ display: 'flex', gap: '6px', marginTop: '4px' }}>
              <button
                type="button"
                class="btn small primary"
                data-testid="channel-evaluate-btn"
                onClick={runSlice}
                style={{ flex: 1 }}
              >
                Evaluate 1D SDF Channel
              </button>
            </div>

            <div
              style={{
                marginTop: '6px',
                padding: '8px 10px',
                borderRadius: '6px',
                border: '1px solid var(--line)',
                background: 'var(--panel-2)',
              }}
            >
              <div style={{ fontWeight: 600, fontSize: '11.5px', marginBottom: '6px' }}>
                Bind Evaluated Film &amp; Pressure BCs onto 3D .fsm Region
              </div>
              <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
                <select
                  class="select"
                  style={{ flex: 1 }}
                  value={targetRegion}
                  data-testid="channel-bind-region-select"
                  onChange={(e) => setTargetRegion((e.target as HTMLSelectElement).value)}
                >
                  <option value="">— Select Target Surface Region —</option>
                  {store.doc.regions.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.id}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  class="btn small primary"
                  data-testid="channel-bind-bcs-btn"
                  disabled={!targetRegion}
                  onClick={bindToRegion}
                >
                  Bind BCs to Region
                </button>
              </div>
            </div>
          </div>

          {/* Right column: Measured SDF Readout + Profile */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '10px', fontSize: '12px' }}>
            {err && (
              <div class="error-box" data-testid="channel-error">
                {err}
              </div>
            )}
            {report && (
              <>
                <div
                  data-testid="channel-summary-grid"
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '1fr 1fr',
                    gap: '6px',
                    padding: '8px 10px',
                    borderRadius: '6px',
                    background: 'var(--panel-2)',
                    border: '1px solid var(--line)',
                    fontFamily: 'var(--mono)',
                    fontSize: '11.5px',
                  }}
                >
                  <div>
                    Mean ⌀_h: <strong>{report.mean_hydraulic_diameter_mm.toFixed(3)} mm</strong>
                  </div>
                  <div>
                    Area Range: <strong>{report.min_area_mm2.toFixed(1)}–{report.max_area_mm2.toFixed(1)} mm²</strong>
                  </div>
                  <div>
                    Viscous Δp: <strong>{report.viscous_pressure_drop_mpa.toFixed(4)} MPa</strong>
                  </div>
                  <div>
                    Outlet p_out: <strong>{report.outlet_pressure_mpa.toFixed(4)} MPa</strong>
                  </div>
                  <div>
                    Mean h_conv: <strong>{report.mean_h_conv_w_m2_k.toFixed(1)} W/(m²·K)</strong>
                  </div>
                  <div>
                    Outlet T_b: <strong>{report.outlet_temp_c.toFixed(2)} °C</strong>
                  </div>
                </div>

                {st.length >= 2 && (
                  <svg
                    viewBox="0 0 420 150"
                    style={{
                      width: '100%',
                      height: '150px',
                      background: 'var(--panel-2)',
                      border: '1px solid var(--line)',
                      borderRadius: '6px',
                    }}
                  >
                    <polyline
                      fill="none"
                      stroke="#22d3ee"
                      stroke-width="2"
                      points={st
                        .map((p, i) => {
                          const x = 32 + (i / Math.max(1, st.length - 1)) * 372;
                          const y = 128 - (p.hydraulic_diameter_mm / dhMax) * 96;
                          return `${x.toFixed(1)},${y.toFixed(1)}`;
                        })
                        .join(' ')}
                    />
                    <polyline
                      fill="none"
                      stroke="#f59e0b"
                      stroke-width="1.8"
                      stroke-dasharray="4 2"
                      points={st
                        .map((p, i) => {
                          const x = 32 + (i / Math.max(1, st.length - 1)) * 372;
                          const y = 128 - (p.h_conv_w_m2_k / hMax) * 96;
                          return `${x.toFixed(1)},${y.toFixed(1)}`;
                        })
                        .join(' ')}
                    />
                    <text x="36" y="18" fill="currentColor" font-size="10" opacity="0.85">
                      Cyan: ⌀_h(s) [max {dhMax.toFixed(2)} mm] · Dashed Amber: h_conv(s) [max {hMax.toFixed(0)} W/(m²·K)]
                    </text>
                  </svg>
                )}

                <div style={{ maxHeight: '220px', overflowY: 'auto', border: '1px solid var(--line)', borderRadius: '6px' }}>
                  <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '11px', fontFamily: 'var(--mono)' }}>
                    <thead>
                      <tr style={{ background: 'var(--panel-2)', textAlign: 'right' }}>
                        <th style={{ padding: '4px 6px', textAlign: 'left' }}>s (mm)</th>
                        <th style={{ padding: '4px 6px' }}>A (mm²)</th>
                        <th style={{ padding: '4px 6px' }}>⌀_h (mm)</th>
                        <th style={{ padding: '4px 6px' }}>Re</th>
                        <th style={{ padding: '4px 6px' }}>h (W/m²K)</th>
                        <th style={{ padding: '4px 6px' }}>p (MPa)</th>
                      </tr>
                    </thead>
                    <tbody>
                      {st.map((row, idx) => (
                        <tr key={idx} style={{ borderTop: '1px solid var(--line)', textAlign: 'right' }}>
                          <td style={{ padding: '3px 6px', textAlign: 'left' }}>{row.s_mm.toFixed(1)}</td>
                          <td style={{ padding: '3px 6px' }}>{row.area_mm2.toFixed(2)}</td>
                          <td style={{ padding: '3px 6px' }}>{row.hydraulic_diameter_mm.toFixed(2)}</td>
                          <td style={{ padding: '3px 6px' }}>{row.reynolds.toFixed(0)}</td>
                          <td style={{ padding: '3px 6px' }}>{row.h_conv_w_m2_k.toFixed(0)}</td>
                          <td style={{ padding: '3px 6px' }}>{row.static_pressure_mpa.toFixed(4)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
