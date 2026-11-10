// 4-Inch DeadCat FPV Drone Airframe + Gemini Dual-Band (`900 MHz` + `2.4 GHz` ELRS Xrossband)
// Antenna Design, VNA Scattering Matrix (`S₁₁`, `S₂₁`, `VSWR`), 50 Ω Smith Chart Impedance
// Trajectory, 3D Radiation Lobe (`G(θ, φ)`), Anisotropic Carbon-Fibre (`CFRP`) Eddy-Current
// Coupling, and Automated Geometry & L-Network Matching Optimiser.
//
// Backed directly by the canonical Rust solver (`fieldsmith_solve::antenna`) via WASM
// (`solveDroneAntennaVnaWasm` and `optimiseDroneAntennaVnaWasm`).

import type { Document } from './types';
import {
  exportDroneTouchstoneS2pWasm,
  optimiseDroneAntennaVnaWasm,
  parseTouchstoneVnaWasm,
  solveDroneAntennaVnaWasm,
} from './wasm';

export const C0_MM_PER_GHZ = 299.792458;
export const MU0_H_PER_M = 4.0e-7 * Math.PI;
export const Z0_OHM = 50.0;

export type DroneAntennaTopologyId =
  | 'rear_tpu_v_dipole_pod'
  | 'conformal_arm_folded_fpc'
  | 'split_front_rear_orthogonal';

export interface DroneAntennaTopologyOption {
  id: DroneAntennaTopologyId;
  shortLabel: string;
  label: string;
  summary: string;
}

export const DRONE_ANTENNA_TOPOLOGIES: DroneAntennaTopologyOption[] = [
  {
    id: 'rear_tpu_v_dipole_pod',
    shortLabel: 'Rear TPU V-Tail Pod',
    label: 'Rear TPU V-Tail Pod (Dual Coaxial Gemini T-Dipoles, 900 MHz + 2.4 GHz)',
    summary:
      'Two orthogonal coaxial sleeve/trap T-dipoles mounted in an elevated rear 3D-printed TPU/PA12 V-pod behind the rear CFRP standoffs.',
  },
  {
    id: 'conformal_arm_folded_fpc',
    shortLabel: 'Conformal Arm Folded FPC',
    label: 'Conformal Frame-Integrated FPC Folded Dipoles (Arm / Canopy Dielectric Guards)',
    summary:
      'Dual-band flexible PCB / FPC folded dipoles integrated inside dielectric arm bumpers along the swept DeadCat arms.',
  },
  {
    id: 'split_front_rear_orthogonal',
    shortLabel: 'Split Front/Rear Orthogonal',
    label: 'Split Front/Rear Orthogonal Diversity (Rear Horizontal + Front Canopy Vertical)',
    summary:
      'Port 1 horizontal T-dipole on rear TPU boom + Port 2 vertical dipole on the front camera canopy for 360° spatial & polarisation diversity.',
  },
];

export interface DroneAntennaDesignSpec {
  topology: DroneAntennaTopologyId;
  target_low_freq_ghz: number;
  target_high_freq_ghz: number;
  cfrp_standoff_mm: number;
  low_band_arm_length_mm: number;
  high_band_arm_length_mm: number;
  diversity_skew_deg: number;
  elevation_above_plate_mm: number;
  dielectric_pod_wall_mm: number;
  pod_eps_r: number;
  pod_loss_tan: number;
  cfrp_conductivity_s_per_m: number;
  wheelbase_mm: number;
  front_arm_sweep_deg: number;
  rear_arm_sweep_deg: number;
  match_series_nh: number;
  match_shunt_pf: number;
  tx_power_mw: number;
  overlay_3d_lobe?: boolean;
  overlay_band?: 'low_900' | 'high_2400';
}

export function defaultOptimisedDroneAntennaSpec(): DroneAntennaDesignSpec {
  return {
    topology: 'rear_tpu_v_dipole_pod',
    target_low_freq_ghz: 0.915,
    target_high_freq_ghz: 2.44,
    cfrp_standoff_mm: 26.0,
    low_band_arm_length_mm: 135.7,
    high_band_arm_length_mm: 50.8,
    diversity_skew_deg: 88.0,
    elevation_above_plate_mm: 18.0,
    dielectric_pod_wall_mm: 1.5,
    pod_eps_r: 2.75,
    pod_loss_tan: 0.014,
    cfrp_conductivity_s_per_m: 15000.0,
    wheelbase_mm: 172.0,
    front_arm_sweep_deg: 18.0,
    rear_arm_sweep_deg: 34.0,
    match_series_nh: 2.5,
    match_shunt_pf: 0.5,
    tx_power_mw: 100.0,
    overlay_3d_lobe: true,
    overlay_band: 'high_2400',
  };
}

export function unoptimisedTuckedDroneAntennaSpec(): DroneAntennaDesignSpec {
  return {
    topology: 'rear_tpu_v_dipole_pod',
    target_low_freq_ghz: 0.915,
    target_high_freq_ghz: 2.44,
    cfrp_standoff_mm: 6.5,
    low_band_arm_length_mm: 156.0,
    high_band_arm_length_mm: 59.0,
    diversity_skew_deg: 38.0,
    elevation_above_plate_mm: 4.0,
    dielectric_pod_wall_mm: 3.2,
    pod_eps_r: 2.95,
    pod_loss_tan: 0.022,
    cfrp_conductivity_s_per_m: 15000.0,
    wheelbase_mm: 172.0,
    front_arm_sweep_deg: 18.0,
    rear_arm_sweep_deg: 34.0,
    match_series_nh: 0.0,
    match_shunt_pf: 0.0,
    tx_power_mw: 100.0,
    overlay_3d_lobe: true,
    overlay_band: 'high_2400',
  };
}

export interface VnaSamplePoint {
  freq_ghz: number;
  band: 'low_900' | 'high_2400';
  z_real_ohm: number;
  z_imag_ohm: number;
  smith_r: number;
  smith_x: number;
  gamma_re: number;
  gamma_im: number;
  gamma_mag: number;
  s11_db: number;
  vswr: number;
  s21_db: number;
  radiation_efficiency: number;
  total_efficiency: number;
  cfrp_absorption_fraction: number;
}

export interface PolarLobePoint {
  angle_deg: number;
  port1_gain_dbi: number;
  port2_gain_dbi: number;
  gemini_combined_gain_dbi: number;
  cfrp_shadow_loss_db: number;
}

export interface Lobe3dVertex {
  theta_deg: number;
  phi_deg: number;
  gain_dbi: number;
  radius_mm: number;
  pos_mm: [number, number, number];
}

export interface BandPerformanceSummary {
  band_label: string;
  target_freq_ghz: number;
  resonant_freq_ghz: number;
  freq_detuning_mhz: number;
  z_at_target_ohm: [number, number];
  gamma_at_target: [number, number];
  s11_at_target_db: number;
  vswr_at_target: number;
  s21_isolation_db: number;
  envelope_correlation_ecc: number;
  bandwidth_s11_below_10db_mhz: number;
  radiation_efficiency_pct: number;
  total_efficiency_pct: number;
  cfrp_power_loss_pct: number;
  peak_gain_dbi: number;
  p10_blind_spot_floor_dbi: number;
  min_null_gain_dbi: number;
  front_to_back_db: number;
  estimated_los_range_km: number;
  yaw_horizon_cut: PolarLobePoint[];
  pitch_elevation_cut: PolarLobePoint[];
  roll_bank_cut: PolarLobePoint[];
  lobe_3d_rings: Lobe3dVertex[][];
}

export interface DroneAntennaAnalysisReport {
  spec: DroneAntennaDesignSpec;
  low_band_sweep: VnaSamplePoint[];
  high_band_sweep: VnaSamplePoint[];
  low_band_summary: BandPerformanceSummary;
  high_band_summary: BandPerformanceSummary;
  suggested_match_series_nh: number;
  suggested_match_shunt_pf: number;
  composite_cost_j: number;
  all_targets_met: boolean;
  engineering_notes: string[];
}

/** Evaluate full dual-band VNA scattering parameters, 50 Ω Smith chart locus, and 3D radiation lobes via Rust/WASM. */
export function analyzeDroneAntenna(spec: DroneAntennaDesignSpec): DroneAntennaAnalysisReport {
  const { report } = solveDroneAntennaVnaWasm<DroneAntennaAnalysisReport>(spec);
  return {
    ...report,
    spec: { ...spec, ...report.spec },
  };
}

/** Suggest analytical L-network impedance matching (`[series_nH, shunt_pF]`) via the Rust/WASM solver. */
export function suggestLNetworkMatching(spec: DroneAntennaDesignSpec): [number, number] {
  const rep = analyzeDroneAntenna(spec);
  return [rep.suggested_match_series_nh, rep.suggested_match_shunt_pf];
}

/** Run multi-objective coordinate & L-network matching optimisation via the Rust/WASM solver. */
export function optimiseDroneAntenna(
  initial: DroneAntennaDesignSpec,
  maxIters = 8,
): {
  initialReport: DroneAntennaAnalysisReport;
  optimisedReport: DroneAntennaAnalysisReport;
  optimisedSpec: DroneAntennaDesignSpec;
} {
  const out = optimiseDroneAntennaVnaWasm<DroneAntennaAnalysisReport>(initial, maxIters);
  const optimisedSpec: DroneAntennaDesignSpec = {
    ...initial,
    ...out.optimised_report.spec,
    overlay_3d_lobe: initial.overlay_3d_lobe,
    overlay_band: initial.overlay_band,
  };
  return {
    initialReport: out.initial_report,
    optimisedReport: { ...out.optimised_report, spec: optimisedSpec },
    optimisedSpec,
  };
}

/** Build or update the 4-Inch DeadCat FPV Drone + Gemini Dual-Band Antenna assembly in a Fieldsmith Document.
 *  Uses the canonical Rust `build_deadcat_drone_fsm` output from WASM, optionally splicing the 3D radiation
 *  lobe envelope group when `spec.overlay_3d_lobe` is true. */
export function buildDeadCatDroneFsmText(
  spec: DroneAntennaDesignSpec,
  rep?: DroneAntennaAnalysisReport,
): string {
  const solved = solveDroneAntennaVnaWasm<DroneAntennaAnalysisReport>(spec);
  const report = rep ?? solved.report;
  const baseFsm = solved.fsm_3d;

  if (!spec.overlay_3d_lobe) {
    return baseFsm;
  }

  const rearPodY = -58.0 - spec.cfrp_standoff_mm;
  const rearPodZ = 8.0 + spec.elevation_above_plate_mm;
  const activeSummary =
    spec.overlay_band === 'low_900' ? report.low_band_summary : report.high_band_summary;
  const gFront = activeSummary.yaw_horizon_cut[18]?.gemini_combined_gain_dbi ?? -3.0;
  const gRear = activeSummary.yaw_horizon_cut[54]?.gemini_combined_gain_dbi ?? 2.0;
  const gSide = activeSummary.yaw_horizon_cut[0]?.gemini_combined_gain_dbi ?? 1.5;

  const rRear = Math.min(72.0, Math.max(24.0, 42.0 + gRear * 4.5));
  const rSide = Math.min(72.0, Math.max(24.0, 42.0 + gSide * 4.5));
  const rFront = Math.min(68.0, Math.max(16.0, 42.0 + gFront * 3.8));
  const lobeColor = report.all_targets_met ? '#10B981' : '#F43F5E';

  let mainLobeAt: [number, number, number] = [0.0, rearPodY - rRear * 0.22, rearPodZ];
  let secondaryLobeAt: [number, number, number] = [0.0, rearPodY + rFront * 0.35, rearPodZ + 4.0];
  let mainLobeRot: [number, number, number] = [12, 0, 0];
  let secondaryLobeRot: [number, number, number] = [-8, 0, 0];

  if (spec.topology === 'conformal_arm_folded_fpc') {
    const wb = Math.min(220.0, Math.max(140.0, spec.wheelbase_mm ?? 172.0));
    const armR = 0.5 * wb;
    const rAng = ((spec.rear_arm_sweep_deg ?? 34.0) * Math.PI) / 180.0;
    const rmX = armR * Math.cos(rAng);
    const rmY = -18.0 - (armR - 10.0) * Math.sin(rAng);
    const armFeedX = 0.5 * (6.0 + rmX);
    const armFeedY = 0.5 * (-18.0 + rmY) - 0.35 * spec.cfrp_standoff_mm;
    const armFeedZ = 6.0 + 0.5 * spec.elevation_above_plate_mm;
    mainLobeAt = [armFeedX, armFeedY, armFeedZ];
    secondaryLobeAt = [-armFeedX, armFeedY, armFeedZ];
    mainLobeRot = [8, 10, -(spec.rear_arm_sweep_deg ?? 34.0)];
    secondaryLobeRot = [8, -10, spec.rear_arm_sweep_deg ?? 34.0];
  } else if (spec.topology === 'split_front_rear_orthogonal') {
    const frontCanopyY = 46.0 + 0.45 * spec.cfrp_standoff_mm;
    const frontCanopyZ = 22.0 + 0.65 * spec.elevation_above_plate_mm;
    mainLobeAt = [0.0, rearPodY - rRear * 0.18, rearPodZ];
    secondaryLobeAt = [0.0, frontCanopyY + rFront * 0.18, frontCanopyZ];
    mainLobeRot = [12, 0, 0];
    secondaryLobeRot = [-10, 0, 90];
  }

  const lobeGroupFsm = `
group rf_3d_radiation_lobe_envelope {
  torus lobe_rear_main_toroid major=${(rRear * 0.72).toFixed(1)} minor=${(rSide * 0.24).toFixed(1)} at=(${mainLobeAt[0].toFixed(1)}, ${mainLobeAt[1].toFixed(1)}, ${mainLobeAt[2].toFixed(1)}) rotate=(${mainLobeRot[0]}, ${mainLobeRot[1]}, ${mainLobeRot[2].toFixed(1)})
  torus lobe_forward_diffraction_sector major=${(rFront * 0.58).toFixed(1)} minor=${(rFront * 0.18).toFixed(1)} at=(${secondaryLobeAt[0].toFixed(1)}, ${secondaryLobeAt[1].toFixed(1)}, ${secondaryLobeAt[2].toFixed(1)}) rotate=(${secondaryLobeRot[0]}, ${secondaryLobeRot[1]}, ${secondaryLobeRot[2].toFixed(1)})
}
color "${lobeColor}" on rf_3d_radiation_lobe_envelope
color "${lobeColor}" on lobe_rear_main_toroid
color "${lobeColor}" on lobe_forward_diffraction_sector

region rear_standoff_mount`;

  return baseFsm.replace('region rear_standoff_mount', lobeGroupFsm);
}

/** Extract current DroneAntennaDesignSpec from an open DeadCat drone `.fsm` Document if present. */
export function extractDroneAntennaSpecFromDoc(
  doc: Document,
): DroneAntennaDesignSpec | null {
  const getLet = (name: string): number | undefined =>
    doc.lets?.find((l) => l.name === name)?.value;
  const standoff = getLet('cfrp_standoff_mm');
  const l900 = getLet('gemini_l900_mm');
  const l2400 = getLet('gemini_l2400_mm');
  if (standoff === undefined && l900 === undefined && l2400 === undefined) {
    return null;
  }
  const base = defaultOptimisedDroneAntennaSpec();
  return {
    ...base,
    wheelbase_mm: getLet('wheelbase_mm') ?? base.wheelbase_mm,
    cfrp_standoff_mm: standoff ?? base.cfrp_standoff_mm,
    elevation_above_plate_mm: getLet('antenna_elev_mm') ?? base.elevation_above_plate_mm,
    low_band_arm_length_mm: l900 ?? base.low_band_arm_length_mm,
    high_band_arm_length_mm: l2400 ?? base.high_band_arm_length_mm,
    diversity_skew_deg: getLet('gemini_skew_deg') ?? base.diversity_skew_deg,
    dielectric_pod_wall_mm: getLet('tpu_pod_wall_mm') ?? base.dielectric_pod_wall_mm,
    match_series_nh: getLet('match_series_nh') ?? base.match_series_nh,
    match_shunt_pf: getLet('match_shunt_pf') ?? base.match_shunt_pf,
  };
}

/** Format a DroneAntennaAnalysisReport as an industry-standard Touchstone 2-port `.s2p` file
 *  (`# GHz S DB R 50`, `VISION.md §5.2 Pillar IV.4`). */
export function formatTouchstoneS2p(report: DroneAntennaAnalysisReport): string {
  try {
    return exportDroneTouchstoneS2pWasm(report.spec);
  } catch {
    // Fallback when WASM is not yet initialized
  }
  const lines: string[] = [
    '! Fieldsmith VNA Touchstone 2-Port Scattering Matrix (.s2p)',
    `! Topology: ${report.spec.topology} | d_cfrp=${report.spec.cfrp_standoff_mm.toFixed(1)} mm | L_900=${report.spec.low_band_arm_length_mm.toFixed(1)} mm | L_2400=${report.spec.high_band_arm_length_mm.toFixed(1)} mm | skew=${report.spec.diversity_skew_deg.toFixed(1)} deg`,
    '# GHz S DB R 50',
    '! freq_GHz     S11_dB    S11_deg     S21_dB    S21_deg     S12_dB    S12_deg     S22_dB    S22_deg',
  ];
  const distMm =
    report.spec.topology === 'split_front_rear_orthogonal'
      ? 140.0 + report.spec.cfrp_standoff_mm
      : report.spec.topology === 'conformal_arm_folded_fpc'
        ? 95.0 + report.spec.cfrp_standoff_mm
        : Math.max(12.0, 28.0 + 0.35 * report.spec.cfrp_standoff_mm);
  const allPts = [...report.low_band_sweep, ...report.high_band_sweep];
  for (const pt of allPts) {
    const s11Deg = (Math.atan2(pt.gamma_im, pt.gamma_re) * 180.0) / Math.PI;
    const s21Deg = -360.0 * pt.freq_ghz * (distMm / C0_MM_PER_GHZ) - 90.0;
    lines.push(
      `${pt.freq_ghz.toFixed(6).padStart(10)} ${pt.s11_db.toFixed(4).padStart(10)} ${s11Deg.toFixed(3).padStart(10)} ${pt.s21_db.toFixed(4).padStart(10)} ${s21Deg.toFixed(3).padStart(10)} ${pt.s21_db.toFixed(4).padStart(10)} ${s21Deg.toFixed(3).padStart(10)} ${pt.s11_db.toFixed(4).padStart(10)} ${s11Deg.toFixed(3).padStart(10)}`,
    );
  }
  return lines.join('\n') + '\n';
}

/** Parse an industry-standard Touchstone `.s1p` or `.s2p` bench VNA trace into `VnaSamplePoint[]`
 *  for direct overlay on the 50 Ω Smith Chart and VNA return-loss plots (`VISION.md §5.2 Pillar IV.4`). */
export function parseTouchstoneVna(text: string): VnaSamplePoint[] {
  try {
    return parseTouchstoneVnaWasm<VnaSamplePoint[]>(text);
  } catch {
    // Fallback when WASM is not yet initialized
  }
  let freqScaleToGhz = 1.0;
  let formatMode: 'DB' | 'MA' | 'RI' = 'DB';
  let z0 = Z0_OHM;
  const pts: VnaSamplePoint[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith('!')) continue;
    if (line.startsWith('#')) {
      const tokens = line
        .slice(1)
        .trim()
        .split(/\s+/)
        .map((t) => t.toUpperCase());
      for (let i = 0; i < tokens.length; i++) {
        const tok = tokens[i];
        if (tok === 'HZ') freqScaleToGhz = 1e-9;
        else if (tok === 'KHZ') freqScaleToGhz = 1e-6;
        else if (tok === 'MHZ') freqScaleToGhz = 1e-3;
        else if (tok === 'GHZ') freqScaleToGhz = 1.0;
        else if (tok === 'DB') formatMode = 'DB';
        else if (tok === 'MA') formatMode = 'MA';
        else if (tok === 'RI') formatMode = 'RI';
        else if (tok === 'R' && i + 1 < tokens.length) {
          const rVal = parseFloat(tokens[i + 1]);
          if (Number.isFinite(rVal) && rVal > 0) z0 = rVal;
          i++;
        }
      }
      continue;
    }

    const nums = line.split(/\s+/).map((v) => parseFloat(v));
    if (nums.length < 3 || nums.some((n) => !Number.isFinite(n))) continue;
    const freqGhz = nums[0] * freqScaleToGhz;
    let gRe = 0;
    let gIm = 0;
    if (formatMode === 'DB') {
      const mag = Math.min(0.99999, Math.max(1e-6, Math.pow(10, nums[1] / 20)));
      const rad = (nums[2] * Math.PI) / 180;
      gRe = mag * Math.cos(rad);
      gIm = mag * Math.sin(rad);
    } else if (formatMode === 'MA') {
      const mag = Math.min(0.99999, Math.max(1e-6, nums[1]));
      const rad = (nums[2] * Math.PI) / 180;
      gRe = mag * Math.cos(rad);
      gIm = mag * Math.sin(rad);
    } else {
      gRe = nums[1];
      gIm = nums[2];
    }
    const gMag = Math.min(0.99999, Math.max(1e-6, Math.hypot(gRe, gIm)));
    const s11Db = 20 * Math.log10(gMag);
    const vswr = (1 + gMag) / Math.max(1e-6, 1 - gMag);
    const den = Math.max(1e-9, Math.pow(1 - gRe, 2) + gIm * gIm);
    const smithR = Math.max(0.01, (1 - gRe * gRe - gIm * gIm) / den);
    const smithX = (2 * gIm) / den;
    let s21Db = -22.0;
    if (nums.length >= 5) {
      if (formatMode === 'DB') s21Db = nums[3];
      else if (formatMode === 'MA') s21Db = 20 * Math.log10(Math.max(1e-6, nums[3]));
      else s21Db = 20 * Math.log10(Math.max(1e-6, Math.hypot(nums[3], nums[4])));
    }
    const s11LinPwr = Math.pow(10, s11Db / 10);
    const s21LinPwr = Math.pow(10, s21Db / 10);
    const cfrpAbsorptionFraction = Math.max(0.0, Math.min(0.95, 1.0 - s11LinPwr - s21LinPwr));
    const radiationEfficiency = Math.max(0.05, 1.0 - cfrpAbsorptionFraction * 0.45);
    const totalEfficiency = Math.max(0.01, (1.0 - s11LinPwr) * radiationEfficiency);
    pts.push({
      freq_ghz: freqGhz,
      band: freqGhz < 1.5 ? 'low_900' : 'high_2400',
      z_real_ohm: smithR * z0,
      z_imag_ohm: smithX * z0,
      smith_r: smithR,
      smith_x: smithX,
      gamma_re: gRe,
      gamma_im: gIm,
      gamma_mag: gMag,
      s11_db: s11Db,
      vswr,
      s21_db: s21Db,
      radiation_efficiency: radiationEfficiency,
      total_efficiency: totalEfficiency,
      cfrp_absorption_fraction: cfrpAbsorptionFraction,
    });
  }
  return pts;
}
