import { useEffect, useState } from 'preact/hooks';
import {
  runGerrisTestSuiteWasm as runGerrisTestSuiteWasmRaw,
  runSpuriousEddyBenchmarkWasm,
  solveFreeSurfaceSphWasm,
  type GerrisTestSuiteReport,
  type PeregrineAirEntrainmentReport as WasmPeregrineAirEntrainmentReport,
  type SphFluidSpec as WasmSphFluidSpec,
  type SphSimulationReport,
  type SpoutLipPourReport as WasmSpoutLipPourReport,
  type SpuriousEddyBenchmarkReport,
  type SpuriousEddyBenchmarkSpec,
} from '../doc/wasm';
import { applyAddrs, parseAddr } from '../doc/param';
import { store } from '../doc/store';
import { openText, viewApi } from './commands';
import { openCoordMappingModal } from './CoordMappingModal';
import { ui, useApp } from './ui';

export type SphFluidSpec = WasmSphFluidSpec;

export type SpoutLipPourReport = WasmSpoutLipPourReport;

export type PeregrineAirEntrainmentReport = Omit<
  WasmPeregrineAirEntrainmentReport,
  'wall_impulse_profile' | 'air_pocket_trace'
> & {
  wall_impulse_profile: Array<{
    z_over_h: number;
    impulse_pa_s: number;
    seabed_x_over_h: number;
    seabed_impulse_pa_s: number;
  }>;
  air_pocket_trace: Array<{
    t_ms: number;
    p_abs_kpa: number;
    p_gauge_kpa: number;
    xi_over_delta0: number;
  }>;
};

export type GerrisSuiteSummary = GerrisTestSuiteReport & {
  grid_n?: number;
  laplace_number?: number;
};

const runGerrisTestSuiteWasm = runGerrisTestSuiteWasmRaw as (
  gridN?: number,
  laplaceNumber?: number,
) => GerrisSuiteSummary;

export const PRESET_SPECS: Record<string, SphFluidSpec> = {
  spout_lip_knurl_1100c: {
    scenario: 'spout_lip_knurl_1100c',
    auto_switch_regime: true,
    density_kg_m3: 2350.0,
    viscosity_pa_s: 795.0,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 0.344,
    dgamma_dt_n_m_k: -7.0e-5,
    contact_angle_deg: 135.0,
    characteristic_radius_mm: 12.0,
    droplet_aspect_perturbation: 0.12,
    tpms_lattice_kind: 'none',
    tpms_period_mm: 4.0,
    tpms_solid_fraction: 0.3,
    thermal_gradient_delta_k: 65.0,
    gravity_m_s2: 9.80665,
    reservoir_height_mm: 38.0,
    steps: 60,
  },
  peregrine_wave_air_entrainment: {
    scenario: 'peregrine_wave_air_entrainment',
    auto_switch_regime: true,
    density_kg_m3: 1025.0,
    viscosity_pa_s: 1.08e-3,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 0.0735,
    dgamma_dt_n_m_k: -1.5e-4,
    contact_angle_deg: 25.0,
    characteristic_radius_mm: 4.5,
    droplet_aspect_perturbation: 0.22,
    tpms_lattice_kind: 'gyroid',
    tpms_period_mm: 5.0,
    tpms_solid_fraction: 0.28,
    thermal_gradient_delta_k: 12.0,
    gravity_m_s2: 9.80665,
    reservoir_height_mm: 180.0,
    steps: 60,
  },
  droplet_oscillation_a15: {
    scenario: 'droplet_oscillation_a15',
    auto_switch_regime: true,
    density_kg_m3: 998.0,
    viscosity_pa_s: 1.0e-3,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 0.0728,
    dgamma_dt_n_m_k: -1.5e-4,
    contact_angle_deg: 28.0,
    characteristic_radius_mm: 2.0,
    droplet_aspect_perturbation: 0.15,
    tpms_lattice_kind: 'gyroid',
    tpms_period_mm: 4.0,
    tpms_solid_fraction: 0.3,
    thermal_gradient_delta_k: 40.0,
    gravity_m_s2: 0.0,
    reservoir_height_mm: 45.0,
    steps: 60,
  },
  capillary_rise_a16: {
    scenario: 'capillary_rise_a16',
    auto_switch_regime: true,
    density_kg_m3: 998.0,
    viscosity_pa_s: 1.0e-3,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 0.0728,
    dgamma_dt_n_m_k: -1.5e-4,
    contact_angle_deg: 25.0,
    characteristic_radius_mm: 0.8,
    droplet_aspect_perturbation: 0.15,
    tpms_lattice_kind: 'gyroid',
    tpms_period_mm: 3.5,
    tpms_solid_fraction: 0.32,
    thermal_gradient_delta_k: 40.0,
    gravity_m_s2: 9.80665,
    reservoir_height_mm: 45.0,
    steps: 60,
  },
  marangoni_melt_pool: {
    scenario: 'marangoni_melt_pool',
    auto_switch_regime: true,
    density_kg_m3: 7000.0,
    viscosity_pa_s: 4.5e-3,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 1.72,
    dgamma_dt_n_m_k: -4.3e-4,
    contact_angle_deg: 35.0,
    characteristic_radius_mm: 3.0,
    droplet_aspect_perturbation: 0.12,
    tpms_lattice_kind: 'gyroid',
    tpms_period_mm: 4.0,
    tpms_solid_fraction: 0.3,
    thermal_gradient_delta_k: 180.0,
    gravity_m_s2: 9.80665,
    reservoir_height_mm: 45.0,
    steps: 60,
  },
  dambreak_fsi_t18: {
    scenario: 'dambreak_fsi_t18',
    auto_switch_regime: true,
    density_kg_m3: 998.0,
    viscosity_pa_s: 1.0e-3,
    gas_density_kg_m3: 1.225,
    gas_viscosity_pa_s: 1.81e-5,
    surface_tension_n_m: 0.0728,
    dgamma_dt_n_m_k: -1.5e-4,
    contact_angle_deg: 32.0,
    characteristic_radius_mm: 1.2,
    droplet_aspect_perturbation: 0.15,
    tpms_lattice_kind: 'gyroid',
    tpms_period_mm: 4.0,
    tpms_solid_fraction: 0.3,
    thermal_gradient_delta_k: 45.0,
    gravity_m_s2: 9.80665,
    reservoir_height_mm: 50.0,
    steps: 60,
  },
};

function regimeColor(code: string): string {
  switch (code) {
    case 'implicit_stokes_visco_capillary':
      return '#f59e0b';
    case 'peregrine_potential':
      return '#38bdf8';
    case 'inertial_dfsph':
      return '#6366f1';
    case 'capillary_mls_sph':
    default:
      return '#10b981';
  }
}

export function openFreeSurfaceModal(initialPreset?: string) {
  if (initialPreset === 'spurious_eddies') {
    freeSurfaceShared.showSpuriousEddies = true;
    freeSurfaceShared.showGerris = false;
  } else if (initialPreset === 'gerris_suite') {
    freeSurfaceShared.showGerris = true;
    freeSurfaceShared.showSpuriousEddies = false;
  } else if (initialPreset && PRESET_SPECS[initialPreset]) {
    freeSurfaceShared.spec = { ...PRESET_SPECS[initialPreset] };
    freeSurfaceShared.showGerris = false;
    freeSurfaceShared.showSpuriousEddies = false;
  }
  ui.set('freeSurfaceOpen' as any, true);
}

export const freeSurfaceShared: {
  spec: SphFluidSpec;
  showGerris?: boolean;
  showSpuriousEddies?: boolean;
  spuriousSpec?: SpuriousEddyBenchmarkSpec;
} = {
  spec: { ...PRESET_SPECS.spout_lip_knurl_1100c },
  showGerris: false,
  showSpuriousEddies: false,
  spuriousSpec: {
    droplet_radius_mm: 2.0,
    resolution_d_over_dx: 24,
    laplace_number: 12000,
    density_kg_m3: 998.0,
    surface_tension_n_m: 0.0728,
    stencil_anisotropy: 0.15,
    active_scheme: 'monge_parabolic_mls',
  },
};

export function loadA20DropletVesselToViewport(spec?: SpuriousEddyBenchmarkSpec) {
  const s = spec ?? freeSurfaceShared.spuriousSpec ?? {
    droplet_radius_mm: 2.0,
    resolution_d_over_dx: 24,
    laplace_number: 12000,
    density_kg_m3: 998.0,
    surface_tension_n_m: 0.0728,
    stencil_anisotropy: 0.15,
    active_scheme: 'monge_parabolic_mls',
  };
  const r0 = Math.max(0.5, s.droplet_radius_mm);
  const resN = Math.max(8, Math.round(s.resolution_d_over_dx));
  const la = Math.max(100, Math.round(s.laplace_number));
  const anis = Number(s.stencil_anisotropy.toFixed(2));
  const bench = runSpuriousEddyBenchmarkWasm(s);
  const active =
    bench.schemes.find((sc) => sc.scheme_code === bench.spec.active_scheme) ?? bench.schemes[2];
  const curvErrPct =
    (active as { curvature_l2_error_pct?: number }).curvature_l2_error_pct ??
    active.rms_curvature_noise_dimless * 100 + Math.abs(active.mean_curvature_bias_pct);
  const effP = Number(
    ((bench.exact_laplace_pressure_pa * 1e-3) * (1 + curvErrPct * 0.01)).toFixed(4),
  );
  const fsmText = [
    'fieldsmith 1',
    'name "A20 — Spurious Capillary Eddies & Surface-Tension Geometry Approximation Errors (V65)"',
    'material ti64',
    '',
    `let droplet_r_mm = ${r0}`,
    `let resolution_d_dx = ${resN}`,
    `let laplace_la = ${la}`,
    `let stencil_anisotropy = ${anis}`,
    '',
    'sphere droplet_shell r=12 at=(0, 0, 12)',
    'sphere droplet_cavity r=9.2 at=(0, 0, 12) subtract',
    'sphere optical_trap_core r=1.6 at=(0, 0, 0) subtract',
    '',
    'region cell_base = optical_trap_core.all',
    'region droplet_interface = droplet_cavity.all',
    '',
    'support clamp cell_base fixed=xyz',
    `load laplace_jump droplet_interface pressure=${effP}`,
    'solve',
    '',
  ].join('\n');
  openText(fsmText);
}

export function SpuriousEddyExplorerPanel({
  compact = false,
}: {
  compact?: boolean;
}) {
  useApp();
  const doc = store.doc;
  const getLet = (name: string, fallback: number): number => {
    const found = (doc.lets ?? []).find((l) => l.name === name);
    return found && Number.isFinite(found.value) ? found.value : fallback;
  };

  const [localSpec, setLocalSpec] = useState<SpuriousEddyBenchmarkSpec>(() => {
    const base = freeSurfaceShared.spuriousSpec ?? {
      droplet_radius_mm: 2.0,
      resolution_d_over_dx: 24,
      laplace_number: 12000,
      density_kg_m3: 998.0,
      surface_tension_n_m: 0.0728,
      stencil_anisotropy: 0.15,
      active_scheme: 'monge_parabolic_mls',
    };
    const init: SpuriousEddyBenchmarkSpec = {
      ...base,
      droplet_radius_mm: getLet('droplet_r_mm', base.droplet_radius_mm),
      resolution_d_over_dx: Math.round(getLet('resolution_d_dx', base.resolution_d_over_dx)),
      laplace_number: getLet('laplace_la', base.laplace_number),
      surface_tension_n_m: getLet('gamma_n_m', base.surface_tension_n_m),
      stencil_anisotropy: getLet('stencil_anisotropy', base.stencil_anisotropy),
    };
    freeSurfaceShared.spuriousSpec = init;
    return init;
  });

  const docLetsSig = (doc.lets ?? []).map((l) => `${l.name}:${l.value}`).join('|');
  useEffect(() => {
    const hasSpuriousLets = (store.doc.lets ?? []).some((l) =>
      ['droplet_r_mm', 'resolution_d_dx', 'laplace_la', 'gamma_n_m', 'stencil_anisotropy'].includes(l.name),
    );
    if (!hasSpuriousLets) return;
    setLocalSpec((prev) => {
      const next: SpuriousEddyBenchmarkSpec = {
        ...prev,
        droplet_radius_mm: getLet('droplet_r_mm', prev.droplet_radius_mm),
        resolution_d_over_dx: Math.round(getLet('resolution_d_dx', prev.resolution_d_over_dx)),
        laplace_number: getLet('laplace_la', prev.laplace_number),
        surface_tension_n_m: getLet('gamma_n_m', prev.surface_tension_n_m),
        stencil_anisotropy: getLet('stencil_anisotropy', prev.stencil_anisotropy),
      };
      freeSurfaceShared.spuriousSpec = next;
      return next;
    });
  }, [docLetsSig, store.loads]);

  const updateParam = (patch: Partial<SpuriousEddyBenchmarkSpec>, letName?: string, letVal?: number) => {
    const next = { ...localSpec, ...patch };
    freeSurfaceShared.spuriousSpec = next;
    setLocalSpec(next);
    if (letName && letVal !== undefined) {
      const hasLet = (store.doc.lets ?? []).some((l) => l.name === letName);
      const hasLaplaceLoad = (store.doc.loads ?? []).some(
        (l) => l.id === 'laplace_jump' && l.kind.type === 'pressure',
      );
      if (hasLet || hasLaplaceLoad) {
        try {
          store.transact(`Spurious eddy ${letName}`, (d) => {
            const pairs: [ReturnType<typeof parseAddr>, number][] = [];
            if ((d.lets ?? []).some((l) => l.name === letName)) {
              pairs.push([parseAddr(letName), letVal]);
            }
            if ((d.loads ?? []).some((l) => l.id === 'laplace_jump' && l.kind.type === 'pressure')) {
              const bench = runSpuriousEddyBenchmarkWasm(next);
              const active =
                bench.schemes.find((sc) => sc.scheme_code === bench.spec.active_scheme) ??
                bench.schemes[2];
              const curvErrPct =
                (active as { curvature_l2_error_pct?: number }).curvature_l2_error_pct ??
                active.rms_curvature_noise_dimless * 100 + Math.abs(active.mean_curvature_bias_pct);
              const effP = (bench.exact_laplace_pressure_pa * 1e-3) * (1 + curvErrPct * 0.01);
              pairs.push([parseAddr('laplace_jump.mpa'), Number(effP.toFixed(4))]);
            }
            if (pairs.length > 0) {
              applyAddrs(d, pairs, true);
            }
          });
        } catch {
          // ignore if let not present
        }
      }
    }
  };

  let rep: SpuriousEddyBenchmarkReport;
  try {
    rep = runSpuriousEddyBenchmarkWasm(localSpec);
  } catch (e) {
    return (
      <div class="tut-step-card" style={{ borderColor: 'var(--danger)' }}>
        <strong>Spurious Eddy Benchmark Error:</strong> {String(e)}
      </div>
    );
  }

  const activeScheme =
    rep.schemes.find((s) => s.scheme_code === rep.spec.active_scheme) ?? rep.schemes[2];

  const schemeColors: Record<string, string> = {
    voxel_staircase_csf: '#ef4444',
    kernel_color_csf: '#f97316',
    monge_parabolic_mls: '#eab308',
    height_function_vof: '#38bdf8',
    pratt_quadric_mls: '#10b981',
  };

  // Build exaggerated droplet curvature perimeter path in 2D SVG (center 135, 135, R_svg = 76)
  // scaled directly from the solver's curvature error & parasitic capillary number:
  const cx = 135;
  const cy = 135;
  const rSvg = 76;
  const sigmaAt8 = Math.max(1e-12, activeScheme.convergence_samples[0]?.[1] ?? activeScheme.rms_curvature_noise_dimless);
  const caAt8 = Math.max(1e-15, activeScheme.convergence_samples[0]?.[2] ?? activeScheme.parasitic_capillary_number);
  const activeCaMax =
    (activeScheme as { parasitic_ca_max?: number }).parasitic_ca_max ??
    activeScheme.parasitic_capillary_number;
  const resCaRatio = Math.max(0.0, Math.min(2.0, activeCaMax / caAt8));
  const curvL2Pct =
    (activeScheme as { curvature_l2_error_pct?: number }).curvature_l2_error_pct ??
    activeScheme.rms_curvature_noise_dimless * 100 + Math.abs(activeScheme.mean_curvature_bias_pct);
  const eddyVisualGain = Math.max(
    0.08,
    Math.min(1.15, Math.pow(resCaRatio, 0.28) * Math.min(1.2, 0.25 + 0.22 * Math.log10(1 + curvL2Pct * 1e4))),
  );
  const rippleAmp = Math.max(0.005, Math.min(0.22, 0.045 * Math.pow(curvL2Pct, 0.42)));

  const distortedPts = activeScheme.azimuthal_profile
    .map(([deg, r0Kappa]) => {
      const rad = (deg * Math.PI) / 180;
      const deltaK = (r0Kappa - 2.0) / (2.2 * sigmaAt8);
      const rMod = rSvg * (1 + Math.max(-0.22, Math.min(0.22, deltaK * rippleAmp)));
      const x = cx + rMod * Math.cos(rad);
      const y = cy - rMod * Math.sin(rad);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');

  return (
    <div
      data-testid="spurious-eddy-explorer"
      style={{
        padding: compact ? '9px 10px' : '11px 13px',
        borderRadius: '8px',
        background: 'color-mix(in srgb, #a855f7 9%, var(--panel))',
        border: '1px solid rgba(168, 85, 247, 0.45)',
        display: 'flex',
        flexDirection: 'column',
        gap: '9px',
        fontSize: '11.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
        <div>
          <strong style={{ fontSize: '12.5px', color: '#c084fc' }}>
            🌀 Numerical Noise Eddies (Spurious / Parasitic Currents) vs. Surface-Tension Geometry Approximation
          </strong>
          <div style={{ fontSize: '10.5px', color: 'var(--text-2)' }}>
            Stationary 3D Droplet (<span class="num">u_exact ≡ 0, Δp = 2γ/R₀ = {rep.exact_laplace_pressure_pa.toFixed(2)} Pa, La = {rep.spec.laplace_number.toFixed(0)}, Oh = {rep.ohnesorge_number.toFixed(4)}</span>) — Interfacial curl <span class="num">∇×(γκ_h n̂ δ_s) ≠ 0</span> drives 8 quadrupolar noise eddies.
          </div>
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
          <button
            type="button"
            class="btn small"
            data-testid="spurious-sync-3d-droplet-btn"
            onClick={() => loadA20DropletVesselToViewport(rep.spec)}
            style={{ borderColor: '#38bdf8', color: '#7dd3fc', fontSize: '10.5px', padding: '3px 7px' }}
            title="Load the A20 3D Spherical Droplet Containment Vessel into the 3D viewport (replaces any active cuboid/cylinder tutorial model)"
          >
            🔵 Sync 3D Droplet Vessel in Viewport
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="spurious-open-coord-fem-btn"
            onClick={() =>
              openCoordMappingModal({
                dimension: 2,
                coordSystem: 'polar_cylindrical',
                pdeKind: 'biharmonic_eddy_stream',
                gridN: rep.spec.resolution_d_over_dx,
                q1Max: rep.spec.droplet_radius_mm,
              })
            }
            style={{ borderColor: '#22d3ee', color: '#22d3ee', fontSize: '10.5px', padding: '3px 7px' }}
            title="Open the 1D–4D Coordinate Mappings & Pullback FEM Workbench in 2D Polar (r, θ) Biharmonic Eddy mode"
          >
            🌐 1D–4D Coordinate FEM Suite →
          </button>
          <span
            class="num"
            data-testid="spurious-eddy-suppression-badge"
            style={{
              padding: '3px 8px',
              borderRadius: '4px',
              fontWeight: 700,
              fontSize: '10.5px',
              background: 'rgba(16, 185, 129, 0.18)',
              color: '#34d399',
              border: '1px solid rgba(16, 185, 129, 0.45)',
            }}
          >
            Pratt vs Parabolic MLS: {rep.parabolic_to_pratt_suppression_ratio.toExponential(2)}× Eddy Suppression
          </span>
        </div>
      </div>

      {/* 5-Scheme Selector Buttons */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px' }}>
        {rep.schemes.map((s) => {
          const isSel = s.scheme_code === activeScheme.scheme_code;
          const col = schemeColors[s.scheme_code] ?? '#a855f7';
          return (
            <button
              key={s.scheme_code}
              type="button"
              data-testid={`spurious-scheme-btn-${s.scheme_code}`}
              onClick={() => updateParam({ active_scheme: s.scheme_code })}
              style={{
                padding: '5px 8px',
                borderRadius: '5px',
                fontSize: '10.5px',
                fontWeight: isSel ? 700 : 500,
                cursor: 'pointer',
                background: isSel ? `color-mix(in srgb, ${col} 22%, var(--bg))` : 'var(--bg)',
                border: isSel ? `1.5px solid ${col}` : '1px solid var(--line)',
                color: 'var(--text)',
                display: 'flex',
                alignItems: 'center',
                gap: '5px',
              }}
            >
              <span
                style={{
                  width: '8px',
                  height: '8px',
                  borderRadius: '50%',
                  background: col,
                  display: 'inline-block',
                }}
              />
              <span>{s.scheme_title.replace(/^\d+\.\s*/, '')}</span>
              <span class="num" style={{ color: col, fontWeight: 700 }}>
                Ca={s.parasitic_capillary_number.toExponential(1)}
              </span>
            </button>
          );
        })}
      </div>

      {/* Interactive Sliders */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(165px, 1fr))',
          gap: '8px',
          padding: '7px 9px',
          borderRadius: '6px',
          background: 'var(--bg)',
          border: '1px solid var(--line)',
        }}
      >
        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Grid Resolution D / Δx</span>
            <strong class="num" data-testid="spurious-res-val">{rep.spec.resolution_d_over_dx} cells/D</strong>
          </div>
          <input
            type="range"
            min="8"
            max="64"
            step="4"
            value={rep.spec.resolution_d_over_dx}
            data-testid="spurious-res-slider"
            onInput={(e) => {
              const v = Number((e.target as HTMLInputElement).value);
              updateParam({ resolution_d_over_dx: v }, 'resolution_d_dx', v);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Laplace Number La = ργD/μ²</span>
            <strong class="num" data-testid="spurious-la-val">{rep.spec.laplace_number.toFixed(0)}</strong>
          </div>
          <input
            type="range"
            min="200"
            max="60000"
            step="200"
            value={rep.spec.laplace_number}
            data-testid="spurious-la-slider"
            onInput={(e) => {
              const v = Number((e.target as HTMLInputElement).value);
              updateParam({ laplace_number: v }, 'laplace_la', v);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Stencil Anisotropy η_grid</span>
            <strong class="num" data-testid="spurious-anis-val">{rep.spec.stencil_anisotropy.toFixed(2)}</strong>
          </div>
          <input
            type="range"
            min="0.02"
            max="0.35"
            step="0.01"
            value={rep.spec.stencil_anisotropy}
            data-testid="spurious-anis-slider"
            onInput={(e) => {
              const v = Number((e.target as HTMLInputElement).value);
              updateParam({ stencil_anisotropy: v }, 'stencil_anisotropy', v);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Droplet Radius R₀</span>
            <strong class="num">{rep.spec.droplet_radius_mm.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="0.5"
            max="6.0"
            step="0.5"
            value={rep.spec.droplet_radius_mm}
            onInput={(e) => {
              const v = Number((e.target as HTMLInputElement).value);
              updateParam({ droplet_radius_mm: v }, 'droplet_r_mm', v);
            }}
          />
        </label>
      </div>

      {/* Active Scheme Telemetry Cards */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(135px, 1fr))',
          gap: '6px',
        }}
      >
        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>RMS Curvature Noise σ*_κ</div>
          <div class="num" data-testid="spurious-active-sigma-kappa" style={{ fontSize: '12.5px', fontWeight: 700, color: schemeColors[activeScheme.scheme_code] }}>
            {activeScheme.rms_curvature_noise_dimless.toExponential(3)}
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            Bias: {activeScheme.mean_curvature_bias_pct.toFixed(4)}% · p = {activeScheme.measured_convergence_order.toFixed(2)}
          </div>
        </div>

        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Parasitic Capillary Ca_max</div>
          <div class="num" data-testid="spurious-active-ca-max" style={{ fontSize: '12.5px', fontWeight: 700, color: schemeColors[activeScheme.scheme_code] }}>
            {activeScheme.parasitic_capillary_number.toExponential(3)}
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            μ ‖u_eddy‖_∞ / γ (Exact = 0)
          </div>
        </div>

        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Peak Noise Eddy Speed U_max</div>
          <div class="num" data-testid="spurious-active-umax" style={{ fontSize: '12.5px', fontWeight: 700 }}>
            {activeScheme.max_eddy_velocity_m_s >= 1e-2
              ? `${activeScheme.max_eddy_velocity_m_s.toFixed(3)} m/s`
              : activeScheme.max_eddy_velocity_m_s >= 1e-5
                ? `${(activeScheme.max_eddy_velocity_m_s * 1e3).toFixed(3)} mm/s`
                : `${(activeScheme.max_eddy_velocity_m_s * 1e9).toFixed(2)} nm/s`}
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            Re_eddy = {activeScheme.parasitic_reynolds_number.toExponential(2)}
          </div>
        </div>

        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Eddy Vorticity &amp; Energy</div>
          <div class="num" style={{ fontSize: '12.5px', fontWeight: 700 }}>
            ω_max = {activeScheme.max_eddy_vorticity_inv_s.toExponential(2)} s⁻¹
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            E_eddy = {activeScheme.eddy_kinetic_energy_j.toExponential(2)} J ({activeScheme.eddy_vortex_cell_count} cells)
          </div>
        </div>
      </div>

      {/* Dual Visual Plots: Left = 8-Eddy Equatorial Vorticity & Velocity Field; Right = Log-Log Convergence */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: compact ? '1fr' : 'repeat(auto-fit, minmax(265px, 1fr))',
          gap: '8px',
        }}
      >
        {/* Left: 2D Equatorial Cross-Section of the 8 Quadrupolar Numerical Noise Eddies */}
        <div
          style={{
            padding: '7px 9px',
            borderRadius: '6px',
            background: 'var(--bg)',
            border: '1px solid var(--line)',
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <strong style={{ fontSize: '11px' }}>
              8-Vortex Parasitic Eddy Field u_eddy(x,y) &amp; Vorticity ω_z
            </strong>
            <span class="num" style={{ fontSize: '10px', color: schemeColors[activeScheme.scheme_code] }}>
              {activeScheme.scheme_code}
            </span>
          </div>
          <svg
            viewBox="0 0 270 270"
            data-testid="spurious-eddy-vorticity-svg"
            style={{
              width: '100%',
              maxWidth: '290px',
              margin: '0 auto',
              height: 'auto',
              background: '#090d16',
              borderRadius: '6px',
              border: '1px solid var(--line)',
            }}
          >
            <defs>
              <marker id="eddy-arrow" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="4" markerHeight="4" orient="auto-start-reverse">
                <path d="M 0 1 L 10 5 L 0 9 z" fill="#e2e8f0" />
              </marker>
            </defs>

            {/* Discrete Cartesian cut-cell grid overlay for D / Δx */}
            {(() => {
              const resN = Math.max(8, Math.min(64, rep.spec.resolution_d_over_dx));
              const visCells = Math.min(32, resN);
              const cellPx = (2 * rSvg) / visCells;
              const halfSpan = Math.floor(visCells / 2) + 1;
              const lines = [];
              for (let i = -halfSpan; i <= halfSpan; i++) {
                const gxLine = cx + i * cellPx;
                const gyLine = cy + i * cellPx;
                lines.push(
                  <line
                    key={`gx-${i}`}
                    x1={gxLine}
                    y1={cy - rSvg - 16}
                    x2={gxLine}
                    y2={cy + rSvg + 16}
                    stroke="#1e293b"
                    strokeWidth="0.65"
                    strokeOpacity="0.65"
                  />,
                  <line
                    key={`gy-${i}`}
                    x1={cx - rSvg - 16}
                    y1={gyLine}
                    x2={cx + rSvg + 16}
                    y2={gyLine}
                    stroke="#1e293b"
                    strokeWidth="0.65"
                    strokeOpacity="0.65"
                  />,
                );
              }
              return <g data-testid="spurious-eddy-grid-overlay">{lines}</g>;
            })()}

            {/* Render 13×13 vorticity & velocity vector cells scaled consistently from velocity_field & parasitic_ca_max */}
            {(
              (activeScheme as { velocity_field?: [number, number, number, number, number][] }).velocity_field ??
              rep.active_eddy_field_grid
            ).map(([xn, yn, ux, uy, vort], idx) => {
              const gx = cx + (xn / 1.35) * 116;
              const gy = cy - (yn / 1.35) * 116;
              const omegaMax = Math.max(1e-30, activeScheme.max_eddy_vorticity_inv_s);
              const vNorm = Math.max(-1, Math.min(1, vort / (0.45 * omegaMax)));
              const caScale = Math.max(0.12, Math.min(1.2, 0.18 + 0.16 * Math.log10(1 + activeCaMax * 1e8)));
              const alpha = Math.min(0.62, Math.abs(vNorm) * 0.55 * eddyVisualGain * caScale);
              const fillCol = vNorm >= 0 ? `rgba(244, 63, 94, ${alpha.toFixed(2)})` : `rgba(56, 189, 248, ${alpha.toFixed(2)})`;

              const uMax = Math.max(1e-30, activeScheme.max_eddy_velocity_m_s);
              const uxNorm = ux / uMax;
              const uyNorm = uy / uMax;
              const mag = Math.hypot(uxNorm, uyNorm);
              const arrowScale = (4.5 + 11.5 * eddyVisualGain) * caScale;
              const x2 = gx + uxNorm * arrowScale;
              const y2 = gy - uyNorm * arrowScale;

              return (
                <g key={idx}>
                  <rect x={gx - 9} y={gy - 9} width="18" height="18" fill={fillCol} rx="2" />
                  {mag > 0.08 && (
                    <line
                      x1={gx}
                      y1={gy}
                      x2={x2}
                      y2={y2}
                      stroke="#f8fafc"
                      strokeWidth="1.15"
                      strokeOpacity={Math.min(0.95, (0.25 + mag) * Math.max(0.35, eddyVisualGain))}
                      markerEnd="url(#eddy-arrow)"
                    />
                  )}
                </g>
              );
            })}

            {/* Coordinate axes & 45° diagonal symmetry lines */}
            <line x1="15" y1={cy} x2="255" y2={cy} stroke="#334155" strokeWidth="0.8" strokeDasharray="3,3" />
            <line x1={cx} y1="15" x2={cx} y2="255" stroke="#334155" strokeWidth="0.8" strokeDasharray="3,3" />
            <line x1="35" y1="35" x2="235" y2="235" stroke="#1e293b" strokeWidth="0.8" strokeDasharray="2,4" />
            <line x1="35" y1="235" x2="235" y2="35" stroke="#1e293b" strokeWidth="0.8" strokeDasharray="2,4" />

            {/* Exact sphere circle r = R₀ */}
            <circle
              cx={cx}
              cy={cy}
              r={rSvg}
              fill="none"
              stroke="#38bdf8"
              strokeWidth="1.4"
              strokeDasharray="4,3"
            />

            {/* Distorted discrete curvature contour κ_h(θ) */}
            <polygon
              points={distortedPts}
              fill="none"
              stroke={schemeColors[activeScheme.scheme_code]}
              strokeWidth="2.1"
            />

            <text x="8" y="14" fill="#f43f5e" fontSize="8.5" fontFamily="monospace" fontWeight="bold">
              ■ +ω_z (CCW Eddy)
            </text>
            <text x="102" y="14" fill="#38bdf8" fontSize="8.5" fontFamily="monospace" fontWeight="bold">
              ■ −ω_z (CW Eddy)
            </text>
            <text x="190" y="14" fill={schemeColors[activeScheme.scheme_code]} fontSize="8.5" fontFamily="monospace" fontWeight="bold">
              — κ_h(θ) Ripple
            </text>
            <text x="8" y="252" fill="#38bdf8" fontSize="8.5" fontFamily="monospace" fontWeight="bold">
              D/Δx = {rep.spec.resolution_d_over_dx} (Δx = {((2 * rep.spec.droplet_radius_mm) / rep.spec.resolution_d_over_dx).toFixed(3)} mm) · Ca_max = {activeScheme.parasitic_capillary_number.toExponential(2)}
            </text>
            <text x="8" y="263" fill="#cbd5e1" fontSize="8.5" fontFamily="monospace">
              Inflow at 0°,90° axes · Outflow jets at ±45° diagonals (m=4)
            </text>
          </svg>
        </div>

        {/* Right: Log-Log Convergence of Parasitic Capillary Number Ca_max vs Grid Resolution D/Δx */}
        <div
          style={{
            padding: '7px 9px',
            borderRadius: '6px',
            background: 'var(--bg)',
            border: '1px solid var(--line)',
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <strong style={{ fontSize: '11px' }}>
              Log₁₀(Ca_max) Convergence vs. Resolution D / Δx
            </strong>
            <span class="num" style={{ fontSize: '10px', color: 'var(--text-3)' }}>
              Active: D/Δx = {rep.spec.resolution_d_over_dx}
            </span>
          </div>
          <svg
            viewBox="0 0 285 215"
            data-testid="spurious-eddy-convergence-svg"
            style={{
              width: '100%',
              height: 'auto',
              background: '#090d16',
              borderRadius: '6px',
              border: '1px solid var(--line)',
            }}
          >
            {/* Log10 Y-axis grid from 10⁰ down to 10⁻¹¹ */}
            {[0, -2, -4, -6, -8, -10].map((expVal) => {
              const y = 22 + ((0 - expVal) / 11.0) * 162;
              return (
                <g key={expVal}>
                  <line x1="42" y1={y} x2="272" y2={y} stroke="#1e293b" strokeWidth="1" />
                  <text x="4" y={y + 3} fill="#94a3b8" fontSize="8.5" fontFamily="monospace">
                    10^{expVal}
                  </text>
                </g>
              );
            })}

            {/* X-axis ticks D/Δx = 8, 16, 32, 64 */}
            {[8, 16, 32, 64].map((resVal, idx) => {
              const x = 52 + (idx / 3.0) * 210;
              return (
                <g key={resVal}>
                  <line x1={x} y1="20" x2={x} y2="186" stroke="#1e293b" strokeWidth="1" />
                  <text x={x - 10} y="198" fill="#cbd5e1" fontSize="8.5" fontFamily="monospace">
                    {resVal}
                  </text>
                </g>
              );
            })}

            {/* Live vertical cursor line at current slider resolution D/Δx */}
            {(() => {
              const resCur = Math.max(8, Math.min(64, rep.spec.resolution_d_over_dx));
              const xCur = 52 + ((Math.log2(resCur) - 3.0) / 3.0) * 210;
              return (
                <g data-testid="spurious-eddy-cursor-line">
                  <line
                    x1={xCur}
                    y1="18"
                    x2={xCur}
                    y2="186"
                    stroke="#c084fc"
                    strokeWidth="1.4"
                    strokeDasharray="3,2"
                  />
                  <text
                    x={Math.max(46, Math.min(220, xCur - 22))}
                    y="14"
                    fill="#e9d5ff"
                    fontSize="8.5"
                    fontFamily="monospace"
                    fontWeight="bold"
                  >
                    D/Δx={resCur}
                  </text>
                </g>
              );
            })()}

            {/* Plot each of the 5 schemes + live operating-point marker at current D/Δx */}
            {rep.schemes.map((s) => {
              const col = schemeColors[s.scheme_code] ?? '#fff';
              const isSel = s.scheme_code === activeScheme.scheme_code;
              const pts = s.convergence_samples
                .map(([, , caVal], idx) => {
                  const x = 52 + (idx / 3.0) * 210;
                  const logCa = Math.max(-11, Math.min(0, Math.log10(Math.max(1e-12, caVal))));
                  const y = 22 + ((0 - logCa) / 11.0) * 162;
                  return `${x.toFixed(1)},${y.toFixed(1)}`;
                })
                .join(' ');
              const resCur = Math.max(8, Math.min(64, rep.spec.resolution_d_over_dx));
              const xCur = 52 + ((Math.log2(resCur) - 3.0) / 3.0) * 210;
              const logCaCur = Math.max(
                -11,
                Math.min(0, Math.log10(Math.max(1e-12, s.parasitic_capillary_number))),
              );
              const yCur = 22 + ((0 - logCaCur) / 11.0) * 162;
              return (
                <g key={s.scheme_code}>
                  <polyline
                    fill="none"
                    stroke={col}
                    strokeWidth={isSel ? '2.6' : '1.5'}
                    strokeOpacity={isSel ? '1.0' : '0.75'}
                    points={pts}
                  />
                  {s.convergence_samples.map(([, , caVal], idx) => {
                    const x = 52 + (idx / 3.0) * 210;
                    const logCa = Math.max(-11, Math.min(0, Math.log10(Math.max(1e-12, caVal))));
                    const y = 22 + ((0 - logCa) / 11.0) * 162;
                    return (
                      <circle
                        key={idx}
                        cx={x}
                        cy={y}
                        r={isSel ? '3.2' : '2.1'}
                        fill={col}
                      />
                    );
                  })}
                  {/* Live operating point at current slider D/Δx */}
                  {isSel && (
                    <circle
                      cx={xCur}
                      cy={yCur}
                      r="6.2"
                      fill="none"
                      stroke="#ffffff"
                      strokeWidth="1.5"
                    />
                  )}
                  <circle
                    cx={xCur}
                    cy={yCur}
                    r={isSel ? '4.0' : '2.8'}
                    fill={col}
                    stroke="#090d16"
                    strokeWidth="1"
                  />
                </g>
              );
            })}

            <text x="68" y="211" fill="#94a3b8" fontSize="8.5" fontFamily="monospace">
              Droplet Grid Resolution D / Δx (log₂ scale)
            </text>
          </svg>

          <div class="num" style={{ fontSize: '10px', color: 'var(--text-2)', lineHeight: 1.4 }}>
            <strong>Active Basis:</strong> <code>{activeScheme.basis_formula}</code>
            <br />
            <strong>Vorticity Source:</strong> ‖∇_s(γκ_h)×n̂‖_∞ = {activeScheme.curl_vorticity_source_n_m3.toExponential(2)} N/m³
          </div>
        </div>
      </div>

      {/* 5-Scheme Quantitative Comparison Table */}
      <div style={{ overflowX: 'auto' }}>
        <table
          data-testid="spurious-eddy-scheme-table"
          style={{
            width: '100%',
            borderCollapse: 'collapse',
            fontSize: '10.5px',
            background: 'var(--bg)',
            borderRadius: '6px',
            overflow: 'hidden',
          }}
        >
          <thead>
            <tr style={{ background: 'rgba(148, 163, 184, 0.12)', textAlign: 'left' }}>
              <th style={{ padding: '5px 7px' }}>Geometry Approximation Scheme</th>
              <th style={{ padding: '5px 7px' }}>Order p</th>
              <th style={{ padding: '5px 7px', textAlign: 'right' }}>σ*_κ (RMS)</th>
              <th style={{ padding: '5px 7px', textAlign: 'right' }}>U_eddy,max</th>
              <th style={{ padding: '5px 7px', textAlign: 'right' }}>Ca_max</th>
              <th style={{ padding: '5px 7px', textAlign: 'right' }}>E_eddy [J]</th>
            </tr>
          </thead>
          <tbody>
            {rep.schemes.map((s) => {
              const isSel = s.scheme_code === activeScheme.scheme_code;
              const col = schemeColors[s.scheme_code] ?? '#a855f7';
              return (
                <tr
                  key={s.scheme_code}
                  onClick={() => updateParam({ active_scheme: s.scheme_code })}
                  style={{
                    cursor: 'pointer',
                    borderTop: '1px solid var(--line)',
                    background: isSel ? `color-mix(in srgb, ${col} 14%, transparent)` : 'transparent',
                  }}
                >
                  <td style={{ padding: '4px 7px', fontWeight: isSel ? 700 : 500 }}>
                    <span style={{ color: col }}>●</span> {s.scheme_title}
                  </td>
                  <td class="num" style={{ padding: '4px 7px' }}>
                    {s.measured_convergence_order.toFixed(2)} ({s.theoretical_order.split('—')[0].trim()})
                  </td>
                  <td class="num" style={{ padding: '4px 7px', textAlign: 'right' }}>
                    {s.rms_curvature_noise_dimless.toExponential(2)}
                  </td>
                  <td class="num" style={{ padding: '4px 7px', textAlign: 'right' }}>
                    {s.max_eddy_velocity_m_s.toExponential(2)} m/s
                  </td>
                  <td class="num" style={{ padding: '4px 7px', textAlign: 'right', fontWeight: 700, color: col }}>
                    {s.parasitic_capillary_number.toExponential(2)}
                  </td>
                  <td class="num" style={{ padding: '4px 7px', textAlign: 'right' }}>
                    {s.eddy_kinetic_energy_j.toExponential(2)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function FreeSurfaceWorkbench({
  embedded = false,
  initialScenario,
  initialGerrisMode = false,
  initialGerrisCaseId = 'G1',
  initialSpuriousEddyMode = false,
}: {
  embedded?: boolean;
  initialScenario?: string;
  initialGerrisMode?: boolean;
  initialGerrisCaseId?: string;
  initialSpuriousEddyMode?: boolean;
}) {
  useApp();
  const [spec, setSpec] = useState<SphFluidSpec>(() =>
    initialScenario && PRESET_SPECS[initialScenario]
      ? { ...PRESET_SPECS[initialScenario] }
      : { ...freeSurfaceShared.spec },
  );
  const [showGerris, setShowGerris] = useState<boolean>(() =>
    embedded ? initialGerrisMode : initialGerrisMode || Boolean(freeSurfaceShared.showGerris),
  );
  const [showSpuriousEddies, setShowSpuriousEddies] = useState<boolean>(() =>
    embedded
      ? initialSpuriousEddyMode
      : initialSpuriousEddyMode || Boolean(freeSurfaceShared.showSpuriousEddies),
  );
  const [selectedGerrisId, setSelectedGerrisId] = useState<string>(initialGerrisCaseId);
  const [gerrisGridN, setGerrisGridN] = useState<number>(64);
  const [gerrisLaplaceNumber, setGerrisLaplaceNumber] = useState<number>(12000);
  const [particleColorMode, setParticleColorMode] = useState<
    'class' | 'speed' | 'pressure' | 'curvature'
  >('class');
  const [simPlaying, setSimPlaying] = useState<boolean>(true);
  const [simPhase, setSimPhase] = useState<number>(0);

  useEffect(() => {
    if (!embedded) return;
    setShowGerris(initialGerrisMode);
    setShowSpuriousEddies(initialSpuriousEddyMode);
    setSelectedGerrisId(initialGerrisCaseId);
    if (initialScenario && PRESET_SPECS[initialScenario]) {
      const next = { ...PRESET_SPECS[initialScenario] };
      freeSurfaceShared.spec = next;
      setSpec(next);
    }
  }, [
    embedded,
    initialScenario,
    initialGerrisMode,
    initialGerrisCaseId,
    initialSpuriousEddyMode,
  ]);

  useEffect(() => {
    if (!simPlaying) return;
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(0.08, (now - last) / 1000);
      last = now;
      setSimPhase((p) => (p + dt * 0.45) % 1.0);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [simPlaying]);

  const updateSpec = (patch: Partial<SphFluidSpec>) => {
    const next = { ...spec, ...patch };
    freeSurfaceShared.spec = next;
    setSpec(next);
  };

  // Sync FreeSurfaceWorkbench parameters from active .fsm document lets on mount / change
  const docLetsSig = (store.doc.lets ?? []).map((l) => `${l.name}:${l.value}`).join('|');
  useEffect(() => {
    const getLet = (k: string): number | undefined => {
      const found = (store.doc.lets ?? []).find((l) => l.name === k);
      return found && Number.isFinite(found.value) ? found.value : undefined;
    };
    const pourRate = getLet('pour_rate_kg_s');
    const spoutLipR = getLet('spout_lip_r_mm');
    const baffleX = getLet('baffle_x');
    const dropletR = getLet('droplet_r_mm');
    const boreR = getLet('bore_r_mm');
    const gammaNm = getLet('gamma_n_m');
    const contactAngle = getLet('contact_angle_deg');
    const hSill = getLet('h_sill');

    const patch: Partial<SphFluidSpec> = {};
    if (spoutLipR !== undefined && spoutLipR > 0) {
      patch.characteristic_radius_mm = spoutLipR;
    } else if (dropletR !== undefined && dropletR > 0) {
      patch.characteristic_radius_mm = dropletR;
    } else if (boreR !== undefined && boreR > 0) {
      patch.characteristic_radius_mm = boreR;
    }
    if (gammaNm !== undefined && gammaNm > 0) {
      patch.surface_tension_n_m = gammaNm;
    }
    if (contactAngle !== undefined && contactAngle >= 0 && contactAngle <= 180) {
      patch.contact_angle_deg = contactAngle;
    }
    if (pourRate !== undefined && pourRate > 0) {
      patch.reservoir_height_mm = Math.max(8, Math.min(200, pourRate * 18));
    } else if (hSill !== undefined && hSill > 0) {
      patch.reservoir_height_mm = Math.max(8, Math.min(200, hSill * 0.6));
    }
    if (baffleX !== undefined && patch.characteristic_radius_mm === undefined) {
      patch.characteristic_radius_mm = Math.max(1.5, Math.min(25, Math.abs(baffleX) * 0.25));
    }

    if (Object.keys(patch).length > 0) {
      setSpec((prev) => {
        const next = { ...prev, ...patch };
        freeSurfaceShared.spec = next;
        return next;
      });
    }
  }, [docLetsSig, store.doc.name]);

  const selectPreset = (key: string) => {
    const next = { ...(PRESET_SPECS[key] ?? PRESET_SPECS.spout_lip_knurl_1100c) };
    freeSurfaceShared.spec = next;
    freeSurfaceShared.showGerris = false;
    freeSurfaceShared.showSpuriousEddies = false;
    setShowGerris(false);
    setShowSpuriousEddies(false);
    setSpec(next);
  };

  let gerrisSuite: GerrisSuiteSummary | null = null;
  if (showGerris) {
    try {
      gerrisSuite = runGerrisTestSuiteWasm(gerrisGridN, gerrisLaplaceNumber);
    } catch {
      gerrisSuite = null;
    }
  }

  let bundle: { report: SphSimulationReport; fsm_3d: string; wall_ms: number };
  try {
    bundle = solveFreeSurfaceSphWasm(spec);
  } catch (e) {
    return (
      <div class="tut-step-card" style={{ borderColor: 'var(--danger)' }}>
        <strong>Free-Surface SPH Solver Error:</strong> {String(e)}
      </div>
    );
  }

  const rep = bundle.report;
  const spout: SpoutLipPourReport = rep.spout_lip_report;
  const pereg = rep.peregrine_air_report as unknown as PeregrineAirEntrainmentReport;
  const badgeColor = regimeColor(rep.primary_regime_code);

  const buildGerrisCase3dFsm = (
    caseId: string,
    suiteIn: GerrisSuiteSummary | null = gerrisSuite,
  ): { fsm: string; fileName: string; summary: string } => {
    const suite = suiteIn ?? runGerrisTestSuiteWasm(gerrisGridN, gerrisLaplaceNumber);
    const activeCase = suite.cases.find((c) => c.id === caseId) ?? suite.cases[0];
    const nVal = suite.grid_n ?? gerrisGridN;
    const laVal = suite.laplace_number ?? gerrisLaplaceNumber;
    const errStr = activeCase ? `${activeCase.rel_error_pct.toFixed(4)}%` : '0.01%';

    if (caseId === 'G1') {
      const pLapMeas = activeCase ? activeCase.fieldsmith_measured : 72.8;
      const pLapMpa = Number(Math.max(0.01, (pLapMeas * 1e-3) * (1 + 25 / Math.max(16, nVal))).toFixed(5));
      const rOuter = Number((14 + 0.02 * (nVal - 64)).toFixed(2));
      const rInner = Number((rOuter - 3.0).toFixed(2));
      const rTrap = Number((0.6 * (rOuter - rInner)).toFixed(2));
      return {
        fileName: 'gerris-g1-spurious-droplet.fsm',
        summary: `Loaded G1 (gerris/test/spurious) 3D Stationary Droplet Vessel (N=${nVal}, La=${laVal}, Δp=${pLapMeas.toFixed(2)} Pa, Err=${errStr}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G1 (test/spurious) — Stationary Droplet Young–Laplace Jump & Parasitic Currents (N=${nVal}, La=${laVal})"`,
          'material ti64',
          '',
          'let droplet_r_mm = 2.0',
          `let resolution_d_dx = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let measured_dp_pa = ${pLapMeas.toFixed(4)}`,
          '',
          `sphere droplet_outer_shell r=${rOuter} at=(0, 0, ${rOuter})`,
          `sphere droplet_inner_cavity r=${rInner} at=(0, 0, ${rOuter}) subtract`,
          `sphere optical_trap_core r=${rTrap} at=(0, 0, 0) subtract`,
          '',
          'region mount_base = optical_trap_core.all',
          'region meniscus_wall = droplet_inner_cavity.all',
          '',
          'support clamp mount_base fixed=xyz',
          `load young_laplace_jump meniscus_wall pressure=${pLapMpa}`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G2') {
      const a2T = activeCase ? activeCase.fieldsmith_measured : 0.874;
      const hCrest = Number((8 + 4 * a2T).toFixed(2));
      const pWave = Number((0.045 * Math.max(0.2, a2T) * (1 + 1200 / Math.max(100, laVal))).toFixed(4));
      return {
        fileName: 'gerris-g2-prosperetti-capwave.fsm',
        summary: `Loaded G2 (gerris/test/capwave) 3D Prosperetti Viscous Capillary Wave Tank (a(2T₀)/a₀=${a2T.toFixed(4)}, N=${nVal}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G2 (test/capwave) — Prosperetti (1981) Viscous Damped Standing Capillary Wave (N=${nVal}, La=${laVal})"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let crest_ratio = ${a2T.toFixed(4)}`,
          '',
          'box wave_flume_bed size=(72, 32, 6) at=(0, 0, -3) round=0.8',
          `box standing_wave_bulk size=(68, 28, ${hCrest}) at=(0, 0, ${(0.5 * hCrest - 0.5).toFixed(2)}) round=1.2`,
          `cylinder wave_crest_center r=8 h=28 at=(0, 0, ${hCrest}) rotate=(90, 0, 0)`,
          `cylinder wave_trough_left r=6 h=30 at=(-18, 0, ${(hCrest + 1.5).toFixed(2)}) rotate=(90, 0, 0) subtract`,
          `cylinder wave_trough_right r=6 h=30 at=(18, 0, ${(hCrest + 1.5).toFixed(2)}) rotate=(90, 0, 0) subtract`,
          '',
          'region flume_floor = wave_flume_bed.nz',
          'region crest_surface = wave_crest_center.all',
          '',
          'support anchor flume_floor fixed=xyz',
          `load capillary_wave_restore crest_surface pressure=${pWave}`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G3') {
      const t2Ms = activeCase ? activeCase.fieldsmith_measured : 23.26;
      const rOuter = 14.0;
      const rInner = 11.0;
      const rTrap = Number((0.6 * (rOuter - rInner)).toFixed(2));
      const pModalMpa = Number((0.18 * (23.26 / Math.max(5, t2Ms)) * (1 + 800 / Math.max(100, laVal))).toFixed(5));
      return {
        fileName: 'gerris-g3-rayleigh-lamb-oscillation.fsm',
        summary: `Loaded G3 (gerris/test/oscillation) 3D Rayleigh–Lamb Quadrupole Oscillating Droplet (T₂=${t2Ms.toFixed(2)} ms, N=${nVal}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G3 (test/oscillation) — Rayleigh–Lamb Capillary Droplet Eigenmodes n=2..6 (T₂=${t2Ms.toFixed(2)} ms)"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let period_t2_ms = ${t2Ms.toFixed(3)}`,
          '',
          `sphere quadrupole_core r=${rOuter} at=(0, 0, ${rOuter})`,
          `sphere inner_fluid_cavity r=${rInner} at=(0, 0, ${rOuter}) subtract`,
          `sphere optical_trap_core r=${rTrap} at=(0, 0, 0) subtract`,
          '',
          'region base_clamp = optical_trap_core.all',
          'region mode_wall = inner_fluid_cavity.all',
          '',
          'support mount base_clamp fixed=xyz',
          `load rayleigh_lamb_modal_pressure mode_wall pressure=${pModalMpa}`,
          'solve modes=6',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G4') {
      const omegaMeas = activeCase ? activeCase.fieldsmith_measured : 0.3433;
      const hMin = suite.eggers_min_neck_radius_over_r0 ?? 0.014;
      const rSat = suite.eggers_satellite_drop_radius_over_r0 ?? 0.295;
      const r0Jet = 10.0;
      const rSatMm = Number(Math.max(2.8, rSat * r0Jet * 1.4).toFixed(2));
      const rNeckMm = Number(Math.max(1.4, hMin * r0Jet * 12 + 1.2).toFixed(2));
      const pPinchMpa = Number((0.42 * omegaMeas / Math.max(0.008, hMin * 10)).toFixed(4));
      return {
        fileName: 'gerris-g4-eggers-plateau-pinchoff.fsm',
        summary: `Loaded G4 (gerris/test/plateau) 3D Eggers Slender-Jet Pinch-Off Neck (h_min/R₀=${hMin.toFixed(4)}) & Satellite Bulb (R_sat/R₀=${rSat.toFixed(3)}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G4 (test/plateau) — Rayleigh–Plateau & Eggers Slender-Jet Pinch-Off (h_min/R₀=${hMin.toFixed(4)}, R_sat/R₀=${rSat.toFixed(3)})"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let h_min_over_r0 = ${hMin.toFixed(4)}`,
          `let r_sat_over_r0 = ${rSat.toFixed(3)}`,
          '',
          'cylinder lower_nozzle_collar r=14 h=4 at=(0, 0, 2)',
          'sphere bottom_main_drop r=12.5 at=(0, 0, 12)',
          `cylinder lower_pinchoff_neck r=${rNeckMm} h=18 at=(0, 0, 23)`,
          `sphere central_satellite_bulb r=${rSatMm} at=(0, 0, 33)`,
          `cylinder upper_pinchoff_neck r=${rNeckMm} h=18 at=(0, 0, 43)`,
          'sphere upper_main_drop r=12.5 at=(0, 0, 54)',
          '',
          'region base_clamp = lower_nozzle_collar.bottom',
          'region neck_wall_lower = lower_pinchoff_neck.all',
          'region neck_wall_upper = upper_pinchoff_neck.all',
          '',
          'support clamp base_clamp fixed=xyz',
          `load eggers_self_similar_pinch_lower neck_wall_lower pressure=${pPinchMpa}`,
          `load eggers_self_similar_pinch_upper neck_wall_upper pressure=${pPinchMpa}`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G5') {
      const qMeas = activeCase ? activeCase.fieldsmith_measured : 0.3333;
      const hFilm = Number((36 * qMeas).toFixed(2));
      const pSlopeMpa = Number((0.195 * qMeas * (1 + 600 / Math.max(100, laVal))).toFixed(4));
      return {
        fileName: 'gerris-g5-nusselt-poiseuille-film.fsm',
        summary: `Loaded G5 (gerris/test/poiseuille) 3D Nusselt Gravity-Driven Viscous Free-Surface Film (Q*=${qMeas.toFixed(4)}, N=${nVal}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G5 (test/poiseuille) — Nusselt (1916) Gravity-Driven Viscous Free-Surface Film (Q*=${qMeas.toFixed(5)})"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let film_flux_q = ${qMeas.toFixed(5)}`,
          '',
          'box inclined_no_slip_wall size=(76, 36, 6) at=(0, 0, -3) round=0.8',
          `box viscous_nusselt_film_layer size=(72, 32, ${hFilm}) at=(0, 0, ${(0.5 * hFilm - 0.5).toFixed(2)}) round=1.2`,
          `cylinder velocity_probe_well r=3.5 h=${hFilm} at=(12, 0, ${(0.5 * hFilm + 0.5).toFixed(2)}) subtract`,
          '',
          'region bed_wall = inclined_no_slip_wall.nz',
          'region film_free_surface = viscous_nusselt_film_layer.pz',
          'region film_inlet = viscous_nusselt_film_layer.nx',
          '',
          'support no_slip_bed bed_wall fixed=xyz',
          `load gravity_body_slope film_inlet pressure=${pSlopeMpa}`,
          'load zero_shear_meniscus film_free_surface pressure=0.012',
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G6') {
      const uMid = activeCase ? activeCase.fieldsmith_measured : 0.3889;
      const pTorqueMpa = Number((0.245 * uMid * (1 + 500 / Math.max(100, laVal))).toFixed(4));
      return {
        fileName: 'gerris-g6-taylor-couette-sdf.fsm',
        summary: `Loaded G6 (gerris/test/couette) 3D Concentric Taylor–Couette Cylindrical Shear Cell (u_θ(1.5R₁)=${uMid.toFixed(4)}, N=${nVal}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G6 (test/couette) — Concentric Taylor–Couette Cylindrical Shear on Cut-Cell SDF (N=${nVal})"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let u_mid_gap = ${uMid.toFixed(5)}`,
          '',
          'cylinder outer_stator_housing r=24 h=22 at=(0, 0, 11)',
          'cylinder couette_annular_gap r=20 h=20 at=(0, 0, 12) subtract',
          'cylinder inner_rotating_cylinder r=10 h=24 at=(0, 0, 12)',
          'cylinder inner_shaft_bore r=4.5 h=26 at=(0, 0, 12) subtract',
          '',
          'region stator_mount = outer_stator_housing.bottom',
          'region rotor_wet_wall = inner_rotating_cylinder.all',
          '',
          'support outer_stator stator_mount fixed=xyz',
          `load couette_viscous_torque rotor_wet_wall pressure=${pTorqueMpa}`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (caseId === 'G7') {
      const eRatio = activeCase ? activeCase.fieldsmith_measured : 0.1353;
      const pVortMpa = Number((0.55 * eRatio * (1 + 1000 / Math.max(100, laVal))).toFixed(4));
      return {
        fileName: 'gerris-g7-taylor-green-vortex.fsm',
        summary: `Loaded G7 (gerris/test/reynolds) 3D Taylor–Green 4-Vortex Periodic Cell (E(1)/E₀=${eRatio.toFixed(4)}, N=${nVal}) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Gerris G7 (test/reynolds) — Taylor–Green Unsteady Navier–Stokes 4-Vortex Viscous Decay (E/E₀=${eRatio.toFixed(5)})"`,
          'material ti64',
          '',
          `let grid_n = ${nVal}`,
          `let laplace_la = ${laVal}`,
          `let energy_ratio = ${eRatio.toFixed(5)}`,
          '',
          'box periodic_vortex_domain size=(52, 52, 14) at=(0, 0, 7) round=1.5',
          'cylinder vortex_core_q1 r=7.5 h=16 at=(12, 12, 7) subtract',
          'cylinder vortex_core_q2 r=7.5 h=16 at=(-12, 12, 7) subtract',
          'cylinder vortex_core_q3 r=7.5 h=16 at=(-12, -12, 7) subtract',
          'cylinder vortex_core_q4 r=7.5 h=16 at=(12, -12, 7) subtract',
          '',
          'region domain_base = periodic_vortex_domain.nz',
          'region vortex_wall_1 = vortex_core_q1.all',
          'region vortex_wall_3 = vortex_core_q3.all',
          '',
          'support periodic_anchor domain_base fixed=xyz',
          `load taylor_green_vorticity_q1 vortex_wall_1 pressure=${pVortMpa}`,
          `load taylor_green_vorticity_q3 vortex_wall_3 pressure=${pVortMpa}`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    const tSlosh = activeCase ? activeCase.fieldsmith_measured : 4.0128;
    const pSloshMpa = Number((0.055 * (4.0128 / Math.max(1.0, tSlosh)) * (1 + 500 / Math.max(100, laVal))).toFixed(4));
    return {
      fileName: 'gerris-g8-thacker-parabolic-basin.fsm',
      summary: `Loaded G8 (gerris/test/parabola) 3D Thacker Parabolic Basin Moving-Shoreline Model (T=${tSlosh.toFixed(4)} s, N=${nVal}) to Viewport`,
      fsm: [
        'fieldsmith 1',
        `name "Gerris G8 (test/parabola) — Thacker (1981) Parabolic Basin Moving-Shoreline Slosh (T=${tSlosh.toFixed(4)} s, N=${nVal})"`,
        'material ti64',
        '',
        `let grid_n = ${nVal}`,
        `let laplace_la = ${laVal}`,
        `let slosh_period_s = ${tSlosh.toFixed(4)}`,
        '',
        'cylinder parabolic_basin_foundation r=30 h=14 at=(0, 0, 7)',
        'sphere parabolic_bowl_cavity r=26 at=(0, 0, 22) subtract',
        'cylinder sloshing_water_lens r=18 h=5 at=(3.5, 0, 9.5)',
        '',
        'region foundation_base = parabolic_basin_foundation.bottom',
        'region shoreline_wet_bowl = sloshing_water_lens.top',
        '',
        'support basin_anchor foundation_base fixed=xyz',
        `load thacker_hydrostatic_slosh shoreline_wet_bowl pressure=${pSloshMpa}`,
        'solve',
        '',
      ].join('\n'),
    };
  };

  const syncGerrisCaseToViewport = (caseId?: string) => {
    const built = buildGerrisCase3dFsm(caseId ?? selectedGerrisId, gerrisSuite);
    openText(built.fsm, built.fileName);
    viewApi.fit(false);
    ui.set('freeSurfaceOpen', false);
    store.setLens('results');
    ui.notify(built.summary);
  };

  const buildActiveScenario3dFsm = (): { fsm: string; fileName: string; summary: string } => {
    if (showGerris) {
      return buildGerrisCase3dFsm(selectedGerrisId, gerrisSuite);
    }
    const r0 = Math.max(1.2, spec.characteristic_radius_mm);
    const h0 = Math.max(15, spec.reservoir_height_mm);
    const pLaplaceMpa = Number(Math.max(0.005, Math.abs(rep.sph_laplace_pressure_pa) * 1e-6).toFixed(5));
    const dT = Math.max(5, Math.round(spec.thermal_gradient_delta_k));

    if (spec.scenario === 'spout_lip_knurl_1100c') {
      const hEq = Math.max(3.5, Math.min(14, spout.equilibrium_ribbon_thickness_mm));
      const lHeel = Math.max(10, Math.min(34, spout.heel_wetback_length_mm));
      const pHydroMpa = Number(Math.max(0.02, (spec.density_kg_m3 * 9.81 * (h0 * 1e-3)) * 1e-6).toFixed(5));
      return {
        fileName: 'spout-lip-1100c-float-ribbon.fsm',
        summary: `Loaded 3D 1100 °C Spout-Lip, Reverse Wet-Back Heel (${lHeel.toFixed(1)} mm) & Float Glass Ribbon (${hEq.toFixed(2)} mm) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "1100 °C Float-Bath Spout-Lip, Wet-Back Heel & Knurl Selvedge (Oh = ${rep.ohnesorge_number.toFixed(1)})"`,
          'custommaterial sodalime_melt_1100c name="1100C Soda-Lime Melt & Refractory" e=68000 nu=0.22 k=1.8e-3 a=9.0e-6 yield=45 density=2.50',
          'material sodalime_melt_1100c',
          '',
          `let spout_lip_r_mm = ${r0.toFixed(2)}`,
          `let pour_rate_kg_s = ${(h0 / 18).toFixed(2)}`,
          `let gamma_n_m = ${spec.surface_tension_n_m.toFixed(4)}`,
          `let contact_angle_deg = ${spec.contact_angle_deg.toFixed(1)}`,
          `let p_fsi_mpa = ${pHydroMpa}`,
          `let p_cap_mpa = ${pLaplaceMpa}`,
          '',
          'box tin_bath_bed size=(96, 68, 8) at=(6, 0, -4) round=1.2',
          'box refractory_spout_lip size=(34, 48, 18) at=(-24, 0, 7) round=1.5',
          `box wetback_heel_meniscus size=(${lHeel.toFixed(1)}, 46, 10) at=(${(-10 - 0.35 * lHeel).toFixed(1)}, 0, 2) round=2.5`,
          `box float_glass_ribbon size=(66, 56, ${hEq.toFixed(2)}) at=(18, 0, ${(0.5 * hEq - 1.0).toFixed(2)}) round=1.5`,
          `cylinder knurl_roll_left r=7 h=8 at=(24, 25, ${(hEq + 3.5).toFixed(2)}) rotate=(90, 0, 0)`,
          `cylinder knurl_roll_right r=7 h=8 at=(24, -25, ${(hEq + 3.5).toFixed(2)}) rotate=(90, 0, 0)`,
          '',
          'region bath_anchor = tin_bath_bed.nz',
          'region spout_wet_lip = refractory_spout_lip.px',
          'region ribbon_top = float_glass_ribbon.pz',
          '',
          'support bath_mount bath_anchor fixed=xyz',
          `load glass_head_pressure spout_wet_lip pressure=${pHydroMpa} case="1: 1100 °C Spout Pour & Wet-Back"`,
          `load knurl_pull_traction ribbon_top pressure=${pLaplaceMpa} case="1: 1100 °C Spout Pour & Wet-Back"`,
          `temperature spout_core spout_wet_lip temp=1100 case="1: 1100 °C Spout Pour & Wet-Back"`,
          `temperature chilled_skin ribbon_top temp=${Math.max(850, 1100 - dT)} case="1: 1100 °C Spout Pour & Wet-Back"`,
          'solve tref=1050',
          '',
        ].join('\n'),
      };
    }

    if (spec.scenario === 'peregrine_wave_air_entrainment') {
      const rTube = Math.max(3.0, Math.min(11.0, pereg.trapped_air_tube_radius_mm));
      const pSpikeMpa = Number(Math.max(0.04, (pereg.peak_wall_impulse_pa_s ?? 45) * 0.0025).toFixed(5));
      return {
        fileName: 'peregrine-wave-air-pocket-impact.fsm',
        summary: `Loaded 3D Howell Peregrine Plunging Wave, Trapped Air-Tube (R=${rTube.toFixed(1)} mm) & Seawall Impact to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "Howell Peregrine Plunging Wave, Trapped Air-Tube & Seawall Impulse (ξ₀ = ${pereg.iribarren_number.toFixed(2)})"`,
          'material ti64',
          '',
          `let h_sill = ${h0.toFixed(1)}`,
          `let p_fsi_mpa = ${pSpikeMpa}`,
          `let p_cap_mpa = ${pLaplaceMpa}`,
          `let hinze_radius_mm = ${pereg.hinze_bubble_radius_mm.toFixed(3)}`,
          '',
          'box shoaling_seabed size=(88, 48, 8) at=(0, 0, -10) round=1',
          'box coastal_seawall size=(10, 48, 42) at=(32, 0, 11) round=1',
          'cylinder overturning_wave_crest r=16 h=44 at=(8, 0, 4) rotate=(90, 0, 0)',
          `cylinder trapped_air_pocket_tube r=${rTube.toFixed(2)} h=46 at=(10, 0, 3) rotate=(90, 0, 0) subtract`,
          'box plunging_jet_tongue size=(26, 42, 10) at=(22, 0, 11) round=2',
          '',
          'region seabed_base = shoaling_seabed.nz',
          'region seawall_impact_face = coastal_seawall.nx',
          'region wave_crest_top = plunging_jet_tongue.pz',
          '',
          'support anchor seabed_base fixed=xyz',
          `load peregrine_flip_impact seawall_impact_face pressure=${pSpikeMpa} case="1: Cooker-Peregrine Wave Impulse"`,
          `load air_pocket_cushion wave_crest_top pressure=${pLaplaceMpa} case="1: Cooker-Peregrine Wave Impulse"`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (spec.scenario === 'droplet_oscillation_a15') {
      const rShell = Math.max(8, Math.min(22, r0 * 5.0));
      const rInner = Number((rShell * 0.76).toFixed(2));
      const rTrap = Number((0.6 * (rShell - rInner)).toFixed(2));
      const pDropMpa = Number(Math.max(0.08, rep.sph_laplace_pressure_pa * 1e-3).toFixed(4));
      return {
        fileName: 'a15-young-laplace-droplet.fsm',
        summary: `Loaded 3D A15 Young–Laplace Oscillating Droplet Shell (Δp = ${rep.sph_laplace_pressure_pa.toFixed(1)} Pa, T₂ = ${rep.sph_rayleigh_period_ms.toFixed(2)} ms) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "A15 — 3D Young–Laplace Capillary Droplet & Rayleigh–Lamb Quadrupole Mode (T₂ = ${rep.sph_rayleigh_period_ms.toFixed(2)} ms)"`,
          'material ti64',
          '',
          `let droplet_r_mm = ${r0.toFixed(2)}`,
          `let gamma_n_m = ${spec.surface_tension_n_m.toFixed(4)}`,
          `let laplace_mpa = ${pDropMpa}`,
          '',
          `sphere droplet_outer_meniscus r=${rShell.toFixed(2)} at=(0, 0, ${rShell.toFixed(2)})`,
          `sphere droplet_inner_core r=${rInner.toFixed(2)} at=(0, 0, ${rShell.toFixed(2)}) subtract`,
          `sphere optical_trap_core r=${rTrap.toFixed(2)} at=(0, 0, 0) subtract`,
          '',
          'region pedestal_mount = optical_trap_core.all',
          'region meniscus_wall = droplet_inner_core.all',
          '',
          'support fixed_base pedestal_mount fixed=xyz',
          `load young_laplace_jump meniscus_wall pressure=${pDropMpa}`,
          'solve modes=6',
          '',
        ].join('\n'),
      };
    }

    if (spec.scenario === 'capillary_rise_a16') {
      const rBore = Math.max(3.5, Math.min(10, r0 * 4.5));
      const rOuter = rBore + 3.5;
      const hCol = Math.max(18, Math.min(44, Math.abs(rep.sph_capillary_rise_mm) * 1.4));
      const pCapMpa = Number(Math.max(0.05, Math.abs(rep.tpms_capillary_retention_pa) * 1e-4).toFixed(4));
      return {
        fileName: 'a16-jurin-capillary-gyroid-wick.fsm',
        summary: `Loaded 3D A16 Jurin Capillary Tube & Concave Meniscus (h = ${rep.sph_capillary_rise_mm.toFixed(2)} mm, θ = ${spec.contact_angle_deg.toFixed(0)}°) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "A16 — Jurin Capillary Rise Column (h = ${rep.sph_capillary_rise_mm.toFixed(2)} mm) & TPMS Gyroid Wick"`,
          'material ti64',
          '',
          `let bore_r_mm = ${r0.toFixed(2)}`,
          `let contact_angle_deg = ${spec.contact_angle_deg.toFixed(1)}`,
          `let p_cap_mpa = ${pCapMpa}`,
          '',
          `cylinder capillary_tube_wall r=${rOuter.toFixed(2)} h=${hCol.toFixed(1)} at=(0, 0, ${(0.5 * hCol).toFixed(1)})`,
          `cylinder capillary_bore r=${rBore.toFixed(2)} h=${(hCol * 0.85).toFixed(1)} at=(0, 0, ${(0.58 * hCol).toFixed(1)}) subtract`,
          `sphere concave_meniscus_cap r=${(rBore * 1.15).toFixed(2)} at=(0, 0, ${(hCol - 1.5).toFixed(1)}) subtract`,
          '',
          'region dish_base = capillary_tube_wall.bottom',
          'region tube_top_rim = capillary_tube_wall.top',
          'region bore_wet_wall = capillary_bore.all',
          '',
          'support base_clamp dish_base fixed=xyz',
          `load jurin_meniscus_suction bore_wet_wall pressure=${pCapMpa}`,
          `fill gyroid sheet cell=${spec.tpms_period_mm.toFixed(1)} skin=0.8 rho=(0.28, 0.72) curve=1.0 wall=0.6`,
          'solve',
          '',
        ].join('\n'),
      };
    }

    if (spec.scenario === 'marangoni_melt_pool') {
      const rPool = Math.max(9, Math.min(22, r0 * 4.2));
      const pRecoilMpa = Number(Math.max(0.06, rep.marangoni_surface_velocity_m_s * 0.25).toFixed(4));
      return {
        fileName: 'marangoni-thermocapillary-melt-pool.fsm',
        summary: `Loaded 3D Marangoni Laser Melt Pool (u_M = ${rep.marangoni_surface_velocity_m_s.toFixed(3)} m/s, ΔT = ${dT} K) to Viewport`,
        fsm: [
          'fieldsmith 1',
          `name "3D Thermocapillary Marangoni Melt Pool (u_M = ${rep.marangoni_surface_velocity_m_s.toFixed(3)} m/s, Ma = ${rep.marangoni_number.toExponential(2)})"`,
          'material ti64',
          '',
          `let gamma_n_m = ${spec.surface_tension_n_m.toFixed(4)}`,
          `let p_fsi_mpa = ${pRecoilMpa}`,
          '',
          'box build_plate_substrate size=(56, 56, 18) at=(0, 0, 9) round=1.2',
          `sphere laser_keyhole_crater r=${rPool.toFixed(2)} at=(0, 0, ${(18 + 0.45 * rPool).toFixed(2)}) subtract`,
          `cylinder solidified_weld_bead_rim r=${(rPool * 1.18).toFixed(2)} h=4 at=(0, 0, 18.5)`,
          `cylinder weld_crater_core r=${(rPool * 0.88).toFixed(2)} h=6 at=(0, 0, 19) subtract`,
          '',
          'region substrate_bottom = build_plate_substrate.nz',
          'region melt_pool_crater = laser_keyhole_crater.all',
          'region bead_rim_top = solidified_weld_bead_rim.top',
          '',
          'support chill_plate substrate_bottom fixed=xyz',
          `load marangoni_recoil_shear melt_pool_crater pressure=${pRecoilMpa} case="1: Thermocapillary Melt Pool"`,
          `temperature cold_substrate substrate_bottom temp=40 case="1: Thermocapillary Melt Pool"`,
          `temperature laser_spot bead_rim_top temp=${40 + dT * 4} case="1: Thermocapillary Melt Pool"`,
          'solve tref=40',
          '',
        ].join('\n'),
      };
    }

    return {
      fileName: 't18-multi-regime-free-surface-fsi.fsm',
      summary: `Loaded T18 3D Dam-Break FSI (${rep.peak_fsi_impact_pressure_kpa.toFixed(2)} kPa) + TPMS Capillary Wick CAD Template`,
      fsm: bundle.fsm_3d,
    };
  };

  const projectFsiTo3d = () => {
    const built = buildActiveScenario3dFsm();
    openText(built.fsm, built.fileName);
    viewApi.fit(false);
    ui.set('freeSurfaceOpen', false);
    if (spec.scenario === 'droplet_oscillation_a15' || (showGerris && selectedGerrisId === 'G3')) {
      ui.set('resultsRampPlay', true);
    }
    store.setLens('results');
    ui.notify(built.summary);
  };

  return (
    <div
      data-testid="free-surface-workbench"
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: '10px',
        fontSize: '12px',
      }}
    >
      {/* 1. Scenario & Multi-Regime Preset Selector (hidden when embedded in a dedicated A15–A20 / T18 sheet) */}
      {!embedded ? (
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '6px',
            padding: '8px 10px',
            borderRadius: '7px',
            background: 'var(--panel)',
            border: '1px solid var(--line)',
          }}
        >
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              gap: '6px',
              alignItems: 'center',
              justifyContent: 'space-between',
            }}
          >
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
              <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-3)', marginRight: '2px' }}>
                3D Scenarios:
              </span>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'spout_lip_knurl_1100c' ? 'primary' : ''}`}
                data-testid="sph-preset-spout-1100c"
                onClick={() => selectPreset('spout_lip_knurl_1100c')}
              >
                🔥 1100 °C Spout-Lip &amp; Knurl Heel (Oh ≫ 1)
              </button>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'peregrine_wave_air_entrainment' ? 'primary' : ''}`}
                data-testid="sph-preset-peregrine-wave"
                onClick={() => selectPreset('peregrine_wave_air_entrainment')}
              >
                🌊 Howell Peregrine Wave &amp; Air Entrainment
              </button>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'droplet_oscillation_a15' ? 'primary' : ''}`}
                data-testid="sph-preset-a15"
                onClick={() => selectPreset('droplet_oscillation_a15')}
              >
                💧 A15: Young–Laplace Droplet
              </button>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'capillary_rise_a16' ? 'primary' : ''}`}
                data-testid="sph-preset-a16"
                onClick={() => selectPreset('capillary_rise_a16')}
              >
                🧽 A16: Jurin &amp; TPMS Gyroid Wick
              </button>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'marangoni_melt_pool' ? 'primary' : ''}`}
                data-testid="sph-preset-marangoni"
                onClick={() => selectPreset('marangoni_melt_pool')}
              >
                🌡️ Marangoni Melt Pool
              </button>
              <button
                type="button"
                class={`btn small ${!showGerris && !showSpuriousEddies && spec.scenario === 'dambreak_fsi_t18' ? 'primary' : ''}`}
                data-testid="sph-preset-t18"
                onClick={() => selectPreset('dambreak_fsi_t18')}
              >
                🛡️ T18: Dam-Break FSI Baffle
              </button>
            </div>
          </div>

          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              gap: '6px',
              alignItems: 'center',
              justifyContent: 'space-between',
              paddingTop: '6px',
              borderTop: '1px solid var(--line)',
            }}
          >
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
              <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-3)', marginRight: '2px' }}>
                Verification Suites:
              </span>
              <button
                type="button"
                class={`btn small ${showGerris ? 'primary' : ''}`}
                data-testid="sph-preset-gerris-suite"
                onClick={() => {
                  const next = !showGerris;
                  freeSurfaceShared.showGerris = next;
                  if (next) {
                    freeSurfaceShared.showSpuriousEddies = false;
                    setShowSpuriousEddies(false);
                  }
                  setShowGerris(next);
                }}
                style={{ borderColor: '#22d3ee' }}
              >
                📐 Gerris Analytic Suite (G1–G8)
              </button>
              <button
                type="button"
                class={`btn small ${showSpuriousEddies ? 'primary' : ''}`}
                data-testid="sph-preset-spurious-eddies"
                onClick={() => {
                  const next = !showSpuriousEddies;
                  freeSurfaceShared.showSpuriousEddies = next;
                  if (next) {
                    freeSurfaceShared.showGerris = false;
                    setShowGerris(false);
                  }
                  setShowSpuriousEddies(next);
                  if (next && !/A20|Spurious/i.test(store.doc.name || '')) {
                    loadA20DropletVesselToViewport(freeSurfaceShared.spuriousSpec);
                  }
                }}
                style={{ borderColor: '#c084fc' }}
              >
                🌀 A20: Spurious Eddies &amp; Curvature Error
              </button>
            </div>

            {!showSpuriousEddies && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
                <button
                  type="button"
                  class={`btn small ${simPlaying ? 'primary' : ''}`}
                  data-testid="sph-time-play-toggle"
                  onClick={() => setSimPlaying((p) => !p)}
                  title="Play / Pause live free-surface flow & capillary oscillation time-stepping"
                  style={{ fontFamily: 'var(--mono)', fontSize: '11px' }}
                >
                  {simPlaying ? '⏸ Pause Flow' : '▶ Play Flow'} (t/T = {(simPhase * 100).toFixed(0)}%)
                </button>
                <button
                  type="button"
                  class="btn small primary"
                  data-testid="sph-project-3d-fsi-btn"
                  onClick={projectFsiTo3d}
                  title="Build and solve the 3D CAD & Free-Surface FSI model for the active scenario in the 3D Viewport"
                >
                  🌊 Sync 3D Scenario Geometry &amp; Loads to Viewport →
                </button>
              </div>
            )}
          </div>
        </div>
      ) : (
        !showGerris &&
        !showSpuriousEddies && (
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              gap: '6px',
              alignItems: 'center',
              justifyContent: 'space-between',
            }}
          >
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
              <button
                type="button"
                class={`btn small ${simPlaying ? 'primary' : ''}`}
                data-testid="sph-time-play-toggle"
                onClick={() => setSimPlaying((p) => !p)}
                title="Play / Pause live free-surface flow & capillary oscillation time-stepping"
                style={{ fontFamily: 'var(--mono)', fontSize: '11px' }}
              >
                {simPlaying ? '⏸ Pause Flow' : '▶ Play Flow'} (t/T = {(simPhase * 100).toFixed(0)}%)
              </button>
              <button
                type="button"
                class="btn small primary"
                data-testid="sph-project-3d-fsi-btn"
                onClick={projectFsiTo3d}
                title="Build and solve the 3D CAD & Free-Surface FSI model for this scenario in the 3D Viewport"
              >
                🌊 Sync 3D Scenario Geometry &amp; Loads to Viewport →
              </button>
            </div>
            <button
              type="button"
              class="btn small"
              data-testid="sph-open-standalone-modal-btn"
              onClick={() => openFreeSurfaceModal(spec.scenario)}
              title="Open the standalone Multi-Regime Free-Surface & Capillary Workbench modal"
            >
              🌊 Open Full Multi-Regime Workbench…
            </button>
          </div>
        )
      )}

      {/* 1A. Spurious Capillary Eddies & Surface-Tension Geometry Approximation Inspector */}
      {showSpuriousEddies && <SpuriousEddyExplorerPanel />}

      {/* 1B. Gerris Flow Solver (Popinet 2003 / 2009) 8-Case Analytic Verification Matrix & Profile Plotter */}
      {showGerris && gerrisSuite && (() => {
        const activeCase =
          gerrisSuite.cases.find((c) => c.id === selectedGerrisId) ?? gerrisSuite.cases[0];
        const samples = activeCase.profile_samples ?? [];
        const xs = samples.map((s) => s[0]);
        const yExact = samples.map((s) => s[1]);
        const yNum = samples.map((s) => s[2]);
        const xMin = xs.length > 0 ? Math.min(...xs) : 0;
        const xMax = xs.length > 0 ? Math.max(...xs) : 1;
        const allY = [...yExact, ...yNum];
        const yMinRaw = allY.length > 0 ? Math.min(...allY) : 0;
        const yMaxRaw = allY.length > 0 ? Math.max(...allY) : 1;
        const ySpan = Math.max(1e-9, yMaxRaw - yMinRaw);
        const yMin = yMinRaw - 0.1 * ySpan;
        const yMax = yMaxRaw + 0.1 * ySpan;
        const mapX = (x: number) =>
          44 + ((x - xMin) / Math.max(1e-9, xMax - xMin)) * 392;
        const mapY = (y: number) =>
          172 - ((y - yMin) / Math.max(1e-9, yMax - yMin)) * 140;
        const exactPts = xs
          .map((x: number, idx: number) => `${mapX(x).toFixed(1)},${mapY(yExact[idx]).toFixed(1)}`)
          .join(' ');
        const numPts = xs
          .map((x: number, idx: number) => `${mapX(x).toFixed(1)},${mapY(yNum[idx]).toFixed(1)}`)
          .join(' ');
        const meanErr =
          gerrisSuite.cases.reduce((acc, c) => acc + c.rel_error_pct, 0) /
          Math.max(1, gerrisSuite.cases.length);

        return (
          <div
            data-testid="gerris-suite-panel"
            style={{
              padding: '10px 12px',
              borderRadius: '7px',
              background: 'color-mix(in srgb, #06b6d4 10%, var(--panel))',
              border: '1px solid rgba(34, 211, 238, 0.45)',
              display: 'flex',
              flexDirection: 'column',
              gap: '8px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
              <div>
                <strong style={{ fontSize: '12.5px', color: '#22d3ee' }}>
                  📐 Gerris Flow Solver (Popinet 2003 / 2009) Analytic Verification Suite (G1–G8)
                </strong>
                <div style={{ fontSize: '11px', color: 'var(--text-2)' }}>
                  Click any benchmark row (<code>G1</code>–<code>G8</code>) to inspect its closed-form PDE, live 2D flow field, and analytical vs. numerical profile.
                </div>
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
                <button
                  type="button"
                  class="btn small"
                  data-testid="gerris-sync-3d-btn"
                  onClick={() => syncGerrisCaseToViewport(activeCase.id)}
                  style={{ borderColor: '#22d3ee', color: '#67e8f9', fontSize: '10.5px', padding: '3px 8px' }}
                  title="Build and solve the 3D CAD/FEM model for the selected Gerris benchmark in the 3D Viewport"
                >
                  📐 Sync {activeCase.id} ({activeCase.gerris_slug}) to 3D Viewport
                </button>
                <button
                  type="button"
                  class="btn small"
                  data-testid="gerris-open-coord-fem-btn"
                  onClick={() =>
                    openCoordMappingModal({
                      dimension: 2,
                      coordSystem:
                        activeCase.id === 'G1' || activeCase.id === 'G3' || activeCase.id === 'G6'
                          ? 'polar_cylindrical'
                          : activeCase.id === 'G8'
                            ? 'parabolic'
                            : 'cartesian',
                      pdeKind:
                        activeCase.id === 'G1'
                          ? 'biharmonic_eddy_stream'
                          : activeCase.id === 'G2' || activeCase.id === 'G3' || activeCase.id === 'G7'
                            ? 'helmholtz_screened'
                            : 'laplace_beltrami_poisson',
                      gridN: Math.min(40, Math.max(12, Math.round(gerrisGridN / 2))),
                    })
                  }
                  style={{ borderColor: '#a855f7', color: '#d8b4fe', fontSize: '10.5px', padding: '3px 8px' }}
                  title="Open this Gerris benchmark's coordinate chart and PDE operator in the 1D–4D Coordinate Mappings FEM Workbench"
                >
                  🌐 2D Coordinate FEM Suite →
                </button>
                <span
                  class="num"
                  data-testid="gerris-suite-summary-badge"
                  style={{
                    padding: '3px 9px',
                    borderRadius: '4px',
                    fontWeight: 700,
                    fontSize: '11px',
                    background: 'rgba(16, 185, 129, 0.2)',
                    color: '#34d399',
                    border: '1px solid rgba(16, 185, 129, 0.45)',
                  }}
                >
                  {gerrisSuite.passed_tests}/{gerrisSuite.total_tests} PASS · N={gerrisGridN} · La={gerrisLaplaceNumber} · Max Err {gerrisSuite.max_rel_error_pct.toFixed(3)}% · Mean Err {meanErr.toFixed(3)}%
                </span>
              </div>
            </div>

            {/* Popinet Parametric Grid Resolution N & Laplace Number La Selector Bar + Sliders */}
            <div
              data-testid="gerris-parametric-controls"
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
                gap: '10px',
                padding: '7px 10px',
                borderRadius: '6px',
                background: 'var(--bg)',
                border: '1px solid var(--line)',
                fontSize: '11px',
              }}
            >
              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '5px', flexWrap: 'wrap' }}>
                  <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                    Quadtree / Octree Grid Resolution N: <strong class="num" style={{ color: '#22d3ee' }}>{gerrisGridN} cells</strong>
                  </span>
                  <div style={{ display: 'flex', gap: '4px' }}>
                    {[32, 64, 96, 128].map((nVal) => (
                      <button
                        key={nVal}
                        type="button"
                        data-testid={`gerris-grid-n-${nVal}`}
                        class={`btn small ${gerrisGridN === nVal ? 'primary' : 'ghost'}`}
                        onClick={() => setGerrisGridN(nVal)}
                        style={{ padding: '2px 6px', fontSize: '10px', fontFamily: 'var(--mono)' }}
                      >
                        N={nVal}
                      </button>
                    ))}
                  </div>
                </div>
                <input
                  type="range"
                  min="16"
                  max="128"
                  step="4"
                  value={gerrisGridN}
                  data-testid="gerris-grid-n-slider"
                  onInput={(e) => setGerrisGridN(Number((e.target as HTMLInputElement).value))}
                />
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '5px', flexWrap: 'wrap' }}>
                  <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                    Capillary Laplace Number La = ργD/μ²: <strong class="num" style={{ color: '#34d399' }}>{gerrisLaplaceNumber.toLocaleString()}</strong>
                  </span>
                  <div style={{ display: 'flex', gap: '4px' }}>
                    {[120, 1200, 12000, 50000].map((laVal) => (
                      <button
                        key={laVal}
                        type="button"
                        data-testid={`gerris-la-${laVal}`}
                        class={`btn small ${gerrisLaplaceNumber === laVal ? 'primary' : 'ghost'}`}
                        onClick={() => setGerrisLaplaceNumber(laVal)}
                        style={{ padding: '2px 6px', fontSize: '10px', fontFamily: 'var(--mono)' }}
                      >
                        La={laVal.toLocaleString()}
                      </button>
                    ))}
                  </div>
                </div>
                <input
                  type="range"
                  min="100"
                  max="60000"
                  step="100"
                  value={gerrisLaplaceNumber}
                  data-testid="gerris-la-slider"
                  onInput={(e) => setGerrisLaplaceNumber(Number((e.target as HTMLInputElement).value))}
                />
              </div>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: embedded ? '1fr' : 'repeat(auto-fit, minmax(320px, 1fr))',
                gap: '10px',
              }}
            >
              {/* Left: 8-Row Gerris Benchmark Matrix */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                {gerrisSuite.cases.map((c) => {
                  const isSel = c.id === activeCase.id;
                  return (
                    <button
                      key={c.id}
                      type="button"
                      data-testid={`gerris-row-${c.id}`}
                      onClick={() => setSelectedGerrisId(c.id)}
                      style={{
                        textAlign: 'left',
                        padding: '6px 8px',
                        borderRadius: '5px',
                        cursor: 'pointer',
                        background: isSel ? 'rgba(34, 211, 238, 0.14)' : 'var(--bg)',
                        border: isSel ? '1px solid #22d3ee' : '1px solid var(--line)',
                        color: 'var(--text)',
                        display: 'flex',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                        gap: '6px',
                      }}
                    >
                      <div style={{ minWidth: 0 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                          <strong style={{ color: '#22d3ee', fontFamily: 'var(--mono)' }}>{c.id}</strong>
                          <code style={{ fontSize: '10.5px', color: 'var(--text-3)' }}>gerris/test/{c.gerris_slug}</code>
                          <span style={{ fontWeight: 600, fontSize: '11px' }}>{c.title.replace(/^G\d+\s*\([^)]+\):\s*/, '')}</span>
                        </div>
                        <div class="num" style={{ fontSize: '10px', color: 'var(--text-3)', marginTop: '2px' }}>
                          Ref: {c.analytic_reference.toFixed(4)} {c.unit} · Fieldsmith: {c.fieldsmith_measured.toFixed(4)} {c.unit}
                        </div>
                      </div>
                      <span
                        class="num"
                        style={{
                          padding: '2px 6px',
                          borderRadius: '4px',
                          fontSize: '10px',
                          fontWeight: 700,
                          whiteSpace: 'nowrap',
                          background: c.passed ? 'rgba(16, 185, 129, 0.18)' : 'rgba(239, 68, 68, 0.18)',
                          color: c.passed ? '#34d399' : '#f87171',
                        }}
                      >
                        {c.passed ? 'PASS' : 'FAIL'} ({c.rel_error_pct.toFixed(3)}%)
                      </span>
                    </button>
                  );
                })}
              </div>

              {/* Right: Selected Gerris Case 2D Spatial Flow Field + 1D Exact vs Numerical Curve */}
              <div
                style={{
                  padding: '8px 10px',
                  borderRadius: '6px',
                  background: 'var(--bg)',
                  border: '1px solid var(--line)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '4px' }}>
                  <strong data-testid="gerris-active-title" style={{ fontSize: '12px' }}>
                    {activeCase.title}
                  </strong>
                  <span class="sys-badge" style={{ color: regimeColor(activeCase.regime_code) }}>
                    {activeCase.regime_code}
                  </span>
                </div>
                <div style={{ fontSize: '10.5px', color: 'var(--text-3)' }}>
                  Citation: <em>{activeCase.citation}</em>
                </div>
                <div
                  class="num"
                  style={{
                    fontFamily: 'var(--mono)',
                    fontSize: '10.5px',
                    padding: '4px 7px',
                    borderRadius: '4px',
                    background: 'var(--panel)',
                    border: '1px solid var(--line)',
                  }}
                >
                  {activeCase.closed_form_equation}
                </div>

                {/* Live 2D Physical Flow-Field, VOF Interface & Quadtree Mesh Visualization for G1–G8 */}
                {(() => {
                  const nCellsVis = Math.max(6, Math.min(20, Math.round(gerrisGridN / 6)));
                  const phaseRad = 2 * Math.PI * simPhase;
                  const dampLa = Math.pow(12000 / Math.max(100, gerrisLaplaceNumber), 0.22);
                  const samples = activeCase.profile_samples ?? [];
                  const xs = samples.map((s) => s[0]);
                  const ys = samples.flatMap((s) => [s[1], s[2]]);
                  const xMin = xs.length ? Math.min(...xs) : 0;
                  const xMax = xs.length ? Math.max(...xs) : 1;
                  const yMin = ys.length ? Math.min(...ys) : 0;
                  const yMax = ys.length ? Math.max(...ys) : 1;
                  const xSpan = Math.max(1e-9, xMax - xMin);
                  const ySpan = Math.max(1e-9, yMax - yMin);

                  return (
                    <svg
                      viewBox="0 0 455 170"
                      data-testid="gerris-2d-field-svg"
                      style={{
                        width: '100%',
                        height: 'auto',
                        background: '#090f1d',
                        borderRadius: '6px',
                        border: '1px solid rgba(34, 211, 238, 0.35)',
                      }}
                    >
                      {/* Background Quadtree Grid scaled by gerrisGridN */}
                      {Array.from({ length: nCellsVis + 1 }, (_, gi) => {
                        const gx = 24 + (gi / nCellsVis) * 408;
                        const gy = 18 + (gi / nCellsVis) * 134;
                        return (
                          <g key={gi}>
                            <line x1={gx} y1="18" x2={gx} y2="152" stroke="#16213a" strokeWidth="0.8" />
                            <line x1="24" y1={gy} x2="432" y2={gy} stroke="#16213a" strokeWidth="0.8" />
                          </g>
                        );
                      })}

                      {activeCase.id === 'G1' && (() => {
                        const cx = 228;
                        const cy = 85;
                        const rDrop = 48;
                        const ripple = 2.4 * Math.pow(32 / Math.max(16, gerrisGridN), 1.4) * dampLa;
                        const polyPts = Array.from({ length: 48 }, (_, k) => {
                          const th = (2 * Math.PI * k) / 48;
                          const rLoc = rDrop + ripple * Math.cos(4 * th);
                          return `${(cx + rLoc * Math.cos(th)).toFixed(1)},${(cy + rLoc * Math.sin(th)).toFixed(1)}`;
                        }).join(' ');
                        return (
                          <g>
                            <circle cx={cx} cy={cy} r={rDrop} fill="rgba(14, 165, 233, 0.18)" stroke="#38bdf8" strokeWidth="1.2" strokeDasharray="4,3" />
                            <polygon points={polyPts} fill="rgba(16, 185, 129, 0.14)" stroke="#34d399" strokeWidth="2" />
                            {/* Plot actual profile_samples radial jump points along horizontal cut */}
                            {samples.map(([rOverR0, pEx, pNum], si) => {
                              const px = cx + (rOverR0 / Math.max(1.5, xMax)) * (rDrop * 1.65);
                              const py = cy + 22 - ((pNum - yMin) / ySpan) * 44;
                              const pyEx = cy + 22 - ((pEx - yMin) / ySpan) * 44;
                              return (
                                <g key={si}>
                                  <circle cx={px} cy={pyEx} r="1.8" fill="#38bdf8" />
                                  <circle cx={px} cy={py} r="2.6" fill="#fbbf24" />
                                </g>
                              );
                            })}
                            {/* 8 quadrupolar parasitic capillary vortices around r = R0 */}
                            {Array.from({ length: 8 }, (_, m) => {
                              const th0 = ((m + 0.5) * Math.PI) / 4;
                              const sign = m % 2 === 0 ? 1 : -1;
                              const vx = cx + (rDrop + 6) * Math.cos(th0);
                              const vy = cy + (rDrop + 6) * Math.sin(th0);
                              const rot = th0 + sign * phaseRad;
                              const rVort = 11 * Math.max(0.35, Math.min(1.35, (32 / gerrisGridN) * dampLa));
                              const tx = vx + rVort * 0.82 * Math.cos(rot);
                              const ty = vy + rVort * 0.82 * Math.sin(rot);
                              return (
                                <g key={m}>
                                  <circle
                                    cx={vx}
                                    cy={vy}
                                    r={rVort}
                                    fill="none"
                                    stroke={sign > 0 ? '#f43f5e' : '#a855f7'}
                                    strokeWidth="1.3"
                                    strokeDasharray="3,2"
                                  />
                                  <circle cx={tx} cy={ty} r="2.3" fill={sign > 0 ? '#fb7185' : '#c084fc'} />
                                </g>
                              );
                            })}
                            <text x="32" y="32" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D VOF Cut-Cell Droplet (N={gerrisGridN}, Δp={activeCase.fieldsmith_measured.toFixed(2)} Pa)
                            </text>
                            <text x="32" y="146" fill="#34d399" fontSize="9" fontFamily="monospace">
                              8 Quadrupolar Spurious Eddies · La={gerrisLaplaceNumber} · Err={activeCase.rel_error_pct.toFixed(4)}%
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G2' && (() => {
                        const env = Math.exp(-0.22 * dampLa * simPhase) * Math.cos(phaseRad);
                        const ampPx = 28 * env * (activeCase.fieldsmith_measured || 0.87);
                        const surfPts = Array.from({ length: 49 }, (_, k) => {
                          const xNorm = k / 48;
                          const xPx = 28 + xNorm * 400;
                          const yPx = 78 - ampPx * Math.cos(2 * Math.PI * 2 * xNorm);
                          return `${xPx.toFixed(1)},${yPx.toFixed(1)}`;
                        }).join(' ');
                        const samplePts = samples
                          .map(([tNorm, , aNum]) => {
                            const sx = 28 + ((tNorm - xMin) / xSpan) * 400;
                            const sy = 78 - ((aNum - 0) / Math.max(0.5, ySpan)) * 28;
                            return `${sx.toFixed(1)},${sy.toFixed(1)}`;
                          })
                          .join(' ');
                        return (
                          <g>
                            <polygon
                              points={`28,150 ${surfPts} 428,150`}
                              fill="rgba(14, 165, 233, 0.22)"
                            />
                            <polyline fill="none" stroke="#38bdf8" strokeWidth="2.4" points={surfPts} />
                            <polyline fill="none" stroke="#fbbf24" strokeWidth="1.5" strokeDasharray="3,2" points={samplePts} />
                            {samples.map(([tNorm, , aNum], si) => {
                              const sx = 28 + ((tNorm - xMin) / xSpan) * 400;
                              const sy = 78 - (aNum / Math.max(0.5, ySpan)) * 28;
                              return <circle key={si} cx={sx} cy={sy} r="2.2" fill="#fbbf24" />;
                            })}
                            {/* Sub-surface Stokes vorticity boundary-layer rollers */}
                            {[0.125, 0.375, 0.625, 0.875].map((xf, idx) => {
                              const cx = 28 + xf * 400;
                              const sign = idx % 2 === 0 ? 1 : -1;
                              return (
                                <g key={idx}>
                                  <ellipse
                                    cx={cx}
                                    cy={98}
                                    rx="26"
                                    ry={Math.max(7, Math.min(20, 12 * dampLa))}
                                    fill="none"
                                    stroke={sign > 0 ? '#f43f5e' : '#a855f7'}
                                    strokeWidth="1.4"
                                    strokeDasharray="4,2"
                                  />
                                  <circle
                                    cx={cx + 16 * Math.cos(sign * phaseRad)}
                                    cy={98 + 8 * Math.sin(sign * phaseRad)}
                                    r="2.5"
                                    fill="#34d399"
                                  />
                                </g>
                              );
                            })}
                            <text x="32" y="30" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Prosperetti Standing Capillary Wave η(x,t) &amp; Vorticity Boundary Layer ω(x,z)
                            </text>
                            <text x="32" y="146" fill="#34d399" fontSize="9" fontFamily="monospace">
                              N_z={gerrisGridN} · La={gerrisLaplaceNumber} · a(2T₀)/a₀={activeCase.fieldsmith_measured.toFixed(4)}
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G3' && (() => {
                        const modeSamples = samples.length > 0 ? samples : [[2, 270, 270]];
                        return (
                          <g>
                            {modeSamples.map(([mVal, omegaEx, omegaNum], idx) => {
                              const m = Math.round(mVal);
                              const cx = 68 + idx * 80;
                              const cy = 88;
                              const rBase = 24 + (omegaNum / Math.max(100, yMax)) * 6;
                              const osc = (0.24 / Math.max(0.6, dampLa)) * Math.cos(phaseRad * (omegaNum / Math.max(1, omegaEx)) * Math.sqrt((m * (m - 1) * (m + 2)) / 8));
                              const pts = Array.from({ length: 48 }, (_, k) => {
                                const th = (2 * Math.PI * k) / 48;
                                const rLoc = rBase * (1 + osc * Math.cos(m * th));
                                return `${(cx + rLoc * Math.cos(th)).toFixed(1)},${(cy + rLoc * Math.sin(th)).toFixed(1)}`;
                              }).join(' ');
                              return (
                                <g key={m}>
                                  <circle cx={cx} cy={cy} r={rBase} fill="none" stroke="#475569" strokeWidth="1" strokeDasharray="2,2" />
                                  <polygon points={pts} fill="rgba(52, 211, 153, 0.18)" stroke="#34d399" strokeWidth="1.8" />
                                  <circle cx={cx} cy={cy - rBase} r="2.2" fill="#fbbf24" />
                                  <text x={cx - 22} y="138" fill="#cbd5e1" fontSize="8.5" fontWeight="bold" fontFamily="monospace">
                                    n={m} ({omegaNum.toFixed(0)})
                                  </text>
                                </g>
                              );
                            })}
                            <text x="32" y="30" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Rayleigh–Lamb Eigenmodes (N={gerrisGridN}, La={gerrisLaplaceNumber}, T₂={activeCase.fieldsmith_measured.toFixed(2)} ms)
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G4' && (() => {
                        const prof = gerrisSuite.eggers_pinchoff_profile ?? [];
                        const hMin = gerrisSuite.eggers_min_neck_radius_over_r0 ?? 0.014;
                        const rSat = gerrisSuite.eggers_satellite_drop_radius_over_r0 ?? 0.295;
                        const pinchFrac = 0.35 + 0.65 * (0.5 - 0.5 * Math.cos(phaseRad));
                        const topPts = prof
                          .map((p) => {
                            const x = 32 + p.z_over_lambda * 392;
                            const hCur =
                              (1 - pinchFrac) * p.h_initial_over_r0 + pinchFrac * p.h_pinchoff_over_r0;
                            const y = 85 - hCur * 38;
                            return `${x.toFixed(1)},${y.toFixed(1)}`;
                          })
                          .join(' ');
                        const botPts = prof
                          .slice()
                          .reverse()
                          .map((p) => {
                            const x = 32 + p.z_over_lambda * 392;
                            const hCur =
                              (1 - pinchFrac) * p.h_initial_over_r0 + pinchFrac * p.h_pinchoff_over_r0;
                            const y = 85 + hCur * 38;
                            return `${x.toFixed(1)},${y.toFixed(1)}`;
                          })
                          .join(' ');
                        return (
                          <g>
                            <line x1="28" y1="85" x2="428" y2="85" stroke="#475569" strokeWidth="1" strokeDasharray="3,3" />
                            <polygon points={`${topPts} ${botPts}`} fill="rgba(244, 63, 94, 0.25)" stroke="#fb7185" strokeWidth="2" />
                            {samples.map(([xWave, , wNum], si) => {
                              const sx = 32 + ((xWave - xMin) / xSpan) * 392;
                              const sy = 144 - ((wNum - yMin) / ySpan) * 34;
                              return <circle key={si} cx={sx} cy={sy} r="2.2" fill="#34d399" />;
                            })}
                            <circle cx={32 + 0.28 * 392} cy={85 - hMin * 38} r="3.5" fill="#fbbf24" />
                            <circle cx={32 + 0.72 * 392} cy={85 - hMin * 38} r="3.5" fill="#fbbf24" />
                            <circle cx={32 + 0.5 * 392} cy={85 - rSat * 38} r="4" fill="#38bdf8" />
                            <text x="32" y="28" fill="#fb7185" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Eggers Pinch-Off (N={gerrisGridN}, La={gerrisLaplaceNumber}): h_min/R₀={hMin.toFixed(4)}, R_sat/R₀={rSat.toFixed(3)}
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G5' && (() => {
                        const rowSamples = samples.length > 0 ? samples : [[0.5, 0.375, 0.375]];
                        const polyNum = rowSamples
                          .map(([zStar, , uNum]) => `${(90 + uNum * 220 * (1 / Math.max(0.5, dampLa * 0.8 + 0.2))).toFixed(1)},${(136 - zStar * 94).toFixed(1)}`)
                          .join(' ');
                        return (
                          <g>
                            {/* Inclined bed at bottom y=136 and free surface at y=42 */}
                            <rect x="32" y="136" width="392" height="10" fill="#475569" />
                            <rect x="32" y="42" width="392" height="94" fill="rgba(14, 165, 233, 0.16)" />
                            <line x1="32" y1="42" x2="424" y2="42" stroke="#38bdf8" strokeWidth="2.2" />
                            <polyline fill="none" stroke="#fbbf24" strokeWidth="1.6" strokeDasharray="3,2" points={polyNum} />
                            {/* Solved profile_samples velocity vectors u*(z*) and moving tracers */}
                            {rowSamples.map(([zStar, uEx, uNum], ri) => {
                              const y = 136 - zStar * 94;
                              const xHead = 90 + uNum * 220;
                              const xHeadEx = 90 + uEx * 220;
                              const tracerX = 40 + ((simPhase * Math.max(0.05, uNum) * (720 / dampLa) + ri * 38) % 375);
                              return (
                                <g key={ri}>
                                  <line x1="90" y1={y} x2={xHead} y2={y} stroke="#34d399" strokeWidth="1.6" />
                                  <circle cx={xHeadEx} cy={y} r="2.0" fill="#38bdf8" />
                                  <circle cx={xHead} cy={y} r="2.6" fill="#fbbf24" />
                                  <circle cx={tracerX} cy={y} r="2.8" fill="#38bdf8" />
                                </g>
                              );
                            })}
                            <text x="36" y="30" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Nusselt Film (N={gerrisGridN}, La={gerrisLaplaceNumber}) · Flux Q* = {activeCase.fieldsmith_measured.toFixed(5)}
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G6' && (() => {
                        const cx = 228;
                        const cy = 85;
                        const r1 = 26;
                        const r2 = 62;
                        return (
                          <g>
                            <circle cx={cx} cy={cy} r={r2} fill="rgba(14, 165, 233, 0.14)" stroke="#38bdf8" strokeWidth="2" />
                            <circle cx={cx} cy={cy} r={r1} fill="#1e293b" stroke="#fbbf24" strokeWidth="2.2" />
                            {samples.map(([rNorm, uEx, uNum], ri) => {
                              const rf = Math.max(0, Math.min(1, rNorm - 1.0));
                              const r = r1 + rf * (r2 - r1);
                              return (
                                <g key={ri}>
                                  <circle cx={cx + r} cy={cy - uEx * 36} r="2.0" fill="#38bdf8" />
                                  <circle cx={cx + r} cy={cy - uNum * 36} r="2.6" fill="#fbbf24" />
                                  {ri % 2 === 1 && ri < samples.length - 1 && (
                                    <g>
                                      <circle cx={cx} cy={cy} r={r} fill="none" stroke="#334155" strokeWidth="0.8" strokeDasharray="3,3" />
                                      {Array.from({ length: 6 }, (_, k) => {
                                        const th = (2 * Math.PI * k) / 6 + (phaseRad * uNum) / Math.max(0.5, dampLa);
                                        return (
                                          <circle
                                            key={k}
                                            cx={cx + r * Math.cos(th)}
                                            cy={cy + r * Math.sin(th)}
                                            r="2.4"
                                            fill="#34d399"
                                          />
                                        );
                                      })}
                                    </g>
                                  )}
                                </g>
                              );
                            })}
                            <text x="32" y="30" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Cut-Cell Taylor–Couette (N={gerrisGridN}, La={gerrisLaplaceNumber}) · u_θ(1.5R₁)={activeCase.fieldsmith_measured.toFixed(5)}
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G7' && (() => {
                        const centers = [
                          { cx: 145, cy: 56, sign: 1 },
                          { cx: 310, cy: 56, sign: -1 },
                          { cx: 145, cy: 116, sign: -1 },
                          { cx: 310, cy: 116, sign: 1 },
                        ];
                        const decayPts = samples
                          .map(([tStar, , eNum]) => {
                            const sx = 36 + ((tStar - xMin) / xSpan) * 384;
                            const sy = 144 - ((eNum - yMin) / ySpan) * 96;
                            return `${sx.toFixed(1)},${sy.toFixed(1)}`;
                          })
                          .join(' ');
                        return (
                          <g>
                            {centers.map((v, vi) => (
                              <g key={vi}>
                                {[12, 22, 32].map((r, ri) => (
                                  <ellipse
                                    key={ri}
                                    cx={v.cx}
                                    cy={v.cy}
                                    rx={r * 1.35 * Math.max(0.65, 1 / dampLa)}
                                    ry={r * 0.78 * Math.max(0.65, 1 / dampLa)}
                                    fill="none"
                                    stroke={v.sign > 0 ? '#38bdf8' : '#f43f5e'}
                                    strokeWidth="1.4"
                                    strokeOpacity={0.85 - ri * 0.2}
                                  />
                                ))}
                                {Array.from({ length: 4 }, (_, k) => {
                                  const th = (2 * Math.PI * k) / 4 + (v.sign * phaseRad) / dampLa;
                                  return (
                                    <circle
                                      key={k}
                                      cx={v.cx + 24 * Math.cos(th)}
                                      cy={v.cy + 14 * Math.sin(th)}
                                      r="2.6"
                                      fill={v.sign > 0 ? '#38bdf8' : '#fb7185'}
                                    />
                                  );
                                })}
                              </g>
                            ))}
                            <polyline fill="none" stroke="#34d399" strokeWidth="1.8" strokeDasharray="4,2" points={decayPts} />
                            {samples.map(([tStar, , eNum], si) => {
                              const sx = 36 + ((tStar - xMin) / xSpan) * 384;
                              const sy = 144 - ((eNum - yMin) / ySpan) * 96;
                              return <circle key={si} cx={sx} cy={sy} r="2.3" fill="#fbbf24" />;
                            })}
                            <text x="32" y="28" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Taylor–Green 4-Vortex Decay (N={gerrisGridN}, La={gerrisLaplaceNumber}) · E(1)/E₀ = {activeCase.fieldsmith_measured.toFixed(5)}
                            </text>
                          </g>
                        );
                      })()}

                      {activeCase.id === 'G8' && (() => {
                        const xiNorm = Math.cos(phaseRad) * Math.exp(-0.08 * dampLa);
                        const bowlPts = Array.from({ length: 49 }, (_, k) => {
                          const xNorm = -1.5 + (3.0 * k) / 48; // [-1.5, 1.5]
                          const xPx = 228 + xNorm * 125;
                          const zb = 0.5 * (xNorm * xNorm - 1.0);
                          const yPx = 96 - zb * 72;
                          return `${xPx.toFixed(1)},${yPx.toFixed(1)}`;
                        }).join(' ');
                        const xLeft = -1.0 + 0.2 * xiNorm;
                        const xRight = 1.0 + 0.2 * xiNorm;
                        const wetPts = Array.from({ length: 33 }, (_, k) => {
                          const xNorm = xLeft + ((xRight - xLeft) * k) / 32;
                          const xPx = 228 + xNorm * 125;
                          const zb = 0.5 * (xNorm * xNorm - 1.0);
                          const yPx = 96 - zb * 72;
                          return `${xPx.toFixed(1)},${yPx.toFixed(1)}`;
                        }).join(' ');
                        const yLeft = 96 - 0.5 * (xLeft * xLeft - 1.0) * 72;
                        const yRight = 96 - 0.5 * (xRight * xRight - 1.0) * 72;
                        return (
                          <g>
                            <polygon
                              points={`${(228 + xLeft * 125).toFixed(1)},${yLeft.toFixed(1)} ${wetPts} ${(228 + xRight * 125).toFixed(1)},${yRight.toFixed(1)}`}
                              fill="rgba(14, 165, 233, 0.28)"
                              stroke="#38bdf8"
                              strokeWidth="2"
                            />
                            <polyline fill="none" stroke="#94a3b8" strokeWidth="2.2" points={bowlPts} />
                            {samples.map(([tOverT, etaEx, etaNum], si) => {
                              const sx = 45 + ((tOverT - xMin) / xSpan) * 366;
                              const syEx = 96 - etaEx * 72;
                              const syNum = 96 - etaNum * 72;
                              return (
                                <g key={si}>
                                  <circle cx={sx} cy={syEx} r="1.8" fill="#38bdf8" />
                                  <circle cx={sx} cy={syNum} r="2.4" fill="#fbbf24" />
                                </g>
                              );
                            })}
                            <circle cx={228 + xLeft * 125} cy={yLeft} r="4" fill="#fbbf24" />
                            <circle cx={228 + xRight * 125} cy={yRight} r="4" fill="#34d399" />
                            <text x="32" y="28" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                              2D Thacker Parabolic Bowl (N={gerrisGridN}, La={gerrisLaplaceNumber}) · T={activeCase.fieldsmith_measured.toFixed(4)} s
                            </text>
                          </g>
                        );
                      })()}
                    </svg>
                  );
                })()}

                <svg
                  viewBox="0 0 455 198"
                  data-testid="gerris-profile-svg"
                  style={{
                    width: '100%',
                    height: 'auto',
                    background: '#0f172a',
                    borderRadius: '6px',
                    border: '1px solid var(--line)',
                  }}
                >
                  {/* Grid lines */}
                  {[0, 0.25, 0.5, 0.75, 1.0].map((frac, gi) => {
                    const y = 32 + frac * 140;
                    const val = yMax - frac * (yMax - yMin);
                    return (
                      <g key={gi}>
                        <line x1="44" y1={y} x2="436" y2={y} stroke="#1e293b" strokeWidth="1" />
                        <text x="6" y={y + 3} fill="#94a3b8" fontSize="8.5" fontFamily="monospace">
                          {val.toFixed(2)}
                        </text>
                      </g>
                    );
                  })}
                  {/* Analytic reference curve */}
                  <polyline
                    fill="none"
                    stroke="#38bdf8"
                    strokeWidth="2.2"
                    strokeDasharray="5,3"
                    points={exactPts}
                  />
                  {/* Fieldsmith numerical curve */}
                  <polyline
                    fill="none"
                    stroke="#34d399"
                    strokeWidth="2"
                    points={numPts}
                  />
                  {xs.map((x: number, idx: number) => (
                    <circle
                      key={idx}
                      cx={mapX(x)}
                      cy={mapY(yNum[idx])}
                      r="3"
                      fill="#34d399"
                      stroke="#0f172a"
                      strokeWidth="1"
                    />
                  ))}
                  {/* Legend & Axis labels */}
                  <text x="50" y="18" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                    --- Exact Analytic ({activeCase.metric_name})
                  </text>
                  <text x="255" y="18" fill="#34d399" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                    ●— Fieldsmith Numerical (Err {activeCase.rel_error_pct.toFixed(3)}%)
                  </text>
                  <text x="155" y="191" fill="#cbd5e1" fontSize="9" fontFamily="monospace">
                    gerris/test/{activeCase.gerris_slug} [{xMin.toFixed(2)} .. {xMax.toFixed(2)}]
                  </text>
                </svg>

                {/* Eggers (1993 / 1997) 1D Nonlinear Slender-Jet Pinch-Off Neck & Satellite Drop Profile (G4 / plateau) */}
                {(activeCase.id === 'G4' || activeCase.gerris_slug === 'plateau') &&
                  gerrisSuite.eggers_pinchoff_profile &&
                  gerrisSuite.eggers_pinchoff_profile.length > 0 &&
                  (() => {
                    const prof = gerrisSuite.eggers_pinchoff_profile!;
                    const hMinR0 = gerrisSuite.eggers_min_neck_radius_over_r0 ?? 0.018;
                    const rSatR0 = gerrisSuite.eggers_satellite_drop_radius_over_r0 ?? 0.29;
                    const mapZ = (z: number) => 44 + z * 392;
                    const mapHTop = (h: number) => 98 - (h / 1.55) * 68;
                    const mapHBot = (h: number) => 98 + (h / 1.55) * 68;
                    const initTopPts = prof
                      .map((p) => `${mapZ(p.z_over_lambda).toFixed(1)},${mapHTop(p.h_initial_over_r0).toFixed(1)}`)
                      .join(' ');
                    const pinchTopPts = prof
                      .map((p) => `${mapZ(p.z_over_lambda).toFixed(1)},${mapHTop(p.h_pinchoff_over_r0).toFixed(1)}`)
                      .join(' ');
                    const pinchBotPts = prof
                      .slice()
                      .reverse()
                      .map((p) => `${mapZ(p.z_over_lambda).toFixed(1)},${mapHBot(p.h_pinchoff_over_r0).toFixed(1)}`)
                      .join(' ');
                    const minPt = prof.reduce(
                      (best, p) => (p.h_pinchoff_over_r0 < best.h_pinchoff_over_r0 ? p : best),
                      prof[0],
                    );

                    return (
                      <div
                        data-testid="gerris-eggers-pinchoff-card"
                        style={{
                          marginTop: '4px',
                          padding: '7px 9px',
                          borderRadius: '6px',
                          background: 'rgba(244, 63, 94, 0.08)',
                          border: '1px solid rgba(244, 63, 94, 0.35)',
                          display: 'flex',
                          flexDirection: 'column',
                          gap: '5px',
                        }}
                      >
                        <div
                          style={{
                            display: 'flex',
                            justifyContent: 'space-between',
                            alignItems: 'center',
                            flexWrap: 'wrap',
                            gap: '4px',
                            fontSize: '11px',
                          }}
                        >
                          <strong style={{ color: '#fb7185' }}>
                            🩸 Eggers (1993 / 1997) 1D Nonlinear Slender-Jet Pinch-Off &amp; Satellite Bulb Profile h(z/λ, t_pinch)
                          </strong>
                          <span class="num" style={{ fontSize: '10.5px', color: '#fda4af' }}>
                            h_min/R₀ = <strong data-testid="eggers-hmin-val">{hMinR0.toFixed(4)}</strong> · R_sat/R₀ ={' '}
                            <strong data-testid="eggers-rsat-val">{rSatR0.toFixed(3)}</strong>
                          </span>
                        </div>
                        <svg
                          viewBox="0 0 455 186"
                          data-testid="gerris-eggers-pinchoff-svg"
                          style={{
                            width: '100%',
                            height: 'auto',
                            background: '#090d16',
                            borderRadius: '5px',
                            border: '1px solid var(--line)',
                          }}
                        >
                          {/* Axis centerline z */}
                          <line x1="44" y1="98" x2="436" y2="98" stroke="#334155" strokeWidth="1" strokeDasharray="3,3" />
                          {/* Initial sinusoidal perturbation h(z, 0)/R0 */}
                          <polyline
                            fill="none"
                            stroke="#94a3b8"
                            strokeWidth="1.4"
                            strokeDasharray="4,3"
                            points={initTopPts}
                          />
                          {/* Asymmetric self-similar pinch-off jet envelope +/- h(z, t_pinch)/R0 */}
                          <polygon
                            points={`${pinchTopPts} ${pinchBotPts}`}
                            fill="rgba(244, 63, 94, 0.26)"
                            stroke="#fb7185"
                            strokeWidth="1.8"
                          />
                          {/* Highlight symmetric pinch-off neck minimum h_min */}
                          <circle
                            cx={mapZ(minPt.z_over_lambda)}
                            cy={mapHTop(minPt.h_pinchoff_over_r0)}
                            r="4"
                            fill="#fbbf24"
                            stroke="#090d16"
                            strokeWidth="1.2"
                          />
                          <circle
                            cx={mapZ(1.0 - minPt.z_over_lambda)}
                            cy={mapHTop(minPt.h_pinchoff_over_r0)}
                            r="4"
                            fill="#fbbf24"
                            stroke="#090d16"
                            strokeWidth="1.2"
                          />
                          <text
                            x={Math.max(48, Math.min(285, mapZ(minPt.z_over_lambda) - 30))}
                            y={mapHTop(minPt.h_pinchoff_over_r0) - 8}
                            fill="#fbbf24"
                            fontSize="8.5"
                            fontWeight="bold"
                            fontFamily="monospace"
                          >
                            Self-Similar Necks h_min/R₀ = {hMinR0.toFixed(4)}
                          </text>
                          {/* Highlight central satellite bulb crest at z/lambda = 0.5 */}
                          <circle cx={mapZ(0.5)} cy={mapHTop(rSatR0)} r="4" fill="#38bdf8" stroke="#090d16" strokeWidth="1" />
                          <text x={mapZ(0.5) - 62} y={Math.max(20, mapHTop(rSatR0) - 8)} fill="#38bdf8" fontSize="9" fontWeight="bold" fontFamily="monospace">
                            Satellite Bulb R_sat/R₀ = {rSatR0.toFixed(3)}
                          </text>
                          <text x="50" y="176" fill="#cbd5e1" fontSize="8.5" fontFamily="monospace">
                            Eggers Universal Scaling: ∂h/∂t + u ∂h/∂z = −½ h ∂u/∂z · z/λ ∈ [0, 1]
                          </text>
                        </svg>
                      </div>
                    );
                  })()}

                <div style={{ fontSize: '10.5px', color: 'var(--text-2)', lineHeight: 1.45 }}>
                  {activeCase.secondary_summary}
                </div>
              </div>
            </div>
          </div>
        );
      })()}

      {!showGerris && !showSpuriousEddies && (
        <>
          {/* 2. Automatic Dimensionless State Classifier Banner */}
          <div
            data-testid="sph-regime-banner"
        style={{
          padding: '9px 12px',
          borderRadius: '7px',
          background: `color-mix(in srgb, ${badgeColor} 11%, var(--panel))`,
          border: `1px solid color-mix(in srgb, ${badgeColor} 45%, var(--line))`,
          display: 'flex',
          flexDirection: 'column',
          gap: '5px',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
            <span
              style={{
                padding: '2px 8px',
                borderRadius: '4px',
                fontWeight: 700,
                fontSize: '10.5px',
                background: badgeColor,
                color: '#fff',
              }}
            >
              AUTO-SWITCHED REGIME
            </span>
            <strong data-testid="sph-primary-regime-title" style={{ fontSize: '12.5px' }}>
              {rep.primary_regime_title}
            </strong>
          </div>
          <span class="num" style={{ fontSize: '10.5px', color: 'var(--text-3)' }}>
            {rep.particle_count} 3D nodes · h = {rep.smoothing_length_mm.toFixed(2)} mm · {bundle.wall_ms.toFixed(1)} ms
          </span>
        </div>
        <div
          class="num"
          style={{
            fontFamily: 'var(--mono)',
            fontSize: '11px',
            padding: '4px 8px',
            borderRadius: '4px',
            background: 'var(--bg)',
            border: '1px solid var(--line)',
            color: 'var(--text)',
          }}
        >
          {rep.primary_governing_operator}
        </div>
        {/* Dimensionless Groups Strip */}
        <div
          class="num"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(115px, 1fr))',
            gap: '6px',
            marginTop: '2px',
            fontSize: '11px',
          }}
        >
          <div>
            <span style={{ color: 'var(--text-3)' }}>Reynolds Re: </span>
            <strong data-testid="sph-re-val">
              {rep.reynolds_number < 0.1 || rep.reynolds_number > 1e4
                ? rep.reynolds_number.toExponential(2)
                : rep.reynolds_number.toFixed(2)}
            </strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Ohnesorge Oh: </span>
            <strong data-testid="sph-oh-val">
              {rep.ohnesorge_number < 0.01
                ? rep.ohnesorge_number.toExponential(2)
                : rep.ohnesorge_number.toFixed(2)}
            </strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Weber We: </span>
            <strong>
              {rep.weber_number < 0.01
                ? rep.weber_number.toExponential(2)
                : rep.weber_number.toFixed(2)}
            </strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Bond Bo: </span>
            <strong>{rep.bond_number.toFixed(3)}</strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Capillary Ca: </span>
            <strong>
              {rep.capillary_number < 0.01
                ? rep.capillary_number.toExponential(2)
                : rep.capillary_number.toFixed(2)}
            </strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Marangoni Ma: </span>
            <strong>{rep.marangoni_number.toExponential(2)}</strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Density Ratio ρ_l/ρ_g: </span>
            <strong data-testid="sph-density-ratio-val">
              {(spec.density_kg_m3 / Math.max(0.05, spec.gas_density_kg_m3 ?? 1.225)).toFixed(0)}
            </strong>
          </div>
          <div>
            <span style={{ color: 'var(--text-3)' }}>Viscosity Ratio μ_l/μ_g: </span>
            <strong data-testid="sph-viscosity-ratio-val">
              {(() => {
                const muRatio = spec.viscosity_pa_s / Math.max(1e-7, spec.gas_viscosity_pa_s ?? 1.81e-5);
                return muRatio >= 1e4 ? muRatio.toExponential(2) : muRatio.toFixed(1);
              })()}
            </strong>
          </div>
        </div>
      </div>

      {/* 3. Multi-Regime Spatial/Temporal Zone Partition Table */}
      <div
        style={{
          padding: '8px 10px',
          borderRadius: '6px',
          background: 'var(--panel)',
          border: '1px solid var(--line)',
        }}
      >
        <div style={{ fontWeight: 700, fontSize: '11.5px', marginBottom: '6px' }}>
          Multi-Regime Spatial / Temporal Partition (Automatic Formulation Switching Across Domain)
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
          {rep.regime_zones.map((z, idx) => {
            const zColor = regimeColor(z.regime_code);
            return (
              <div
                key={idx}
                data-testid={`sph-zone-row-${idx}`}
                style={{
                  padding: '6px 9px',
                  borderRadius: '5px',
                  background: 'var(--bg)',
                  borderLeft: `4px solid ${zColor}`,
                  borderTop: '1px solid var(--line)',
                  borderRight: '1px solid var(--line)',
                  borderBottom: '1px solid var(--line)',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '4px' }}>
                  <strong>{z.zone_name}</strong>
                  <span class="num" style={{ fontSize: '10.5px', fontWeight: 700, color: zColor }}>
                    {z.regime_title} ({(z.domain_fraction * 100).toFixed(0)}% of domain)
                  </span>
                </div>
                <div style={{ fontSize: '11px', color: 'var(--text-2)', marginTop: '2px' }}>
                  {z.switching_criterion}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* 4. Interactive Visual Diagram + Parameter Controls */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: embedded ? '1fr' : 'repeat(auto-fit, minmax(310px, 1fr))',
          gap: '10px',
        }}
      >
        {/* Left: Scenario-Aware Physics Schematic & Particle Cloud */}
        <div
          style={{
            padding: '10px',
            borderRadius: '6px',
            background: 'var(--panel)',
            border: '1px solid var(--line)',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          {spec.scenario === 'spout_lip_knurl_1100c' ? (
            <>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <strong>1100 °C Float-Bath Spout-Lip Pour &amp; Reverse-Flow Heel to Knurl</strong>
                <span class="sys-badge">Implicit Stokes (Oh = {rep.ohnesorge_number.toFixed(1)})</span>
              </div>
              <svg
                viewBox="0 0 460 210"
                data-testid="sph-spout-svg"
                style={{
                  width: '100%',
                  height: 'auto',
                  background: '#0f172a',
                  borderRadius: '6px',
                  border: '1px solid var(--line)',
                }}
              >
                {/* Molten Tin Bath Pool */}
                <rect x="10" y="150" width="440" height="50" fill="#334155" />
                <text x="330" y="182" fill="#cbd5e1" fontSize="10" fontFamily="monospace">
                  Molten Tin Bath (ρ = 6500 kg/m³)
                </text>
                {/* Refractory Spout Lip Block */}
                <polygon points="15,25 175,25 175,92 125,92 125,115 15,115" fill="#78716c" stroke="#a8a29e" strokeWidth="1.5" />
                <text x="26" y="55" fill="#f5f5f4" fontSize="10" fontWeight="bold">
                  Refractory Spout Lip
                </text>
                <text x="26" y="70" fill="#d6d3d1" fontSize="9" fontFamily="monospace">
                  Skin ΔT = -{spec.thermal_gradient_delta_k.toFixed(0)} K (Bad Glass)
                </text>
                {/* 1100 °C Molten Glass Stream + Reverse-Flowing Heel Meniscus + Forward Ribbon (Dynamic with Sliders & Time) */}
                {(() => {
                  const hEqPx = Math.max(18, Math.min(62, spout.equilibrium_ribbon_thickness_mm * 5.2));
                  const ribbonTopY = 150 - hEqPx;
                  const heelLenPx = Math.max(28, Math.min(115, spout.heel_wetback_length_mm * 3.2));
                  const heelTipX = Math.max(42, 175 - heelLenPx);
                  const saddleX = Math.max(
                    heelTipX + 16,
                    Math.min(228, 175 + spout.stagnation_saddle_x_mm * 2.8),
                  );
                  const pourTopY = Math.max(16, 48 - spec.reservoir_height_mm * 0.35);
                  return (
                    <>
                      <path
                        d={`M 175,${pourTopY.toFixed(1)} Q 215,${(pourTopY + 4).toFixed(1)} 232,${(ribbonTopY - 10).toFixed(1)} Q 248,${ribbonTopY.toFixed(1)} 440,${ribbonTopY.toFixed(1)} L 440,150 L ${heelTipX.toFixed(1)},150 Q ${(heelTipX - 18).toFixed(1)},132 125,115 L 175,92 Z`}
                        fill="rgba(245, 158, 11, 0.42)"
                        stroke="#f59e0b"
                        strokeWidth="2"
                      />
                      {/* Dividing Streamline ψ = 0 & Stagnation Saddle Point */}
                      <path
                        d={`M 175,78 Q ${(0.5 * (175 + saddleX) + 8).toFixed(1)},104 ${saddleX.toFixed(1)},150`}
                        fill="none"
                        stroke="#38bdf8"
                        strokeWidth="2"
                        strokeDasharray="4,3"
                      />
                      <circle cx={saddleX} cy="148" r="5" fill="#ef4444" stroke="#fff" strokeWidth="1.5" />
                      <text x={saddleX + 8} y="144" fill="#fca5a5" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                        Stagnation Saddle (x = {spout.stagnation_saddle_x_mm.toFixed(1)} mm)
                      </text>
                      {/* Animated forward clean-ribbon & reverse heel tracer packets */}
                      {[0, 0.25, 0.5, 0.75].map((off, ti) => {
                        const s = (simPhase + off) % 1.0;
                        const fx = 185 + s * 245;
                        const fy = pourTopY + 18 + (ribbonTopY + 0.45 * hEqPx - (pourTopY + 18)) * Math.min(1, s * 1.45);
                        const bx = saddleX - 6 - s * Math.max(18, saddleX - heelTipX - 12);
                        const by = 140 - Math.sin(s * Math.PI) * 12;
                        return (
                          <g key={ti}>
                            <circle cx={fx} cy={fy} r="3.2" fill="#34d399" opacity="0.9" />
                            <circle cx={bx} cy={by} r="3.0" fill="#fb7185" opacity="0.9" />
                          </g>
                        );
                      })}
                      {/* Reverse-Flowing Heel Arrow (Bad Surface Glass -> Knurl Edge) */}
                      <path
                        d={`M ${(saddleX - 8).toFixed(1)},135 Q ${(0.5 * (saddleX + heelTipX)).toFixed(1)},142 ${(heelTipX + 10).toFixed(1)},132`}
                        fill="none"
                        stroke="#ef4444"
                        strokeWidth="2.2"
                      />
                      <polygon points={`${(heelTipX + 10).toFixed(1)},132 ${(heelTipX + 18).toFixed(1)},128 ${(heelTipX + 17).toFixed(1)},137`} fill="#ef4444" />
                      <text x="22" y="138" fill="#fda4af" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                        Heel Backflow ({spout.backflow_diversion_pct.toFixed(1)}%) → Knurl ({spout.bad_glass_to_knurl_rejection_pct.toFixed(1)}% Flushed)
                      </text>
                      {/* Forward Clean Core Ribbon Arrow */}
                      <path d={`M 215,${(pourTopY + 34).toFixed(1)} Q 255,${(ribbonTopY + 0.5 * hEqPx).toFixed(1)} 365,${(ribbonTopY + 0.5 * hEqPx).toFixed(1)}`} fill="none" stroke="#34d399" strokeWidth="2.2" />
                      <polygon points={`365,${(ribbonTopY + 0.5 * hEqPx).toFixed(1)} 356,${(ribbonTopY + 0.5 * hEqPx - 5).toFixed(1)} 356,${(ribbonTopY + 0.5 * hEqPx + 5).toFixed(1)}`} fill="#34d399" />
                      <text x="255" y={Math.max(40, ribbonTopY - 6)} fill="#6ee7b7" fontSize="10" fontWeight="bold" fontFamily="monospace">
                        Clean Float Ribbon (h_eq = {spout.equilibrium_ribbon_thickness_mm.toFixed(2)} mm)
                      </text>
                    </>
                  );
                })()}
              </svg>

              {/* 3D (x, y, z) Top-Down Planform Velocity Vector & Bad-Skin Lateral Flush to Knurl Map */}
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: '2px' }}>
                <strong style={{ fontSize: '11.5px', color: '#fbbf24' }}>
                  3D Planform (x, y) Wet-Back Heel &amp; Transverse Cross-Bath Flow v_y(x,y) → Left/Right Knurls
                </strong>
                <span class="num" style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                  W_spout = {spout.spout_width_y_mm.toFixed(0)} mm · Q_y/Q_x = {spout.lateral_to_reverse_flux_ratio.toFixed(2)}
                </span>
              </div>
              <svg
                viewBox="0 0 460 195"
                data-testid="sph-spout-3d-planform-svg"
                style={{
                  width: '100%',
                  height: 'auto',
                  background: '#090d16',
                  borderRadius: '6px',
                  border: '1px solid var(--line)',
                }}
              >
                <defs>
                  <marker id="spout3d-arrow" viewBox="0 0 10 10" refX="7" refY="5" markerWidth="4" markerHeight="4" orient="auto-start-reverse">
                    <path d="M 0 1 L 10 5 L 0 9 z" fill="#f8fafc" />
                  </marker>
                </defs>

                {/* Top & Bottom Knurl Selvedge Lanes (y = +W/2 and y = -W/2) */}
                <rect x="215" y="10" width="230" height="24" fill="rgba(239, 68, 68, 0.22)" stroke="#ef4444" strokeWidth="1" strokeDasharray="4,2" />
                <text x="225" y="25" fill="#fca5a5" fontSize="9" fontWeight="bold" fontFamily="monospace">
                  ⚙ Top-Roll Left Knurl Selvedge (+y = +{(spout.spout_width_y_mm / 2).toFixed(0)} mm, W_knurl = {spout.knurl_selvedge_width_mm.toFixed(0)} mm)
                </text>
                <rect x="215" y="161" width="230" height="24" fill="rgba(239, 68, 68, 0.22)" stroke="#ef4444" strokeWidth="1" strokeDasharray="4,2" />
                <text x="225" y="176" fill="#fca5a5" fontSize="9" fontWeight="bold" fontFamily="monospace">
                  ⚙ Top-Roll Right Knurl Selvedge (−y = −{(spout.spout_width_y_mm / 2).toFixed(0)} mm, W_knurl = {spout.knurl_selvedge_width_mm.toFixed(0)} mm)
                </text>

                {/* Sealed Wet-Back Meniscus Line at x = -L_heel */}
                <line x1="38" y1="22" x2="38" y2="173" stroke="#a8a29e" strokeWidth="2.5" />
                <text x="12" y="100" fill="#d6d3d1" fontSize="8.5" fontFamily="monospace" transform="rotate(-90 22,100)">
                  Sealed Heel (−L_heel)
                </text>

                {/* Spout Lip Front Edge x = 0 */}
                <line x1="215" y1="20" x2="215" y2="175" stroke="#f59e0b" strokeWidth="1.8" strokeDasharray="5,3" />
                <text x="165" y="18" fill="#fbbf24" fontSize="8.5" fontFamily="monospace">
                  Spout Lip x=0
                </text>

                {/* Render 11×9 3D (x, y) velocity vectors (u_x, v_y) & bad-skin tracer heatmap */}
                {(spout.planform_3d_grid ?? []).map(([xm, ym, ux, vy, , badTracer], idx) => {
                  const lHeel = Math.max(1, spout.heel_wetback_length_mm);
                  const halfW = Math.max(1, 0.5 * spout.spout_width_y_mm);
                  const px = 215 + (xm / lHeel) * 168;
                  const py = 97.5 - (ym / halfW) * 68;
                  const vScale = Math.max(1e-3, spout.lateral_exit_velocity_mm_s, spout.centerline_reverse_velocity_mm_s);
                  const vxN = ux / vScale;
                  const vyN = vy / vScale;
                  const mag = Math.hypot(vxN, vyN);
                  const len = Math.min(18, Math.max(4, mag * 16));
                  const uxDir = mag > 1e-6 ? vxN / mag : 0;
                  const uyDir = mag > 1e-6 ? vyN / mag : 0;
                  const colCell =
                    badTracer > 0.55
                      ? `rgba(239, 68, 68, ${(0.14 + 0.25 * badTracer).toFixed(2)})`
                      : `rgba(16, 185, 129, ${(0.12 + 0.22 * (1 - badTracer)).toFixed(2)})`;

                  return (
                    <g key={idx}>
                      <rect x={px - 14} y={py - 7.5} width="28" height="15" fill={colCell} rx="2" />
                      {mag > 0.04 && (
                        <line
                          x1={px}
                          y1={py}
                          x2={px + uxDir * len}
                          y2={py - uyDir * len}
                          stroke={xm < spout.stagnation_saddle_x_mm ? '#fda4af' : '#6ee7b7'}
                          strokeWidth="1.25"
                          markerEnd="url(#spout3d-arrow)"
                        />
                      )}
                    </g>
                  );
                })}

                {/* 3D Stagnation Saddle Point (x_stag, y = 0) */}
                {(() => {
                  const lHeel = Math.max(1, spout.heel_wetback_length_mm);
                  const sx = 215 + (spout.stagnation_saddle_x_mm / lHeel) * 168;
                  return (
                    <g>
                      <circle cx={sx} cy={97.5} r="5.5" fill="#38bdf8" stroke="#fff" strokeWidth="1.5" />
                      <text x={sx + 8} y={101} fill="#38bdf8" fontSize="9" fontWeight="bold" fontFamily="monospace">
                        3D Saddle (u_x=0, v_y=0)
                      </text>
                    </g>
                  );
                })()}
              </svg>

              {/* Spout-Lip Key Engineering Readout (Including 3D Transverse Velocities) */}
              <div
                class="num"
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(135px, 1fr))',
                  gap: '6px',
                  fontSize: '11px',
                  padding: '6px 8px',
                  background: 'var(--bg)',
                  borderRadius: '5px',
                  border: '1px solid var(--line)',
                }}
              >
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>1100 °C Glass Viscosity</div>
                  <strong>{spout.glass_viscosity_pa_s.toFixed(0)} Pa·s</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Implicit Stokes Speedup</div>
                  <strong>{spout.implicit_stiffness_speedup.toExponential(2)}×</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Equilibrium Ribbon h_eq</div>
                  <strong data-testid="sph-spout-heq">{spout.equilibrium_ribbon_thickness_mm.toFixed(2)} mm</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Wet-Back Heel (x × y)</div>
                  <strong>{spout.heel_wetback_length_mm.toFixed(1)} × {spout.spout_width_y_mm.toFixed(0)} mm</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>3D Heel Inflow −u_x(y=0)</div>
                  <strong data-testid="sph-spout-rev-ux">{spout.centerline_reverse_velocity_mm_s.toFixed(2)} mm/s</strong> ({spout.backflow_diversion_pct.toFixed(1)}%)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>3D Lateral Ear Exit |v_y|</div>
                  <strong data-testid="sph-spout-lat-vy" style={{ color: '#fbbf24' }}>{spout.lateral_exit_velocity_mm_s.toFixed(2)} mm/s</strong> (τ = {spout.heel_lateral_residence_time_s.toFixed(0)} s)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Bad-Glass to Knurl</div>
                  <strong style={{ color: 'var(--ok)' }}>{spout.bad_glass_to_knurl_rejection_pct.toFixed(1)}%</strong> ({spout.knurl_selvedge_width_mm.toFixed(1)} mm edge)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Cox–Voinov Ca_cl</div>
                  <strong data-testid="sph-spout-cacl" style={{ color: '#38bdf8' }}>
                    {(spout.cox_voinov_capillary_number ?? 0).toExponential(2)}
                  </strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Dynamic Contact θ_app</div>
                  <strong data-testid="sph-spout-theta-app" style={{ color: '#f472b6' }}>
                    {(spout.dynamic_contact_angle_deg ?? spec.contact_angle_deg).toFixed(1)}°
                  </strong>{' '}
                  (θ_e = {spec.contact_angle_deg.toFixed(0)}°)
                </div>
              </div>
            </>
          ) : spec.scenario === 'peregrine_wave_air_entrainment' ? (
            <>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '4px' }}>
                <strong>Howell Peregrine (1983 / 2001) Breaking-Wave Air Entrainment Cascade</strong>
                <div style={{ display: 'flex', gap: '5px', flexWrap: 'wrap' }}>
                  <span class="sys-badge">3 Regimes Active (ξ₀ = {pereg.iribarren_number.toFixed(2)})</span>
                  <span
                    class="sys-badge"
                    data-testid="sph-peregrine-impulse-badge"
                    style={{ color: '#fbbf24', borderColor: 'rgba(251, 191, 36, 0.45)' }}
                  >
                    Peak Impulse {(pereg.peak_wall_impulse_pa_s ?? 0).toFixed(1)} Pa·s
                  </span>
                  <span
                    class="sys-badge"
                    data-testid="sph-peregrine-flip-badge"
                    style={{ color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.45)' }}
                  >
                    Flip-Through w_flip = {(pereg.flip_through_jet_velocity_m_s ?? 0).toFixed(2)} m/s ({(pereg.flip_through_amplification_ratio ?? 1).toFixed(1)}× u₀)
                  </span>
                </div>
              </div>
              <svg
                viewBox="0 0 460 210"
                data-testid="sph-peregrine-svg"
                style={{
                  width: '100%',
                  height: 'auto',
                  background: '#0f172a',
                  borderRadius: '6px',
                  border: '1px solid var(--line)',
                }}
              >
                {/* Shoaling Seabed */}
                <polygon points="0,195 460,155 460,210 0,210" fill="#475569" />
                {/* Zone 1: Boussinesq-Peregrine Potential Shoaling */}
                <path
                  d="M 10,135 Q 85,120 145,55 L 145,182 L 10,194 Z"
                  fill="rgba(56, 189, 248, 0.35)"
                  stroke="#38bdf8"
                  strokeWidth="2"
                />
                <text x="18" y="30" fill="#38bdf8" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                  Zone 1: Peregrine Potential
                </text>
                {/* Zone 2: DFSPH Overturning Plunging Jet & Trapped Air Tube (Dynamic from rep.particles & air_pocket_trace) */}
                {(() => {
                  const rTubeMm = Math.max(1.0, pereg.trapped_air_tube_radius_mm);
                  const uJet = Math.max(0.2, pereg.plunging_jet_velocity_m_s);
                  const voidPct = Math.max(0.5, pereg.entrained_air_void_fraction_pct);
                  const traceArr = pereg.air_pocket_trace ?? [];
                  const wallArr = pereg.wall_impulse_profile ?? [];
                  const curTracePt =
                    traceArr.length > 0
                      ? traceArr[Math.min(traceArr.length - 1, Math.floor(simPhase * traceArr.length))]
                      : undefined;
                  const xiBreath = curTracePt ? Math.sqrt(Math.max(0.45, Math.min(1.45, 1 - 0.5 * curTracePt.xi_over_delta0))) : 1.0;
                  const rxPx = Math.max(8, Math.min(38, (11 + rTubeMm * 1.15) * xiBreath));
                  const ryPx = Math.max(6, Math.min(28, rxPx * 0.68));
                  const jetSurge = Math.sin(2 * Math.PI * simPhase) * 6.5;
                  const jetReachX = Math.max(242, Math.min(312, 236 + uJet * 11.5 + jetSurge));
                  const crestY = Math.max(18, Math.min(54, 48 - uJet * 4.5));
                  const cavityCx = 192 + (jetReachX - 242) * 0.38;
                  const cavityCy = 108;
                  const rHinzePx = Math.max(1.6, Math.min(6.2, pereg.hinze_bubble_radius_mm * 2.2));
                  const bubbleScale = Math.max(0.65, Math.min(1.65, voidPct / 18.0));
                  const r0Part = Math.max(0.5, spec.characteristic_radius_mm);
                  const maxImpVal = Math.max(1, pereg.peak_wall_impulse_pa_s ?? 1);
                  const wallOverlayPts = wallArr
                    .map((w) => `${(448 - (w.impulse_pa_s / maxImpVal) * 44).toFixed(1)},${(155 - w.z_over_h * 115).toFixed(1)}`)
                    .join(' ');
                  const pocketOverlayPts = traceArr
                    .slice(0, 32)
                    .map((pt, ti) => `${(150 + (ti / 31) * 90).toFixed(1)},${(cavityCy - pt.xi_over_delta0 * 14).toFixed(1)}`)
                    .join(' ');

                  return (
                    <>
                      <path
                        d={`M 145,55 Q ${(145 + jetReachX) * 0.52},${crestY} ${jetReachX},125 Q ${jetReachX - 26},130 ${cavityCx + rxPx * 0.6},95 Q ${cavityCx - rxPx * 0.8},75 ${cavityCx - rxPx},115 Q ${cavityCx - rxPx * 0.35},140 ${jetReachX},125 L ${jetReachX + 17},170 L 145,182 Z`}
                        fill="rgba(99, 102, 241, 0.42)"
                        stroke="#818cf8"
                        strokeWidth="2"
                      />
                      {/* Trapped Air Cavity Tube */}
                      <ellipse
                        cx={cavityCx}
                        cy={cavityCy}
                        rx={rxPx}
                        ry={ryPx}
                        fill="#0f172a"
                        stroke="#f8fafc"
                        strokeWidth="1.5"
                        strokeDasharray="3,2"
                      />
                      {pocketOverlayPts && (
                        <polyline fill="none" stroke="#f43f5e" strokeWidth="1.3" strokeDasharray="2,2" points={pocketOverlayPts} />
                      )}
                      {wallOverlayPts && (
                        <polyline fill="none" stroke="#fbbf24" strokeWidth="1.6" points={wallOverlayPts} />
                      )}
                      <text x="155" y="30" fill="#a5b4fc" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                        Zone 2: DFSPH Jet &amp; Air-Tube
                      </text>
                      <text x={cavityCx - rxPx * 0.75} y={cavityCy + 3} fill="#f8fafc" fontSize="8.5" fontFamily="monospace">
                        R={pereg.trapped_air_tube_radius_mm.toFixed(1)}mm
                      </text>
                      {/* Zone 3: Turbulent Roller Bore & Hinze Bubble Cascade */}
                      <path
                        d={`M ${jetReachX},125 Q ${jetReachX + 52},${Math.max(82, 105 - voidPct * 0.45)} ${jetReachX + 95},125 Q 405,130 450,135 L 450,156 L ${jetReachX + 17},170 Z`}
                        fill="rgba(16, 185, 129, 0.36)"
                        stroke="#10b981"
                        strokeWidth="2"
                      />
                      <text x="295" y="30" fill="#34d399" fontSize="9.5" fontWeight="bold" fontFamily="monospace">
                        Zone 3: Hinze Bubble CSF
                      </text>
                      {/* Live SPH particles from rep.particles projected across Zones 1–3 */}
                      {rep.particles.slice(0, 64).map((pt, pi) => {
                        const xn = Math.max(-1, Math.min(1, pt.pos_mm[0] / r0Part));
                        const zn = Math.max(-1, Math.min(1, pt.pos_mm[2] / r0Part));
                        const px = 28 + (xn + 1) * 0.5 * 405;
                        const py = 122 - zn * 44;
                        const col = regimeColor(pt.regime_code);
                        const rDot = pt.regime_code === 'capillary_mls_sph' ? Math.max(1.5, Math.min(5.5, rHinzePx * 0.75 * bubbleScale)) : 2.1;
                        return (
                          <circle
                            key={pi}
                            cx={px}
                            cy={py}
                            r={rDot}
                            fill={pt.is_free_surface ? 'rgba(255,255,255,0.82)' : col}
                            stroke={col}
                            strokeWidth="0.9"
                          />
                        );
                      })}
                      <text x="285" y="82" fill="#6ee7b7" fontSize="9.5" fontFamily="monospace">
                        a_Hinze = {pereg.hinze_bubble_radius_mm.toFixed(2)} mm (α_air = {pereg.entrained_air_void_fraction_pct.toFixed(1)}%)
                      </text>
                    </>
                  );
                })()}
              </svg>

              {/* Cooker & Peregrine (1995) Wall/Seabed Pressure-Impulse + Bagnold (1939) Air-Pocket Trace Dual SVG */}
              {(() => {
                const wallProf = pereg.wall_impulse_profile ?? [];
                const airTrace = pereg.air_pocket_trace ?? [];
                const maxImp = Math.max(
                  1,
                  pereg.peak_wall_impulse_pa_s ?? 1,
                  ...wallProf.map((w) => Math.max(w.impulse_pa_s, w.seabed_impulse_pa_s)),
                );
                const wallPts = wallProf
                  .map((w) => {
                    const x = 36 + (w.impulse_pa_s / maxImp) * 172;
                    const y = 132 - w.z_over_h * 104;
                    return `${x.toFixed(1)},${y.toFixed(1)}`;
                  })
                  .join(' ');
                const bedPts = wallProf
                  .map((w) => {
                    const x = 36 + (w.seabed_x_over_h / 2.0) * 172;
                    const y = 132 - (w.seabed_impulse_pa_s / maxImp) * 104;
                    return `${x.toFixed(1)},${y.toFixed(1)}`;
                  })
                  .join(' ');

                const maxT = Math.max(1, ...(airTrace.map((a) => a.t_ms)));
                const maxPg = Math.max(10, ...(airTrace.map((a) => a.p_gauge_kpa)));
                const minPg = Math.min(-5, ...(airTrace.map((a) => a.p_gauge_kpa)));
                const pgSpan = Math.max(1, maxPg - minPg);
                const mapAirX = (t: number) => 36 + (t / maxT) * 172;
                const mapAirY = (pg: number) => 132 - ((pg - minPg) / pgSpan) * 104;
                const yZero = mapAirY(0);
                const airPts = airTrace
                  .map((a) => `${mapAirX(a.t_ms).toFixed(1)},${mapAirY(a.p_gauge_kpa).toFixed(1)}`)
                  .join(' ');
                const peakAirPt =
                  airTrace.length > 0
                    ? airTrace.reduce((best, a) => (a.p_gauge_kpa > best.p_gauge_kpa ? a : best), airTrace[0])
                    : { t_ms: 0, p_gauge_kpa: 0 };
                const troughAirPt =
                  airTrace.length > 0
                    ? airTrace.reduce((best, a) => (a.p_gauge_kpa < best.p_gauge_kpa ? a : best), airTrace[0])
                    : { t_ms: 0, p_gauge_kpa: 0 };

                return (
                  <div
                    data-testid="sph-peregrine-diagnostics-grid"
                    style={{
                      display: 'grid',
                      gridTemplateColumns: 'repeat(auto-fit, minmax(215px, 1fr))',
                      gap: '8px',
                    }}
                  >
                    {/* Plot 1: Cooker & Peregrine (1995) Wall Pressure-Impulse P_wall(z/H) & Seabed Impulse P_bed(x/H) */}
                    <div
                      style={{
                        padding: '6px 8px',
                        borderRadius: '6px',
                        background: '#090d16',
                        border: '1px solid var(--line)',
                      }}
                    >
                      <div style={{ fontSize: '10.5px', fontWeight: 700, color: '#fbbf24', marginBottom: '3px' }}>
                        Cooker &amp; Peregrine (1995) Impulse P_wall(z/H) &amp; P_bed(x/H)
                      </div>
                      <svg
                        viewBox="0 0 220 148"
                        data-testid="sph-cooker-peregrine-impulse-svg"
                        style={{ width: '100%', height: 'auto', display: 'block' }}
                      >
                        <line x1="36" y1="26" x2="36" y2="132" stroke="#475569" strokeWidth="1.2" />
                        <line x1="36" y1="132" x2="210" y2="132" stroke="#475569" strokeWidth="1.2" />
                        {/* Wall pressure-impulse curve P_wall(z/H) */}
                        <polyline
                          fill="none"
                          stroke="#fbbf24"
                          strokeWidth="2.1"
                          points={wallPts}
                        />
                        {/* Seabed pressure-impulse curve P_bed(x/H) */}
                        <polyline
                          fill="none"
                          stroke="#38bdf8"
                          strokeWidth="1.8"
                          strokeDasharray="4,2"
                          points={bedPts}
                        />
                        <text x="40" y="18" fill="#fbbf24" fontSize="8" fontWeight="bold" fontFamily="monospace">
                          — P_wall(z/H) Peak {(pereg.peak_wall_impulse_pa_s ?? 0).toFixed(1)} Pa·s
                        </text>
                        <text x="40" y="29" fill="#38bdf8" fontSize="8" fontFamily="monospace">
                          -- P_bed(x/H) · w_flip={(pereg.flip_through_jet_velocity_m_s ?? 0).toFixed(1)} m/s
                        </text>
                        <text x="4" y="34" fill="#94a3b8" fontSize="7.5" fontFamily="monospace">
                          z/H=1
                        </text>
                        <text x="4" y="132" fill="#94a3b8" fontSize="7.5" fontFamily="monospace">
                          z/H=0
                        </text>
                        <text x="65" y="144" fill="#cbd5e1" fontSize="8" fontFamily="monospace">
                          Impulse P (Pa·s) / Offshore x/H
                        </text>
                      </svg>
                    </div>

                    {/* Plot 2: Bagnold (1939) / Wood-Peregrine-Bruce (2000) Polytropic Air-Pocket Oscillation p_gauge(t) */}
                    <div
                      style={{
                        padding: '6px 8px',
                        borderRadius: '6px',
                        background: '#090d16',
                        border: '1px solid var(--line)',
                      }}
                    >
                      <div style={{ fontSize: '10.5px', fontWeight: 700, color: '#f472b6', marginBottom: '3px' }}>
                        Bagnold (1939) Air-Pocket p_gauge(t) (γ = 1.4 Polytropic)
                      </div>
                      <svg
                        viewBox="0 0 220 148"
                        data-testid="sph-bagnold-air-pocket-svg"
                        style={{ width: '100%', height: 'auto', display: 'block' }}
                      >
                        {/* Sub-atmospheric rarefaction shading below p_gauge = 0 */}
                        <rect
                          x="36"
                          y={Math.min(132, Math.max(26, yZero))}
                          width="174"
                          height={Math.max(0, 132 - Math.min(132, Math.max(26, yZero)))}
                          fill="rgba(244, 63, 94, 0.14)"
                        />
                        <line x1="36" y1="26" x2="36" y2="132" stroke="#475569" strokeWidth="1.2" />
                        <line x1="36" y1={yZero} x2="210" y2={yZero} stroke="#94a3b8" strokeWidth="1" strokeDasharray="3,2" />
                        <polyline
                          fill="none"
                          stroke="#f472b6"
                          strokeWidth="2"
                          points={airPts}
                        />
                        {/* Compression spike dot */}
                        <circle
                          cx={mapAirX(peakAirPt.t_ms)}
                          cy={mapAirY(peakAirPt.p_gauge_kpa)}
                          r="3.2"
                          fill="#34d399"
                        />
                        {/* Sub-atmospheric rarefaction suction trough dot */}
                        <circle
                          cx={mapAirX(troughAirPt.t_ms)}
                          cy={mapAirY(troughAirPt.p_gauge_kpa)}
                          r="3.5"
                          fill="#fb7185"
                        />
                        {airTrace.length > 0 && (() => {
                          const curPt = airTrace[Math.min(airTrace.length - 1, Math.floor(simPhase * airTrace.length))];
                          const cx = mapAirX(curPt.t_ms);
                          const cy = mapAirY(curPt.p_gauge_kpa);
                          return (
                            <g>
                              <line x1={cx} y1="26" x2={cx} y2="132" stroke="#38bdf8" strokeWidth="1" strokeDasharray="2,2" />
                              <circle cx={cx} cy={cy} r="4.2" fill="#38bdf8" stroke="#fff" strokeWidth="1.2" />
                            </g>
                          );
                        })()}
                        <text x="40" y="18" fill="#34d399" fontSize="8" fontWeight="bold" fontFamily="monospace">
                          ● Spike +{peakAirPt.p_gauge_kpa.toFixed(1)} kPa
                        </text>
                        <text x="40" y="29" fill="#fb7185" fontSize="8" fontWeight="bold" fontFamily="monospace">
                          ● Suction Trough {(pereg.min_rebound_gauge_pressure_kpa ?? troughAirPt.p_gauge_kpa).toFixed(1)} kPa (&lt;0)
                        </text>
                        <text x="5" y={yZero + 3} fill="#cbd5e1" fontSize="7.5" fontFamily="monospace">
                          0 kPa
                        </text>
                        <text x="68" y="144" fill="#cbd5e1" fontSize="8" fontFamily="monospace">
                          Time t (0 .. {maxT.toFixed(1)} ms)
                        </text>
                      </svg>
                    </div>
                  </div>
                );
              })()}

              <div
                class="num"
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(135px, 1fr))',
                  gap: '6px',
                  fontSize: '11px',
                  padding: '6px 8px',
                  background: 'var(--bg)',
                  borderRadius: '5px',
                  border: '1px solid var(--line)',
                }}
              >
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Plunging Jet u_jet</div>
                  <strong data-testid="sph-peregrine-ujet">{pereg.plunging_jet_velocity_m_s.toFixed(2)} m/s</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Flip-Through Jet w_flip</div>
                  <strong data-testid="sph-peregrine-wflip" style={{ color: '#38bdf8' }}>
                    {(pereg.flip_through_jet_velocity_m_s ?? 0).toFixed(2)} m/s
                  </strong>{' '}
                  ({(pereg.flip_through_amplification_ratio ?? 1).toFixed(1)}× u₀)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Peak Wall Impulse P_pk</div>
                  <strong data-testid="sph-peregrine-peak-impulse" style={{ color: '#fbbf24' }}>
                    {(pereg.peak_wall_impulse_pa_s ?? 0).toFixed(1)} Pa·s
                  </strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Rebound Suction p_min</div>
                  <strong data-testid="sph-peregrine-rebound-kpa" style={{ color: '#fb7185' }}>
                    {(pereg.min_rebound_gauge_pressure_kpa ?? 0).toFixed(1)} kPa
                  </strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Roller Circulation Γ</div>
                  <strong>{pereg.roller_circulation_m2_s.toFixed(3)} m²/s</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Trapped Air-Tube R_tube</div>
                  <strong>{pereg.trapped_air_tube_radius_mm.toFixed(1)} mm</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Hinze Bubble Scale a_H</div>
                  <strong data-testid="sph-peregrine-hinze">{pereg.hinze_bubble_radius_mm.toFixed(2)} mm</strong> (d₃₂ = {pereg.sauter_mean_bubble_diam_mm.toFixed(2)} mm)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Entrained Air Void α</div>
                  <strong style={{ color: '#38bdf8' }}>{pereg.entrained_air_void_fraction_pct.toFixed(1)}%</strong>
                </div>
              </div>
            </>
          ) : (
            <>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
                <strong>3D SPH Particle Cross-Section, MLS Curvature Normals &amp; Capillary Meniscus</strong>
                <span class="sys-badge">
                  κ_MLS = {rep.sph_mean_curvature_inv_mm.toFixed(3)} mm⁻¹ (Err {rep.curvature_rel_error_pct.toFixed(2)}%)
                </span>
              </div>
              {/* Scalar Field Coloring Pill Selector */}
              {(() => {
                const sliceParticles = rep.particles
                  .filter((p) => Math.abs(p.pos_mm[1]) <= 0.45 * spec.characteristic_radius_mm)
                  .slice(0, 260);
                const scalarValue = (p: (typeof sliceParticles)[number]): number => {
                  if (particleColorMode === 'speed') {
                    return Math.hypot(p.vel_m_s[0], p.vel_m_s[1], p.vel_m_s[2]);
                  }
                  if (particleColorMode === 'pressure') {
                    return p.pressure_pa;
                  }
                  if (particleColorMode === 'curvature') {
                    return Math.abs(p.curvature_inv_mm) * 1000.0; // 1/m
                  }
                  return 0;
                };
                const vals = sliceParticles.map(scalarValue);
                const vMin = vals.length > 0 ? Math.min(...vals) : 0;
                const vMax = vals.length > 0 ? Math.max(...vals) : 1;
                const colorRamp = (tRaw: number): string => {
                  const t = Math.max(0, Math.min(1, tRaw));
                  // Viridis/Turbo-style 4-stop ramp: deep blue -> cyan -> emerald/yellow -> coral red
                  const r = Math.round(30 + 215 * Math.pow(t, 1.3));
                  const g = Math.round(90 + 145 * Math.sin(Math.PI * t));
                  const b = Math.round(235 - 195 * t);
                  return `rgb(${r}, ${g}, ${b})`;
                };
                const unitLabel =
                  particleColorMode === 'speed'
                    ? 'm/s'
                    : particleColorMode === 'pressure'
                      ? 'Pa'
                      : 'm⁻¹';

                return (
                  <>
                    <div
                      data-testid="sph-particle-color-selector"
                      style={{
                        display: 'flex',
                        flexWrap: 'wrap',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                        gap: '6px',
                        fontSize: '10.5px',
                      }}
                    >
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px' }}>
                        {(
                          [
                            { id: 'class', label: 'Surface / Contact Class' },
                            { id: 'speed', label: 'Velocity Speed |u| (m/s)' },
                            { id: 'pressure', label: 'Pressure p (Pa)' },
                            { id: 'curvature', label: 'Capillary Curvature |κ| (1/m)' },
                          ] as const
                        ).map((mode) => (
                          <button
                            key={mode.id}
                            type="button"
                            data-testid={`sph-color-pill-${mode.id}`}
                            class={`btn small ${particleColorMode === mode.id ? 'primary' : 'ghost'}`}
                            onClick={() => setParticleColorMode(mode.id)}
                            style={{ padding: '2px 7px', fontSize: '10px' }}
                          >
                            {mode.label}
                          </button>
                        ))}
                      </div>
                      <div
                        data-testid="sph-particle-color-legend"
                        style={{
                          fontFamily: 'var(--mono)',
                          fontSize: '10px',
                          color: 'var(--text-2)',
                        }}
                      >
                        {particleColorMode === 'class' ? (
                          <span>
                            ● Bulk (#6366f1) · ● Free-Surface (#38bdf8) · ● Contact-Line (#f59e0b)
                          </span>
                        ) : (
                          <span>
                            Min: <strong>{vMin.toFixed(3)} {unitLabel}</strong> → Max:{' '}
                            <strong>{vMax.toFixed(3)} {unitLabel}</strong>
                          </span>
                        )}
                      </div>
                    </div>

                    <svg
                      viewBox="-140 -105 280 210"
                      data-testid="sph-particle-svg"
                      style={{
                        width: '100%',
                        height: 'auto',
                        background: '#0f172a',
                        borderRadius: '6px',
                        border: '1px solid var(--line)',
                      }}
                    >
                      {sliceParticles.map((p, idx) => {
                        const rZoom = Math.pow(Math.max(0.4, spec.characteristic_radius_mm) / 2.0, 0.35);
                        const baseScale = (58.0 / Math.max(0.25, spec.characteristic_radius_mm)) * rZoom;
                        const osc = Math.cos(2 * Math.PI * simPhase);
                        let dx = p.pos_mm[0] * baseScale;
                        let dz = p.pos_mm[2] * baseScale;
                        if (spec.scenario === 'droplet_oscillation_a15') {
                          const eps = Math.max(0.06, spec.droplet_aspect_perturbation) * osc;
                          dx *= 1 + eps;
                          dz *= 1 - eps;
                        } else if (spec.scenario === 'capillary_rise_a16') {
                          const rNorm = Math.min(1, Math.abs(p.pos_mm[0]) / Math.max(0.2, spec.characteristic_radius_mm));
                          const risePx = Math.min(28, rep.sph_capillary_rise_mm * 0.45) * (0.65 + 0.35 * Math.sin(Math.PI * simPhase));
                          const meniscusDish = 14 * Math.cos((spec.contact_angle_deg * Math.PI) / 180) * (rNorm * rNorm - 0.35);
                          dz += risePx - 12 + meniscusDish;
                        } else {
                          dx += p.vel_m_s[0] * 14 * Math.sin(2 * Math.PI * simPhase);
                          dz += p.vel_m_s[2] * 14 * Math.sin(2 * Math.PI * simPhase);
                        }
                        const cx = Math.max(-132, Math.min(132, dx));
                        const cy = Math.max(-98, Math.min(98, -dz));
                        let fill = p.is_contact_line
                          ? '#f59e0b'
                          : p.is_free_surface
                            ? '#38bdf8'
                            : '#6366f1';
                        if (particleColorMode !== 'class') {
                          const v = vals[idx] ?? 0;
                          const norm = (v - vMin) / Math.max(1e-9, vMax - vMin);
                          fill = colorRamp(norm);
                        }
                        return (
                          <g key={idx}>
                            <circle
                              cx={cx}
                              cy={cy}
                              r={p.is_free_surface ? 3.2 : 2.3}
                              fill={fill}
                              opacity={0.92}
                            />
                            {p.is_free_surface && (
                              <line
                                x1={cx}
                                y1={cy}
                                x2={cx + p.normal[0] * 10}
                                y2={cy - p.normal[2] * 10}
                                stroke="#34d399"
                                strokeWidth="1"
                              />
                            )}
                          </g>
                        );
                      })}
                    </svg>
                  </>
                );
              })()}
              <div
                class="num"
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(auto-fit, minmax(135px, 1fr))',
                  gap: '6px',
                  fontSize: '11px',
                  padding: '6px 8px',
                  background: 'var(--bg)',
                  borderRadius: '5px',
                  border: '1px solid var(--line)',
                }}
              >
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Young–Laplace Δp</div>
                  <strong data-testid="sph-laplace-pa">{rep.sph_laplace_pressure_pa.toFixed(2)} Pa</strong> (Exact {rep.exact_laplace_pressure_pa.toFixed(2)} Pa)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Rayleigh–Lamb Period T₂</div>
                  <strong>{rep.sph_rayleigh_period_ms.toFixed(2)} ms</strong> (Err {rep.rayleigh_period_rel_error_pct.toFixed(2)}%)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Jurin Capillary Rise h</div>
                  <strong data-testid="sph-jurin-mm">{rep.sph_capillary_rise_mm.toFixed(2)} mm</strong> (r_eff = {rep.effective_pore_radius_mm.toFixed(2)} mm)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>TPMS Retention Δp_cap</div>
                  <strong>{rep.tpms_capillary_retention_pa.toFixed(1)} Pa</strong> (K = {rep.tpms_permeability_mm2.toFixed(4)} mm²)
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Marangoni Surface u_M</div>
                  <strong>{rep.marangoni_surface_velocity_m_s.toFixed(3)} m/s</strong>
                </div>
                <div>
                  <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Peak FSI Baffle Impact</div>
                  <strong>{rep.peak_fsi_impact_pressure_kpa.toFixed(2)} kPa</strong> ({rep.total_fsi_baffle_force_n.toFixed(2)} N)
                </div>
              </div>
            </>
          )}
        </div>

        {/* Right: Interactive Fluid, Capillary & Spout/Wave Parameter Controls */}
        <div
          style={{
            padding: '10px',
            borderRadius: '6px',
            background: 'var(--panel)',
            border: '1px solid var(--line)',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          <strong>Interactive Fluid State &amp; Boundary Levers (Live Regime Re-Classification)</strong>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(175px, 1fr))', gap: '8px', fontSize: '11px' }}>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Dynamic Viscosity μ</span>
                <strong class="num">
                  {spec.viscosity_pa_s >= 1.0
                    ? `${spec.viscosity_pa_s.toFixed(0)} Pa·s`
                    : `${(spec.viscosity_pa_s * 1e3).toFixed(2)} mPa·s`}
                </strong>
              </div>
              <input
                type="range"
                min="-3"
                max="3.2"
                step="0.05"
                value={Math.log10(Math.max(1e-3, spec.viscosity_pa_s))}
                data-testid="sph-slider-viscosity"
                onInput={(e) =>
                  updateSpec({
                    viscosity_pa_s: Math.pow(10, parseFloat((e.target as HTMLInputElement).value)),
                  })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Surface Tension γ₀</span>
                <strong class="num">{spec.surface_tension_n_m.toFixed(3)} N/m</strong>
              </div>
              <input
                type="range"
                min="0.02"
                max="1.80"
                step="0.01"
                value={spec.surface_tension_n_m}
                data-testid="sph-slider-gamma"
                onInput={(e) =>
                  updateSpec({ surface_tension_n_m: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Young–Dupré Contact Angle θ_e</span>
                <strong class="num">{spec.contact_angle_deg.toFixed(0)}°</strong>
              </div>
              <input
                type="range"
                min="5"
                max="165"
                step="1"
                value={spec.contact_angle_deg}
                data-testid="sph-slider-theta"
                onInput={(e) =>
                  updateSpec({ contact_angle_deg: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Spout / Wave / Dam Head H₀</span>
                <strong class="num">{spec.reservoir_height_mm.toFixed(0)} mm</strong>
              </div>
              <input
                type="range"
                min="15"
                max="250"
                step="1"
                value={spec.reservoir_height_mm}
                data-testid="sph-slider-head"
                onInput={(e) =>
                  updateSpec({ reservoir_height_mm: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Characteristic Radius / Depth R₀</span>
                <strong class="num">{spec.characteristic_radius_mm.toFixed(2)} mm</strong>
              </div>
              <input
                type="range"
                min="0.4"
                max="20.0"
                step="0.2"
                value={spec.characteristic_radius_mm}
                onInput={(e) =>
                  updateSpec({ characteristic_radius_mm: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Thermal Skin / Gradient ΔT</span>
                <strong class="num">{spec.thermal_gradient_delta_k.toFixed(0)} K</strong>
              </div>
              <input
                type="range"
                min="0"
                max="200"
                step="5"
                value={spec.thermal_gradient_delta_k}
                onInput={(e) =>
                  updateSpec({ thermal_gradient_delta_k: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Ambient Gas Density ρ_g</span>
                <strong class="num" data-testid="sph-gas-density-val">
                  {(spec.gas_density_kg_m3 ?? 1.225).toFixed(3)} kg/m³
                </strong>
              </div>
              <input
                type="range"
                min="0.2"
                max="12.0"
                step="0.025"
                value={spec.gas_density_kg_m3 ?? 1.225}
                data-testid="sph-slider-gas-density"
                onInput={(e) =>
                  updateSpec({ gas_density_kg_m3: parseFloat((e.target as HTMLInputElement).value) })
                }
              />
            </label>
          </div>

          {/* Zaleski Two-Phase Gas-Liquid Density & Viscosity Ratio Bar */}
          <div
            data-testid="sph-zaleski-twophase-bar"
            style={{
              padding: '6px 8px',
              borderRadius: '5px',
              background: 'var(--bg)',
              border: '1px solid var(--line)',
              display: 'flex',
              flexWrap: 'wrap',
              justifyContent: 'space-between',
              alignItems: 'center',
              gap: '6px',
              fontSize: '10.5px',
            }}
          >
            <div class="num" style={{ display: 'flex', gap: '10px', flexWrap: 'wrap' }}>
              <span>
                Two-Phase Density Ratio ρ_l/ρ_g:{' '}
                <strong style={{ color: '#38bdf8' }}>
                  {(spec.density_kg_m3 / Math.max(0.05, spec.gas_density_kg_m3 ?? 1.225)).toFixed(1)}
                </strong>
              </span>
              <span>
                Viscosity Ratio μ_l/μ_g:{' '}
                <strong style={{ color: '#fbbf24' }}>
                  {(() => {
                    const mr = spec.viscosity_pa_s / Math.max(1e-7, spec.gas_viscosity_pa_s ?? 1.81e-5);
                    return mr >= 1e4 ? mr.toExponential(2) : mr.toFixed(1);
                  })()}
                </strong>
              </span>
            </div>
            <div style={{ display: 'flex', gap: '5px', flexWrap: 'wrap' }}>
              <button
                type="button"
                data-testid="sph-gas-preset-ambient"
                class={`btn small ${Math.abs((spec.gas_density_kg_m3 ?? 1.225) - 1.225) < 0.05 ? 'primary' : 'ghost'}`}
                onClick={() => updateSpec({ gas_density_kg_m3: 1.225, gas_viscosity_pa_s: 1.81e-5 })}
                style={{ padding: '2px 7px', fontSize: '10px' }}
              >
                Ambient Air (1.225 kg/m³)
              </button>
              <button
                type="button"
                data-testid="sph-gas-preset-combustor"
                class={`btn small ${Math.abs((spec.gas_density_kg_m3 ?? 1.225) - 4.8) < 0.1 ? 'primary' : 'ghost'}`}
                onClick={() => updateSpec({ gas_density_kg_m3: 4.8, gas_viscosity_pa_s: 3.6e-5 })}
                style={{ padding: '2px 7px', fontSize: '10px' }}
              >
                High-P Combustor Gas (4.8 kg/m³)
              </button>
            </div>
          </div>

          {/* Verification Check Summary Table */}
          <div
            style={{
              marginTop: '4px',
              padding: '6px 8px',
              borderRadius: '5px',
              background: 'var(--bg)',
              border: '1px solid var(--line)',
              fontSize: '10.5px',
            }}
          >
            <div style={{ fontWeight: 700, marginBottom: '4px' }}>
              Closed-Form Verification Gates (A15 Young–Laplace / A16 Jurin / Spout Langmuir / Peregrine Hinze)
            </div>
            <div class="num" style={{ display: 'flex', flexWrap: 'wrap', gap: '10px' }}>
              <span>
                • MLS Curvature Err: <strong>{rep.curvature_rel_error_pct.toFixed(3)}%</strong> (&lt; 1%)
              </span>
              <span>
                • Young–Laplace Err: <strong>{rep.laplace_pressure_rel_error_pct.toFixed(3)}%</strong> (&lt; 1.5%)
              </span>
              <span>
                • Jurin Rise Err: <strong>{rep.jurin_rise_rel_error_pct.toFixed(3)}%</strong> (&lt; 1.5%)
              </span>
              <span>
                • Pairwise Momentum Drift: <strong>{rep.pairwise_momentum_drift_ns.toExponential(1)} N·s</strong>
              </span>
            </div>
          </div>
        </div>
      </div>
        </>
      )}
    </div>
  );
}

export function FreeSurfaceModal() {
  useApp();
  const isOpen = Boolean(ui.freeSurfaceOpen);
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        ui.set('freeSurfaceOpen', false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div
      class="modal-backdrop"
      data-testid="free-surface-modal"
      onClick={(e) => {
        if (e.target === e.currentTarget) ui.set('freeSurfaceOpen', false);
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
        padding: '10px',
      }}
    >
      <div
        class="modal-card"
        style={{
          width: 'min(1080px, 96vw)',
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
          class="modal-head"
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: '8px',
            padding: '10px 14px',
            background: 'var(--panel-2)',
            borderBottom: '1px solid var(--line)',
            flex: 'none',
          }}
        >
          <div>
            <strong style={{ fontSize: '14px' }}>
              🌊 3D Multi-Regime Free-Surface &amp; Capillary Fluid Workbench
            </strong>
            <div style={{ fontSize: '11px', color: 'var(--text-3)' }}>
              Automatic PDE Switching: Boussinesq–Peregrine Potential ↔ DFSPH Plunging Jet ↔ MLS-CSF Hinze Capillary ↔ Implicit Stokes 1100 °C Spout-Lip Pour
            </div>
          </div>
          <button
            type="button"
            class="btn small"
            data-testid="free-surface-modal-close"
            onClick={() => ui.set('freeSurfaceOpen', false)}
            style={{ fontWeight: 700, flexShrink: 0 }}
          >
            ✕ Close
          </button>
        </div>

        <div
          style={{
            padding: '12px 14px',
            overflowY: 'auto',
            flex: 1,
            minHeight: 0,
            WebkitOverflowScrolling: 'touch',
          }}
        >
          <FreeSurfaceWorkbench />
        </div>
      </div>
    </div>
  );
}
