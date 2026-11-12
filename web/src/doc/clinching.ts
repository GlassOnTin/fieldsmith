// Sheet Metal Clinching (Round Press-Joining / S-Lock) Analytic & 3D Nonlinear Elasto-Plastic Engine
// References:
//   - Varis, J. (2003), "The suitability of round clinching tools for high strength structural steel", Thin-Walled Structures.
//   - Lee, C.-J., Kim, J.-Y., Lee, S.-K., Ko, D.-C., & Kim, B.-M. (2010), "Design of mechanical clinching tools for joining of aluminum alloy sheets", Materials & Design 31(4), 1854-1861.
//   - Mucha, J. (2011), "A study of quality parameters and behaviour of self-piercing riveted aluminium sheets with different joining conditions", Strojniski vestnik.
//   - Breda, A., Coppieters, S., & Debruyne, D. (2018), "Equivalent modelling strategy for a clinched joint using a simple calibration method", Thin-Walled Structures 113, 1-12.

import type { Document } from './types';
import {
  analyzeClinchingWindowWasm,
  evaluateClinchingPointWasm,
  extractClinchingSpecWasm,
  isClinchingDocWasm,
} from './wasm';

export interface ClinchingSpec {
  /** Upper (punch-side) sheet thickness t_1 (mm). */
  t1Mm: number;
  /** Lower (die-side) sheet thickness t_2 (mm). */
  t2Mm: number;
  /** Punch radius R_p (mm). */
  punchRadiusMm: number;
  /** Radial punch-to-die clearance c = R_d - R_p (mm). */
  clearanceMm: number;
  /** Punch corner fillet radius r_p (mm) — controls 3D notch triaxiality K_t at the neck root. */
  punchFilletMm: number;
  /** Residual compressed bottom coin thickness X (mm) — the primary dial-in process parameter. */
  bottomThicknessXMm: number;
  /** Sheet metal elastic Young's modulus E_0 (MPa). */
  youngsModulusMpa: number;
  /** Initial yield strength sigma_y (MPa). */
  yieldStrengthMpa: number;
  /** Ultimate tensile strength sigma_UTS (MPa). */
  utsMpa: number;
  /** Hollomon strength coefficient K (MPa) in sigma = K * eps^n. */
  hollomonKMpa: number;
  /** Hollomon / Ramberg-Osgood strain-hardening exponent n (dimensionless, typically 0.12-0.26). */
  hardeningExponentN: number;
  /** Interfacial sheet-to-sheet Coulomb friction coefficient mu. */
  frictionMu: number;
  /** Service loading angle theta_load (deg): 0 = pure cross-tension (axial pull-out), 45 = mixed peel/shear, 90 = lap-shear. */
  loadAngleDeg: number;
  /** Applied service pull force F_service (N). */
  serviceLoadN: number;
}

export interface ClinchingPointEvaluation {
  xMm: number;
  /** Effective logarithmic compression strain in the bottom coin ln((t1+t2)/X). */
  coinStrain: number;
  /** Mean flow stress in the coin (MPa). */
  coinFlowStressMpa: number;
  /** Breda-Avitzur upper-bound punch forming force F_punch (kN). */
  formingForceKn: number;
  /** Isochoric thinned neck wall thickness t_n(X) (mm). */
  neckThicknessMm: number;
  /** Rigid-plastic geometric interlock undercut t_u(X) before springback (mm). */
  undercutRigidMm: number;
  /** Radial elastic springback relaxation delta_tu (mm). */
  springbackLossMm: number;
  /** Effective post-springback mechanical interlock t_u^eff(X) (mm). */
  undercutEffMm: number;
  /** S-lock undercut contact angle alpha_lock (deg). */
  lockAngleDeg: number;
  /** 1D Lee-Kim (2010) analytic neck fracture load F_neck^1D (N). */
  neckFracture1dN: number;
  /** 1D Lee-Kim (2010) analytic button pull-out (unbuttoning) load F_pull^1D (N). */
  buttonPullout1dN: number;
  /** 1D analytic joint strength F_joint^1D = min(F_neck^1D, F_pull^1D) (N). */
  jointStrength1dN: number;
  /** Elastic notch stress concentration factor K_t at the punch-to-neck fillet r_p. */
  elasticKt: number;
  /** Neuber-Hencky elasto-plastic stress concentration factor K_sigma = K_t^(2n/(n+1)). */
  plasticKsigma: number;
  /** Non-axisymmetric peel/shear circumferential arc factor eta_arc(theta_load) in (0, 1]. */
  circumferentialArcFactor: number;
  /** 3D Linear Elastic predicted first-yield load (N) — overly conservative due to unredistributed notch spike. */
  firstYield3dLinearN: number;
  /** 3D Nonlinear Elasto-Plastic (J2 / Ramberg-Osgood) joint strength F_joint^3D-NL (N) accounting for K_sigma redistribution, springback, and peel arc localization. */
  jointStrength3dNonlinearN: number;
  /** Average nominal axial neck stress at serviceLoadN (MPa). */
  nominalNeckStressMpa: number;
  /** 3D Linear Elastic peak notch von Mises stress at serviceLoadN (MPa). */
  linearPeakNotchStressMpa: number;
  /** 3D Nonlinear Elasto-Plastic redistributed neck flow stress at serviceLoadN (MPa). */
  nonlinearPeakNeckStressMpa: number;
  /** 3D Nonlinear Elasto-Plastic peak equivalent plastic strain eps_p (%) at serviceLoadN. */
  nonlinearPeakPlasticStrainPct: number;
  /** Dominant failure mode at this X ('button_pullout' | 'optimal_transition' | 'neck_fracture'). */
  failureMode: 'button_pullout' | 'optimal_transition' | 'neck_fracture';
}

export interface ClinchingSweepAnalysis {
  spec: ClinchingSpec;
  current: ClinchingPointEvaluation;
  /** Optimal bottom coin thickness X* (mm) where F_pull(X*) = F_neck(X*). */
  optimalXMm: number;
  /** Peak 1D analytic joint strength at X* (N). */
  peakStrength1dN: number;
  /** Peak 3D nonlinear elasto-plastic joint strength (N). */
  peakStrength3dNonlinearN: number;
  /** Safe process window [X_min, X_max] achieving >= 85% of peak strength (mm). */
  windowRangeMm: [number, number];
  /** Sampled hat-curve points from X = 0.35 mm to X = 1.60 mm. */
  curve: ClinchingPointEvaluation[];
}

export const DEFAULT_CLINCHING_SPEC: ClinchingSpec = {
  t1Mm: 1.5,
  t2Mm: 1.5,
  punchRadiusMm: 2.6,
  clearanceMm: 0.75,
  punchFilletMm: 0.30,
  bottomThicknessXMm: 0.85,
  youngsModulusMpa: 70000,
  yieldStrengthMpa: 65,
  utsMpa: 240,
  hollomonKMpa: 390,
  hardeningExponentN: 0.20,
  frictionMu: 0.18,
  loadAngleDeg: 0,
  serviceLoadN: 2400,
};

function mapRustSpec(raw: Record<string, unknown>): ClinchingSpec {
  return {
    t1Mm: Number(raw.t1_mm),
    t2Mm: Number(raw.t2_mm),
    punchRadiusMm: Number(raw.punch_radius_mm),
    clearanceMm: Number(raw.clearance_mm),
    punchFilletMm: Number(raw.punch_fillet_mm),
    bottomThicknessXMm: Number(raw.bottom_thickness_x_mm),
    youngsModulusMpa: Number(raw.youngs_modulus_mpa),
    yieldStrengthMpa: Number(raw.yield_strength_mpa),
    utsMpa: Number(raw.uts_mpa),
    hollomonKMpa: Number(raw.hollomon_k_mpa),
    hardeningExponentN: Number(raw.hardening_exponent_n),
    frictionMu: Number(raw.friction_mu),
    loadAngleDeg: Number(raw.load_angle_deg),
    serviceLoadN: Number(raw.service_load_n),
  };
}

function mapRustPoint(raw: Record<string, unknown>): ClinchingPointEvaluation {
  return {
    xMm: Number(raw.x_mm),
    coinStrain: Number(raw.coin_strain),
    coinFlowStressMpa: Number(raw.coin_flow_stress_mpa),
    formingForceKn: Number(raw.forming_force_kn),
    neckThicknessMm: Number(raw.neck_thickness_mm),
    undercutRigidMm: Number(raw.undercut_rigid_mm),
    springbackLossMm: Number(raw.springback_loss_mm),
    undercutEffMm: Number(raw.undercut_eff_mm),
    lockAngleDeg: Number(raw.lock_angle_deg),
    neckFracture1dN: Number(raw.neck_fracture_1d_n),
    buttonPullout1dN: Number(raw.button_pullout_1d_n),
    jointStrength1dN: Number(raw.joint_strength_1d_n),
    elasticKt: Number(raw.elastic_kt),
    plasticKsigma: Number(raw.plastic_ksigma),
    circumferentialArcFactor: Number(raw.circumferential_arc_factor),
    firstYield3dLinearN: Number(raw.first_yield_3d_linear_n),
    jointStrength3dNonlinearN: Number(raw.joint_strength_3d_nonlinear_n),
    nominalNeckStressMpa: Number(raw.nominal_neck_stress_mpa),
    linearPeakNotchStressMpa: Number(raw.linear_peak_notch_stress_mpa),
    nonlinearPeakNeckStressMpa: Number(raw.nonlinear_peak_neck_stress_mpa),
    nonlinearPeakPlasticStrainPct: Number(raw.nonlinear_peak_plastic_strain_pct),
    failureMode: raw.failure_mode as ClinchingPointEvaluation['failureMode'],
  };
}

export function extractClinchingSpecFromDoc(doc: Document): ClinchingSpec {
  return mapRustSpec(extractClinchingSpecWasm(doc));
}

export function evaluateClinchingAtX(
  spec: ClinchingSpec,
  xOverrideMm?: number,
): ClinchingPointEvaluation {
  return mapRustPoint(
    evaluateClinchingPointWasm(spec as unknown as Record<string, unknown>, xOverrideMm),
  );
}

export function analyzeClinchingProcessWindow(spec: ClinchingSpec): ClinchingSweepAnalysis {
  const raw = analyzeClinchingWindowWasm(spec as unknown as Record<string, unknown>);
  const win = raw.window_range_mm as [number, number];
  const curveRaw = (raw.curve as Record<string, unknown>[]) ?? [];
  return {
    spec: mapRustSpec(raw.spec as Record<string, unknown>),
    current: mapRustPoint(raw.current as Record<string, unknown>),
    optimalXMm: Number(raw.optimal_x_mm),
    peakStrength1dN: Number(raw.peak_strength_1d_n),
    peakStrength3dNonlinearN: Number(raw.peak_strength_3d_nonlinear_n),
    windowRangeMm: [Number(win[0]), Number(win[1])],
    curve: curveRaw.map(mapRustPoint),
  };
}

export function isClinchingDocument(doc: Document): boolean {
  return isClinchingDocWasm(doc);
}
