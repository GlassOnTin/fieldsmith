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
import { addressableParams } from '../doc/param';
import { analysis } from './analysis';
import { sweep, useSweep, computeParetoIndices, metricValueOf } from './sweep';
import type { ParetoMetric, SweepPointRow } from './sweep';
import { fmt } from './fields';
import * as THREE from 'three';
import type { LoadReport } from '../doc/wasm';
import type { LevelResult } from '../doc/wasm';

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

/** Quantity of one facet for the current display mode. `u` is the displacement the shown
 * mesh carries (the static field, or the mode shape while one animates — shownDisp). */
function facetValue(lvl: LevelResult, mode: ResultMode, ci: number, f: number, u: Float64Array): { value: number; unit: string } {
  const owner = lvl.surfaceOwner[f];
  if (mode === 'deflection' || mode === 'vectors') {
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
  if (mode === 'peel') {
    const peel = (ci >= 0 ? lvl.peelElem?.[ci] : lvl.peelElemEnvelope)?.[owner] ?? 0;
    return { value: peel, unit: '×' };
  }
  const vm = (ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope)[owner];
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
  return Math.max(
    1e-9,
    ...(ci >= 0 ? [cases[ci].max_vm_elem] : cases.map((c) => c.max_vm_elem)),
  );
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
  if (mode === 'deflection') return { min: '0', max: `${fmt(max)} mm`, caption: 'displacement magnitude' };
  if (mode === 'temperature') {
    const have = !!tempField(lvl, ci);
    const which = ci >= 0 ? 'nodal' : 'per-node max across thermal cases, nodal';
    return have
      ? { min: '', max: '', caption: `temperature, ${which} (°C)` }
      : { min: '', max: '', caption: 'no temperature field in this run — add a temperature statement' };
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
  const unit = mode === 'stress' ? 'MPa' : mode === 'peel' ? '×' : mode === 'temperature' ? '°C' : 'mm';
  const levelKey = `${analysis.state.resultsFor ?? ''}:${lvl.label}`;
  const r = ui.resultsRange;
  if (r && r.mode === mode && r.levelKey === levelKey) {
    return { lo: r.lo, hi: Math.max(r.hi, r.lo + 1e-12), auto: false, levelKey, unit };
  }
  return { lo: modeMin(lvl, mode, ci), hi: modeMax(lvl, mode, ci), auto: true, levelKey, unit };
}

function isRamped(m: ResultMode): m is 'stress' | 'peel' | 'deflection' | 'temperature' {
  return m === 'stress' || m === 'peel' || m === 'deflection' || m === 'temperature';
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
let resultSubTrisPerFacet = 1;
let lastFacetPositions: Float32Array | null = null;
let lastFacetColors: Float32Array | null = null;
let resultSig = '';

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

const MODE_NAMES: Record<ResultMode, string> = {
  stress: 'stress',
  safety: 'safety',
  peel: 'peel',
  deflection: 'deflection',
  vectors: 'vectors',
  temperature: 'temperature',
};

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
  const tField = mode === 'temperature' ? tempField(lvl, ci) : null;
  const hasScalarField = mode !== 'vectors' && (mode !== 'temperature' || !!tField);

  // Deformed positions for all P2 nodes (corners + mid-edge nodes)
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

  // Ridge-Aligned Bevel/Fillet Surface Fairing:
  // On coarse BCC meshes where `round` fillet radius < grid spacing h (e.g. T12 round=2),
  // BCC corner and centre planes alternate along curved edges, creating a sawtooth zig-zag.
  // Fair curved/bevel corner nodes (nodeMinDot < 0.992) along the bevel ridge while keeping
  // flat planar interiors (nodeMinDot >= 0.992) untouched.
  const visNodes = new Float32Array(defNodes);
  if (useP2 && edgeA.length > 0) {
    const tmpNodes = new Float32Array(visNodes.length);
    for (let pass = 0; pass < 4; pass++) {
      tmpNodes.set(visNodes);
      for (let n = 0; n < nNodes; n++) {
        // Only fair corner nodes on curved/bevel features (leave flat faces exact)
        if (nodeMinDot[n] >= 0.992) continue;
        const nbrs = cornerAdj[n];
        if (nbrs.length < 2) continue;
        let sx = 0, sy = 0, sz = 0, sw = 0;
        const nx = nodeMeanNorm[3 * n];
        const ny = nodeMeanNorm[3 * n + 1];
        const nz = nodeMeanNorm[3 * n + 2];
        for (let k = 0; k < nbrs.length; k++) {
          const nb = nbrs[k];
          // Prefer fellow ridge/bevel neighbours with similar normal orientation
          const isCurvedNb = nodeMinDot[nb] < 0.992;
          const nDot = Math.max(
            0,
            nx * nodeMeanNorm[3 * nb] +
              ny * nodeMeanNorm[3 * nb + 1] +
              nz * nodeMeanNorm[3 * nb + 2],
          );
          const w = isCurvedNb ? 1.5 + 2.5 * nDot : 0.25;
          sx += w * tmpNodes[3 * nb];
          sy += w * tmpNodes[3 * nb + 1];
          sz += w * tmpNodes[3 * nb + 2];
          sw += w;
        }
        if (sw > 1e-12) {
          const alpha = 0.48;
          visNodes[3 * n] = (1 - alpha) * tmpNodes[3 * n] + alpha * (sx / sw);
          visNodes[3 * n + 1] = (1 - alpha) * tmpNodes[3 * n + 1] + alpha * (sy / sw);
          visNodes[3 * n + 2] = (1 - alpha) * tmpNodes[3 * n + 2] + alpha * (sz / sw);
        }
      }
    }
    // Update quadratic mid-side nodes to sit smoothly along the faired edges
    for (let e = 0; e < edgeA.length; e++) {
      const na = edgeA[e];
      const nb = edgeB[e];
      const nm = edgeMid[e];
      for (let c = 0; c < 3; c++) {
        const origMid = defNodes[3 * nm + c];
        const origLin = 0.5 * (defNodes[3 * na + c] + defNodes[3 * nb + c]);
        const newLin = 0.5 * (visNodes[3 * na + c] + visNodes[3 * nb + c]);
        // Keep genuine quadratic arch on smooth bores while damping sawtooth linear jumps
        const arch = origMid - origLin;
        visNodes[3 * nm + c] = newLin + 0.35 * arch;
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

  // ParaView / VTK-style Smooth P2 Volumetric Ray-Marched Iso-Shells & Glowing Scalar Cloud:
  // Uses the 4-subtriangle P2 geometry with per-vertex Hermite-feathered additive emission
  // so iso-contours fade smoothly to zero inside sub-triangles with zero jagged facet steps.
  if (ui.resultsVolumeOn && nFacets > 0 && hasScalarField) {
    let cx = 0, cy = 0, cz = 0;
    for (let n = 0; n < nNodes; n++) {
      cx += visNodes[3 * n];
      cy += visNodes[3 * n + 1];
      cz += visNodes[3 * n + 2];
    }
    cx /= Math.max(1, nNodes);
    cy /= Math.max(1, nNodes);
    cz /= Math.max(1, nNodes);

    const thresholds = [0.20, 0.38, 0.56, 0.72, 0.86];
    const shellScales = [0.97, 0.93, 0.88, 0.82, 0.75];
    const subTriLocal = [
      0, 3, 5,
      3, 1, 4,
      5, 4, 2,
      3, 4, 5,
    ];
    for (let sIdx = 0; sIdx < thresholds.length; sIdx++) {
      const thr = thresholds[sIdx];
      const shScale = shellScales[sIdx];
      const hotPos: number[] = [];
      const hotCol: number[] = [];
      for (let f = 0; f < nFacets; f++) {
        const fBase = 6 * f;
        for (let st = 0; st < 4; st++) {
          const na = lvl.surfaceFacets[fBase + subTriLocal[3 * st]];
          const nb = lvl.surfaceFacets[fBase + subTriLocal[3 * st + 1]];
          const nc = lvl.surfaceFacets[fBase + subTriLocal[3 * st + 2]];
          const sa = useP2 ? nodeNorm[na] : facetScalars[f];
          const sb = useP2 ? nodeNorm[nb] : facetScalars[f];
          const sc = useP2 ? nodeNorm[nc] : facetScalars[f];
          if (sa < thr && sb < thr && sc < thr) continue;
          const triNodes = [na, nb, nc];
          const triScalars = [sa, sb, sc];
          for (let j = 0; j < 3; j++) {
            const nj = triNodes[j];
            const sv = triScalars[j];
            const w = Math.max(0, Math.min(1, (sv - thr) / Math.max(0.08, 1 - thr)));
            const feather = w * w * (3 - 2 * w);
            rampColor(Math.min(1, Math.max(thr, sv) * 0.92 + 0.08), col, lutRamp);
            const px = cx + (visNodes[3 * nj] - cx) * shScale;
            const py = cy + (visNodes[3 * nj + 1] - cy) * shScale;
            const pz = cz + (visNodes[3 * nj + 2] - cz) * shScale;
            hotPos.push(px, py, pz);
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
            opacity: 0.18 + 0.05 * sIdx,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
          }),
        );
        sMesh.renderOrder = 3 + sIdx;
        resultGroup.add(sMesh);
      }
    }
  }

  if (mode === 'vectors') {
    const diag = modelDiag(lvl) || 40;
    const stride = Math.max(1, Math.floor(nFacets / 180));
    for (let f = 0; f < nFacets; f += stride) {
      let ux = 0, uy = 0, uz = 0;
      let px = 0, py = 0, pz = 0;
      for (let j = 0; j < 3; j++) {
        const n = lvl.surfaceFacets[6 * f + j];
        ux += u[3 * n]; uy += u[3 * n + 1]; uz += u[3 * n + 2];
        px += lvl.nodes[3 * n] + scale * u[3 * n];
        py += lvl.nodes[3 * n + 1] + scale * u[3 * n + 1];
        pz += lvl.nodes[3 * n + 2] + scale * u[3 * n + 2];
      }
      ux /= 3; uy /= 3; uz /= 3;
      const mag = Math.hypot(ux, uy, uz);
      if (mag < 0.02) continue;
      const len = Math.min(0.15 * diag, Math.max(0.6, mag * scale * 0.25));
      const arrow = new THREE.ArrowHelper(
        new THREE.Vector3(ux / mag, uy / mag, uz / mag),
        new THREE.Vector3(px / 3, py / 3, pz / 3),
        len,
        0xffffff,
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

function playTick(t: number) {
  const owner = analysis.latest;
  const modeAnim = ui.resultsModePlay && owner && animatingShape(owner) !== null;
  const rampAnim = ui.resultsRampPlay && owner && animatingShape(owner) === null;
  if (store.lens !== 'results' || (!modeAnim && !rampAnim)) {
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
  const modeWant = store.lens === 'results' && lvl && ui.resultsModePlay && animatingShape(lvl) !== null;
  const rampWant = store.lens === 'results' && lvl && ui.resultsRampPlay && animatingShape(lvl) === null;
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
  const nextShow = store.lens !== 'results';
  if (vp.showField !== nextShow) {
    vp.showField = nextShow;
    vp.invalidate();
  }
  const lvl = analysis.latest;
  // A stale animation knob (a new solve carried fewer or no shapes) must not animate on
  // the wrong mesh: reset before the sig, and let this pass's emit re-run sync.
  if (ui.resultsModeShape >= 0 && (!lvl || !shapeOf(lvl, ui.resultsModeShape))) {
    ui.set('resultsModeShape', -1);
    ui.set('resultsModePlay', false);
    ui.set('resultsModePhase', 0);
    return;
  }
  ensureTicker(lvl);
  const on = store.lens === 'results' && !!lvl;
  const sig = on
    ? `${lvl && lvl.label}|${lvl && lvl.nodes.length}${lvl && lvl.meta.provenance.total_ms}|${ui.resultsMode}|${ui.resultsColormap}|${ui.resultsInterp}|${ui.resultsVolumeOn ? 'vol' : 'surf'}|${ui.resultsCase}|${ui.resultsScale}|${ui.resultsRamp}|${
        ui.resultsRange ? `${ui.resultsRange.mode}/${ui.resultsRange.levelKey}/${ui.resultsRange.lo}/${ui.resultsRange.hi}` : '-'
      }|${ui.resultsModeShape}|${ui.resultsModePhase}|${ui.resultsModePlay ? 'p' : 's'}|${ui.resultsRampPlay ? 'rp' : 'rs'}`
    : 'off';
  if (sig !== resultSig) {
    resultSig = sig;
    while (resultGroup.children.length) {
      const c = resultGroup.children.pop()!;
      disposeDeep(c);
    }
    resultMesh = null;
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
  // shader: hand every overlay material the viewport's persistent plane (stable identity,
  // so this costs a traverse, not a recompile). It runs after the build above so a fresh
  // material picks up the plane in the same pass, and after clip off (planes → null).
  const planes = vp.clipPlanes();
  let clipped = false;
  resultGroup.traverse((c) => {
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
  const hits = raycaster.intersectObject(resultMesh, false);
  if (hits.length === 0) {
    if (ui.probe) ui.set('probe', null);
    return true;
  }
  const rawFace = hits[0].faceIndex ?? -1;
  const f = resultSubTrisPerFacet > 1 ? Math.floor(rawFace / resultSubTrisPerFacet) : rawFace;
  if (f < 0 || f >= lvl.surfaceRegion.length) return false;
  const shown = shownDisp(lvl);
  const { value, unit } = facetValue(lvl, shown.animating ? 'deflection' : ui.resultsMode, effCase(lvl), f, shown.u);
  const p = hits[0].point;
  ui.set('probe', { value, unit, point: [p.x, p.y, p.z] });
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

function AnalysisBadge() {
  useApp();
  const s = analysis.state;
  if (s.status === 'idle') return null;
  if (s.status === 'running') {
    return (
      <span class="num" data-testid="badge-running">
        analysing{s.levelLabel ? ` · ${s.levelLabel}` : ''}
      </span>
    );
  }
  if (s.status === 'error') {
    // An incomplete study (no support or no load yet) is a hint, not a failure — show
    // the message neutrally instead of the red 'analysis failed'.
    if (s.errorKind === 'setup') {
      return (
        <span class="num" data-testid="setup-block" title={s.error ?? ''}>
          {s.error}
        </span>
      );
    }
    return (
      <span class="num bad" data-testid="badge-error" title={s.error ?? ''}>
        analysis failed
      </span>
    );
  }
  const lvl = s.levels[s.levels.length - 1];
  const prev = s.levels[s.levels.length - 2];
  if (!lvl) return null;
  // In the fill lens the fill badge shows these numbers plus mass — avoid the duplicate.
  if (store.lens === 'fill' && lvl.meta.fill) return null;
  const { peak, defl } = envelopes(lvl);
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
    <span class="num" data-testid="analysis-badge" title={title}>
      peak {fmt(peak)} MPa · deflect {fmt(defl)} mm
    </span>
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
        <span class="plain">Sweep</span>
        <span class="meta">the ladder question at each value, each on a copy of the document — the document keeps its values</span>
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
  const peakVm = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_vm_elem] : lvl.meta.cases.map((c) => c.max_vm_elem)));
  const peakDefl = Math.max(1e-9, ...(ci >= 0 ? [lvl.meta.cases[ci].max_displacement] : lvl.meta.cases.map((c) => c.max_displacement)));
  const ramp = ui.resultsRamp;
  const playing = isModeAnim ? ui.resultsModePlay : ui.resultsRampPlay;

  const milestones = isModeAnim
    ? [
        { label: '1. +Peak (0°)', val: 0, title: 'Positive displacement crest (cos = +1.0)' },
        { label: '2. Node (90°)', val: Math.PI * 0.5, title: 'Zero-crossing nodal equilibrium (cos = 0.0)' },
        { label: '3. −Peak (180°)', val: Math.PI, title: 'Negative displacement trough (cos = −1.0)' },
        { label: '4. Full Cycle (360°)', val: 0, title: 'Complete harmonic oscillation cycle' },
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
            {isModeAnim
              ? `Phase ${Math.round((ui.resultsModePhase * 180) / Math.PI)}°`
              : `Load ${(ramp * 100).toFixed(0)}% · ${fmt(peakVm * ramp, 1)} MPa`}
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

function ResultsPanel() {
  useApp();
  const [compact, setCompact] = useState(false);
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
  const lvl = s.levels[s.levels.length - 1];
  const shown = lvl ? shownDisp(lvl) : null;
  const animating = shown?.animating ?? false;
  const ci = lvl ? effCase(lvl) : -1;
  const mode: ResultMode = animating ? 'deflection' : ui.resultsMode;
  const hi = animating && shown ? shown.rampMax : lvl ? modeMax(lvl, mode, ci) : 0;
  const legend = lvl ? legendLabels(lvl, mode, ci, hi, lvl.meta.yield_strength) : null;
  if (legend && animating) legend.caption = shown?.caption ?? '';
  const rng = lvl ? shownRange(lvl, shown!) : null;
  const peakVmSummary = lvl ? Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem)) : 0;
  const peakDeflSummary = lvl ? Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement)) : 0;
  // Arrow-key step inside the legend inputs: a tenth of the top bound, on the input's decade.
  const rangeStep = rng ? Math.pow(10, Math.floor(Math.log10(Math.max(1e-9, rng.hi))) - 1) : 1;
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
              <span>{fmt(peakVmSummary)} MPa · {fmt(peakDeflSummary)} mm</span>
            </span>
          )}
          {minimized && !lvl && s.status === 'running' && (
            <span class="meta">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</span>
          )}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }} onClick={(e) => e.stopPropagation()}>
          {!minimized && lvl && <span class="meta">{lvl.label}</span>}
          {!minimized && lvl && (
            <button
              type="button"
              class="mini-btn"
              data-testid="results-compact-toggle"
              title="Toggle 5S Compact Summary vs Full Rungs & Sweep View"
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
          {s.status === 'error' && (s.errorKind === 'setup' ? <div class="hint">{s.error}</div> : <div class="error-box">{s.error}</div>)}
          {s.status === 'running' && <div class="hint">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</div>}
          {!lvl && s.status === 'ready' && <div class="hint">No results yet.</div>}
          {lvl && (
        <>
          <div class="field-grid">
            {/* Full-span row: five mode chips need the whole row width — with the 42px
                label+gap the seg stopped at 282px against 307px of chip text and the last
                chip cropped in the 2026-10-14 e2e. The chips are self-descriptive, so the
                unlabelled seg is the smaller loss. */}
            <div class="show-row">
              <Seg<ResultMode>
                value={mode}
                options={(Object.keys(MODE_NAMES) as ResultMode[]).map((m) => ({ value: m, label: m[0].toUpperCase() + m.slice(1) }))}
                onChange={(v) => {
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
            <span class="plain">Peak von Mises</span>
            <span class="num">{fmt(Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem)))} MPa</span>
            <span class="plain">Max deflection</span>
            <span class="num">{fmt(Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement)))} mm</span>
            {lvl.meta.max_layer_peel && lvl.meta.max_layer_peel.length > 0 && (
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
              return (
                <Fragment key={c.name}>
                  <span class="plain">{c.name}</span>
                  <span class="num">
                    {fmt(c.max_vm_elem)} MPa · {fmt(c.max_displacement)} mm
                    {uf != null ? ` · yield ×${fmt(uf, 2)}` : ''}
                    {bf != null ? ` · buckling ×${fmt(bf, 2)}` : ''}
                    {c.conduction ? ` · conduction ${c.conduction.iterations} iters` : ''}
                  </span>
                </Fragment>
              );
            })}
          </div>
          {!compact && s.levels.length > 0 && (
            <div class="field-grid" data-testid="rung-table">
              <span class="plain">Rungs</span>
              <span class="meta">fieldsmith wasm · one row per rung; changes are rung-to-rung estimates, not bounds</span>
              {s.levels.map((lv, i) => {
                const prevLv = s.levels[i - 1];
                const ch = i > 0 && prevLv ? changePct(lv, prevLv) : null;
                const sig = (v: number) => `${v >= 0 ? '+' : ''}${v.toFixed(1)}%`;
                return (
                  <Fragment key={`${lv.label}·${i}`}>
                    <span class="plain">rung {i}</span>
                    <span class="num">
                      {lv.label} · {lv.meta.provenance.dofs} dofs · {lv.meta.provenance.elements} elements · wall {fmt(lv.runMs, 0)} ms
                      {ch && <> · case envelope vs rung {i - 1} (estimate): defl {sig(ch.defl)} · vm {sig(ch.peak)}</>}
                    </span>
                  </Fragment>
                );
              })}
            </div>
          )}
          {!compact && <SweepSection />}
          {!compact && lvl.meta.cases.some((c) => (c.load_reports ?? []).length > 0) && (
            <div class="field-grid" data-testid="load-provenance">
              <span class="plain">Loads (applied)</span>
              <span class="meta">resultant and moment about the document origin</span>
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
          {!ui.probe && <div class="hint" data-testid="probe-hint">Click the surface to read the value at that point.</div>}
          {ui.probe && (
            <div class="probe" data-testid="probe">
              {fmt(ui.probe.value)} {ui.probe.unit} at ({ui.probe.point.map((v) => v.toFixed(1)).join(', ')}) mm
            </div>
          )}
            </>
          )}
        </>
      )}
    </div>
  );
}
viewOverlays.push(ResultsPanel);