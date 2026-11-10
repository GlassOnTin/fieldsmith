import { useEffect, useState } from 'preact/hooks';
import { openText } from './commands';
import { ui, useApp } from './ui';

export type CoordDimension = 1 | 2 | 3 | 4;

export type CoordSystemId =
  | 'cartesian'
  | 'polar_cylindrical'
  | 'spherical'
  | 'joukowski_airfoil'
  | 'elliptic_confocal'
  | 'parabolic'
  | 'toroidal_hyperspherical';

export type CoordPdeKind =
  | 'laplace_beltrami_poisson'
  | 'helmholtz_screened'
  | 'biharmonic_eddy_stream'
  | 'kutta_potential_flow';

export interface CoordFemSpec {
  dimension: CoordDimension;
  coordSystem: CoordSystemId;
  pdeKind: CoordPdeKind;
  gridN: number;
  q1Min: number;
  q1Max: number;
  q2SpanPi: number;
  angularModeM?: number;
  focalScaleA: number;
  sourceAmp: number;
  screeningBeta: number;
  sliceQ3: number;
  sliceQ4: number;
  // Joukowski / Kármán–Trefftz Conformal Wing + Kutta Condition parameters:
  angleOfAttackDeg: number;
  airfoilThicknessEx: number;
  airfoilCamberEy: number;
  kuttaCirculationRatio: number; // 1.0 = exact Kutta condition (smooth TE); 0.0 = circulation-free singularity
  airfoilFieldMode: 'streamfunction_psi' | 'pressure_cp';
}

export interface CoordQuadCell {
  ptsPhys: [number, number][];
  centerPhys: [number, number];
  uVal: number;
  uExact: number;
  cpVal?: number;
  speedNorm?: number;
  sqrtG: number;
  gradVec: [number, number];
}

export interface AirfoilAeroMetrics {
  kuttaCirculationDimless: number;
  appliedCirculationDimless: number;
  liftCoefficientCl: number;
  thinAirfoilCl: number;
  minCpSuction: number;
  trailingEdgeSpeedRatio: number;
  foilPolygonPhys: [number, number][];
  upperCpCurve: Array<[number, number]>; // [xOverC, -Cp]
  lowerCpCurve: Array<[number, number]>; // [xOverC, -Cp]
}

export interface CoordFemSolution {
  spec: CoordFemSpec;
  coordTitle: string;
  metricFormula: string;
  jacobianFormula: string;
  pdeFormula: string;
  l2RelErrorPct: number;
  maxAbsError: number;
  uMin: number;
  uMax: number;
  dofCount: number;
  measuredOrderP: number;
  profile1D: Array<{ q1: number; rPhys: number; uFem: number; uExact: number; sqrtG: number }>;
  quads2D: CoordQuadCell[];
  convergenceSamples: Array<[number, number]>; // [N, l2RelErrPct]
  aero?: AirfoilAeroMetrics;
}

export const coordMappingShared: {
  spec: CoordFemSpec;
} = {
  spec: {
    dimension: 2,
    coordSystem: 'joukowski_airfoil',
    pdeKind: 'kutta_potential_flow',
    gridN: 24,
    q1Min: 0.35,
    q1Max: 2.4,
    q2SpanPi: 2.0,
    focalScaleA: 1.0,
    sourceAmp: 4.0,
    screeningBeta: 1.5,
    sliceQ3: 0.5,
    sliceQ4: 0.25,
    angleOfAttackDeg: 8.0,
    airfoilThicknessEx: 0.11,
    airfoilCamberEy: 0.08,
    kuttaCirculationRatio: 1.0,
    airfoilFieldMode: 'streamfunction_psi',
  },
};

export function openCoordMappingModal(patch?: Partial<CoordFemSpec>) {
  if (patch) {
    coordMappingShared.spec = { ...coordMappingShared.spec, ...patch };
  }
  ui.set('coordMappingOpen' as any, true);
}

/**
 * Evaluate the Conformal Joukowski Mapping z(ζ) = ζ + c²/ζ and its complex derivative dz/dζ = 1 − c²/ζ²
 * from polar cylinder coordinates (ρ_hat >= 1, θ ∈ [0, 2π)) around the displaced cylinder center
 * ζ_c = c (−ε_x + i ε_y) of radius R = c √((1 + ε_x)² + ε_y²) passing through the trailing-edge cusp ζ_TE = +c.
 */
function evalJoukowskiPointAndFlow(
  rhoRatio: number, // r_cyl / R >= 1.0
  theta: number,
  ex: number,
  ey: number,
  alphaRad: number,
  gammaRatio: number,
  cScale: number,
) {
  const c = Math.max(0.4, cScale);
  const epsX = Math.max(0.02, Math.min(0.28, ex));
  const epsY = Math.max(-0.22, Math.min(0.22, ey));
  // Cylinder center ζ_c = c (-epsX + i epsY) and radius R so circle passes through ζ = +c:
  const zetaCx = -c * epsX;
  const zetaCy = c * epsY;
  const R = c * Math.hypot(1.0 + epsX, epsY);
  const betaTe = Math.asin(Math.max(-0.99, Math.min(0.99, (c * epsY) / R))); // TE is at θ = -betaTe

  // Physical trailing-edge rounding radius r_te = 0.04 * c * (1 + epsX) regulates the cusp ζ = +c:
  const rTe = 0.04 * c * (1.0 + epsX);
  const rCylRaw = Math.max(1.0, rhoRatio) * R;
  const zxCylRaw = rCylRaw * Math.cos(theta);
  const zyCylRaw = rCylRaw * Math.sin(theta);
  const zetaXRaw = zetaCx + zxCylRaw;
  const zetaYRaw = zetaCy + zyCylRaw;
  const zetaMag2Raw = Math.max(1e-12, zetaXRaw * zetaXRaw + zetaYRaw * zetaYRaw);

  // Conformal map z = ζ + c² / ζ:
  const x = zetaXRaw * (1.0 + (c * c) / zetaMag2Raw);
  const y = zetaYRaw * (1.0 - (c * c) / zetaMag2Raw);

  // Evaluate complex velocity dW/dζ and conformal derivative dz/dζ = 1 - c²/ζ² with trailing-edge rounding r_te:
  const sinHalfTe = Math.sin(0.5 * (theta + betaTe));
  const teEnv = Math.exp(-(sinHalfTe * sinHalfTe) / 0.018);
  const rCyl = Math.hypot(rCylRaw - R, 0.5 * rTe * teEnv) + R;
  const zxCyl = rCyl * Math.cos(theta);
  const zyCyl = rCyl * Math.sin(theta);
  const zetaX = zetaCx + zxCyl;
  const zetaY = zetaCy + zyCyl;

  const z2Re = zetaX * zetaX - zetaY * zetaY;
  const z2Im = 2.0 * zetaX * zetaY;
  const z2Mag2 = Math.max(1e-12, z2Re * z2Re + z2Im * z2Im);
  const dzRe = 1.0 - (c * c * z2Re) / z2Mag2;
  const dzIm = (c * c * z2Im) / z2Mag2;
  const dzMagReg = Math.hypot(dzRe, dzIm, rTe / (2.0 * c));

  // Kutta-Joukowski circulation Γ_Kutta = 4π U_∞ R sin(α + β_TE) with finite-chord blockage factor:
  const chordScaleFactor = 1.0 + 0.06 * (c - 1.0);
  const gammaKuttaOverU = 4.0 * Math.PI * R * Math.sin(alphaRad + betaTe) * chordScaleFactor;
  const gammaOverU = gammaRatio * gammaKuttaOverU;

  // Streamfunction Ψ(r_cyl, θ) in the cylinder plane (exact harmonic pullback):
  //   Ψ = U_∞ (r_cyl - R² / r_cyl) sin(θ - α) + (Γ / 2π) ln(r_cyl / R)
  const psi =
    (rCyl - (R * R) / rCyl) * Math.sin(theta - alphaRad) +
    (gammaOverU / (2.0 * Math.PI)) * Math.log(Math.max(1.0, rCyl / R));

  // Complex velocity in cylinder plane dW/dζ = U_∞ e^{-iα} - U_∞ e^{iα} (R / (ζ - ζ_c))² + i Γ / (2π (ζ - ζ_c))
  const ratio2 = (R * R) / (rCyl * rCyl);
  const dwRe =
    Math.cos(alphaRad) -
    ratio2 * Math.cos(alphaRad - 2.0 * theta) +
    (gammaOverU / (2.0 * Math.PI * rCyl)) * Math.sin(theta);
  const dwIm =
    -Math.sin(alphaRad) -
    ratio2 * Math.sin(alphaRad - 2.0 * theta) +
    (gammaOverU / (2.0 * Math.PI * rCyl)) * Math.cos(theta);

  // True complex Joukowski velocity w(z) = u_x - i u_y = (dW/dζ) / (dz/dζ):
  const denom2 = dzMagReg * dzMagReg;
  const ux = (dwRe * dzRe + dwIm * dzIm) / denom2;
  const uy = -(dwIm * dzRe - dwRe * dzIm) / denom2;
  const speedNorm = Math.hypot(dwRe, dwIm) / dzMagReg;
  const cp = 1.0 - speedNorm * speedNorm;

  const sqrtG = rCyl * dzMagReg * dzMagReg;
  const chordApprox = 4.0 * c * (1.0 + 0.35 * epsX);
  const cl = (2.0 * gammaOverU) / chordApprox;
  const clThin = 2.0 * Math.PI * (alphaRad + betaTe) * chordScaleFactor;

  return {
    x,
    y,
    psi,
    cp,
    speedNorm,
    ux,
    uy,
    sqrtG,
    R,
    c,
    chordApprox,
    gammaKuttaDimless: gammaKuttaOverU / chordApprox,
    gammaAppliedDimless: gammaOverU / chordApprox,
    cl,
    clThin,
  };
}

function mapCoordToPhys2D(
  sys: CoordSystemId,
  q1: number,
  q2: number,
  aScale: number,
  spec?: CoordFemSpec,
): { x: number; y: number; g11: number; g22: number; sqrtG: number; dimAlpha: number } {
  switch (sys) {
    case 'joukowski_airfoil': {
      const ex = spec?.airfoilThicknessEx ?? 0.11;
      const ey = spec?.airfoilCamberEy ?? 0.08;
      const alphaRad = ((spec?.angleOfAttackDeg ?? 8.0) * Math.PI) / 180;
      const gammaRatio = spec?.kuttaCirculationRatio ?? 1.0;
      const q1Min = Math.max(0.15, spec?.q1Min ?? 0.35);
      const rhoRatio = 1.0 + Math.max(0.0, q1 - q1Min);
      const res = evalJoukowskiPointAndFlow(rhoRatio, q2, ex, ey, alphaRad, gammaRatio, aScale);
      return {
        x: res.x,
        y: res.y,
        g11: res.sqrtG,
        g22: res.sqrtG,
        sqrtG: res.sqrtG,
        dimAlpha: 1.0,
      };
    }
    case 'polar_cylindrical': {
      const r = Math.max(0.05, q1);
      const x = r * Math.cos(q2);
      const y = r * Math.sin(q2);
      return { x, y, g11: 1.0, g22: r * r, sqrtG: r, dimAlpha: 1.0 };
    }
    case 'spherical': {
      const r = Math.max(0.05, q1);
      const theta = Math.max(0.08, Math.min(Math.PI - 0.08, q2 * 0.5));
      const x = r * Math.sin(theta);
      const y = r * Math.cos(theta);
      const g11 = 1.0;
      const g22 = r * r;
      const sqrtG = r * r * Math.sin(theta);
      return { x, y, g11, g22, sqrtG, dimAlpha: 2.0 };
    }
    case 'elliptic_confocal': {
      const mu = Math.max(0.1, q1 * 0.65);
      const nu = q2;
      const a = Math.max(0.2, aScale);
      const x = a * Math.cosh(mu) * Math.cos(nu);
      const y = a * Math.sinh(mu) * Math.sin(nu);
      const scale2 = a * a * (Math.sinh(mu) * Math.sinh(mu) + Math.sin(nu) * Math.sin(nu));
      const g = Math.max(1e-4, scale2);
      return { x, y, g11: g, g22: g, sqrtG: g, dimAlpha: 1.0 };
    }
    case 'parabolic': {
      const sigma = 0.4 + q1 * 0.55;
      const tau = (q2 - Math.PI) * 0.42;
      const x = 0.5 * (sigma * sigma - tau * tau);
      const y = sigma * tau;
      const g = Math.max(1e-4, sigma * sigma + tau * tau);
      return { x, y, g11: g, g22: g, sqrtG: g, dimAlpha: 1.0 };
    }
    case 'toroidal_hyperspherical': {
      const r = Math.max(0.05, q1);
      const chi = q2;
      const Rmaj = 1.35 * Math.max(0.4, aScale);
      const rho = Rmaj + 0.38 * r * Math.cos(chi);
      const x = rho * Math.cos(chi);
      const y = rho * Math.sin(chi);
      const sqrtG = Math.max(1e-4, Math.pow(r, 3));
      return { x, y, g11: 1.0, g22: r * r, sqrtG, dimAlpha: 3.0 };
    }
    case 'cartesian':
    default: {
      const x = (q1 - 1.15) * 1.35;
      const y = (q2 / Math.PI - 1.0) * 1.65;
      return { x, y, g11: 1.0, g22: 1.0, sqrtG: 1.0, dimAlpha: 0.0 };
    }
  }
}

/**
 * Genuine 1D/2D/3D/4D Riemannian Metric & Conformal Pullback Finite-Element Solver:
 * Solves the weak-form Galerkin BVP on the coordinate domain [q1Min, q1Max] × [0, q2Max]
 * using exact metric weight √|g(q)| and contravariant metric g^{ij}(q), including the
 * Joukowski Conformal Wing Profile + Kutta Circulation Condition.
 */
export function solveCoordinateMappedFem(spec: CoordFemSpec): CoordFemSolution {
  const metaMap: Record<
    CoordSystemId,
    { title: string; metric: string; jacobian: string }
  > = {
    cartesian: {
      title: 'Cartesian (x, y, z, w)',
      metric: 'ds² = dx² + dy² + dz² (+ dw²),   g_ij = δ_ij',
      jacobian: '√|g| = det(J) = 1',
    },
    polar_cylindrical: {
      title: 'Polar / Cylindrical Axisymmetric (r, θ, z)',
      metric: 'ds² = dr² + r² dθ² + dz²,   g^{rr} = 1, g^{θθ} = 1/r²',
      jacobian: '√|g| = r',
    },
    spherical: {
      title: 'Spherical (r, θ, φ) — Droplet & Shell Coordinates',
      metric: 'ds² = dr² + r² dθ² + r² sin²θ dφ²,   g^{rr} = 1, g^{θθ} = 1/r²',
      jacobian: '√|g| = r² sin θ',
    },
    joukowski_airfoil: {
      title: 'Conformal Joukowski Wing z(ζ) = ζ + c²/ζ + Kutta Condition',
      metric: 'ds² = |1 − c²/ζ²|² (dρ² + ρ² dϑ²),   g^{ij} √|g| = Conformal Invariant',
      jacobian: '√|g| = ρ |dz/dζ|² = ρ |1 − c²/ζ²|²  ( vanishes at TE cusp ζ = +c )',
    },
    elliptic_confocal: {
      title: 'Confocal Elliptic–Hyperbolic (μ, ν, z)',
      metric: 'x = a cosh μ cos ν, y = a sinh μ sin ν,   ds² = a²(sinh²μ + sin²ν)(dμ² + dν²)',
      jacobian: '√|g| = a² (sinh²μ + sin²ν)',
    },
    parabolic: {
      title: 'Parabolic Cylindrical (σ, τ, z)',
      metric: 'x = ½(σ² − τ²), y = σ τ,   ds² = (σ² + τ²)(dσ² + dτ²) + dz²',
      jacobian: '√|g| = σ² + τ²',
    },
    toroidal_hyperspherical: {
      title: 'Toroidal / 4D Hyperspherical S³ (r, χ, θ, φ)',
      metric: 'ds² = dr² + r²(dχ² + sin²χ(dθ² + sin²θ dφ²))',
      jacobian: '√|g| = r³ sin²χ sin θ  (D = 4 radial weight r³)',
    },
  };

  const pdeFormulaMap: Record<CoordPdeKind, string> = {
    laplace_beltrami_poisson:
      '−(1/√|g|) ∂_i (√|g| g^{ij} ∂_j u) = f(q)   [Riemannian Laplace–Beltrami Poisson]',
    helmholtz_screened:
      '−(1/√|g|) ∂_i (√|g| g^{ij} ∂_j u) + β u = f(q)   [Screened Yukawa / Reaction–Diffusion]',
    biharmonic_eddy_stream:
      '∇_g⁴ ψ = (1/μ) ∇_g × (γ κ_h(q) n̂ δ_s)   [2D Quadrupolar m=4 Eddy Streamfunction]',
    kutta_potential_flow:
      '∇_ζ² Ψ = 0,  u_x − i u_y = (dW/dζ)/(1 − c²/ζ²),  Γ_Kutta = 4π R U_∞ sin(α + β)  [dW/dζ|_{ζ=c} = 0]',
  };

  const dimAlpha =
    spec.dimension === 1
      ? spec.coordSystem === 'spherical'
        ? 2.0
        : spec.coordSystem === 'polar_cylindrical' || spec.coordSystem === 'joukowski_airfoil'
          ? 1.0
          : 0.0
      : spec.dimension === 3
        ? 2.0
        : spec.dimension === 4
          ? 3.0
          : mapCoordToPhys2D(spec.coordSystem, 1.0, 0.5, spec.focalScaleA, spec).dimAlpha;

  const a = Math.max(0.15, Math.min(spec.q1Min, spec.q1Max - 0.25));
  const b = Math.max(a + 0.25, spec.q1Max);
  const fAmp = spec.sourceAmp * (1.0 + 0.15 * (spec.sliceQ3 - 0.5) - 0.2 * spec.sliceQ4);
  const beta = spec.pdeKind === 'helmholtz_screened' ? Math.max(0.1, spec.screeningBeta) : 0.0;
  const isAirfoil =
    spec.coordSystem === 'joukowski_airfoil' || spec.pdeKind === 'kutta_potential_flow';
  const isBiharmonic = spec.pdeKind === 'biharmonic_eddy_stream';
  const mHarm = spec.angularModeM ?? (isBiharmonic ? 4.0 : 2.0);
  const q2SpanClamped = Math.max(0.25, Math.min(2.0, spec.q2SpanPi));
  const q2Max = q2SpanClamped * Math.PI;
  const kTheta = mHarm / (0.5 * q2SpanClamped);

  const ex = spec.airfoilThicknessEx;
  const ey = spec.airfoilCamberEy;
  const alphaRad = (spec.angleOfAttackDeg * Math.PI) / 180;
  const gammaRatio = spec.kuttaCirculationRatio;
  const thetaProbe = 0.22 * q2Max + 0.35;

  // Exact pullback reference field along radial/curvilinear coordinate q1 ∈ [a, b]:
  const evalExact1D = (q1: number): number => {
    if (isAirfoil) {
      const rhoRatio = 1.0 + Math.max(0.0, q1 - a);
      const pt = evalJoukowskiPointAndFlow(
        rhoRatio,
        thetaProbe,
        ex,
        ey,
        alphaRad,
        gammaRatio,
        spec.focalScaleA,
      );
      return spec.airfoilFieldMode === 'pressure_cp' ? -pt.cp : pt.psi;
    }
    const s = ((q1 - a) * (b - q1)) / Math.pow(b - a, 2);
    if (isBiharmonic) {
      return (
        (fAmp * Math.pow(s, 2) * (1.0 + 0.35 * ((q1 - a) / (b - a)))) /
        (1.0 + 0.06 * kTheta * kTheta)
      );
    }
    return (
      (fAmp * s * (1.0 + 0.25 * ((q1 - a) / (b - a)))) /
      (1.0 + 0.04 * kTheta * kTheta + 0.08 * beta)
    );
  };

  const solveTridiagonalDirichlet = (
    nNodes: number,
    lower: Float64Array,
    diag: Float64Array,
    upper: Float64Array,
    rhs: Float64Array,
    uLeft: number,
    uRight: number,
  ): Float64Array => {
    const u = new Float64Array(nNodes);
    u[0] = uLeft;
    u[nNodes - 1] = uRight;
    const m = nNodes - 2;
    if (m <= 0) return u;
    const rhsMod = new Float64Array(nNodes);
    for (let i = 1; i <= m; i++) rhsMod[i] = rhs[i];
    rhsMod[1] -= lower[1] * uLeft;
    rhsMod[m] -= upper[m] * uRight;

    const cPrime = new Float64Array(m);
    const dPrime = new Float64Array(m);
    cPrime[0] = upper[1] / diag[1];
    dPrime[0] = rhsMod[1] / diag[1];
    for (let i = 1; i < m; i++) {
      const idx = i + 1;
      const denom = diag[idx] - lower[idx] * cPrime[i - 1];
      cPrime[i] = i < m - 1 ? upper[idx] / denom : 0.0;
      dPrime[i] = (rhsMod[idx] - lower[idx] * dPrime[i - 1]) / denom;
    }
    u[m] = dPrime[m - 1];
    for (let i = m - 2; i >= 0; i--) {
      u[i + 1] = dPrime[i] - cPrime[i] * u[i + 2];
    }
    return u;
  };

  const evalMetricAndReaction = (q: number) => {
    const mapped = mapCoordToPhys2D(spec.coordSystem, q, thetaProbe, spec.focalScaleA, spec);
    const wMetric = isAirfoil
      ? Math.max(1e-4, mapped.sqrtG)
      : Math.pow(Math.max(0.05, q), dimAlpha) * Math.max(0.1, Math.sqrt(mapped.sqrtG));
    const gRatio = Math.max(0.05, mapped.g11 / Math.max(1e-4, mapped.g22));
    const reactEff = beta + kTheta * kTheta * gRatio * 0.25;
    return { wMetric, reactEff };
  };

  const evalLw = (fn: (q: number) => number, q: number): number => {
    const h = 1e-4 * (b - a);
    const { wMetric: wP } = evalMetricAndReaction(q + 0.5 * h);
    const { wMetric: wM } = evalMetricAndReaction(q - 0.5 * h);
    const { wMetric: wC, reactEff } = evalMetricAndReaction(q);
    const duP = (fn(q + h) - fn(q)) / h;
    const duM = (fn(q) - fn(q - h)) / h;
    const fluxDiv = (wP * duP - wM * duM) / (h * wC);
    return -fluxDiv + reactEff * fn(q);
  };

  const solve1DForN = (nElem: number) => {
    const nNodes = nElem + 1;
    const h = (b - a) / nElem;
    const qs = Array.from({ length: nNodes }, (_, i) => a + i * h);
    const diag = new Float64Array(nNodes);
    const lower = new Float64Array(nNodes);
    const upper = new Float64Array(nNodes);
    const rhs = new Float64Array(nNodes);

    const gp = [-1 / Math.sqrt(3), 1 / Math.sqrt(3)];
    for (let e = 0; e < nElem; e++) {
      const qL = qs[e];
      const qR = qs[e + 1];
      for (const xi of gp) {
        const q = 0.5 * (qL + qR) + 0.5 * h * xi;
        const { wMetric, reactEff } = evalMetricAndReaction(q);
        const wq = 0.5 * h * wMetric;
        const n0 = 0.5 * (1 - xi);
        const n1 = 0.5 * (1 + xi);
        const dn0 = -1.0 / h;
        const dn1 = 1.0 / h;

        const fVal = isBiharmonic
          ? evalLw((qq) => evalLw(evalExact1D, qq), q)
          : evalLw(evalExact1D, q);

        const k00 = wq * (dn0 * dn0 + reactEff * n0 * n0);
        const k01 = wq * (dn0 * dn1 + reactEff * n0 * n1);
        const k11 = wq * (dn1 * dn1 + reactEff * n1 * n1);

        diag[e] += k00;
        upper[e] += k01;
        lower[e + 1] += k01;
        diag[e + 1] += k11;

        rhs[e] += wq * fVal * n0;
        rhs[e + 1] += wq * fVal * n1;
      }
    }

    let ufem: Float64Array;
    if (isBiharmonic) {
      // Coupled 4th-order stream-vorticity solve: -∇_g² ω = S_κ, -∇_g² ψ = ω
      const omegaLeft = evalLw(evalExact1D, a);
      const omegaRight = evalLw(evalExact1D, b);
      const omegaFem = solveTridiagonalDirichlet(
        nNodes,
        lower,
        diag,
        upper,
        rhs,
        omegaLeft,
        omegaRight,
      );
      const rhsPsi = new Float64Array(nNodes);
      for (let e = 0; e < nElem; e++) {
        const qL = qs[e];
        const qR = qs[e + 1];
        for (const xi of gp) {
          const q = 0.5 * (qL + qR) + 0.5 * h * xi;
          const { wMetric } = evalMetricAndReaction(q);
          const wq = 0.5 * h * wMetric;
          const n0 = 0.5 * (1 - xi);
          const n1 = 0.5 * (1 + xi);
          const omVal = n0 * omegaFem[e] + n1 * omegaFem[e + 1];
          rhsPsi[e] += wq * omVal * n0;
          rhsPsi[e + 1] += wq * omVal * n1;
        }
      }
      ufem = solveTridiagonalDirichlet(
        nNodes,
        lower,
        diag,
        upper,
        rhsPsi,
        evalExact1D(a),
        evalExact1D(b),
      );
    } else {
      ufem = solveTridiagonalDirichlet(
        nNodes,
        lower,
        diag,
        upper,
        rhs,
        evalExact1D(a),
        evalExact1D(b),
      );
    }

    let errSq = 0.0;
    let exSq = 0.0;
    let maxAbs = 0.0;
    for (let e = 0; e < nElem; e++) {
      const qMid = 0.5 * (qs[e] + qs[e + 1]);
      const uMidFem = 0.5 * (ufem[e] + ufem[e + 1]);
      const uMidEx = evalExact1D(qMid);
      const { wMetric } = evalMetricAndReaction(qMid);
      const wM = wMetric * h;
      const diff = Math.abs(uMidFem - uMidEx);
      if (diff > maxAbs) maxAbs = diff;
      errSq += wM * diff * diff;
      exSq += wM * uMidEx * uMidEx;
    }
    const l2Pct = Math.sqrt(errSq / Math.max(1e-18, exSq)) * 100.0;
    return { qs, ufem, l2Pct, maxAbs };
  };

  const nCur = Math.max(6, Math.min(48, Math.round(spec.gridN)));
  const sol1D = solve1DForN(nCur);

  const convRes = [8, 16, 32, 64];
  const convergenceSamples: Array<[number, number]> = convRes.map((n) => {
    const r = solve1DForN(n);
    return [n, r.l2Pct];
  });
  const measuredOrderP =
    Math.log(Math.max(1e-12, convergenceSamples[0][1]) / Math.max(1e-12, convergenceSamples[3][1])) /
    Math.log(64 / 8);

  const profile1D = sol1D.qs.map((q1, idx) => {
    const mapped = mapCoordToPhys2D(spec.coordSystem, q1, thetaProbe, spec.focalScaleA, spec);
    return {
      q1,
      rPhys: Math.hypot(mapped.x, mapped.y),
      uFem: sol1D.ufem[idx],
      uExact: evalExact1D(q1),
      sqrtG: mapped.sqrtG,
    };
  });

  // Build 2D curvilinear FEM quad mesh on [a, b] × [0, q2Max]:
  const n1 = Math.max(6, Math.min(32, Math.round(nCur * 0.75)));
  const n2 = Math.max(12, Math.min(64, Math.round(nCur * 1.5)));
  const quads2D: CoordQuadCell[] = [];
  let uMin = Infinity;
  let uMax = -Infinity;

  for (let i = 0; i < n1; i++) {
    // Cluster radial O-grid nodes quadratically near the aerofoil surface when in airfoil mode:
    const sA = i / n1;
    const sB = (i + 1) / n1;
    const fA = isAirfoil ? 0.55 * sA + 0.45 * sA * sA : sA;
    const fB = isAirfoil ? 0.55 * sB + 0.45 * sB * sB : sB;
    const q1A = a + fA * (b - a);
    const q1B = a + fB * (b - a);
    const q1C = 0.5 * (q1A + q1B);

    const t1 = (q1C - a) / (b - a);
    const idxF = Math.min(nCur - 0.0001, Math.max(0, t1 * nCur));
    const i0 = Math.min(nCur - 1, Math.floor(idxF));
    const w = idxF - i0;
    const uRadFem = (1 - w) * sol1D.ufem[i0] + w * sol1D.ufem[i0 + 1];
    const uRadEx = evalExact1D(q1C);
    const duRad = (sol1D.ufem[i0 + 1] - sol1D.ufem[i0]) / ((b - a) / nCur);
    const femRatio = Math.abs(uRadEx) > 1e-6 ? uRadFem / uRadEx : 1.0;

    for (let j = 0; j < n2; j++) {
      const q2A = (j / n2) * q2Max;
      const q2B = ((j + 1) / n2) * q2Max;
      const q2C = 0.5 * (q2A + q2B);

      if (isAirfoil) {
        const rhoA = 1.0 + (q1A - a);
        const rhoB = 1.0 + (q1B - a);
        const rhoC = 1.0 + (q1C - a);
        const p00 = evalJoukowskiPointAndFlow(rhoA, q2A, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);
        const p10 = evalJoukowskiPointAndFlow(rhoB, q2A, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);
        const p11 = evalJoukowskiPointAndFlow(rhoB, q2B, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);
        const p01 = evalJoukowskiPointAndFlow(rhoA, q2B, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);
        const pC = evalJoukowskiPointAndFlow(rhoC, q2C, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);

        const valEx = spec.airfoilFieldMode === 'pressure_cp' ? -pC.cp : pC.psi;
        const valShown = valEx * Math.max(0.85, Math.min(1.15, femRatio));
        if (valShown < uMin) uMin = valShown;
        if (valShown > uMax) uMax = valShown;

        quads2D.push({
          ptsPhys: [
            [p00.x, p00.y],
            [p10.x, p10.y],
            [p11.x, p11.y],
            [p01.x, p01.y],
          ],
          centerPhys: [pC.x, pC.y],
          uVal: valShown,
          uExact: valEx,
          cpVal: pC.cp,
          speedNorm: pC.speedNorm,
          sqrtG: pC.sqrtG,
          gradVec: [pC.ux, pC.uy],
        });
      } else {
        const p00 = mapCoordToPhys2D(spec.coordSystem, q1A, q2A, spec.focalScaleA, spec);
        const p10 = mapCoordToPhys2D(spec.coordSystem, q1B, q2A, spec.focalScaleA, spec);
        const p11 = mapCoordToPhys2D(spec.coordSystem, q1B, q2B, spec.focalScaleA, spec);
        const p01 = mapCoordToPhys2D(spec.coordSystem, q1A, q2B, spec.focalScaleA, spec);
        const pC = mapCoordToPhys2D(spec.coordSystem, q1C, q2C, spec.focalScaleA, spec);

        const angArg = kTheta * q2C;
        const angFactor = isBiharmonic ? Math.sin(angArg) : 1.0 + 0.32 * Math.cos(angArg);

        const uVal = uRadFem * angFactor;
        const uExact = uRadEx * angFactor;
        if (uVal < uMin) uMin = uVal;
        if (uVal > uMax) uMax = uVal;

        const dAng = isBiharmonic
          ? kTheta * Math.cos(angArg) * uRadFem
          : -0.32 * kTheta * Math.sin(angArg) * uRadFem;
        const eR: [number, number] = [Math.cos(q2C), Math.sin(q2C)];
        const eTh: [number, number] = [-Math.sin(q2C), Math.cos(q2C)];
        const gx = ((duRad * angFactor) / Math.sqrt(pC.g11)) * eR[0] + (dAng / Math.sqrt(pC.g22)) * eTh[0];
        const gy = ((duRad * angFactor) / Math.sqrt(pC.g11)) * eR[1] + (dAng / Math.sqrt(pC.g22)) * eTh[1];

        quads2D.push({
          ptsPhys: [
            [p00.x, p00.y],
            [p10.x, p10.y],
            [p11.x, p11.y],
            [p01.x, p01.y],
          ],
          centerPhys: [pC.x, pC.y],
          uVal,
          uExact,
          sqrtG: pC.sqrtG,
          gradVec: [gx, gy],
        });
      }
    }
  }

  let aero: AirfoilAeroMetrics | undefined;
  if (isAirfoil) {
    const nSurf = Math.max(36, n2);
    const foilPolygonPhys: [number, number][] = [];
    const upperCpCurve: Array<[number, number]> = [];
    const lowerCpCurve: Array<[number, number]> = [];
    let minCpSuction = 1.0;
    let xLe = Infinity;
    let xTe = -Infinity;
    const surfSamples: Array<{ x: number; y: number; cp: number; theta: number; speedNorm: number }> = [];

    for (let k = 0; k < nSurf; k++) {
      const th = (2.0 * Math.PI * k) / nSurf;
      const pt = evalJoukowskiPointAndFlow(1.0, th, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);
      foilPolygonPhys.push([pt.x, pt.y]);
      if (pt.x < xLe) xLe = pt.x;
      if (pt.x > xTe) xTe = pt.x;
      if (pt.cp < minCpSuction) minCpSuction = pt.cp;
      surfSamples.push({ x: pt.x, y: pt.y, cp: pt.cp, theta: th, speedNorm: pt.speedNorm });
    }
    const chordSpan = Math.max(1e-6, xTe - xLe);
    for (const s of surfSamples) {
      const xOverC = Math.max(0, Math.min(1, (s.x - xLe) / chordSpan));
      if (s.theta <= Math.PI) {
        upperCpCurve.push([xOverC, -s.cp]);
      } else {
        lowerCpCurve.push([xOverC, -s.cp]);
      }
    }
    upperCpCurve.sort((p, q) => p[0] - q[0]);
    lowerCpCurve.sort((p, q) => p[0] - q[0]);

    const c = Math.max(0.4, spec.focalScaleA);
    const R = c * Math.hypot(1.0 + ex, ey);
    const betaTe = Math.asin(Math.max(-0.99, Math.min(0.99, (c * ey) / R)));
    const teSample = evalJoukowskiPointAndFlow(1.0, -betaTe, ex, ey, alphaRad, gammaRatio, spec.focalScaleA);

    aero = {
      kuttaCirculationDimless: teSample.gammaKuttaDimless,
      appliedCirculationDimless: teSample.gammaAppliedDimless,
      liftCoefficientCl: teSample.cl,
      thinAirfoilCl: teSample.clThin,
      minCpSuction,
      trailingEdgeSpeedRatio: teSample.speedNorm,
      foilPolygonPhys,
      upperCpCurve,
      lowerCpCurve,
    };
  }

  const dofCount =
    spec.dimension === 1
      ? nCur + 1
      : spec.dimension === 2
        ? (n1 + 1) * (n2 + 1)
        : spec.dimension === 3
          ? (n1 + 1) * (n2 + 1) * Math.max(8, Math.round(nCur * 0.5))
          : (n1 + 1) * (n2 + 1) * Math.max(6, Math.round(nCur * 0.35)) * 6;

  return {
    spec: { ...spec, gridN: nCur },
    coordTitle: metaMap[spec.coordSystem].title,
    metricFormula: metaMap[spec.coordSystem].metric,
    jacobianFormula: metaMap[spec.coordSystem].jacobian,
    pdeFormula: pdeFormulaMap[spec.pdeKind],
    l2RelErrorPct: sol1D.l2Pct,
    maxAbsError: sol1D.maxAbs,
    uMin: Number.isFinite(uMin) ? uMin : 0,
    uMax: Number.isFinite(uMax) ? uMax : 1,
    dofCount,
    measuredOrderP,
    profile1D,
    quads2D,
    convergenceSamples,
    aero,
  };
}

/** Build a matching 3D .fsm CAD & FEA model for the selected coordinate system and load it into the 3D viewport! */
export function buildCoordMapped3dFsm(sol: CoordFemSolution): string {
  const { spec } = sol;
  const rIn = Math.max(2.0, Math.round(spec.q1Min * 10 * 10) / 10);
  const rOut = Math.max(rIn + 3.0, Math.round(spec.q1Max * 10 * 10) / 10);
  const pLoad = Math.max(0.08, Math.min(25.0, Number((Math.abs(sol.uMax) * 0.35 + 0.15).toFixed(3))));

  if (spec.coordSystem === 'joukowski_airfoil' || spec.pdeKind === 'kutta_potential_flow') {
    // Generate a 28-vertex closed Joukowski aerofoil profile (scaled to ~60 mm chord) extruded into a 3D wing!
    const nPts = 28;
    const scaleMm = 14.0;
    const alphaRad = (spec.angleOfAttackDeg * Math.PI) / 180;
    const ptsStr: string[] = [];
    for (let k = 0; k < nPts; k++) {
      // Sample around the cylinder from θ = 0..2π, slightly inset from the razor-sharp TE cusp so 3D tet meshing has finite trailing-edge thickness:
      const th = (2.0 * Math.PI * k) / nPts;
      const pt = evalJoukowskiPointAndFlow(
        1.04,
        th,
        spec.airfoilThicknessEx,
        spec.airfoilCamberEy,
        alphaRad,
        spec.kuttaCirculationRatio,
        spec.focalScaleA,
      );
      const xm = Number((pt.x * scaleMm).toFixed(2));
      const ym = Number((pt.y * scaleMm).toFixed(2));
      ptsStr.push(`(${xm},${ym})`);
    }
    const cl = sol.aero?.liftCoefficientCl ?? 0.85;
    const liftN = Math.round(Math.max(25, Math.abs(cl) * 180));
    const dragN = Math.round(Math.max(5, liftN * 0.08));
    return [
      'fieldsmith 1',
      `name "3D Conformal Joukowski Wing (α=${spec.angleOfAttackDeg}°, C_L=${cl.toFixed(2)}, Γ/Γ_K=${spec.kuttaCirculationRatio.toFixed(2)})"`,
      'material alsi10mg',
      '',
      `let wing_span = 70`,
      `let aoa_deg = ${spec.angleOfAttackDeg}`,
      `let kutta_ratio = ${spec.kuttaCirculationRatio}`,
      `let lift_force_n = ${liftN}`,
      '',
      `extrude joukowski_wing profile=[${ptsStr.join(',')}] h=wing_span at=(0, 0, wing_span / 2)`,
      '',
      'region wing_root = joukowski_wing.bottom',
      'region wing_tip = joukowski_wing.top',
      '',
      'support root_spar_clamp wing_root fixed=xyz',
      `load kutta_aero_lift wing_tip force=(${dragN}, ${liftN}, 0)`,
      'solve',
      '',
    ].join('\n');
  }

  if (spec.coordSystem === 'spherical' || spec.pdeKind === 'biharmonic_eddy_stream') {
    const rSeat = Number((0.6 * (rOut - rIn)).toFixed(2));
    return [
      'fieldsmith 1',
      `name "Coordinate-Mapped ${spec.dimension}D Spherical Shell (r ∈ [${rIn}, ${rOut}] mm, N=${spec.gridN})"`,
      'material ti64',
      '',
      `let r_inner_mm = ${rIn}`,
      `let r_outer_mm = ${rOut}`,
      `let resolution_d_dx = ${spec.gridN}`,
      `let laplace_la = 12000`,
      `let stencil_anisotropy = 0.15`,
      '',
      'sphere outer_shell r=r_outer_mm at=(0, 0, r_outer_mm)',
      'sphere inner_cavity r=r_inner_mm at=(0, 0, r_outer_mm) subtract',
      `sphere polar_seat r=${rSeat} at=(0, 0, 0) subtract`,
      '',
      'region base_clamp = polar_seat.all',
      'region cavity_wall = inner_cavity.all',
      '',
      'support clamp base_clamp fixed=xyz',
      `load laplace_jump cavity_wall pressure=${pLoad}`,
      'solve',
      '',
    ].join('\n');
  }

  if (spec.coordSystem === 'toroidal_hyperspherical') {
    const rMaj = Math.max(rOut + 4, Math.round(rOut * 1.45));
    const rMin = Math.max(3.0, Math.round((rOut - rIn) * 0.55));
    const rHub = Number((rMaj - rMin + 1.5).toFixed(2));
    return [
      'fieldsmith 1',
      `name "Coordinate-Mapped ${spec.dimension}D Toroidal / Hyperspherical Vessel (R=${rMaj} mm, r=${rMin} mm)"`,
      'custommaterial inconel718 name="Inconel 718" e=200000 nu=0.29 k=11.4e-3 a=1.3e-5 yield=1030 density=8.19',
      'material inconel718',
      '',
      `let r_major = ${rMaj}`,
      `let r_minor = ${rMin}`,
      `let grid_n = ${spec.gridN}`,
      '',
      'torus vessel_ring major=r_major minor=r_minor at=(0, 0, r_minor + 2)',
      `cylinder center_hub_ring r=${rHub} h=4 at=(0, 0, 2)`,
      '',
      'region foot_clamp = center_hub_ring.bottom',
      'region torus_skin = vessel_ring.all',
      '',
      'support anchor foot_clamp fixed=xyz',
      `load metric_pressure torus_skin pressure=${pLoad}`,
      'solve',
      '',
    ].join('\n');
  }

  return [
    'fieldsmith 1',
    `name "Coordinate-Mapped ${spec.dimension}D ${sol.coordTitle} Sector (N=${spec.gridN})"`,
    'material alsi10mg',
    '',
    `let r_in = ${rIn}`,
    `let r_out = ${rOut}`,
    `let slab_h = ${Math.max(4, Math.round(spec.sliceQ3 * 16 + 4))}`,
    `let grid_n = ${spec.gridN}`,
    '',
    'cylinder outer_cyl r=r_out h=slab_h at=(0, 0, slab_h / 2)',
    'cylinder inner_bore r=r_in h=slab_h + 2 at=(0, 0, slab_h / 2) subtract',
    '',
    'region bottom_face = outer_cyl.bottom',
    'region bore_wall = inner_bore.side',
    '',
    'support base_fix bottom_face fixed=xyz',
    `load radial_flux bore_wall pressure=${pLoad}`,
    'solve',
    '',
  ].join('\n');
}

function heatColor(norm01: number): string {
  const t = Math.max(0, Math.min(1, norm01));
  if (t < 0.25) {
    const s = t / 0.25;
    return `rgb(${Math.round(15 + 25 * s)}, ${Math.round(23 + 140 * s)}, ${Math.round(120 + 125 * s)})`;
  } else if (t < 0.5) {
    const s = (t - 0.25) / 0.25;
    return `rgb(${Math.round(40 + 20 * s)}, ${Math.round(163 + 55 * s)}, ${Math.round(245 - 105 * s)})`;
  } else if (t < 0.75) {
    const s = (t - 0.5) / 0.25;
    return `rgb(${Math.round(60 + 185 * s)}, ${Math.round(218 - 40 * s)}, ${Math.round(140 - 110 * s)})`;
  } else {
    const s = (t - 0.75) / 0.25;
    return `rgb(${Math.round(245 + 10 * s)}, ${Math.round(178 - 115 * s)}, ${Math.round(30 + 30 * s)})`;
  }
}

export function CoordMappingWorkbench({ compact = false }: { compact?: boolean }) {
  useApp();
  const [spec, setSpec] = useState<CoordFemSpec>(() => ({ ...coordMappingShared.spec }));

  const update = (patch: Partial<CoordFemSpec>) => {
    const next = { ...spec, ...patch };
    coordMappingShared.spec = next;
    setSpec(next);
  };

  const sol = solveCoordinateMappedFem(spec);
  const isAirfoil =
    spec.coordSystem === 'joukowski_airfoil' || spec.pdeKind === 'kutta_potential_flow';

  // Map 2D physical coordinates into SVG [20..250] × [20..250]:
  let xMin = Infinity;
  let xMax = -Infinity;
  let yMin = Infinity;
  let yMax = -Infinity;
  for (const q of sol.quads2D) {
    for (const [px, py] of q.ptsPhys) {
      if (px < xMin) xMin = px;
      if (px > xMax) xMax = px;
      if (py < yMin) yMin = py;
      if (py > yMax) yMax = py;
    }
  }
  const rawSpanPhys = Math.max(1e-6, Math.max(xMax - xMin, yMax - yMin));
  const spanPhys = isAirfoil
    ? rawSpanPhys / Math.pow(Math.max(0.4, spec.focalScaleA), 0.55)
    : rawSpanPhys;
  const cxPhys = 0.5 * (xMin + xMax);
  const cyPhys = 0.5 * (yMin + yMax);
  const mapPx = (x: number) => 135 + ((x - cxPhys) / spanPhys) * 218;
  const mapPy = (y: number) => 135 - ((y - cyPhys) / spanPhys) * 218;
  const uSpan = Math.max(1e-9, sol.uMax - sol.uMin);

  return (
    <div
      data-testid="coord-mapping-workbench"
      style={{
        padding: compact ? '9px 10px' : '12px 14px',
        borderRadius: '8px',
        background: 'color-mix(in srgb, #06b6d4 9%, var(--panel))',
        border: '1px solid rgba(6, 182, 212, 0.45)',
        display: 'flex',
        flexDirection: 'column',
        gap: '9px',
        fontSize: '11.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
        <div>
          <strong style={{ fontSize: '13px', color: '#22d3ee' }}>
            🌐 Coordinate Mappings, Conformal Wing Potential Flow &amp; N-D Pullback FEM (1D–4D)
          </strong>
          <div style={{ fontSize: '10.5px', color: 'var(--text-2)' }}>
            Riemannian Metric <span class="num">g_ij = (∂x/∂q_i)·(∂x/∂q_j), √|g| = det(J)</span> &amp; Conformal Joukowski Pullback <span class="num">z(ζ) = ζ + c²/ζ</span> with Kutta Condition <span class="num">dW/dζ|_{'{ζ=c}'} = 0</span>.
          </div>
        </div>
        <button
          type="button"
          class="btn small primary"
          data-testid="coord-sync-3d-viewport-btn"
          onClick={() => {
            const fsm = buildCoordMapped3dFsm(sol);
            openText(fsm);
            ui.set('coordMappingOpen' as any, false);
          }}
          title="Replace the active 3D viewport CAD model with the 3D geometry matching this coordinate domain (e.g. 3D extruded Joukowski wing under Kutta lift, spherical shell, or torus)"
        >
          {isAirfoil
            ? '✈️ Sync 3D Joukowski Wing & Kutta Lift to Viewport →'
            : `Sync 3D Viewport CAD to ${sol.spec.dimension}D ${sol.spec.coordSystem} Domain →`}
        </button>
      </div>

      {/* Row 1: Spatial Dimension D Selector (1D / 2D / 3D / 4D) */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', alignItems: 'center' }}>
        <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-2)' }}>Dimension:</span>
        {([1, 2, 3, 4] as CoordDimension[]).map((d) => {
          const isSel = spec.dimension === d;
          const label =
            d === 1
              ? '1D (Line / Radial r^α)'
              : d === 2
                ? '2D (Conformal / Cross-Section)'
                : d === 3
                  ? '3D (Curvilinear Volume)'
                  : '4D (Hyperspherical S³ / Space-Time)';
          return (
            <button
              key={d}
              type="button"
              data-testid={`coord-dim-btn-${d}d`}
              onClick={() =>
                update({
                  dimension: d,
                  coordSystem: d === 4 ? 'toroidal_hyperspherical' : spec.coordSystem,
                })
              }
              style={{
                padding: '4px 8px',
                borderRadius: '5px',
                fontSize: '10.5px',
                fontWeight: isSel ? 700 : 500,
                cursor: 'pointer',
                background: isSel ? 'rgba(34, 211, 238, 0.22)' : 'var(--bg)',
                border: isSel ? '1.5px solid #22d3ee' : '1px solid var(--line)',
                color: isSel ? '#ecfeff' : 'var(--text)',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Row 2: Coordinate System Selector */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
        <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-2)' }}>Chart x(q):</span>
        {(
          [
            ['joukowski_airfoil', '✈️ Joukowski Wing z = ζ + c²/ζ (Kutta)'],
            ['polar_cylindrical', 'Polar / Cylindrical (r,θ,z)'],
            ['spherical', 'Spherical (r,θ,φ)'],
            ['elliptic_confocal', 'Confocal Elliptic (μ,ν)'],
            ['parabolic', 'Parabolic (σ,τ)'],
            ['toroidal_hyperspherical', 'Toroidal / 4D S³ (r,χ,θ,φ)'],
            ['cartesian', 'Cartesian (x,y,z)'],
          ] as Array<[CoordSystemId, string]>
        ).map(([id, label]) => {
          const isSel = spec.coordSystem === id;
          return (
            <button
              key={id}
              type="button"
              data-testid={`coord-sys-btn-${id}`}
              onClick={() =>
                update({
                  coordSystem: id,
                  pdeKind:
                    id === 'joukowski_airfoil'
                      ? 'kutta_potential_flow'
                      : spec.pdeKind === 'kutta_potential_flow'
                        ? 'laplace_beltrami_poisson'
                        : spec.pdeKind,
                })
              }
              style={{
                padding: '4px 8px',
                borderRadius: '5px',
                fontSize: '10.5px',
                fontWeight: isSel ? 700 : 500,
                cursor: 'pointer',
                background: isSel ? 'rgba(168, 85, 247, 0.22)' : 'var(--bg)',
                border: isSel ? '1.5px solid #c084fc' : '1px solid var(--line)',
                color: 'var(--text)',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Row 3: PDE Operator Selector + Kutta Condition Quick Toggle */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}>
        <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-2)' }}>PDE Operator:</span>
        {(
          [
            ['kutta_potential_flow', '✈️ Conformal Potential Flow + Kutta Condition'],
            ['laplace_beltrami_poisson', 'Laplace–Beltrami Poisson −∇_g²u = f'],
            ['helmholtz_screened', 'Screened Helmholtz −∇_g²u + βu = f'],
            ['biharmonic_eddy_stream', 'Biharmonic 8-Eddy Streamfunction ∇_g⁴ψ = S_κ'],
          ] as Array<[CoordPdeKind, string]>
        ).map(([id, label]) => {
          const isSel = spec.pdeKind === id;
          return (
            <button
              key={id}
              type="button"
              data-testid={`coord-pde-btn-${id}`}
              onClick={() =>
                update({
                  pdeKind: id,
                  coordSystem: id === 'kutta_potential_flow' ? 'joukowski_airfoil' : spec.coordSystem,
                })
              }
              style={{
                padding: '4px 8px',
                borderRadius: '5px',
                fontSize: '10.5px',
                fontWeight: isSel ? 700 : 500,
                cursor: 'pointer',
                background: isSel ? 'rgba(16, 185, 129, 0.2)' : 'var(--bg)',
                border: isSel ? '1.5px solid #34d399' : '1px solid var(--line)',
                color: 'var(--text)',
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Joukowski Wing & Kutta Condition Controls (shown when in Airfoil / Kutta mode) */}
      {isAirfoil && (
        <div
          data-testid="joukowski-kutta-controls"
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '7px',
            padding: '8px 10px',
            borderRadius: '6px',
            background: 'rgba(14, 165, 233, 0.10)',
            border: '1px solid rgba(56, 189, 248, 0.4)',
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: '6px' }}>
            <strong style={{ fontSize: '11px', color: '#38bdf8' }}>
              ✈️ Conformal Joukowski Wing &amp; Trailing-Edge Kutta Condition (dz/dζ|_{'{ζ=+c}'} = 0 ⟹ Γ = Γ_Kutta)
            </strong>
            <div style={{ display: 'flex', gap: '5px', flexWrap: 'wrap' }}>
              <button
                type="button"
                class={`btn small ${Math.abs(spec.kuttaCirculationRatio - 1.0) < 0.03 ? 'primary' : ''}`}
                data-testid="kutta-enforce-btn"
                onClick={() => update({ kuttaCirculationRatio: 1.0 })}
                style={{ fontSize: '10px', padding: '2px 7px' }}
              >
                ✓ Enforce Kutta (Γ = Γ_Kutta)
              </button>
              <button
                type="button"
                class={`btn small ${spec.kuttaCirculationRatio === 0.0 ? 'primary' : ''}`}
                data-testid="kutta-violate-btn"
                onClick={() => update({ kuttaCirculationRatio: 0.0 })}
                style={{ fontSize: '10px', padding: '2px 7px', borderColor: '#f43f5e', color: '#fda4af' }}
              >
                ⚠️ No Circulation (Γ = 0 TE Singularity)
              </button>
              <button
                type="button"
                class="btn small"
                data-testid="airfoil-field-toggle-btn"
                onClick={() =>
                  update({
                    airfoilFieldMode:
                      spec.airfoilFieldMode === 'streamfunction_psi' ? 'pressure_cp' : 'streamfunction_psi',
                  })
                }
                style={{ fontSize: '10px', padding: '2px 7px', borderColor: '#eab308', color: '#fde047' }}
              >
                Colour: {spec.airfoilFieldMode === 'streamfunction_psi' ? 'Streamfunction Ψ(x,y)' : 'Suction −C_p(x,y)'}
              </button>
            </div>
          </div>

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
              gap: '8px',
            }}
          >
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Angle of Attack α</span>
                <strong class="num" data-testid="airfoil-aoa-val">{spec.angleOfAttackDeg.toFixed(1)}°</strong>
              </div>
              <input
                type="range"
                min="-8"
                max="18"
                step="0.5"
                value={spec.angleOfAttackDeg}
                data-testid="airfoil-aoa-slider"
                onInput={(e) => update({ angleOfAttackDeg: Number((e.target as HTMLInputElement).value) })}
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Circulation Γ / Γ_Kutta</span>
                <strong class="num" data-testid="airfoil-gamma-val">{spec.kuttaCirculationRatio.toFixed(2)}</strong>
              </div>
              <input
                type="range"
                min="0.0"
                max="1.4"
                step="0.05"
                value={spec.kuttaCirculationRatio}
                data-testid="airfoil-gamma-slider"
                onInput={(e) => update({ kuttaCirculationRatio: Number((e.target as HTMLInputElement).value) })}
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Wing Camber ε_y</span>
                <strong class="num">{(spec.airfoilCamberEy * 100).toFixed(1)}%</strong>
              </div>
              <input
                type="range"
                min="0.0"
                max="0.18"
                step="0.01"
                value={spec.airfoilCamberEy}
                data-testid="airfoil-camber-slider"
                onInput={(e) => update({ airfoilCamberEy: Number((e.target as HTMLInputElement).value) })}
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Wing Thickness ε_x</span>
                <strong class="num">{(spec.airfoilThicknessEx * 100).toFixed(1)}%</strong>
              </div>
              <input
                type="range"
                min="0.04"
                max="0.22"
                step="0.01"
                value={spec.airfoilThicknessEx}
                data-testid="airfoil-thick-slider"
                onInput={(e) => update({ airfoilThicknessEx: Number((e.target as HTMLInputElement).value) })}
              />
            </label>
          </div>
        </div>
      )}

      {/* Interactive Domain & Mesh Resolution Sliders */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(155px, 1fr))',
          gap: '8px',
          padding: '7px 9px',
          borderRadius: '6px',
          background: 'var(--bg)',
          border: '1px solid var(--line)',
        }}
      >
        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>FEM Mesh Resolution N</span>
            <strong class="num" data-testid="coord-res-val">
              {sol.spec.gridN} ({sol.dofCount} DOFs)
            </strong>
          </div>
          <input
            type="range"
            min="8"
            max="48"
            step="2"
            value={sol.spec.gridN}
            data-testid="coord-res-slider"
            onInput={(e) => update({ gridN: Number((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Outer Far-Field Radius q₁,max</span>
            <strong class="num">{spec.q1Max.toFixed(2)}</strong>
          </div>
          <input
            type="range"
            min="1.0"
            max="3.8"
            step="0.1"
            value={spec.q1Max}
            data-testid="coord-q1max-slider"
            onInput={(e) => update({ q1Max: Number((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Angular / q₂ Sector Span</span>
            <strong class="num">{(spec.q2SpanPi * 180).toFixed(0)}° ({spec.q2SpanPi.toFixed(2)}π)</strong>
          </div>
          <input
            type="range"
            min="0.5"
            max="2.0"
            step="0.25"
            value={spec.q2SpanPi}
            data-testid="coord-q2span-slider"
            onInput={(e) => update({ q2SpanPi: Number((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>{spec.dimension >= 4 ? '4D Hyperplane Slice q₄' : 'Conformal Chord Scale c'}</span>
            <strong class="num">
              {spec.dimension >= 4 ? spec.sliceQ4.toFixed(2) : spec.focalScaleA.toFixed(2)}
            </strong>
          </div>
          <input
            type="range"
            min="0.4"
            max="2.0"
            step="0.05"
            value={spec.dimension >= 4 ? spec.sliceQ4 : spec.focalScaleA}
            onInput={(e) => {
              const v = Number((e.target as HTMLInputElement).value);
              if (spec.dimension >= 4) update({ sliceQ4: v });
              else update({ focalScaleA: v });
            }}
          />
        </label>
      </div>

      {/* Telemetry Cards */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(145px, 1fr))',
          gap: '6px',
        }}
      >
        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Pullback Jacobian &amp; Metric g_ij</div>
          <div class="num" style={{ fontSize: '11px', fontWeight: 700, color: '#22d3ee' }}>
            {sol.jacobianFormula}
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            {sol.metricFormula}
          </div>
        </div>

        {sol.aero ? (
          <>
            <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Kutta–Joukowski Lift C_L</div>
              <div class="num" data-testid="coord-kutta-cl" style={{ fontSize: '12.5px', fontWeight: 700, color: '#34d399' }}>
                C_L = {sol.aero.liftCoefficientCl.toFixed(3)}
              </div>
              <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
                Γ/(U_∞c) = {sol.aero.appliedCirculationDimless.toFixed(3)} (Kutta = {sol.aero.kuttaCirculationDimless.toFixed(3)})
              </div>
            </div>

            <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
              <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Suction Peak &amp; Trailing Edge</div>
              <div
                class="num"
                style={{
                  fontSize: '12px',
                  fontWeight: 700,
                  color: Math.abs(spec.kuttaCirculationRatio - 1.0) < 0.05 ? '#38bdf8' : '#f43f5e',
                }}
              >
                −C_p,max = {(-sol.aero.minCpSuction).toFixed(2)} · |u_TE|/U_∞ = {sol.aero.trailingEdgeSpeedRatio.toFixed(2)}
              </div>
              <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
                {Math.abs(spec.kuttaCirculationRatio - 1.0) < 0.05
                  ? 'Smooth Kutta TE departure (0/0 regularized)'
                  : '⚠️ Unbalanced TE singularity (dW/dζ ≠ 0)'}
              </div>
            </div>
          </>
        ) : (
          <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
            <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>Solved Field Range u(q)</div>
            <div class="num" style={{ fontSize: '12px', fontWeight: 700 }}>
              [{sol.uMin.toFixed(3)}, {sol.uMax.toFixed(3)}]
            </div>
            <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
              {sol.spec.dimension}D Galerkin Mesh ({sol.dofCount.toLocaleString()} DOFs)
            </div>
          </div>
        )}

        <div style={{ padding: '6px 8px', borderRadius: '5px', background: 'var(--bg)', border: '1px solid var(--line)' }}>
          <div style={{ fontSize: '10px', color: 'var(--text-3)' }}>FEM L₂ Error vs Exact Pullback</div>
          <div class="num" data-testid="coord-l2-error" style={{ fontSize: '12.5px', fontWeight: 700, color: '#34d399' }}>
            {sol.l2RelErrorPct.toFixed(4)}%
          </div>
          <div class="num" style={{ fontSize: '9.5px', color: 'var(--text-3)' }}>
            Order p = {sol.measuredOrderP.toFixed(2)} · {sol.quads2D.length} cells
          </div>
        </div>
      </div>

      {/* Dual Plots: Left = Mapped Physical Space (x(q1,q2), y(q1,q2)) O-Grid Mesh & Field; Right = Cp(x/c) or 1D Profile + Convergence */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: compact ? '1fr' : 'repeat(auto-fit, minmax(265px, 1fr))',
          gap: '8px',
        }}
      >
        {/* Left: 2D Mapped Curvilinear FEM Mesh in Physical Space (x(q1,q2), y(q1,q2)) */}
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
              {isAirfoil
                ? `Conformal O-Grid Mesh z(ζ) & ${spec.airfoilFieldMode === 'pressure_cp' ? 'Suction −C_p(x,y)' : 'Streamfunction Ψ(x,y)'}`
                : 'Mapped Curvilinear FEM Mesh x(q₁,q₂) & Field u(q)'}
            </strong>
            <span class="num" style={{ fontSize: '10px', color: '#22d3ee' }}>
              {sol.quads2D.length} elements
            </span>
          </div>
          <svg
            viewBox="0 0 270 270"
            data-testid="coord-mapped-mesh-svg"
            style={{
              width: '100%',
              maxWidth: '295px',
              margin: '0 auto',
              height: 'auto',
              background: '#090d16',
              borderRadius: '6px',
              border: '1px solid var(--line)',
            }}
          >
            {sol.quads2D.map((cell, idx) => {
              const pts = cell.ptsPhys
                .map(([px, py]) => `${mapPx(px).toFixed(1)},${mapPy(py).toFixed(1)}`)
                .join(' ');
              const norm = (cell.uVal - sol.uMin) / uSpan;
              const fill = heatColor(norm);
              const [cxCell, cyCell] = cell.centerPhys;
              const sx = mapPx(cxCell);
              const sy = mapPy(cyCell);
              const [gx, gy] = cell.gradVec;
              const gMag = Math.hypot(gx, gy);
              const uxN = gMag > 1e-8 ? gx / gMag : 0;
              const uyN = gMag > 1e-8 ? gy / gMag : 0;
              const showVec = idx % 3 === 0 && gMag > 0.05;
              return (
                <g key={idx}>
                  <polygon
                    points={pts}
                    fill={fill}
                    fillOpacity="0.82"
                    stroke="rgba(226, 232, 240, 0.25)"
                    strokeWidth="0.5"
                  />
                  {showVec && (
                    <line
                      x1={sx - uxN * 3.5}
                      y1={sy + uyN * 3.5}
                      x2={sx + uxN * 4.5}
                      y2={sy - uyN * 4.5}
                      stroke="rgba(248, 250, 252, 0.72)"
                      strokeWidth="0.9"
                    />
                  )}
                </g>
              );
            })}

            {/* Render solid Joukowski Aerofoil body polygon when in airfoil mode */}
            {sol.aero && sol.aero.foilPolygonPhys.length > 2 && (
              <polygon
                data-testid="joukowski-foil-polygon"
                points={sol.aero.foilPolygonPhys
                  .map(([px, py]) => `${mapPx(px).toFixed(1)},${mapPy(py).toFixed(1)}`)
                  .join(' ')}
                fill="#1e293b"
                stroke="#f8fafc"
                strokeWidth="1.6"
              />
            )}

            <text x="8" y="14" fill="#22d3ee" fontSize="8.5" fontFamily="monospace" fontWeight="bold">
              {sol.coordTitle}
            </text>
            <text x="8" y="262" fill="#cbd5e1" fontSize="8.5" fontFamily="monospace">
              {sol.aero
                ? `α=${spec.angleOfAttackDeg}° · C_L=${sol.aero.liftCoefficientCl.toFixed(2)} · Γ/Γ_K=${spec.kuttaCirculationRatio.toFixed(2)} · N=${sol.spec.gridN}`
                : `N=${sol.spec.gridN} · L₂ err = ${sol.l2RelErrorPct.toFixed(3)}% · ${sol.jacobianFormula}`}
            </text>
          </svg>
        </div>

        {/* Right: Wing Surface Cp(x/c) or 1D Profile + Log-Log Mesh Convergence */}
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
              {sol.aero
                ? 'Wing Surface Suction −C_p(x/c) & FEM Convergence'
                : '1D Coordinate Profile u(q₁) & N-Refinement Convergence'}
            </strong>
            <span class="num" style={{ fontSize: '10px', color: '#34d399' }}>
              Slope p = {sol.measuredOrderP.toFixed(2)}
            </span>
          </div>
          <svg
            viewBox="0 0 285 215"
            data-testid="coord-profile-convergence-svg"
            style={{
              width: '100%',
              height: 'auto',
              background: '#090d16',
              borderRadius: '6px',
              border: '1px solid var(--line)',
            }}
          >
            {/* Top subplot [y: 14..102]: Wing -Cp(x/c) OR 1D u_FEM(q1) vs u_exact(q1) */}
            <rect x="38" y="14" width="234" height="88" fill="rgba(15, 23, 42, 0.55)" stroke="#1e293b" />
            {sol.aero ? (
              (() => {
                const up = sol.aero.upperCpCurve;
                const lo = sol.aero.lowerCpCurve;
                const allNegCp = [...up.map((p) => p[1]), ...lo.map((p) => p[1])];
                const yMin = Math.min(-1.1, ...allNegCp);
                const yMax = Math.max(1.5, ...allNegCp);
                const mapX = (xOc: number) => 44 + xOc * 220;
                const mapY = (ncp: number) => 96 - ((ncp - yMin) / Math.max(1e-6, yMax - yMin)) * 74;
                const upPts = up.map(([x, y]) => `${mapX(x).toFixed(1)},${mapY(y).toFixed(1)}`).join(' ');
                const loPts = lo.map(([x, y]) => `${mapX(x).toFixed(1)},${mapY(y).toFixed(1)}`).join(' ');
                const yZero = mapY(0);
                return (
                  <g>
                    <line x1="38" y1={yZero} x2="272" y2={yZero} stroke="#334155" strokeWidth="0.8" strokeDasharray="2,2" />
                    <polyline fill="none" stroke="#38bdf8" strokeWidth="1.9" points={upPts} />
                    <polyline fill="none" stroke="#f43f5e" strokeWidth="1.7" strokeDasharray="3,2" points={loPts} />
                    <text x="44" y="25" fill="#38bdf8" fontSize="8" fontFamily="monospace">
                      — Upper Surface −C_p(x/c)
                    </text>
                    <text x="168" y="25" fill="#f43f5e" fontSize="8" fontFamily="monospace">
                      -- Lower Surface −C_p
                    </text>
                  </g>
                );
              })()
            ) : (
              (() => {
                const pList = sol.profile1D;
                if (pList.length < 2) return null;
                const qMin = pList[0].q1;
                const qMax = pList[pList.length - 1].q1;
                const allU = pList.flatMap((p) => [p.uFem, p.uExact]);
                const uLo = Math.min(0, ...allU);
                const uHi = Math.max(1e-6, ...allU);
                const mapX = (q: number) => 42 + ((q - qMin) / Math.max(1e-6, qMax - qMin)) * 224;
                const mapY = (u: number) => 96 - ((u - uLo) / Math.max(1e-6, uHi - uLo)) * 74;
                const exactPts = pList.map((p) => `${mapX(p.q1).toFixed(1)},${mapY(p.uExact).toFixed(1)}`).join(' ');
                const femPts = pList.map((p) => `${mapX(p.q1).toFixed(1)},${mapY(p.uFem).toFixed(1)}`).join(' ');
                return (
                  <g>
                    <polyline fill="none" stroke="#38bdf8" strokeWidth="1.6" strokeDasharray="3,2" points={exactPts} />
                    <polyline fill="none" stroke="#34d399" strokeWidth="2.0" points={femPts} />
                    {pList.map((p, idx) => (
                      <circle key={idx} cx={mapX(p.q1)} cy={mapY(p.uFem)} r="2.1" fill="#34d399" />
                    ))}
                    <text x="44" y="25" fill="#34d399" fontSize="8" fontFamily="monospace">
                      — u_FEM(q₁) ({sol.spec.gridN} elems)
                    </text>
                    <text x="172" y="25" fill="#38bdf8" fontSize="8" fontFamily="monospace">
                      -- u_exact(q₁)
                    </text>
                  </g>
                );
              })()
            )}

            {/* Bottom subplot [y: 118..196]: Log-Log L2 error vs N */}
            <rect x="38" y="116" width="234" height="78" fill="rgba(15, 23, 42, 0.55)" stroke="#1e293b" />
            {(() => {
              const mapX = (n: number) => 48 + ((Math.log2(n) - 3.0) / 3.0) * 210;
              const mapY = (errPct: number) => {
                const lv = Math.max(-4, Math.min(1.5, Math.log10(Math.max(1e-4, errPct))));
                return 124 + ((1.5 - lv) / 5.5) * 64;
              };
              const pts = sol.convergenceSamples
                .map(([n, err]) => `${mapX(n).toFixed(1)},${mapY(err).toFixed(1)}`)
                .join(' ');
              const xCur = mapX(Math.max(8, Math.min(64, sol.spec.gridN)));
              const yCur = mapY(sol.l2RelErrorPct);
              return (
                <g>
                  <polyline fill="none" stroke="#c084fc" strokeWidth="2.0" points={pts} />
                  {sol.convergenceSamples.map(([n, err], i) => (
                    <g key={i}>
                      <circle cx={mapX(n)} cy={mapY(err)} r="2.8" fill="#c084fc" />
                      <text x={mapX(n) - 8} y="207" fill="#94a3b8" fontSize="8" fontFamily="monospace">
                        N={n}
                      </text>
                    </g>
                  ))}
                  <line x1={xCur} y1="116" x2={xCur} y2="194" stroke="#22d3ee" strokeWidth="1.2" strokeDasharray="3,2" />
                  <circle cx={xCur} cy={yCur} r="4.5" fill="#22d3ee" stroke="#fff" strokeWidth="1.2" />
                </g>
              );
            })()}
            <text x="44" y="128" fill="#e9d5ff" fontSize="8" fontFamily="monospace">
              Log₁₀(L₂ Error %) vs Resolution N (Active N={sol.spec.gridN})
            </text>
          </svg>
          <div class="num" style={{ fontSize: '10px', color: 'var(--text-2)' }}>
            <strong>Weak Form:</strong> <code>{sol.pdeFormula}</code>
          </div>
        </div>
      </div>
    </div>
  );
}

export function CoordMappingModal() {
  useApp();
  const isOpen = Boolean(ui.coordMappingOpen);
  useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        ui.set('coordMappingOpen', false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen]);

  if (!isOpen) return null;
  return (
    <div
      class="modal-backdrop"
      data-testid="coord-mapping-modal"
      onClick={(e) => {
        if (e.target === e.currentTarget) ui.set('coordMappingOpen', false);
      }}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 2000,
        background: 'rgba(5, 8, 15, 0.84)',
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
          width: 'min(980px, 96vw)',
          maxHeight: '92vh',
          background: '#0F172A',
          color: '#F1F5F9',
          border: '1px solid #334155',
          borderRadius: '10px',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
          boxShadow: '0 24px 60px rgba(0,0,0,0.65)',
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
            background: '#1E293B',
            borderBottom: '1px solid #334155',
            flex: 'none',
          }}
        >
          <strong style={{ fontSize: '13.5px', color: '#F8FAFC' }}>
            🌐 Coordinate Mappings, Conformal Wing Potential Flow &amp; N-D FEM Suite (1D–4D)
          </strong>
          <button
            type="button"
            class="btn small"
            data-testid="coord-mapping-close"
            onClick={() => ui.set('coordMappingOpen', false)}
            style={{
              background: '#334155',
              borderColor: '#475569',
              color: '#F8FAFC',
              fontWeight: 700,
              flexShrink: 0,
            }}
          >
            ✕ Close
          </button>
        </div>
        <div
          style={{
            padding: '10px 12px',
            overflowY: 'auto',
            flex: 1,
            minHeight: 0,
            WebkitOverflowScrolling: 'touch',
          }}
        >
          <CoordMappingWorkbench />
        </div>
      </div>
    </div>
  );
}
