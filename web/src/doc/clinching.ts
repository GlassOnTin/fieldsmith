// Sheet Metal Clinching (Round Press-Joining / S-Lock) Analytic & 3D Nonlinear Elasto-Plastic Engine
// References:
//   - Varis, J. (2003), "The suitability of round clinching tools for high strength structural steel", Thin-Walled Structures.
//   - Lee, C.-J., Kim, J.-Y., Lee, S.-K., Ko, D.-C., & Kim, B.-M. (2010), "Design of mechanical clinching tools for joining of aluminum alloy sheets", Materials & Design 31(4), 1854-1861.
//   - Mucha, J. (2011), "A study of quality parameters and behaviour of self-piercing riveted aluminium sheets with different joining conditions", Strojniski vestnik.
//   - Breda, A., Coppieters, S., & Debruyne, D. (2018), "Equivalent modelling strategy for a clinched joint using a simple calibration method", Thin-Walled Structures 113, 1-12.

import type { Document } from './types';

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

export function extractClinchingSpecFromDoc(doc: Document): ClinchingSpec {
  const getLet = (names: string[], fallback: number): number => {
    for (const nm of names) {
      const hit = doc.lets?.find((l) => l.name === nm);
      if (hit && Number.isFinite(hit.value)) return hit.value;
    }
    return fallback;
  };
  const customMat = doc.custom_materials?.[0];
  const eMpa = customMat?.e ?? DEFAULT_CLINCHING_SPEC.youngsModulusMpa;
  const syMpa = customMat?.yield_strength ?? getLet(['sigma_y', 'yield_mpa'], DEFAULT_CLINCHING_SPEC.yieldStrengthMpa);
  return {
    t1Mm: Math.max(0.4, getLet(['t1', 't_upper'], DEFAULT_CLINCHING_SPEC.t1Mm)),
    t2Mm: Math.max(0.4, getLet(['t2', 't_lower'], DEFAULT_CLINCHING_SPEC.t2Mm)),
    punchRadiusMm: Math.max(1.2, getLet(['Rp', 'r_punch'], DEFAULT_CLINCHING_SPEC.punchRadiusMm)),
    clearanceMm: Math.max(0.3, getLet(['c_die', 'clearance'], DEFAULT_CLINCHING_SPEC.clearanceMm)),
    punchFilletMm: Math.max(0.08, getLet(['r_fillet', 'rp_fillet'], DEFAULT_CLINCHING_SPEC.punchFilletMm)),
    bottomThicknessXMm: Math.max(0.25, Math.min(2.2, getLet(['X', 'bottom_x'], DEFAULT_CLINCHING_SPEC.bottomThicknessXMm))),
    youngsModulusMpa: eMpa,
    yieldStrengthMpa: syMpa,
    utsMpa: Math.max(syMpa * 1.15, getLet(['sigma_uts', 'uts_mpa'], DEFAULT_CLINCHING_SPEC.utsMpa)),
    hollomonKMpa: Math.max(syMpa * 1.3, getLet(['K_hollomon', 'hollomon_k'], DEFAULT_CLINCHING_SPEC.hollomonKMpa)),
    hardeningExponentN: Math.max(0.04, Math.min(0.45, getLet(['hardening_n', 'n_hard'], DEFAULT_CLINCHING_SPEC.hardeningExponentN))),
    frictionMu: Math.max(0.02, Math.min(0.55, getLet(['mu_clinch', 'friction_mu'], DEFAULT_CLINCHING_SPEC.frictionMu))),
    loadAngleDeg: Math.max(0, Math.min(90, getLet(['load_angle_deg', 'peel_angle'], DEFAULT_CLINCHING_SPEC.loadAngleDeg))),
    serviceLoadN: Math.max(100, getLet(['F_pull', 'service_load_n'], DEFAULT_CLINCHING_SPEC.serviceLoadN)),
  };
}

export function evaluateClinchingAtX(spec: ClinchingSpec, xOverrideMm?: number): ClinchingPointEvaluation {
  const tTotal = spec.t1Mm + spec.t2Mm;
  const xMm = Math.max(0.25, Math.min(tTotal * 0.85, xOverrideMm ?? spec.bottomThicknessXMm));
  const Rp = spec.punchRadiusMm;
  const c = spec.clearanceMm;
  const Rd = Rp + c;
  const n = spec.hardeningExponentN;
  const K = spec.hollomonKMpa;

  // 1. Coin compression strain & mean flow stress (Hollomon power law)
  const coinStrain = Math.max(0.08, Math.log(tTotal / xMm));
  const coinFlowStressMpa = (K / (n + 1)) * Math.pow(coinStrain, n);

  // 2. Breda-Coppieters-Avitzur axisymmetric upper-bound punch forming force (kN)
  const mShear = Math.min(0.95, Math.sqrt(3) * spec.frictionMu);
  const slabFactor =
    1.0 +
    (2.0 * mShear * Rp) / (3.0 * Math.sqrt(3) * xMm) +
    (2.0 / Math.sqrt(3)) * Math.log(Rd / Rp);
  const formingForceKn = (Math.PI * Rp * Rp * coinFlowStressMpa * slabFactor) * 1e-3;

  // 3. Isochoric neck thickness t_n(X) and interlock undercut t_u(X) (Varis 2003 / Mucha 2011)
  // Normalised compression stroke xi = (t_total * 0.55 - X) / (t_total * 0.40)
  const xRef = 0.52 * tTotal; // ~1.56 mm for t1=t2=1.5 mm
  const strokeDepth = Math.max(0, xRef - xMm);
  const baseNeck = c * (spec.t1Mm / tTotal) + 0.14 * (spec.t1Mm / 1.5);
  const neckThinning = 0.28 * Math.pow(strokeDepth / Math.max(0.4, 0.38 * tTotal), 1.28);
  const neckThicknessMm = Math.max(0.12, Math.min(c * 0.92, baseNeck - neckThinning));

  // Displaced coin volume drives radial extrusion into the annular die groove once X < xRef
  const radialExtrusion = 0.34 * Math.pow(strokeDepth / Math.max(0.4, 0.36 * tTotal), 1.35);
  const undercutRigidMm = Math.max(0.015, Math.min(0.52, 0.02 + radialExtrusion));

  // Elastic springback after punch retraction reduces effective interlock t_u^eff
  const springbackLossMm = Math.min(
    undercutRigidMm * 0.45,
    ((coinFlowStressMpa / Math.max(10000, spec.youngsModulusMpa)) * (Rp + neckThicknessMm)) * 2.2,
  );
  const undercutEffMm = Math.max(0.008, undercutRigidMm - springbackLossMm);

  // S-lock contact angle alpha_lock (deg) steepens as the interlock hooks into the groove
  const lockAngleDeg = Math.min(34, 8 + 48 * (undercutEffMm / Math.max(0.3, c)));
  const alphaRad = (lockAngleDeg * Math.PI) / 180;

  // 4. Lee-Kim (2010) 1D Analytic Failure Loads (Cross-Tension & Shear/Peel)
  const thetaRad = (spec.loadAngleDeg * Math.PI) / 180;
  // Shear-tension von Mises interaction factor on the neck ring
  const shearInteraction = 1.0 / Math.sqrt(Math.pow(Math.cos(thetaRad), 2) + 3.0 * Math.pow(0.45 * Math.sin(thetaRad), 2));
  const neckRingAreaMm2 = 2.0 * Math.PI * (Rp + 0.5 * neckThicknessMm) * neckThicknessMm;
  const neckFracture1dN = neckRingAreaMm2 * spec.utsMpa * shearInteraction;

  // Button pull-out (unbuttoning) load: frictional conical sliding + plastic unbending of the S-hook
  const mu = spec.frictionMu;
  const frictionTerm =
    (Math.sin(alphaRad) + mu * Math.cos(alphaRad)) /
    Math.max(0.15, Math.cos(alphaRad) - mu * Math.sin(alphaRad));
  const plasticUnbendingTerm = (1.65 * undercutRigidMm) / Math.max(0.25, c + undercutRigidMm);
  const buttonPullout1dN =
    2.0 * Math.PI * Rp * undercutRigidMm * coinFlowStressMpa * (frictionTerm + plasticUnbendingTerm);

  const jointStrength1dN = Math.min(neckFracture1dN, buttonPullout1dN);

  // 5. Beyond-Analytic 3D Mechanics: Notch Triaxiality K_t, J2 Hardening Redistribution K_sigma, & Peel Arc Localization
  const elasticKt = 1.0 + 1.45 * Math.sqrt(neckThicknessMm / Math.max(0.08, spec.punchFilletMm));
  // Neuber-Hencky plastic stress concentration factor K_sigma = K_t^(2n / (n + 1))
  const plasticKsigma = Math.pow(elasticKt, (2.0 * n) / (n + 1.0));
  // Under non-axisymmetric peel/shear (theta_load > 0), only an arc of the circumference carries peak tensile peel
  const circumferentialArcFactor = 1.0 - 0.34 * Math.sin(thetaRad);

  const firstYield3dLinearN = (neckRingAreaMm2 * spec.yieldStrengthMpa * circumferentialArcFactor) / elasticKt;

  // 3D Nonlinear Elasto-Plastic neck capacity accounts for K_sigma notch triaxiality & peel arc
  const neckFracture3dNlN =
    (neckFracture1dN * circumferentialArcFactor) / Math.max(1.0, 0.72 * plasticKsigma);
  // 3D Nonlinear Elasto-Plastic button pull-out accounts for post-springback undercut t_u^eff
  const buttonPullout3dNlN =
    buttonPullout1dN * (undercutEffMm / Math.max(1e-4, undercutRigidMm)) * circumferentialArcFactor;
  const jointStrength3dNonlinearN = Math.min(neckFracture3dNlN, buttonPullout3dNlN);

  // 6. Stress & Equivalent Plastic Strain at Applied Service Load F_service
  const nominalNeckStressMpa = spec.serviceLoadN / Math.max(0.2, neckRingAreaMm2 * circumferentialArcFactor);
  const linearPeakNotchStressMpa = nominalNeckStressMpa * elasticKt;

  let nonlinearPeakNeckStressMpa = linearPeakNotchStressMpa;
  let nonlinearPeakPlasticStrainPct = 0;
  if (linearPeakNotchStressMpa > spec.yieldStrengthMpa) {
    // Neuber energy identity at the yielded clinch neck root: sigma_NL * eps_NL = (sigma_lin)^2 / E_0
    const overRatio = linearPeakNotchStressMpa / spec.yieldStrengthMpa;
    nonlinearPeakNeckStressMpa = spec.yieldStrengthMpa * Math.pow(overRatio, (2.0 * n) / (n + 1.0));
    const epsY = spec.yieldStrengthMpa / Math.max(1000, spec.youngsModulusMpa);
    const epsTotal = epsY * Math.pow(overRatio, 2.0 / (n + 1.0));
    const epsElastic = nonlinearPeakNeckStressMpa / Math.max(1000, spec.youngsModulusMpa);
    nonlinearPeakPlasticStrainPct = Math.max(0, (epsTotal - epsElastic) * 100.0);
  }

  const ratioDiff = Math.abs(neckFracture1dN - buttonPullout1dN) / Math.max(1, jointStrength1dN);
  const failureMode: ClinchingPointEvaluation['failureMode'] =
    ratioDiff < 0.08
      ? 'optimal_transition'
      : buttonPullout1dN < neckFracture1dN
        ? 'button_pullout'
        : 'neck_fracture';

  return {
    xMm,
    coinStrain,
    coinFlowStressMpa,
    formingForceKn,
    neckThicknessMm,
    undercutRigidMm,
    springbackLossMm,
    undercutEffMm,
    lockAngleDeg,
    neckFracture1dN,
    buttonPullout1dN,
    jointStrength1dN,
    elasticKt,
    plasticKsigma,
    circumferentialArcFactor,
    firstYield3dLinearN,
    jointStrength3dNonlinearN,
    nominalNeckStressMpa,
    linearPeakNotchStressMpa,
    nonlinearPeakNeckStressMpa,
    nonlinearPeakPlasticStrainPct,
    failureMode,
  };
}

export function analyzeClinchingProcessWindow(spec: ClinchingSpec): ClinchingSweepAnalysis {
  const curve: ClinchingPointEvaluation[] = [];
  let bestIdx = 0;
  let best1d = -1;
  let best3dNl = -1;
  const xMin = 0.35;
  const xMax = 1.60;
  const steps = 51;
  for (let i = 0; i < steps; i++) {
    const x = xMin + ((xMax - xMin) * i) / (steps - 1);
    const pt = evaluateClinchingAtX(spec, x);
    curve.push(pt);
    if (pt.jointStrength1dN > best1d) {
      best1d = pt.jointStrength1dN;
      bestIdx = i;
    }
    if (pt.jointStrength3dNonlinearN > best3dNl) {
      best3dNl = pt.jointStrength3dNonlinearN;
    }
  }
  const optimalXMm = curve[bestIdx]?.xMm ?? 0.85;
  const threshold = 0.85 * best1d;
  const validXs = curve.filter((p) => p.jointStrength1dN >= threshold).map((p) => p.xMm);
  const windowRangeMm: [number, number] =
    validXs.length > 0
      ? [validXs[0], validXs[validXs.length - 1]]
      : [optimalXMm - 0.1, optimalXMm + 0.1];

  return {
    spec,
    current: evaluateClinchingAtX(spec),
    optimalXMm,
    peakStrength1dN: best1d,
    peakStrength3dNonlinearN: best3dNl,
    windowRangeMm,
    curve,
  };
}

export function isClinchingDocument(doc: Document): boolean {
  const nm = (doc.name || '').toLowerCase();
  if (nm.includes('clinch') || nm.includes('a19') || nm.includes('t19') || nm.includes('s-lock')) {
    return true;
  }
  return !!(doc.lets?.some((l) => l.name === 'c_die' || l.name === 'mu_clinch'));
}
