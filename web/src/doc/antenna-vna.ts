// 4-Inch DeadCat FPV Drone Airframe + Gemini Dual-Band (`900 MHz` + `2.4 GHz` ELRS Xrossband)
// Antenna Design, VNA Scattering Matrix (`S₁₁`, `S₂₁`, `VSWR`), 50 Ω Smith Chart Impedance
// Trajectory, 3D Radiation Lobe (`G(θ, φ)`), Anisotropic Carbon-Fibre (`CFRP`) Eddy-Current
// Coupling, and Automated Geometry & L-Network Matching Optimiser.
//
// Backed directly by the canonical Rust solver (`fieldsmith_solve::antenna`) via WASM
// (`solveDroneAntennaVnaWasm` and `optimiseDroneAntennaVnaWasm`).

import type { Document } from './types';
import { optimiseDroneAntennaVnaWasm, solveDroneAntennaVnaWasm } from './wasm';

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

  const podY = -58.0 - spec.cfrp_standoff_mm;
  const podZ = 8.0 + spec.elevation_above_plate_mm;
  const activeSummary =
    spec.overlay_band === 'low_900' ? report.low_band_summary : report.high_band_summary;
  const gFront = activeSummary.yaw_horizon_cut[18]?.gemini_combined_gain_dbi ?? -3.0;
  const gRear = activeSummary.yaw_horizon_cut[54]?.gemini_combined_gain_dbi ?? 2.0;
  const gSide = activeSummary.yaw_horizon_cut[0]?.gemini_combined_gain_dbi ?? 1.5;

  const rRear = Math.min(72.0, Math.max(24.0, 42.0 + gRear * 4.5));
  const rSide = Math.min(72.0, Math.max(24.0, 42.0 + gSide * 4.5));
  const rFront = Math.min(68.0, Math.max(16.0, 42.0 + gFront * 3.8));
  const lobeColor = report.all_targets_met ? '#10B981' : '#F43F5E';

  const lobeGroupFsm = `
group rf_3d_radiation_lobe_envelope {
  torus lobe_rear_main_toroid major=${(rRear * 0.72).toFixed(1)} minor=${(rSide * 0.24).toFixed(1)} at=(0, ${(podY - rRear * 0.22).toFixed(1)}, ${podZ.toFixed(1)}) rotate=(12, 0, 0)
  torus lobe_forward_diffraction_sector major=${(rFront * 0.58).toFixed(1)} minor=${(rFront * 0.18).toFixed(1)} at=(0, ${(podY + rFront * 0.35).toFixed(1)}, ${(podZ + 4.0).toFixed(1)}) rotate=(-8, 0, 0)
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
