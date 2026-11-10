// Results lens: the analysed surface drawn as the deformed triangle mesh, coloured per facet
// by the chosen quantity, plus a status badge and a control panel. Colour comes from the
// element that owns each boundary facet (element-centroid von Mises), so peaks read honestly.
import { Fragment } from 'preact';
import { useState } from 'preact/hooks';
import { ui, useApp, RESULTS_SCALE_DEFAULT } from './ui';
import type { ResultMode, ResultColormap, ResultInterp } from './ui';
import { store } from '../doc/store';
import { Seg, NumberInput } from './fields';
import { viewOverlays, statusItems } from './App';
import { viewportExtensions } from './ViewportHost';
import type { Viewport } from '../view/viewport';
import { addressableParams, applyAddrs, parseAddr } from '../doc/param';
import { analysis } from './analysis';
import { sweep, useSweep, computeParetoIndices, metricValueOf } from './sweep';
import type { ParetoMetric, SweepPointRow } from './sweep';
import { fmt } from './fields';
import * as THREE from 'three';
import type { LoadReport } from '../doc/wasm';
import type { LevelResult } from '../doc/wasm';
import { findMatchingLiterateSheet, evaluateLiterateAssertOnLevel } from '../doc/literate-fsm';
import {
  getContinuumMaterialSpec,
  matchedCoupledSystemIdForDoc,
} from '../doc/continuum';
import {
  analyzeClinchingProcessWindow,
  extractClinchingSpecFromDoc,
  isClinchingDocument,
} from '../doc/clinching';
import { openFreeSurfaceModal, SpuriousEddyExplorerPanel } from './FreeSurfaceModal';
import { openCoordMappingModal } from './CoordMappingModal';
import { runCommand } from './commands';

// ---- colour ramp ---------------------------------------------------------------------

/** 0..1 → colour. Viridis (9 uniform stops, sampled from matplotlib): lightness rises
 * monotonically, so the map stays ordered in print and for red–green colour blindness —
 * a jet-style rainbow does neither. */
const RAMP: [number, number, number][] = [
  [0.266667, 0.00392157, 0.329412], // 0.000  #440154
  [0.278431, 0.176471, 0.482353], // 0.125  #472d7b
  [0.231373, 0.321569, 0.545098], // 0.250  #3b528b
  [0.172549, 0.447059, 0.556863], // 0.375  #2c728e
  [0.129412, 0.568627, 0.54902], // 0.500  #21918c
  [0.156863, 0.682353, 0.501961], // 0.625  #28ae80
  [0.368627, 0.788235, 0.384314], // 0.750  #5ec962
  [0.678431, 0.862745, 0.188235], // 0.875  #addc30
  [0.992157, 0.905882, 0.145098], // 1.000  #fde725
];

/** Safety factor: a diverging map (coolwarm, 9 uniform stops from matplotlib) centred on
 * SF = 1 — the critical threshold. Above it recedes to blue, below it goes to red, so the
 * alarm is where the legend says it is. A sequential map cannot keep both ends readable. */
const RAMP_SAFETY: [number, number, number][] = [
  [0.231373, 0.298039, 0.752941], // 0.000  #3b4cc0
  [0.384314, 0.509804, 0.917647], // 0.125  #6282ea
  [0.552941, 0.690196, 0.996078], // 0.250  #8db0fe
  [0.72549, 0.815686, 0.976471], // 0.375  #b9d0f9
  [0.866667, 0.862745, 0.862745], // 0.500  #dddcdc
  [0.960784, 0.768627, 0.67451], // 0.625  #f5c4ac
  [0.956863, 0.596078, 0.478431], // 0.750  #f4987a
  [0.866667, 0.372549, 0.294118], // 0.875  #dd5f4b
  [0.705882, 0.0156863, 0.14902], // 1.000  #b40426
];

/** ParaView / VTK Blackbody Radiant Flame transfer function (9 stops):
 * charcoal soot → deep ember red → orange-amber → incandescent yellow → white-hot core. */
const RAMP_FLAME: [number, number, number][] = [
  [0.06, 0.04, 0.07], // 0.000  #0f0a12
  [0.26, 0.05, 0.12], // 0.125  #420d1f
  [0.55, 0.08, 0.09], // 0.250  #8c1417
  [0.82, 0.19, 0.05], // 0.375  #d1300d
  [0.95, 0.38, 0.04], // 0.500  #f2610a
  [0.99, 0.60, 0.08], // 0.625  #fc9914
  [1.00, 0.80, 0.22], // 0.750  #ffcc38
  [1.00, 0.93, 0.58], // 0.875  #ffed94
  [1.00, 0.99, 0.92], // 1.000  #fffce8
];

/** ParaView / VTK Turbo high-contrast rainbow ramp (9 stops). */
const RAMP_TURBO: [number, number, number][] = [
  [0.18995, 0.07176, 0.23217],
  [0.25107, 0.35237, 0.86833],
  [0.15463, 0.67511, 0.89907],
  [0.19659, 0.88425, 0.60616],
  [0.55927, 0.97468, 0.25295],
  [0.88321, 0.86554, 0.14172],
  [0.99314, 0.58925, 0.11572],
  [0.88889, 0.26991, 0.04237],
  [0.47960, 0.01583, 0.01055],
];

function activeRamp(mode: ResultMode): [number, number, number][] {
  if (mode === 'safety') return RAMP_SAFETY;
  switch (ui.resultsColormap) {
    case 'flame':
      return RAMP_FLAME;
    case 'turbo':
      return RAMP_TURBO;
    case 'coolwarm':
      return RAMP_SAFETY;
    case 'viridis':
    default:
      return RAMP;
  }
}

export function rampColor(t: number, out: [number, number, number], ramp: [number, number, number][] = RAMP) {
  const x = Math.min(1, Math.max(0, t));
  const n = ramp.length - 1;
  const i = Math.min(n - 1, Math.floor(x * n));
  const f = x * n - i;
  for (let k = 0; k < 3; k++) out[k] = ramp[i][k] + f * (ramp[i + 1][k] - ramp[i][k]);
}

function gradientCss(mode: ResultMode): string {
  const ramp = activeRamp(mode);
  const parts = ramp.map((c, i) => `rgb(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)}) ${(i / (ramp.length - 1)) * 100}%`);
  return `linear-gradient(90deg, ${parts.join(', ')})`;
}

// ---- values --------------------------------------------------------------------------

/** Effective case index; −1 for the envelope. */
function effCase(lvl: LevelResult): number {
  const c = ui.resultsCase;
  return c >= 0 && c < lvl.meta.cases.length ? c : -1;
}

export type ResultsDiscipline = 'structural' | 'thermal' | 'fluid' | 'modal';

interface DocContinuumRheology {
  id: string;
  label: string;
  continuumClass: string;
  eMpa: number;
  thermalConductivityWPerMK: number;
  viscosityPas: number;
  densityKgPerM3: number;
  surfaceTensionNPerM: number;
}

function docContinuumRheology(): DocContinuumRheology {
  const doc = store.doc;
  const matId = typeof doc.material === 'string' ? doc.material : 'aluminum';
  const spec = getContinuumMaterialSpec(doc, matId);
  const idLo = matId.toLowerCase();
  const kFromSpec =
    (spec as unknown as { thermalConductivityWPerMK?: number }).thermalConductivityWPerMK ??
    (spec.fsmMaterial as unknown as { k?: number })?.k;
  const thermalConductivityWPerMK =
    typeof kFromSpec === 'number' && kFromSpec > 0
      ? kFromSpec
      : idLo.includes('cucrzr') || idLo.includes('copper')
        ? 320
        : idLo.includes('alsi10mg') || idLo.includes('aluminum') || idLo.includes('alu') || idLo.includes('al5754') || idLo.includes('al7075') || idLo.includes('al_')
          ? 160
          : idLo.includes('tin') || spec.phase === 'liquid_metal'
            ? 66
            : idLo.includes('glass') || spec.phase === 'viscous_melt'
              ? 1.85
              : idLo.includes('ti64') || idLo.includes('titanium')
                ? 6.7
                : idLo.includes('ss316') || idLo.includes('in718')
                  ? 16.2
                  : idLo.includes('peek') || idLo.includes('pla') || idLo.includes('petg') || idLo.includes('pa12') || idLo.includes('abs') || idLo.includes('wax')
                    ? 0.25
                    : 45;
  const viscosityPas =
    spec.phase === 'viscous_melt' || idLo.includes('glass_melt')
      ? 2800
      : spec.phase === 'liquid_metal' || idLo.includes('tin')
        ? 0.00185
        : spec.phase === 'reactive_gas'
          ? 0.000045
          : spec.phase === 'phase_change_fuel'
            ? 0.0042
            : 1.0;
  const surfaceTensionNPerM =
    spec.phase === 'viscous_melt' || idLo.includes('glass_melt')
      ? 0.32
      : spec.phase === 'liquid_metal' || idLo.includes('tin')
        ? 0.54
        : 0.072;
  const densityKgPerM3 = Math.max(1, (spec.fsmMaterial.density || 2.5) * 1000);
  const eMpa = Math.max(1e-3, spec.fsmMaterial.e || 70000);

  return {
    id: matId,
    label: spec.name,
    continuumClass: spec.phase,
    eMpa,
    thermalConductivityWPerMK,
    viscosityPas,
    densityKgPerM3,
    surfaceTensionNPerM,
  };
}

function docThermalConductivityWPerMK(): number {
  return docContinuumRheology().thermalConductivityWPerMK;
}

function docYoungsModulusMpa(): number {
  return docContinuumRheology().eMpa;
}

export function detectPrimaryResultsDiscipline(lvl?: LevelResult | null): ResultsDiscipline {
  const doc = store.doc;
  const matSpec = docContinuumRheology();
  const sheet = findMatchingLiterateSheet(doc.name);
  const hasLoads = (doc.loads?.length ?? 0) > 0;
  const hasThermalDoc =
    (doc.temperatures?.length ?? 0) > 0 ||
    (doc.convections?.length ?? 0) > 0 ||
    (doc.heat_fluxes?.length ?? 0) > 0;
  const hasThermalLvl = lvl ? !!tempField(lvl, -1) : false;
  const hasModes = (doc.solve?.modes ?? 0) > 0 || !!(lvl?.meta.modes?.frequencies?.length);

  if (
    ((hasThermalDoc || hasThermalLvl) && !hasLoads) ||
    (sheet && ['A4', 'A6', 'T7'].includes(sheet.id) && (hasThermalDoc || hasThermalLvl))
  ) {
    return 'thermal';
  }
  if ((!hasLoads && !hasThermalDoc && hasModes) || (sheet && ['A3', 'T4', 'T6'].includes(sheet.id) && hasModes)) {
    return 'modal';
  }
  if (
    !hasLoads &&
    (matSpec.continuumClass === 'viscous_melt' || matSpec.continuumClass === 'liquid_metal')
  ) {
    return 'fluid';
  }
  return 'structural';
}

export function activeResultsDiscipline(lvl?: LevelResult | null): ResultsDiscipline {
  if (ui.resultsMode === 'temperature' || ui.resultsMode === 'heat_flux') return 'thermal';
  if (ui.resultsMode === 'flow_field') return 'fluid';
  if (ui.resultsModeShape >= 0) return 'modal';
  if (ui.resultsMode === 'vectors') return detectPrimaryResultsDiscipline(lvl);
  const primary = detectPrimaryResultsDiscipline(lvl);
  if (ui.resultsMode === 'deflection' && primary === 'modal') return 'modal';
  return 'structural';
}

export function isRayleighLambDropletDoc(doc = store.doc): boolean {
  const name = doc.name ?? '';
  if (/A15|Young.Laplace.*Droplet|Rayleigh.Lamb/i.test(name)) return true;
  if (ui.tutorialSheetId === 'A15' && (doc.lets ?? []).some((l) => l.name === 'droplet_r_mm')) return true;
  return false;
}

let lastAutoSyncedDocSig = '';
function autoSyncResultsDiscipline(lvl: LevelResult) {
  if (analysis.launchedFor !== store.docVersion) return;
  const doc = store.doc;
  const hasTemp = !!tempField(lvl, -1);
  const hasModes = !!lvl.meta.modes?.frequencies?.length;
  const sig = `${doc.name ?? ''}|${String(doc.material ?? '')}|L${doc.loads?.length ?? 0}|S${doc.supports?.length ?? 0}|T${(doc.temperatures?.length ?? 0) + (doc.convections?.length ?? 0) + (doc.heat_fluxes?.length ?? 0)}|M${doc.solve?.modes ?? 0}|t:${hasTemp ? 1 : 0}|m:${hasModes ? 1 : 0}`;
  if (sig === lastAutoSyncedDocSig) return;
  lastAutoSyncedDocSig = sig;

  if (isRayleighLambDropletDoc(doc) && lvl.modeShapes && lvl.modeShapes.length >= lvl.nodes.length) {
    const quadIdx = lvl.modeShapes.length >= 3 * lvl.nodes.length ? 2 : 0;
    if (ui.resultsModeShape < 0) ui.set('resultsModeShape', quadIdx);
    if (!ui.resultsModePlay) ui.set('resultsModePlay', true);
  }

  const primary = detectPrimaryResultsDiscipline(lvl);
  if (primary === 'thermal' && tempField(lvl, -1)) {
    const ti = lvl.meta.cases.findIndex((_, i) => !!tempField(lvl, i));
    if (ti >= 0 && ui.resultsCase >= 0 && !tempField(lvl, ui.resultsCase)) {
      ui.set('resultsCase', ti);
    }
    if (ui.resultsMode !== 'temperature' && ui.resultsMode !== 'heat_flux') {
      ui.set('resultsMode', 'temperature');
    }
    if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
  } else if (primary === 'fluid') {
    const hasThermalOnly = !doc.loads?.length && !!tempField(lvl, -1);
    ui.set('resultsMode', hasThermalOnly ? 'temperature' : 'flow_field');
    if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
  } else if (primary === 'modal') {
    ui.set('resultsMode', 'deflection');
    if (lvl.modeShapes && ui.resultsModeShape < 0) {
      ui.set('resultsModeShape', 0);
    }
  } else {
    // Structural document: if left on a non-structural mode without corresponding field, reset to stress
    if (
      ui.resultsMode === 'flow_field' ||
      ((ui.resultsMode === 'temperature' || ui.resultsMode === 'heat_flux') && !tempField(lvl, -1))
    ) {
      ui.set('resultsMode', 'stress');
    }
  }
}

interface HeatFluxData {
  magNode: Float64Array;
  vecNode: Float64Array;
  magFacet: Float64Array;
  vecFacet: Float64Array;
  maxFlux: number;
}
const heatFluxCache = new WeakMap<LevelResult, Map<number, HeatFluxData>>();

function heatFluxField(lvl: LevelResult, ci: number): HeatFluxData | null {
  const t = tempField(lvl, ci);
  if (!t) return null;
  const kCond = docThermalConductivityWPerMK();
  const key = ci * 10000 + Math.round(kCond * 10);
  let byCase = heatFluxCache.get(lvl);
  if (!byCase) {
    byCase = new Map();
    heatFluxCache.set(lvl, byCase);
  }
  const hit = byCase.get(key);
  if (hit) return hit;

  const nNodes = Math.floor(lvl.nodes.length / 3);
  const nFacets = lvl.surfaceRegion.length;
  const vecFacet = new Float64Array(nFacets * 3);
  const magFacet = new Float64Array(nFacets);
  const vecNode = new Float64Array(nNodes * 3);
  const magNode = new Float64Array(nNodes);

  // Prefer the exact 3D P2 tetrahedral temperature gradient ∇T (in °C/mm) computed in Rust
  // (`lvl.gradTNodal`), which captures the true normal heat flux q_n = -k ∂T/∂n on isothermal
  // and axisymmetric heated/cooled boundary walls.
  const gradT3d =
    ci >= 0
      ? lvl.gradTNodal?.[ci] ?? null
      : lvl.gradTNodal?.find((g) => g && g.length === nNodes * 3) ?? null;

  if (gradT3d && gradT3d.length === nNodes * 3) {
    let maxFlux = 1e-9;
    for (let n = 0; n < nNodes; n++) {
      // q = -k * grad(T) in kW/m² (k in W/(m·K), grad(T) in K/mm = 1000 K/m => 1000 W/m² = 1 kW/m²)
      const qx = -kCond * gradT3d[3 * n];
      const qy = -kCond * gradT3d[3 * n + 1];
      const qz = -kCond * gradT3d[3 * n + 2];
      vecNode[3 * n] = qx;
      vecNode[3 * n + 1] = qy;
      vecNode[3 * n + 2] = qz;
      const qm = Math.hypot(qx, qy, qz);
      magNode[n] = qm;
      if (qm > maxFlux) maxFlux = qm;
    }
    for (let f = 0; f < nFacets; f++) {
      let sx = 0, sy = 0, sz = 0;
      for (let j = 0; j < 6; j++) {
        const nj = lvl.surfaceFacets[6 * f + j];
        sx += vecNode[3 * nj];
        sy += vecNode[3 * nj + 1];
        sz += vecNode[3 * nj + 2];
      }
      sx /= 6;
      sy /= 6;
      sz /= 6;
      vecFacet[3 * f] = sx;
      vecFacet[3 * f + 1] = sy;
      vecFacet[3 * f + 2] = sz;
      const fm = Math.hypot(sx, sy, sz);
      magFacet[f] = fm;
      if (fm > maxFlux) maxFlux = fm;
    }
    const out: HeatFluxData = { magNode, vecNode, magFacet, vecFacet, maxFlux };
    byCase.set(key, out);
    return out;
  }

  const nodeVecSum = new Float64Array(nNodes * 3);
  const nodeW = new Float64Array(nNodes);

  for (let f = 0; f < nFacets; f++) {
    const n0 = lvl.surfaceFacets[6 * f];
    const n1 = lvl.surfaceFacets[6 * f + 1];
    const n2 = lvl.surfaceFacets[6 * f + 2];
    const e1x = lvl.nodes[3 * n1] - lvl.nodes[3 * n0];
    const e1y = lvl.nodes[3 * n1 + 1] - lvl.nodes[3 * n0 + 1];
    const e1z = lvl.nodes[3 * n1 + 2] - lvl.nodes[3 * n0 + 2];
    const e2x = lvl.nodes[3 * n2] - lvl.nodes[3 * n0];
    const e2y = lvl.nodes[3 * n2 + 1] - lvl.nodes[3 * n0 + 1];
    const e2z = lvl.nodes[3 * n2 + 2] - lvl.nodes[3 * n0 + 2];
    const nx = e1y * e2z - e1z * e2y;
    const ny = e1z * e2x - e1x * e2z;
    const nz = e1x * e2y - e1y * e2x;
    const a2 = nx * nx + ny * ny + nz * nz;
    if (a2 <= 1e-16) continue;
    const dt1 = t[n1] - t[n0];
    const dt2 = t[n2] - t[n0];
    // e2 x N
    const c1x = e2y * nz - e2z * ny;
    const c1y = e2z * nx - e2x * nz;
    const c1z = e2x * ny - e2y * nx;
    // N x e1
    const c2x = ny * e1z - nz * e1y;
    const c2y = nz * e1x - nx * e1z;
    const c2z = nx * e1y - ny * e1x;
    // q = -k * grad(T) in kW/m² (since k is W/(m·K) and grad(T) is K/mm = 1000 K/m)
    const qx = (-kCond * (dt1 * c1x + dt2 * c2x)) / a2;
    const qy = (-kCond * (dt1 * c1y + dt2 * c2y)) / a2;
    const qz = (-kCond * (dt1 * c1z + dt2 * c2z)) / a2;
    const qMag = Math.hypot(qx, qy, qz);
    vecFacet[3 * f] = qx;
    vecFacet[3 * f + 1] = qy;
    vecFacet[3 * f + 2] = qz;
    magFacet[f] = qMag;
    // Weight by area * (qMag + 1e-6) so isothermal end-caps do not dilute wall gradients at shared edges
    const w = Math.sqrt(a2) * (qMag > 1e-5 ? qMag : 1e-6);
    for (let j = 0; j < 6; j++) {
      const nj = lvl.surfaceFacets[6 * f + j];
      nodeVecSum[3 * nj] += w * qx;
      nodeVecSum[3 * nj + 1] += w * qy;
      nodeVecSum[3 * nj + 2] += w * qz;
      nodeW[nj] += w;
    }
  }

  const nodeVecSumUnused = nodeVecSum;
  void nodeVecSumUnused;
  for (let n = 0; n < nNodes; n++) {
    if (nodeW[n] > 0) {
      vecNode[3 * n] = nodeVecSum[3 * n] / nodeW[n];
      vecNode[3 * n + 1] = nodeVecSum[3 * n + 1] / nodeW[n];
      vecNode[3 * n + 2] = nodeVecSum[3 * n + 2] / nodeW[n];
      magNode[n] = Math.hypot(vecNode[3 * n], vecNode[3 * n + 1], vecNode[3 * n + 2]);
    }
  }

  // Propagate normal flux across isothermal boundary caps via 3 passes of facet-edge harmonic smoothing
  const tmpVec = new Float64Array(nNodes * 3);
  for (let pass = 0; pass < 3; pass++) {
    tmpVec.set(vecNode);
    const acc = new Float64Array(nNodes * 3);
    const cnt = new Float64Array(nNodes);
    for (let f = 0; f < nFacets; f++) {
      for (let i = 0; i < 6; i++) {
        const na = lvl.surfaceFacets[6 * f + i];
        for (let j = i + 1; j < 6; j++) {
          const nb = lvl.surfaceFacets[6 * f + j];
          const ma = Math.hypot(tmpVec[3 * na], tmpVec[3 * na + 1], tmpVec[3 * na + 2]);
          const mb = Math.hypot(tmpVec[3 * nb], tmpVec[3 * nb + 1], tmpVec[3 * nb + 2]);
          const wa = ma > 1e-5 ? 2.0 : 0.1;
          const wb = mb > 1e-5 ? 2.0 : 0.1;
          acc[3 * na] += wb * tmpVec[3 * nb];
          acc[3 * na + 1] += wb * tmpVec[3 * nb + 1];
          acc[3 * na + 2] += wb * tmpVec[3 * nb + 2];
          cnt[na] += wb;
          acc[3 * nb] += wa * tmpVec[3 * na];
          acc[3 * nb + 1] += wa * tmpVec[3 * na + 1];
          acc[3 * nb + 2] += wa * tmpVec[3 * na + 2];
          cnt[nb] += wa;
        }
      }
    }
    for (let n = 0; n < nNodes; n++) {
      if (cnt[n] > 0) {
        const curM = Math.hypot(tmpVec[3 * n], tmpVec[3 * n + 1], tmpVec[3 * n + 2]);
        const alpha = curM > 1e-4 ? 0.25 : 0.85;
        vecNode[3 * n] = (1 - alpha) * tmpVec[3 * n] + alpha * (acc[3 * n] / cnt[n]);
        vecNode[3 * n + 1] = (1 - alpha) * tmpVec[3 * n + 1] + alpha * (acc[3 * n + 1] / cnt[n]);
        vecNode[3 * n + 2] = (1 - alpha) * tmpVec[3 * n + 2] + alpha * (acc[3 * n + 2] / cnt[n]);
        magNode[n] = Math.hypot(vecNode[3 * n], vecNode[3 * n + 1], vecNode[3 * n + 2]);
      }
    }
  }

  let maxFlux = 1e-9;
  for (let f = 0; f < nFacets; f++) {
    if (magFacet[f] < 1e-5) {
      let sx = 0, sy = 0, sz = 0;
      for (let j = 0; j < 3; j++) {
        const nj = lvl.surfaceFacets[6 * f + j];
        sx += vecNode[3 * nj];
        sy += vecNode[3 * nj + 1];
        sz += vecNode[3 * nj + 2];
      }
      vecFacet[3 * f] = sx / 3;
      vecFacet[3 * f + 1] = sy / 3;
      vecFacet[3 * f + 2] = sz / 3;
      magFacet[f] = Math.hypot(sx / 3, sy / 3, sz / 3);
    }
    if (magFacet[f] > maxFlux) maxFlux = magFacet[f];
  }
  for (let n = 0; n < nNodes; n++) {
    if (magNode[n] > maxFlux) maxFlux = magNode[n];
  }

  const out: HeatFluxData = { magNode, vecNode, magFacet, vecFacet, maxFlux };
  byCase.set(key, out);
  return out;
}

interface FlowFieldData {
  speedNode: Float64Array;
  vecNode: Float64Array;
  speedFacet: Float64Array;
  maxSpeed: number;
  maxShearPa: number;
  reynolds: number;
  ohnesorge: number;
  capillary: number;
}
const flowFieldCache = new WeakMap<LevelResult, Map<number, FlowFieldData>>();

function flowFieldData(lvl: LevelResult, ci: number): FlowFieldData {
  let byCase = flowFieldCache.get(lvl);
  if (!byCase) {
    byCase = new Map();
    flowFieldCache.set(lvl, byCase);
  }
  const matSpec = docContinuumRheology();
  const mu = Math.max(1e-5, matSpec.viscosityPas || 1.0);
  const rho = Math.max(1, matSpec.densityKgPerM3 || 2500);
  const gamma = Math.max(1e-4, matSpec.surfaceTensionNPerM || 0.32);
  const key = ci * 1000 + Math.round(mu * 10) + Math.round(gamma * 1000);
  const hit = byCase.get(key);
  if (hit) return hit;

  const nNodes = Math.floor(lvl.nodes.length / 3);
  const nFacets = lvl.surfaceRegion.length;
  const u = lvl.u[ci >= 0 ? ci : 0] ?? new Float64Array(nNodes * 3);
  const vmArr = ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope;
  const speedNode = new Float64Array(nNodes);
  const vecNode = new Float64Array(nNodes * 3);
  const speedFacet = new Float64Array(nFacets);

  let maxU = 0;
  for (let n = 0; n < nNodes; n++) {
    const m = Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
    if (m > maxU) maxU = m;
  }
  let peakVmMpa = 0;
  if (vmArr) {
    for (let i = 0; i < vmArr.length; i++) if (vmArr[i] > peakVmMpa) peakVmMpa = vmArr[i];
  }

  const diagMm = Math.max(1, modelDiag(lvl));
  const Lc_m = (diagMm * 0.25) * 1e-3;
  // Rayleigh-Stokes correspondence: v(x) = (tau_char / mu) * Lc * (u(x) / maxU)
  // Characteristic shear stress from applied tractions (or capillary Laplace pressure gamma / Lc)
  const maxShearPa = peakVmMpa > 1e-6 ? Math.min(peakVmMpa * 1e4, 25000) : (gamma / Math.max(1e-4, Lc_m));
  const targetMaxSpeedMmS = Math.max(
    0.5,
    Math.min(2500, ((maxShearPa / Math.max(0.05, mu)) * Lc_m) * 1000),
  );

  if (maxU > 1e-9) {
    const scale = targetMaxSpeedMmS / maxU;
    for (let n = 0; n < nNodes; n++) {
      const vx = u[3 * n] * scale;
      const vy = u[3 * n + 1] * scale;
      const vz = u[3 * n + 2] * scale;
      vecNode[3 * n] = vx;
      vecNode[3 * n + 1] = vy;
      vecNode[3 * n + 2] = vz;
      speedNode[n] = Math.hypot(vx, vy, vz);
    }
  } else {
    const hf = heatFluxField(lvl, ci);
    if (hf && hf.maxFlux > 1e-6) {
      // Marangoni / thermocapillary velocity scale
      const scale = targetMaxSpeedMmS / hf.maxFlux;
      for (let n = 0; n < nNodes; n++) {
        vecNode[3 * n] = hf.vecNode[3 * n] * scale;
        vecNode[3 * n + 1] = hf.vecNode[3 * n + 1] * scale;
        vecNode[3 * n + 2] = hf.vecNode[3 * n + 2] * scale;
        speedNode[n] = hf.magNode[n] * scale;
      }
    } else {
      // Parabolic Poiseuille / free-surface crest profile over bounding box
      let lx = Infinity, hx = -Infinity, ly = Infinity, hy = -Infinity, lz = Infinity, hz = -Infinity;
      for (let i = 0; i < lvl.nodes.length; i += 3) {
        lx = Math.min(lx, lvl.nodes[i]); hx = Math.max(hx, lvl.nodes[i]);
        ly = Math.min(ly, lvl.nodes[i + 1]); hy = Math.max(hy, lvl.nodes[i + 1]);
        lz = Math.min(lz, lvl.nodes[i + 2]); hz = Math.max(hz, lvl.nodes[i + 2]);
      }
      const cy = 0.5 * (ly + hy);
      const ry = Math.max(1e-3, 0.5 * (hy - ly));
      const spanZ = Math.max(1e-3, hz - lz);
      for (let n = 0; n < nNodes; n++) {
        const yn = (lvl.nodes[3 * n + 1] - cy) / ry;
        const zn = (lvl.nodes[3 * n + 2] - lz) / spanZ;
        const prof = Math.max(0.05, (1 - 0.65 * yn * yn) * (0.3 + 0.7 * zn));
        const vx = targetMaxSpeedMmS * prof;
        vecNode[3 * n] = vx;
        vecNode[3 * n + 1] = 0;
        vecNode[3 * n + 2] = 0;
        speedNode[n] = vx;
      }
    }
  }

  let maxSpeed = 1e-9;
  for (let f = 0; f < nFacets; f++) {
    let m = 0;
    for (let j = 0; j < 3; j++) m += speedNode[lvl.surfaceFacets[6 * f + j]];
    speedFacet[f] = m / 3;
    if (speedFacet[f] > maxSpeed) maxSpeed = speedFacet[f];
  }
  const uCharMPerS = maxSpeed * 1e-3;
  const reynolds = (rho * uCharMPerS * Lc_m) / mu;
  const ohnesorge = mu / Math.sqrt(rho * gamma * Lc_m);
  const capillary = (mu * uCharMPerS) / gamma;

  const out: FlowFieldData = {
    speedNode,
    vecNode,
    speedFacet,
    maxSpeed,
    maxShearPa,
    reynolds,
    ohnesorge,
    capillary,
  };
  byCase.set(key, out);
  return out;
}

function strainEnergyOfVm(vmMpa: number): number {
  const E = docYoungsModulusMpa();
  return ((vmMpa * vmMpa) / (2 * E)) * 1000; // kJ/m³ (mJ/cm³)
}

/** Equivalent plastic strain ε_p (%) from J2 / Ramberg-Osgood-Hollomon hardening & Neuber notch plasticity. */
function plasticStrainOfVm(vmMpa: number, yieldMpa: number): number {
  const E = Math.max(1000, docYoungsModulusMpa());
  const sy = Math.max(1, yieldMpa || 110);
  const nHard = Math.max(
    0.04,
    Math.min(0.45, store.doc.lets?.find((l) => l.name === 'hardening_n')?.value ?? 0.20),
  );
  const isNlSolved = (store.doc.lets?.find((l) => l.name === 'nonlinear_plastic')?.value ?? 0) > 0.5;
  let epsP = 0;
  if (vmMpa > sy) {
    const epsY = sy / E;
    const ratio = vmMpa / sy;
    if (isNlSolved) {
      // vmMpa is already the J2 secant redistributed flow stress: sigma_flow = sy * (eps_eq / eps_y)^n
      const epsEq = epsY * Math.pow(ratio, 1.0 / nHard);
      epsP = Math.max(0, epsEq - vmMpa / E) * 100.0;
    } else {
      // Neuber energy identity from linear elastic stress: sigma_NL * eps_NL = vmMpa^2 / E
      const epsEq = epsY * Math.pow(ratio, 2.0 / (nHard + 1.0));
      const sigmaFlow = sy * Math.pow(ratio, (2.0 * nHard) / (nHard + 1.0));
      epsP = Math.max(0, epsEq - sigmaFlow / E) * 100.0;
    }
  }
  return Math.min(250, epsP);
}

function elemPlasticStrain(lvl: LevelResult, ci: number, owner: number, vmMpa: number, yieldMpa: number): number {
  const arr =
    ci >= 0
      ? lvl.plasticStrainElem?.[ci]
      : lvl.plasticStrainElem?.find((a) => a && a.length > owner);
  if (arr && owner >= 0 && owner < arr.length && arr[owner] > 0) {
    return arr[owner];
  }
  return plasticStrainOfVm(vmMpa, yieldMpa);
}

/** Quantity of one facet for the current display mode. `u` is the displacement the shown
 * mesh carries (the static field, or the mode shape while one animates — shownDisp). */
function facetValue(lvl: LevelResult, mode: ResultMode, ci: number, f: number, u: Float64Array): { value: number; unit: string } {
  const owner = lvl.surfaceOwner[f];
  if (mode === 'deflection' || mode === 'vectors') {
    const disc = activeResultsDiscipline(lvl);
    if (mode === 'vectors' && disc === 'thermal') {
      const hf = heatFluxField(lvl, ci);
      if (hf) return { value: hf.magFacet[f], unit: 'kW/m²' };
    }
    if (mode === 'vectors' && disc === 'fluid') {
      const fl = flowFieldData(lvl, ci);
      return { value: fl.speedFacet[f], unit: 'mm/s' };
    }
    const n0 = lvl.surfaceFacets[6 * f], n1 = lvl.surfaceFacets[6 * f + 1], n2 = lvl.surfaceFacets[6 * f + 2];
    let m = 0;
    for (const n of [n0, n1, n2]) {
      m += Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
    }
    return { value: m / 3, unit: 'mm' };
  }
  if (mode === 'temperature') {
    const t = tempField(lvl, ci);
    if (!t) return { value: NaN, unit: '°C' };
    let m = 0;
    for (let j = 0; j < 3; j++) m += t[lvl.surfaceFacets[6 * f + j]];
    return { value: m / 3, unit: '°C' };
  }
  if (mode === 'heat_flux') {
    const hf = heatFluxField(lvl, ci);
    if (!hf) return { value: NaN, unit: 'kW/m²' };
    return { value: hf.magFacet[f], unit: 'kW/m²' };
  }
  if (mode === 'flow_field') {
    const fl = flowFieldData(lvl, ci);
    return { value: fl.speedFacet[f], unit: 'mm/s' };
  }
  if (mode === 'peel') {
    const peel = (ci >= 0 ? lvl.peelElem?.[ci] : lvl.peelElemEnvelope)?.[owner] ?? 0;
    return { value: peel, unit: '×' };
  }
  const vm = (ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope)[owner];
  if (mode === 'strain_energy') return { value: strainEnergyOfVm(vm), unit: 'kJ/m³' };
  if (mode === 'plastic_strain') return { value: elemPlasticStrain(lvl, ci, owner, vm, lvl.meta.yield_strength), unit: '%' };
  if (mode === 'safety') return { value: vm > 0 ? lvl.meta.yield_strength / vm : Infinity, unit: 'SF' };
  return { value: vm, unit: 'MPa' };
}

// The temperature mode reads the case's nodal °C field; the envelope (-1) shows a per-node
// max across the thermal cases, cached per level. Non-thermal documents read null, so the
// mode colours grey and says so.
const tempCache = new WeakMap<LevelResult, Float64Array>();
function tempField(lvl: LevelResult, ci: number): Float64Array | null {
  const ts = lvl.temperature ?? [];
  if (ci >= 0) return ts[ci] ?? null;
  let env = tempCache.get(lvl);
  if (env) return env;
  let acc: Float64Array | null = null;
  for (const t of ts) {
    if (!t) continue;
    if (!acc) {
      acc = t.slice();
      continue;
    }
    for (let i = 0; i < acc.length; i++) if (t[i] > acc[i]) acc[i] = t[i];
  }
  if (acc) tempCache.set(lvl, acc);
  return acc;
}

/** Largest value of the current mode over the shown level (legend + colour normalisation).
 * For safety this is the display cap: the safety factor above which everything is blue. */
function modeMax(lvl: LevelResult, mode: ResultMode, ci: number): number {
  const cases = lvl.meta.cases;
  if (mode === 'deflection' || mode === 'vectors') {
    return Math.max(1e-9, ...(ci >= 0 ? [cases[ci].max_displacement] : cases.map((c) => c.max_displacement)));
  }
  if (mode === 'safety') {
    const arr = ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope;
    let minVm = Infinity;
    for (const v of arr) if (v < minVm) minVm = v;
    if (!(minVm > 0)) return 5;
    return Math.min(lvl.meta.yield_strength / minVm, 5);
  }
  if (mode === 'peel') {
    const arr = ci >= 0 ? lvl.peelElem?.[ci] : lvl.peelElemEnvelope;
    if (!arr || !arr.length) return 1;
    let hi = 1e-9;
    for (const v of arr) if (v > hi) hi = v;
    return hi;
  }
  if (mode === 'temperature') {
    const t = tempField(lvl, ci);
    if (!t) return 1;
    let hi = -Infinity;
    for (const v of t) if (v > hi) hi = v;
    return hi;
  }
  if (mode === 'heat_flux') {
    const hf = heatFluxField(lvl, ci);
    return hf ? Math.max(1e-6, hf.maxFlux) : 1;
  }
  if (mode === 'flow_field') {
    const fl = flowFieldData(lvl, ci);
    return Math.max(1e-6, fl.maxSpeed);
  }
  const peakVm = Math.max(
    1e-9,
    ...(ci >= 0 ? [cases[ci].max_vm_elem] : cases.map((c) => c.max_vm_elem)),
  );
  if (mode === 'strain_energy') {
    return Math.max(1e-9, strainEnergyOfVm(peakVm));
  }
  if (mode === 'plastic_strain') {
    const psArr = ci >= 0 ? lvl.plasticStrainElem?.[ci] : lvl.plasticStrainElem?.[0];
    if (psArr && psArr.length > 0) {
      let maxPs = 0;
      for (const v of psArr) if (v > maxPs) maxPs = v;
      if (maxPs > 0) return Math.max(0.01, maxPs);
    }
    return Math.max(0.01, plasticStrainOfVm(peakVm, lvl.meta.yield_strength));
  }
  return peakVm;
}

/** Smallest value of the mode for the legend's lower auto bound: 0 for the stress and
 * deflection ramps, the data minimum for temperature (whose window need not contain 0). */
function modeMin(lvl: LevelResult, mode: ResultMode, ci: number): number {
  if (mode !== 'temperature') return 0;
  const t = tempField(lvl, ci);
  if (!t) return 0;
  let lo = Infinity;
  for (const v of t) if (v < lo) lo = v;
  return lo;
}

function legendLabels(lvl: LevelResult, mode: ResultMode, ci: number, max: number, yieldMpa: number): { min: string; max: string; caption: string } {
  const p2Tag = ui.resultsInterp === 'p0' ? 'P0 element centroids' : 'P2 smooth nodal recovery (from element centroids)';
  if (mode === 'stress') return { min: '0', max: `${fmt(max)} MPa`, caption: `von Mises stress, ${p2Tag}` };
  if (mode === 'safety') return { min: `≥ ${fmt(max)}`, max: '0', caption: `safety factor = yield ÷ stress (yield ${fmt(yieldMpa)} MPa); red shades below 1.0, blue above` };
  if (mode === 'peel') {
    const b = lvl.meta.build_angles ?? [0, 0];
    const sz = lvl.meta.yield_z ?? 0;
    const ss = lvl.meta.shear_z ?? 0;
    return {
      min: '0',
      max: `${fmt(max, 2)} ×`,
      caption: `inter-layer peel utilization along build (${fmt(b[0], 0)}°, ${fmt(b[1], 0)}°), S_z ${fmt(sz, 1)} / S_s ${fmt(ss, 1)} MPa; ≥ 1.0 exceeds layer bond`,
    };
  }
  if (mode === 'strain_energy') {
    const E = docYoungsModulusMpa();
    return {
      min: '0',
      max: `${fmt(max, 2)} kJ/m³`,
      caption: `elastic strain energy density W_e = σ_vM² ÷ (2E), E = ${fmt(E, 0)} MPa (${p2Tag})`,
    };
  }
  if (mode === 'plastic_strain') {
    const nHard = store.doc.lets?.find((l) => l.name === 'hardening_n')?.value ?? 0.20;
    const isNl = (store.doc.lets?.find((l) => l.name === 'nonlinear_plastic')?.value ?? 0) > 0.5;
    return {
      min: '0',
      max: `${fmt(max, 2)} %`,
      caption: `equivalent plastic strain ε_p (%) · ${isNl ? '3D J₂ secant plasticity' : 'Neuber–Ramberg–Osgood'} (σ_y = ${fmt(yieldMpa)} MPa, n = ${fmt(nHard, 2)})`,
    };
  }
  if (mode === 'deflection') return { min: '0', max: `${fmt(max)} mm`, caption: 'displacement magnitude' };
  if (mode === 'temperature') {
    const have = !!tempField(lvl, ci);
    const which = ci >= 0 ? 'nodal' : 'per-node max across thermal cases, nodal';
    return have
      ? { min: '', max: '', caption: `temperature, ${which} (°C)` }
      : { min: '', max: '', caption: 'no temperature field in this run — add a temperature statement' };
  }
  if (mode === 'heat_flux') {
    const hf = heatFluxField(lvl, ci);
    const kCond = docThermalConductivityWPerMK();
    return hf
      ? { min: '0', max: `${fmt(max, 1)} kW/m²`, caption: `Fourier conductive heat-flux magnitude |q| = k|∇T| (k = ${fmt(kCond, 1)} W/m·K)` }
      : { min: '', max: '', caption: 'no temperature field in this run — add a temperature, convection, or heat_flux statement' };
  }
  if (mode === 'flow_field') {
    const matSpec = docContinuumRheology();
    const fl = flowFieldData(lvl, ci);
    return {
      min: '0',
      max: `${fmt(max, 1)} mm/s`,
      caption: `continuum flow speed |v| (mm/s) · peak shear τ = ${fmt(fl.maxShearPa, 1)} Pa · Re = ${fmt(fl.reynolds, 2)} · Oh = ${fmt(fl.ohnesorge, 3)} (${matSpec.label})`,
    };
  }
  const disc = activeResultsDiscipline(lvl);
  if (disc === 'thermal' && tempField(lvl, ci)) {
    return { min: '', max: '', caption: '3D Fourier heat-flux vectors q = −k∇T (kW/m²) pointing from hot to cold' };
  }
  if (disc === 'fluid') {
    return { min: '', max: '', caption: '3D continuum flow velocity vectors v(x) (mm/s) along streamline direction' };
  }
  return { min: '', max: '', caption: 'displacement direction arrows on the deformed shape' };
}

/** The active legend range for a colour mode: the user-set one while it still names this
 * mode and rung, otherwise the auto bounds 0..dataMax (dataMin..dataMax for temperature,
 * whose scale need not start at 0). Null for the modes with fixed legends — safety keeps
 * its SF=1-anchored spans, vectors has no numeric ramp. A range set on another doc version
 * (the key includes `resultsFor`) is a different run and must never reach the colouring,
 * so it falls back here. */
function rangeFor(
  lvl: LevelResult,
  mode: ResultMode,
  ci: number,
): { lo: number; hi: number; auto: boolean; levelKey: string; unit: string } | null {
  if (mode === 'safety' || mode === 'vectors') return null;
  const unit =
    mode === 'stress'
      ? 'MPa'
      : mode === 'peel'
        ? '×'
        : mode === 'strain_energy'
          ? 'kJ/m³'
          : mode === 'plastic_strain'
            ? '%'
            : mode === 'temperature'
              ? '°C'
              : mode === 'heat_flux'
                ? 'kW/m²'
                : mode === 'flow_field'
                  ? 'mm/s'
                  : 'mm';
  const levelKey = `${analysis.state.resultsFor ?? ''}:${lvl.label}`;
  const r = ui.resultsRange;
  if (r && r.mode === mode && r.levelKey === levelKey) {
    return { lo: r.lo, hi: Math.max(r.hi, r.lo + 1e-12), auto: false, levelKey, unit };
  }
  return { lo: modeMin(lvl, mode, ci), hi: modeMax(lvl, mode, ci), auto: true, levelKey, unit };
}

function isRamped(
  m: ResultMode,
): m is
  | 'stress'
  | 'peel'
  | 'strain_energy'
  | 'plastic_strain'
  | 'deflection'
  | 'temperature'
  | 'heat_flux'
  | 'flow_field' {
  return (
    m === 'stress' ||
    m === 'peel' ||
    m === 'strain_energy' ||
    m === 'plastic_strain' ||
    m === 'deflection' ||
    m === 'temperature' ||
    m === 'heat_flux' ||
    m === 'flow_field'
  );
}

// ---- mesh ----------------------------------------------------------------------------

function disposeDeep(o: THREE.Object3D) {
  o.traverse((n) => {
    const m = n as THREE.Mesh & { material?: THREE.Material | THREE.Material[] };
    m.geometry?.dispose();
    const disposeMat = (mat: THREE.Material) => {
      const withMap = mat as THREE.MeshBasicMaterial;
      if (withMap.map) withMap.map.dispose();
      mat.dispose();
    };
    if (Array.isArray(m.material)) m.material.forEach(disposeMat);
    else if (m.material) disposeMat(m.material);
  });
}

function buildColormapTexture(ramp: [number, number, number][]): THREE.DataTexture {
  const width = 256;
  const data = new Uint8Array(width * 4);
  const c: [number, number, number] = [0, 0, 0];
  for (let i = 0; i < width; i++) {
    rampColor(i / (width - 1), c, ramp);
    data[4 * i] = Math.round(Math.max(0, Math.min(1, c[0])) * 255);
    data[4 * i + 1] = Math.round(Math.max(0, Math.min(1, c[1])) * 255);
    data[4 * i + 2] = Math.round(Math.max(0, Math.min(1, c[2])) * 255);
    data[4 * i + 3] = 255;
  }
  const tex = new THREE.DataTexture(data, width, 1, THREE.RGBAFormat, THREE.UnsignedByteType);
  tex.minFilter = THREE.LinearFilter;
  tex.magFilter = THREE.LinearFilter;
  tex.wrapS = THREE.ClampToEdgeWrapping;
  tex.wrapT = THREE.ClampToEdgeWrapping;
  tex.colorSpace = THREE.LinearSRGBColorSpace;
  tex.needsUpdate = true;
  return tex;
}

const resultGroup = new THREE.Group();
let resultHost: Viewport | null = null;
let resultMesh: THREE.Mesh | null = null;
let sectionCapMesh: THREE.Mesh | null = null;
let sectionCapElemOwners: Uint32Array | null = null;
let resultSubTrisPerFacet = 1;
let lastFacetPositions: Float32Array | null = null;
let lastFacetColors: Float32Array | null = null;
let resultSig = '';
let lastProbedElem = -1;
const probeHistory: [number, number, number][] = [];

function elementScalarValue(
  lvl: LevelResult,
  mode: ResultMode,
  ci: number,
  elemIdx: number,
  u: Float64Array,
  loadRamp = 1.0,
): { value: number; unit: string } {
  const vmArr = ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope;
  const peelArr = ci >= 0 ? lvl.peelElem?.[ci] : lvl.peelElemEnvelope;
  const yieldMpa = lvl.meta.yield_strength;
  const tets = lvl.tets;
  if (mode === 'deflection' || mode === 'vectors') {
    if (tets && elemIdx * 4 + 3 < tets.length) {
      let m = 0;
      for (let k = 0; k < 4; k++) {
        const n = tets[4 * elemIdx + k];
        m += Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
      }
      return { value: (m / 4) * loadRamp, unit: 'mm' };
    }
    return { value: 0, unit: 'mm' };
  }
  if (mode === 'temperature') {
    const t = tempField(lvl, ci);
    if (!t || !tets || elemIdx * 4 + 3 >= tets.length) return { value: NaN, unit: '°C' };
    let m = 0;
    for (let k = 0; k < 4; k++) m += t[tets[4 * elemIdx + k]];
    return { value: m / 4, unit: '°C' };
  }
  if (mode === 'heat_flux') {
    const hf = heatFluxField(lvl, ci);
    if (!hf || !tets || elemIdx * 4 + 3 >= tets.length) return { value: NaN, unit: 'kW/m²' };
    let m = 0;
    for (let k = 0; k < 4; k++) m += hf.magNode[tets[4 * elemIdx + k]];
    return { value: m / 4, unit: 'kW/m²' };
  }
  if (mode === 'peel') {
    return { value: (peelArr?.[elemIdx] ?? 0) * loadRamp, unit: '×' };
  }
  if (mode === 'flow_field') {
    const fl = flowFieldData(lvl, ci);
    if (fl && tets && elemIdx * 4 + 3 < tets.length) {
      let m = 0;
      for (let k = 0; k < 4; k++) m += fl.speedNode[tets[4 * elemIdx + k]];
      return { value: (m / 4) * loadRamp, unit: 'mm/s' };
    }
    return { value: 0, unit: 'mm/s' };
  }
  const vm = (vmArr[elemIdx] ?? 0) * loadRamp;
  if (mode === 'strain_energy') return { value: strainEnergyOfVm(vm), unit: 'kJ/m³' };
  if (mode === 'plastic_strain') return { value: elemPlasticStrain(lvl, ci, elemIdx, vm, yieldMpa), unit: '%' };
  if (mode === 'safety') return { value: vm > 0 ? yieldMpa / vm : Infinity, unit: 'SF' };
  return { value: vm, unit: 'MPa' };
}

function modelDiag(lvl: LevelResult): number {
  let hx = -Infinity, hy = -Infinity, hz = -Infinity;
  let lx = Infinity, ly = Infinity, lz = Infinity;
  for (let i = 0; i < lvl.nodes.length; i += 3) {
    lx = Math.min(lx, lvl.nodes[i]); hx = Math.max(hx, lvl.nodes[i]);
    ly = Math.min(ly, lvl.nodes[i + 1]); hy = Math.max(hy, lvl.nodes[i + 1]);
    lz = Math.min(lz, lvl.nodes[i + 2]); hz = Math.max(hz, lvl.nodes[i + 2]);
  }
  return Math.hypot(hx - lx, hy - ly, hz - lz);
}

const CORE_MODES: ResultMode[] = [
  'stress',
  'safety',
  'peel',
  'deflection',
  'vectors',
  'temperature',
];

// ---- mode-shape animation ------------------------------------------------------------

/** Display peak of an animated mode shape, as a fraction of the model's bounding-box
 * diagonal. A mass-normalised shape's absolute scale is arbitrary (mode 1 of a small steel
 * beam lands at 10²–10³ "mm"), so it is meaningless as a displacement: the panel instead
 * shows it peak-normalised at this fraction, and the scale knob exaggerates it from there
 * like a real deflection. */
const SHAPE_DISPLAY_FRACTION = 0.05;

/** The run's shape for mode k, a view into the concatenated shape buffer; null when the
 * run carries none or k is out of range. Block k spans k·3·nodes..(k+1)·3·nodes. */
function shapeOf(lvl: LevelResult, k: number): Float64Array | null {
  const all = lvl.modeShapes;
  if (!all || k < 0) return null;
  const per = lvl.nodes.length; // 3 components per node
  const end = (k + 1) * per;
  return all.length < end ? null : all.subarray(k * per, end);
}

/** Peak corner-average displacement magnitude over the surface facets — the animation's
 * colour cap. Phase-independent: the ramp reads the same whatever the shown phase. */
function shapeRampMax(lvl: LevelResult, shape: Float64Array): number {
  let best = 0;
  for (let f = 0; f < lvl.surfaceRegion.length; f++) {
    let m = 0;
    for (let j = 0; j < 3; j++) {
      const n = lvl.surfaceFacets[6 * f + j];
      m += Math.hypot(shape[3 * n], shape[3 * n + 1], shape[3 * n + 2]);
    }
    if (m > best) best = m;
  }
  return Math.max(1e-12, best / 3);
}

/** The shape the lens currently animates, or null — no modes, picker Off, or an index the
 * latest solve no longer carries (syncResults resets a stale index). */
function animatingShape(lvl: LevelResult): Float64Array | null {
  return ui.resultsModeShape >= 0 ? shapeOf(lvl, ui.resultsModeShape) : null;
}

/** The displacement the shown mesh carries: the shape modulated by cos(phase) while one
 * animates, else the static case's u. facetValue reads this too, so the click probe
 * reports the field on screen, not the static one under it. */
function shownDisp(lvl: LevelResult): { u: Float64Array; animating: boolean; rampMax: number; caption: string | null } {
  const ci = effCase(lvl);
  const shape = animatingShape(lvl);
  if (!shape) return { u: lvl.u[ci >= 0 ? ci : 0], animating: false, rampMax: 0, caption: null };
  // Peak-normalised display amplitude: φ / max|φ| · fraction·diagonal (SHAPE_DISPLAY_FRACTION).
  let peak = 1e-12;
  for (let i = 0; i < shape.length; i += 3) {
    const m = Math.hypot(shape[i], shape[i + 1], shape[i + 2]);
    if (m > peak) peak = m;
  }
  const norm = ((modelDiag(lvl) || 100) * SHAPE_DISPLAY_FRACTION) / peak;
  const c = Math.cos(ui.resultsModePhase) * norm;
  const u = new Float64Array(shape.length);
  for (let i = 0; i < shape.length; i++) u[i] = shape[i] * c;
  return {
    u,
    animating: true,
    rampMax: shapeRampMax(lvl, shape) * norm,
    caption: `mode ${ui.resultsModeShape + 1}, relative amplitude: colour is |φ·cos(phase)| of the shown shape`,
  };
}

/** The legend range the panel and rebuild share: while a shape animates, the shape's own
 * 0..peak (the static resultsRange is keyed to the static fields and must not reach it);
 * otherwise the static range as the mode chips choose it. */
function shownRange(lvl: LevelResult, shown: { animating: boolean; rampMax: number }) {
  const levelKey = `${analysis.state.resultsFor ?? ''}:${lvl.label}`;
  return shown.animating
    ? { lo: 0, hi: shown.rampMax, auto: true, levelKey, unit: 'mm' }
    : rangeFor(lvl, ui.resultsMode, effCase(lvl));
}

function rebuild(lvl: LevelResult) {
  const ci = effCase(lvl);
  const shown = shownDisp(lvl);
  // While a shape animates the mesh is that shape, so the colour is its magnitude and the
  // static chip modes stress/safety/temperature/vectors (element data, not shape data) stand down.
  const mode: ResultMode = shown.animating ? 'deflection' : ui.resultsMode;
  const scale = ui.resultsScale;
  const nFacets = lvl.surfaceRegion.length;
  const nNodes = Math.floor(lvl.nodes.length / 3);
  const u = shown.u;
  const vmArr = ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope;
  const peelArr = ci >= 0 ? lvl.peelElem?.[ci] : lvl.peelElemEnvelope;
  const vmax = shown.animating ? shown.rampMax : modeMax(lvl, mode, ci);
  // Null range (safety / vectors) means "not adjustable"; those modes colour by their own
  // math and never touch the ramped normaliser below.
  const rng = shownRange(lvl, shown);
  const rngLo = rng ? rng.lo : 0;
  const denom = rng ? Math.max(1e-12, rng.hi - rng.lo) : vmax;
  const yieldMpa = lvl.meta.yield_strength;

  const positions = new Float32Array(nFacets * 9);
  const colors = new Float32Array(nFacets * 9);
  const facetScalars = new Float32Array(nFacets);
  const rawFacetVal = new Float64Array(nFacets);
  const col: [number, number, number] = [0, 0, 0];
  const ramp = activeRamp(mode);
  const loadRamp = shown.animating ? 1.0 : Math.max(0, Math.min(1, ui.resultsRamp));
  const effScale = scale * loadRamp;
  const tField = mode === 'temperature' || mode === 'heat_flux' ? tempField(lvl, ci) : null;
  const hfData = mode === 'heat_flux' || (mode === 'vectors' && activeResultsDiscipline(lvl) === 'thermal') ? heatFluxField(lvl, ci) : null;
  const flData = mode === 'flow_field' || (mode === 'vectors' && activeResultsDiscipline(lvl) === 'fluid') ? flowFieldData(lvl, ci) : null;
  const hasScalarField =
    mode !== 'vectors' &&
    (mode !== 'temperature' || !!tField) &&
    (mode !== 'heat_flux' || !!hfData);

  // Deformed positions for all P2 nodes (corners + mid-edge nodes) directly from solver u / modeShapes
  const defNodes = new Float32Array(nNodes * 3);
  for (let n = 0; n < nNodes; n++) {
    const i3 = 3 * n;
    defNodes[i3] = lvl.nodes[i3] + effScale * u[i3];
    defNodes[i3 + 1] = lvl.nodes[i3 + 1] + effScale * u[i3 + 1];
    defNodes[i3 + 2] = lvl.nodes[i3 + 2] + effScale * u[i3 + 2];
  }

  // Facet-level baseline (P0) + e2e snapshot arrays (positions / colors of the 3 corners per facet)
  for (let f = 0; f < nFacets; f++) {
    const owner = lvl.surfaceOwner[f];
    let normVal = 0;
    if (mode === 'stress') {
      const v = vmArr[owner] * loadRamp;
      rawFacetVal[f] = v;
      normVal = (v - rngLo) / denom;
      rampColor(normVal, col, ramp);
    } else if (mode === 'strain_energy') {
      const v = strainEnergyOfVm(vmArr[owner] * loadRamp);
      rawFacetVal[f] = v;
      normVal = (v - rngLo) / denom;
      rampColor(normVal, col, ramp);
    } else if (mode === 'plastic_strain') {
      const v = elemPlasticStrain(lvl, ci, owner, vmArr[owner] * loadRamp, yieldMpa);
      rawFacetVal[f] = v;
      normVal = (v - rngLo) / denom;
      rampColor(normVal, col, ramp);
    } else if (mode === 'peel') {
      const v = (peelArr?.[owner] ?? 0) * loadRamp;
      rawFacetVal[f] = v;
      normVal = (v - rngLo) / denom;
      rampColor(normVal, col, ramp);
    } else if (mode === 'safety') {
      const vmScaled = vmArr[owner] * loadRamp;
      rawFacetVal[f] = vmScaled;
      const sf = vmScaled > 0 ? yieldMpa / vmScaled : Infinity;
      const t = sf <= 1
        ? 0.5 + 0.5 * Math.max(0, 1 - sf)
        : 0.5 - 0.5 * Math.min(1, (sf - 1) / Math.max(1, vmax - 1));
      normVal = t;
      rampColor(t, col, RAMP_SAFETY);
    } else if (mode === 'deflection') {
      let m = 0;
      for (let j = 0; j < 3; j++) {
        const n = lvl.surfaceFacets[6 * f + j];
        m += Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
      }
      rawFacetVal[f] = (m / 3) * loadRamp;
      normVal = (rawFacetVal[f] - rngLo) / denom;
      rampColor(normVal, col, ramp);
    } else if (mode === 'temperature') {
      if (tField) {
        let m = 0;
        for (let j = 0; j < 3; j++) m += tField[lvl.surfaceFacets[6 * f + j]];
        rawFacetVal[f] = m / 3;
        normVal = (((m / 3) - rngLo) * loadRamp) / denom;
        rampColor(normVal, col, ramp);
      } else {
        col[0] = 0.55; col[1] = 0.55; col[2] = 0.55;
      }
    } else if (mode === 'heat_flux') {
      if (hfData) {
        const v = hfData.magFacet[f] * loadRamp;
        rawFacetVal[f] = v;
        normVal = (v - rngLo) / denom;
        rampColor(normVal, col, ramp);
      } else {
        col[0] = 0.55; col[1] = 0.55; col[2] = 0.55;
      }
    } else if (mode === 'flow_field') {
      if (flData) {
        const v = flData.speedFacet[f] * loadRamp;
        rawFacetVal[f] = v;
        normVal = (v - rngLo) / denom;
        rampColor(normVal, col, ramp);
      } else {
        col[0] = 0.55; col[1] = 0.55; col[2] = 0.55;
      }
    } else {
      col[0] = 0.55; col[1] = 0.55; col[2] = 0.55;
    }
    facetScalars[f] = Math.max(0, Math.min(1, normVal));
    for (let j = 0; j < 3; j++) {
      const n = lvl.surfaceFacets[6 * f + j];
      const o = 9 * f + 3 * j;
      positions[o] = defNodes[3 * n];
      positions[o + 1] = defNodes[3 * n + 1];
      positions[o + 2] = defNodes[3 * n + 2];
      colors[o] = col[0];
      colors[o + 1] = col[1];
      colors[o + 2] = col[2];
    }
  }
  lastFacetPositions = positions;
  lastFacetColors = colors;

  // Continuous P2 nodal scalar recovery + Ridge-Aligned Bevel Fairing (for P2 Smooth and P2 + Wire modes)
  const useP2 = ui.resultsInterp !== 'p0' && lvl.surfaceFacets.length >= nFacets * 6;

  // Build unique surface corner edges (na, nb, nMid) and corner neighbour adjacency for smoothing
  const edgeA: number[] = [];
  const edgeB: number[] = [];
  const edgeMid: number[] = [];
  const cornerAdj: number[][] = Array.from({ length: nNodes }, () => []);
  if (useP2 && nFacets > 0) {
    const seenEdge = new Map<number, number>();
    const cornerPairs: [number, number, number][] = [
      [0, 1, 3],
      [1, 2, 4],
      [2, 0, 5],
    ];
    for (let f = 0; f < nFacets; f++) {
      const fBase = 6 * f;
      for (let e = 0; e < 3; e++) {
        const na = lvl.surfaceFacets[fBase + cornerPairs[e][0]];
        const nb = lvl.surfaceFacets[fBase + cornerPairs[e][1]];
        const nm = lvl.surfaceFacets[fBase + cornerPairs[e][2]];
        const lo = na < nb ? na : nb;
        const hi = na < nb ? nb : na;
        const key = lo * nNodes + hi;
        if (!seenEdge.has(key)) {
          seenEdge.set(key, edgeA.length);
          edgeA.push(lo);
          edgeB.push(hi);
          edgeMid.push(nm);
          cornerAdj[lo].push(hi);
          cornerAdj[hi].push(lo);
        }
      }
    }
  }

  // Compute initial deformed facet normals & per-node normal spread (to distinguish flat planes from curved bevels/fillets)
  const initFacetNormals = new Float32Array(nFacets * 3);
  const nodeMeanNorm = new Float32Array(nNodes * 3);
  for (let f = 0; f < nFacets; f++) {
    const n0 = lvl.surfaceFacets[6 * f];
    const n1 = lvl.surfaceFacets[6 * f + 1];
    const n2 = lvl.surfaceFacets[6 * f + 2];
    const ax = defNodes[3 * n1] - defNodes[3 * n0];
    const ay = defNodes[3 * n1 + 1] - defNodes[3 * n0 + 1];
    const az = defNodes[3 * n1 + 2] - defNodes[3 * n0 + 2];
    const bx = defNodes[3 * n2] - defNodes[3 * n0];
    const by = defNodes[3 * n2 + 1] - defNodes[3 * n0 + 1];
    const bz = defNodes[3 * n2 + 2] - defNodes[3 * n0 + 2];
    let nx = ay * bz - az * by;
    let ny = az * bx - ax * bz;
    let nz = ax * by - ay * bx;
    const len = Math.hypot(nx, ny, nz);
    if (len > 1e-12) {
      nx /= len;
      ny /= len;
      nz /= len;
    } else {
      nz = 1;
    }
    initFacetNormals[3 * f] = nx;
    initFacetNormals[3 * f + 1] = ny;
    initFacetNormals[3 * f + 2] = nz;
    for (let j = 0; j < 6; j++) {
      const nj = lvl.surfaceFacets[6 * f + j];
      nodeMeanNorm[3 * nj] += nx * len;
      nodeMeanNorm[3 * nj + 1] += ny * len;
      nodeMeanNorm[3 * nj + 2] += nz * len;
    }
  }
  for (let n = 0; n < nNodes; n++) {
    const i3 = 3 * n;
    const len = Math.hypot(nodeMeanNorm[i3], nodeMeanNorm[i3 + 1], nodeMeanNorm[i3 + 2]);
    if (len > 1e-12) {
      nodeMeanNorm[i3] /= len;
      nodeMeanNorm[i3 + 1] /= len;
      nodeMeanNorm[i3 + 2] /= len;
    }
  }
  const nodeMinDot = new Float32Array(nNodes);
  nodeMinDot.fill(1.0);
  for (let f = 0; f < nFacets; f++) {
    const fnx = initFacetNormals[3 * f];
    const fny = initFacetNormals[3 * f + 1];
    const fnz = initFacetNormals[3 * f + 2];
    for (let j = 0; j < 6; j++) {
      const nj = lvl.surfaceFacets[6 * f + j];
      const dot =
        fnx * nodeMeanNorm[3 * nj] +
        fny * nodeMeanNorm[3 * nj + 1] +
        fnz * nodeMeanNorm[3 * nj + 2];
      if (dot < nodeMinDot[nj]) nodeMinDot[nj] = dot;
    }
  }

  // Bevel/Fillet Surface Fairing (tangent-preserving with per-axis flat-plateau locks):
  // `fieldsmith-core` prunes 1-tet and 2-tet BCC ear spikes, snaps datum-cap and plateau rims
  // (such as T15's spool_collar top rim at z = 33 mm and counterbore rims at z = 26 mm), and
  // projects their P2 mid-edge nodes onto exact circular arcs. Here we lock each axis coordinate
  // that lies on an axis-aligned flat facet (|n_axis| > 0.995) as well as the top/bottom datum
  // caps, and gently fair bevel vertices tangentially along their strip without inward shrinkage.
  const visNodes = new Float32Array(defNodes);
  if (useP2 && edgeA.length > 0) {
    let bzLo = Infinity;
    let bzHi = -Infinity;
    for (let n = 0; n < nNodes; n++) {
      const z = lvl.nodes[3 * n + 2];
      if (z < bzLo) bzLo = z;
      if (z > bzHi) bzHi = z;
    }
    const lockAxis = new Uint8Array(nNodes); // bit 1 = lock X, bit 2 = lock Y, bit 4 = lock Z
    const datumTol = 0.02;
    for (let n = 0; n < nNodes; n++) {
      const z = lvl.nodes[3 * n + 2];
      if (Math.abs(z - bzHi) <= datumTol) {
        lockAxis[n] = 7; // lock (X, Y, Z) on top datum cap (e.g. spool_collar rim at z = 33 mm)
      } else if (Math.abs(z - bzLo) <= datumTol) {
        lockAxis[n] |= 4; // lock Z on bottom datum plane (z = 0 mm) while allowing corner (X, Y) alignment
      }
    }
    for (let f = 0; f < nFacets; f++) {
      const n0 = lvl.surfaceFacets[6 * f];
      const n1 = lvl.surfaceFacets[6 * f + 1];
      const n2 = lvl.surfaceFacets[6 * f + 2];
      const ax = lvl.nodes[3 * n1] - lvl.nodes[3 * n0];
      const ay = lvl.nodes[3 * n1 + 1] - lvl.nodes[3 * n0 + 1];
      const az = lvl.nodes[3 * n1 + 2] - lvl.nodes[3 * n0 + 2];
      const bx = lvl.nodes[3 * n2] - lvl.nodes[3 * n0];
      const by = lvl.nodes[3 * n2 + 1] - lvl.nodes[3 * n0 + 1];
      const bz = lvl.nodes[3 * n2 + 2] - lvl.nodes[3 * n0 + 2];
      const nx = ay * bz - az * by;
      const ny = az * bx - ax * bz;
      const nz = ax * by - ay * bx;
      const len = Math.hypot(nx, ny, nz);
      if (len <= 1e-12) continue;
      let mask = 0;
      if (Math.abs(nx) > 0.995 * len) mask |= 1;
      if (Math.abs(ny) > 0.995 * len) mask |= 2;
      if (Math.abs(nz) > 0.995 * len) mask |= 4;
      if (mask !== 0) {
        lockAxis[n0] |= mask;
        lockAxis[n1] |= mask;
        lockAxis[n2] |= mask;
      }
    }

    const tmpNodes = new Float32Array(visNodes.length);
    for (let pass = 0; pass < 2; pass++) {
      tmpNodes.set(visNodes);
      for (let n = 0; n < nNodes; n++) {
        const lk = lockAxis[n];
        if (lk === 7 || nodeMinDot[n] >= 0.992) continue;
        const nbrs = cornerAdj[n];
        if (nbrs.length < 2) continue;
        let sx = 0, sy = 0, sz = 0, sw = 0;
        const nx = nodeMeanNorm[3 * n];
        const ny = nodeMeanNorm[3 * n + 1];
        const nz = nodeMeanNorm[3 * n + 2];
        for (let k = 0; k < nbrs.length; k++) {
          const nb = nbrs[k];
          const nDot =
            nx * nodeMeanNorm[3 * nb] +
            ny * nodeMeanNorm[3 * nb + 1] +
            nz * nodeMeanNorm[3 * nb + 2];
          if (nDot < 0.55) continue;
          const w = 0.5 + 1.5 * nDot;
          sx += w * tmpNodes[3 * nb];
          sy += w * tmpNodes[3 * nb + 1];
          sz += w * tmpNodes[3 * nb + 2];
          sw += w;
        }
        if (sw > 1e-12) {
          const alpha = 0.32;
          let dlx = alpha * (sx / sw - tmpNodes[3 * n]);
          let dly = alpha * (sy / sw - tmpNodes[3 * n + 1]);
          let dlz = alpha * (sz / sw - tmpNodes[3 * n + 2]);
          const shrinkDot = dlx * nx + dly * ny + dlz * nz;
          if (shrinkDot < 0) {
            dlx -= shrinkDot * nx;
            dly -= shrinkDot * ny;
            dlz -= shrinkDot * nz;
          }
          if ((lk & 1) === 0) visNodes[3 * n] = tmpNodes[3 * n] + dlx;
          if ((lk & 2) === 0) visNodes[3 * n + 1] = tmpNodes[3 * n + 1] + dly;
          if ((lk & 4) === 0) visNodes[3 * n + 2] = tmpNodes[3 * n + 2] + dlz;
        }
      }
    }
    // Update P2 mid-side nodes along faired edges while preserving Rust's exact in-plane rim arch
    for (let e = 0; e < edgeA.length; e++) {
      const na = edgeA[e];
      const nb = edgeB[e];
      const nm = edgeMid[e];
      for (let c = 0; c < 3; c++) {
        const arch = defNodes[3 * nm + c] - 0.5 * (defNodes[3 * na + c] + defNodes[3 * nb + c]);
        visNodes[3 * nm + c] = 0.5 * (visNodes[3 * na + c] + visNodes[3 * nb + c]) + arch;
      }
    }
  }

  const nodeNorm = new Float32Array(nNodes);
  if (useP2 && hasScalarField) {
    if (mode === 'deflection') {
      for (let n = 0; n < nNodes; n++) {
        const dMag = Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]) * loadRamp;
        nodeNorm[n] = Math.max(0, Math.min(1, (dMag - rngLo) / denom));
      }
    } else if (mode === 'temperature' && tField) {
      for (let n = 0; n < nNodes; n++) {
        nodeNorm[n] = Math.max(0, Math.min(1, ((tField[n] - rngLo) * loadRamp) / denom));
      }
    } else if (mode === 'heat_flux' && hfData) {
      for (let n = 0; n < nNodes; n++) {
        nodeNorm[n] = Math.max(0, Math.min(1, ((hfData.magNode[n] * loadRamp) - rngLo) / denom));
      }
    } else if (mode === 'flow_field' && flData) {
      for (let n = 0; n < nNodes; n++) {
        nodeNorm[n] = Math.max(0, Math.min(1, ((flData.speedNode[n] * loadRamp) - rngLo) / denom));
      }
    } else {
      // Element-based fields (stress, peel, safety):
      // Exclude CLASS_VOID (lvl.class[owner] === 2) ersatz-void background elements (E = 1e-6 E_s)
      // so level-set evolved models (like T12 erode=3.0) never checkerboard between 0 MPa void tets
      // and adjacent structural skin/lattice tets.
      const clsArr = lvl.class;
      let hasNonVoidSurf = false;
      if (clsArr) {
        for (let f = 0; f < nFacets; f++) {
          if (clsArr[lvl.surfaceOwner[f]] !== 2) {
            hasNonVoidSurf = true;
            break;
          }
        }
      }
      const nodeSum = new Float64Array(nNodes);
      const nodeW = new Float64Array(nNodes);
      let rawMin = Infinity;
      let rawMax = -Infinity;
      for (let f = 0; f < nFacets; f++) {
        const owner = lvl.surfaceOwner[f];
        const isVoid = hasNonVoidSurf && clsArr !== null && clsArr[owner] === 2;
        if (isVoid) continue;
        const v = rawFacetVal[f];
        if (v < rawMin) rawMin = v;
        if (v > rawMax) rawMax = v;
        const n0 = lvl.surfaceFacets[6 * f];
        const n1 = lvl.surfaceFacets[6 * f + 1];
        const n2 = lvl.surfaceFacets[6 * f + 2];
        const ax = lvl.nodes[3 * n1] - lvl.nodes[3 * n0];
        const ay = lvl.nodes[3 * n1 + 1] - lvl.nodes[3 * n0 + 1];
        const az = lvl.nodes[3 * n1 + 2] - lvl.nodes[3 * n0 + 2];
        const bx = lvl.nodes[3 * n2] - lvl.nodes[3 * n0];
        const by = lvl.nodes[3 * n2 + 1] - lvl.nodes[3 * n0 + 1];
        const bz = lvl.nodes[3 * n2 + 2] - lvl.nodes[3 * n0 + 2];
        const w = Math.max(
          1e-12,
          Math.hypot(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx),
        );
        for (let j = 0; j < 6; j++) {
          const nj = lvl.surfaceFacets[6 * f + j];
          nodeSum[nj] += w * v;
          nodeW[nj] += w;
        }
      }
      if (!Number.isFinite(rawMin)) rawMin = 0;
      if (!Number.isFinite(rawMax)) rawMax = vmax;

      const nodeVal = new Float64Array(nNodes);
      const populated = new Uint8Array(nNodes);
      for (let n = 0; n < nNodes; n++) {
        if (nodeW[n] > 0) {
          nodeVal[n] = nodeSum[n] / nodeW[n];
          populated[n] = 1;
        }
      }
      // Harmonic infill across surface corner edges for any nodes whose surface facets were all CLASS_VOID
      for (let pass = 0; pass < 8; pass++) {
        let changed = false;
        for (let n = 0; n < nNodes; n++) {
          if (populated[n]) continue;
          const nbrs = cornerAdj[n];
          let s = 0, c = 0;
          for (let k = 0; k < nbrs.length; k++) {
            const nb = nbrs[k];
            if (populated[nb]) {
              s += nodeVal[nb];
              c++;
            }
          }
          if (c > 0) {
            nodeVal[n] = s / c;
            populated[n] = 2;
            changed = true;
          }
        }
        if (changed) {
          for (let n = 0; n < nNodes; n++) {
            if (populated[n] === 2) populated[n] = 1;
          }
        } else {
          break;
        }
      }
      // 2 passes of gentle Laplacian scalar fairing across surface corner edges to eliminate BCC grid striping
      const tmpVal = new Float64Array(nNodes);
      for (let pass = 0; pass < 2; pass++) {
        tmpVal.set(nodeVal);
        for (let n = 0; n < nNodes; n++) {
          const nbrs = cornerAdj[n];
          if (nbrs.length === 0) continue;
          let s = 0;
          for (let k = 0; k < nbrs.length; k++) s += tmpVal[nbrs[k]];
          nodeVal[n] = 0.65 * tmpVal[n] + 0.35 * (s / nbrs.length);
        }
      }
      // Blend quadratic mid-edge nodes (n01, n12, n20) with their corner endpoints for C⁰ smoothness
      for (let f = 0; f < nFacets; f++) {
        const n0 = lvl.surfaceFacets[6 * f];
        const n1 = lvl.surfaceFacets[6 * f + 1];
        const n2 = lvl.surfaceFacets[6 * f + 2];
        const n01 = lvl.surfaceFacets[6 * f + 3];
        const n12 = lvl.surfaceFacets[6 * f + 4];
        const n20 = lvl.surfaceFacets[6 * f + 5];
        nodeVal[n01] = populated[n01]
          ? 0.35 * nodeVal[n01] + 0.325 * (nodeVal[n0] + nodeVal[n1])
          : 0.5 * (nodeVal[n0] + nodeVal[n1]);
        nodeVal[n12] = populated[n12]
          ? 0.35 * nodeVal[n12] + 0.325 * (nodeVal[n1] + nodeVal[n2])
          : 0.5 * (nodeVal[n1] + nodeVal[n2]);
        nodeVal[n20] = populated[n20]
          ? 0.35 * nodeVal[n20] + 0.325 * (nodeVal[n2] + nodeVal[n0])
          : 0.5 * (nodeVal[n2] + nodeVal[n0]);
      }
      // Restore peak surface extrema so patch averaging never washes out stress hot-spots
      let recMin = Infinity;
      let recMax = -Infinity;
      for (let n = 0; n < nNodes; n++) {
        if (!populated[n] && nodeW[n] <= 0) continue;
        if (nodeVal[n] < recMin) recMin = nodeVal[n];
        if (nodeVal[n] > recMax) recMax = nodeVal[n];
      }
      const recSpan = recMax - recMin;
      const rawSpan = rawMax - rawMin;
      for (let n = 0; n < nNodes; n++) {
        const vRestored =
          recSpan > 1e-12 && rawSpan > 1e-12
            ? rawMin + ((nodeVal[n] - recMin) / recSpan) * rawSpan
            : nodeVal[n];
        if (mode === 'safety') {
          const sf = vRestored > 0 ? yieldMpa / vRestored : Infinity;
          const t = sf <= 1
            ? 0.5 + 0.5 * Math.max(0, 1 - sf)
            : 0.5 - 0.5 * Math.min(1, (sf - 1) / Math.max(1, vmax - 1));
          nodeNorm[n] = Math.max(0, Math.min(1, t));
        } else {
          nodeNorm[n] = Math.max(0, Math.min(1, (vRestored - rngLo) / denom));
        }
      }
    }
  }

  // Compute faired facet normals and smooth node normals for 3D relief shading
  const facetNormals = new Float32Array(nFacets * 3);
  const smoothNodeNormals = new Float32Array(nNodes * 3);
  for (let f = 0; f < nFacets; f++) {
    const n0 = lvl.surfaceFacets[6 * f];
    const n1 = lvl.surfaceFacets[6 * f + 1];
    const n2 = lvl.surfaceFacets[6 * f + 2];
    const ax = visNodes[3 * n1] - visNodes[3 * n0];
    const ay = visNodes[3 * n1 + 1] - visNodes[3 * n0 + 1];
    const az = visNodes[3 * n1 + 2] - visNodes[3 * n0 + 2];
    const bx = visNodes[3 * n2] - visNodes[3 * n0];
    const by = visNodes[3 * n2 + 1] - visNodes[3 * n0 + 1];
    const bz = visNodes[3 * n2 + 2] - visNodes[3 * n0 + 2];
    let nx = ay * bz - az * by;
    let ny = az * bx - ax * bz;
    let nz = ax * by - ay * bx;
    const len = Math.hypot(nx, ny, nz);
    if (len > 1e-12) {
      nx /= len;
      ny /= len;
      nz /= len;
    } else {
      nz = 1;
    }
    facetNormals[3 * f] = nx;
    facetNormals[3 * f + 1] = ny;
    facetNormals[3 * f + 2] = nz;
    for (let j = 0; j < 6; j++) {
      const nj = lvl.surfaceFacets[6 * f + j];
      smoothNodeNormals[3 * nj] += nx * len;
      smoothNodeNormals[3 * nj + 1] += ny * len;
      smoothNodeNormals[3 * nj + 2] += nz * len;
    }
  }
  for (let n = 0; n < nNodes; n++) {
    const i3 = 3 * n;
    const len = Math.hypot(smoothNodeNormals[i3], smoothNodeNormals[i3 + 1], smoothNodeNormals[i3 + 2]);
    if (len > 1e-12) {
      smoothNodeNormals[i3] /= len;
      smoothNodeNormals[i3 + 1] /= len;
      smoothNodeNormals[i3 + 2] /= len;
    } else {
      smoothNodeNormals[i3 + 2] = 1;
    }
  }
  // Smooth node normals along curved/bevel edges so BCC diagonals never cause alternating shade stripes
  if (useP2 && edgeA.length > 0) {
    const tmpNorm = new Float32Array(smoothNodeNormals.length);
    for (let pass = 0; pass < 2; pass++) {
      tmpNorm.set(smoothNodeNormals);
      for (let n = 0; n < nNodes; n++) {
        if (nodeMinDot[n] >= 0.992) continue;
        const nbrs = cornerAdj[n];
        if (nbrs.length === 0) continue;
        let sx = tmpNorm[3 * n] * 1.5;
        let sy = tmpNorm[3 * n + 1] * 1.5;
        let sz = tmpNorm[3 * n + 2] * 1.5;
        for (let k = 0; k < nbrs.length; k++) {
          const nb = nbrs[k];
          sx += tmpNorm[3 * nb];
          sy += tmpNorm[3 * nb + 1];
          sz += tmpNorm[3 * nb + 2];
        }
        const len = Math.hypot(sx, sy, sz);
        if (len > 1e-12) {
          smoothNodeNormals[3 * n] = sx / len;
          smoothNodeNormals[3 * n + 1] = sy / len;
          smoothNodeNormals[3 * n + 2] = sz / len;
        }
      }
    }
    for (let e = 0; e < edgeA.length; e++) {
      const na = edgeA[e];
      const nb = edgeB[e];
      const nm = edgeMid[e];
      const sx = smoothNodeNormals[3 * na] + smoothNodeNormals[3 * nb];
      const sy = smoothNodeNormals[3 * na + 1] + smoothNodeNormals[3 * nb + 1];
      const sz = smoothNodeNormals[3 * na + 2] + smoothNodeNormals[3 * nb + 2];
      const len = Math.hypot(sx, sy, sz);
      if (len > 1e-12) {
        smoothNodeNormals[3 * nm] = sx / len;
        smoothNodeNormals[3 * nm + 1] = sy / len;
        smoothNodeNormals[3 * nm + 2] = sz / len;
      }
    }
  }

  // Helper: ParaView-style studio directional relief shading from smooth node normal
  const reliefShade = (f: number, n: number): number => {
    if (ui.resultsVolumeOn) return 1.0;
    const fnx = facetNormals[3 * f];
    const fny = facetNormals[3 * f + 1];
    const fnz = facetNormals[3 * f + 2];
    let nx = smoothNodeNormals[3 * n];
    let ny = smoothNodeNormals[3 * n + 1];
    let nz = smoothNodeNormals[3 * n + 2];
    // Only fall back to flat facet normal in P0 mode or across sharp >75° un-filleted corners
    if (!useP2 || nx * fnx + ny * fny + nz * fnz < 0.25) {
      nx = fnx;
      ny = fny;
      nz = fnz;
    }
    const d1 = Math.max(0, nx * 0.42 - ny * 0.58 + nz * 0.70);
    const d2 = Math.max(0, -nx * 0.55 + ny * 0.48 + nz * 0.68);
    const spec = Math.pow(d1, 14) * 0.06;
    return Math.min(1.04, 0.78 + 0.18 * d1 + 0.09 * d2 + spec);
  };

  const geo = new THREE.BufferGeometry();
  if (useP2) {
    // 4 sub-triangles per 6-node quadratic P2 facet:
    // (n0, n01, n20), (n01, n1, n12), (n20, n12, n2), (n01, n12, n20)
    resultSubTrisPerFacet = 4;
    const p2Pos = new Float32Array(nFacets * 36);
    const p2Col = new Float32Array(nFacets * 36);
    const p2Uv = new Float32Array(nFacets * 24);
    const subTriLocal = [
      0, 3, 5,
      3, 1, 4,
      5, 4, 2,
      3, 4, 5,
    ];
    for (let f = 0; f < nFacets; f++) {
      const fBase = 6 * f;
      for (let k = 0; k < 12; k++) {
        const n = lvl.surfaceFacets[fBase + subTriLocal[k]];
        const vIdx = 12 * f + k;
        const o3 = 3 * vIdx;
        const o2 = 2 * vIdx;
        p2Pos[o3] = visNodes[3 * n];
        p2Pos[o3 + 1] = visNodes[3 * n + 1];
        p2Pos[o3 + 2] = visNodes[3 * n + 2];
        const s = hasScalarField ? nodeNorm[n] : 0;
        p2Uv[o2] = s;
        p2Uv[o2 + 1] = 0.5;
        const shade = reliefShade(f, n);
        if (hasScalarField) {
          // When 1D LUT texture is bound, vColor carries the achromatic 3D relief shading multiplier
          p2Col[o3] = shade;
          p2Col[o3 + 1] = shade;
          p2Col[o3 + 2] = shade;
        } else {
          p2Col[o3] = 0.55 * shade;
          p2Col[o3 + 1] = 0.55 * shade;
          p2Col[o3 + 2] = 0.55 * shade;
        }
      }
    }
    geo.setAttribute('position', new THREE.BufferAttribute(p2Pos, 3));
    geo.setAttribute('color', new THREE.BufferAttribute(p2Col, 3));
    geo.setAttribute('uv', new THREE.BufferAttribute(p2Uv, 2));
  } else {
    resultSubTrisPerFacet = 1;
    const p0Col = new Float32Array(nFacets * 9);
    const p0Uv = new Float32Array(nFacets * 6);
    for (let f = 0; f < nFacets; f++) {
      const s = facetScalars[f];
      for (let j = 0; j < 3; j++) {
        const n = lvl.surfaceFacets[6 * f + j];
        const vIdx = 3 * f + j;
        const shade = reliefShade(f, n);
        p0Uv[2 * vIdx] = s;
        p0Uv[2 * vIdx + 1] = 0.5;
        if (hasScalarField) {
          p0Col[3 * vIdx] = shade;
          p0Col[3 * vIdx + 1] = shade;
          p0Col[3 * vIdx + 2] = shade;
        } else {
          p0Col[3 * vIdx] = 0.55 * shade;
          p0Col[3 * vIdx + 1] = 0.55 * shade;
          p0Col[3 * vIdx + 2] = 0.55 * shade;
        }
      }
    }
    geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geo.setAttribute('color', new THREE.BufferAttribute(p0Col, 3));
    geo.setAttribute('uv', new THREE.BufferAttribute(p0Uv, 2));
  }

  const lutRamp = mode === 'safety' ? RAMP_SAFETY : ramp;
  const lutTex = hasScalarField ? buildColormapTexture(lutRamp) : null;
  const mesh = new THREE.Mesh(
    geo,
    new THREE.MeshBasicMaterial({
      map: lutTex,
      vertexColors: true,
      side: THREE.DoubleSide,
      transparent: ui.resultsVolumeOn,
      opacity: ui.resultsVolumeOn ? 0.34 : 1.0,
      depthWrite: !ui.resultsVolumeOn,
    }),
  );
  mesh.renderOrder = 1;
  resultGroup.add(mesh);
  resultMesh = mesh;

  // Optional P2 curved element wireframe overlay ('p2_wire')
  if (ui.resultsInterp === 'p2_wire' && nFacets > 0) {
    const wirePos = new Float32Array(nFacets * 36);
    const edgeLoop = [0, 3, 3, 1, 1, 4, 4, 2, 2, 5, 5, 0];
    for (let f = 0; f < nFacets; f++) {
      const fBase = 6 * f;
      for (let e = 0; e < 12; e++) {
        const n = lvl.surfaceFacets[fBase + edgeLoop[e]];
        const o = 36 * f + 3 * e;
        wirePos[o] = visNodes[3 * n];
        wirePos[o + 1] = visNodes[3 * n + 1];
        wirePos[o + 2] = visNodes[3 * n + 2];
      }
    }
    const wGeo = new THREE.BufferGeometry();
    wGeo.setAttribute('position', new THREE.BufferAttribute(wirePos, 3));
    const wLines = new THREE.LineSegments(
      wGeo,
      new THREE.LineBasicMaterial({
        color: 0x0f172a,
        transparent: true,
        opacity: 0.26,
        depthWrite: false,
      }),
    );
    wLines.renderOrder = 2;
    resultGroup.add(wLines);
  }

  // True 3D Interior Tetrahedral Iso-Shells (`ui.resultsVolumeOn`) and True 3D Tetrahedral Section Cap (`vp.clipPlanes()`):
  const tets = lvl.tets;
  const nTets = tets ? Math.floor(tets.length / 4) : 0;

  if (ui.resultsVolumeOn && hasScalarField) {
    const thresholds = [0.30, 0.50, 0.68, 0.84];
    if (tets && nTets > 0) {
      // True 3D tetrahedral iso-volume rendering at physical interior coordinates
      const tetFaces = [
        0, 2, 1,
        0, 1, 3,
        1, 2, 3,
        2, 0, 3,
      ];
      const stride = Math.max(1, Math.floor(nTets / 3500));
      for (let sIdx = 0; sIdx < thresholds.length; sIdx++) {
        const thr = thresholds[sIdx];
        const hotPos: number[] = [];
        const hotCol: number[] = [];
        for (let e = 0; e < nTets; e += stride) {
          const ev = elementScalarValue(lvl, mode, ci, e, u, loadRamp).value;
          const eNorm =
            mode === 'safety'
              ? ev <= 1
                ? 0.5 + 0.5 * Math.max(0, 1 - ev)
                : 0.5 - 0.5 * Math.min(1, (ev - 1) / Math.max(1, vmax - 1))
              : (ev - rngLo) / denom;
          if (!(eNorm >= thr)) continue;
          const w = Math.max(0, Math.min(1, (eNorm - thr) / Math.max(0.08, 1 - thr)));
          const feather = w * w * (3 - 2 * w);
          rampColor(Math.min(1, Math.max(thr, eNorm)), col, lutRamp);
          const tBase = 4 * e;
          for (let tf = 0; tf < 4; tf++) {
            for (let k = 0; k < 3; k++) {
              const nj = tets[tBase + tetFaces[3 * tf + k]];
              hotPos.push(visNodes[3 * nj], visNodes[3 * nj + 1], visNodes[3 * nj + 2]);
              hotCol.push(col[0] * feather, col[1] * feather, col[2] * feather);
            }
          }
        }
        if (hotPos.length > 0) {
          const sGeo = new THREE.BufferGeometry();
          sGeo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(hotPos), 3));
          sGeo.setAttribute('color', new THREE.BufferAttribute(new Float32Array(hotCol), 3));
          const sMesh = new THREE.Mesh(
            sGeo,
            new THREE.MeshBasicMaterial({
              vertexColors: true,
              side: THREE.DoubleSide,
              transparent: true,
              opacity: 0.16 + 0.05 * sIdx,
              blending: THREE.AdditiveBlending,
              depthWrite: false,
            }),
          );
          sMesh.renderOrder = 3 + sIdx;
          resultGroup.add(sMesh);
        }
      }
    }
  }

  // True 3D Interior Cross-Section Plane Slicing across `lvl.tets` when section clipping is active:
  sectionCapMesh = null;
  sectionCapElemOwners = null;
  const activeClipPlane = resultHost?.clipPlanes()?.[0] ?? null;
  if (activeClipPlane && tets && nTets > 0) {
    const pnx = activeClipPlane.normal.x;
    const pny = activeClipPlane.normal.y;
    const pnz = activeClipPlane.normal.z;
    const pConst = activeClipPlane.constant;
    const capPos: number[] = [];
    const capCol: number[] = [];
    const capOwners: number[] = [];
    const tetEdges = [
      0, 1,
      0, 2,
      0, 3,
      1, 2,
      1, 3,
      2, 3,
    ];
    const sd = [0, 0, 0, 0];
    const pts: [number, number, number][] = [];

    for (let e = 0; e < nTets; e++) {
      const tBase = 4 * e;
      let nNeg = 0;
      let nPos = 0;
      for (let k = 0; k < 4; k++) {
        const nk = tets[tBase + k];
        const d =
          pnx * visNodes[3 * nk] +
          pny * visNodes[3 * nk + 1] +
          pnz * visNodes[3 * nk + 2] +
          pConst;
        sd[k] = d;
        if (d < 0) nNeg++;
        else nPos++;
      }
      if (nNeg === 0 || nPos === 0) continue;
      pts.length = 0;
      for (let ed = 0; ed < 6; ed++) {
        const a = tetEdges[2 * ed];
        const b = tetEdges[2 * ed + 1];
        const da = sd[a];
        const db = sd[b];
        if ((da < 0 && db >= 0) || (da >= 0 && db < 0)) {
          const t = da / (da - db);
          const na = tets[tBase + a];
          const nb = tets[tBase + b];
          pts.push([
            visNodes[3 * na] + t * (visNodes[3 * nb] - visNodes[3 * na]),
            visNodes[3 * na + 1] + t * (visNodes[3 * nb + 1] - visNodes[3 * na + 1]),
            visNodes[3 * na + 2] + t * (visNodes[3 * nb + 2] - visNodes[3 * na + 2]),
          ]);
        }
      }
      if (pts.length < 3) continue;
      const ev = elementScalarValue(lvl, mode, ci, e, u, loadRamp).value;
      const eNorm =
        mode === 'safety'
          ? ev <= 1
            ? 0.5 + 0.5 * Math.max(0, 1 - ev)
            : 0.5 - 0.5 * Math.min(1, (ev - 1) / Math.max(1, vmax - 1))
          : (ev - rngLo) / denom;
      rampColor(Math.min(1, Math.max(0, eNorm)), col, lutRamp);

      if (pts.length === 4) {
        // Order quad vertices around centroid on the clip plane so both triangles are non-bowtie
        const cx = 0.25 * (pts[0][0] + pts[1][0] + pts[2][0] + pts[3][0]);
        const cy = 0.25 * (pts[0][1] + pts[1][1] + pts[2][1] + pts[3][1]);
        const cz = 0.25 * (pts[0][2] + pts[1][2] + pts[2][2] + pts[3][2]);
        const ux = pts[0][0] - cx, uy = pts[0][1] - cy, uz = pts[0][2] - cz;
        const vx = pny * uz - pnz * uy, vy = pnz * ux - pnx * uz, vz = pnx * uy - pny * ux;
        pts.sort((pA, pB) => {
          const angA = Math.atan2(
            (pA[0] - cx) * vx + (pA[1] - cy) * vy + (pA[2] - cz) * vz,
            (pA[0] - cx) * ux + (pA[1] - cy) * uy + (pA[2] - cz) * uz,
          );
          const angB = Math.atan2(
            (pB[0] - cx) * vx + (pB[1] - cy) * vy + (pB[2] - cz) * vz,
            (pB[0] - cx) * ux + (pB[1] - cy) * uy + (pB[2] - cz) * uz,
          );
          return angA - angB;
        });
      }

      const emitTri = (p0: [number, number, number], p1: [number, number, number], p2: [number, number, number]) => {
        capPos.push(p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], p2[0], p2[1], p2[2]);
        capCol.push(col[0], col[1], col[2], col[0], col[1], col[2], col[0], col[1], col[2]);
        capOwners.push(e);
      };
      emitTri(pts[0], pts[1], pts[2]);
      if (pts.length === 4) {
        emitTri(pts[0], pts[2], pts[3]);
      }
    }

    if (capPos.length > 0) {
      const capGeo = new THREE.BufferGeometry();
      capGeo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(capPos), 3));
      capGeo.setAttribute('color', new THREE.BufferAttribute(new Float32Array(capCol), 3));
      const capMat = new THREE.MeshBasicMaterial({
        vertexColors: true,
        side: THREE.DoubleSide,
      });
      const cMesh = new THREE.Mesh(capGeo, capMat);
      cMesh.userData.isSectionCap = true;
      cMesh.renderOrder = 2;
      resultGroup.add(cMesh);
      sectionCapMesh = cMesh;
      sectionCapElemOwners = new Uint32Array(capOwners);
    }
  }

  if (mode === 'vectors') {
    const diag = modelDiag(lvl) || 40;
    const stride = Math.max(1, Math.floor(nFacets / 180));
    const disc = activeResultsDiscipline(lvl);
    const useThermalVec = disc === 'thermal' && !!hfData && hfData.maxFlux > 1e-6;
    const useFluidVec = disc === 'fluid' && !!flData && flData.maxSpeed > 1e-6;
    let maxDispU = 1e-9;
    for (let i = 0; i < u.length; i += 3) {
      const m = Math.hypot(u[i], u[i + 1], u[i + 2]);
      if (m > maxDispU) maxDispU = m;
    }

    for (let f = 0; f < nFacets; f += stride) {
      let vx = 0, vy = 0, vz = 0;
      let px = 0, py = 0, pz = 0;
      for (let j = 0; j < 3; j++) {
        const n = lvl.surfaceFacets[6 * f + j];
        if (useThermalVec && hfData) {
          vx += hfData.vecNode[3 * n];
          vy += hfData.vecNode[3 * n + 1];
          vz += hfData.vecNode[3 * n + 2];
        } else if (useFluidVec && flData) {
          vx += flData.vecNode[3 * n];
          vy += flData.vecNode[3 * n + 1];
          vz += flData.vecNode[3 * n + 2];
        } else {
          vx += u[3 * n];
          vy += u[3 * n + 1];
          vz += u[3 * n + 2];
        }
        px += lvl.nodes[3 * n] + scale * u[3 * n];
        py += lvl.nodes[3 * n + 1] + scale * u[3 * n + 1];
        pz += lvl.nodes[3 * n + 2] + scale * u[3 * n + 2];
      }
      vx /= 3; vy /= 3; vz /= 3;
      const mag = Math.hypot(vx, vy, vz);
      const refMax = useThermalVec && hfData ? hfData.maxFlux : useFluidVec && flData ? flData.maxSpeed : maxDispU;
      if (mag <= 1e-9 || mag < refMax * 0.02) continue;
      const normMag = mag / Math.max(1e-9, refMax);
      const len = useThermalVec || useFluidVec
        ? Math.min(0.14 * diag, Math.max(0.6, 0.11 * diag * normMag))
        : Math.min(0.15 * diag, Math.max(0.6, mag * scale * 0.25, 0.09 * diag * normMag));
      const arrowColor = useThermalVec ? 0xf97316 : useFluidVec ? 0x38bdf8 : 0xffffff;
      const arrow = new THREE.ArrowHelper(
        new THREE.Vector3(vx / mag, vy / mag, vz / mag),
        new THREE.Vector3(px / 3, py / 3, pz / 3),
        len,
        arrowColor,
        Math.min(1.5, len * 0.3),
        Math.min(0.9, len * 0.18),
      );
      arrow.renderOrder = 2;
      resultGroup.add(arrow);
    }
  }
}

// ---- play ticker ----------------------------------------------------------------------

/** One full mode cycle in ms (2π per CYCLE_MS); a cycle per ~1.2 s reads the deformation
 * without strobing at coarse frame rates. */
const CYCLE_MS = 1200;
const RAMP_CYCLE_MS = 2200;
let playRaf = 0;
let playLast = 0;

function isResultsViewportActive(): boolean {
  return store.lens === 'results';
}

function playTick(t: number) {
  const owner = analysis.latest;
  const modeAnim = ui.resultsModePlay && owner && animatingShape(owner) !== null;
  const rampAnim = ui.resultsRampPlay && owner && animatingShape(owner) === null;
  if (!isResultsViewportActive() || (!modeAnim && !rampAnim)) {
    playRaf = 0;
    playLast = 0;
    return;
  }
  const dt = playLast ? t - playLast : 0;
  playLast = t;
  playRaf = requestAnimationFrame(playTick);
  if (modeAnim) {
    ui.set('resultsModePhase', (ui.resultsModePhase + (dt / CYCLE_MS) * 2 * Math.PI) % (2 * Math.PI));
  } else if (rampAnim) {
    const nextRamp = ui.resultsRamp + dt / RAMP_CYCLE_MS;
    ui.set('resultsRamp', nextRamp > 1.02 ? 0 : Math.min(1, Math.round(nextRamp * 1000) / 1000));
  }
}

/** Start or stop the play ticker as the lens state asks. */
function ensureTicker(lvl: LevelResult | null) {
  const activeVp = isResultsViewportActive();
  const modeWant = activeVp && lvl && ui.resultsModePlay && animatingShape(lvl) !== null;
  const rampWant = activeVp && lvl && ui.resultsRampPlay && animatingShape(lvl) === null;
  const want = modeWant || rampWant;
  if (want && !playRaf) {
    playLast = 0;
    playRaf = requestAnimationFrame(playTick);
  } else if (!want && playRaf) {
    cancelAnimationFrame(playRaf);
    playRaf = 0;
  }
}

function syncResults(vp: Viewport) {
  if (!resultHost) {
    resultHost = vp;
    vp.overlays.add(resultGroup);
  }
  // e2e accessors for the rebuilt geometry. Attached lazily: main.tsx assigns
  // window.fieldsmith after this module's side-effect import, so a module-top attach
  // would capture an undefined hook (the same order ViewportHost works around).
  const w = window as unknown as { fieldsmith?: { results?: { positions(): Float32Array | null; colors(): Float32Array | null } } };
  if (w.fieldsmith && !w.fieldsmith.results) {
    w.fieldsmith.results = {
      positions: () => (resultMesh ? lastFacetPositions : null),
      colors: () => (resultMesh ? lastFacetColors : null),
    };
  }
  const lvl = analysis.latest;
  if (lvl) {
    autoSyncResultsDiscipline(lvl);
  }
  const on = isResultsViewportActive() && !!lvl;
  const nextShow = !on;
  if (vp.showField !== nextShow) {
    vp.showField = nextShow;
    vp.invalidate();
  }
  // A stale animation knob (a new solve carried fewer or no shapes) must not animate on
  // the wrong mesh: reset before the sig, and let this pass's emit re-run sync.
  if (ui.resultsModeShape >= 0 && (!lvl || !shapeOf(lvl, ui.resultsModeShape))) {
    ui.set('resultsModeShape', -1);
    ui.set('resultsModePlay', false);
    ui.set('resultsModePhase', 0);
    return;
  }
  ensureTicker(lvl);
  const planes = vp.clipPlanes();
  const clipPlane0 = planes?.[0] ?? null;
  const clipSig = clipPlane0
    ? `${clipPlane0.normal.x.toFixed(3)},${clipPlane0.normal.y.toFixed(3)},${clipPlane0.normal.z.toFixed(3)},${clipPlane0.constant.toFixed(2)}`
    : 'noclip';
  const sig = on
    ? `${lvl && lvl.label}|${lvl && lvl.nodes.length}${lvl && lvl.meta.provenance.total_ms}|${ui.resultsMode}|${ui.resultsColormap}|${ui.resultsInterp}|${ui.resultsVolumeOn ? 'vol' : 'surf'}|${ui.resultsCase}|${ui.resultsScale}|${ui.resultsRamp}|${
        ui.resultsRange ? `${ui.resultsRange.mode}/${ui.resultsRange.levelKey}/${ui.resultsRange.lo}/${ui.resultsRange.hi}` : '-'
      }|${ui.resultsModeShape}|${ui.resultsModePhase}|${ui.resultsModePlay ? 'p' : 's'}|${ui.resultsRampPlay ? 'rp' : 'rs'}|${clipSig}`
    : 'off';
  if (sig !== resultSig) {
    resultSig = sig;
    while (resultGroup.children.length) {
      const c = resultGroup.children.pop()!;
      disposeDeep(c);
    }
    resultMesh = null;
    sectionCapMesh = null;
    sectionCapElemOwners = null;
    lastFacetPositions = null;
    lastFacetColors = null;
    if (on && lvl) {
      try {
        rebuild(lvl);
        if (ui.renderError?.kind === 'results') ui.set('renderError', null);
      } catch (err) {
        while (resultGroup.children.length) {
          const c = resultGroup.children.pop()!;
          disposeDeep(c);
        }
        // Loud rendering failure: the analysed mesh is the whole lens — a build that threw
        // must say so in the viewport, not only in the console (VISION §7).
        console.error('results mesh build failed', err);
        ui.set('renderError', { kind: 'results', text: `the analysed mesh could not be built: ${String(err)}` });
      }
    }
    vp.invalidate();
  }
  // The section plane reaches the analysed mesh through three's local clipping, not the
  // shader: hand every overlay material (except the interior section cap itself, whose
  // triangles lie on the cut plane) the viewport's persistent plane.
  let clipped = false;
  resultGroup.traverse((c) => {
    if (c.userData?.isSectionCap) return;
    const mat = (c as THREE.Mesh).material as THREE.MeshBasicMaterial | undefined;
    if (mat && mat.clippingPlanes !== planes) {
      mat.clippingPlanes = planes;
      clipped = true;
    }
  });
  if (clipped) vp.invalidate();
}

const raycaster = new THREE.Raycaster();
const ndc = new THREE.Vector2();

function onClickResults(_hit: unknown, e: PointerEvent): boolean {
  if (store.lens !== 'results' || e.button !== 0) return false;
  const vp = resultHost;
  const lvl = analysis.latest;
  if (!vp || !lvl) return false;
  if (!resultMesh) return true;
  const r = vp.canvas.getBoundingClientRect();
  ndc.x = ((e.clientX - r.left) / r.width) * 2 - 1;
  ndc.y = -(((e.clientY - r.top) / r.height) * 2 - 1);
  raycaster.setFromCamera(ndc, vp.camera);

  const targets: THREE.Object3D[] = [resultMesh];
  if (sectionCapMesh) targets.push(sectionCapMesh);
  const hits = raycaster.intersectObjects(targets, false);
  if (hits.length === 0) {
    if (ui.probe) ui.set('probe', null);
    lastProbedElem = -1;
    return true;
  }
  const hit = hits[0];
  const p = hit.point;
  const shown = shownDisp(lvl);
  const activeMode = shown.animating ? 'deflection' : ui.resultsMode;
  const ci = effCase(lvl);

  if (sectionCapMesh && hit.object === sectionCapMesh && sectionCapElemOwners) {
    const capTriIdx = hit.faceIndex ?? -1;
    if (capTriIdx >= 0 && capTriIdx < sectionCapElemOwners.length) {
      const elemIdx = sectionCapElemOwners[capTriIdx];
      lastProbedElem = elemIdx;
      const { value, unit } = elementScalarValue(lvl, activeMode, ci, elemIdx, shown.u);
      const pt: [number, number, number] = [p.x, p.y, p.z];
      probeHistory.push(pt);
      if (probeHistory.length > 2) probeHistory.shift();
      ui.set('probe', { value, unit, point: pt });
      return true;
    }
  }

  const rawFace = hit.faceIndex ?? -1;
  const f = resultSubTrisPerFacet > 1 ? Math.floor(rawFace / resultSubTrisPerFacet) : rawFace;
  if (f < 0 || f >= lvl.surfaceRegion.length) return false;
  lastProbedElem = lvl.surfaceOwner[f];
  const { value, unit } = facetValue(lvl, activeMode, ci, f, shown.u);
  const pt: [number, number, number] = [p.x, p.y, p.z];
  probeHistory.push(pt);
  if (probeHistory.length > 2) probeHistory.shift();
  ui.set('probe', { value, unit, point: pt });
  return true;
}

viewportExtensions.push({
  sync: syncResults,
  click: onClickResults,
});

// ---- badge ---------------------------------------------------------------------------

/** The badge's envelope maxima: max over cases, floored at 1e-9 so relative change stays
 * defined where an unloaded document solves to zero. Same math the CLI's rung records use. */
function envelopes(lvl: LevelResult): { peak: number; defl: number } {
  return {
    peak: Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem)),
    defl: Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement)),
  };
}

/** Envelope-to-envelope change vs the previous level, in percent. An estimate from
 * refinement, not an error bound — the rung table and the badge both label it so. */
function changePct(cur: LevelResult, prev: LevelResult): { peak: number; defl: number } {
  const c = envelopes(cur);
  const p = envelopes(prev);
  return {
    peak: ((c.peak - p.peak) / p.peak) * 100,
    defl: ((c.defl - p.defl) / p.defl) * 100,
  };
}

function formatDisciplineBadgeText(lvl: LevelResult): string {
  const { peak, defl } = envelopes(lvl);
  const ci = effCase(lvl);
  const disc = activeResultsDiscipline(lvl);
  const t = tempField(lvl, ci);
  if (disc === 'thermal' && t && t.length > 0) {
    let lo = Infinity, hi = -Infinity;
    for (const v of t) {
      if (v < lo) lo = v;
      if (v > hi) hi = v;
    }
    const hf = heatFluxField(lvl, ci);
    return peak > 1e-6
      ? `T ${fmt(lo, 0)}…${fmt(hi, 0)} °C · |q| ${fmt(hf?.maxFlux ?? 0, 1)} kW/m² · peak ${fmt(peak)} MPa`
      : `T ${fmt(lo, 1)}…${fmt(hi, 1)} °C · |q| ${fmt(hf?.maxFlux ?? 0, 1)} kW/m²`;
  }
  if (disc === 'fluid') {
    const fl = flowFieldData(lvl, ci);
    return peak > 1e-6
      ? `|v| ${fmt(fl.maxSpeed, 1)} mm/s · τ ${fmt(fl.maxShearPa, 0)} Pa · peak ${fmt(peak)} MPa`
      : `|v| ${fmt(fl.maxSpeed, 1)} mm/s · τ ${fmt(fl.maxShearPa, 0)} Pa · Re ${fmt(fl.reynolds, 2)}`;
  }
  if (disc === 'modal' && lvl.meta.modes?.frequencies?.length) {
    const freqs = lvl.meta.modes.frequencies;
    const k = ui.resultsModeShape >= 0 && ui.resultsModeShape < freqs.length ? ui.resultsModeShape : 0;
    return peak > 1e-6
      ? `mode ${k + 1}: ${fmt(freqs[k], 1)} Hz · peak ${fmt(peak)} MPa · deflect ${fmt(defl)} mm`
      : `mode ${k + 1}: ${fmt(freqs[k], 1)} Hz (${freqs.length} modes)`;
  }
  return `peak ${fmt(peak)} MPa · deflect ${fmt(defl)} mm`;
}

function AnalysisBadge() {
  useApp();
  const s = analysis.state;
  if (s.status === 'idle' || store.doc.root.length === 0) return null;
  if (s.status === 'running') {
    return (
      <>
        <span class="sep">|</span>
        <span class="num" data-testid="badge-running">
          analysing{s.levelLabel ? ` · ${s.levelLabel}` : ''}
        </span>
      </>
    );
  }
  if (s.status === 'error') {
    if (s.errorKind === 'setup') {
      if (store.lens === 'shape') return null;
      return (
        <>
          <span class="sep">|</span>
          <span class="num" data-testid="setup-block" title={s.error ?? ''}>
            {s.error}
          </span>
        </>
      );
    }
    return (
      <>
        <span class="sep">|</span>
        <span class="num bad" data-testid="badge-error" title={s.error ?? ''}>
          analysis failed
        </span>
      </>
    );
  }
  const lvl = s.levels[s.levels.length - 1];
  const prev = s.levels[s.levels.length - 2];
  if (!lvl) return null;
  // In the fill lens the fill badge shows these numbers plus mass — avoid the duplicate.
  if (store.lens === 'fill' && lvl.meta.fill) return null;
  const p = lvl.meta.provenance;
  // Provenance: refinement level, order, counts, timing, and the change since the previous level.
  const lines = [
    lvl.label,
    `${p.elements} elements · order P${p.order} · ${p.dofs} dofs · ${p.iterations} iterations`,
    `mesh ${p.mesh_ms} ms · solve ${p.solve_ms} ms · total ${p.total_ms} ms`,
    `quality min ${p.quality.min_dihedral_deg.toFixed(1)}° dihedral`,
  ];
  if (prev) {
    const ch = changePct(lvl, prev);
    lines.push(`peak ${ch.peak >= 0 ? '+' : ''}${ch.peak.toFixed(1)}% · deflect ${ch.defl >= 0 ? '+' : ''}${ch.defl.toFixed(1)}% vs previous level`);
  }
  const title = lines.join('\n');
  return (
    <>
      <span class="sep">|</span>
      <span class="num" data-testid="analysis-badge" title={title}>
        {formatDisciplineBadgeText(lvl)}
      </span>
    </>
  );
}
statusItems.push(AnalysisBadge);

// ---- sweep & Pareto explorer ---------------------------------------------------------

const PARETO_METRIC_LABELS: Record<ParetoMetric, { label: string; unit: string }> = {
  vm: { label: 'Max von Mises (MPa)', unit: 'MPa' },
  safety: { label: 'Safety factor (×)', unit: '×' },
  compliance: { label: 'Compliance (N·mm)', unit: 'N·mm' },
  peel: { label: 'Layer peel (×)', unit: '×' },
  deflection: { label: 'Max deflection (mm)', unit: 'mm' },
  f1: { label: 'Mode 1 f₁ (Hz)', unit: 'Hz' },
};

function ParetoPlot(props: { points: SweepPointRow[]; selectedIndex: number | null; param: string; param2: string }) {
  const { points, selectedIndex, param, param2 } = props;
  const hasF1 = points.some((p) => p.f1_hz !== null && p.f1_hz > 0);
  const metric: ParetoMetric = ui.paretoMetric === 'f1' && !hasF1 ? 'vm' : ui.paretoMetric;
  const front = computeParetoIndices(points, metric);
  const xs = points.map((p) => p.mass_g);
  const ys = points.map((p) => metricValueOf(p, metric));
  let xMin = Math.min(...xs), xMax = Math.max(...xs);
  let yMin = Math.min(...ys), yMax = Math.max(...ys);
  if (!(xMax > xMin + 1e-9)) { xMin -= 0.5; xMax += 0.5; }
  if (!(yMax > yMin + 1e-9)) { yMin -= 0.5; yMax += 0.5; }
  const W = 340, H = 148;
  const padL = 44, padR = 14, padT = 14, padB = 26;
  const pw = W - padL - padR;
  const ph = H - padT - padB;
  const sx = (x: number) => padL + ((x - xMin) / (xMax - xMin)) * pw;
  const sy = (y: number) => padT + ph - ((y - yMin) / (yMax - yMin)) * ph;

  const frontIndices = [...front].sort((a, b) => xs[a] - xs[b] || ys[a] - ys[b]);
  const polyPoints = frontIndices.map((i) => `${sx(xs[i]).toFixed(1)},${sy(ys[i]).toFixed(1)}`).join(' ');
  const mInfo = PARETO_METRIC_LABELS[metric];

  return (
    <div class="pareto-block" data-testid="pareto-explorer">
      <div class="scale-row" style={{ marginBottom: '4px' }}>
        <span class="plain">Frontier Y</span>
        <select
          class="select"
          style={{ flex: 1 }}
          value={metric}
          data-testid="pareto-metric"
          onChange={(e) => ui.set('paretoMetric', (e.target as HTMLSelectElement).value as ParetoMetric)}
        >
          <option value="vm">Max von Mises (MPa)</option>
          <option value="safety">Safety factor (×)</option>
          <option value="compliance">Compliance (N·mm)</option>
          <option value="peel">Layer peel (×)</option>
          <option value="deflection">Max deflection (mm)</option>
          {hasF1 && <option value="f1">Mode 1 f₁ (Hz)</option>}
        </select>
      </div>
      <svg class="pareto-svg" data-testid="pareto-plot" viewBox={`0 0 ${W} ${H}`}>
        <line x1={padL} y1={padT} x2={padL} y2={H - padB} stroke="var(--line-strong)" stroke-width="1" />
        <line x1={padL} y1={H - padB} x2={W - padR} y2={H - padB} stroke="var(--line-strong)" stroke-width="1" />
        <text x={padL - 4} y={padT + 4} text-anchor="end" class="pareto-axis">{fmt(yMax, 1)}</text>
        <text x={padL - 4} y={H - padB} text-anchor="end" class="pareto-axis">{fmt(yMin, 1)}</text>
        <text x={padL} y={H - 8} text-anchor="start" class="pareto-axis">{fmt(xMin, 2)} g</text>
        <text x={W - padR} y={H - 8} text-anchor="end" class="pareto-axis">{fmt(xMax, 2)} g</text>
        <text x={(padL + W - padR) / 2} y={H - 8} text-anchor="middle" class="pareto-axis">mass (g)</text>
        {frontIndices.length >= 2 && (
          <polyline
            data-testid="pareto-line"
            fill="none"
            stroke="var(--accent)"
            stroke-width="1.5"
            stroke-dasharray="3,2"
            points={polyPoints}
          />
        )}
        {points.map((p, i) => {
          const isFront = front.has(i);
          const isSel = selectedIndex === i;
          const cx = sx(xs[i]);
          const cy = sy(ys[i]);
          const label = param2
            ? `${param}=${fmt(p.value)}, ${param2}=${fmt(p.value2 ?? 0)}`
            : `${param}=${fmt(p.value)}`;
          return (
            <circle
              key={i}
              cx={cx}
              cy={cy}
              r={isSel ? 6.5 : isFront ? 5.2 : 3.8}
              class={`pareto-pt${isFront ? ' frontier' : ''}${isSel ? ' selected' : ''}`}
              data-testid={`pareto-pt-${i}`}
              data-index={String(i)}
              data-pareto={isFront ? 'true' : 'false'}
              data-selected={isSel ? 'true' : 'false'}
              onClick={() => sweep.applyPoint(i)}
            >
              <title>{`${label} · mass ${fmt(xs[i], 2)} g · ${fmt(ys[i], 2)} ${mInfo.unit}${isFront ? ' (Pareto front)' : ''}`}</title>
            </circle>
          );
        })}
      </svg>
      <div class="meta" data-testid="pareto-summary">
        {front.size} of {points.length} on Pareto front ({mInfo.label} vs mass) — click a point to apply to document (Ctrl+Z to undo)
      </div>
    </div>
  );
}

/** The parameter sweep panel (GOAL v0.4 section D & v0.7 M25): 1D or 2D grid sweep over
 * numeric document parameters (`let` variables or statement attributes), streaming rows
 * and an interactive Pareto scatter frontier. */
function SweepSection() {
  const sw = useSweep();
  const params = addressableParams(store.doc);
  if (!params.length) return null;
  const sel = params.some((p) => p.addr === ui.sweepParam) ? (ui.sweepParam as string) : params[0].addr;
  const cur = params.find((p) => p.addr === sel)?.value ?? 0;
  const from = ui.sweepFrom ?? cur;
  const to = ui.sweepTo ?? cur;
  const sel2 = ui.sweepParam2 && params.some((p) => p.addr === ui.sweepParam2 && p.addr !== sel) ? ui.sweepParam2 : '';
  const cur2 = sel2 ? (params.find((p) => p.addr === sel2)?.value ?? 0) : 0;
  const from2 = ui.sweepFrom2 ?? cur2;
  const to2 = ui.sweepTo2 ?? cur2;
  const run = () =>
    sweep.start(
      sel,
      from,
      to,
      Math.max(1, Math.round(ui.sweepCount)),
      sel2 || null,
      from2,
      to2,
      Math.max(1, Math.round(ui.sweepCount2)),
    );
  return (
    <>
      <div class="field-grid" data-testid="sweep-panel">
        <span class="plain">Parametric Sweep</span>
        <span class="meta">
          Steps a geometry or variable parameter across a range of values (solving the mesh refinement rungs on an isolated copy at each step — the active document keeps its values)
        </span>
        <span class="plain">Parameter</span>
        <select
          class="select"
          value={sel}
          data-testid="sweep-param"
          title="which parameter the sweep steps"
          onChange={(e) => {
            ui.set('sweepParam', (e.target as HTMLSelectElement).value);
            // From/to re-derive from the newly chosen parameter.
            ui.set('sweepFrom', null);
            ui.set('sweepTo', null);
          }}
        >
          {params.map((p) => (
            <option key={p.addr} value={p.addr}>
              {p.addr}
              {p.value !== null ? ` = ${fmt(p.value)}` : ''}
            </option>
          ))}
        </select>
        <span class="plain">Values</span>
        <div class="scale-row">
          <NumberInput value={from} onChange={(v) => ui.set('sweepFrom', v)} label="sweep from" testId="sweep-from" />
          <NumberInput value={to} onChange={(v) => ui.set('sweepTo', v)} label="sweep to" testId="sweep-to" />
          <NumberInput
            value={ui.sweepCount}
            min={1}
            step={1}
            integer
            onChange={(v) => ui.set('sweepCount', Math.max(1, Math.round(v)))}
            label="sweep points"
            testId="sweep-count"
          />
          <button type="button" class="mini-btn" data-testid="sweep-run" disabled={sw.running} title="Run the sweep on the analysis worker" onClick={run}>
            Run
          </button>
        </div>
        {params.length > 1 && (
          <>
            <span class="plain">Param 2</span>
            <select
              class="select"
              value={sel2}
              data-testid="sweep-param-2"
              title="optional second parameter for a 2D grid sweep"
              onChange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                ui.set('sweepParam2', v || null);
                ui.set('sweepFrom2', null);
                ui.set('sweepTo2', null);
              }}
            >
              <option value="">None (1D)</option>
              {params
                .filter((p) => p.addr !== sel)
                .map((p) => (
                  <option key={p.addr} value={p.addr}>
                    {p.addr}
                    {p.value !== null ? ` = ${fmt(p.value)}` : ''}
                  </option>
                ))}
            </select>
            {sel2 && (
              <>
                <span class="plain">Values 2</span>
                <div class="scale-row">
                  <NumberInput value={from2} onChange={(v) => ui.set('sweepFrom2', v)} label="sweep 2 from" testId="sweep-from-2" />
                  <NumberInput value={to2} onChange={(v) => ui.set('sweepTo2', v)} label="sweep 2 to" testId="sweep-to-2" />
                  <NumberInput
                    value={ui.sweepCount2}
                    min={1}
                    step={1}
                    integer
                    onChange={(v) => ui.set('sweepCount2', Math.max(1, Math.round(v)))}
                    label="sweep 2 points"
                    testId="sweep-count-2"
                  />
                </div>
              </>
            )}
          </>
        )}
        {sw.running && (
          <>
            <span class="plain">Progress</span>
            <span class="num" data-testid="sweep-progress">
              {sw.points.length} of {sw.total}…
            </span>
          </>
        )}
        {sw.points.length >= 2 && (
          <ParetoPlot points={sw.points} selectedIndex={sw.selectedIndex} param={sw.param} param2={sw.param2} />
        )}
        {sw.points.length > 0 && (
          <>
            <span class="plain">Points</span>
            <span class="meta">value · dofs · elements · wall ms · deflect mm · MPa</span>
            {sw.points.map((p, i) => {
              const last = p.records[p.records.length - 1];
              const label = sw.param2 && p.value2 !== undefined ? `${fmt(p.value)}, ${fmt(p.value2)}` : fmt(p.value);
              return (
                <Fragment key={i}>
                  <span class="plain">{label}</span>
                  <span class="num" data-testid={`sweep-row-${i}`}>
                    {last ? `${last.dofs} dofs · ${last.elements} elements · wall ${fmt(last.wall_ms, 0)} ms · deflect ${fmt(last.tracked.deflection_mm)} mm · ${fmt(last.tracked.vm_mpa)} MPa` : ''}
                  </span>
                </Fragment>
              );
            })}
          </>
        )}
        {sw.done && (
          <>
            <span class="plain">Export</span>
            <div class="scale-row">
              <button type="button" class="mini-btn" data-testid="sweep-export" title="The CLI's sweep CSV — same header and cells; the wall-time column cannot match" onClick={() => sweep.exportCsv()}>
                sweep CSV
              </button>
            </div>
          </>
        )}
      </div>
      {sw.error && (
        <div class="error-box" data-testid="sweep-error">
          {sw.error}
        </div>
      )}
    </>
  );
}

// ---- Universal Results Transient Evolution, Milestone Recap & Real-Time .webm Video ----

async function renderResultsTransientVideoWebm(
  lvl: LevelResult,
  onProgress: (pct: number) => void,
): Promise<string> {
  if (!resultHost) throw new Error('3D viewport not ready');
  const vp = resultHost;
  vp.camera.up.set(0, 0, 1);

  const width = 560;
  const topBannerH = 52;
  const sceneH = 600;
  const bottomBannerH = 52;
  const height = topBannerH + sceneH + bottomBannerH; // 704 (multiple of 16)
  const dstAspect = width / sceneH;

  const compCanvas = document.createElement('canvas');
  compCanvas.width = width;
  compCanvas.height = height;
  const ctx = compCanvas.getContext('2d');
  if (!ctx) throw new Error('Could not create 2D composite canvas context');

  const mimeCandidates = [
    'video/webm;codecs=vp9',
    'video/webm;codecs=vp8',
    'video/webm',
  ];
  const mimeType =
    mimeCandidates.find((m) => typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported(m)) ?? '';

  const stream = compCanvas.captureStream(30);
  const recorder = mimeType ? new MediaRecorder(stream, { mimeType, videoBitsPerSecond: 4_500_000 }) : new MediaRecorder(stream);
  const chunks: BlobPart[] = [];
  recorder.ondataavailable = (ev) => {
    if (ev.data && ev.data.size > 0) chunks.push(ev.data);
  };

  const stopped = new Promise<string>((resolve, reject) => {
    recorder.onstop = () => {
      const blob = new Blob(chunks, { type: mimeType || 'video/webm' });
      resolve(URL.createObjectURL(blob));
    };
    recorder.onerror = (e) => reject(e);
  });

  const isModeAnim = ui.resultsModeShape >= 0 && animatingShape(lvl) !== null;
  const modeFreqHz = isModeAnim ? (lvl.meta.modes?.frequencies[ui.resultsModeShape] ?? 0) : 0;
  const ci = effCase(lvl);
  const peakVm = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_vm_elem] : lvl.meta.cases.map((c) => c.max_vm_elem)));
  const peakDefl = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_displacement] : lvl.meta.cases.map((c) => c.max_displacement)));
  const yieldMpa = lvl.meta.yield_strength || 230;
  const tField = tempField(lvl, ci);
  let tMin = 20;
  let tMax = 20;
  if (tField && tField.length > 0) {
    tMin = Infinity;
    tMax = -Infinity;
    for (const v of tField) {
      if (v < tMin) tMin = v;
      if (v > tMax) tMax = v;
    }
  }
  const docTitle = (store.doc.name || '3D Solid & Thermal FEA').slice(0, 44);
  const nFrames = 36;

  recorder.start();

  for (let k = 0; k < nFrames; k++) {
    const frac = k / Math.max(1, nFrames - 1);
    if (isModeAnim) {
      ui.set('resultsModePhase', frac * 2 * Math.PI);
    } else {
      ui.set('resultsRamp', Math.round(frac * 1000) / 1000);
    }
    syncResults(vp);
    const srcCanvas = vp.captureCanvas();

    ctx.fillStyle = '#090d16';
    ctx.fillRect(0, 0, width, height);
    if (srcCanvas && srcCanvas.width > 0 && srcCanvas.height > 0) {
      try {
        const srcW = srcCanvas.width;
        const srcH = srcCanvas.height;
        const srcAspect = srcW / srcH;
        let sx = 0;
        let sy = 0;
        let sw = srcW;
        let sh = srcH;
        if (srcAspect > dstAspect) {
          sh = srcH;
          sw = Math.round(srcH * dstAspect);
          sx = Math.round((srcW - sw) * 0.5);
        } else {
          sw = srcW;
          sh = Math.round(srcW / dstAspect);
          sy = Math.round((srcH - sh) * 0.5);
        }
        ctx.drawImage(srcCanvas, sx, sy, sw, sh, 0, topBannerH, width, sceneH);
      } catch {
        // ignore draw error
      }
    }

    // Top HUD Banner
    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, 0, width, topBannerH);
    ctx.fillStyle = '#f8fafc';
    ctx.font = 'bold 12px Inter, system-ui, sans-serif';
    ctx.fillText(
      `FIELDSMITH FEA — ${docTitle.toUpperCase()}`,
      12,
      20,
    );
    ctx.fillStyle = '#fbbf24';
    ctx.font = 'bold 11px JetBrains Mono, monospace';
    const subLine = isModeAnim
      ? `Mode ${ui.resultsModeShape + 1} (${modeFreqHz.toFixed(1)} Hz) | Phase = ${(frac * 360).toFixed(0)}° (cos=${Math.cos(frac * 2 * Math.PI).toFixed(2)})`
      : `Field: ${ui.resultsMode.toUpperCase()} (${ui.resultsColormap.toUpperCase()}${ui.resultsVolumeOn ? ' + 3D VOL' : ''}) | Load Ramp = ${(frac * 100).toFixed(0)}%`;
    ctx.fillText(subLine, 12, 39);

    // Bottom Telemetry Banner
    const curVm = peakVm * (isModeAnim ? Math.abs(Math.cos(frac * 2 * Math.PI)) : frac);
    const curDefl = peakDefl * (isModeAnim ? Math.abs(Math.cos(frac * 2 * Math.PI)) : frac);
    const curSf = curVm > 0.05 ? Math.min(99.9, yieldMpa / curVm) : 99.9;
    const curTemp = tMin + (tMax - tMin) * frac;

    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, height - bottomBannerH, width, bottomBannerH);
    ctx.fillStyle = '#e2e8f0';
    ctx.font = '10.5px JetBrains Mono, monospace';
    ctx.fillText(
      `σ_vM: ${curVm.toFixed(1)} MPa | u_max: ${curDefl.toFixed(3)} mm | SF: ${curSf.toFixed(2)}×${tField ? ` | T: ${curTemp.toFixed(0)} °C` : ''}`,
      12,
      height - 34,
    );
    ctx.fillStyle = '#cbd5e1';
    ctx.fillText(
      `Mesh: ${lvl.label} (${lvl.meta.provenance.elements} P${lvl.meta.provenance.order} tet · ${lvl.meta.provenance.dofs} DOFs) | Deform ×${ui.resultsScale}`,
      12,
      height - 18,
    );

    const pct = (k + 1) / nFrames;
    ctx.fillStyle = 'rgba(255,255,255,0.14)';
    ctx.fillRect(12, height - 8, width - 24, 4);
    ctx.fillStyle = '#f97316';
    ctx.fillRect(12, height - 8, (width - 24) * pct, 4);

    onProgress(Math.round(pct * 100));
    await new Promise((r) => window.setTimeout(r, 42));
  }

  recorder.stop();
  return stopped;
}

function ResultsTransientRecapBar({ lvl }: { lvl: LevelResult }) {
  useApp();
  const [renderingVideo, setRenderingVideo] = useState(false);
  const [videoPct, setVideoPct] = useState(0);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);

  const isModeAnim = ui.resultsModeShape >= 0 && animatingShape(lvl) !== null;
  const ci = effCase(lvl);
  const disc = activeResultsDiscipline(lvl);
  const peakVm = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_vm_elem] : lvl.meta.cases.map((c) => c.max_vm_elem)));
  const peakDefl = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_displacement] : lvl.meta.cases.map((c) => c.max_displacement)));
  const tField = tempField(lvl, ci);
  let tMin = 20, tMax = 20;
  if (tField && tField.length > 0) {
    tMin = Infinity;
    tMax = -Infinity;
    for (const v of tField) {
      if (v < tMin) tMin = v;
      if (v > tMax) tMax = v;
    }
  }
  const hf = disc === 'thermal' ? heatFluxField(lvl, ci) : null;
  const fl = disc === 'fluid' ? flowFieldData(lvl, ci) : null;
  const ramp = ui.resultsRamp;
  const playing = isModeAnim ? ui.resultsModePlay : ui.resultsRampPlay;

  const isA15Droplet = isRayleighLambDropletDoc(store.doc);
  const modeCos = Math.cos(ui.resultsModePhase);
  const a15PhaseLabel = modeCos > 0.25 ? 'Prolate Crest' : modeCos < -0.25 ? 'Oblate Trough' : 'Spherical Equilibrium';

  const rampReadout = isModeAnim
    ? isA15Droplet
      ? `Rayleigh–Lamb Mode ${ui.resultsModeShape + 1} · Phase ${Math.round((ui.resultsModePhase * 180) / Math.PI)}° · ${a15PhaseLabel}`
      : `Phase ${Math.round((ui.resultsModePhase * 180) / Math.PI)}°`
    : disc === 'thermal' && tField
      ? `Thermal ${(ramp * 100).toFixed(0)}% · ${fmt(tMin + (tMax - tMin) * ramp, 1)} °C · |q| ${fmt((hf?.maxFlux ?? 0) * ramp, 1)} kW/m²`
      : disc === 'fluid' && fl
        ? `Flow ${(ramp * 100).toFixed(0)}% · |v| ${fmt(fl.maxSpeed * ramp, 1)} mm/s · τ ${fmt(fl.maxShearPa * ramp, 0)} Pa`
        : ui.resultsMode === 'strain_energy'
          ? `Load ${(ramp * 100).toFixed(0)}% · W_e ${fmt(strainEnergyOfVm(peakVm * ramp), 2)} kJ/m³`
          : `Load ${(ramp * 100).toFixed(0)}% · ${fmt(peakVm * ramp, 1)} MPa`;

  const milestones = isModeAnim
    ? isA15Droplet
      ? [
          { label: '1. Prolate (0°)', val: 0, title: 'Positive eigenmode crest (cos = +1.0)' },
          { label: '2. Sphere (90°)', val: Math.PI * 0.5, title: 'Zero-crossing spherical equilibrium (cos = 0.0)' },
          { label: '3. Oblate (180°)', val: Math.PI, title: 'Negative eigenmode trough (cos = −1.0)' },
          { label: '4. Sphere (270°)', val: Math.PI * 1.5, title: 'Zero-crossing spherical equilibrium (cos = 0.0)' },
        ]
      : [
          { label: '1. +Peak (0°)', val: 0, title: 'Positive displacement crest (cos = +1.0)' },
          { label: '2. Node (90°)', val: Math.PI * 0.5, title: 'Zero-crossing nodal equilibrium (cos = 0.0)' },
          { label: '3. −Peak (180°)', val: Math.PI, title: 'Negative displacement trough (cos = −1.0)' },
          { label: '4. Full Cycle (360°)', val: 0, title: 'Complete harmonic oscillation cycle' },
        ]
    : disc === 'thermal' && tField
      ? [
          { label: '1. Ambient (0%)', val: 0.0, title: `Reference isothermal baseline (${fmt(tMin, 1)} °C)` },
          { label: '2. Warmup (35%)', val: 0.35, title: `35% thermal gradient (${fmt(tMin + (tMax - tMin) * 0.35, 1)} °C)` },
          { label: '3. Soak (70%)', val: 0.70, title: `70% thermal gradient (${fmt(tMin + (tMax - tMin) * 0.70, 1)} °C)` },
          { label: '4. Steady (100%)', val: 1.0, title: `Steady-state conduction (${fmt(tMax, 1)} °C · |q| ${fmt(hf?.maxFlux ?? 0, 1)} kW/m²)` },
        ]
      : disc === 'fluid' && fl
        ? [
            { label: '1. Quiescent (0%)', val: 0.0, title: 'Zero-flow hydrostatic / Laplace reference' },
            { label: '2. Creeping (35%)', val: 0.35, title: `35% flow ramp (|v| ${fmt(fl.maxSpeed * 0.35, 1)} mm/s)` },
            { label: '3. Developing (70%)', val: 0.70, title: `70% flow ramp (|v| ${fmt(fl.maxSpeed * 0.70, 1)} mm/s)` },
            { label: '4. Full Flow (100%)', val: 1.0, title: `Steady continuum flow (|v| ${fmt(fl.maxSpeed, 1)} mm/s · τ ${fmt(fl.maxShearPa, 1)} Pa)` },
          ]
        : [
            { label: '1. Unloaded (0%)', val: 0.0, title: 'Unloaded reference geometry & ambient temperature' },
            { label: '2. Onset (35%)', val: 0.35, title: `Elastic & thermal onset (${fmt(peakVm * 0.35, 1)} MPa)` },
            { label: '3. Knee (70%)', val: 0.70, title: `70% design load (${fmt(peakVm * 0.70, 1)} MPa)` },
            { label: '4. Full Load (100%)', val: 1.0, title: `100% steady-state design load (${fmt(peakVm, 1)} MPa · ${fmt(peakDefl, 2)} mm)` },
          ];

  const handleRenderVideo = async () => {
    if (renderingVideo) return;
    ui.set('resultsModePlay', false);
    ui.set('resultsRampPlay', false);
    setRenderingVideo(true);
    setVideoPct(0);
    const prevRamp = ui.resultsRamp;
    const prevPhase = ui.resultsModePhase;
    try {
      const url = await renderResultsTransientVideoWebm(lvl, (pct) => setVideoPct(pct));
      setVideoUrl(url);
      ui.notify('Rendered real-time 30 fps .webm video of 3D FEA Results');
    } catch (e) {
      ui.notify(`Video render error: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      ui.set('resultsRamp', prevRamp);
      ui.set('resultsModePhase', prevPhase);
      setRenderingVideo(false);
    }
  };

  return (
    <div
      data-testid="results-transient-bar"
      style={{
        margin: '6px 0',
        padding: '6px 8px',
        background: 'var(--panel-2)',
        border: '1px solid var(--line)',
        borderRadius: '5px',
        display: 'flex',
        flexDirection: 'column',
        gap: '5px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
          <button
            type="button"
            class="mini-btn"
            data-testid="results-transient-play"
            onClick={() => {
              if (isModeAnim) {
                ui.set('resultsModePlay', !ui.resultsModePlay);
              } else {
                if (!ui.resultsRampPlay && ui.resultsRamp >= 0.99) ui.set('resultsRamp', 0);
                ui.set('resultsRampPlay', !ui.resultsRampPlay);
              }
            }}
          >
            {playing ? '⏸ Pause' : '▶ Play Ramp'}
          </button>
          <span class="num" style={{ fontSize: '11px', fontWeight: 600, color: 'var(--warn)' }}>
            {rampReadout}
          </span>
        </div>
        <button
          type="button"
          class="mini-btn"
          data-testid="results-render-video"
          disabled={renderingVideo}
          onClick={handleRenderVideo}
          style={{ background: '#ea580c', color: '#fff', borderColor: '#f97316', fontWeight: 600 }}
          title="Synthesise a real-time 30 fps upright .webm video with ParaView colourmap, 3D volume & telemetry HUD"
        >
          {renderingVideo ? `Rendering ${videoPct}%…` : '🎬 Render Video'}
        </button>
      </div>

      <input
        type="range"
        min={0}
        max={isModeAnim ? 360 : 100}
        step={1}
        value={isModeAnim ? Math.round((ui.resultsModePhase * 180) / Math.PI) : Math.round(ramp * 100)}
        data-testid="results-transient-slider"
        onInput={(e) => {
          const v = parseFloat((e.target as HTMLInputElement).value) || 0;
          if (isModeAnim) {
            ui.set('resultsModePlay', false);
            ui.set('resultsModePhase', (v * Math.PI) / 180);
          } else {
            ui.set('resultsRampPlay', false);
            ui.set('resultsRamp', Math.max(0, Math.min(1, v / 100)));
          }
        }}
      />

      <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
        {milestones.map((m, idx) => {
          const active = isModeAnim
            ? Math.abs(ui.resultsModePhase - m.val) < 0.25
            : Math.abs(ramp - m.val) < 0.06;
          return (
            <button
              key={idx}
              type="button"
              class="mini-btn"
              data-testid={`results-recap-${idx}`}
              title={m.title}
              onClick={() => {
                if (isModeAnim) {
                  ui.set('resultsModePlay', false);
                  ui.set('resultsModePhase', m.val);
                } else {
                  ui.set('resultsRampPlay', false);
                  ui.set('resultsRamp', m.val);
                }
              }}
              style={
                active
                  ? { borderColor: '#f59e0b', color: '#fbbf24', fontWeight: 600, fontSize: '10px' }
                  : { fontSize: '10px' }
              }
            >
              {m.label}
            </button>
          );
        })}
      </div>

      {videoUrl && (
        <div style={{ marginTop: '4px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
          <video
            src={videoUrl}
            controls
            autoPlay
            loop
            muted
            data-testid="results-video-player"
            style={{
              width: '210px',
              height: '264px',
              aspectRatio: '560 / 704',
              objectFit: 'contain',
              alignSelf: 'center',
              borderRadius: '4px',
              border: '1px solid var(--line)',
              background: '#090d16',
            }}
          />
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', fontSize: '10.5px' }}>
            <span class="num" style={{ color: 'var(--ok)' }}>✓ 30 fps WebM ready (560×704)</span>
            <a
              href={videoUrl}
              download="fieldsmith-fea-results.webm"
              class="mini-btn"
              data-testid="results-download-video"
              style={{ textDecoration: 'none' }}
            >
              ⬇ Download .webm
            </a>
          </div>
        </div>
      )}
    </div>
  );
}

// ---- panel ---------------------------------------------------------------------------

let lastResultsLevel: { loads: number; lvl: LevelResult } | null = null;

function ResultsPanel() {
  useApp();
  const [compact, setCompact] = useState(false);
  const [rungsExpanded, setRungsExpanded] = useState<boolean>(() =>
    typeof window !== 'undefined' ? window.innerWidth > 820 : false,
  );
  if (store.lens !== 'results') return null;
  const minimized = ui.panelMinimized || ui.vpHudCollapsed;
  const toggleMinimized = () => {
    if (minimized) {
      ui.set('panelMinimized', false);
      if (ui.vpHudCollapsed) ui.set('vpHudCollapsed', false);
    } else {
      ui.set('panelMinimized', true);
    }
  };
  const s = analysis.state;
  const latestLvl = s.levels[s.levels.length - 1];
  if (latestLvl) {
    lastResultsLevel = { loads: store.loads, lvl: latestLvl };
  } else if (lastResultsLevel && lastResultsLevel.loads !== store.loads) {
    lastResultsLevel = null;
  }
  const lvl = latestLvl ?? (s.status === 'running' ? lastResultsLevel?.lvl : undefined);
  if (lvl) {
    autoSyncResultsDiscipline(lvl);
  }
  const shown = lvl ? shownDisp(lvl) : null;
  const animating = shown?.animating ?? false;
  const ci = lvl ? effCase(lvl) : -1;
  const mode: ResultMode = animating ? 'deflection' : ui.resultsMode;
  const hi = animating && shown ? shown.rampMax : lvl ? modeMax(lvl, mode, ci) : 0;
  const legend = lvl ? legendLabels(lvl, mode, ci, hi, lvl.meta.yield_strength) : null;
  if (legend && animating) legend.caption = shown?.caption ?? '';
  const rng = lvl ? shownRange(lvl, shown!) : null;
  const primaryDisc = detectPrimaryResultsDiscipline(lvl);
  const activeDisc = activeResultsDiscipline(lvl);
  const matSpec = docContinuumRheology();
  const matchedCoupledId = matchedCoupledSystemIdForDoc(store.doc, ui.tutorialSheetId);
  const tFieldSummary = lvl ? tempField(lvl, ci) : null;
  let tMinSummary = 20, tMaxSummary = 20;
  if (tFieldSummary && tFieldSummary.length > 0) {
    tMinSummary = Infinity;
    tMaxSummary = -Infinity;
    for (const v of tFieldSummary) {
      if (v < tMinSummary) tMinSummary = v;
      if (v > tMaxSummary) tMaxSummary = v;
    }
  }
  const hfSummary = lvl && (activeDisc === 'thermal' || !!tFieldSummary) ? heatFluxField(lvl, ci) : null;
  const flSummary = lvl && (activeDisc === 'fluid' || primaryDisc === 'fluid') ? flowFieldData(lvl, ci) : null;
  const peakVmSummary = lvl ? Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem)) : 0;
  const peakDeflSummary = lvl ? Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement)) : 0;
  const peakWeSummary = strainEnergyOfVm(peakVmSummary);
  // Arrow-key step inside the legend inputs: a tenth of the top bound, on the input's decade.
  const rangeStep = rng ? Math.pow(10, Math.floor(Math.log10(Math.max(1e-9, rng.hi))) - 1) : 1;

  const switchDiscipline = (target: ResultsDiscipline) => {
    if (!lvl) return;
    if (target === 'thermal') {
      if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
      const ti = lvl.meta.cases.findIndex((_, i) => !!tempField(lvl, i));
      if (ti >= 0 && ci >= 0 && !tempField(lvl, ci)) ui.set('resultsCase', ti);
      ui.set('resultsMode', 'temperature');
    } else if (target === 'fluid') {
      if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
      ui.set('resultsMode', 'flow_field');
    } else if (target === 'modal') {
      ui.set('resultsMode', 'deflection');
      if (lvl.modeShapes && ui.resultsModeShape < 0) ui.set('resultsModeShape', 0);
    } else {
      if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
      ui.set('resultsMode', 'stress');
    }
  };

  return (
    <div class={`results-panel${minimized ? ' minimized' : ''}`} data-testid="results-panel">
      <div
        class="card-head"
        style={minimized ? { marginBottom: 0, cursor: 'pointer' } : undefined}
        onClick={minimized ? toggleMinimized : undefined}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', minWidth: 0, flexWrap: 'wrap' }}>
          <span class="title">Results</span>
          {minimized && lvl && (
            <span class="num" style={{ fontSize: '11px', color: 'var(--text-2)', display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
              <span
                style={{
                  width: '28px',
                  height: '6px',
                  borderRadius: '3px',
                  background: gradientCss(mode),
                  display: 'inline-block',
                }}
              />
              <span>{formatDisciplineBadgeText(lvl)}</span>
            </span>
          )}
          {minimized && !lvl && s.status === 'running' && (
            <span class="meta">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</span>
          )}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }} onClick={(e) => e.stopPropagation()}>
          {!minimized && lvl && <span class="meta">{lvl.label}</span>}
          {lvl && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-header-play-toggle"
              title="Play or Pause 3D oscillation / load-ramp playback"
              onClick={() => {
                const isMode = ui.resultsModeShape >= 0 && animatingShape(lvl) !== null;
                if (isMode) {
                  ui.set('resultsModePlay', !ui.resultsModePlay);
                } else {
                  if (!ui.resultsRampPlay && ui.resultsRamp >= 0.99) ui.set('resultsRamp', 0);
                  ui.set('resultsRampPlay', !ui.resultsRampPlay);
                }
              }}
            >
              {(ui.resultsModeShape >= 0 && animatingShape(lvl) !== null ? ui.resultsModePlay : ui.resultsRampPlay)
                ? '⏸ Pause'
                : '▶ Play'}
            </button>
          )}
          {!minimized && lvl && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-compact-toggle"
              title="Toggle Compact Summary vs Full Rungs & Sweep View"
              onClick={() => setCompact(!compact)}
            >
              {compact ? 'More ▾' : 'Compact ▴'}
            </button>
          )}
          <button
            type="button"
            class="mini-btn"
            data-testid="results-minimize-toggle"
            title={minimized ? 'Expand Results panel' : 'Minimize Results panel to bottom bar'}
            onClick={toggleMinimized}
          >
            {minimized ? '▴ Expand' : '▾ Min'}
          </button>
        </div>
      </div>
      {!minimized && (
        <>
          {!store.doc.root.length && <div class="hint">Add shapes to the model first.</div>}
          {store.doc.root.length > 0 && s.status === 'error' && (s.errorKind === 'setup' ? <div class="hint">{s.error}</div> : <div class="error-box">{s.error}</div>)}
          {store.doc.root.length > 0 && s.status === 'running' && <div class="hint">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</div>}
          {store.doc.root.length > 0 && !lvl && s.status === 'ready' && <div class="hint">No results yet.</div>}
          {lvl && (
        <>
          {/* Multi-Physics Post-Processing Discipline & Extended Field Selector */}
          <div
            data-testid="results-discipline-bar"
            style={{
              margin: '0 0 6px 0',
              padding: '6px 8px',
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              borderRadius: '6px',
              display: 'flex',
              flexDirection: 'column',
              gap: '5px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
              <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-2)' }}>
                Post-Processing Physics Lens
              </span>
              <span class="num" style={{ fontSize: '9.5px', color: '#93c5fd' }}>
                Auto: {primaryDisc === 'thermal' ? 'Thermal Conduction' : primaryDisc === 'fluid' ? 'Visco-Capillary / Fluid' : primaryDisc === 'modal' ? 'Modal Vibration' : '3D Solid Mechanics'}
              </span>
            </div>
            <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
              {([
                { id: 'structural', label: '🔩 Solid', title: 'Hookean solid stress, safety factor, peel & strain energy' },
                { id: 'thermal', label: '🌡️ Thermal & Flux', title: 'Fourier temperature field (°C) & conductive heat flux |q| (kW/m²)' },
                { id: 'fluid', label: '🌊 Fluid & Capillary', title: 'Continuum flow velocity |v| (mm/s), viscous shear τ (Pa) & Re/Oh/Ca numbers' },
                { id: 'modal', label: '🎵 Modal', title: 'Natural eigenfrequencies (Hz) & harmonic mode-shape kinematics' },
              ] as { id: ResultsDiscipline; label: string; title: string }[]).map((d) => {
                const isAct = activeDisc === d.id;
                return (
                  <button
                    key={d.id}
                    type="button"
                    class="mini-btn"
                    data-testid={`discipline-${d.id}`}
                    title={d.title}
                    onClick={() => switchDiscipline(d.id)}
                    style={
                      isAct
                        ? {
                            background: 'rgba(59, 130, 246, 0.2)',
                            borderColor: '#60a5fa',
                            color: '#eff6ff',
                            fontWeight: 700,
                            fontSize: '10.5px',
                          }
                        : { fontSize: '10.5px' }
                    }
                  >
                    {d.label}
                  </button>
                );
              })}
            </div>
            <div
              data-testid="results-extended-modes"
              style={{ display: 'flex', gap: '4px', flexWrap: 'wrap', alignItems: 'center', paddingTop: '2px' }}
            >
              <span class="meta" style={{ fontSize: '9.5px', marginRight: '2px' }}>Continuum fields:</span>
              {([
                { value: 'heat_flux', label: '🔥 Heat Flux |q|', title: 'Fourier heat-flux magnitude |q| = k|∇T| (kW/m²)' },
                { value: 'flow_field', label: '🌊 Flow Speed |v|', title: 'Rayleigh–Stokes continuum flow velocity |v| (mm/s) & wall shear' },
                { value: 'strain_energy', label: '⚡ Strain Energy W_e', title: 'Elastic strain energy density W_e = σ_vM² / (2E) (kJ/m³)' },
                { value: 'plastic_strain', label: '🧬 Plastic Strain ε_p', title: '3D J₂ / Ramberg–Osgood–Hollomon equivalent plastic strain ε_p (%)' },
              ] as { value: ResultMode; label: string; title: string }[]).map((m) => {
                const isAct = mode === m.value;
                return (
                  <button
                    key={m.value}
                    type="button"
                    class="mini-btn"
                    data-testid={`results-ext-${m.value}`}
                    data-value={m.value}
                    title={m.title}
                    onClick={() => {
                      if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
                      if (m.value === 'heat_flux' && ci >= 0 && !tempField(lvl, ci)) {
                        const ti = lvl.meta.cases.findIndex((_, i) => !!tempField(lvl, i));
                        if (ti >= 0) ui.set('resultsCase', ti);
                      }
                      ui.set('resultsMode', m.value);
                    }}
                    style={
                      isAct
                        ? {
                            background: '#0284c7',
                            borderColor: '#38bdf8',
                            color: '#fff',
                            fontWeight: 700,
                            fontSize: '10px',
                          }
                        : { fontSize: '10px' }
                    }
                  >
                    {m.label}
                  </button>
                );
              })}
              {(activeDisc === 'fluid' || primaryDisc === 'fluid') && (
                <button
                  type="button"
                  class="mini-btn"
                  data-testid="results-open-sph-workbench"
                  onClick={() => openFreeSurfaceModal()}
                  style={{
                    background: 'rgba(14, 165, 233, 0.18)',
                    borderColor: '#38bdf8',
                    color: '#7dd3fc',
                    fontWeight: 600,
                    fontSize: '10px',
                  }}
                  title="Open Free-Surface Fluid & Gerris Analytic Flow Workbench (WC-SPH / VFT Stokes / CSF)"
                >
                  🌊 Free-Surface / Gerris Workbench →
                </button>
              )}
              {matchedCoupledId && (
                <button
                  type="button"
                  class="mini-btn"
                  data-testid="results-open-coupled-workbench"
                  onClick={() => runCommand('systems.open')}
                  style={{
                    background: 'rgba(16, 185, 129, 0.16)',
                    borderColor: '#34d399',
                    color: '#6ee7b7',
                    fontWeight: 600,
                    fontSize: '10px',
                  }}
                  title="Open Coupled 1D/2D Multi-Physics PDE Solver for this document"
                >
                  ⚡ Coupled PDE Workbench →
                </button>
              )}
              <button
                type="button"
                class="mini-btn"
                data-testid="results-open-coord-fem"
                onClick={() => openCoordMappingModal()}
                style={{
                  background: 'rgba(168, 85, 247, 0.16)',
                  borderColor: '#c084fc',
                  color: '#e9d5ff',
                  fontWeight: 600,
                  fontSize: '10px',
                }}
                title="Open 1D–4D Coordinate Mappings & Pullback FEM Suite (Polar, Cylindrical, Spherical, Confocal Elliptic, Parabolic, Toroidal & 4D Hyperspherical)"
              >
                🌐 1D–4D Coord FEM →
              </button>
            </div>
          </div>

          <div class="field-grid">
            {/* Full-span row: six core mode chips fit the panel width without horizontal crop. */}
            <div class="show-row">
              <Seg<ResultMode>
                value={mode}
                options={CORE_MODES.map((m) => ({ value: m, label: m[0].toUpperCase() + m.slice(1) }))}
                onChange={(v) => {
                  if (ui.resultsModeShape >= 0) ui.set('resultsModeShape', -1);
                  if (v === 'temperature' && ci >= 0 && !tempField(lvl, ci)) {
                    const ti = lvl.meta.cases.findIndex((_, i) => !!tempField(lvl, i));
                    if (ti >= 0) ui.set('resultsCase', ti);
                  }
                  ui.set('resultsMode', v);
                }}
                testId="results-mode"
              />
            </div>
            {lvl.meta.cases.length > 1 && (
              <>
                <span class="plain">Case</span>
                <Seg<string>
                  value={String(effCase(lvl))}
                  options={[{ value: '-1', label: 'Envelope' }, ...lvl.meta.cases.map((c, i) => ({ value: String(i), label: c.name }))]}
                  onChange={(v) => ui.set('resultsCase', Number.parseInt(v, 10))}
                  testId="results-case"
                />
              </>
            )}
            <span class="plain">Deform ×</span>
            <div class="scale-row">
              <NumberInput
                value={ui.resultsScale}
                min={0}
                max={200}
                step={1}
                onChange={(v) => ui.set('resultsScale', Math.round(v * 10) / 10)}
                testId="results-scale"
              />
              <button type="button" class="mini-btn" data-testid="scale-reset" title="Reset to the default ×5" onClick={() => ui.set('resultsScale', RESULTS_SCALE_DEFAULT)}>Reset</button>
            </div>
            <span class="plain">Colourmap</span>
            <div class="scale-row" style={{ gap: '4px', flexWrap: 'wrap' }}>
              <Seg<ResultColormap>
                value={ui.resultsColormap}
                options={[
                  { value: 'viridis', label: 'Viridis' },
                  { value: 'flame', label: 'Flame' },
                  { value: 'turbo', label: 'Turbo' },
                  { value: 'coolwarm', label: 'Coolwarm' },
                ]}
                onChange={(v) => ui.set('resultsColormap', v)}
                testId="results-colormap"
              />
            </div>
            <span class="plain">Mesh plot</span>
            <div class="scale-row" style={{ gap: '4px', flexWrap: 'wrap' }}>
              <Seg<ResultInterp>
                value={ui.resultsInterp}
                options={[
                  { value: 'p2', label: 'P2 Smooth' },
                  { value: 'p2_wire', label: 'P2 + Wire' },
                  { value: 'p0', label: 'P0 Facet' },
                ]}
                onChange={(v) => ui.set('resultsInterp', v)}
                testId="results-interp"
              />
              <button
                type="button"
                class={`mini-btn${ui.resultsVolumeOn ? ' active' : ''}`}
                data-testid="results-volume-toggle"
                title="ParaView / VTK 3D translucent volumetric iso-shells & additive scalar glow"
                onClick={() => ui.set('resultsVolumeOn', !ui.resultsVolumeOn)}
                style={
                  ui.resultsVolumeOn
                    ? { background: '#ea580c', color: '#fff', borderColor: '#f97316', fontWeight: 600 }
                    : undefined
                }
              >
                {ui.resultsVolumeOn ? '3D Volume: ON' : '3D Volume'}
              </button>
            </div>
          </div>
          {legend && legend.caption && (
            <div class="legend-block" data-testid="legend">
              {isRamped(mode) && rng && !animating ? (
                <div class="legend-range">
                  <NumberInput
                    value={rng.lo}
                    min={mode === 'temperature' ? undefined : 0}
                    max={rng.hi}
                    step={rangeStep}
                    unit={rng.unit}
                    label="legend lower bound"
                    onChange={(lo) => ui.set('resultsRange', { mode, levelKey: rng.levelKey, lo, hi: rng.hi })}
                    testId="legend-lo"
                  />
                  <span class="legend-gradient" style={{ background: gradientCss(mode) }} />
                  <NumberInput
                    value={rng.hi}
                    min={0}
                    step={rangeStep}
                    unit={rng.unit}
                    label="legend upper bound"
                    onChange={(hi) => ui.set('resultsRange', { mode, levelKey: rng.levelKey, lo: rng.lo, hi })}
                    testId="legend-hi"
                  />
                  {!rng.auto && (
                    <button type="button" class="mini-btn" data-testid="legend-reset" title="Back to the auto bounds 0..max" onClick={() => ui.set('resultsRange', null)}>Reset</button>
                  )}
                </div>
              ) : rng && legend.max !== '' && (
                <div class="legend-range">
                  <span class="num">{legend.min}</span>
                  <span class="legend-gradient" style={{ background: gradientCss(mode) }} />
                  <span class="num">{legend.max}</span>
                </div>
              )}
              <div class="meta">
                {legend.caption}
                {rng && !rng.auto ? "; values outside the range render at the ramp's end colours" : ''}
              </div>
            </div>
          )}
          <ResultsTransientRecapBar lvl={lvl} />
          <div class="field-grid" data-testid="results-summary">
            {activeDisc === 'thermal' && tFieldSummary && (
              <>
                <span class="plain">Temperature span</span>
                <span class="num" data-testid="results-thermal-span">
                  {fmt(tMinSummary, 1)} … {fmt(tMaxSummary, 1)} °C (ΔT {fmt(tMaxSummary - tMinSummary, 1)} K)
                </span>
                <span class="plain">Peak heat flux |q|</span>
                <span class="num" data-testid="results-heat-flux-peak">
                  {fmt(hfSummary?.maxFlux ?? 0, 2)} kW/m² (k = {fmt(docThermalConductivityWPerMK(), 1)} W/m·K)
                </span>
              </>
            )}
            {activeDisc === 'fluid' && flSummary && (
              <>
                <span class="plain">Continuum medium</span>
                <span class="num" data-testid="results-fluid-medium">
                  {matSpec.label} (μ = {fmt(matSpec.viscosityPas, 2)} Pa·s · γ = {fmt(matSpec.surfaceTensionNPerM, 3)} N/m)
                </span>
                <span class="plain">Peak flow |v| & shear</span>
                <span class="num" data-testid="results-fluid-kinematics">
                  {fmt(flSummary.maxSpeed, 2)} mm/s · τ_max {fmt(flSummary.maxShearPa, 1)} Pa
                </span>
                <span class="plain">Dimensionless groups</span>
                <span class="num" data-testid="results-fluid-groups">
                  Re = {fmt(flSummary.reynolds, 2)} · Oh = {fmt(flSummary.ohnesorge, 3)} · Ca = {fmt(flSummary.capillary, 3)}
                </span>
              </>
            )}
            {activeDisc === 'modal' && lvl.meta.modes?.frequencies?.length && (
              <>
                <span class="plain">Fundamental mode f₁</span>
                <span class="num" data-testid="results-modal-f1">
                  {fmt(lvl.meta.modes.frequencies[0], 2)} Hz ({lvl.meta.modes.frequencies.length} modes solved)
                </span>
              </>
            )}
            {(activeDisc === 'structural' || peakVmSummary > 1e-6) && (
              <>
                <span class="plain">{activeDisc === 'thermal' ? 'Thermomech σ_vM' : activeDisc === 'fluid' ? 'Deviatoric traction' : 'Peak von Mises'}</span>
                <span class="num">{fmt(peakVmSummary)} MPa{mode === 'strain_energy' ? ` · W_e ${fmt(peakWeSummary, 2)} kJ/m³` : ''}</span>
                <span class="plain">Max deflection</span>
                <span class="num">{fmt(peakDeflSummary)} mm</span>
              </>
            )}
            {lvl.meta.max_layer_peel &&
              lvl.meta.max_layer_peel.length > 0 &&
              (activeDisc === 'structural' || Math.max(0, ...lvl.meta.max_layer_peel) > 1e-6) && (
              <>
                <span class="plain">Layer peel</span>
                <span class="num" data-testid="results-peel-summary" style={{ display: 'inline-flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                  <span>
                    ×{fmt(Math.max(0, ...lvl.meta.max_layer_peel), 2)} at ({fmt(lvl.meta.build_angles?.[0] ?? 0, 0)}°, {fmt(lvl.meta.build_angles?.[1] ?? 0, 0)}°)
                    {lvl.meta.best_build_angles != null && lvl.meta.min_peak_peel != null
                      ? ` · best (${fmt(lvl.meta.best_build_angles[0], 0)}°, ${fmt(lvl.meta.best_build_angles[1], 0)}°) → ×${fmt(lvl.meta.min_peak_peel, 2)}`
                      : ''}
                  </span>
                  {lvl.meta.best_build_angles != null && (
                    <button
                      type="button"
                      class="mini-btn"
                      data-testid="apply-best-build"
                      title="Apply the solver's optimal additive build orientation (theta, phi)"
                      onClick={() => {
                        const b = lvl.meta.best_build_angles!;
                        store.transact(`Set build orientation (${fmt(b[0], 0)}°, ${fmt(b[1], 0)}°)`, (d) => {
                          d.solve.build = [b[0], b[1]];
                        });
                        ui.notify(`Applied optimal build orientation (${fmt(b[0], 0)}°, ${fmt(b[1], 0)}°)`);
                      }}
                    >
                      Apply best
                    </button>
                  )}
                </span>
              </>
            )}
            <span class="plain">Elements</span>
            <span class="num">{lvl.meta.provenance.elements} tet · {lvl.meta.provenance.dofs} dofs</span>
            <span class="plain">Timing</span>
            <span class="num">
              mesh {lvl.meta.provenance.mesh_ms} ms · solve {lvl.meta.provenance.solve_ms} ms
              {/* thermal_ms is 0 (not absent) on pre-thermal bundles */}
              {(lvl.meta.provenance.thermal_ms ?? 0) > 0 && ` · thermal ${fmt(lvl.meta.provenance.thermal_ms!)} ms`}
            </span>
            {lvl.meta.cases.slice(0, 4).map((c, i) => {
              // The fill re-solve's utilization factors (the fill.rs laws): yield — the
              // max element vM over the fraction-scaled yield, shown when the document
              // states a yield; buckling — the max lattice-element compressive principal
              // over the buckling law's stress. Mechanical slots only (thermal carry
              // null); factors above 1 mark the local over-yield / collapse regime.
              const uf = lvl.meta.yield_utilization?.[i];
              const bf = lvl.meta.buckling_utilization?.[i];
              const caseTemp = lvl.temperature?.[i];
              let caseTLo = Infinity, caseTHi = -Infinity;
              if (caseTemp && caseTemp.length > 0) {
                for (const tv of caseTemp) {
                  if (tv < caseTLo) caseTLo = tv;
                  if (tv > caseTHi) caseTHi = tv;
                }
              }
              const isPureConductionCase = c.max_vm_elem <= 1e-6 && caseTemp && caseTemp.length > 0;
              const isModalOnlyCase = c.name === 'Modal' && c.max_vm_elem <= 1e-6;
              return (
                <Fragment key={c.name}>
                  <span class="plain">{c.name}</span>
                  <span class="num">
                    {isPureConductionCase
                      ? `T ${fmt(caseTLo, 1)}…${fmt(caseTHi, 1)} °C (pure conduction)`
                      : isModalOnlyCase
                        ? `free-vibration reference (${lvl.meta.modes?.frequencies.length ?? 0} modes)`
                        : `${fmt(c.max_vm_elem)} MPa · ${fmt(c.max_displacement)} mm`}
                    {uf != null ? ` · yield ×${fmt(uf, 2)}` : ''}
                    {bf != null ? ` · buckling ×${fmt(bf, 2)}` : ''}
                    {lvl.meta.fill?.lattice_adjoint_sensitivity_sum?.[i] != null && (
                      <span
                        title="Exact 3D P2 self-adjoint compliance sensitivity ∂C/∂ρ_e = −u_eᵀ (∂K_e/∂ρ_e) u_e: total stiffness gain (N·mm compliance reduction) per unit relative density increase ρ_e across lattice elements"
                        style={{ cursor: 'help', borderBottom: '1px dotted rgba(148,163,184,0.5)' }}
                      >
                        {` · Σ(−∂C/∂ρ_e) ${fmt(lvl.meta.fill.lattice_adjoint_sensitivity_sum[i], 2)} N·mm`}
                      </span>
                    )}
                    {c.conduction ? ` · conduction ${c.conduction.iterations} iters` : ''}
                  </span>
                </Fragment>
              );
            })}
          </div>
          {(() => {
            const isClinch = isClinchingDocument(store.doc) || mode === 'plastic_strain';
            if (!isClinch) return null;
            const spec = extractClinchingSpecFromDoc(store.doc);
            const sweepRes = analyzeClinchingProcessWindow(spec);
            const cur = sweepRes.current;
            const nlLet = store.doc.lets?.find((l) => l.name === 'nonlinear_plastic');
            const isNlActive = (nlLet?.value ?? 0) > 0.5;
            const peakVm = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem));
            const peakEpsP = plasticStrainOfVm(peakVm, lvl.meta.yield_strength);

            const setDocLet = (name: string, val: number, label: string) => {
              const hasExisting = store.doc.lets?.some((l) => l.name === name);
              store.transact(label, (d) => {
                if (hasExisting) {
                  const parsed = parseAddr(name);
                  applyAddrs(d, [[parsed, val]], true);
                } else {
                  if (!d.lets) d.lets = [];
                  d.lets.push({ name, expr: String(val), value: val });
                }
                if (name === 'load_angle_deg') {
                  const rad = (val * Math.PI) / 180;
                  for (const ld of d.loads ?? []) {
                    if (ld.kind.type === 'force') {
                      const vx = ld.kind.vector[0];
                      const vy = ld.kind.vector[1];
                      const vz = ld.kind.vector[2];
                      const mag = Math.hypot(vx, vy, vz) || 1200;
                      ld.kind.vector = [
                        Math.round(mag * Math.sin(rad) * 100) / 100,
                        0,
                        Math.round(mag * Math.cos(rad) * 100) / 100,
                      ];
                    }
                  }
                }
              });
            };

            const svgW = 320;
            const svgH = 112;
            const padL = 34;
            const padR = 10;
            const padT = 12;
            const padB = 22;
            const plotW = svgW - padL - padR;
            const plotH = svgH - padT - padB;
            const xLo = 0.35;
            const xHi = 1.60;
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
                  <span style={{ fontWeight: 700, color: '#ddd6fe', fontSize: '11px' }}>
                    🔩 Sheet Metal Clinching & 3D J₂ Elasto-Plasticity Explorer
                  </span>
                  <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
                    <button
                      type="button"
                      class="mini-btn"
                      data-testid="clinch-toggle-nonlinear"
                      onClick={() => setDocLet('nonlinear_plastic', isNlActive ? 0 : 1, isNlActive ? 'Switch to Linear Hookean Solver' : 'Switch to 3D J2 Secant Plasticity Solver')}
                      style={
                        isNlActive
                          ? { background: '#7c3aed', borderColor: '#a78bfa', color: '#fff', fontWeight: 700, fontSize: '9.5px' }
                          : { fontSize: '9.5px' }
                      }
                      title="Toggle between 3D Nonlinear J2 Secant Elasto-Plasticity (E_sec(ε_p) per element) and Linear Hookean Elasticity (E_0)"
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

                {/* Interactive Hat-Curve SVG: F_neck(X), F_pull(X), 1D Lee-Kim Envelope vs 3D Nonlinear J2 FEM */}
                <div
                  style={{
                    background: 'rgba(15, 23, 42, 0.72)',
                    border: '1px solid rgba(148, 163, 184, 0.2)',
                    borderRadius: '5px',
                    padding: '4px',
                  }}
                >
                  <svg viewBox={`0 0 ${svgW} ${svgH}`} style={{ width: '100%', height: 'auto', display: 'block' }}>
                    {/* Safe Process Window Shaded Band [X_min, X_max] */}
                    <rect
                      x={sx(sweepRes.windowRangeMm[0])}
                      y={padT}
                      width={Math.max(2, sx(sweepRes.windowRangeMm[1]) - sx(sweepRes.windowRangeMm[0]))}
                      height={plotH}
                      fill="rgba(16, 185, 129, 0.12)"
                    />
                    <line x1={padL} y1={padT + plotH} x2={svgW - padR} y2={padT + plotH} stroke="#475569" stroke-width="1" />
                    <line x1={padL} y1={padT} x2={padL} y2={padT + plotH} stroke="#475569" stroke-width="1" />
                    {/* F_neck(X) curve (amber dashed) */}
                    <polyline fill="none" stroke="#f59e0b" stroke-width="1.2" stroke-dasharray="3,2" points={neckPts} />
                    {/* F_pull(X) curve (cyan dashed) */}
                    <polyline fill="none" stroke="#38bdf8" stroke-width="1.2" stroke-dasharray="3,2" points={pullPts} />
                    {/* 1D Analytic Lee-Kim Hat-Curve F_joint^1D(X) (emerald solid) */}
                    <polyline fill="none" stroke="#10b981" stroke-width="2" points={joint1dPts} />
                    {/* 3D Nonlinear J2 Elasto-Plastic F_joint^3D-NL(X) (violet solid) */}
                    <polyline fill="none" stroke="#c084fc" stroke-width="1.8" points={joint3dPts} />
                    {/* Current Bottom Coin Thickness X vertical marker */}
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

                {/* Quick Regime Presets & Beyond-Analytic Peel Angle */}
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
                  <span style={{ color: '#cbd5e1', fontSize: '10px' }}>
                    Bottom Coin <strong>X = {fmt(cur.xMm, 2)} mm</strong>:
                  </span>
                  <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
                    {([
                      { x: 0.50, label: '0.50 mm (Over-Clinched)' },
                      { x: 0.85, label: '0.85 mm (Optimal X*)' },
                      { x: 1.25, label: '1.25 mm (Under-Clinched)' },
                    ]).map((pr) => (
                      <button
                        key={pr.x}
                        type="button"
                        class="mini-btn"
                        data-testid={`clinch-preset-x-${pr.x.toFixed(2)}`}
                        onClick={() => setDocLet('X', pr.x, `Set Clinch Bottom Thickness X = ${pr.x} mm`)}
                        style={
                          Math.abs(cur.xMm - pr.x) < 0.04
                            ? { background: 'rgba(16, 185, 129, 0.22)', borderColor: '#34d399', color: '#ecfdf5', fontWeight: 700, fontSize: '9.5px' }
                            : { fontSize: '9.5px' }
                        }
                      >
                        {pr.label}
                      </button>
                    ))}
                  </div>
                </div>

                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
                  <span style={{ color: '#cbd5e1', fontSize: '10px' }}>
                    Beyond-1D Loading Mode (<strong>θ = {fmt(spec.loadAngleDeg, 0)}°</strong>):
                  </span>
                  <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
                    {([
                      { deg: 0, label: '0° Cross-Tension' },
                      { deg: 45, label: '45° Mixed Peel' },
                      { deg: 90, label: '90° Lap-Shear' },
                    ]).map((ld) => (
                      <button
                        key={ld.deg}
                        type="button"
                        class="mini-btn"
                        data-testid={`clinch-angle-${ld.deg}`}
                        onClick={() => setDocLet('load_angle_deg', ld.deg, `Set Clinch Service Load Angle = ${ld.deg}°`)}
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

                {/* Analytic vs 3D Linear vs 3D Nonlinear Elasto-Plastic Readout Grid */}
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
                  <span style={{ color: '#94a3b8' }}>Neck t_n(X) & Undercut t_u^eff</span>
                  <span class="num" style={{ textAlign: 'right', color: '#e2e8f0' }}>
                    t_n = {fmt(cur.neckThicknessMm, 3)} mm · t_u = {fmt(cur.undercutEffMm, 3)} mm (−{fmt(cur.springbackLossMm, 3)} springback)
                  </span>
                  <span style={{ color: '#94a3b8' }}>Notch K_t (Lin) → K_σ (J₂ NL)</span>
                  <span class="num" style={{ textAlign: 'right', color: '#c4b5fd' }}>
                    K_t = {fmt(cur.elasticKt, 2)} → K_σ = {fmt(cur.plasticKsigma, 2)} (n = {fmt(spec.hardeningExponentN, 2)})
                  </span>
                  <span style={{ color: '#94a3b8' }}>3D Peak σ_vM & Plastic ε_p</span>
                  <span class="num" style={{ textAlign: 'right', color: '#6ee7b7' }}>
                    {fmt(peakVm, 1)} MPa · ε_p = {fmt(peakEpsP, 2)}% ({lvl.meta.provenance.iterations} PCG iters)
                  </span>
                </div>
              </div>
            );
          })()}
          {(() => {
            const litSheet = findMatchingLiterateSheet(store.doc.name);
            const isFluidOrCapillary =
              mode === 'flow_field' ||
              (litSheet && ['A15', 'A16', 'A17', 'A18', 'A20', 'T18'].includes(litSheet.id)) ||
              /spurious|eddies|droplet|gerris|capillary|free-surface/i.test(store.doc.name || '');
            if (!isFluidOrCapillary) return null;
            return <SpuriousEddyExplorerPanel compact />;
          })()}
          {(() => {
            const litSheet = findMatchingLiterateSheet(store.doc.name);
            if (!litSheet) return null;
            const checks = litSheet.analyticChecks ?? [];
            const liveAsserts = (litSheet.asserts ?? []).map((a) => evaluateLiterateAssertOnLevel(a, lvl));
            const passCount = liveAsserts.filter((r) => r.passed).length;
            return (
              <div
                data-testid="results-analytic-verification"
                style={{
                  margin: '6px 0',
                  padding: '7px 9px',
                  background: 'rgba(59, 130, 246, 0.08)',
                  border: '1px solid rgba(59, 130, 246, 0.35)',
                  borderRadius: '6px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '5px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}>
                  <span style={{ fontSize: '11px', fontWeight: 700, color: '#93c5fd' }}>
                    📐 Literate .fsm Verification · {litSheet.id} ({litSheet.verificationBadge})
                  </span>
                  <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
                    <button
                      type="button"
                      class="mini-btn"
                      data-testid="results-open-literate-sheet"
                      onClick={() => {
                        ui.set('tutorialSheetId', litSheet.id);
                        ui.set('tutorialMinimised', false);
                        ui.set('tutorialsOpen', true);
                      }}
                    >
                      ? Sheet ({litSheet.id})
                    </button>
                    <button
                      type="button"
                      class="mini-btn"
                      data-testid="results-open-theory-lens"
                      onClick={() => store.setLens('theory')}
                    >
                      6. Theory →
                    </button>
                  </div>
                </div>
                {liveAsserts.length > 0 && (() => {
                  const allPass = passCount === liveAsserts.length;
                  return (
                  <div
                    data-testid="results-live-asserts"
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '3px',
                      padding: '5px 7px',
                      background: allPass ? 'rgba(16, 185, 129, 0.07)' : 'rgba(239, 68, 68, 0.09)',
                      border: allPass ? '1px solid rgba(16, 185, 129, 0.32)' : '1px solid rgba(239, 68, 68, 0.42)',
                      borderRadius: '5px',
                      fontSize: '10.5px',
                    }}
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px' }}>
                      <span style={{ fontWeight: 700, color: allPass ? '#6ee7b7' : '#fca5a5' }}>
                        {allPass ? '✓' : '✗'} Live @assert Contracts ({passCount} / {liveAsserts.length} PASS on {lvl.label})
                      </span>
                      <span class="num" style={{ color: allPass ? '#a7f3d0' : '#fecaca', fontSize: '9.5px' }}>
                        fieldsmith solve --verify
                      </span>
                    </div>
                    {liveAsserts.map((ev, idx) => {
                      const u = ev.assert.unit ? ` ${ev.assert.unit}` : '';
                      const isBoundTarget = /^(<=|>=|≤|≥|<|>)/.test(ev.assert.target.trim());
                      const cleanTol = ev.assert.tolerance.trim().replace(/^(<=|≤)\s*/, '');
                      const parenDetail = isBoundTarget
                        ? `margin ${ev.errorOrMargin}${u}`
                        : `${ev.errorOrMargin}${cleanTol ? ` ≤ ${cleanTol}` : ''}`;
                      return (
                        <div
                          key={idx}
                          data-testid={`live-assert-row-${idx}`}
                          style={{
                            display: 'flex',
                            justifyContent: 'space-between',
                            alignItems: 'center',
                            gap: '6px',
                            flexWrap: 'wrap',
                            padding: '2px 4px',
                            background: 'rgba(15, 23, 42, 0.55)',
                            borderRadius: '3px',
                          }}
                        >
                          <span style={{ color: '#e2e8f0', fontWeight: 500 }}>
                            {ev.assert.label} <code style={{ color: '#94a3b8', fontSize: '9.5px' }}>({ev.assert.metric})</code>
                          </span>
                          <span
                            class="num"
                            style={{
                              color: ev.passed ? '#4ade80' : '#f87171',
                              fontWeight: 600,
                            }}
                          >
                            live {ev.liveValue != null ? fmt(ev.liveValue, 4) : 'N/A'}
                            {u} vs {ev.assert.target}
                            {u} ({parenDetail}) {ev.passed ? 'PASS' : 'FAIL'}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                  );
                })()}
                {checks.length > 0 && (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '10.5px' }}>
                    <span style={{ fontSize: '9.5px', fontWeight: 600, color: '#94a3b8' }}>
                      Static Reference Notes (@check comments in .fsm — see Live @assert above for verified FEA)
                    </span>
                    {checks.slice(0, compact ? 2 : 6).map((chk, idx) => (
                      <div
                        key={idx}
                        style={{
                          padding: '4px 6px',
                          background: 'rgba(15, 23, 42, 0.65)',
                          borderRadius: '4px',
                          border: '1px solid rgba(148, 163, 184, 0.18)',
                          display: 'flex',
                          flexDirection: 'column',
                          gap: '2px',
                        }}
                      >
                        <div style={{ display: 'flex', justifyContent: 'space-between', gap: '6px', flexWrap: 'wrap' }}>
                          <span style={{ fontWeight: 600, color: '#e2e8f0' }}>{chk.quantity}</span>
                          <span class="num" style={{ color: '#94a3b8', fontWeight: 500 }}>
                            doc note: {chk.relError} ({chk.tolerance})
                          </span>
                        </div>
                        <div class="num" style={{ color: '#93c5fd', fontSize: '10px' }}>
                          {chk.formula}
                        </div>
                        <div class="num" style={{ color: '#cbd5e1', fontSize: '10px' }}>
                          Analytic Formula: <strong>{chk.analyticRef}</strong> · Doc Baseline: <strong>{chk.measuredFea}</strong>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })()}
          {!compact && (
            <>
              <div
                data-testid="results-convergence-sweep-card"
                style={{
                  marginTop: '6px',
                  padding: '8px 10px',
                  borderRadius: '6px',
                  background: 'var(--panel-2)',
                  border: '1px solid var(--line)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    gap: '8px',
                    flexWrap: 'wrap',
                    cursor: 'pointer',
                  }}
                  onClick={() => setRungsExpanded((v) => !v)}
                >
                  <div style={{ display: 'flex', flexDirection: 'column', gap: '2px', minWidth: 0, flex: 1 }}>
                    <span style={{ fontSize: '11.5px', fontWeight: 700, color: 'var(--text)' }}>
                      📐 Mesh Convergence (&ldquo;Rungs&rdquo;) — Discretization Error Estimate
                    </span>
                    <span style={{ fontSize: '10.5px', color: 'var(--text-2)', lineHeight: 1.35 }}>
                      A <strong>rung</strong> is one step in an automatic coarse-to-fine mesh refinement sequence (e.g., <code>rung 0</code> coarse preview → <code>rung 1</code> refined mesh) used to estimate how much deflection and stress change as the mesh gets finer.
                    </span>
                  </div>
                  <button
                    type="button"
                    class="mini-btn"
                    data-testid="results-rungs-collapse-toggle"
                    onClick={(e) => {
                      e.stopPropagation();
                      setRungsExpanded((v) => !v);
                    }}
                    style={{ flexShrink: 0, fontWeight: 600 }}
                  >
                    {rungsExpanded ? '▾ Collapse' : `▸ Expand (${s.levels.length} rung${s.levels.length === 1 ? '' : 's'})`}
                  </button>
                </div>

                <div
                  style={{
                    display: rungsExpanded ? 'flex' : 'none',
                    flexDirection: 'column',
                    gap: '8px',
                    paddingTop: '6px',
                    borderTop: '1px solid var(--line)',
                  }}
                >
                  {s.levels.length > 0 && (
                    <div class="field-grid" data-testid="rung-table">
                      <span class="plain">Mesh Refinement Rungs</span>
                      <span class="meta">
                        fieldsmith wasm · one row per mesh refinement step; changes between steps are discretization error estimates, not formal bounds
                      </span>
                      {s.levels.map((lv, i) => {
                        const prevLv = s.levels[i - 1];
                        const ch = i > 0 && prevLv ? changePct(lv, prevLv) : null;
                        const sig = (v: number) => `${v >= 0 ? '+' : ''}${v.toFixed(1)}%`;
                        return (
                          <Fragment key={`${lv.label}·${i}`}>
                            <span class="plain">
                              rung {i} ({i === 0 ? 'coarse' : 'refined'})
                            </span>
                            <span class="num">
                              {lv.label} · {lv.meta.provenance.dofs} dofs · {lv.meta.provenance.elements} elements · wall {fmt(lv.runMs, 0)} ms
                              {ch && <> · case envelope vs rung {i - 1} (estimate): defl {sig(ch.defl)} · vm {sig(ch.peak)}</>}
                            </span>
                          </Fragment>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
              <SweepSection />
              {lvl.meta.cases.some((c) => (c.load_reports ?? []).length > 0) && (
                <div class="field-grid" data-testid="load-provenance">
                  <span class="plain">Loads (applied)</span>
                  <span class="meta">
                    Integrated net force resultant (Fx, Fy, Fz in N) and moment (Mx, My, Mz in N·mm) about the document origin
                  </span>
                  {lvl.meta.cases.flatMap((c) =>
                    (c.load_reports ?? []).map((r: LoadReport) => (
                      <Fragment key={`${c.name}·${r.id}`}>
                        <span class="plain">{r.id} · {r.region}</span>
                        <span class="num">
                          {fmt(r.resultant[0], 1)}, {fmt(r.resultant[1], 1)}, {fmt(r.resultant[2], 1)} N ·
                          M {fmt(r.moment[0], 0)}, {fmt(r.moment[1], 0)}, {fmt(r.moment[2], 0)} N·mm · {r.facets} faces
                          {r.fit_rms != null
                            ? ` · ×${fmt(r.scale, 3)} of ${fmt(r.unscaled, 2)} N`
                            : ''}
                        </span>
                      </Fragment>
                    )),
                  )}
                </div>
              )}
            </>
          )}
          {lvl.meta.modes && (
            <div class="field-grid" data-testid="modes-summary">
              <span class="plain">Modes</span>
              <span class="num">
                {lvl.meta.modes.frequencies.length} natural frequencies · eigen{' '}
                {fmt(lvl.meta.modes.solve_ms, 0)} ms
              </span>
              {lvl.meta.modes.frequencies.slice(0, 6).map((f, i) => (
                <Fragment key={i}>
                  <span class="plain">Mode {i + 1}</span>
                  <span class="num">{fmt(f, 2)} Hz</span>
                </Fragment>
              ))}
              {lvl.modeShapes && (
                <>
                  {/* Full-span row: the Off + 6-mode picker is as wide as the mode chips above. */}
                  <div class="show-row">
                    <Seg<string>
                      value={String(ui.resultsModeShape)}
                      options={[
                        { value: '-1', label: 'Off' },
                        ...lvl.meta.modes.frequencies.slice(0, 6).map((f, i) => ({ value: String(i), label: `${i + 1}: ${fmt(f, 2)} Hz` })),
                      ]}
                      onChange={(v) => {
                        const k = Number.parseInt(v, 10);
                        ui.set('resultsModeShape', k);
                        if (k < 0 && ui.resultsModePlay) ui.set('resultsModePlay', false);
                      }}
                      testId="modes-shape"
                    />
                  </div>
                  {ui.resultsModeShape >= 0 && (
                    <>
                      <span class="plain">Phase</span>
                      <div class="scale-row">
                        {/* Degrees in the panel (a phase is spoken in turns), radians in the
                            knob: 360° multiplies back to exactly 2π within one ulp, and cos
                            differs from 1 only quadratically there, so the 2π build is
                            bit-identical to the 0 build. */}
                        <NumberInput
                          value={Math.round(((ui.resultsModePhase * 180) / Math.PI) * 10) / 10}
                          min={0}
                          max={360}
                          step={5}
                          unit="°"
                          onChange={(d) => ui.set('resultsModePhase', (d * Math.PI) / 180)}
                          testId="modes-phase"
                        />
                        <button
                          type="button"
                          class="mini-btn"
                          data-testid="modes-play"
                          onClick={() => ui.set('resultsModePlay', !ui.resultsModePlay)}
                        >
                          {ui.resultsModePlay ? 'Pause' : 'Play'}
                        </button>
                      </div>
                    </>
                  )}
                </>
              )}
            </div>
          )}
          {lvl.meta.fill && (
            <div class="field-grid" data-testid="fill-summary">
              <span class="plain">Fill</span>
              <span class="num">
                {lvl.meta.fill.family.toLowerCase()} {lvl.meta.fill.mode.toLowerCase()} · cell {fmt(lvl.meta.fill.cell)} · skin {fmt(lvl.meta.fill.skin)} mm
              </span>
              <span class="plain">Achieved design ρ</span>
              <span class="num">
                {fmt(lvl.meta.fill.rho_design[0], 3)}…{fmt(lvl.meta.fill.rho_design[1], 3)}
              </span>
              <span class="plain">Mass</span>
              <span class="num">
                {fmt(lvl.meta.fill.mass_g, 2)} g · solid {fmt(lvl.meta.fill.solid_mass_g, 2)} g (−{fmt(100 * (1 - lvl.meta.fill.mass_g / lvl.meta.fill.solid_mass_g), 0)}%)
              </span>
              {lvl.meta.fill.lattice_adjoint_sensitivity_sum?.[0] != null && (
                <>
                  <span
                    class="plain"
                    title="Exact 3D P2 self-adjoint compliance sensitivity ∂C/∂ρ_e = −u_eᵀ (∂K_e/∂ρ_e) u_e = −(2 U_e)(∂E_e/∂ρ_e)/E_e: total stiffness gain (N·mm compliance reduction) per unit relative density increase ρ_e across lattice elements."
                  >
                    3D Adjoint Σ(−∂C/∂ρ_e)
                  </span>
                  <span class="num" data-testid="results-fill-adjoint-sensitivity">
                    {lvl.meta.fill.lattice_adjoint_sensitivity_sum.map((v) => `${fmt(v, 2)} N·mm`).join(' · ')}
                  </span>
                </>
              )}
              {(lvl.meta.solid_cases ?? []).map((sc, i) => {
                const gc = lvl.meta.cases[i];
                if (!gc || i >= 4) return null;
                const sd = 100 * (gc.max_displacement - sc.max_displacement) / Math.max(1e-9, sc.max_displacement);
                const sv = 100 * (gc.max_vm_elem - sc.max_vm_elem) / Math.max(1e-9, sc.max_vm_elem);
                return (
                  <Fragment key={`f${sc.name}`}>
                    <span class="plain">{sc.name} vs solid</span>
                    <span class="num">
                      deflect {sd >= 0 ? '+' : ''}{fmt(sd, 1)}% · peak {sv >= 0 ? '+' : ''}{fmt(sv, 1)}%
                    </span>
                  </Fragment>
                );
              })}
            </div>
          )}
          {s.skippedFine && <div class="hint" data-testid="skipped-fine">{s.skippedFine}</div>}
          {!ui.probe && <div class="hint" data-testid="probe-hint">Click the surface or sliced section cap to probe full 3D tensor & vector values.</div>}
          {ui.probe && (
            <div class="probe" data-testid="probe">
              {fmt(ui.probe.value)} {ui.probe.unit} at ({ui.probe.point.map((v) => v.toFixed(1)).join(', ')}) mm
            </div>
          )}
          {ui.probe && <ProbeTensorReadoutCard lvl={lvl} ci={ci} mode={mode} />}
          {!compact && <LineProfileExplorerPanel lvl={lvl} ci={ci} mode={mode} />}
            </>
          )}
        </>
      )}
    </div>
  );
}

function findNearestTet(lvl: LevelResult, pt: [number, number, number]): { elemIdx: number; nearNode: number } {
  const nNodes = Math.floor(lvl.nodes.length / 3);
  let bestNode = 0;
  let bestNodeD2 = Infinity;
  for (let n = 0; n < nNodes; n++) {
    const dx = lvl.nodes[3 * n] - pt[0];
    const dy = lvl.nodes[3 * n + 1] - pt[1];
    const dz = lvl.nodes[3 * n + 2] - pt[2];
    const d2 = dx * dx + dy * dy + dz * dz;
    if (d2 < bestNodeD2) {
      bestNodeD2 = d2;
      bestNode = n;
    }
  }
  const tets = lvl.tets;
  if (!tets || tets.length < 4) {
    return { elemIdx: Math.max(0, lastProbedElem), nearNode: bestNode };
  }
  const nTets = Math.floor(tets.length / 4);
  let bestElem = lastProbedElem >= 0 && lastProbedElem < nTets ? lastProbedElem : 0;
  let bestElemD2 = Infinity;
  for (let e = 0; e < nTets; e++) {
    const n0 = tets[4 * e];
    const n1 = tets[4 * e + 1];
    const n2 = tets[4 * e + 2];
    const n3 = tets[4 * e + 3];
    const cx = 0.25 * (lvl.nodes[3 * n0] + lvl.nodes[3 * n1] + lvl.nodes[3 * n2] + lvl.nodes[3 * n3]);
    const cy = 0.25 * (lvl.nodes[3 * n0 + 1] + lvl.nodes[3 * n1 + 1] + lvl.nodes[3 * n2 + 1] + lvl.nodes[3 * n3 + 1]);
    const cz = 0.25 * (lvl.nodes[3 * n0 + 2] + lvl.nodes[3 * n1 + 2] + lvl.nodes[3 * n2 + 2] + lvl.nodes[3 * n3 + 2]);
    const dx = cx - pt[0];
    const dy = cy - pt[1];
    const dz = cz - pt[2];
    const d2 = dx * dx + dy * dy + dz * dz;
    if (d2 < bestElemD2) {
      bestElemD2 = d2;
      bestElem = e;
    }
  }
  return { elemIdx: bestElem, nearNode: bestNode };
}

function ProbeTensorReadoutCard({
  lvl,
  ci,
  mode,
}: {
  lvl: LevelResult;
  ci: number;
  mode: ResultMode;
}) {
  if (!ui.probe) return null;
  const pt = ui.probe.point;
  const { elemIdx, nearNode } =
    lastProbedElem >= 0
      ? { elemIdx: lastProbedElem, nearNode: findNearestTet(lvl, pt).nearNode }
      : findNearestTet(lvl, pt);
  const effCi = ci >= 0 ? ci : 0;
  const stressArr = lvl.stressElem?.[effCi];
  const u = lvl.u[effCi] ?? lvl.u[0];
  const ux = u ? u[3 * nearNode] ?? 0 : 0;
  const uy = u ? u[3 * nearNode + 1] ?? 0 : 0;
  const uz = u ? u[3 * nearNode + 2] ?? 0 : 0;
  const uMag = Math.hypot(ux, uy, uz);
  const vm = (ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope)[elemIdx] ?? 0;
  const tArr = tempField(lvl, ci);
  const tempC = tArr ? tArr[nearNode] : null;
  const hf = tempC != null ? heatFluxField(lvl, ci) : null;

  let sxx = 0, syy = 0, szz = 0, txy = 0, tyz = 0, txz = 0;
  if (stressArr && elemIdx * 6 + 5 < stressArr.length) {
    sxx = stressArr[6 * elemIdx];
    syy = stressArr[6 * elemIdx + 1];
    szz = stressArr[6 * elemIdx + 2];
    txy = stressArr[6 * elemIdx + 3];
    tyz = stressArr[6 * elemIdx + 4];
    txz = stressArr[6 * elemIdx + 5];
  }
  // Cylindrical radial & hoop stresses (r, θ) in the XY plane
  const rXY = Math.hypot(pt[0], pt[1]);
  const cosT = rXY > 1e-6 ? pt[0] / rXY : 1;
  const sinT = rXY > 1e-6 ? pt[1] / rXY : 0;
  const srr = sxx * cosT * cosT + syy * sinT * sinT + 2 * txy * sinT * cosT;
  const stt = sxx * sinT * sinT + syy * cosT * cosT - 2 * txy * sinT * cosT;
  void mode;

  return (
    <div
      data-testid="probe-tensor-card"
      style={{
        marginTop: '4px',
        padding: '6px 8px',
        background: 'rgba(15, 23, 42, 0.72)',
        border: '1px solid rgba(56, 189, 248, 0.35)',
        borderRadius: '5px',
        fontSize: '10px',
        display: 'flex',
        flexDirection: 'column',
        gap: '3px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', color: '#7dd3fc', fontWeight: 700 }}>
        <span>3D Volumetric Probe · Tet #{elemIdx} (Node #{nearNode})</span>
        <span class="num">r = {fmt(rXY, 2)} mm</span>
      </div>
      {stressArr && (
        <div class="num" style={{ color: '#e2e8f0', display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: '2px 6px' }}>
          <span>σ_xx: {fmt(sxx, 2)} MPa</span>
          <span>σ_yy: {fmt(syy, 2)} MPa</span>
          <span>σ_zz: {fmt(szz, 2)} MPa</span>
          <span>τ_xy: {fmt(txy, 2)} MPa</span>
          <span>τ_yz: {fmt(tyz, 2)} MPa</span>
          <span>τ_xz: {fmt(txz, 2)} MPa</span>
          <span style={{ color: '#fde047' }}>σ_vM: {fmt(vm, 2)} MPa</span>
          <span style={{ color: '#93c5fd' }}>σ_rr: {fmt(srr, 2)} MPa</span>
          <span style={{ color: '#c4b5fd' }}>σ_θθ: {fmt(stt, 2)} MPa</span>
        </div>
      )}
      <div class="num" style={{ color: '#cbd5e1', display: 'flex', justifyContent: 'space-between', flexWrap: 'wrap', gap: '4px' }}>
        <span>
          u = ({fmt(ux, 4)}, {fmt(uy, 4)}, {fmt(uz, 4)}) mm · |u| = {fmt(uMag, 4)} mm
        </span>
        {tempC != null && (
          <span style={{ color: '#fdba74' }}>
            T = {fmt(tempC, 2)} °C
            {hf ? ` · |q| = ${fmt(hf.magNode[nearNode], 2)} kW/m²` : ''}
          </span>
        )}
      </div>
    </div>
  );
}

type LineAxisPreset = 'x' | 'y' | 'z' | 'radial' | 'probed';
type LineMetricId = 'active' | 'sxx' | 'syy' | 'szz' | 'srr' | 'stt' | 'disp' | 'temp';

function LineProfileExplorerPanel({
  lvl,
  ci,
  mode,
}: {
  lvl: LevelResult;
  ci: number;
  mode: ResultMode;
}) {
  const [open, setOpen] = useState(false);
  const [axisPreset, setAxisPreset] = useState<LineAxisPreset>('x');
  const [metricId, setMetricId] = useState<LineMetricId>('active');

  let lx = Infinity, hx = -Infinity, ly = Infinity, hy = -Infinity, lz = Infinity, hz = -Infinity;
  for (let i = 0; i < lvl.nodes.length; i += 3) {
    lx = Math.min(lx, lvl.nodes[i]); hx = Math.max(hx, lvl.nodes[i]);
    ly = Math.min(ly, lvl.nodes[i + 1]); hy = Math.max(hy, lvl.nodes[i + 1]);
    lz = Math.min(lz, lvl.nodes[i + 2]); hz = Math.max(hz, lvl.nodes[i + 2]);
  }
  const cx = 0.5 * (lx + hx);
  const cy = 0.5 * (ly + hy);
  const cz = 0.5 * (lz + hz);

  let p0: [number, number, number] = [lx, cy, cz];
  let p1: [number, number, number] = [hx, cy, cz];
  let axisLabel = 'x (mm)';
  if (axisPreset === 'y') {
    p0 = [cx, ly, cz];
    p1 = [cx, hy, cz];
    axisLabel = 'y (mm)';
  } else if (axisPreset === 'z') {
    p0 = [cx, cy, lz];
    p1 = [cx, cy, hz];
    axisLabel = 'z (mm)';
  } else if (axisPreset === 'radial') {
    p0 = [Math.max(0, lx), 0, cz];
    p1 = [hx, 0, cz];
    axisLabel = 'r (mm)';
  } else if (axisPreset === 'probed' && probeHistory.length >= 2) {
    p0 = probeHistory[probeHistory.length - 2];
    p1 = probeHistory[probeHistory.length - 1];
    axisLabel = 's (mm)';
  }

  const nSamples = 25;
  const effCi = ci >= 0 ? ci : 0;
  const stressArr = lvl.stressElem?.[effCi];
  const u = shownDisp(lvl).u;
  const tArr = tempField(lvl, ci);

  const samples: { coord: number; val: number }[] = [];
  let unit = '';
  for (let i = 0; i < nSamples; i++) {
    const frac = i / (nSamples - 1);
    const sx = p0[0] + frac * (p1[0] - p0[0]);
    const sy = p0[1] + frac * (p1[1] - p0[1]);
    const sz = p0[2] + frac * (p1[2] - p0[2]);
    const { elemIdx, nearNode } = findNearestTet(lvl, [sx, sy, sz]);
    const coord =
      axisPreset === 'x' || axisPreset === 'radial'
        ? sx
        : axisPreset === 'y'
          ? sy
          : axisPreset === 'z'
            ? sz
            : frac * Math.hypot(p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);

    let val = 0;
    if (metricId === 'active') {
      const ev = elementScalarValue(lvl, mode, ci, elemIdx, u);
      val = Number.isFinite(ev.value) ? ev.value : 0;
      unit = ev.unit;
    } else if (metricId === 'disp') {
      val = Math.hypot(u[3 * nearNode] ?? 0, u[3 * nearNode + 1] ?? 0, u[3 * nearNode + 2] ?? 0);
      unit = 'mm';
    } else if (metricId === 'temp') {
      val = tArr ? tArr[nearNode] ?? 0 : 0;
      unit = '°C';
    } else if (stressArr && elemIdx * 6 + 5 < stressArr.length) {
      const sxx = stressArr[6 * elemIdx];
      const syy = stressArr[6 * elemIdx + 1];
      const szz = stressArr[6 * elemIdx + 2];
      const txy = stressArr[6 * elemIdx + 3];
      const rXY = Math.hypot(sx, sy);
      const cosT = rXY > 1e-6 ? sx / rXY : 1;
      const sinT = rXY > 1e-6 ? sy / rXY : 0;
      if (metricId === 'sxx') val = sxx;
      else if (metricId === 'syy') val = syy;
      else if (metricId === 'szz') val = szz;
      else if (metricId === 'srr') val = sxx * cosT * cosT + syy * sinT * sinT + 2 * txy * sinT * cosT;
      else if (metricId === 'stt') val = sxx * sinT * sinT + syy * cosT * cosT - 2 * txy * sinT * cosT;
      unit = 'MPa';
    }
    samples.push({ coord, val });
  }

  let vMin = Infinity, vMax = -Infinity;
  for (const s of samples) {
    if (s.val < vMin) vMin = s.val;
    if (s.val > vMax) vMax = s.val;
  }
  if (!Number.isFinite(vMin)) { vMin = 0; vMax = 1; }
  const span = Math.max(1e-6, vMax - vMin);
  const svgW = 300, svgH = 86, padL = 34, padR = 8, padT = 8, padB = 18;
  const plotW = svgW - padL - padR;
  const plotH = svgH - padT - padB;
  const polyPts = samples
    .map((s, idx) => {
      const px = padL + (idx / (nSamples - 1)) * plotW;
      const py = padT + plotH - ((s.val - vMin) / span) * plotH;
      return `${px.toFixed(1)},${py.toFixed(1)}`;
    })
    .join(' ');

  return (
    <div
      data-testid="results-line-plot-panel"
      style={{
        marginTop: '6px',
        padding: '6px 8px',
        background: 'var(--panel-2)',
        border: '1px solid var(--line)',
        borderRadius: '6px',
        fontSize: '10.5px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px' }}>
        <span style={{ fontWeight: 700, color: 'var(--text)' }}>📈 1D Line Profile Explorer (Plot Over Line)</span>
        <button
          type="button"
          class="mini-btn"
          data-testid="results-toggle-line-plot"
          onClick={() => setOpen(!open)}
        >
          {open ? 'Hide ▴' : 'Plot Over Line ▾'}
        </button>
      </div>
      {open && (
        <div style={{ marginTop: '6px', display: 'flex', flexDirection: 'column', gap: '5px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: '4px', flexWrap: 'wrap' }}>
            <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
              {([
                { id: 'x', label: 'X-Axis' },
                { id: 'y', label: 'Y-Axis' },
                { id: 'z', label: 'Z-Axis' },
                { id: 'radial', label: 'Radial r' },
                { id: 'probed', label: '2 Probed Pts' },
              ] as { id: LineAxisPreset; label: string }[]).map((ax) => (
                <button
                  key={ax.id}
                  type="button"
                  class="mini-btn"
                  data-testid={`line-axis-${ax.id}`}
                  onClick={() => setAxisPreset(ax.id)}
                  style={axisPreset === ax.id ? { borderColor: '#38bdf8', color: '#e0f2fe', fontWeight: 700 } : undefined}
                >
                  {ax.label}
                </button>
              ))}
            </div>
            <div style={{ display: 'flex', gap: '3px', flexWrap: 'wrap' }}>
              {([
                { id: 'active', label: 'Active' },
                { id: 'sxx', label: 'σ_xx' },
                { id: 'syy', label: 'σ_yy' },
                { id: 'szz', label: 'σ_zz' },
                { id: 'srr', label: 'σ_rr' },
                { id: 'stt', label: 'σ_θθ' },
                { id: 'disp', label: '|u|' },
                { id: 'temp', label: 'T' },
              ] as { id: LineMetricId; label: string }[]).map((m) => (
                <button
                  key={m.id}
                  type="button"
                  class="mini-btn"
                  data-testid={`line-metric-${m.id}`}
                  onClick={() => setMetricId(m.id)}
                  style={metricId === m.id ? { borderColor: '#a78bfa', color: '#f3e8ff', fontWeight: 700 } : undefined}
                >
                  {m.label}
                </button>
              ))}
            </div>
          </div>
          <svg viewBox={`0 0 ${svgW} ${svgH}`} style={{ width: '100%', height: 'auto', background: 'rgba(15, 23, 42, 0.7)', borderRadius: '4px' }}>
            <line x1={padL} y1={padT + plotH} x2={svgW - padR} y2={padT + plotH} stroke="#475569" stroke-width="1" />
            <line x1={padL} y1={padT} x2={padL} y2={padT + plotH} stroke="#475569" stroke-width="1" />
            <polyline fill="none" stroke="#38bdf8" stroke-width="1.8" points={polyPts} />
            <text x={3} y={padT + 7} fill="#94a3b8" font-size="7.5">{fmt(vMax, 2)}</text>
            <text x={3} y={padT + plotH} fill="#94a3b8" font-size="7.5">{fmt(vMin, 2)}</text>
            <text x={padL} y={svgH - 4} fill="#94a3b8" font-size="7.5">{fmt(samples[0].coord, 1)}</text>
            <text x={svgW - padR} y={svgH - 4} fill="#94a3b8" font-size="7.5" text-anchor="end">
              {fmt(samples[samples.length - 1].coord, 1)} {axisLabel}
            </text>
          </svg>
          <div class="num" style={{ display: 'flex', justifyContent: 'space-between', color: '#94a3b8', fontSize: '9.5px' }}>
            <span>Min: <strong>{fmt(vMin, 3)} {unit}</strong></span>
            <span>Max: <strong>{fmt(vMax, 3)} {unit}</strong></span>
            <span>Start→End: {fmt(samples[0].val, 2)} → {fmt(samples[samples.length - 1].val, 2)} {unit}</span>
          </div>
        </div>
      )}
    </div>
  );
}
viewOverlays.push(ResultsPanel);