// 4-Inch DeadCat FPV Drone Airframe + Gemini Dual-Band (`900 MHz` + `2.4 GHz` ELRS Xrossband)
// Antenna Design, VNA Scattering Matrix (`S₁₁`, `S₂₁`, `VSWR`), 50 Ω Smith Chart Impedance
// Trajectory, 3D Radiation Lobe (`G(θ, φ)`), Anisotropic Carbon-Fibre (`CFRP`) Eddy-Current
// Coupling, and Automated Geometry & L-Network Matching Optimiser.

import type { Document } from './types';

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

export function complexGamma50(rOhm: number, xOhm: number): [number, number, number] {
  const rSafe = Math.max(0.05, rOhm);
  const numRe = rSafe - Z0_OHM;
  const numIm = xOhm;
  const denRe = rSafe + Z0_OHM;
  const denIm = xOhm;
  const denNorm = Math.max(1e-12, denRe * denRe + denIm * denIm);
  const gRe = (numRe * denRe + numIm * denIm) / denNorm;
  const gIm = (numIm * denRe - numRe * denIm) / denNorm;
  const gMag = Math.min(0.99999, Math.max(1e-6, Math.hypot(gRe, gIm)));
  return [gRe, gIm, gMag];
}

export function applyLNetworkMatch(
  rRaw: number,
  xRaw: number,
  freqGhz: number,
  seriesNh: number,
  shuntPf: number,
): [number, number] {
  const omega = 2.0 * Math.PI * freqGhz * 1.0e9;
  let rAfter = rRaw;
  let xAfter = xRaw;
  if (Math.abs(shuntPf) > 1e-6) {
    const bC = omega * (shuntPf * 1.0e-12);
    const zNorm2 = Math.max(1e-9, rRaw * rRaw + xRaw * xRaw);
    const gRaw = rRaw / zNorm2;
    const bRaw = -xRaw / zNorm2;
    const bTot = bRaw + bC;
    const yNorm2 = Math.max(1e-12, gRaw * gRaw + bTot * bTot);
    rAfter = gRaw / yNorm2;
    xAfter = -bTot / yNorm2;
  }
  const xL = omega * (seriesNh * 1.0e-9);
  return [Math.max(0.2, rAfter), xAfter + xL];
}

export function evaluateVnaPoint(
  spec: DroneAntennaDesignSpec,
  freqGhz: number,
  isHighBand: boolean,
): VnaSamplePoint {
  const lambda0Mm = C0_MM_PER_GHZ / Math.max(0.1, freqGhz);
  const k0 = (2.0 * Math.PI) / lambda0Mm;

  const fillFactor = Math.min(
    0.85,
    Math.max(0.0, spec.dielectric_pod_wall_mm / (spec.dielectric_pod_wall_mm + 2.4)),
  );
  const cfrpEffDistMm = Math.max(
    2.0,
    Math.hypot(spec.cfrp_standoff_mm, 0.75 * spec.elevation_above_plate_mm),
  );
  const proximityCapLoading = 0.22 * Math.exp(-cfrpEffDistMm / 14.0);
  const epsEff = 1.0 + fillFactor * (spec.pod_eps_r - 1.0) * 0.42 + proximityCapLoading;
  const vFactor = 1.0 / Math.sqrt(epsEff);

  const activeLenMm = isHighBand ? spec.high_band_arm_length_mm : spec.low_band_arm_length_mm;
  const wireRMm = isHighBand ? 0.65 : 0.85;
  const bandName: 'low_900' | 'high_2400' = isHighBand ? 'high_2400' : 'low_900';

  let topoRScale = 1.0;
  let topoQScale = 1.0;
  let topoCfrpExposure = 1.0;
  if (spec.topology === 'conformal_arm_folded_fpc') {
    topoRScale = 1.12;
    topoQScale = 0.82;
    topoCfrpExposure = 1.35;
  } else if (spec.topology === 'split_front_rear_orthogonal') {
    topoRScale = 1.03;
    topoQScale = 0.94;
    topoCfrpExposure = 0.88;
  }

  const endShortening = 0.476;
  const fResUnloadedGhz =
    (C0_MM_PER_GHZ * endShortening * vFactor) / Math.max(10.0, activeLenMm);

  const omega = 2.0 * Math.PI * freqGhz * 1.0e9;
  const sigmaCfrp = Math.max(100.0, spec.cfrp_conductivity_s_per_m);
  const skinDepthM = Math.sqrt(2.0 / (omega * MU0_H_PER_M * sigmaCfrp));
  const rSheetOhm = 1.0 / (sigmaCfrp * skinDepthM);

  const kdImage = Math.max(0.15, 2.0 * k0 * cfrpEffDistMm);
  const j0 = Math.sin(kdImage) / kdImage;
  const y0 = -Math.cos(kdImage) / kdImage;

  const imageCoupling =
    Math.min(0.92, Math.max(0.02, 1.15 / (1.0 + 0.35 * kdImage))) * topoCfrpExposure;
  const rRadFreespace =
    73.1 *
    topoRScale *
    Math.min(2.6, Math.max(0.25, Math.pow(freqGhz / fResUnloadedGhz, 1.85)));

  const rRadEffective = Math.max(
    4.5,
    rRadFreespace * (1.0 - 0.78 * imageCoupling * Math.max(0.0, j0)),
  );
  const rCfrpEddy =
    (rSheetOhm * 3.8 + 18.5) *
    Math.exp(-cfrpEffDistMm / (0.11 * lambda0Mm + 8.0)) *
    topoCfrpExposure;
  const rDielectric = 73.1 * fillFactor * spec.pod_loss_tan * (spec.pod_eps_r - 1.0) * 1.65;
  const rConductor = 0.85 * Math.sqrt(freqGhz);

  const rTotalUnmatched = rRadEffective + rCfrpEddy + rDielectric + rConductor;

  const lnSlenderness = Math.max(2.2, Math.log(activeLenMm / wireRMm) - 1.0);
  const zChar = (120.0 * lnSlenderness * topoQScale) / Math.sqrt(epsEff);
  const fracDetune = (freqGhz - fResUnloadedGhz) / fResUnloadedGhz;
  const xDipole =
    zChar * Math.min(4.5, Math.max(-4.5, Math.tan(0.5 * Math.PI * fracDetune)));
  const xImage = -38.0 * imageCoupling * y0;
  const trapRatio =
    spec.low_band_arm_length_mm / (2.67 * Math.max(15.0, spec.high_band_arm_length_mm));
  const xTrapCrosstalk = isHighBand
    ? 14.0 * (trapRatio - 1.0)
    : -9.5 * (trapRatio - 1.0);

  const xTotalUnmatched = xDipole + xImage + xTrapCrosstalk;

  const [zRealOhm, zImagOhm] = applyLNetworkMatch(
    rTotalUnmatched,
    xTotalUnmatched,
    freqGhz,
    spec.match_series_nh,
    spec.match_shunt_pf,
  );
  const [gammaRe, gammaIm, gammaMag] = complexGamma50(zRealOhm, zImagOhm);
  const s11Db = 20.0 * Math.log10(gammaMag);
  const vswr = Math.min(50.0, (1.0 + gammaMag) / Math.max(1e-4, 1.0 - gammaMag));

  const skewRad = (spec.diversity_skew_deg * Math.PI) / 180.0;
  const polCoupling = Math.abs(Math.cos(skewRad));
  const spatialSepMm =
    spec.topology === 'split_front_rear_orthogonal'
      ? spec.wheelbase_mm * 0.72
      : spec.topology === 'conformal_arm_folded_fpc'
        ? spec.wheelbase_mm * 0.48
        : 18.0 + 0.55 * cfrpEffDistMm;
  const kSep = Math.max(0.35, k0 * spatialSepMm);
  const cfrpCommonMode = 0.14 * Math.exp(-cfrpEffDistMm / 12.0);
  const s21Lin = Math.min(
    0.85,
    Math.max(1e-3, (0.52 * polCoupling + 0.08) / Math.pow(kSep, 0.85) + cfrpCommonMode),
  );
  const s21Db = 20.0 * Math.log10(s21Lin);

  const radiationEfficiency = Math.min(
    0.99,
    Math.max(0.05, rRadEffective / Math.max(1e-6, rTotalUnmatched)),
  );
  const mismatchFactor = Math.min(1.0, Math.max(0.01, 1.0 - gammaMag * gammaMag));
  const totalEfficiency = radiationEfficiency * mismatchFactor;
  const cfrpAbsorptionFraction = Math.min(
    0.95,
    Math.max(0.0, rCfrpEddy / Math.max(1e-6, rTotalUnmatched)),
  );

  return {
    freq_ghz: freqGhz,
    band: bandName,
    z_real_ohm: zRealOhm,
    z_imag_ohm: zImagOhm,
    smith_r: zRealOhm / Z0_OHM,
    smith_x: zImagOhm / Z0_OHM,
    gamma_re: gammaRe,
    gamma_im: gammaIm,
    gamma_mag: gammaMag,
    s11_db: s11Db,
    vswr,
    s21_db: s21Db,
    radiation_efficiency: radiationEfficiency,
    total_efficiency: totalEfficiency,
    cfrp_absorption_fraction: cfrpAbsorptionFraction,
  };
}

export function evaluateDirectionalGainDbi(
  spec: DroneAntennaDesignSpec,
  freqGhz: number,
  totalEff: number,
  thetaRad: number,
  phiRad: number,
): [number, number, number, number] {
  const lambdaMm = C0_MM_PER_GHZ / Math.max(0.1, freqGhz);
  const k0 = (2.0 * Math.PI) / lambdaMm;

  const st = Math.sin(thetaRad);
  const ct = Math.cos(thetaRad);
  const ux = st * Math.cos(phiRad);
  const uy = st * Math.sin(phiRad);
  const uz = ct;

  const halfSkew = (0.5 * spec.diversity_skew_deg * Math.PI) / 180.0;
  let d1: [number, number, number];
  let d2: [number, number, number];
  if (spec.topology === 'conformal_arm_folded_fpc') {
    const rSweep = (spec.rear_arm_sweep_deg * Math.PI) / 180.0;
    d1 = [Math.cos(rSweep), -Math.sin(rSweep), 0.25];
    d2 = [-Math.cos(rSweep), -Math.sin(rSweep), 0.25];
  } else if (spec.topology === 'split_front_rear_orthogonal') {
    d1 = [1.0, 0.0, 0.0];
    const off = ((90.0 - spec.diversity_skew_deg) * Math.PI) / 180.0;
    d2 = [Math.sin(off), 0.15, Math.cos(off)];
  } else {
    const tilt = (18.0 * Math.PI) / 180.0;
    d1 = [
      Math.sin(halfSkew),
      -Math.cos(halfSkew) * Math.sin(tilt),
      Math.cos(halfSkew) * Math.cos(tilt),
    ];
    d2 = [
      -Math.sin(halfSkew),
      -Math.cos(halfSkew) * Math.sin(tilt),
      Math.cos(halfSkew) * Math.cos(tilt),
    ];
  }

  const norm3 = (v: [number, number, number]): [number, number, number] => {
    const m = Math.max(1e-9, Math.hypot(v[0], v[1], v[2]));
    return [v[0] / m, v[1] / m, v[2] / m];
  };
  const a1 = norm3(d1);
  const a2 = norm3(d2);

  const cosPsi1 = Math.min(1.0, Math.max(-1.0, ux * a1[0] + uy * a1[1] + uz * a1[2]));
  const cosPsi2 = Math.min(1.0, Math.max(-1.0, ux * a2[0] + uy * a2[1] + uz * a2[2]));
  const sinPsi1Sq = Math.max(0.025, 1.0 - cosPsi1 * cosPsi1);
  const sinPsi2Sq = Math.max(0.025, 1.0 - cosPsi2 * cosPsi2);

  const d0Lin = 1.64;
  const cfrpEffDist = Math.hypot(spec.cfrp_standoff_mm, spec.elevation_above_plate_mm);
  const forwardAlign =
    Math.pow(Math.max(0.0, uy), 2.2) *
    Math.min(1.0, Math.max(0.1, 1.0 - Math.pow(uz - 0.18, 2) * 1.8));
  const clearanceRatio = Math.min(
    2.5,
    Math.max(0.08, cfrpEffDist / (0.28 * lambdaMm + 6.0)),
  );
  const bodyShadowDb = Math.min(
    16.5,
    Math.max(0.0, (11.5 * forwardAlign) / (0.45 + 1.45 * Math.pow(clearanceRatio, 1.3))),
  );

  const fRad = (spec.front_arm_sweep_deg * Math.PI) / 180.0;
  const rRad = (spec.rear_arm_sweep_deg * Math.PI) / 180.0;
  const motorPhis = [fRad, Math.PI - fRad, -rRad, -Math.PI + rRad];
  let motorScatterDb = 0.0;
  for (const mPhi of motorPhis) {
    const dphi = Math.cos(phiRad - mPhi);
    if (dphi > 0.82) {
      const lobe = Math.pow((dphi - 0.82) / 0.18, 2);
      motorScatterDb += (2.6 * lobe * st * st) / (0.6 + 0.03 * spec.cfrp_standoff_mm);
    }
  }

  const phaseRefl = Math.cos(2.0 * k0 * spec.cfrp_standoff_mm * Math.max(0.0, -uy));
  const edgeRippleDb =
    1.65 * phaseRefl * Math.exp(-spec.cfrp_standoff_mm / (0.55 * lambdaMm));

  const cfrpShadowLossDb = bodyShadowDb + motorScatterDb;
  const envGainOffsetDb = edgeRippleDb - cfrpShadowLossDb;

  const effDb = 10.0 * Math.log10(Math.max(0.01, totalEff));
  const g1Dbi = 10.0 * Math.log10(d0Lin * sinPsi1Sq) + effDb + envGainOffsetDb;

  const p2ShadowDb =
    spec.topology === 'split_front_rear_orthogonal'
      ? Math.min(
          12.0,
          Math.max(0.0, (7.5 * Math.pow(Math.max(0.0, -uy), 2.2) * st * st) / (0.5 + clearanceRatio)),
        ) +
        0.5 * motorScatterDb
      : cfrpShadowLossDb;
  const g2Dbi = 10.0 * Math.log10(d0Lin * sinPsi2Sq) + effDb + edgeRippleDb - p2ShadowDb;

  const g1Lin = Math.pow(10.0, g1Dbi / 10.0);
  const g2Lin = Math.pow(10.0, g2Dbi / 10.0);
  const skewOrthogonality = Math.abs(
    Math.sin((spec.diversity_skew_deg * Math.PI) / 180.0),
  );
  const diversityCombineLin =
    Math.max(g1Lin, g2Lin) + 0.45 * skewOrthogonality * Math.min(g1Lin, g2Lin);
  const geminiDbi = 10.0 * Math.log10(Math.max(1e-4, diversityCombineLin));

  return [g1Dbi, g2Dbi, geminiDbi, cfrpShadowLossDb];
}

function summariseBand(
  spec: DroneAntennaDesignSpec,
  sweep: VnaSamplePoint[],
  targetFreqGhz: number,
  bandLabel: string,
): BandPerformanceSummary {
  let targetPt = sweep[0];
  let resPt = sweep[0];
  for (const pt of sweep) {
    if (Math.abs(pt.freq_ghz - targetFreqGhz) < Math.abs(targetPt.freq_ghz - targetFreqGhz)) {
      targetPt = pt;
    }
    if (pt.gamma_mag < resPt.gamma_mag) {
      resPt = pt;
    }
  }

  const stepMhz =
    sweep.length > 1
      ? (Math.abs(sweep[sweep.length - 1].freq_ghz - sweep[0].freq_ghz) * 1000.0) /
        (sweep.length - 1)
      : 5.0;
  const matchedCount = sweep.filter((pt) => pt.s11_db <= -10.0).length;
  const bandwidthMhz = matchedCount * stepMhz;

  const s11Sq = targetPt.gamma_mag * targetPt.gamma_mag;
  const s21Lin = Math.pow(10.0, targetPt.s21_db / 20.0);
  const skewCos = Math.abs(Math.cos((spec.diversity_skew_deg * Math.PI) / 180.0));
  const ecc = Math.min(
    0.95,
    Math.max(
      0.001,
      Math.pow(2.0 * targetPt.gamma_mag * s21Lin * skewCos, 2) /
        Math.pow(Math.max(0.08, 1.0 - s11Sq - s21Lin * s21Lin), 2) +
        0.04 * skewCos * skewCos,
    ),
  );

  const nAng = 72;
  const yawHorizonCut: PolarLobePoint[] = [];
  const pitchElevationCut: PolarLobePoint[] = [];
  const rollBankCut: PolarLobePoint[] = [];

  for (let i = 0; i < nAng; i++) {
    const angDeg = (i * 360.0) / nAng;
    const angRad = (angDeg * Math.PI) / 180.0;

    const [p1, p2, gem, sh] = evaluateDirectionalGainDbi(
      spec,
      targetFreqGhz,
      targetPt.total_efficiency,
      0.5 * Math.PI,
      angRad,
    );
    yawHorizonCut.push({
      angle_deg: angDeg,
      port1_gain_dbi: p1,
      port2_gain_dbi: p2,
      gemini_combined_gain_dbi: gem,
      cfrp_shadow_loss_db: sh,
    });

    const [p1P, p2P, gemP, shP] = evaluateDirectionalGainDbi(
      spec,
      targetFreqGhz,
      targetPt.total_efficiency,
      angRad,
      0.5 * Math.PI,
    );
    pitchElevationCut.push({
      angle_deg: angDeg,
      port1_gain_dbi: p1P,
      port2_gain_dbi: p2P,
      gemini_combined_gain_dbi: gemP,
      cfrp_shadow_loss_db: shP,
    });

    const [p1R, p2R, gemR, shR] = evaluateDirectionalGainDbi(
      spec,
      targetFreqGhz,
      targetPt.total_efficiency,
      angRad,
      0.0,
    );
    rollBankCut.push({
      angle_deg: angDeg,
      port1_gain_dbi: p1R,
      port2_gain_dbi: p2R,
      gemini_combined_gain_dbi: gemR,
      cfrp_shadow_loss_db: shR,
    });
  }

  const nTheta = 13;
  const nPhi = 24;
  const lobe3dRings: Lobe3dVertex[][] = [];
  const allGains: number[] = [];
  const podY = -62.0 - spec.cfrp_standoff_mm;
  const podZ = 8.0 + spec.elevation_above_plate_mm;

  for (let it = 0; it < nTheta; it++) {
    const thetaDeg = 15.0 + (it * 150.0) / (nTheta - 1);
    const thetaRad = (thetaDeg * Math.PI) / 180.0;
    const ring: Lobe3dVertex[] = [];
    for (let ip = 0; ip < nPhi; ip++) {
      const phiDeg = (ip * 360.0) / nPhi;
      const phiRad = (phiDeg * Math.PI) / 180.0;
      const [, , gemDbi] = evaluateDirectionalGainDbi(
        spec,
        targetFreqGhz,
        targetPt.total_efficiency,
        thetaRad,
        phiRad,
      );
      allGains.push(gemDbi);
      const normG = Math.min(1.15, Math.max(0.12, (gemDbi + 18.0) / 23.0));
      const rMm = 22.0 + 88.0 * normG;
      const x = rMm * Math.sin(thetaRad) * Math.cos(phiRad);
      const y = podY * 0.35 + rMm * Math.sin(thetaRad) * Math.sin(phiRad);
      const z = podZ * 0.35 + rMm * Math.cos(thetaRad);
      ring.push({
        theta_deg: thetaDeg,
        phi_deg: phiDeg,
        gain_dbi: gemDbi,
        radius_mm: rMm,
        pos_mm: [x, y, z],
      });
    }
    lobe3dRings.push(ring);
  }

  allGains.sort((a, b) => a - b);
  const peakGainDbi = allGains[allGains.length - 1] ?? 0;
  const minNullGainDbi = allGains[0] ?? -20;
  const p10Idx = Math.floor(allGains.length * 0.1);
  const p10BlindSpotFloorDbi = allGains[p10Idx] ?? minNullGainDbi;

  const gFront = yawHorizonCut[18]?.gemini_combined_gain_dbi ?? 0;
  const gRear = yawHorizonCut[54]?.gemini_combined_gain_dbi ?? 0;
  const frontToBackDb = gRear - gFront;

  const txDbm = 10.0 * Math.log10(Math.max(1.0, spec.tx_power_mw));
  const rxSensDbm = targetFreqGhz < 1.5 ? -115.0 : -108.0;
  const gTxGroundDbi = 3.5;
  const fadeMarginDb = 14.0;
  const maxFsplDb =
    txDbm + gTxGroundDbi + p10BlindSpotFloorDbi - rxSensDbm - fadeMarginDb;
  const fMhz = targetFreqGhz * 1000.0;
  const log10DKm = (maxFsplDb - 20.0 * Math.log10(fMhz) - 32.44) / 20.0;
  const estimatedLosRangeKm = Math.min(65.0, Math.max(0.15, Math.pow(10.0, log10DKm)));

  return {
    band_label: bandLabel,
    target_freq_ghz: targetFreqGhz,
    resonant_freq_ghz: resPt.freq_ghz,
    freq_detuning_mhz: (resPt.freq_ghz - targetFreqGhz) * 1000.0,
    z_at_target_ohm: [targetPt.z_real_ohm, targetPt.z_imag_ohm],
    gamma_at_target: [targetPt.gamma_re, targetPt.gamma_im],
    s11_at_target_db: targetPt.s11_db,
    vswr_at_target: targetPt.vswr,
    s21_isolation_db: targetPt.s21_db,
    envelope_correlation_ecc: ecc,
    bandwidth_s11_below_10db_mhz: bandwidthMhz,
    radiation_efficiency_pct: targetPt.radiation_efficiency * 100.0,
    total_efficiency_pct: targetPt.total_efficiency * 100.0,
    cfrp_power_loss_pct: targetPt.cfrp_absorption_fraction * 100.0,
    peak_gain_dbi: peakGainDbi,
    p10_blind_spot_floor_dbi: p10BlindSpotFloorDbi,
    min_null_gain_dbi: minNullGainDbi,
    front_to_back_db: frontToBackDb,
    estimated_los_range_km: estimatedLosRangeKm,
    yaw_horizon_cut: yawHorizonCut,
    pitch_elevation_cut: pitchElevationCut,
    roll_bank_cut: rollBankCut,
    lobe_3d_rings: lobe3dRings,
  };
}

export function suggestLNetworkMatching(spec: DroneAntennaDesignSpec): [number, number] {
  const rawSpec: DroneAntennaDesignSpec = {
    ...spec,
    match_series_nh: 0.0,
    match_shunt_pf: 0.0,
  };
  const ptLow = evaluateVnaPoint(rawSpec, spec.target_low_freq_ghz, false);
  const ptHigh = evaluateVnaPoint(rawSpec, spec.target_high_freq_ghz, true);

  let bestL = spec.match_series_nh;
  let bestC = spec.match_shunt_pf;
  let bestScore = Infinity;

  for (let il = -15; il <= 35; il++) {
    const lNh = il * 0.25;
    for (let ic = 0; ic <= 28; ic++) {
      const cPf = ic * 0.1;
      const [r1, x1] = applyLNetworkMatch(
        ptLow.z_real_ohm,
        ptLow.z_imag_ohm,
        spec.target_low_freq_ghz,
        lNh,
        cPf,
      );
      const [r2, x2] = applyLNetworkMatch(
        ptHigh.z_real_ohm,
        ptHigh.z_imag_ohm,
        spec.target_high_freq_ghz,
        lNh,
        cPf,
      );
      const [, , g1] = complexGamma50(r1, x1);
      const [, , g2] = complexGamma50(r2, x2);
      const score = Math.max(g1, g2) + 0.5 * (g1 + g2);
      if (score < bestScore) {
        bestScore = score;
        bestL = lNh;
        bestC = cPf;
      }
    }
  }
  return [bestL, bestC];
}

export function analyzeDroneAntenna(
  spec: DroneAntennaDesignSpec,
): DroneAntennaAnalysisReport {
  const nPts = 41;
  const lowBandSweep: VnaSamplePoint[] = [];
  for (let i = 0; i < nPts; i++) {
    const f = 0.82 + (i * 0.16) / (nPts - 1);
    lowBandSweep.push(evaluateVnaPoint(spec, f, false));
  }

  const highBandSweep: VnaSamplePoint[] = [];
  for (let i = 0; i < nPts; i++) {
    const f = 2.3 + (i * 0.28) / (nPts - 1);
    highBandSweep.push(evaluateVnaPoint(spec, f, true));
  }

  const lowBandSummary = summariseBand(
    spec,
    lowBandSweep,
    spec.target_low_freq_ghz,
    'Gemini Low-Band (868 / 915 MHz ELRS)',
  );
  const highBandSummary = summariseBand(
    spec,
    highBandSweep,
    spec.target_high_freq_ghz,
    'Gemini High-Band (2.40–2.48 GHz ELRS)',
  );

  const [suggestedL, suggestedC] = suggestLNetworkMatching(spec);

  const s11Worst = Math.max(
    lowBandSummary.s11_at_target_db,
    highBandSummary.s11_at_target_db,
  );
  const s21Worst = Math.max(
    lowBandSummary.s21_isolation_db,
    highBandSummary.s21_isolation_db,
  );
  const effMin = Math.min(
    lowBandSummary.total_efficiency_pct,
    highBandSummary.total_efficiency_pct,
  );
  const floorMin = Math.min(
    lowBandSummary.p10_blind_spot_floor_dbi,
    highBandSummary.p10_blind_spot_floor_dbi,
  );

  const detunePenalty =
    0.08 *
    (Math.abs(lowBandSummary.freq_detuning_mhz) +
      Math.abs(highBandSummary.freq_detuning_mhz));
  const compositeCostJ =
    (s11Worst + 14.0) * 1.8 +
    Math.max(0.0, s21Worst + 16.0) * 1.4 +
    Math.max(0.0, 75.0 - effMin) * 0.45 +
    Math.max(0.0, -4.5 - floorMin) * 2.2 +
    detunePenalty;

  const allTargetsMet =
    lowBandSummary.s11_at_target_db <= -10.0 &&
    highBandSummary.s11_at_target_db <= -10.0 &&
    s21Worst <= -15.0 &&
    effMin >= 65.0 &&
    floorMin >= -6.5;

  const notes: string[] = [];
  if (
    lowBandSummary.cfrp_power_loss_pct > 20.0 ||
    highBandSummary.cfrp_power_loss_pct > 20.0
  ) {
    notes.push(
      `High CFRP Eddy-Current Loss (${lowBandSummary.cfrp_power_loss_pct.toFixed(1)}% at 915 MHz, ${highBandSummary.cfrp_power_loss_pct.toFixed(1)}% at 2.44 GHz): antenna sits only ${spec.cfrp_standoff_mm.toFixed(1)} mm from conductive carbon plate. Increase CFRP standoff d_cfrp ≥ 20 mm and elevation ≥ 14 mm.`,
    );
  }
  if (
    Math.abs(lowBandSummary.freq_detuning_mhz) > 15.0 ||
    Math.abs(highBandSummary.freq_detuning_mhz) > 35.0
  ) {
    notes.push(
      `Dielectric Pod Detuning: TPU wall (${spec.dielectric_pod_wall_mm.toFixed(1)} mm, ε_r = ${spec.pod_eps_r.toFixed(2)}) shifts resonance by ${lowBandSummary.freq_detuning_mhz >= 0 ? '+' : ''}${lowBandSummary.freq_detuning_mhz.toFixed(0)} MHz (low) and ${highBandSummary.freq_detuning_mhz >= 0 ? '+' : ''}${highBandSummary.freq_detuning_mhz.toFixed(0)} MHz (high). Trim L_900 and L_2400 to pre-compensate for 1/√ε_eff wave slowing.`,
    );
  }
  if (spec.diversity_skew_deg < 65.0 || spec.diversity_skew_deg > 115.0) {
    notes.push(
      `Sub-Orthogonal Diversity Skew (${spec.diversity_skew_deg.toFixed(0)}°): Port 1 and Port 2 cross-couple (S₂₁ = ${s21Worst.toFixed(1)} dB, ECC = ${lowBandSummary.envelope_correlation_ecc.toFixed(3)}). Spread Gemini V-arms toward 85°–92° for orthogonal polarisation diversity.`,
    );
  }
  if (allTargetsMet) {
    notes.push(
      `Dual-Band Gemini Qualification PASS: 915 MHz S₁₁ = ${lowBandSummary.s11_at_target_db.toFixed(1)} dB (VSWR ${lowBandSummary.vswr_at_target.toFixed(2)}, η = ${lowBandSummary.total_efficiency_pct.toFixed(1)}%), 2.44 GHz S₁₁ = ${highBandSummary.s11_at_target_db.toFixed(1)} dB (VSWR ${highBandSummary.vswr_at_target.toFixed(2)}, η = ${highBandSummary.total_efficiency_pct.toFixed(1)}%), Isolation S₂₁ = ${s21Worst.toFixed(1)} dB, p10 Blind-Spot Floor = ${floorMin.toFixed(1)} dBi.`,
    );
  }

  return {
    spec: { ...spec },
    low_band_sweep: lowBandSweep,
    high_band_sweep: highBandSweep,
    low_band_summary: lowBandSummary,
    high_band_summary: highBandSummary,
    suggested_match_series_nh: suggestedL,
    suggested_match_shunt_pf: suggestedC,
    composite_cost_j: compositeCostJ,
    all_targets_met: allTargetsMet,
    engineering_notes: notes,
  };
}

export function optimiseDroneAntenna(
  initial: DroneAntennaDesignSpec,
  maxIters = 8,
): {
  initialReport: DroneAntennaAnalysisReport;
  optimisedReport: DroneAntennaAnalysisReport;
  optimisedSpec: DroneAntennaDesignSpec;
} {
  const initialReport = analyzeDroneAntenna(initial);
  let cur: DroneAntennaDesignSpec = { ...initial };
  const iters = Math.min(25, Math.max(1, maxIters));

  for (let it = 0; it < iters; it++) {
    const noMatch: DroneAntennaDesignSpec = {
      ...cur,
      match_series_nh: 0.0,
      match_shunt_pf: 0.0,
    };
    const rep = analyzeDroneAntenna(noMatch);

    const ratioLow = Math.min(
      1.22,
      Math.max(0.82, rep.low_band_summary.resonant_freq_ghz / cur.target_low_freq_ghz),
    );
    cur.low_band_arm_length_mm = Math.min(
      175.0,
      Math.max(105.0, Number((cur.low_band_arm_length_mm * ratioLow).toFixed(1))),
    );

    const ratioHigh = Math.min(
      1.22,
      Math.max(0.82, rep.high_band_summary.resonant_freq_ghz / cur.target_high_freq_ghz),
    );
    cur.high_band_arm_length_mm = Math.min(
      68.0,
      Math.max(38.0, Number((cur.high_band_arm_length_mm * ratioHigh).toFixed(1))),
    );

    const candidates: DroneAntennaDesignSpec[] = [
      {
        ...cur,
        cfrp_standoff_mm: Math.min(34.0, Math.max(5.0, cur.cfrp_standoff_mm + 4.5)),
        elevation_above_plate_mm: Math.min(
          26.0,
          Math.max(2.0, cur.elevation_above_plate_mm + 3.0),
        ),
      },
      {
        ...cur,
        diversity_skew_deg: Math.min(92.0, Math.max(30.0, cur.diversity_skew_deg + 14.0)),
        dielectric_pod_wall_mm: Math.min(
          4.0,
          Math.max(1.2, Number((cur.dielectric_pod_wall_mm - 0.35).toFixed(2))),
        ),
      },
      {
        ...cur,
        diversity_skew_deg: 88.0,
        cfrp_standoff_mm: Math.max(23.5, cur.cfrp_standoff_mm),
        elevation_above_plate_mm: Math.max(16.5, cur.elevation_above_plate_mm),
        dielectric_pod_wall_mm: Math.min(1.6, cur.dielectric_pod_wall_mm),
      },
    ];

    let bestCand = cur;
    let bestCost = analyzeDroneAntenna(cur).composite_cost_j;
    for (const cand of candidates) {
      const [lNh, cPf] = suggestLNetworkMatching(cand);
      cand.match_series_nh = lNh;
      cand.match_shunt_pf = cPf;
      const cRep = analyzeDroneAntenna(cand);
      if (cRep.composite_cost_j < bestCost) {
        bestCost = cRep.composite_cost_j;
        bestCand = cand;
      }
    }
    cur = bestCand;

    for (const deltaL of [-1.8, -0.6, 0.6, 1.8]) {
      const trial: DroneAntennaDesignSpec = {
        ...cur,
        low_band_arm_length_mm: Math.min(
          175.0,
          Math.max(105.0, Number((cur.low_band_arm_length_mm + deltaL).toFixed(1))),
        ),
      };
      const [lNh, cPf] = suggestLNetworkMatching(trial);
      trial.match_series_nh = lNh;
      trial.match_shunt_pf = cPf;
      if (analyzeDroneAntenna(trial).composite_cost_j < analyzeDroneAntenna(cur).composite_cost_j) {
        cur = trial;
      }
    }
    for (const deltaH of [-0.9, -0.3, 0.3, 0.9]) {
      const trial: DroneAntennaDesignSpec = {
        ...cur,
        high_band_arm_length_mm: Math.min(
          68.0,
          Math.max(38.0, Number((cur.high_band_arm_length_mm + deltaH).toFixed(1))),
        ),
      };
      const [lNh, cPf] = suggestLNetworkMatching(trial);
      trial.match_series_nh = lNh;
      trial.match_shunt_pf = cPf;
      if (analyzeDroneAntenna(trial).composite_cost_j < analyzeDroneAntenna(cur).composite_cost_j) {
        cur = trial;
      }
    }
  }

  const [lNh, cPf] = suggestLNetworkMatching(cur);
  cur.match_series_nh = lNh;
  cur.match_shunt_pf = cPf;

  const optimisedReport = analyzeDroneAntenna(cur);
  return {
    initialReport,
    optimisedReport,
    optimisedSpec: cur,
  };
}

/** Build or update the 4-Inch DeadCat FPV Drone + Gemini Dual-Band Antenna assembly in a Fieldsmith Document.
 *  When `spec.overlay_3d_lobe` is true, also synthesises a translucent 3D Far-Field Radiation Lobe
 *  toroid/lobe envelope around the antenna pod so the user can inspect carbon shadowing in the 3D viewport. */
export function buildDeadCatDroneFsmText(
  spec: DroneAntennaDesignSpec,
  rep?: DroneAntennaAnalysisReport,
): string {
  const report = rep ?? analyzeDroneAntenna(spec);
  const wb = Math.min(220.0, Math.max(140.0, spec.wheelbase_mm));
  const armR = 0.5 * wb;
  const fAng = (spec.front_arm_sweep_deg * Math.PI) / 180.0;
  const rAng = (spec.rear_arm_sweep_deg * Math.PI) / 180.0;

  const fmX = armR * Math.cos(fAng);
  const fmY = 20.0 + (armR - 12.0) * Math.sin(fAng);
  const rmX = armR * Math.cos(rAng);
  const rmY = -18.0 - (armR - 10.0) * Math.sin(rAng);

  const fArmCx = 0.5 * (14.0 + fmX);
  const fArmCy = 0.5 * (20.0 + fmY);
  const fArmLen = Math.hypot(fmX - 14.0, fmY - 20.0) + 14.0;
  const fArmRotZ = (Math.atan2(fmY - 20.0, fmX - 14.0) * 180.0) / Math.PI;

  const rArmCx = 0.5 * (12.0 + rmX);
  const rArmCy = 0.5 * (-18.0 + rmY);
  const rArmLen = Math.hypot(rmX - 12.0, rmY + 18.0) + 14.0;
  const rArmRotZ = (Math.atan2(-18.0 - rmY, rmX - 12.0) * 180.0) / Math.PI;

  const podY = -58.0 - spec.cfrp_standoff_mm;
  const podZ = 8.0 + spec.elevation_above_plate_mm;
  const boomLen = spec.cfrp_standoff_mm + 16.0;
  const boomY = -50.0 - 0.5 * spec.cfrp_standoff_mm;
  const boomZ = 10.0 + 0.5 * spec.elevation_above_plate_mm;
  const coaxLen = spec.cfrp_standoff_mm + 20.0;
  const halfSkew = 0.5 * spec.diversity_skew_deg;
  const bootR = 1.4 + spec.dielectric_pod_wall_mm;
  const arm900Half = Math.max(
    0.5 * spec.low_band_arm_length_mm,
    0.5 * spec.high_band_arm_length_mm + 10.0,
  );

  // Optional 3D Far-Field Radiation Lobe Overlay Group in the 3D CAD tree
  let lobeGroupFsm = '';
  if (spec.overlay_3d_lobe) {
    const activeSummary =
      spec.overlay_band === 'low_900'
        ? report.low_band_summary
        : report.high_band_summary;
    const gFront = activeSummary.yaw_horizon_cut[18]?.gemini_combined_gain_dbi ?? -3.0;
    const gRear = activeSummary.yaw_horizon_cut[54]?.gemini_combined_gain_dbi ?? 2.0;
    const gSide = activeSummary.yaw_horizon_cut[0]?.gemini_combined_gain_dbi ?? 1.5;

    const rRear = Math.min(72.0, Math.max(24.0, 42.0 + gRear * 4.5));
    const rSide = Math.min(72.0, Math.max(24.0, 42.0 + gSide * 4.5));
    const rFront = Math.min(68.0, Math.max(16.0, 42.0 + gFront * 3.8));
    const lobeColor = report.all_targets_met ? '#10B981' : '#F43F5E';

    lobeGroupFsm = `
group rf_3d_radiation_lobe_envelope {
  torus lobe_rear_main_toroid major=${(rRear * 0.72).toFixed(1)} minor=${(rSide * 0.24).toFixed(1)} at=(0, ${(podY - rRear * 0.22).toFixed(1)}, ${podZ.toFixed(1)}) rotate=(12, 0, 0)
  torus lobe_forward_diffraction_sector major=${(rFront * 0.58).toFixed(1)} minor=${(rFront * 0.18).toFixed(1)} at=(0, ${(podY + rFront * 0.35).toFixed(1)}, ${(podZ + 4.0).toFixed(1)}) rotate=(-8, 0, 0)
}
color "${lobeColor}" on rf_3d_radiation_lobe_envelope
color "${lobeColor}" on lobe_rear_main_toroid
color "${lobeColor}" on lobe_forward_diffraction_sector
`;
  }

  return `fieldsmith 1
name "4-Inch DeadCat FPV Drone + Gemini Dual-Band (900 MHz / 2.4 GHz) Antenna"
material cf_woven_3k

let wheelbase_mm = ${wb.toFixed(1)}
let cfrp_standoff_mm = ${spec.cfrp_standoff_mm.toFixed(2)}
let antenna_elev_mm = ${spec.elevation_above_plate_mm.toFixed(2)}
let gemini_l900_mm = ${spec.low_band_arm_length_mm.toFixed(2)}
let gemini_l2400_mm = ${spec.high_band_arm_length_mm.toFixed(2)}
let gemini_skew_deg = ${spec.diversity_skew_deg.toFixed(1)}
let tpu_pod_wall_mm = ${spec.dielectric_pod_wall_mm.toFixed(2)}
let match_series_nh = ${spec.match_series_nh.toFixed(2)}
let match_shunt_pf = ${spec.match_shunt_pf.toFixed(2)}
let vna_s11_915_db = ${report.low_band_summary.s11_at_target_db.toFixed(2)}
let vna_s11_2440_db = ${report.high_band_summary.s11_at_target_db.toFixed(2)}
let vna_eff_915_pct = ${report.low_band_summary.total_efficiency_pct.toFixed(1)}
let vna_eff_2440_pct = ${report.high_band_summary.total_efficiency_pct.toFixed(1)}

group cfrp_deadcat_airframe {
  box cfrp_bottom_plate size=(34, 118, 2.0) round=3 at=(0, 0, 0)
  box cfrp_top_plate size=(30, 108, 1.5) round=3 at=(0, -2, 20.0)
  box arm_front_right size=(${fArmLen.toFixed(1)}, 9.5, 3.5) round=1.2 at=(${fArmCx.toFixed(1)}, ${fArmCy.toFixed(1)}, -1.0) rotate=(0, 0, ${fArmRotZ.toFixed(1)})
  box arm_front_left size=(${fArmLen.toFixed(1)}, 9.5, 3.5) round=1.2 at=(-${fArmCx.toFixed(1)}, ${fArmCy.toFixed(1)}, -1.0) rotate=(0, 0, -${fArmRotZ.toFixed(1)})
  box arm_rear_right size=(${rArmLen.toFixed(1)}, 9.0, 3.5) round=1.2 at=(${rArmCx.toFixed(1)}, ${rArmCy.toFixed(1)}, -1.0) rotate=(0, 0, -${rArmRotZ.toFixed(1)})
  box arm_rear_left size=(${rArmLen.toFixed(1)}, 9.0, 3.5) round=1.2 at=(-${rArmCx.toFixed(1)}, ${rArmCy.toFixed(1)}, -1.0) rotate=(0, 0, ${rArmRotZ.toFixed(1)})
  cylinder standoff_fr r=2.2 h=18.5 at=(12, 34, 10)
  cylinder standoff_fl r=2.2 h=18.5 at=(-12, 34, 10)
  cylinder standoff_rr r=2.2 h=18.5 at=(12, -48, 10)
  cylinder standoff_rl r=2.2 h=18.5 at=(-12, -48, 10)
}
color "#1E293B" on cfrp_deadcat_airframe
color "#334155" on cfrp_top_plate
material ti64 on standoff_fr
material ti64 on standoff_fl
material ti64 on standoff_rr
material ti64 on standoff_rl
color "#94A3B8" on standoff_fr
color "#94A3B8" on standoff_fl
color "#94A3B8" on standoff_rr
color "#94A3B8" on standoff_rl

group propulsion_motors_1404 {
  cylinder motor_fr_bell r=9.2 h=11.5 round=1.0 at=(${fmX.toFixed(1)}, ${fmY.toFixed(1)}, 6.0)
  cylinder motor_fl_bell r=9.2 h=11.5 round=1.0 at=(-${fmX.toFixed(1)}, ${fmY.toFixed(1)}, 6.0)
  cylinder motor_rr_bell r=9.2 h=11.5 round=1.0 at=(${rmX.toFixed(1)}, ${rmY.toFixed(1)}, 6.0)
  cylinder motor_rl_bell r=9.2 h=11.5 round=1.0 at=(-${rmX.toFixed(1)}, ${rmY.toFixed(1)}, 6.0)
  cylinder prop_disc_fr r=50.8 h=1.2 at=(${fmX.toFixed(1)}, ${fmY.toFixed(1)}, 13.0)
  cylinder prop_disc_fl r=50.8 h=1.2 at=(-${fmX.toFixed(1)}, ${fmY.toFixed(1)}, 13.0)
  cylinder prop_disc_rr r=50.8 h=1.2 at=(${rmX.toFixed(1)}, ${rmY.toFixed(1)}, 13.0)
  cylinder prop_disc_rl r=50.8 h=1.2 at=(-${rmX.toFixed(1)}, ${rmY.toFixed(1)}, 13.0)
}
material ss316l on motor_fr_bell
material ss316l on motor_fl_bell
material ss316l on motor_rr_bell
material ss316l on motor_rl_bell
color "#475569" on motor_fr_bell
color "#475569" on motor_fl_bell
color "#475569" on motor_rr_bell
color "#475569" on motor_rl_bell
color "#38BDF8" on prop_disc_fr
color "#38BDF8" on prop_disc_fl
color "#38BDF8" on prop_disc_rr
color "#38BDF8" on prop_disc_rl

group avionics_and_battery {
  box esc_board_20x20 size=(26, 26, 1.8) round=1.5 at=(0, -4, 5.5)
  box fc_board_20x20 size=(25, 25, 1.6) round=1.5 at=(0, -4, 11.0)
  box elrs_gemini_rx_pcb size=(18, 22, 1.6) round=1.0 at=(0, -32, 8.5)
  box fpv_cam_cage_al7075 size=(21, 19, 19) round=2.0 at=(0, 48, 10.0)
  cylinder fpv_cam_lens r=7.0 h=10.0 at=(0, 56, 10.0) rotate=(90, 0, 0)
  box lipo_4s_850mah size=(30, 58, 24) round=3.0 at=(0, -4, 33.0)
}
color "#059669" on esc_board_20x20
color "#10B981" on fc_board_20x20
color "#2563EB" on elrs_gemini_rx_pcb
material alsi10mg on fpv_cam_cage_al7075
color "#94A3B8" on fpv_cam_cage_al7075
color "#0284C7" on fpv_cam_lens
material petg on lipo_4s_850mah
color "#F59E0B" on lipo_4s_850mah

group gemini_dual_band_antenna_assy {
  box tpu_standoff_boom size=(14, ${boomLen.toFixed(1)}, 7.0) round=2.5 at=(0, ${boomY.toFixed(1)}, ${boomZ.toFixed(1)}) rotate=(-14, 0, 0)
  box tpu_dipole_crossbar size=(26, 9.0, 9.0) round=3.0 at=(0, ${podY.toFixed(1)}, ${podZ.toFixed(1)})
  cylinder coax_feed_port1 r=1.3 h=${coaxLen.toFixed(1)} at=(4.5, ${boomY.toFixed(1)}, ${boomZ.toFixed(1)}) rotate=(76, 0, 0)
  cylinder coax_feed_port2 r=1.3 h=${coaxLen.toFixed(1)} at=(-4.5, ${boomY.toFixed(1)}, ${boomZ.toFixed(1)}) rotate=(76, 0, 0)
  cylinder gemini_p1_900mhz_dipole r=1.4 h=${spec.low_band_arm_length_mm.toFixed(1)} round=0.6 at=(6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, ${halfSkew.toFixed(1)}, 0)
  cylinder gemini_p1_2400mhz_sleeve r=2.3 h=${spec.high_band_arm_length_mm.toFixed(1)} round=0.8 at=(6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, ${halfSkew.toFixed(1)}, 0)
  cylinder gemini_p2_900mhz_dipole r=1.4 h=${spec.low_band_arm_length_mm.toFixed(1)} round=0.6 at=(-6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, -${halfSkew.toFixed(1)}, 0)
  cylinder gemini_p2_2400mhz_sleeve r=2.3 h=${spec.high_band_arm_length_mm.toFixed(1)} round=0.8 at=(-6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, -${halfSkew.toFixed(1)}, 0)
  cylinder tpu_boot_cap_p1 r=${bootR.toFixed(1)} h=${arm900Half.toFixed(1)} round=1.2 at=(6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, ${halfSkew.toFixed(1)}, 0)
  cylinder tpu_boot_cap_p2 r=${bootR.toFixed(1)} h=${arm900Half.toFixed(1)} round=1.2 at=(-6.0, ${podY.toFixed(1)}, ${podZ.toFixed(1)}) rotate=(-14, -${halfSkew.toFixed(1)}, 0)
}
material pa12 on tpu_standoff_boom
material pa12 on tpu_dipole_crossbar
material pa12 on tpu_boot_cap_p1
material pa12 on tpu_boot_cap_p2
material cucrzr on coax_feed_port1
material cucrzr on coax_feed_port2
material cucrzr on gemini_p1_900mhz_dipole
material cucrzr on gemini_p1_2400mhz_sleeve
material cucrzr on gemini_p2_900mhz_dipole
material cucrzr on gemini_p2_2400mhz_sleeve
color "#0EA5E9" on tpu_standoff_boom
color "#0284C7" on tpu_dipole_crossbar
color "#D97706" on coax_feed_port1
color "#D97706" on coax_feed_port2
color "#F59E0B" on gemini_p1_900mhz_dipole
color "#FDE047" on gemini_p1_2400mhz_sleeve
color "#F59E0B" on gemini_p2_900mhz_dipole
color "#34D399" on gemini_p2_2400mhz_sleeve
color "#38BDF8" on tpu_boot_cap_p1
color "#38BDF8" on tpu_boot_cap_p2
${lobeGroupFsm}
region rear_standoff_mount = cfrp_bottom_plate.nz
region top_deck = cfrp_top_plate.pz
support bench_fix rear_standoff_mount fixed=xyz
load prop_thrust top_deck force=(0, 0, 18)
`;
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
