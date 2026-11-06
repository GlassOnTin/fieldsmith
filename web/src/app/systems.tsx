// Coupled Engineering Systems Workbench (V52):
// Interactive multi-physics PDE/ODE solver, 3D Three.js Multi-Physics Digital Twin,
// 2D float-bath field visualiser, main 3D viewport multi-physics overlay,
// sparse Adjoint design sensitivity explorer, and epistemic uncertainty (p10/p50/p90) workbench.
import { useEffect, useRef, useState } from 'preact/hooks';
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import {
  listCoupledSystems,
  solveCoupledSystem,
  type CandleFlameVisual,
  type CandleTransientFrame,
  type CoupledDistortionPoint,
  type CoupledSolveBundle,
  type CoupledSystemSummary,
  type FloatBathVisualFields,
  type SimulationRecapEvent,
} from '../doc/wasm';
import { store } from '../doc/store';
import { detectCoupledPhysicsRegime } from '../doc/continuum';
import { viewOverlays } from './App';
import { extraCommands, openText, viewApi } from './commands';
import { apiUrl, ui, useApp } from './ui';
import { viewportExtensions, liveViewport } from './ViewportHost';
import type { Viewport } from '../view/viewport';

export type BathFieldMode =
  | 'glass_temp'
  | 'u_tin'
  | 'tin_temp'
  | 'redox_film'
  | 'optical_waves'
  | 'bath_depth'
  | 'roof_temp';

const BATH_FIELD_TABS: { id: BathFieldMode; label: string; shortLabel: string; unit: string }[] = [
  { id: 'glass_temp', label: 'Glass T_g', shortLabel: 'T_g', unit: '°C' },
  { id: 'u_tin', label: 'Tin Velocity u_Sn', shortLabel: 'u_Sn', unit: 'm/s' },
  { id: 'tin_temp', label: 'Tin T_Sn', shortLabel: 'T_Sn', unit: '°C' },
  { id: 'redox_film', label: 'Redox Film δ_film', shortLabel: 'δ_film', unit: 'μm' },
  { id: 'optical_waves', label: 'Optical Waves P_opt', shortLabel: 'Waves', unit: 'Mdpt' },
  { id: 'bath_depth', label: 'Bath Depth H', shortLabel: 'Depth H', unit: 'mm' },
  { id: 'roof_temp', label: 'Roof T_r', shortLabel: 'T_r', unit: '°C' },
];

/** Colour ramp helper: t in [0, 1] -> [r, g, b] (0..255). */
function turboRamp(t: number): [number, number, number] {
  const x = Math.max(0, Math.min(1, t));
  const r = Math.round(255 * Math.min(1, Math.max(0, 1.5 - Math.abs(4 * x - 3))));
  const g = Math.round(255 * Math.min(1, Math.max(0, 1.5 - Math.abs(4 * x - 2))));
  const b = Math.round(255 * Math.min(1, Math.max(0, 1.5 - Math.abs(4 * x - 1))));
  return [r, g, b];
}

/** Diverging blue-white-red ramp for signed velocity u_Sn (blue = return eddy < 0, red = forward > 0). */
function divergingRamp(v: number, maxAbs: number): [number, number, number] {
  const s = Math.max(-1, Math.min(1, v / Math.max(maxAbs, 1e-6)));
  if (s < 0) {
    const a = -s;
    return [Math.round(240 * (1 - 0.78 * a)), Math.round(244 * (1 - 0.55 * a)), Math.round(250)];
  }
  const a = s;
  return [Math.round(250), Math.round(244 * (1 - 0.65 * a)), Math.round(240 * (1 - 0.82 * a))];
}

/** Interpolate 1D optical distortion curve (varicose or sinuous) at axial station xM (m). */
function interpOpticalPower(pts: CoupledDistortionPoint[], xM: number): number {
  if (!pts || pts.length === 0) return 0;
  if (xM <= pts[0].x_m) return pts[0].optical_power_mdpt;
  const last = pts[pts.length - 1];
  if (xM >= last.x_m) return last.optical_power_mdpt;
  let lo = 0;
  let hi = pts.length - 1;
  while (lo + 1 < hi) {
    const mid = (lo + hi) >> 1;
    if (pts[mid].x_m <= xM) lo = mid;
    else hi = mid;
  }
  const p0 = pts[lo];
  const p1 = pts[hi];
  const t = (xM - p0.x_m) / Math.max(p1.x_m - p0.x_m, 1e-6);
  return p0.optical_power_mdpt * (1 - t) + p1.optical_power_mdpt * t;
}

/** Sample the 2D scalar value for a given BathFieldMode at node (i, j). */
function sampleFieldValue(vis: FloatBathVisualFields, mode: BathFieldMode, i: number, j: number): number {
  switch (mode) {
    case 'glass_temp':
      return vis.glass_temp_c[i]?.[j] ?? 0;
    case 'u_tin':
      return vis.u_tin_m_s[i]?.[j] ?? 0;
    case 'tin_temp':
      return vis.tin_temp_c[i]?.[j] ?? 0;
    case 'redox_film':
      return vis.redox_film_um?.[i]?.[j] ?? 0;
    case 'bath_depth':
      return vis.bath_depth_mm[i]?.[j] ?? 0;
    case 'roof_temp':
      return vis.roof_temp_c[i]?.[j] ?? 0;
    case 'optical_waves': {
      const xM = vis.x_m[i] ?? 0;
      const yM = vis.y_m[i]?.[j] ?? 0;
      const wHalf = Math.max(vis.ribbon_edge_y_m[i] ?? 1.6, 0.2);
      const pVar = Math.abs(interpOpticalPower(vis.varicose_points, xM));
      const pSin = Math.abs(interpOpticalPower(vis.sinuous_points, xM));
      const edgeFrac = Math.min(1, yM / wHalf);
      return pVar * (1 - 0.35 * edgeFrac) + pSin * (0.35 + 0.65 * edgeFrac * edgeFrac);
    }
  }
}

function fieldMinMax(vis: FloatBathVisualFields, mode: BathFieldMode): { minV: number; maxV: number; maxAbs: number } {
  let minV = Infinity;
  let maxV = -Infinity;
  for (let i = 0; i < vis.nx; i++) {
    for (let j = 0; j < vis.ny; j++) {
      const v = sampleFieldValue(vis, mode, i, j);
      if (v < minV) minV = v;
      if (v > maxV) maxV = v;
    }
  }
  if (!Number.isFinite(minV)) minV = 0;
  if (!Number.isFinite(maxV)) maxV = 1;
  const maxAbs = Math.max(Math.abs(minV), Math.abs(maxV), 0.01);
  return { minV, maxV, maxAbs };
}

function colorForFieldValue(
  mode: BathFieldMode,
  v: number,
  minV: number,
  maxV: number,
  maxAbs: number,
): [number, number, number] {
  if (mode === 'u_tin') {
    return divergingRamp(v, maxAbs);
  }
  return turboRamp((v - minV) / Math.max(maxV - minV, 1e-6));
}

/** Generate a parametric 3D .fsm document representing the active Float-Bath + Sill + Tapered Ribbon + CVD Coater configuration. */
function buildFloatBathFsm(designP: number[]): string {
  const hSillMm = designP[0] ?? 65.0;
  const xBeamM = designP[1] ?? 28.5;
  const qBeamKw = designP[2] ?? 34.0;
  const pH2Pct = designP[3] ?? 4.2;
  const iMotorA = designP[4] ?? 260.0;
  const topRollCantDeg = designP[5] ?? 0.0;

  const bathLenMm = 216;
  const bathWidthMm = 52;
  const sillStepMm = Math.max(4, Math.min(9, Math.round((115 - hSillMm) * 0.16)));
  const sillZMm = (15.0 + 0.5 * sillStepMm).toFixed(1);
  const beamX = Math.max(-70, Math.min(70, Math.round(xBeamM * 4 - bathLenMm / 2)));
  const ribbonWMm = Math.max(24, Math.min(36, Math.round(32 + topRollCantDeg * 2.0)));
  const hotSpreadWMm = Math.max(32, Math.min(42, ribbonWMm + 8));
  const coaterWaterC = Math.max(110, Math.min(230, Math.round(185 - (qBeamKw - 28.0) * 2.2)));

  return `fieldsmith 1
# Pilkington UK5 Float-Bath + Stepped Sill + Tapered Glass Ribbon + Water-Cooled CVD Coater
# Synthesised from Coupled Systems Workbench:
#   H_sill = ${hSillMm.toFixed(1)} mm | x_beam = ${xBeamM.toFixed(1)} m | Q_beam = ${qBeamKw.toFixed(0)} kW | H2 = ${pH2Pct.toFixed(2)}% | I_motor = ${iMotorA.toFixed(0)} A
name "UK5 Float-Bath & CVD Coater (H_sill=${hSillMm.toFixed(0)}mm, x_beam=${xBeamM.toFixed(1)}m)"
material ss316l
custommaterial sic_refractory name="Alumino-Silicate / SiC Refractory" e=95000 nu=0.20 yield=140 density=2.65 wall=1.5 a=0.0000046 source="Fused-cast float-bath refractory"
custommaterial molten_tin name="Molten Tin Bath Sn(l)" e=1200 nu=0.45 yield=18 density=6.50 wall=0.8 a=0.0000909 source="Pilkington tin2fe liquid metal (Ron Smith JFM 1990)"
custommaterial soda_lime_glass name="Soda-Lime Float Glass (VFT Melt)" e=72000 nu=0.22 yield=65 density=2.50 wall=0.8 a=0.000009 source="Pilkington float2fe VFT melt log10 eta = -2.65 + 4210/(T-264)"
custommaterial h2_n2_gas name="Reducing H2/N2 Bath Atmosphere" e=100 nu=0.30 yield=5 density=0.15 wall=0.8 a=0.000015 source="Forming-gas headspace + SnO/H2 Navier-slip cushion"

let bath_len = ${bathLenMm}
let bath_w = ${bathWidthMm}
let sill_step = ${sillStepMm}
let beam_x = ${beamX}
let h_sill = ${hSillMm}
let u_pull = 0.16
let i_motor = ${iMotorA}
let p_h2 = ${pH2Pct}
let q_beam = ${qBeamKw}
let top_roll_cant = ${topRollCantDeg}

# Central structural support pedestal (isolates fixed base from hot spout thermal expansion)
box support_pedestal size=(76, 36, 6) at=(0, 0, 3)
color "#52525b" on support_pedestal

# Refractory casing floor, molten-tin continuum & side walls
box refractory_casing size=(bath_len + 16, bath_w + 12, 20) at=(0, 0, 14)
box tin_cavity size=(bath_len - 12, bath_w - 8, 9) at=(0, 0, 21) subtract
material sic_refractory on refractory_casing
color "#6b665e" on refractory_casing

# Active Molten-Tin Pool Continuum (ρ = 6500 kg/m³, VMS circulation + Lorentz MHD)
box tin_bath_pool size=(bath_len - 14, bath_w - 10, 4.5) at=(0, 0, 14.5)
material molten_tin on tin_bath_pool
color "#64748b" on tin_bath_pool

# Downstream Refractory Sill Step (controls bath depth H(x) = ${hSillMm.toFixed(1)} mm)
box refractory_sill size=(116, bath_w - 4, sill_step) at=(32, 0, ${sillZMm})
material sic_refractory on refractory_sill
color "#c87d3b" on refractory_sill

# Upstream Hot-Spread Glass Zone & Downstream Stretched Glass Ribbon (VFT Viscous Melt)
box ribbon_hot_spread size=(78, ${hotSpreadWMm}, 6) at=(-58, 0, 17)
material soda_lime_glass on ribbon_hot_spread
color "#f59e0b" on ribbon_hot_spread
box glass_ribbon size=(142, ${ribbonWMm}, 5) at=(28, 0, 16.5)
material soda_lime_glass on glass_ribbon
color "#38bdf8" on glass_ribbon

# Top-Roll Attenuator Shafts engaging ribbon edges in the stretching zone
box top_roll_north size=(12, 16, 5) at=(-24, 22, 19.5)
box top_roll_south size=(12, 16, 5) at=(-24, -22, 19.5)
color "#94a3b8" on top_roll_north
color "#94a3b8" on top_roll_south

# Overhead Water-Cooled CVD Coating Beam Bridge at x = ${xBeamM.toFixed(1)} m (Q_beam = ${qBeamKw.toFixed(0)} kW)
box cvd_coater_beam size=(14, bath_w + 8, 9) at=(beam_x, 0, 24.5)
color "#e05a47" on cvd_coater_beam

# Linear Electromagnetic Motor Inductors (I_motor = ${iMotorA.toFixed(0)} A)
box linear_motor_north size=(84, 6, 8) at=(12, 26, 23)
box linear_motor_south size=(84, 6, 8) at=(12, -26, 23)
color "#d9a441" on linear_motor_north
color "#d9a441" on linear_motor_south

region casing_base = support_pedestal.nz
region coater_slot = cvd_coater_beam.pz
region hot_spout = refractory_casing.nx

support fix_base casing_base fixed=xyz
load coater_weight coater_slot force=(0, 0, -28000) case="Coater Thermal & Gravity"
temperature spout_hot hot_spout temp=310
temperature coater_water coater_slot temp=${coaterWaterC}
solve tref=20
`;
}

// ---- Shared Coupled Multi-Physics State for Workbench & Main 3D Viewport Overlay ----

interface CoupledViewportOverlayConfig {
  enabled: boolean;
  mode: BathFieldMode;
  showVectors: boolean;
  showRibbon: boolean;
  showBathFloor: boolean;
  showCoater: boolean;
  showWaves: boolean;
  zScale: number;
}

const coupledShared: {
  designP: number[];
  epistemicTheta: number[];
  bundle: CoupledSolveBundle | null;
  summary: CoupledSystemSummary | null;
  overlay: CoupledViewportOverlayConfig;
} = {
  designP: [65.0, 28.5, 34.0, 4.2, 260.0, 0.0, 620.0],
  epistemicTheta: [5.0, 92.0, 1.15, 0.42],
  bundle: null,
  summary: null,
  overlay: {
    enabled: true,
    mode: 'glass_temp',
    showVectors: true,
    showRibbon: true,
    showBathFloor: true,
    showCoater: true,
    showWaves: true,
    zScale: 1.0,
  },
};

function ensureCoupledSolved() {
  if (coupledShared.bundle) return;
  try {
    const cat = listCoupledSystems();
    const sys = cat.find((s) => s.system_id === 'pilkington_float_bath_cvd_coater_v1') ?? cat[0];
    if (sys) {
      coupledShared.summary = sys;
      coupledShared.designP = sys.design_parameters.map((p) => p.nominal);
      coupledShared.epistemicTheta = sys.epistemic_parameters.map((t) => t.nominal);
      coupledShared.bundle = solveCoupledSystem(
        sys.system_id,
        coupledShared.designP,
        coupledShared.epistemicTheta,
      );
    }
  } catch {
    // ignore before wasm init
  }
}

/** Dispose all Geometries and Materials inside a THREE.Group. */
function clearThreeGroup(group: THREE.Group) {
  while (group.children.length > 0) {
    const child = group.children[group.children.length - 1];
    group.remove(child);
    child.traverse((obj) => {
      const m = obj as THREE.Mesh;
      if (m.geometry) m.geometry.dispose();
      if (m.material) {
        if (Array.isArray(m.material)) m.material.forEach((mat) => mat.dispose());
        else m.material.dispose();
      }
    });
  }
}

/**
 * Populate a Three.js Group with the 3D Multi-Physics Float-Bath + Stepped Sill + Velocity Vectors
 * + Curved Free-Surface Glass Ribbon + Optical Distortion Waves + Bowed Water-Cooled CVD Coater Beam.
 *
 * Works in two coordinate spaces:
 * - `'workbench'`: Full cutaway 3D digital twin inside the Coupled Systems Workbench
 *   (`X ∈ [-102, +102]`, `Y ∈ [-22, +22]`, bath floor `Z ∈ [-12, -6]`, ribbon `Z ≈ 0`, coater `Z ≈ +6`).
 * - `'fsm_viewport'`: Superimposed directly into the main Fieldsmith 3D Viewport (`vp.overlays`)
 *   aligned with the `.fsm` trough (`X ∈ [-100, +100] mm`, `Y ∈ [-21, +21] mm`, `Z ∈ [16.5, 29] mm`).
 */
function populateFloatBath3DGroup(
  group: THREE.Group,
  vis: FloatBathVisualFields,
  designP: number[],
  cfg: CoupledViewportOverlayConfig,
  coordSpace: 'workbench' | 'fsm_viewport',
) {
  clearThreeGroup(group);

  const { nx, ny } = vis;
  if (nx < 2 || ny < 2) return;

  const xMax = vis.x_m[nx - 1] || 54.72;
  const yMax = Math.max(...vis.y_m.map((row) => row[ny - 1] ?? 0), 2.32);

  const toX = (xM: number) => (xM / xMax - 0.5) * 200.0;
  const toY = (yM: number) => (yM / yMax) * 20.5;

  const zScale = cfg.zScale || 1.0;
  const baseZ = coordSpace === 'fsm_viewport' ? 20.35 : 0.0;

  const { minV, maxV, maxAbs } = fieldMinMax(vis, cfg.mode);
  const { maxAbs: maxAbsVel } = fieldMinMax(vis, 'u_tin');

  // Build mirrored full-width Y rows: jFull in 0 .. (2*ny - 1) from -yMax to +yMax
  const nYFull = 2 * ny - 1;
  const mapJ = (jFull: number): { j: number; sign: number } => {
    if (jFull < ny - 1) {
      return { j: ny - 1 - jFull, sign: -1 };
    }
    return { j: jFull - (ny - 1), sign: 1 };
  };

  // 1. 3D Stepped Bath Floor & Molten Tin Bath Continuum Heightfield (z = -H(x,y))
  if (cfg.showBathFloor && coordSpace === 'workbench') {
    const bathPos = new Float32Array(nx * nYFull * 3);
    const bathCol = new Float32Array(nx * nYFull * 3);
    const indices: number[] = [];

    for (let i = 0; i < nx; i++) {
      const xM = vis.x_m[i];
      const px = toX(xM);
      for (let jf = 0; jf < nYFull; jf++) {
        const { j, sign } = mapJ(jf);
        const yM = sign * (vis.y_m[i]?.[j] ?? 0);
        const py = toY(yM);
        const depthMm = vis.bath_depth_mm[i]?.[j] ?? 90;
        const pz = baseZ - (depthMm / 105.0) * 9.5 * zScale;

        const idx = (i * nYFull + jf) * 3;
        bathPos[idx] = px;
        bathPos[idx + 1] = py;
        bathPos[idx + 2] = pz;

        // Colour floor by Tin Temperature, Tin Velocity, or Bath Depth
        const floorMode: BathFieldMode =
          cfg.mode === 'u_tin' || cfg.mode === 'tin_temp' || cfg.mode === 'bath_depth'
            ? cfg.mode
            : 'bath_depth';
        const { minV: fMin, maxV: fMax, maxAbs: fAbs } = fieldMinMax(vis, floorMode);
        const fv = sampleFieldValue(vis, floorMode, i, j);
        const [r, g, b] = colorForFieldValue(floorMode, fv, fMin, fMax, fAbs);
        bathCol[idx] = (r / 255) * 0.78;
        bathCol[idx + 1] = (g / 255) * 0.78;
        bathCol[idx + 2] = (b / 255) * 0.78;
      }
    }

    for (let i = 0; i < nx - 1; i++) {
      for (let jf = 0; jf < nYFull - 1; jf++) {
        const a = i * nYFull + jf;
        const b = (i + 1) * nYFull + jf;
        const c = (i + 1) * nYFull + (jf + 1);
        const d = i * nYFull + (jf + 1);
        indices.push(a, b, c, a, c, d);
      }
    }

    const bathGeo = new THREE.BufferGeometry();
    bathGeo.setAttribute('position', new THREE.BufferAttribute(bathPos, 3));
    bathGeo.setAttribute('color', new THREE.BufferAttribute(bathCol, 3));
    bathGeo.setIndex(indices);
    bathGeo.computeVertexNormals();

    const bathMesh = new THREE.Mesh(
      bathGeo,
      new THREE.MeshLambertMaterial({
        vertexColors: true,
        side: THREE.DoubleSide,
      }),
    );
    group.add(bathMesh);

    const wireMesh = new THREE.Mesh(
      bathGeo,
      new THREE.MeshBasicMaterial({
        color: 0xffffff,
        wireframe: true,
        transparent: true,
        opacity: 0.12,
      }),
    );
    group.add(wireMesh);

    // Highlighted Refractory Sill Step line at x = 20.5 m
    const sillX = toX(20.5);
    const sillPts = [
      new THREE.Vector3(sillX, -20.5, baseZ - 9.5 * zScale),
      new THREE.Vector3(sillX, -20.5, baseZ + 0.5),
      new THREE.Vector3(sillX, 20.5, baseZ + 0.5),
      new THREE.Vector3(sillX, 20.5, baseZ - 9.5 * zScale),
    ];
    const sillLineGeo = new THREE.BufferGeometry().setFromPoints(sillPts);
    group.add(
      new THREE.Line(
        sillLineGeo,
        new THREE.LineBasicMaterial({ color: 0xfbbf24, linewidth: 2 }),
      ),
    );
  }

  // 2. 3D Free-Surface Forming Glass Ribbon + Varicose/Sinuous Optical Wave Ripples
  if (cfg.showRibbon) {
    const rNy = Math.max(vis.r_ny, 2);
    const nRibFull = 2 * rNy - 1;
    const ribPos = new Float32Array(nx * nRibFull * 3);
    const ribCol = new Float32Array(nx * nRibFull * 3);
    const ribIndices: number[] = [];

    for (let i = 0; i < nx; i++) {
      const xM = vis.x_m[i];
      const px = toX(xM);
      const wHalfM = Math.max(vis.ribbon_edge_y_m[i] ?? 1.6, 0.2);
      const hMm = vis.ribbon_thickness_mm?.[i] ?? 5.85;
      const pVar = interpOpticalPower(vis.varicose_points, xM);
      const pSin = interpOpticalPower(vis.sinuous_points, xM);

      for (let rf = 0; rf < nRibFull; rf++) {
        const sign = rf < rNy - 1 ? -1 : 1;
        const j = rf < rNy - 1 ? rNy - 1 - rf : rf - (rNy - 1);
        const yFrac = j / (rNy - 1);
        const yM = sign * yFrac * wHalfM;
        const py = toY(yM);

        // 3D Varicose longitudinal waves + Sinuous flexural edge waves
        let waveDz = 0;
        if (cfg.showWaves) {
          const varWave = 0.045 * pVar * Math.cos(xM * 1.85);
          const sinWave = 0.11 * pSin * Math.sin(xM * 1.25) * (0.2 + 0.8 * yFrac * yFrac) * sign;
          waveDz = (varWave + sinWave) * zScale;
        }
        const thickZ =
          coordSpace === 'fsm_viewport'
            ? baseZ + 0.18 * (hMm - 5.0) + waveDz * 0.65
            : baseZ + 0.26 * hMm * zScale + waveDz;

        const idx = (i * nRibFull + rf) * 3;
        ribPos[idx] = px;
        ribPos[idx + 1] = py;
        ribPos[idx + 2] = thickZ;

        const val = sampleFieldValue(vis, cfg.mode, i, j);
        const [r, g, b] = colorForFieldValue(cfg.mode, val, minV, maxV, maxAbs);
        ribCol[idx] = r / 255;
        ribCol[idx + 1] = g / 255;
        ribCol[idx + 2] = b / 255;
      }
    }

    for (let i = 0; i < nx - 1; i++) {
      for (let rf = 0; rf < nRibFull - 1; rf++) {
        const a = i * nRibFull + rf;
        const b = (i + 1) * nRibFull + rf;
        const c = (i + 1) * nRibFull + (rf + 1);
        const d = i * nRibFull + (rf + 1);
        ribIndices.push(a, b, c, a, c, d);
      }
    }

    const ribGeo = new THREE.BufferGeometry();
    ribGeo.setAttribute('position', new THREE.BufferAttribute(ribPos, 3));
    ribGeo.setAttribute('color', new THREE.BufferAttribute(ribCol, 3));
    ribGeo.setIndex(ribIndices);
    ribGeo.computeVertexNormals();

    const ribMesh = new THREE.Mesh(
      ribGeo,
      new THREE.MeshLambertMaterial({
        vertexColors: true,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: coordSpace === 'fsm_viewport' ? 0.92 : 0.94,
      }),
    );
    ribMesh.renderOrder = 3;
    group.add(ribMesh);

    // Bright white free-boundary edge lines +W(x)/2 and -W(x)/2
    for (const sign of [-1, 1]) {
      const edgePts: THREE.Vector3[] = [];
      for (let i = 0; i < nx; i++) {
        const rf = sign < 0 ? 0 : nRibFull - 1;
        const idx = (i * nRibFull + rf) * 3;
        edgePts.push(new THREE.Vector3(ribPos[idx], ribPos[idx + 1], ribPos[idx + 2] + 0.12));
      }
      const edgeGeo = new THREE.BufferGeometry().setFromPoints(edgePts);
      const edgeLine = new THREE.Line(
        edgeGeo,
        new THREE.LineBasicMaterial({ color: 0xffffff, linewidth: 2 }),
      );
      edgeLine.renderOrder = 4;
      group.add(edgeLine);
    }
  }

  // 3. 3D Molten-Tin Recirculation Velocity Vectors (u_Sn, v_Sn)
  // Red/Amber = forward Couette drag under ribbon (u_Sn > 0)
  // Cyan/Blue = cold upstream return eddy in outer channels & over sill (u_Sn < 0)
  if (cfg.showVectors) {
    for (let i = 0; i < nx; i++) {
      const xM = vis.x_m[i];
      const px = toX(xM);
      for (let j = 0; j < ny; j += 2) {
        const u = vis.u_tin_m_s[i]?.[j] ?? 0;
        const v = vis.v_tin_m_s[i]?.[j] ?? 0;
        const speed = Math.hypot(u, v);
        if (speed < 1e-4) continue;

        const depthMm = vis.bath_depth_mm[i]?.[j] ?? 90;
        const arrowZ =
          coordSpace === 'fsm_viewport'
            ? baseZ + 0.75
            : baseZ - (depthMm / 105.0) * 4.2 * zScale;

        const signs = j === 0 ? [1] : [-1, 1];
        for (const sign of signs) {
          const yM = sign * (vis.y_m[i]?.[j] ?? 0);
          const py = toY(yM);
          const dir = new THREE.Vector3(u, sign * v * 2.2, 0).normalize();
          const len = Math.max(2.2, Math.min(9.5, (speed / maxAbsVel) * 9.5));
          const colHex = u >= 0 ? 0xf97316 : 0x38bdf8;
          const arrow = new THREE.ArrowHelper(
            dir,
            new THREE.Vector3(px, py, arrowZ),
            len,
            colHex,
            Math.min(2.2, len * 0.36),
            Math.min(1.3, len * 0.22),
          );
          arrow.renderOrder = 5;
          group.add(arrow);
        }
      }
    }
  }

  // 4. 3D Overhead Water-Cooled CVD Coating Beam with Exaggerated Thermal Bow Δz(y) & Slot Curtain
  // + 3D Top-Roll Attenuator Wheels & Linear Motor Banks
  if (cfg.showCoater) {
    const xBeamM = designP[1] ?? 28.5;
    const beamPx = toX(xBeamM);
    const bowArr = vis.coater_beam_bow_mm ?? [];
    const ySpanArr = vis.coater_beam_y_m ?? [];
    const midBowMm = bowArr[0] ?? 0.18;
    const bowExceeded = midBowMm > 0.24;

    // Construct bowed bottom flange curve across y in [-21, +21]
    const nSpan = Math.max(ySpanArr.length, 8);
    const curtainPos: number[] = [];
    const beamTopPts: THREE.Vector3[] = [];
    const beamBotPts: THREE.Vector3[] = [];

    const totalPts = 2 * nSpan - 1;
    for (let k = 0; k < totalPts; k++) {
      const sign = k < nSpan - 1 ? -1 : 1;
      const idx = k < nSpan - 1 ? nSpan - 1 - k : k - (nSpan - 1);
      const yM =
        ySpanArr.length > idx
          ? sign * ySpanArr[idx]
          : sign * ((idx / Math.max(nSpan - 1, 1)) * yMax);
      const py = toY(yM);
      const bowMm =
        bowArr.length > idx
          ? bowArr[idx]
          : midBowMm * (1 - Math.pow(idx / Math.max(nSpan - 1, 1), 2));

      // Exaggerate mm thermal bow so the 0.15..0.30 mm mid-span sag is visually unmistakable in 3D
      const zSlotNominal = coordSpace === 'fsm_viewport' ? baseZ + 3.2 : baseZ + 5.5 * zScale;
      const zSlotBowed = zSlotNominal - bowMm * 9.0 * zScale;
      const zRibbon = coordSpace === 'fsm_viewport' ? baseZ + 0.4 : baseZ + 1.4 * zScale;

      beamBotPts.push(new THREE.Vector3(beamPx, py, zSlotBowed));
      beamTopPts.push(new THREE.Vector3(beamPx, py, zSlotNominal + 2.8));

      curtainPos.push(beamPx, py, zRibbon, beamPx, py, zSlotBowed);
    }

    // Slot-to-ribbon clearance curtain (green when Δz <= 0.24 mm, red when bow exceeds 0.24 mm)
    const curtainIndices: number[] = [];
    for (let k = 0; k < totalPts - 1; k++) {
      const a = 2 * k;
      const b = 2 * k + 1;
      const c = 2 * (k + 1) + 1;
      const d = 2 * (k + 1);
      curtainIndices.push(a, b, c, a, c, d);
    }
    const curtainGeo = new THREE.BufferGeometry();
    curtainGeo.setAttribute('position', new THREE.Float32BufferAttribute(curtainPos, 3));
    curtainGeo.setIndex(curtainIndices);
    curtainGeo.computeVertexNormals();

    const curtainCol = bowExceeded ? 0xef4444 : 0x10b981;
    const curtainMesh = new THREE.Mesh(
      curtainGeo,
      new THREE.MeshBasicMaterial({
        color: curtainCol,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.42,
      }),
    );
    curtainMesh.renderOrder = 6;
    group.add(curtainMesh);

    // Bowed bottom slot profile line
    group.add(
      new THREE.Line(
        new THREE.BufferGeometry().setFromPoints(beamBotPts),
        new THREE.LineBasicMaterial({ color: bowExceeded ? 0xf87171 : 0x34d399, linewidth: 3 }),
      ),
    );

    if (coordSpace === 'workbench') {
      // 3D CVD Coater bridge beam body in Workbench 3D Twin
      const beamBoxGeo = new THREE.BoxGeometry(5.5, 43.0, 3.2);
      const beamBox = new THREE.Mesh(
        beamBoxGeo,
        new THREE.MeshLambertMaterial({
          color: bowExceeded ? 0xdc2626 : 0xe05a47,
          transparent: true,
          opacity: 0.85,
        }),
      );
      beamBox.position.set(beamPx, 0, baseZ + 6.8 * zScale - midBowMm * 4.0 * zScale);
      group.add(beamBox);

      // 3D Top-Roll Attenuator Shafts & Wheels along the stretching zone
      const cantRad = ((designP[5] ?? 0.0) * Math.PI) / 180.0;
      const rollMat = new THREE.MeshLambertMaterial({ color: 0xcbd5e1 });
      for (const rx of vis.top_roll_x_m) {
        let bestI = 0;
        for (let i = 0; i < nx; i++) {
          if (Math.abs(vis.x_m[i] - rx) < Math.abs(vis.x_m[bestI] - rx)) bestI = i;
        }
        const rPx = toX(rx);
        const yEdge = toY(vis.ribbon_edge_y_m[bestI] ?? 1.6);
        for (const sign of [-1, 1]) {
          const wheelGeo = new THREE.CylinderGeometry(1.3, 1.3, 1.2, 16);
          const wheel = new THREE.Mesh(wheelGeo, rollMat);
          wheel.position.set(rPx, sign * yEdge, baseZ + 1.8 * zScale);
          wheel.rotation.z = sign * cantRad;
          group.add(wheel);

          const shaftGeo = new THREE.BoxGeometry(0.6, 5.5, 0.6);
          const shaft = new THREE.Mesh(shaftGeo, rollMat);
          shaft.position.set(rPx, sign * (yEdge + 2.6), baseZ + 2.0 * zScale);
          shaft.rotation.z = sign * cantRad;
          group.add(shaft);
        }
      }

      // 3D Linear Electromagnetic Motor Inductors (18 m .. 40 m)
      const iMotorA = designP[4] ?? 260.0;
      const lmX0 = toX(18.0);
      const lmX1 = toX(40.0);
      const lmMat = new THREE.MeshLambertMaterial({
        color: iMotorA > 50 ? 0xf59e0b : 0x64748b,
      });
      for (const sign of [-1, 1]) {
        const lmGeo = new THREE.BoxGeometry(lmX1 - lmX0, 1.8, 2.6);
        const lmMesh = new THREE.Mesh(lmGeo, lmMat);
        lmMesh.position.set(0.5 * (lmX0 + lmX1), sign * 21.8, baseZ + 1.5 * zScale);
        group.add(lmMesh);
      }
    }
  }
}

// ---- Interactive 3D Three.js Multi-Physics Twin Component (Inside Coupled Workbench) ----

function FloatBath3DTwin({
  vis,
  mode,
  designP,
}: {
  vis: FloatBathVisualFields;
  mode: BathFieldMode;
  designP: number[];
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const sceneRef = useRef<{
    renderer: THREE.WebGLRenderer;
    scene: THREE.Scene;
    camera: THREE.PerspectiveCamera;
    controls: OrbitControls;
    group: THREE.Group;
  } | null>(null);

  const [showRibbon, setShowRibbon] = useState(true);
  const [showBathFloor, setShowBathFloor] = useState(true);
  const [showVectors, setShowVectors] = useState(true);
  const [showCoater, setShowCoater] = useState(true);
  const [showWaves, setShowWaves] = useState(true);
  const [zScale, setZScale] = useState(1.0);

  // Initialise Three.js WebGLRenderer + OrbitControls once
  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x121418);

    const w = el.clientWidth || 760;
    const h = el.clientHeight || 290;
    const camera = new THREE.PerspectiveCamera(36, w / h, 1, 1200);
    camera.up.set(0, 0, 1);
    camera.position.set(-28, -96, 54);

    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(w, h);
    el.innerHTML = '';
    el.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(4, 0, -1);
    controls.enableDamping = false;
    controls.update();

    const ambient = new THREE.AmbientLight(0xffffff, 0.78);
    scene.add(ambient);
    const dir1 = new THREE.DirectionalLight(0xffffff, 0.72);
    dir1.position.set(-60, -80, 110);
    scene.add(dir1);
    const dir2 = new THREE.DirectionalLight(0x93c5fd, 0.35);
    dir2.position.set(80, 70, 60);
    scene.add(dir2);

    const group = new THREE.Group();
    scene.add(group);

    const renderFrame = () => {
      renderer.render(scene, camera);
    };
    controls.addEventListener('change', renderFrame);

    const ro = new ResizeObserver(() => {
      const nw = el.clientWidth || 760;
      const nh = el.clientHeight || 290;
      camera.aspect = nw / nh;
      camera.updateProjectionMatrix();
      renderer.setSize(nw, nh);
      renderFrame();
    });
    ro.observe(el);

    sceneRef.current = { renderer, scene, camera, controls, group };

    return () => {
      ro.disconnect();
      controls.dispose();
      clearThreeGroup(group);
      renderer.dispose();
      sceneRef.current = null;
    };
  }, []);

  // Rebuild 3D multi-physics meshes whenever solved state, mode, or layer toggles change
  useEffect(() => {
    const ctx = sceneRef.current;
    if (!ctx) return;
    populateFloatBath3DGroup(
      ctx.group,
      vis,
      designP,
      {
        enabled: true,
        mode,
        showVectors,
        showRibbon,
        showBathFloor,
        showCoater,
        showWaves,
        zScale,
      },
      'workbench',
    );
    ctx.renderer.render(ctx.scene, ctx.camera);
  }, [vis, mode, designP, showRibbon, showBathFloor, showVectors, showCoater, showWaves, zScale]);

  const setCameraPreset = (preset: 'iso' | 'side' | 'coater' | 'top') => {
    const ctx = sceneRef.current;
    if (!ctx) return;
    const { camera, controls, renderer, scene } = ctx;
    if (preset === 'iso') {
      camera.position.set(-28, -96, 54);
      controls.target.set(4, 0, -1);
    } else if (preset === 'side') {
      // Side profile highlighting the stepped sill bathymetry H(x) and vertical gap
      camera.position.set(0, -112, 6);
      controls.target.set(0, 0, -2);
    } else if (preset === 'coater') {
      // Closeup on the Water-Cooled CVD Coating Beam thermal bow Δz(y) & slot gap
      const xMax = vis.x_m[vis.nx - 1] || 54.72;
      const beamPx = ((designP[1] ?? 28.5) / xMax - 0.5) * 200.0;
      camera.position.set(beamPx - 36, -42, 22);
      controls.target.set(beamPx, 0, 2);
    } else if (preset === 'top') {
      camera.position.set(0, -1, 138);
      controls.target.set(0, 0, 0);
    }
    controls.update();
    renderer.render(scene, camera);
  };

  const tabMeta = BATH_FIELD_TABS.find((t) => t.id === mode)!;
  const { minV, maxV, maxAbs } = fieldMinMax(vis, mode);
  const midBow = vis.coater_beam_bow_mm?.[0] ?? 0.18;

  return (
    <div class="sys-3d-wrap" data-testid="float-bath-3d-twin">
      <div class="sys-3d-toolbar">
        <div class="sys-3d-Badge-group">
          <button class="btn small" onClick={() => setCameraPreset('iso')} title="3D Isometric Overview">
            3D Iso
          </button>
          <button
            class="btn small"
            data-testid="cam-side-sill"
            onClick={() => setCameraPreset('side')}
            title="Side Cutaway: Inspect Bath Depth H(x) & Stepped Sill"
          >
            Sill &amp; Depth Profile
          </button>
          <button
            class="btn small"
            data-testid="cam-coater-gap"
            onClick={() => setCameraPreset('coater')}
            title="Zoom to Water-Cooled CVD Coater Thermal Bow Δz(y) & Slot Clearance"
          >
            CVD Coater Bow
          </button>
          <button class="btn small" onClick={() => setCameraPreset('top')} title="Top-Down Plan View">
            Top
          </button>
          <button
            class={`btn small ${zScale > 1.2 ? 'on' : ''}`}
            onClick={() => setZScale(zScale > 1.2 ? 1.0 : 1.65)}
            title="Toggle extra vertical Z-exaggeration for bath depth H(x), optical waves, and beam bow"
          >
            Z×{zScale > 1.2 ? '40' : '25'}
          </button>
        </div>

        <div class="sys-3d-layers">
          <label title="Show 3D curved free-surface glass ribbon W(x), h(x)">
            <input type="checkbox" checked={showRibbon} onChange={(e) => setShowRibbon((e.target as HTMLInputElement).checked)} />
            Ribbon
          </label>
          <label title="Show 3D stepped refractory sill & molten tin bath floor z = -H(x,y)">
            <input type="checkbox" checked={showBathFloor} onChange={(e) => setShowBathFloor((e.target as HTMLInputElement).checked)} />
            Bath &amp; Sill
          </label>
          <label title="Show 3D molten-tin velocity arrows (Orange = forward Couette drag, Blue = cold return eddy)">
            <input type="checkbox" checked={showVectors} onChange={(e) => setShowVectors((e.target as HTMLInputElement).checked)} />
            u_Sn Vectors
          </label>
          <label title="Show 3D water-cooled CVD coater bridge, thermal bow curtain Δz(y), top rolls & linear motors">
            <input type="checkbox" checked={showCoater} onChange={(e) => setShowCoater((e.target as HTMLInputElement).checked)} />
            CVD Beam &amp; Rolls
          </label>
          <label title="Sculpt 3D varicose & sinuous optical distortion ripples onto the ribbon surface">
            <input type="checkbox" checked={showWaves} onChange={(e) => setShowWaves((e.target as HTMLInputElement).checked)} />
            3D Optical Ripples
          </label>
        </div>
      </div>

      <div ref={hostRef} class="sys-3d-host" />

      <div class="sys-3d-legend num">
        <span>
          Surface Field: <strong>{tabMeta.label}</strong> [
          {mode === 'u_tin'
            ? `${(-maxAbs).toFixed(3)} .. +${maxAbs.toFixed(3)}`
            : `${minV.toFixed(1)} .. ${maxV.toFixed(1)}`}{' '}
          {tabMeta.unit}]
        </span>
        <span>
          Arrows: <strong style={{ color: '#fb923c' }}>Forward Drag (+u_Sn)</strong> ·{' '}
          <strong style={{ color: '#38bdf8' }}>Cold Return Eddy (−u_Sn)</strong>
        </span>
        <span>
          CVD Coater Bow Δz(0) ={' '}
          <strong style={{ color: midBow <= 0.24 ? '#34d399' : '#f87171' }}>
            {midBow.toFixed(3)} mm
          </strong>{' '}
          (Drag to orbit · Scroll to zoom)
        </span>
      </div>
    </div>
  );
}

// ---- 2D Plan View + 3D Twin Switchable Visualiser Card ----

function FloatBathCanvas({
  vis,
  mode,
  onChangeMode,
  designP,
}: {
  vis: FloatBathVisualFields;
  mode: BathFieldMode;
  onChangeMode: (m: BathFieldMode) => void;
  designP: number[];
}) {
  const [viewDim, setViewDim] = useState<'3d' | '2d'>('3d');
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [hoverIdx, setHoverIdx] = useState<{ i: number; j: number } | null>(null);

  useEffect(() => {
    if (viewDim !== '2d') return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const W = canvas.width;
    const H = canvas.height;
    const padL = 44;
    const padR = 16;
    const padT = 22;
    const padB = 26;
    const plotW = W - padL - padR;
    const plotH = H - padT - padB;

    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = '#16171a';
    ctx.fillRect(0, 0, W, H);

    const { nx, ny } = vis;
    const { minV, maxV, maxAbs: maxAbsU } = fieldMinMax(vis, mode);

    const cellW = Math.ceil(plotW / nx) + 1;
    const halfRows = ny;
    const totalRows = halfRows * 2;
    const cellH = Math.ceil(plotH / totalRows) + 1;

    for (let i = 0; i < nx; i++) {
      const px = padL + Math.floor((i / nx) * plotW);
      for (let j = 0; j < ny; j++) {
        const v = sampleFieldValue(vis, mode, i, j);
        const [r, g, b] = colorForFieldValue(mode, v, minV, maxV, maxAbsU);
        ctx.fillStyle = `rgb(${r},${g},${b})`;

        const pyTop = padT + Math.floor(((halfRows - 1 - j) / totalRows) * plotH);
        const pyBot = padT + Math.floor(((halfRows + j) / totalRows) * plotH);
        ctx.fillRect(px, pyTop, cellW, cellH);
        ctx.fillRect(px, pyBot, cellW, cellH);
      }
    }

    const xMax = vis.x_m[nx - 1] || 54.72;
    const yMax = Math.max(...vis.y_m.map((row) => row[ny - 1] ?? 0), 2.5);
    const toPx = (xM: number) => padL + (xM / xMax) * plotW;
    const toPy = (yM: number) => padT + (0.5 - yM / (2 * yMax)) * plotH;

    if (mode === 'u_tin') {
      ctx.strokeStyle = 'rgba(20, 22, 26, 0.72)';
      ctx.lineWidth = 1;
      for (let i = 0; i < nx; i += 2) {
        for (let j = 0; j < ny; j += 2) {
          const u = vis.u_tin_m_s[i][j];
          const v = vis.v_tin_m_s[i][j];
          const xM = vis.x_m[i];
          const yM = vis.y_m[i][j];
          for (const sign of [1, -1]) {
            const cx = toPx(xM);
            const cy = toPy(sign * yM);
            const dx = (u / maxAbsU) * 14;
            const dy = -sign * (v / maxAbsU) * 14;
            ctx.beginPath();
            ctx.moveTo(cx, cy);
            ctx.lineTo(cx + dx, cy + dy);
            ctx.stroke();
          }
        }
      }
    }

    ctx.strokeStyle = 'rgba(255,255,255,0.22)';
    ctx.setLineDash([4, 4]);
    ctx.beginPath();
    ctx.moveTo(padL, toPy(0));
    ctx.lineTo(padL + plotW, toPy(0));
    ctx.stroke();
    ctx.setLineDash([]);

    const sillPx = toPx(20.5);
    ctx.strokeStyle = '#fbbf24';
    ctx.lineWidth = 1.5;
    ctx.setLineDash([5, 3]);
    ctx.beginPath();
    ctx.moveTo(sillPx, padT);
    ctx.lineTo(sillPx, padT + plotH);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = '#fbbf24';
    ctx.font = '10px ui-monospace, monospace';
    ctx.fillText(`Sill H=${(designP[0] ?? 65).toFixed(0)}mm`, sillPx + 4, padT + 11);

    for (const sign of [1, -1]) {
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 2.2;
      ctx.beginPath();
      for (let i = 0; i < nx; i++) {
        const px = toPx(vis.x_m[i]);
        const py = toPy(sign * vis.ribbon_edge_y_m[i]);
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.stroke();
    }

    ctx.fillStyle = '#e2e8f0';
    for (const rx of vis.top_roll_x_m) {
      let bestI = 0;
      for (let i = 0; i < nx; i++) {
        if (Math.abs(vis.x_m[i] - rx) < Math.abs(vis.x_m[bestI] - rx)) bestI = i;
      }
      const px = toPx(rx);
      const yEdge = vis.ribbon_edge_y_m[bestI];
      for (const sign of [1, -1]) {
        const py = toPy(sign * yEdge);
        ctx.fillRect(px - 2.5, py - 2.5, 5, 5);
      }
    }

    const lm0 = toPx(18.0);
    const lm1 = toPx(40.0);
    ctx.strokeStyle = '#38bdf8';
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(lm0, padT + 2);
    ctx.lineTo(lm1, padT + 2);
    ctx.moveTo(lm0, padT + plotH - 2);
    ctx.lineTo(lm1, padT + plotH - 2);
    ctx.stroke();

    const beamX = designP[1] ?? 27.5;
    const beamPx = toPx(beamX);
    ctx.fillStyle = 'rgba(239, 68, 68, 0.28)';
    ctx.fillRect(beamPx - 5, padT, 10, plotH);
    ctx.strokeStyle = '#f87171';
    ctx.lineWidth = 2;
    ctx.strokeRect(beamPx - 5, padT, 10, plotH);
    ctx.fillStyle = '#fecaca';
    ctx.font = 'bold 10px ui-monospace, monospace';
    ctx.fillText(`CVD Beam (${beamX.toFixed(1)}m)`, Math.min(beamPx + 7, W - 115), padT + plotH - 6);

    ctx.fillStyle = '#9ca3af';
    ctx.font = '10px ui-monospace, monospace';
    for (let xm = 0; xm <= 50; xm += 10) {
      const px = toPx(xm);
      ctx.fillText(`${xm}m`, px - 8, H - 8);
    }
    ctx.fillText(`+${yMax.toFixed(1)}m`, 4, padT + 10);
    ctx.fillText(`0.0m`, 10, toPy(0) + 3);
    ctx.fillText(`-${yMax.toFixed(1)}m`, 4, padT + plotH);

    const tabMeta = BATH_FIELD_TABS.find((t) => t.id === mode)!;
    const legText =
      mode === 'u_tin'
        ? `${tabMeta.label}: [${(-maxAbsU).toFixed(3)} .. +${maxAbsU.toFixed(3)} ${tabMeta.unit}] (Blue = Cold Upstream Return Eddy)`
        : `${tabMeta.label}: [${minV.toFixed(1)} .. ${maxV.toFixed(1)} ${tabMeta.unit}]`;
    ctx.fillStyle = '#e5e7eb';
    ctx.fillText(legText, padL, 14);
  }, [vis, mode, designP, viewDim]);

  const probeInfo = (() => {
    if (!hoverIdx) return null;
    const { i, j } = hoverIdx;
    return {
      x: vis.x_m[i],
      y: vis.y_m[i]?.[j] ?? 0,
      wEdge: vis.ribbon_edge_y_m[i],
      depth: vis.bath_depth_mm[i]?.[j] ?? 0,
      uSn: vis.u_tin_m_s[i]?.[j] ?? 0,
      tSn: vis.tin_temp_c[i]?.[j] ?? 0,
      tG: vis.glass_temp_c[i]?.[j] ?? 0,
      filmUm: vis.redox_film_um?.[i]?.[j] ?? 0,
    };
  })();

  return (
    <div class="sys-heatmap-card">
      <div class="sys-card-bar">
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <div class="seg">
            <button
              class={viewDim === '3d' ? 'on' : ''}
              data-testid="toggle-3d-twin"
              onClick={() => setViewDim('3d')}
              title="Interactive 3D Multi-Physics Twin (Bath Depth H, Recirculation Vectors u_Sn, Curved Ribbon W(x), Optical Ripples & CVD Coater Bow)"
            >
              3D Multi-Physics Twin
            </button>
            <button
              class={viewDim === '2d' ? 'on' : ''}
              data-testid="toggle-2d-plan"
              onClick={() => setViewDim('2d')}
              title="2D Plan View Heatmap with Node Probe"
            >
              2D Plan View
            </button>
          </div>
        </div>
        <div class="seg">
          {BATH_FIELD_TABS.map((t) => (
            <button
              key={t.id}
              class={mode === t.id ? 'on' : ''}
              data-testid={`select-field-${t.id}`}
              onClick={() => onChangeMode(t.id)}
              title={`${t.label} (${t.unit})`}
            >
              {t.shortLabel} ({t.unit})
            </button>
          ))}
        </div>
      </div>

      {viewDim === '3d' ? (
        <FloatBath3DTwin vis={vis} mode={mode} designP={designP} />
      ) : (
        <>
          <canvas
            ref={canvasRef}
            width={760}
            height={215}
            class="sys-bath-canvas"
            data-testid="float-bath-canvas"
            onMouseMove={(e) => {
              const rect = (e.currentTarget as HTMLCanvasElement).getBoundingClientRect();
              const relX = ((e.clientX - rect.left) / rect.width) * 760;
              const relY = ((e.clientY - rect.top) / rect.height) * 215;
              const padL = 44;
              const padR = 16;
              const padT = 22;
              const padB = 26;
              const plotW = 760 - padL - padR;
              const plotH = 215 - padT - padB;
              const fx = (relX - padL) / plotW;
              const fy = Math.abs((relY - padT) / plotH - 0.5) * 2;
              if (fx >= 0 && fx <= 1 && fy >= 0 && fy <= 1) {
                const i = Math.min(vis.nx - 1, Math.max(0, Math.floor(fx * vis.nx)));
                const j = Math.min(vis.ny - 1, Math.max(0, Math.floor(fy * vis.ny)));
                setHoverIdx({ i, j });
              } else {
                setHoverIdx(null);
              }
            }}
            onMouseLeave={() => setHoverIdx(null)}
          />
          <div class="sys-probe-row num">
            {probeInfo ? (
              <>
                <span>
                  x = {probeInfo.x.toFixed(1)} m, |y| = {probeInfo.y.toFixed(2)} m (W/2 = {probeInfo.wEdge.toFixed(2)} m)
                </span>
                <span>T_g = {probeInfo.tG.toFixed(1)} °C</span>
                <span>T_Sn = {probeInfo.tSn.toFixed(1)} °C</span>
                <span>u_Sn = {probeInfo.uSn.toFixed(4)} m/s</span>
                <span>δ_film = {probeInfo.filmUm.toFixed(2)} μm</span>
                <span>H = {probeInfo.depth.toFixed(1)} mm</span>
              </>
            ) : (
              <span>Hover anywhere on the 2D float bath to probe all coupled PDE states (u_Sn, T_Sn, T_g, δ_film, H, W(x))</span>
            )}
          </div>
        </>
      )}
    </div>
  );
}

// ---- Main 3D Viewport Coupled Multi-Physics Overlay & HUD ----

const coupledVpGroup = new THREE.Group();
let coupledVpHost: Viewport | null = null;

function isFloatBathDocument(): boolean {
  return detectCoupledPhysicsRegime(store.doc) === 'pilkington_uk5_float_bath';
}

function isSolarCandleDocument(): boolean {
  return detectCoupledPhysicsRegime(store.doc) === 'solar_candle_ignition';
}

function isSwirlCombustorDocument(): boolean {
  return detectCoupledPhysicsRegime(store.doc) === 'annular_swirl_spray_combustor';
}

export type SwirlVolumeMode = 'radiant_flame' | 'temp_gas' | 'nox_ppm' | 'droplet_smd' | 'swirl_vel';

const SWIRL_VOLUME_TABS: { id: SwirlVolumeMode; label: string; shortLabel: string; unit: string }[] = [
  { id: 'radiant_flame', label: 'Volumetric Swirl Flame & Spray Cone', shortLabel: 'Radiant Flame', unit: 'W/m²' },
  { id: 'temp_gas', label: 'Combustor Gas Temperature T_gas', shortLabel: 'Temp T_gas', unit: 'K' },
  { id: 'nox_ppm', label: 'Zeldovich Thermal NOx Formation Rate', shortLabel: 'Thermal NOx', unit: 'ppm' },
  { id: 'droplet_smd', label: 'Liquid Spray Droplet SMD D₃₂', shortLabel: 'Droplet D₃₂', unit: 'μm' },
  { id: 'swirl_vel', label: 'Azimuthal & Axial Swirl Velocity', shortLabel: 'Swirl Vel', unit: 'm/s' },
];

interface SwirlViewportOverlayConfig {
  enabled: boolean;
  volumeMode: SwirlVolumeMode;
  showSprayCone: boolean;
  showFlameCore: boolean;
  showDilutionJets: boolean;
  showSwirlVortex: boolean;
  frameIdx: number;
  fuelPressureBar: number;
  equivRatio: number;
}

export const swirlShared: { overlay: SwirlViewportOverlayConfig } = {
  overlay: {
    enabled: true,
    volumeMode: 'radiant_flame',
    showSprayCone: true,
    showFlameCore: true,
    showDilutionJets: true,
    showSwirlVortex: true,
    frameIdx: 39,
    fuelPressureBar: 12.0,
    equivRatio: 0.88,
  },
};

export function computeSwirlMetricsFromDoc() {
  const getLet = (name: string, fallback: number): number => {
    const found = (store.doc.lets ?? []).find((l) => l.name === name);
    return found && Number.isFinite(found.value) ? found.value : fallback;
  };
  const orificeR = getLet('orifice_r', 3.2);
  const swirlSlotR = getLet('swirl_slot_r', 2.4);
  const swirlOffset = getLet('swirl_offset', 5.5);
  const chamberR = getLet('chamber_r', 22.0);
  const linerWall = getLet('liner_wall', 5.0);
  const coolantTemp = getLet('coolant_temp', 60.0);
  const fuelPressureBar = swirlShared.overlay.fuelPressureBar;
  const equivRatio = swirlShared.overlay.equivRatio;

  const swirlNumber = (swirlOffset * orificeR) / Math.max(0.5, 2.0 * swirlSlotR * swirlSlotR);
  const sprayHalfAngleDeg = Math.min(72.0, Math.max(22.0, 18.0 + 23.5 * Math.pow(swirlNumber, 0.56)));
  const filmThicknessUm =
    295.0 *
    Math.pow(orificeR / 2.6, 0.62) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.28) *
    Math.pow(1.65 / Math.max(0.35, swirlNumber), 0.42);
  const smdUm =
    46.5 *
    Math.pow(filmThicknessUm / 255.0, 0.38) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.32) *
    Math.pow(orificeR / 2.6, 0.28) *
    Math.pow(Math.cos((sprayHalfAngleDeg * Math.PI) / 360.0), 0.25);
  const evapTimeMs = (smdUm * smdUm * 1e-3) / 0.9;
  const unmixedness = Math.min(0.38, 0.02 + 0.085 * Math.pow(evapTimeMs, 0.95));
  const tAdiabaticK = 1280.0 + 1020.0 * equivRatio - 260.0 * Math.pow(Math.max(0, equivRatio - 1.02), 2);
  const diffWeight = Math.min(0.75, 1.95 * unmixedness);
  const hxCoolingDropK = 22.0 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.35) * ((180.0 - coolantTemp) / 120.0);
  const tFlameEffK = (1.0 - diffWeight) * tAdiabaticK + diffWeight * 2350.0 - hxCoolingDropK;
  const linerHotWallC = Math.round(
    coolantTemp + (tFlameEffK - 273.15 - coolantTemp) / (1.0 + 1.85 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.42)),
  );
  const promptNoxPpm = 3.6 * Math.pow(equivRatio, 1.3);
  const o2RadicalFactor = Math.sqrt(Math.max(0.015, 1.08 - equivRatio));
  const residenceMs = 3.2;
  const thermalNoxPpm =
    6.8e11 * (residenceMs * 1e-3) * o2RadicalFactor * Math.exp(-38200.0 / Math.max(1200.0, tFlameEffK));
  const totalNoxPpm = promptNoxPpm + thermalNoxPpm;
  const hxPowerKw = 14.5 + 0.052 * (linerHotWallC - coolantTemp) * Math.pow(5.0 / Math.max(2.0, linerWall), 0.3);

  return {
    orificeR,
    swirlSlotR,
    swirlOffset,
    chamberR,
    linerWall,
    coolantTemp,
    swirlNumber,
    sprayHalfAngleDeg,
    filmThicknessUm,
    smdUm,
    evapTimeMs,
    tFlameEffK,
    linerHotWallC,
    totalNoxPpm,
    hxPowerKw,
  };
}

function populateSwirlCombustor3DGroup(group: THREE.Group, cfg: SwirlViewportOverlayConfig) {
  clearThreeGroup(group);
  const m = computeSwirlMetricsFromDoc();
  const fIdx = Math.max(0, Math.min(39, Math.round(cfg.frameIdx)));
  const tFrac = fIdx / 39; // 0..1 over 4.00 s startup
  const sprayProgress = Math.min(1, tFrac / 0.22);
  const ignited = fIdx >= 5;
  const flameGrowth = ignited ? Math.min(1, (tFrac - 0.12) / 0.45) : 0;
  const quenchActive = fIdx >= 20;

  const zOrifice = 24.0;
  const halfAngRad = (m.sprayHalfAngleDeg * Math.PI) / 180;
  const evapLenMm = Math.min(24.0, Math.max(7.5, 6.0 + 4.8 * m.evapTimeMs)) * sprayProgress;

  // 1. 3D Hollow-Cone Liquid Fuel Spray Sheet & Atomising Ligament Rays
  if (cfg.showSprayCone && sprayProgress > 0.02) {
    const rTop = Math.min(m.chamberR - 1.5, m.orificeR + evapLenMm * Math.tan(halfAngRad * 0.55));
    const sprayCone = new THREE.Mesh(
      new THREE.CylinderGeometry(rTop, m.orificeR * 0.92, evapLenMm, 36, 1, true),
      new THREE.MeshBasicMaterial({
        color: m.smdUm <= 35 ? 0x38bdf8 : 0xfbbf24,
        transparent: true,
        opacity: 0.28,
        blending: THREE.AdditiveBlending,
        side: THREE.DoubleSide,
        depthWrite: false,
        depthTest: false,
      }),
    );
    sprayCone.rotation.x = Math.PI / 2;
    sprayCone.position.set(0, 0, zOrifice + 0.5 * evapLenMm);
    sprayCone.renderOrder = 18;
    group.add(sprayCone);

    const rayPos: number[] = [];
    const rayCol: number[] = [];
    const nDrops = 28;
    for (let k = 0; k < nDrops; k++) {
      const ang0 = (k / nDrops) * Math.PI * 2 + fIdx * 0.18;
      const ang1 = ang0 + 0.35 * m.swirlNumber;
      const x0 = m.orificeR * 0.85 * Math.cos(ang0);
      const y0 = m.orificeR * 0.85 * Math.sin(ang0);
      const x1 = rTop * Math.cos(ang1);
      const y1 = rTop * Math.sin(ang1);
      rayPos.push(x0, y0, zOrifice, x1, y1, zOrifice + evapLenMm);
      rayCol.push(0.22, 0.78, 0.98, 0.95, 0.85, 0.35);
    }
    const rg = new THREE.BufferGeometry();
    rg.setAttribute('position', new THREE.Float32BufferAttribute(rayPos, 3));
    rg.setAttribute('color', new THREE.Float32BufferAttribute(rayCol, 3));
    const sprayLines = new THREE.LineSegments(
      rg,
      new THREE.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.72,
        depthWrite: false,
        depthTest: false,
      }),
    );
    sprayLines.renderOrder = 19;
    group.add(sprayLines);
  }

  // 2. 6 Nested ParaView Volumetric Turbulent Swirl-Flame Iso-Shells inside Chamber Bore (z = 26..68 mm)
  if (cfg.showFlameCore && ignited) {
    const nz = 24;
    const nTheta = 32;
    const shellScales = [0.28, 0.45, 0.62, 0.78, 0.92, 1.04];
    const shellAlphas = [0.42, 0.35, 0.28, 0.22, 0.16, 0.11];
    const zStart = 25.0;
    const zSpan = 41.0 * (0.35 + 0.65 * flameGrowth);

    for (let sIdx = 0; sIdx < shellScales.length; sIdx++) {
      const shScale = shellScales[sIdx];
      const pos: number[] = [];
      const col: number[] = [];
      const idx: number[] = [];

      for (let i = 0; i < nz; i++) {
        const xi = i / (nz - 1);
        const zWorld = zStart + xi * zSpan;
        // Swirl recirculation zone expands to chamber_r * 0.86 around z = 40 mm, then necks down at dilution jets (z = 48 mm)
        const env = Math.sin(Math.pow(xi, 0.68) * Math.PI);
        const quenchPinch = quenchActive && zWorld > 45 ? 1.0 - 0.26 * Math.exp(-Math.pow((zWorld - 48) / 5.5, 2)) : 1.0;
        const rFlame = Math.min(
          m.chamberR * 0.88,
          (m.orificeR + (m.chamberR * 0.78 - m.orificeR) * env) * shScale * quenchPinch,
        );
        const swirlTwist = 0.9 * Math.sin(fIdx * 0.35 - xi * 4.5 * m.swirlNumber);
        const cx = 0.8 * (1 - shScale) * Math.cos(swirlTwist);
        const cy = 0.8 * (1 - shScale) * Math.sin(swirlTwist);

        let cr = 1.0;
        let cg = 0.55;
        let cb = 0.15;
        const fineLeanFrac = Math.max(0, Math.min(1, (48.0 - m.smdUm) / 22.0));
        if (cfg.volumeMode === 'radiant_flame') {
          if (xi < 0.28) {
            // Primary swirl stabilization root: intense blue-violet OH*/CH* premixed sheet when SMD is fine
            const blueW = (1.0 - xi / 0.28) * (0.45 + 0.55 * fineLeanFrac);
            const [br, bg, bb] = blackbodyRgb01(0.78 - 0.22 * fineLeanFrac);
            cr = br * (1 - blueW) + 0.18 * blueW;
            cg = bg * (1 - blueW) + 0.58 * blueW;
            cb = bb * (1 - blueW) + 0.98 * blueW;
          } else {
            const tempNorm = Math.max(
              0.15,
              Math.min(1.0, ((m.tFlameEffK - 1450) / 720) * (1.0 - 0.35 * Math.max(0, xi - 0.6))),
            );
            [cr, cg, cb] = blackbodyRgb01(tempNorm * (1.05 - 0.22 * shScale));
          }
        } else if (cfg.volumeMode === 'temp_gas') {
          const tLocal = (m.tFlameEffK - 220 * Math.abs(xi - 0.42)) * (0.85 + 0.15 * (1 - shScale));
          [cr, cg, cb] = blackbodyRgb01((tLocal - 1100) / 1100);
        } else if (cfg.volumeMode === 'nox_ppm') {
          const noxNorm = Math.min(1, (m.totalNoxPpm / 55.0) * Math.sin(xi * Math.PI));
          const [tr, tg, tb] = turboRamp(noxNorm);
          cr = tr / 255;
          cg = tg / 255;
          cb = tb / 255;
        } else if (cfg.volumeMode === 'droplet_smd') {
          const dNorm = Math.max(0, Math.min(1, (m.smdUm / 60.0) * Math.exp(-2.2 * xi)));
          const [tr, tg, tb] = turboRamp(dNorm);
          cr = tr / 255;
          cg = tg / 255;
          cb = tb / 255;
        } else {
          const uNorm = Math.min(1, (m.swirlNumber / 3.4) * (1.0 - 0.45 * xi));
          const [tr, tg, tb] = turboRamp(uNorm);
          cr = tr / 255;
          cg = tg / 255;
          cb = tb / 255;
        }

        for (let t = 0; t <= nTheta; t++) {
          const ang = (t / nTheta) * Math.PI * 2;
          pos.push(cx + rFlame * Math.cos(ang), cy + rFlame * Math.sin(ang), zWorld);
          col.push(cr, cg, cb);
        }
      }

      const ring = nTheta + 1;
      for (let i = 0; i < nz - 1; i++) {
        for (let t = 0; t < nTheta; t++) {
          const a = i * ring + t;
          const b = (i + 1) * ring + t;
          const c = (i + 1) * ring + (t + 1);
          const d = i * ring + (t + 1);
          idx.push(a, b, c, a, c, d);
        }
      }

      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
      g.setIndex(idx);
      const mesh = new THREE.Mesh(
        g,
        new THREE.MeshBasicMaterial({
          vertexColors: true,
          transparent: true,
          opacity: shellAlphas[sIdx] * flameGrowth,
          blending: THREE.AdditiveBlending,
          side: THREE.DoubleSide,
          depthWrite: false,
          depthTest: false,
        }),
      );
      mesh.renderOrder = 20 + sIdx;
      group.add(mesh);
    }
  }

  // 3. 4 Radial Dilution-Jet Quenching Streams at z = 48 mm
  if (cfg.showDilutionJets) {
    const jetDirs: [number, number][] = [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
    ];
    for (const [dx, dy] of jetDirs) {
      const jetCone = new THREE.Mesh(
        new THREE.CylinderGeometry(1.1, 3.1, 18, 20, 1, true),
        new THREE.MeshBasicMaterial({
          color: 0x38bdf8,
          transparent: true,
          opacity: quenchActive ? 0.42 : 0.18,
          blending: THREE.AdditiveBlending,
          side: THREE.DoubleSide,
          depthWrite: false,
          depthTest: false,
        }),
      );
      jetCone.renderOrder = 27;
      if (dx !== 0) {
        jetCone.rotation.z = dx > 0 ? Math.PI / 2 : -Math.PI / 2;
        jetCone.position.set(dx * 13.5, 0, 48);
      } else {
        jetCone.rotation.x = dy > 0 ? -Math.PI / 2 : Math.PI / 2;
        jetCone.position.set(0, dy * 13.5, 48);
      }
      group.add(jetCone);
    }
  }

  // 4. Helical Swirl Recirculation Streamlines
  if (cfg.showSwirlVortex) {
    const nHelix = 8;
    const hPos: number[] = [];
    const hCol: number[] = [];
    for (let h = 0; h < nHelix; h++) {
      const baseAng = (h / nHelix) * Math.PI * 2;
      for (let s = 0; s < 22; s++) {
        const u0 = s / 22;
        const u1 = (s + 1) / 22;
        const z0 = 24 + u0 * 44;
        const z1 = 24 + u1 * 44;
        const r0 = m.orificeR + (m.chamberR * 0.72 - m.orificeR) * Math.sin(u0 * Math.PI * 0.85);
        const r1 = m.orificeR + (m.chamberR * 0.72 - m.orificeR) * Math.sin(u1 * Math.PI * 0.85);
        const a0 = baseAng + u0 * 2.4 * m.swirlNumber - fIdx * 0.15;
        const a1 = baseAng + u1 * 2.4 * m.swirlNumber - fIdx * 0.15;
        const [cr, cg, cb] = turboRamp(1.0 - 0.65 * u0);
        hPos.push(
          r0 * Math.cos(a0),
          r0 * Math.sin(a0),
          z0,
          r1 * Math.cos(a1),
          r1 * Math.sin(a1),
          z1,
        );
        hCol.push(cr / 255, cg / 255, cb / 255, cr / 255, cg / 255, cb / 255);
      }
    }
    const hg = new THREE.BufferGeometry();
    hg.setAttribute('position', new THREE.Float32BufferAttribute(hPos, 3));
    hg.setAttribute('color', new THREE.Float32BufferAttribute(hCol, 3));
    const helixLines = new THREE.LineSegments(
      hg,
      new THREE.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.62,
        depthWrite: false,
        depthTest: false,
      }),
    );
    helixLines.renderOrder = 28;
    group.add(helixLines);
  }
}

export type CandleVolumeMode =
  | 'radiant_flame'
  | 'temperature'
  | 'soot_smoke'
  | 'oh_chemilum'
  | 'buoyant_vel';

const CANDLE_VOLUME_TABS: { id: CandleVolumeMode; label: string; shortLabel: string; unit: string }[] = [
  { id: 'radiant_flame', label: 'Planckian Radiant Flame + OH* + Smoke', shortLabel: 'Radiant Flame', unit: 'W/m²' },
  { id: 'temperature', label: 'Gas Temperature T_gas (Blackbody)', shortLabel: 'Temp T_gas', unit: 'K' },
  { id: 'soot_smoke', label: 'Soot & Smoke Volume Fraction f_v', shortLabel: 'Soot / Smoke f_v', unit: 'ppm' },
  { id: 'oh_chemilum', label: 'Excited OH*/CH* Chemiluminescence', shortLabel: 'Blue OH*', unit: 'ppm' },
  { id: 'buoyant_vel', label: 'Buoyant Updraft Velocity u_z', shortLabel: 'Updraft u_z', unit: 'm/s' },
];

interface CandleViewportOverlayConfig {
  enabled: boolean;
  volumeMode: CandleVolumeMode;
  showSolarRays: boolean;
  showSmokePlume: boolean;
  showLensSoot: boolean;
  showEntrainment: boolean;
  frameIdx: number;
}

const candleShared: {
  designP: number[];
  epistemicTheta: number[];
  bundle: CoupledSolveBundle | null;
  summary: CoupledSystemSummary | null;
  overlay: CandleViewportOverlayConfig;
} = {
  designP: [65.0, 68.0, 68.0, 950.0, 7.5, 0.21],
  epistemicTheta: [1.0, 1.0, 0.115, 0.86],
  bundle: null,
  summary: null,
  overlay: {
    enabled: true,
    volumeMode: 'radiant_flame',
    showSolarRays: true,
    showSmokePlume: true,
    showLensSoot: true,
    showEntrainment: true,
    frameIdx: 35,
  },
};

function ensureCandleSolved() {
  if (candleShared.bundle && candleShared.bundle.candle_flame_visual) return;
  try {
    const cat = listCoupledSystems();
    const sys = cat.find((s) => s.system_id === 'solar_candle_ignition_v1');
    if (sys) {
      candleShared.summary = sys;
      candleShared.designP = sys.design_parameters.map((p) => p.nominal);
      candleShared.epistemicTheta = sys.epistemic_parameters.map((t) => t.nominal);
      candleShared.bundle = solveCoupledSystem(
        sys.system_id,
        candleShared.designP,
        candleShared.epistemicTheta,
      );
    }
  } catch {
    // ignore before wasm init
  }
}

/** Blackbody / Planckian colour ramp: s in [0, 1] -> [r, g, b] in 0..1 */
function blackbodyRgb01(s: number): [number, number, number] {
  const x = Math.max(0, Math.min(1, s));
  if (x < 0.25) {
    const u = x / 0.25;
    return [0.18 + 0.62 * u, 0.04 + 0.08 * u, 0.06 + 0.04 * u];
  }
  if (x < 0.55) {
    const u = (x - 0.25) / 0.30;
    return [0.80 + 0.18 * u, 0.12 + 0.44 * u, 0.10 * (1 - u)];
  }
  if (x < 0.82) {
    const u = (x - 0.55) / 0.27;
    return [0.98 + 0.02 * u, 0.56 + 0.34 * u, 0.05 + 0.38 * u];
  }
  const u = (x - 0.82) / 0.18;
  return [1.0, 0.90 + 0.10 * u, 0.43 + 0.52 * u];
}

/**
 * Populate a Three.js Group with the ParaView-style 3D Volumetric Solar-Focused Candle Flame,
 * Blue OH* Base Sheet, Beer–Lambert Smoke Plume, Thermophoretic Lens Soot Deposit, Refractive
 * Solar Caustic Cone, and Buoyant Convective Entrainment Streamlines.
 */
function populateSolarCandle3DGroup(
  group: THREE.Group,
  vis: CandleFlameVisual,
  designP: number[],
  cfg: CandleViewportOverlayConfig,
  coordSpace: 'workbench' | 'fsm_viewport',
) {
  clearThreeGroup(group);
  const frames = vis.frames ?? [];
  if (frames.length === 0 || vis.nz < 2) return;

  const fIdx = Math.max(0, Math.min(frames.length - 1, Math.round(cfg.frameIdx)));
  const frame: CandleTransientFrame = frames[fIdx];

  const dLens = designP[0] ?? vis.lens_diameter_mm ?? 65.0;
  let rLens = Math.max(17.5, Math.min(55.0, 0.5 * dLens));
  const zLensAboveWick = designP[2] ?? vis.lens_height_mm ?? 68.0;
  let lWick = designP[4] ?? 7.5;

  const candleH = 36.0;
  const wickBaseZ = candleH;
  let wickTopZ = wickBaseZ + lWick;
  let lensZ = wickTopZ + zLensAboveWick;

  if (coordSpace === 'fsm_viewport') {
    const lets = store.doc.lets ?? [];
    const docLensR = lets.find((l) => l.name === 'lens_r')?.value;
    const docLensZ = lets.find((l) => l.name === 'lens_z')?.value;
    const docWickLen = lets.find((l) => l.name === 'wick_len')?.value;
    if (typeof docLensR === 'number' && Number.isFinite(docLensR)) rLens = docLensR;
    if (typeof docWickLen === 'number' && Number.isFinite(docWickLen)) {
      lWick = docWickLen;
      wickTopZ = wickBaseZ + lWick;
    }
    if (typeof docLensZ === 'number' && Number.isFinite(docLensZ)) lensZ = docLensZ;
  }

  const sootCenterNm = frame.lens_soot_thickness_nm ?? 0;
  const sootRadiusMm = Math.max(4.5, frame.lens_soot_radius_mm ?? 0.38 * rLens);
  const lensTransmittanceFrac = Math.max(0.15, Math.min(1.0, (frame.lens_transmittance_pct ?? 91.8) / 91.8));

  // In 'workbench' mode, render the 3D brass holder, paraffin candle, dynamic molten wax pool, wick & BK7 lens
  if (coordSpace === 'workbench') {
    const brassBase = new THREE.Mesh(
      new THREE.CylinderGeometry(24, 26, 6, 40),
      new THREE.MeshStandardMaterial({ color: 0xd4a84f, roughness: 0.32, metalness: 0.72 }),
    );
    brassBase.rotation.x = Math.PI / 2;
    brassBase.position.set(0, 0, 3);
    group.add(brassBase);

    const candleCol = new THREE.Mesh(
      new THREE.CylinderGeometry(16, 16, 33, 40),
      new THREE.MeshStandardMaterial({ color: 0xf5efe2, roughness: 0.55, metalness: 0.04 }),
    );
    candleCol.rotation.x = Math.PI / 2;
    candleCol.position.set(0, 0, 19.5);
    group.add(candleCol);

    // Dynamic molten liquid paraffin pool at the top of the candle cup (z = 36.1 mm)
    if (frame.wax_pool_radius_mm > 0.25) {
      const poolR = Math.min(15.2, frame.wax_pool_radius_mm * 1.35);
      const waxPool = new THREE.Mesh(
        new THREE.CylinderGeometry(poolR, poolR * 0.85, 1.2, 36),
        new THREE.MeshStandardMaterial({
          color: 0xfbbf24,
          emissive: frame.ignited ? 0xd97706 : 0x78350f,
          emissiveIntensity: frame.ignited ? 0.45 : 0.2,
          roughness: 0.15,
          metalness: 0.1,
          transparent: true,
          opacity: 0.82,
        }),
      );
      waxPool.rotation.x = Math.PI / 2;
      waxPool.position.set(0, 0, 35.8);
      group.add(waxPool);
    }

    // Braided cotton wick + glowing hot tip
    const wickGlow = Math.max(0, Math.min(1, (frame.wick_temp_k - 320) / 420));
    const wickMesh = new THREE.Mesh(
      new THREE.CylinderGeometry(1.25, 1.4, lWick, 20),
      new THREE.MeshStandardMaterial({
        color: 0x2c221e,
        emissive: new THREE.Color(wickGlow * 0.95, wickGlow * 0.32, 0.02),
        roughness: 0.8,
      }),
    );
    wickMesh.rotation.x = Math.PI / 2;
    wickMesh.position.set(0, 0, wickBaseZ + 0.5 * lWick);
    group.add(wickMesh);

    // Overhead Biconvex BK7 Optical Glass Lens & Gimbal Ring
    const gimbalRing = new THREE.Mesh(
      new THREE.TorusGeometry(rLens + 2.2, 2.2, 16, 48),
      new THREE.MeshStandardMaterial({ color: 0x64748b, roughness: 0.35, metalness: 0.75 }),
    );
    gimbalRing.position.set(0, 0, lensZ);
    group.add(gimbalRing);

    const glassLens = new THREE.Mesh(
      new THREE.SphereGeometry(rLens, 40, 24),
      new THREE.MeshPhysicalMaterial({
        color: 0x93c5fd,
        transparent: true,
        opacity: 0.42,
        roughness: 0.08,
        metalness: 0.05,
        side: THREE.DoubleSide,
      }),
    );
    glassLens.scale.set(1, 1, Math.max(0.14, Math.min(0.28, 7.5 / rLens)));
    glassLens.position.set(0, 0, lensZ);
    group.add(glassLens);
  }

  // 1. Solar Optical Caustic Ray Cone & Focal Spot Glow
  if (cfg.showSolarRays) {
    const spotR = Math.max(0.6, vis.spot_radius_mm);
    // Translucent golden caustic cone from lens down to wick tip
    const coneH = Math.max(10, lensZ - wickTopZ);
    const causticCone = new THREE.Mesh(
      new THREE.CylinderGeometry(rLens * 0.92, spotR, coneH, 36, 1, true),
      new THREE.MeshBasicMaterial({
        color: 0xfde047,
        transparent: true,
        opacity: 0.14,
        blending: THREE.AdditiveBlending,
        side: THREE.DoubleSide,
        depthWrite: false,
      }),
    );
    causticCone.rotation.x = Math.PI / 2;
    causticCone.position.set(0, 0, wickTopZ + 0.5 * coneH);
    group.add(causticCone);

    // 24 individual collimated incoming solar rays + refracted converging caustic rays
    const rayPos: number[] = [];
    const rayCol: number[] = [];
    const nRays = 24;
    for (let k = 0; k < nRays; k++) {
      const ang = (k / nRays) * Math.PI * 2;
      const ringFrac = k % 2 === 0 ? 0.88 : 0.52;
      const rxLens = rLens * ringFrac * Math.cos(ang);
      const ryLens = rLens * ringFrac * Math.sin(ang);
      const rxSpot = spotR * ringFrac * Math.cos(ang);
      const rySpot = spotR * ringFrac * Math.sin(ang);

      // Collimated sunlight above lens
      rayPos.push(rxLens, ryLens, lensZ + 22, rxLens, ryLens, lensZ);
      rayCol.push(1.0, 0.92, 0.35, 1.0, 0.85, 0.25);
      // Refracted ray converging to focal spot on wick
      rayPos.push(rxLens, ryLens, lensZ, rxSpot, rySpot, wickTopZ);
      rayCol.push(1.0, 0.85, 0.25, 1.0, 0.98, 0.78);
    }
    const rayGeom = new THREE.BufferGeometry();
    rayGeom.setAttribute('position', new THREE.Float32BufferAttribute(rayPos, 3));
    rayGeom.setAttribute('color', new THREE.Float32BufferAttribute(rayCol, 3));
    group.add(
      new THREE.LineSegments(
        rayGeom,
        new THREE.LineBasicMaterial({
          vertexColors: true,
          transparent: true,
          opacity: 0.58,
          blending: THREE.AdditiveBlending,
          depthWrite: false,
        }),
      ),
    );

    // Glowing solar focal spot halo at the wick tip
    const spotHalo = new THREE.Mesh(
      new THREE.SphereGeometry(Math.max(1.8, spotR * 1.45), 24, 18),
      new THREE.MeshBasicMaterial({
        color: frame.ignited ? 0xfef08a : 0xfbbf24,
        transparent: true,
        opacity: 0.65,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      }),
    );
    spotHalo.position.set(0, 0, wickTopZ);
    group.add(spotHalo);
  }

  // 2. ParaView-Style Volumetric Ray-Marched Radiant Flame (Nested Additive Iso-Shells)
  const nz = vis.nz;
  const nTheta = 36;
  const zOrigin = wickBaseZ + 0.35 * lWick;

  if (frame.ignited) {
    // 2a. Blue OH*/CH* Chemiluminescence Base Sheet (cupping the wick base, z_local in 0..14 mm)
    if (cfg.volumeMode === 'radiant_flame' || cfg.volumeMode === 'oh_chemilum') {
      const ohShells = [0.86, 0.98, 1.10];
      for (let sIdx = 0; sIdx < ohShells.length; sIdx++) {
        const rScale = ohShells[sIdx];
        const pos: number[] = [];
        const col: number[] = [];
        const idx: number[] = [];
        const nBaseStations = Math.min(nz, 9);
        for (let i = 0; i < nBaseStations; i++) {
          const zLoc = vis.z_mm[i] ?? 0;
          const zWorld = zOrigin + zLoc * 0.72;
          const rBase = Math.max(1.4, (frame.r_flame_mm[i] ?? 2.0) * rScale);
          const ohVal = frame.y_oh_ppm[i] ?? 0;
          const ohNorm = Math.max(0, Math.min(1, ohVal / 280.0));
          for (let t = 0; t <= nTheta; t++) {
            const ang = (t / nTheta) * Math.PI * 2;
            pos.push(rBase * Math.cos(ang), rBase * Math.sin(ang), zWorld);
            col.push(0.12 * ohNorm, 0.48 * ohNorm, 0.98 * Math.pow(ohNorm, 0.65));
          }
        }
        const ring = nTheta + 1;
        for (let i = 0; i < nBaseStations - 1; i++) {
          for (let t = 0; t < nTheta; t++) {
            const a = i * ring + t;
            const b = (i + 1) * ring + t;
            const c = (i + 1) * ring + (t + 1);
            const d = i * ring + (t + 1);
            idx.push(a, b, c, a, c, d);
          }
        }
        const g = new THREE.BufferGeometry();
        g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
        g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
        g.setIndex(idx);
        const m = new THREE.Mesh(
          g,
          new THREE.MeshBasicMaterial({
            vertexColors: true,
            transparent: true,
            opacity: 0.32 - sIdx * 0.06,
            blending: THREE.AdditiveBlending,
            side: THREE.DoubleSide,
            depthWrite: false,
          }),
        );
        m.renderOrder = 10 + sIdx;
        group.add(m);
      }
    }

    // 2b. 6 Nested Volumetric Luminous Flame Iso-Shells (from inner white-hot core to outer orange-red mantle)
    const shellScales = [0.26, 0.42, 0.58, 0.74, 0.89, 1.04];
    const shellOpacities = [0.46, 0.38, 0.32, 0.25, 0.19, 0.13];

    for (let sIdx = 0; sIdx < shellScales.length; sIdx++) {
      const shScale = shellScales[sIdx];
      const pos: number[] = [];
      const col: number[] = [];
      const idx: number[] = [];

      for (let i = 0; i < nz; i++) {
        const zLoc = vis.z_mm[i] ?? 0;
        const zWorld = zOrigin + zLoc;
        const hFlame = Math.max(8.0, frame.flame_height_mm);
        const xi = zLoc / hFlame;
        // Pinch the luminous flame shells smoothly to a pointed tip at z = 1.08 * hFlame
        const tipPinch = xi <= 1.08 ? Math.pow(Math.max(0, 1 - Math.pow(xi / 1.08, 1.6)), 0.55) : 0.02;
        const rShell = Math.max(0.15, (frame.r_flame_mm[i] ?? 2.0) * shScale * (0.25 + 0.85 * tipPinch));

        // Slight helical buoyancy flicker sway
        const swayAmp = Math.min(2.2, 0.035 * zLoc) * (xi <= 1.2 ? 1.0 : 0.4);
        const cx = swayAmp * Math.sin(frame.flicker_phase_rad - 0.11 * zLoc);
        const cy = 0.45 * swayAmp * Math.cos(frame.flicker_phase_rad - 0.09 * zLoc);

        const tSheet = frame.t_sheet_k[i] ?? 300;
        const fvSoot = frame.fv_soot_ppm[i] ?? 0;
        const yOh = frame.y_oh_ppm[i] ?? 0;
        const uz = frame.u_z_m_s[i] ?? 0;

        let cr = 1.0;
        let cg = 0.6;
        let cb = 0.15;
        if (cfg.volumeMode === 'radiant_flame') {
          // Inner shells glow hotter (incandescent yellow-white soot core), outer shells glow warm orange-red
          const coreBoost = 1.0 - 0.32 * Math.abs(shScale - 0.52);
          const tempNorm = Math.max(0, Math.min(1, ((tSheet - 850) / 1250) * coreBoost)) * (xi <= 1.12 ? 1.0 : 0.08);
          [cr, cg, cb] = blackbodyRgb01(tempNorm);
        } else if (cfg.volumeMode === 'temperature') {
          const tempNorm = Math.max(0, Math.min(1, (tSheet - 300) / 1850));
          [cr, cg, cb] = blackbodyRgb01(tempNorm);
        } else if (cfg.volumeMode === 'soot_smoke') {
          const sootNorm = Math.max(0, Math.min(1, fvSoot / 3.8));
          [cr, cg, cb] = blackbodyRgb01(sootNorm * 0.82);
        } else if (cfg.volumeMode === 'oh_chemilum') {
          const ohNorm = Math.max(0, Math.min(1, yOh / 280));
          cr = 0.15 * ohNorm;
          cg = 0.55 * ohNorm;
          cb = 1.0 * Math.pow(ohNorm, 0.6);
        } else {
          const [tr, tg, tb] = turboRamp(Math.max(0, Math.min(1, uz / 0.95)));
          cr = tr / 255;
          cg = tg / 255;
          cb = tb / 255;
        }

        // Fade out luminous emission above the flame tip (where soot cools into dark smoke)
        const lumFade = xi <= 1.05 ? 1.0 : Math.exp(-3.5 * (xi - 1.05));

        for (let t = 0; t <= nTheta; t++) {
          const ang = (t / nTheta) * Math.PI * 2;
          pos.push(cx + rShell * Math.cos(ang), cy + rShell * Math.sin(ang), zWorld);
          col.push(cr * lumFade, cg * lumFade, cb * lumFade);
        }
      }

      const ring = nTheta + 1;
      for (let i = 0; i < nz - 1; i++) {
        for (let t = 0; t < nTheta; t++) {
          const a = i * ring + t;
          const b = (i + 1) * ring + t;
          const c = (i + 1) * ring + (t + 1);
          const d = i * ring + (t + 1);
          idx.push(a, b, c, a, c, d);
        }
      }

      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
      g.setIndex(idx);
      const m = new THREE.Mesh(
        g,
        new THREE.MeshBasicMaterial({
          vertexColors: true,
          transparent: true,
          opacity: shellOpacities[sIdx],
          blending: THREE.AdditiveBlending,
          side: THREE.DoubleSide,
          depthWrite: false,
        }),
      );
      m.renderOrder = 20 + sIdx;
      group.add(m);
    }
  } else if (frame.pyrolysis_rate_mg_s > 0.01) {
    // Pre-ignition paraffin pyrolysis vapor mist plume rising from the solar-heated wick
    const mistShells = [0.55, 0.95];
    for (let sIdx = 0; sIdx < mistShells.length; sIdx++) {
      const shScale = mistShells[sIdx];
      const pos: number[] = [];
      const col: number[] = [];
      const idx: number[] = [];
      const nVapor = Math.min(nz, 14);
      for (let i = 0; i < nVapor; i++) {
        const zLoc = vis.z_mm[i] ?? 0;
        const zWorld = zOrigin + zLoc * 0.55;
        const rMist = (1.2 + 0.11 * zLoc) * shScale;
        const fade = Math.exp(-zLoc / 28.0);
        for (let t = 0; t <= nTheta; t++) {
          const ang = (t / nTheta) * Math.PI * 2;
          pos.push(rMist * Math.cos(ang), rMist * Math.sin(ang), zWorld);
          col.push(0.82 * fade, 0.86 * fade, 0.94 * fade);
        }
      }
      const ring = nTheta + 1;
      for (let i = 0; i < nVapor - 1; i++) {
        for (let t = 0; t < nTheta; t++) {
          const a = i * ring + t;
          const b = (i + 1) * ring + t;
          const c = (i + 1) * ring + (t + 1);
          const d = i * ring + (t + 1);
          idx.push(a, b, c, a, c, d);
        }
      }
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
      g.setIndex(idx);
      const m = new THREE.Mesh(
        g,
        new THREE.MeshBasicMaterial({
          vertexColors: true,
          transparent: true,
          opacity: 0.28 - sIdx * 0.08,
          side: THREE.DoubleSide,
          depthWrite: false,
        }),
      );
      m.renderOrder = 18 + sIdx;
      group.add(m);
    }
  }

  // 3. Beer–Lambert Absorbing Carbonaceous Smoke Plume above the Flame Tip (with Stagnation Deflection under Lens)
  if (cfg.showSmokePlume && (frame.ignited || frame.smoke_emission_ug_s > 0.3)) {
    const smokeIntensity = Math.max(0.18, Math.min(1.0, frame.smoke_emission_ug_s / 24.0));
    const smokeShells = [0.48, 0.82, 1.15];
    const lensBottomZ = lensZ - 3.25;
    for (let sIdx = 0; sIdx < smokeShells.length; sIdx++) {
      const shScale = smokeShells[sIdx];
      const pos: number[] = [];
      const col: number[] = [];
      const idx: number[] = [];
      const startI = frame.ignited ? Math.max(4, Math.floor(nz * 0.28)) : 1;

      for (let i = startI; i < nz; i++) {
        const zLoc = vis.z_mm[i] ?? 0;
        const rawZ = zOrigin + zLoc;
        // Clamp smoke below the lens underside and flare radially outward as a stagnation plume
        const zWorld = Math.min(lensBottomZ - 0.15 * sIdx, rawZ);
        const impingeProximity = Math.max(0, 1.0 - Math.max(0, lensBottomZ - rawZ) / 18.0);
        const hFlame = Math.max(10.0, frame.flame_height_mm || 18.0);
        const aboveTip = Math.max(0, (zLoc - 0.72 * hFlame) / 45.0);
        const radialFlare = impingeProximity > 0 ? 1.0 + 1.85 * Math.pow(impingeProximity, 1.5) : 1.0;
        const rSmoke = Math.min(rLens * 0.92, (1.1 + 4.8 * Math.pow(aboveTip, 0.72)) * shScale * radialFlare);
        const swayX = (1.0 - 0.65 * impingeProximity) * 2.4 * aboveTip * Math.sin(frame.flicker_phase_rad - 0.13 * zLoc);
        const swayY = (1.0 - 0.65 * impingeProximity) * 1.4 * aboveTip * Math.cos(frame.flicker_phase_rad - 0.10 * zLoc);
        const shade = (0.16 + 0.12 * sIdx) * (1.0 - 0.35 * impingeProximity);

        for (let t = 0; t <= 28; t++) {
          const ang = (t / 28) * Math.PI * 2;
          pos.push(swayX + rSmoke * Math.cos(ang), swayY + rSmoke * Math.sin(ang), zWorld);
          col.push(shade, shade, shade + 0.02);
        }
      }

      const ring = 29;
      const nRows = nz - startI;
      for (let i = 0; i < nRows - 1; i++) {
        for (let t = 0; t < 28; t++) {
          const a = i * ring + t;
          const b = (i + 1) * ring + t;
          const c = (i + 1) * ring + (t + 1);
          const d = i * ring + (t + 1);
          idx.push(a, b, c, a, c, d);
        }
      }
      if (idx.length > 0) {
        const g = new THREE.BufferGeometry();
        g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
        g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
        g.setIndex(idx);
        const m = new THREE.Mesh(
          g,
          new THREE.MeshBasicMaterial({
            vertexColors: true,
            transparent: true,
            opacity: (0.28 - sIdx * 0.06) * smokeIntensity,
            side: THREE.DoubleSide,
            depthWrite: false,
          }),
        );
        m.renderOrder = 30 + sIdx;
        group.add(m);
      }
    }
  }

  // 3b. Thermophoretic Carbonaceous Soot Deposition on the Overhead Optical Glass Lens Underside & Top Silhouette
  if (cfg.showLensSoot && sootCenterNm > 0.35) {
    const nRings = 16;
    const nSec = 40;
    const maxSmudgeR = Math.min(rLens * 0.94, sootRadiusMm * 1.65);
    const depositStrength = Math.min(0.92, 1.0 - Math.exp(-0.026 * sootCenterNm)) * (0.45 + 0.55 * (1.0 - lensTransmittanceFrac + 0.35));

    // Render on both the bottom lens impingement face (lensZ - 3.12) and top optical face (lensZ + 3.12)
    // so the dark velvety soot smudge and warm amber-brown PAH tar halo are visible from above and below.
    const zOffsets = [-3.12, 3.12];
    const layerAlphaFactors = [0.95, 0.78];

    for (let lIdx = 0; lIdx < zOffsets.length; lIdx++) {
      const zSurf = lensZ + zOffsets[lIdx];
      const pos: number[] = [];
      const col: number[] = [];
      const idx: number[] = [];

      for (let j = 0; j < nRings; j++) {
        const rFrac = j / (nRings - 1);
        const rMm = rFrac * maxSmudgeR;
        const gauss = Math.exp(-Math.pow(rMm / Math.max(2.5, sootRadiusMm), 2));
        const localNm = sootCenterNm * gauss;
        // Core is deep velvety carbon-black (#080706); mid-halo is warm brown-amber PAH condensates (#4a2b13); outer edge fades into glass (#6c8fb8)
        const coreWeight = Math.min(1.0, localNm / 22.0);
        const haloWeight = Math.max(0.0, Math.sin(Math.min(1.0, rFrac) * Math.PI)) * (1.0 - 0.65 * coreWeight);
        const cr = 0.04 * coreWeight + 0.36 * haloWeight + 0.35 * (1.0 - coreWeight - 0.5 * haloWeight);
        const cg = 0.03 * coreWeight + 0.20 * haloWeight + 0.52 * (1.0 - coreWeight - 0.5 * haloWeight);
        const cb = 0.03 * coreWeight + 0.09 * haloWeight + 0.72 * (1.0 - coreWeight - 0.5 * haloWeight);

        for (let s = 0; s <= nSec; s++) {
          const ang = (s / nSec) * Math.PI * 2;
          pos.push(rMm * Math.cos(ang), rMm * Math.sin(ang), zSurf);
          col.push(Math.max(0.02, Math.min(1, cr)), Math.max(0.02, Math.min(1, cg)), Math.max(0.02, Math.min(1, cb)));
        }
      }

      const ringStride = nSec + 1;
      for (let j = 0; j < nRings - 1; j++) {
        for (let s = 0; s < nSec; s++) {
          const a = j * ringStride + s;
          const b = (j + 1) * ringStride + s;
          const c = (j + 1) * ringStride + (s + 1);
          const d = j * ringStride + (s + 1);
          idx.push(a, b, c, a, c, d);
        }
      }

      const gSoot = new THREE.BufferGeometry();
      gSoot.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
      gSoot.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
      gSoot.setIndex(idx);
      const mSoot = new THREE.Mesh(
        gSoot,
        new THREE.MeshBasicMaterial({
          vertexColors: true,
          transparent: true,
          opacity: Math.min(0.88, depositStrength * layerAlphaFactors[lIdx]),
          side: THREE.DoubleSide,
          depthWrite: false,
        }),
      );
      mSoot.renderOrder = 36 + lIdx;
      group.add(mSoot);
    }

    // Concentric soot thickness contour rings on the lens so the radial thermophoretic profile stands out clearly
    const contourRadii = [0.32 * sootRadiusMm, 0.68 * sootRadiusMm, 1.05 * sootRadiusMm].filter((r) => r < rLens * 0.92);
    for (let cIdx = 0; cIdx < contourRadii.length; cIdx++) {
      const rRing = contourRadii[cIdx];
      const ringPts: number[] = [];
      for (let s = 0; s < 48; s++) {
        const a0 = (s / 48) * Math.PI * 2;
        const a1 = ((s + 1) / 48) * Math.PI * 2;
        for (const zOff of [-3.15, 3.15]) {
          ringPts.push(
            rRing * Math.cos(a0),
            rRing * Math.sin(a0),
            lensZ + zOff,
            rRing * Math.cos(a1),
            rRing * Math.sin(a1),
            lensZ + zOff,
          );
        }
      }
      const rg = new THREE.BufferGeometry();
      rg.setAttribute('position', new THREE.Float32BufferAttribute(ringPts, 3));
      group.add(
        new THREE.LineSegments(
          rg,
          new THREE.LineBasicMaterial({
            color: cIdx === 0 ? 0x1c1917 : cIdx === 1 ? 0x78350f : 0xb45309,
            transparent: true,
            opacity: Math.min(0.85, 0.35 + 0.012 * sootCenterNm),
          }),
        ),
      );
    }
  }

  // 4. Buoyant Convective Air Entrainment & Plume Streamlines
  if (cfg.showEntrainment) {
    const nStreams = 12;
    const streamPos: number[] = [];
    const streamCol: number[] = [];
    for (let s = 0; s < nStreams; s++) {
      const ang = (s / nStreams) * Math.PI * 2;
      for (let i = 0; i < nz - 1; i++) {
        const z0 = zOrigin + (vis.z_mm[i] ?? 0);
        const z1 = zOrigin + (vis.z_mm[i + 1] ?? 0);
        const frac0 = i / (nz - 1);
        const frac1 = (i + 1) / (nz - 1);
        // Radial air entrainment draws streamlines inward near the base, then buoyant plume expands gently aloft
        const r0 = 22.0 * Math.exp(-3.2 * frac0) + 6.5 + 4.5 * frac0;
        const r1 = 22.0 * Math.exp(-3.2 * frac1) + 6.5 + 4.5 * frac1;
        const uz0 = frame.u_z_m_s[i] ?? 0.1;
        const [cr0, cg0, cb0] = turboRamp(Math.min(1, uz0 / 0.9));
        streamPos.push(
          r0 * Math.cos(ang),
          r0 * Math.sin(ang),
          z0,
          r1 * Math.cos(ang),
          r1 * Math.sin(ang),
          z1,
        );
        streamCol.push(cr0 / 255, cg0 / 255, cb0 / 255, cr0 / 255, cg0 / 255, cb0 / 255);
      }
    }
    const sg = new THREE.BufferGeometry();
    sg.setAttribute('position', new THREE.Float32BufferAttribute(streamPos, 3));
    sg.setAttribute('color', new THREE.Float32BufferAttribute(streamCol, 3));
    group.add(
      new THREE.LineSegments(
        sg,
        new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.65 }),
      ),
    );
  }
}

function syncCoupledViewportOverlay(vp: Viewport) {
  if (!coupledVpHost) {
    coupledVpHost = vp;
    vp.overlays.add(coupledVpGroup);
  }
  if (isSwirlCombustorDocument()) {
    if (!swirlShared.overlay.enabled) {
      if (coupledVpGroup.children.length > 0) {
        clearThreeGroup(coupledVpGroup);
        vp.invalidate();
      }
      return;
    }
    populateSwirlCombustor3DGroup(coupledVpGroup, swirlShared.overlay);
    vp.invalidate();
    return;
  }

  if (isSolarCandleDocument()) {
    if (!candleShared.overlay.enabled) {
      if (coupledVpGroup.children.length > 0) {
        clearThreeGroup(coupledVpGroup);
        vp.invalidate();
      }
      return;
    }
    ensureCandleSolved();
    const vis = candleShared.bundle?.candle_flame_visual;
    if (!vis) return;
    populateSolarCandle3DGroup(
      coupledVpGroup,
      vis,
      candleShared.designP,
      candleShared.overlay,
      'fsm_viewport',
    );
    vp.invalidate();
    return;
  }

  if (!isFloatBathDocument() || !coupledShared.overlay.enabled) {
    if (coupledVpGroup.children.length > 0) {
      clearThreeGroup(coupledVpGroup);
      vp.invalidate();
    }
    return;
  }
  ensureCoupledSolved();
  const vis = coupledShared.bundle?.float_bath_visual;
  if (!vis) return;

  populateFloatBath3DGroup(
    coupledVpGroup,
    vis,
    coupledShared.designP,
    coupledShared.overlay,
    'fsm_viewport',
  );
  vp.invalidate();
}

viewportExtensions.push({
  sync: (vp) => {
    syncCoupledViewportOverlay(vp);
  },
});

function CoupledViewportHud() {
  useApp();
  const [, setTick] = useState(0);
  const bump = () => {
    setTick((t) => t + 1);
    if (liveViewport) syncCoupledViewportOverlay(liveViewport);
  };

  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const resp = await fetch(apiUrl('/api/workspace'));
        if (!resp.ok || !active) return;
        const ws = (await resp.json()) as WorkspaceSnapshot;
        if (lastSeenWorkspaceRevision < 0) {
          lastSeenWorkspaceRevision = typeof ws.revision === 'number' ? ws.revision : 0;
          return;
        }
        if (typeof ws.revision === 'number' && ws.revision > lastSeenWorkspaceRevision) {
          lastSeenWorkspaceRevision = ws.revision;
          if (ws.actor === 'mcp-agent') {
            if (
              ws.coupled_system_id === 'pilkington_float_bath_cvd_coater_v1' &&
              Array.isArray(ws.design_p)
            ) {
              coupledShared.designP = ws.design_p;
              if (Array.isArray(ws.epistemic_theta)) {
                coupledShared.epistemicTheta = ws.epistemic_theta;
              }
              try {
                coupledShared.bundle = solveCoupledSystem(
                  'pilkington_float_bath_cvd_coater_v1',
                  coupledShared.designP,
                  coupledShared.epistemicTheta,
                );
              } catch {
                // ignore
              }
            } else if (
              ws.coupled_system_id === 'solar_candle_ignition_v1' &&
              Array.isArray(ws.design_p)
            ) {
              candleShared.designP = ws.design_p;
              if (Array.isArray(ws.epistemic_theta)) {
                candleShared.epistemicTheta = ws.epistemic_theta;
              }
              try {
                candleShared.bundle = solveCoupledSystem(
                  'solar_candle_ignition_v1',
                  candleShared.designP,
                  candleShared.epistemicTheta,
                );
              } catch {
                // ignore
              }
            }
            if (typeof ws.doc_fsm === 'string' && ws.doc_fsm.trim().length > 0) {
              ui.tutorialsOpen = false;
              ui.systemsOpen = false;
              openText(ws.doc_fsm, 'mcp-shared-workspace.fsm');
              if (ws.last_tool === 'solve' || ws.last_tool === 'solve_study') {
                store.setLens('results');
              } else if (ws.last_tool === 'set_fill') {
                store.setLens('fill');
              } else if (
                ws.last_tool === 'add_support' ||
                ws.last_tool === 'add_load' ||
                ws.last_tool === 'add_temperature' ||
                ws.last_tool === 'add_convection' ||
                ws.last_tool === 'add_heatflux' ||
                ws.last_tool === 'add_region'
              ) {
                store.setLens('setup');
              } else if (
                ws.last_tool === 'open' ||
                ws.last_tool === 'load_fsm' ||
                ws.last_tool === 'new_document' ||
                ws.last_tool === 'upsert_shape' ||
                ws.last_tool === 'add_primitive'
              ) {
                store.setLens('shape');
              }
              viewApi.fit(false);
              if (liveViewport) syncCoupledViewportOverlay(liveViewport);
              ui.notify(`Synced MCP Co-Pilot update (rev #${ws.revision}: ${ws.last_tool})`);
            }
          }
        }
      } catch {
        // ignore offline
      }
    };
    const timer = window.setInterval(poll, 1000);
    poll();
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  const isMobile = typeof window !== 'undefined' && window.innerWidth <= 900;
  if (ui.systemsOpen || store.lens === 'theory' || (isMobile && (store.lens === 'results' || store.lens === 'make'))) {
    return null;
  }

  if (isSwirlCombustorDocument()) {
    return <SwirlCombustorViewportHud bump={bump} />;
  }

  if (isSolarCandleDocument()) {
    ensureCandleSolved();
    const bundle = candleShared.bundle;
    const vis = bundle?.candle_flame_visual;
    if (!bundle || !vis) return null;
    return <SolarCandleViewportHud bundle={bundle} vis={vis} bump={bump} />;
  }

  if (!isFloatBathDocument()) return null;
  ensureCoupledSolved();
  const bundle = coupledShared.bundle;
  const vis = bundle?.float_bath_visual;
  if (!bundle || !vis) return null;

  const ov = coupledShared.overlay;
  const p = coupledShared.designP;

  const updateDesignParam = (idx: number, val: number) => {
    const nextP = [...coupledShared.designP];
    nextP[idx] = val;
    coupledShared.designP = nextP;
    try {
      coupledShared.bundle = solveCoupledSystem(
        'pilkington_float_bath_cvd_coater_v1',
        nextP,
        coupledShared.epistemicTheta,
      );
    } catch {
      // ignore
    }
    bump();
  };

  const pVar = bundle.analysis.objectives[0] ?? 0;
  const pSin = bundle.analysis.objectives[1] ?? 0;
  const dzBeam = bundle.analysis.objectives[2] ?? 0;
  const tgCoater = bundle.analysis.objectives[3] ?? 0;

  return (
    <div class="sys-vp-hud" data-testid="coupled-viewport-hud">
      <div class="sys-vp-hud-head">
        <span>3D Coupled Multi-Physics Overlay (2,520-DOF Live PDE)</span>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
          <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', fontWeight: 500, cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={ov.enabled}
              data-testid="toggle-vp-coupled-overlay"
              onChange={(e) => {
                ov.enabled = (e.target as HTMLInputElement).checked;
                bump();
              }}
            />
            Show in 3D
          </label>
          <button
            type="button"
            class="btn ghost small"
            data-testid="toggle-vp-hud-collapse"
            title={ui.vpHudCollapsed ? 'Expand overlay controls' : 'Minimize overlay controls'}
            onClick={() => ui.set('vpHudCollapsed', !ui.vpHudCollapsed)}
            style={{ padding: '0 5px', height: '20px', fontSize: '11px' }}
          >
            {ui.vpHudCollapsed ? '▴' : '▾'}
          </button>
        </div>
      </div>

      {ov.enabled && !ui.vpHudCollapsed && (
        <>
          <div class="seg" style={{ width: '100%', display: 'flex' }}>
            {BATH_FIELD_TABS.slice(0, 6).map((t) => (
              <button
                key={t.id}
                style={{ flex: 1, fontSize: '10.5px', padding: '0 4px' }}
                class={ov.mode === t.id ? 'on' : ''}
                data-testid={`vp-field-${t.id}`}
                onClick={() => {
                  ov.mode = t.id;
                  bump();
                }}
              >
                {t.shortLabel}
              </button>
            ))}
          </div>

          <div class="sys-vp-hud-row">
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showVectors}
                onChange={(e) => {
                  ov.showVectors = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              3D u_Sn Vectors
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showRibbon}
                onChange={(e) => {
                  ov.showRibbon = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Curved Ribbon W(x)
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showWaves}
                onChange={(e) => {
                  ov.showWaves = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Optical Waves
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showCoater}
                onChange={(e) => {
                  ov.showCoater = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Coater Bow
            </label>
          </div>

          <div class="sys-vp-hud-sliders">
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Sill Depth H_sill</span>
                <strong class="num">{(p[0] ?? 65).toFixed(0)} mm</strong>
              </div>
              <input
                type="range"
                min={55}
                max={110}
                step={1}
                value={p[0] ?? 65}
                onInput={(e) => updateDesignParam(0, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>CVD Beam x_beam</span>
                <strong class="num">{(p[1] ?? 28.5).toFixed(1)} m</strong>
              </div>
              <input
                type="range"
                min={12}
                max={34}
                step={0.5}
                value={p[1] ?? 28.5}
                onInput={(e) => updateDesignParam(1, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Bath H₂ Atmosphere</span>
                <strong class="num">{(p[3] ?? 4.2).toFixed(1)} %</strong>
              </div>
              <input
                type="range"
                min={2.0}
                max={10.0}
                step={0.2}
                value={p[3] ?? 4.2}
                onInput={(e) => updateDesignParam(3, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Linear Motors I_mot</span>
                <strong class="num">{(p[4] ?? 260).toFixed(0)} A</strong>
              </div>
              <input
                type="range"
                min={0}
                max={420}
                step={10}
                value={p[4] ?? 260}
                onInput={(e) => updateDesignParam(4, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
          </div>

          <div class="sys-vp-hud-row num" style={{ borderTop: '1px solid var(--line)', paddingTop: '5px' }}>
            <span title="Peak Varicose Optical Distortion">
              P_var: <strong style={{ color: pVar <= 14.2 ? 'var(--ok)' : 'var(--danger)' }}>{pVar.toFixed(1)} Mdpt</strong>
            </span>
            <span title="Peak Sinuous Viscoelastic Distortion">
              P_sin: <strong style={{ color: pSin <= 5.0 ? 'var(--ok)' : 'var(--danger)' }}>{pSin.toFixed(2)} Mdpt</strong>
            </span>
            <span title="CVD Coater Beam Mid-Span Thermal Bow">
              Δz_beam: <strong style={{ color: dzBeam <= 0.24 ? 'var(--ok)' : 'var(--danger)' }}>{dzBeam.toFixed(3)} mm</strong>
            </span>
            <span title="Glass Temperature under CVD Coater">
              T_g: <strong>{tgCoater.toFixed(0)} °C</strong>
            </span>
            <button
              class="btn small"
              title="Rebuild 3D .fsm CSG & FEA mesh to match current slider positions"
              onClick={() => {
                openText(buildFloatBathFsm(coupledShared.designP), 'uk5-float-bath-coater.fsm');
                ui.set('resultsCase', 0);
              }}
            >
              Sync Solid .fsm
            </button>
          </div>
        </>
      )}
    </div>
  );
}

// ---- Swirl Atomiser Combustor & Heat Exchanger 3D Viewport HUD & Real-Time Video (T9) ----

async function renderSwirlCombustorVideoWebm(
  renderFrameToCanvas: (frameIdx: number) => HTMLCanvasElement | null,
  onProgress: (pct: number) => void,
): Promise<string> {
  const width = 560;
  const topBannerH = 52;
  const sceneH = 600;
  const bottomBannerH = 52;
  const height = topBannerH + sceneH + bottomBannerH;
  const dstAspect = width / sceneH;

  const compCanvas = document.createElement('canvas');
  compCanvas.width = width;
  compCanvas.height = height;
  const ctx = compCanvas.getContext('2d');
  if (!ctx) throw new Error('Could not create 2D composite canvas context');

  const mimeCandidates = ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm'];
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

  const m = computeSwirlMetricsFromDoc();
  const nFrames = 40;
  recorder.start();

  for (let k = 0; k < nFrames; k++) {
    const timeS = (k / (nFrames - 1)) * 4.0;
    const phaseName =
      k < 5
        ? '1. Swirl Fuel Spin-Up & Hollow-Cone Breakup'
        : k < 14
          ? '2. Spark Kernel & Primary Zone Light-Around'
          : k < 25
            ? '3. Vortex Recirculation & D² Evaporation'
            : k < 35
              ? '4. Dilution Quenching & Thermal NOx Control'
              : '5. Conjugate HX Steady-State Operation';

    const srcCanvas = renderFrameToCanvas(k);

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
        // ignore
      }
    }

    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, 0, width, topBannerH);
    ctx.fillStyle = '#f8fafc';
    ctx.font = 'bold 12px Inter, system-ui, sans-serif';
    ctx.fillText('FIELDSMITH T9 — SWIRL ATOMISER SPRAY, COMBUSTOR & NOx KINETICS', 12, 20);
    ctx.fillStyle = k >= 5 ? '#fbbf24' : '#38bdf8';
    ctx.font = 'bold 11px JetBrains Mono, monospace';
    ctx.fillText(`t = ${timeS.toFixed(2)} s | ${phaseName}`, 12, 39);

    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, height - bottomBannerH, width, bottomBannerH);
    ctx.fillStyle = '#e2e8f0';
    ctx.font = '10.5px JetBrains Mono, monospace';
    ctx.fillText(
      `S_g: ${m.swirlNumber.toFixed(2)} (${m.sprayHalfAngleDeg.toFixed(0)}° cone) | SMD D₃₂: ${m.smdUm.toFixed(1)} μm | τ_evap: ${m.evapTimeMs.toFixed(2)} ms`,
      12,
      height - 34,
    );
    ctx.fillStyle = m.totalNoxPpm <= 25 ? '#4ade80' : '#f87171';
    ctx.fillText(
      `T_flame: ${m.tFlameEffK.toFixed(0)} K | NOx: ${m.totalNoxPpm.toFixed(1)} ppm (${m.totalNoxPpm <= 25 ? 'PASS' : 'HIGH'}) | Liner: ${m.linerHotWallC} °C (${m.hxPowerKw.toFixed(1)} kW HX)`,
      12,
      height - 18,
    );

    const pct = (k + 1) / nFrames;
    ctx.fillStyle = 'rgba(255,255,255,0.14)';
    ctx.fillRect(12, height - 8, width - 24, 4);
    ctx.fillStyle = k >= 5 ? '#f97316' : '#38bdf8';
    ctx.fillRect(12, height - 8, (width - 24) * pct, 4);

    onProgress(Math.round(pct * 100));
    await new Promise((r) => window.setTimeout(r, 42));
  }

  recorder.stop();
  return stopped;
}

function SwirlCombustorViewportHud({ bump }: { bump: () => void }) {
  const [playing, setPlaying] = useState(false);
  const [renderingVideo, setRenderingVideo] = useState(false);
  const [videoPct, setVideoPct] = useState(0);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);

  const ov = swirlShared.overlay;
  const fIdx = Math.max(0, Math.min(39, Math.round(ov.frameIdx)));
  const timeS = (fIdx / 39) * 4.0;
  const m = computeSwirlMetricsFromDoc();

  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      swirlShared.overlay.frameIdx = (swirlShared.overlay.frameIdx + 1) % 40;
      bump();
    }, 85);
    return () => window.clearInterval(timer);
  }, [playing]);

  const milestones = [
    { label: '1. Spray Cone (0.0s)', frame: 2, title: `Hollow-cone sheet breakup (D₃₂ = ${m.smdUm.toFixed(1)} μm)` },
    { label: '2. Spark Kernel (0.5s)', frame: 6, title: 'Spark kernel ignition & primary zone light-around' },
    { label: '3. Vortex Flame (1.4s)', frame: 15, title: `Recirculation vortex flame (${m.tFlameEffK.toFixed(0)} K)` },
    { label: '4. Dilution Quench (2.6s)', frame: 26, title: 'Radial dilution-jet cooling & NOx suppression' },
    { label: '5. Steady HX (4.0s)', frame: 39, title: `Steady combustor & HX balance (NOx = ${m.totalNoxPpm.toFixed(1)} ppm)` },
  ];

  const handleRenderVideo = async () => {
    if (renderingVideo) return;
    setPlaying(false);
    setRenderingVideo(true);
    setVideoPct(0);
    const prevIdx = swirlShared.overlay.frameIdx;
    if (liveViewport) {
      liveViewport.camera.up.set(0, 0, 1);
      viewApi.fit(false);
    }
    try {
      const url = await renderSwirlCombustorVideoWebm(
        (k) => {
          swirlShared.overlay.frameIdx = k;
          if (liveViewport) {
            syncCoupledViewportOverlay(liveViewport);
            return liveViewport.captureCanvas();
          }
          return null;
        },
        (pct) => setVideoPct(pct),
      );
      setVideoUrl(url);
      ui.notify('Rendered real-time 30 fps .webm video of Swirl Combustor Spray & Flame');
    } catch (e) {
      ui.notify(`Video render error: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      swirlShared.overlay.frameIdx = prevIdx;
      setRenderingVideo(false);
      bump();
    }
  };

  return (
    <div
      class="sys-vp-hud"
      data-testid="coupled-viewport-hud"
      style={{
        maxWidth: '465px',
        top: 'auto',
        right: 'auto',
        bottom: '44px',
        left: store.lens === 'results' ? '348px' : '10px',
        zIndex: 25,
      }}
    >
      <div class="sys-vp-hud-head">
        <span>3D Volumetric Swirl Spray, Turbulent Flame &amp; NOx Plume (T9)</span>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
          <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', fontWeight: 500, cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={ov.enabled}
              data-testid="toggle-vp-coupled-overlay"
              onChange={(e) => {
                ov.enabled = (e.target as HTMLInputElement).checked;
                bump();
              }}
            />
            Show in 3D
          </label>
          <button
            type="button"
            class="btn ghost small"
            data-testid="toggle-vp-hud-collapse"
            title={ui.vpHudCollapsed ? 'Expand overlay controls' : 'Minimize overlay controls'}
            onClick={() => ui.set('vpHudCollapsed', !ui.vpHudCollapsed)}
            style={{ padding: '0 5px', height: '20px', fontSize: '11px' }}
          >
            {ui.vpHudCollapsed ? '▴' : '▾'}
          </button>
        </div>
      </div>

      {ov.enabled && !ui.vpHudCollapsed && (
        <>
          <div class="seg" style={{ width: '100%', display: 'flex' }}>
            {SWIRL_VOLUME_TABS.map((t) => (
              <button
                key={t.id}
                style={{ flex: 1, fontSize: '10px', padding: '0 3px' }}
                class={ov.volumeMode === t.id ? 'on' : ''}
                data-testid={`vp-swirl-mode-${t.id}`}
                onClick={() => {
                  ov.volumeMode = t.id;
                  bump();
                }}
              >
                {t.shortLabel}
              </button>
            ))}
          </div>

          <div class="sys-vp-hud-row" style={{ flexWrap: 'wrap', gap: '6px' }}>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showSprayCone}
                onChange={(e) => {
                  ov.showSprayCone = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Hollow Spray Cone
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showFlameCore}
                onChange={(e) => {
                  ov.showFlameCore = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Volumetric Flame
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showDilutionJets}
                onChange={(e) => {
                  ov.showDilutionJets = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Dilution Jets
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showSwirlVortex}
                onChange={(e) => {
                  ov.showSwirlVortex = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Swirl Vortex
            </label>
          </div>

          <div
            style={{
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              borderRadius: '5px',
              padding: '6px 8px',
              display: 'flex',
              flexDirection: 'column',
              gap: '5px',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
                <button
                  type="button"
                  class="mini-btn"
                  data-testid="transient-play-btn"
                  onClick={() => setPlaying(!playing)}
                >
                  {playing ? '⏸ Pause' : '▶ Play'}
                </button>
                <span class="num" style={{ fontSize: '11px', color: fIdx >= 5 ? 'var(--warn)' : 'var(--accent)', fontWeight: 600 }}>
                  t = {timeS.toFixed(2)} s · S_g = {m.swirlNumber.toFixed(2)} · D₃₂ = {m.smdUm.toFixed(1)} μm
                </span>
              </div>
              <button
                type="button"
                class="mini-btn"
                data-testid="render-transient-video"
                disabled={renderingVideo}
                onClick={handleRenderVideo}
                style={{ background: '#ea580c', color: '#fff', borderColor: '#f97316', fontWeight: 600 }}
              >
                {renderingVideo ? `Rendering ${videoPct}%…` : '🎬 Render Video'}
              </button>
            </div>

            <input
              type="range"
              min={0}
              max={39}
              step={1}
              value={fIdx}
              data-testid="transient-time-slider"
              onInput={(e) => {
                setPlaying(false);
                swirlShared.overlay.frameIdx = parseInt((e.target as HTMLInputElement).value, 10) || 0;
                bump();
              }}
            />

            <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
              {milestones.map((ms, idx) => (
                <button
                  key={idx}
                  type="button"
                  class="mini-btn"
                  data-testid={`recap-event-${idx}`}
                  title={ms.title}
                  onClick={() => {
                    setPlaying(false);
                    swirlShared.overlay.frameIdx = ms.frame;
                    bump();
                  }}
                  style={
                    Math.abs(fIdx - ms.frame) <= 2
                      ? { borderColor: '#f59e0b', color: '#fbbf24', fontWeight: 600, fontSize: '10px' }
                      : { fontSize: '10px' }
                  }
                >
                  {ms.label}
                </button>
              ))}
            </div>

            {videoUrl && (
              <div style={{ marginTop: '4px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
                <video
                  src={videoUrl}
                  controls
                  autoPlay
                  loop
                  muted
                  data-testid="transient-video-player"
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
                  <span class="num" style={{ color: 'var(--ok)' }}>✓ Real-time rendered video ready (30 fps WebM)</span>
                  <a
                    href={videoUrl}
                    download="swirl-combustor-spray-flame.webm"
                    class="mini-btn"
                    data-testid="download-transient-video"
                    style={{ textDecoration: 'none' }}
                  >
                    ⬇ Download .webm
                  </a>
                </div>
              </div>
            )}
          </div>

          <div class="sys-vp-hud-row num" style={{ borderTop: '1px solid var(--line)', paddingTop: '5px', flexWrap: 'wrap', gap: '6px' }}>
            <span>
              D₃₂: <strong>{m.smdUm.toFixed(1)} μm</strong>
            </span>
            <span>
              T_flame: <strong>{m.tFlameEffK.toFixed(0)} K</strong>
            </span>
            <span>
              NOx: <strong style={{ color: m.totalNoxPpm <= 25 ? 'var(--ok)' : 'var(--danger)' }}>{m.totalNoxPpm.toFixed(1)} ppm</strong>
            </span>
            <span>
              Liner/HX: <strong>{m.linerHotWallC} °C</strong> ({m.hxPowerKw.toFixed(1)} kW)
            </span>
          </div>
        </>
      )}
    </div>
  );
}

// ---- Interactive 3D Rocket Nozzle Multi-Physics Twin (Pack #2) ----

type NozzleFieldMode = 'T_wall' | 'T_cool' | 'p_cool' | 'sigma_th';

const NOZZLE_FIELD_META: Record<NozzleFieldMode, { label: string; unit: string }> = {
  T_wall: { label: 'Liner Wall Temp T_wall', unit: 'K' },
  T_cool: { label: 'Supercritical H₂ Coolant T_cool', unit: 'K' },
  p_cool: { label: 'Coolant Channel Pressure p_cool', unit: 'MPa' },
  sigma_th: { label: 'Thermo-Elastic Hoop Stress σ_th', unit: 'MPa' },
};

function RocketNozzle3DTwin({
  state,
  designP,
}: {
  state: number[];
  designP: number[];
}) {
  const [nozzleMode, setNozzleMode] = useState<NozzleFieldMode>('T_wall');
  const [showChannels, setShowChannels] = useState(true);
  const [cutaway, setCutaway] = useState(true);
  const mountRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<{
    scene: THREE.Scene;
    root: THREE.Group;
    camera: THREE.PerspectiveCamera;
    controls: OrbitControls;
    renderer: THREE.WebGLRenderer;
  } | null>(null);

  const numStations = Math.max(2, Math.floor(state.length / 4));
  const fieldIdx = nozzleMode === 'T_cool' ? 0 : nozzleMode === 'p_cool' ? 1 : nozzleMode === 'T_wall' ? 2 : 3;

  let minV = Infinity;
  let maxV = -Infinity;
  for (let i = 0; i < numStations; i++) {
    const v = state[i * 4 + fieldIdx] ?? 0;
    if (v < minV) minV = v;
    if (v > maxV) maxV = v;
  }
  if (!Number.isFinite(minV)) minV = 0;
  if (!Number.isFinite(maxV)) maxV = 1;

  useEffect(() => {
    const host = mountRef.current;
    if (!host) return;
    const width = Math.max(host.clientWidth, 420);
    const height = Math.max(host.clientHeight, 260);
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    renderer.setClearColor(0x0b0f17, 1);
    host.innerHTML = '';
    host.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(36, width / height, 1, 2000);
    camera.up.set(0, 0, 1);
    camera.position.set(0, -195, 85);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(0, 0, 0);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;
    controls.update();

    scene.add(new THREE.HemisphereLight(0xe2e8f0, 0x1e293b, 0.95));
    const dirLight = new THREE.DirectionalLight(0xffffff, 0.85);
    dirLight.position.set(90, -140, 160);
    scene.add(dirLight);

    const root = new THREE.Group();
    scene.add(root);
    sceneRef.current = { scene, root, camera, controls, renderer };

    let raf = 0;
    const animate = () => {
      raf = requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    };
    animate();

    const ro = new ResizeObserver(() => {
      if (!host) return;
      const w = Math.max(host.clientWidth, 320);
      const h = Math.max(host.clientHeight, 220);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      renderer.setSize(w, h);
    });
    ro.observe(host);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      clearThreeGroup(root);
      renderer.dispose();
      sceneRef.current = null;
    };
  }, []);

  useEffect(() => {
    const ctx = sceneRef.current;
    if (!ctx || numStations < 2) return;
    clearThreeGroup(ctx.root);

    const hThroatMm = designP[0] ?? 1.55;
    const tWallMm = designP[1] ?? 0.95;
    const tTbcUm = designP[2] ?? 145.0;

    // Build a 3D De Laval converging-diverging rocket chamber revolution surface along X (-75..+75 mm)
    const nTheta = cutaway ? 36 : 48;
    const thetaSpan = cutaway ? Math.PI * 1.35 : Math.PI * 2.0;
    const thetaStart = cutaway ? -Math.PI * 0.15 : 0;

    const positions: number[] = [];
    const colors: number[] = [];
    const indices: number[] = [];

    const radiusAtStation = (i: number): number => {
      const xM = (i / (numStations - 1)) * 0.6;
      const bell = Math.exp(-Math.pow((xM - 0.28) / 0.11, 2));
      const rInner = 34 - 16 * bell + (xM > 0.28 ? (xM - 0.28) * 32 : 0);
      return rInner;
    };

    for (let i = 0; i < numStations; i++) {
      const xPos = ((i / (numStations - 1)) - 0.5) * 150;
      const rIn = radiusAtStation(i);
      const v = state[i * 4 + fieldIdx] ?? 0;
      const [cr, cg, cb] = turboRamp((v - minV) / Math.max(maxV - minV, 1e-6));
      for (let t = 0; t <= nTheta; t++) {
        const ang = thetaStart + (t / nTheta) * thetaSpan;
        const y = rIn * Math.cos(ang);
        const z = rIn * Math.sin(ang);
        positions.push(xPos, y, z);
        colors.push(cr / 255, cg / 255, cb / 255);
      }
    }

    const ringVerts = nTheta + 1;
    for (let i = 0; i < numStations - 1; i++) {
      for (let t = 0; t < nTheta; t++) {
        const a = i * ringVerts + t;
        const b = (i + 1) * ringVerts + t;
        const c = (i + 1) * ringVerts + (t + 1);
        const d = i * ringVerts + (t + 1);
        indices.push(a, b, c, a, c, d);
      }
    }

    const linerGeom = new THREE.BufferGeometry();
    linerGeom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
    linerGeom.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
    linerGeom.setIndex(indices);
    linerGeom.computeVertexNormals();
    ctx.root.add(
      new THREE.Mesh(
        linerGeom,
        new THREE.MeshStandardMaterial({
          vertexColors: true,
          roughness: 0.28,
          metalness: 0.35,
          side: THREE.DoubleSide,
        }),
      ),
    );

    // Outer structural jacket + TBC ring + Sonic throat indicator plane (x = 0.28 m)
    const throatX = ((0.28 / 0.6) - 0.5) * 150;
    const throatRing = new THREE.Mesh(
      new THREE.TorusGeometry(21 + tWallMm + hThroatMm * 1.5, 1.8, 16, 48),
      new THREE.MeshStandardMaterial({ color: 0xef4444, roughness: 0.3, metalness: 0.6 }),
    );
    throatRing.rotation.y = Math.PI / 2;
    throatRing.position.set(throatX, 0, 0);
    ctx.root.add(throatRing);

    // 16 Counter-flow Supercritical-H2 Regenerative Cooling Channels
    if (showChannels) {
      const nChan = 14;
      const arrowPos: number[] = [];
      const arrowCol: number[] = [];
      for (let c = 0; c < nChan; c++) {
        const ang = thetaStart + ((c + 0.5) / nChan) * thetaSpan;
        for (let i = 0; i < numStations - 1; i++) {
          const x0 = ((i / (numStations - 1)) - 0.5) * 150;
          const x1 = (((i + 1) / (numStations - 1)) - 0.5) * 150;
          const r0 = radiusAtStation(i) + tWallMm * 1.5 + hThroatMm * 1.4;
          const r1 = radiusAtStation(i + 1) + tWallMm * 1.5 + hThroatMm * 1.4;
          const tCool = state[i * 4] ?? 45;
          const [cr, cg, cb] = turboRamp((tCool - 35) / 180);
          arrowPos.push(
            x0,
            r0 * Math.cos(ang),
            r0 * Math.sin(ang),
            x1,
            r1 * Math.cos(ang),
            r1 * Math.sin(ang),
          );
          arrowCol.push(cr / 255, cg / 255, cb / 255, cr / 255, cg / 255, cb / 255);
        }
      }
      const chanGeom = new THREE.BufferGeometry();
      chanGeom.setAttribute('position', new THREE.Float32BufferAttribute(arrowPos, 3));
      chanGeom.setAttribute('color', new THREE.Float32BufferAttribute(arrowCol, 3));
      ctx.root.add(
        new THREE.LineSegments(
          chanGeom,
          new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.92 }),
        ),
      );
    }

    // Gimbal injector flange (-75 mm) and exit manifold ring (+75 mm)
    const flange = new THREE.Mesh(
      new THREE.TorusGeometry(37, 2.5, 14, 48),
      new THREE.MeshStandardMaterial({ color: 0x64748b, roughness: 0.4, metalness: 0.7 }),
    );
    flange.rotation.y = Math.PI / 2;
    flange.position.set(-75, 0, 0);
    ctx.root.add(flange);

    void tTbcUm;
  }, [state, designP, nozzleMode, showChannels, cutaway]);

  const meta = NOZZLE_FIELD_META[nozzleMode];

  return (
    <div class="sys-heatmap-wrap" data-testid="rocket-nozzle-3d-twin">
      <div class="sys-card-bar">
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
          <span class="sys-card-title">3D De Laval Rocket Chamber &amp; Supercritical-H₂ Cooling Twin</span>
          <div class="seg" role="tablist">
            {(Object.keys(NOZZLE_FIELD_META) as NozzleFieldMode[]).map((m) => (
              <button
                key={m}
                class={nozzleMode === m ? 'active' : ''}
                onClick={() => setNozzleMode(m)}
              >
                {m}
              </button>
            ))}
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '10px', fontSize: '11px' }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input type="checkbox" checked={showChannels} onChange={(e) => setShowChannels((e.target as HTMLInputElement).checked)} />
            H₂ Cooling Channels
          </label>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input type="checkbox" checked={cutaway} onChange={(e) => setCutaway((e.target as HTMLInputElement).checked)} />
            Cutaway
          </label>
          <span class="num" style={{ color: 'var(--text-2)' }}>
            {meta.label}: [{minV.toFixed(1)} .. {maxV.toFixed(1)} {meta.unit}]
          </span>
        </div>
      </div>
      <div
        ref={mountRef}
        style={{ width: '100%', height: '275px', borderRadius: '6px', overflow: 'hidden', position: 'relative' }}
      />
    </div>
  );
}

// ---- ParaView-Style 3D Volumetric Solar-Focused Candle Flame, Transient Scrubber, Recap & Real-Time Video Renderer (Pack #3 / V56) ----

/**
 * Record a real-time 30 fps `.webm` video of the 40-frame transient simulation by stepping
 * through `0..frames.length-1`, synchronously rendering the 3D scene via `renderFrameAt(k)`,
 * and compositing a ParaView-style scientific telemetry HUD banner onto an offscreen canvas.
 */
async function renderCandleTransientVideoWebm(
  vis: CandleFlameVisual,
  renderFrameToCanvas: (frameIdx: number) => HTMLCanvasElement | null,
  onProgress: (pct: number, frameIdx: number) => void,
): Promise<string> {
  const frames = vis.frames ?? [];
  if (frames.length === 0) throw new Error('No transient frames to render');

  const width = 560;
  const topBannerH = 52;
  const sceneH = 600;
  const bottomBannerH = 52;
  const height = topBannerH + sceneH + bottomBannerH; // 704 (exact multiple of 16 for VP8/VP9)
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

  recorder.start();

  for (let k = 0; k < frames.length; k++) {
    const f: CandleTransientFrame = frames[k];
    const srcCanvas = renderFrameToCanvas(k);

    ctx.fillStyle = '#090d16';
    ctx.fillRect(0, 0, width, height);
    if (srcCanvas && srcCanvas.width > 0 && srcCanvas.height > 0) {
      try {
        // Preserve strict 1:1 isotropic pixel aspect ratio (sw / sh === width / sceneH)
        // by taking the centered upright crop from srcCanvas so the vertical candle + lens
        // assembly is never stretched or squashed regardless of the browser window shape.
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
        // ignore cross-context draw error if any
      }
    }

    // Top HUD Telemetry Banner
    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, 0, width, topBannerH);
    ctx.fillStyle = '#f8fafc';
    ctx.font = 'bold 12px Inter, system-ui, sans-serif';
    ctx.fillText(
      `FIELDSMITH V56 — SOLAR-FOCUSED CANDLE IGNITION & BUOYANT FLAME`,
      12,
      20,
    );
    ctx.fillStyle = f.ignited ? '#fbbf24' : '#38bdf8';
    ctx.font = 'bold 11px JetBrains Mono, monospace';
    ctx.fillText(
      `t = ${f.time_s.toFixed(2)} s | ${f.phase_name}`,
      12,
      39,
    );

    // Bottom Telemetry Bar + Timeline Progress (2 clean lines)
    const sootNm = f.lens_soot_thickness_nm ?? 0;
    const tauPct = f.lens_transmittance_pct ?? 91.8;
    ctx.fillStyle = 'rgba(9, 13, 22, 0.92)';
    ctx.fillRect(0, height - bottomBannerH, width, bottomBannerH);
    ctx.fillStyle = '#e2e8f0';
    ctx.font = '10.5px JetBrains Mono, monospace';
    ctx.fillText(
      `T_wick: ${f.wick_temp_k.toFixed(0)} K | T_flame: ${f.peak_flame_temp_k.toFixed(0)} K | H_f: ${f.flame_height_mm.toFixed(1)} mm | P_rad: ${f.radiant_power_w.toFixed(1)} W`,
      12,
      height - 34,
    );
    ctx.fillStyle = '#cbd5e1';
    ctx.fillText(
      `Smoke: ${f.smoke_emission_ug_s.toFixed(1)} μg/s | Lens Soot: ${sootNm.toFixed(1)} nm (τ_lens=${tauPct.toFixed(1)}%) | u_z: ${f.peak_updraft_m_s.toFixed(2)} m/s`,
      12,
      height - 18,
    );

    // Progress bar along bottom edge
    const pct = (k + 1) / frames.length;
    ctx.fillStyle = 'rgba(255,255,255,0.14)';
    ctx.fillRect(12, height - 8, width - 24, 4);
    ctx.fillStyle = f.ignited ? '#f97316' : '#38bdf8';
    ctx.fillRect(12, height - 8, (width - 24) * pct, 4);

    onProgress(Math.round(pct * 100), k);
    await new Promise((r) => window.setTimeout(r, 42));
  }

  recorder.stop();
  return stopped;
}

function SolarCandleViewportHud({
  bundle,
  vis,
  bump,
}: {
  bundle: CoupledSolveBundle;
  vis: CandleFlameVisual;
  bump: () => void;
}) {
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState<number>(1);
  const [renderingVideo, setRenderingVideo] = useState(false);
  const [videoPct, setVideoPct] = useState(0);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);

  const ov = candleShared.overlay;
  const frames = vis.frames ?? [];
  const fIdx = Math.max(0, Math.min(frames.length - 1, Math.round(ov.frameIdx)));
  const curFrame = frames[fIdx] ?? frames[frames.length - 1];
  const p = candleShared.designP;

  useEffect(() => {
    if (!playing || frames.length === 0) return;
    const intervalMs = Math.max(30, Math.round(90 / speed));
    const timer = window.setInterval(() => {
      candleShared.overlay.frameIdx = (candleShared.overlay.frameIdx + 1) % frames.length;
      bump();
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [playing, speed, frames.length]);

  const updateCandleParam = (idx: number, val: number) => {
    const nextP = [...candleShared.designP];
    nextP[idx] = val;
    candleShared.designP = nextP;
    try {
      candleShared.bundle = solveCoupledSystem(
        'solar_candle_ignition_v1',
        nextP,
        candleShared.epistemicTheta,
      );
    } catch {
      // ignore
    }
    bump();
  };

  const handleRenderVideo = async () => {
    if (renderingVideo) return;
    setPlaying(false);
    setRenderingVideo(true);
    setVideoPct(0);
    const prevIdx = candleShared.overlay.frameIdx;
    if (liveViewport) {
      liveViewport.camera.up.set(0, 0, 1);
      viewApi.fit(false);
    }
    try {
      const url = await renderCandleTransientVideoWebm(
        vis,
        (k) => {
          candleShared.overlay.frameIdx = k;
          if (liveViewport) {
            syncCoupledViewportOverlay(liveViewport);
            return liveViewport.captureCanvas();
          }
          return null;
        },
        (pct) => setVideoPct(pct),
      );
      setVideoUrl(url);
      ui.notify('Rendered real-time .webm video of Solar Candle Ignition & Buoyant Flame');
    } catch (e) {
      ui.notify(`Video render error: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      candleShared.overlay.frameIdx = prevIdx;
      setRenderingVideo(false);
      bump();
    }
  };

  return (
    <div
      class="sys-vp-hud"
      data-testid="coupled-viewport-hud"
      style={{
        maxWidth: '465px',
        top: 'auto',
        right: 'auto',
        bottom: '44px',
        left: store.lens === 'results' ? '348px' : '10px',
        zIndex: 25,
      }}
    >
      <div class="sys-vp-hud-head">
        <span>3D Volumetric Solar Candle Flame &amp; Plume (144-DOF + 40-Frame Transient)</span>
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
          <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', fontWeight: 500, cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={ov.enabled}
              data-testid="toggle-vp-coupled-overlay"
              onChange={(e) => {
                ov.enabled = (e.target as HTMLInputElement).checked;
                bump();
              }}
            />
            Show in 3D
          </label>
          <button
            type="button"
            class="btn ghost small"
            data-testid="toggle-vp-hud-collapse"
            title={ui.vpHudCollapsed ? 'Expand overlay controls' : 'Minimize overlay controls'}
            onClick={() => ui.set('vpHudCollapsed', !ui.vpHudCollapsed)}
            style={{ padding: '0 5px', height: '20px', fontSize: '11px' }}
          >
            {ui.vpHudCollapsed ? '▴' : '▾'}
          </button>
        </div>
      </div>

      {ov.enabled && curFrame && !ui.vpHudCollapsed && (
        <>
          <div class="seg" style={{ width: '100%', display: 'flex' }}>
            {CANDLE_VOLUME_TABS.map((t) => (
              <button
                key={t.id}
                style={{ flex: 1, fontSize: '10px', padding: '0 3px' }}
                class={ov.volumeMode === t.id ? 'on' : ''}
                data-testid={`vp-candle-mode-${t.id}`}
                onClick={() => {
                  ov.volumeMode = t.id;
                  bump();
                }}
              >
                {t.shortLabel}
              </button>
            ))}
          </div>

          <div class="sys-vp-hud-row" style={{ flexWrap: 'wrap', gap: '6px' }}>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showSolarRays}
                onChange={(e) => {
                  ov.showSolarRays = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Solar Caustic
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showSmokePlume}
                onChange={(e) => {
                  ov.showSmokePlume = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Smoke Plume
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showLensSoot}
                data-testid="toggle-vp-lens-soot"
                onChange={(e) => {
                  ov.showLensSoot = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Lens Soot
            </label>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={ov.showEntrainment}
                onChange={(e) => {
                  ov.showEntrainment = (e.target as HTMLInputElement).checked;
                  bump();
                }}
              />
              Airflow
            </label>
          </div>

          {/* Time-Dependent Transient Scrubber & Real-Time Video Recorder */}
          <div
            style={{
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              borderRadius: '5px',
              padding: '6px 8px',
              display: 'flex',
              flexDirection: 'column',
              gap: '5px',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '6px' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '5px' }}>
                <button
                  type="button"
                  class="mini-btn"
                  data-testid="transient-play-btn"
                  onClick={() => setPlaying(!playing)}
                >
                  {playing ? '⏸ Pause' : '▶ Play'}
                </button>
                <button
                  type="button"
                  class="mini-btn"
                  onClick={() => setSpeed(speed === 1 ? 2 : speed === 2 ? 0.5 : 1)}
                  title="Cycle playback speed (0.5× / 1× / 2×)"
                >
                  {speed}×
                </button>
                <span class="num" style={{ fontSize: '11px', color: curFrame.ignited ? 'var(--warn)' : 'var(--accent)', fontWeight: 600 }}>
                  t = {curFrame.time_s.toFixed(2)} s · {curFrame.phase_name}
                </span>
              </div>
              <button
                type="button"
                class="mini-btn"
                data-testid="render-transient-video"
                disabled={renderingVideo}
                onClick={handleRenderVideo}
                style={{ background: '#ea580c', color: '#fff', borderColor: '#f97316', fontWeight: 600 }}
                title="Render a real-time 30 fps .webm video from the 40-frame transient simulation"
              >
                {renderingVideo ? `Rendering ${videoPct}%…` : '🎬 Render Video'}
              </button>
            </div>

            <input
              type="range"
              min={0}
              max={Math.max(0, frames.length - 1)}
              step={1}
              value={fIdx}
              data-testid="transient-time-slider"
              onInput={(e) => {
                setPlaying(false);
                candleShared.overlay.frameIdx = parseInt((e.target as HTMLInputElement).value, 10) || 0;
                bump();
              }}
            />

            {/* Milestone Recap Quick-Jump Pills */}
            {vis.recap_events && vis.recap_events.length > 0 && (
              <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
                {vis.recap_events.map((ev: SimulationRecapEvent, idx: number) => (
                  <button
                    key={ev.event_id}
                    type="button"
                    class="mini-btn"
                    data-testid={`recap-event-${idx}`}
                    title={`${ev.title} (${ev.time_s.toFixed(2)} s): ${ev.summary}`}
                    onClick={() => {
                      setPlaying(false);
                      candleShared.overlay.frameIdx = ev.frame_index;
                      bump();
                    }}
                    style={
                      Math.abs(fIdx - ev.frame_index) <= 1
                        ? { borderColor: '#f59e0b', color: '#fbbf24', fontWeight: 600, fontSize: '10px' }
                        : { fontSize: '10px' }
                    }
                  >
                    {idx + 1}. {ev.regime_badge} ({ev.time_s.toFixed(1)}s)
                  </button>
                ))}
              </div>
            )}

            {videoUrl && (
              <div style={{ marginTop: '4px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
                <video
                  src={videoUrl}
                  controls
                  autoPlay
                  loop
                  muted
                  data-testid="transient-video-player"
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
                  <span class="num" style={{ color: 'var(--ok)' }}>✓ Real-time rendered video ready (30 fps WebM)</span>
                  <a
                    href={videoUrl}
                    download="solar-candle-ignition-transient.webm"
                    class="mini-btn"
                    data-testid="download-transient-video"
                    style={{ textDecoration: 'none' }}
                  >
                    ⬇ Download .webm
                  </a>
                </div>
              </div>
            )}
          </div>

          <div class="sys-vp-hud-sliders">
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Lens Aperture D_lens</span>
                <strong class="num">{(p[0] ?? 65).toFixed(0)} mm</strong>
              </div>
              <input
                type="range"
                min={35}
                max={110}
                step={1}
                value={p[0] ?? 65}
                onInput={(e) => updateCandleParam(0, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Lens Height z_lens (f={vis.focal_length_mm.toFixed(0)}mm)</span>
                <strong class="num">{(p[2] ?? 68).toFixed(0)} mm</strong>
              </div>
              <input
                type="range"
                min={45}
                max={125}
                step={1}
                value={p[2] ?? 68}
                onInput={(e) => updateCandleParam(2, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Wick Length L_wick</span>
                <strong class="num">{(p[4] ?? 7.5).toFixed(1)} mm</strong>
              </div>
              <input
                type="range"
                min={3.5}
                max={14.0}
                step={0.5}
                value={p[4] ?? 7.5}
                onInput={(e) => updateCandleParam(4, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
            <div class="sys-vp-mini-slider">
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>Ambient O₂ Fraction</span>
                <strong class="num">{((p[5] ?? 0.21) * 100).toFixed(1)} %</strong>
              </div>
              <input
                type="range"
                min={0.15}
                max={0.30}
                step={0.005}
                value={p[5] ?? 0.21}
                onInput={(e) => updateCandleParam(5, parseFloat((e.target as HTMLInputElement).value))}
              />
            </div>
          </div>

          <div class="sys-vp-hud-row num" style={{ borderTop: '1px solid var(--line)', paddingTop: '5px', flexWrap: 'wrap', gap: '6px' }}>
            <span title="Solar Auto-Ignition Delay">
              t_ign: <strong style={{ color: vis.ignition_delay_s <= 3.5 ? 'var(--ok)' : 'var(--danger)' }}>{vis.ignition_delay_s.toFixed(2)} s</strong>
            </span>
            <span title="Instantaneous Wick / Flame Temperature">
              T_max: <strong>{curFrame.peak_flame_temp_k.toFixed(0)} K</strong>
            </span>
            <span title="Escaping Carbonaceous Smoke Emission">
              Smoke: <strong style={{ color: curFrame.smoke_emission_ug_s <= 18.0 ? 'var(--ok)' : 'var(--danger)' }}>{curFrame.smoke_emission_ug_s.toFixed(1)} μg/s</strong>
            </span>
            <span title="Cumulative Thermophoretic Soot Deposit on Lens Underside & Fouled Optical Transmittance">
              Lens Soot: <strong style={{ color: '#fbbf24' }}>{(curFrame.lens_soot_thickness_nm ?? 0).toFixed(1)} nm</strong> (τ={(curFrame.lens_transmittance_pct ?? 91.8).toFixed(1)}%)
            </span>
            <button
              class="btn small"
              title="Rebuild 3D .fsm CSG & Thermo-Mechanical FEA mesh to match current optical lens & wick parameters"
              onClick={() => {
                if (bundle.fsm_3d) {
                  openText(bundle.fsm_3d, 'solar-candle-ignition.fsm');
                  ui.set('resultsCase', 0);
                }
              }}
            >
              Sync Solid .fsm
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function SolarCandle3DTwin({
  vis,
  designP,
}: {
  vis: CandleFlameVisual;
  designP: number[];
}) {
  const [volumeMode, setVolumeMode] = useState<CandleVolumeMode>(candleShared.overlay.volumeMode);
  const [showSolarRays, setShowSolarRays] = useState(true);
  const [showSmokePlume, setShowSmokePlume] = useState(true);
  const [showLensSoot, setShowLensSoot] = useState(true);
  const [showEntrainment, setShowEntrainment] = useState(true);
  const [frameIdx, setFrameIdx] = useState<number>(candleShared.overlay.frameIdx);
  const [playing, setPlaying] = useState<boolean>(false);
  const [speed, setSpeed] = useState<number>(1);
  const [renderingVideo, setRenderingVideo] = useState<boolean>(false);
  const [videoPct, setVideoPct] = useState<number>(0);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);

  const mountRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<{
    scene: THREE.Scene;
    root: THREE.Group;
    camera: THREE.PerspectiveCamera;
    controls: OrbitControls;
    renderer: THREE.WebGLRenderer;
  } | null>(null);

  const frames = vis.frames ?? [];
  const safeIdx = Math.max(0, Math.min(frames.length - 1, Math.round(frameIdx)));
  const curFrame = frames[safeIdx] ?? frames[frames.length - 1];

  useEffect(() => {
    const host = mountRef.current;
    if (!host) return;
    const width = Math.max(host.clientWidth, 420);
    const height = Math.max(host.clientHeight, 340);
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, preserveDrawingBuffer: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    renderer.setClearColor(0x080c14, 1);
    host.innerHTML = '';
    host.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(38, width / height, 1, 2000);
    camera.up.set(0, 0, 1);
    camera.position.set(55, -215, 92);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(0, 0, 66);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;
    controls.update();

    scene.add(new THREE.HemisphereLight(0xe2e8f0, 0x1e293b, 0.9));
    const dirLight = new THREE.DirectionalLight(0xfffbeb, 1.0);
    dirLight.position.set(60, -110, 180);
    scene.add(dirLight);

    const root = new THREE.Group();
    scene.add(root);
    sceneRef.current = { scene, root, camera, controls, renderer };

    let raf = 0;
    const animate = () => {
      raf = requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    };
    animate();

    const ro = new ResizeObserver(() => {
      if (!host) return;
      const w = Math.max(host.clientWidth, 320);
      const h = Math.max(host.clientHeight, 240);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      renderer.setSize(w, h);
    });
    ro.observe(host);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      clearThreeGroup(root);
      renderer.dispose();
      sceneRef.current = null;
    };
  }, []);

  useEffect(() => {
    const ctx = sceneRef.current;
    if (!ctx) return;
    candleShared.overlay.volumeMode = volumeMode;
    candleShared.overlay.showSolarRays = showSolarRays;
    candleShared.overlay.showSmokePlume = showSmokePlume;
    candleShared.overlay.showLensSoot = showLensSoot;
    candleShared.overlay.showEntrainment = showEntrainment;
    candleShared.overlay.frameIdx = safeIdx;

    populateSolarCandle3DGroup(
      ctx.root,
      vis,
      designP,
      {
        enabled: true,
        volumeMode,
        showSolarRays,
        showSmokePlume,
        showLensSoot,
        showEntrainment,
        frameIdx: safeIdx,
      },
      'workbench',
    );
    ctx.renderer.render(ctx.scene, ctx.camera);
  }, [vis, designP, volumeMode, showSolarRays, showSmokePlume, showLensSoot, showEntrainment, safeIdx]);

  useEffect(() => {
    if (!playing || frames.length === 0) return;
    const intervalMs = Math.max(30, Math.round(90 / speed));
    const timer = window.setInterval(() => {
      setFrameIdx((prev) => (prev + 1) % frames.length);
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [playing, speed, frames.length]);

  const handleRenderVideo = async () => {
    const ctx = sceneRef.current;
    const host = mountRef.current;
    if (!ctx || renderingVideo) return;
    setPlaying(false);
    setRenderingVideo(true);
    setVideoPct(0);
    const prevIdx = safeIdx;
    const prevW = Math.max(host?.clientWidth ?? 420, 320);
    const prevH = Math.max(host?.clientHeight ?? 340, 240);
    const recW = 560;
    const recH = 600;
    try {
      ctx.camera.up.set(0, 0, 1);
      ctx.camera.aspect = recW / recH;
      ctx.camera.updateProjectionMatrix();
      ctx.renderer.setSize(recW, recH, false);

      const url = await renderCandleTransientVideoWebm(
        vis,
        (k) => {
          setFrameIdx(k);
          populateSolarCandle3DGroup(
            ctx.root,
            vis,
            designP,
            {
              enabled: true,
              volumeMode,
              showSolarRays,
              showSmokePlume,
              showLensSoot,
              showEntrainment,
              frameIdx: k,
            },
            'workbench',
          );
          ctx.renderer.render(ctx.scene, ctx.camera);
          return ctx.renderer.domElement;
        },
        (pct) => setVideoPct(pct),
      );
      setVideoUrl(url);
      ui.notify('Rendered real-time 30 fps .webm video of Solar Candle Ignition & Buoyant Flame');
    } catch (e) {
      ui.notify(`Video render failed: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      ctx.camera.aspect = prevW / prevH;
      ctx.camera.updateProjectionMatrix();
      ctx.renderer.setSize(prevW, prevH);
      setFrameIdx(prevIdx);
      setRenderingVideo(false);
    }
  };

  return (
    <div class="sys-heatmap-wrap" data-testid="solar-candle-3d-twin">
      <div class="sys-card-bar" style={{ flexWrap: 'wrap', gap: '6px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
          <span class="sys-card-title">ParaView 3D Volumetric Flame, Solar Caustic &amp; Lens Soot Fouling</span>
          <div class="seg" role="tablist">
            {CANDLE_VOLUME_TABS.map((t) => (
              <button
                key={t.id}
                class={volumeMode === t.id ? 'on active' : ''}
                data-testid={`candle-mode-${t.id}`}
                onClick={() => setVolumeMode(t.id)}
                title={`${t.label} (${t.unit})`}
              >
                {t.shortLabel}
              </button>
            ))}
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '10px', fontSize: '11px' }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={showSolarRays}
              data-testid="toggle-solar-rays"
              onChange={(e) => setShowSolarRays((e.target as HTMLInputElement).checked)}
            />
            Solar Caustic Cone (C={vis.optical_concentration.toFixed(0)}×)
          </label>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={showSmokePlume}
              data-testid="toggle-smoke-plume"
              onChange={(e) => setShowSmokePlume((e.target as HTMLInputElement).checked)}
            />
            Smoke Plume
          </label>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={showLensSoot}
              data-testid="toggle-lens-soot"
              onChange={(e) => setShowLensSoot((e.target as HTMLInputElement).checked)}
            />
            Lens Soot Deposit
          </label>
          <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={showEntrainment}
              data-testid="toggle-buoyant-streamlines"
              onChange={(e) => setShowEntrainment((e.target as HTMLInputElement).checked)}
            />
            Buoyant Streamlines
          </label>
        </div>
      </div>

      <div
        ref={mountRef}
        style={{ width: '100%', height: '340px', borderRadius: '6px', overflow: 'hidden', position: 'relative' }}
      />

      {/* Time-Dependent Transient Playback Scrubber, Milestone Recap Cards & Real-Time Video Synthesis */}
      {curFrame && (
        <div
          data-testid="transient-recap-panel"
          style={{
            marginTop: '8px',
            padding: '8px 10px',
            background: 'var(--bg-2)',
            border: '1px solid var(--line)',
            borderRadius: '6px',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: '8px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
              <button
                type="button"
                class="btn small primary"
                data-testid="transient-play-btn"
                onClick={() => setPlaying(!playing)}
              >
                {playing ? '⏸ Pause Transient' : '▶ Play Transient'}
              </button>
              <button
                type="button"
                class="btn small"
                data-testid="transient-speed-btn"
                onClick={() => setSpeed(speed === 1 ? 2 : speed === 2 ? 0.5 : 1)}
                title="Cycle transient playback speed (0.5× / 1× / 2×)"
              >
                Speed: {speed}×
              </button>
              <span class="num" style={{ fontSize: '11.5px', fontWeight: 600, color: curFrame.ignited ? '#fbbf24' : '#38bdf8' }}>
                Frame {safeIdx + 1}/{frames.length} · t = {curFrame.time_s.toFixed(2)} s · {curFrame.phase_name}
              </span>
            </div>

            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <span class="num" style={{ fontSize: '11px', color: 'var(--text-2)' }}>
                T_wick: <strong>{curFrame.wick_temp_k.toFixed(0)} K</strong> · Pool: <strong>{curFrame.wax_pool_radius_mm.toFixed(1)} mm</strong> · H_f: <strong>{curFrame.flame_height_mm.toFixed(1)} mm</strong> · P_rad: <strong>{curFrame.radiant_power_w.toFixed(1)} W</strong>
              </span>
              <button
                type="button"
                class="btn small"
                data-testid="render-transient-video"
                disabled={renderingVideo}
                onClick={handleRenderVideo}
                style={{ background: '#ea580c', color: '#fff', borderColor: '#f97316', fontWeight: 600 }}
                title="Synthesise a real-time 30 fps .webm video from the slow/transient multiphysics simulation"
              >
                {renderingVideo ? `🎬 Rendering Video (${videoPct}%)…` : '🎬 Render Real-Time Video (.webm)'}
              </button>
            </div>
          </div>

          <input
            type="range"
            min={0}
            max={Math.max(0, frames.length - 1)}
            step={1}
            value={safeIdx}
            data-testid="transient-time-slider"
            onInput={(e) => {
              setPlaying(false);
              setFrameIdx(parseInt((e.target as HTMLInputElement).value, 10) || 0);
            }}
          />

          {/* 5 Clickable Simulation Milestone Recap Cards */}
          {vis.recap_events && vis.recap_events.length > 0 && (
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(5, minmax(0, 1fr))',
                gap: '6px',
              }}
            >
              {vis.recap_events.map((ev: SimulationRecapEvent, idx: number) => {
                const isSelected = Math.abs(safeIdx - ev.frame_index) <= 1;
                return (
                  <div
                    key={ev.event_id}
                    data-testid={`recap-event-${idx}`}
                    onClick={() => {
                      setPlaying(false);
                      setFrameIdx(ev.frame_index);
                    }}
                    style={{
                      padding: '6px 7px',
                      borderRadius: '5px',
                      cursor: 'pointer',
                      background: isSelected ? 'rgba(245, 158, 11, 0.16)' : 'var(--bg-3)',
                      border: isSelected ? '1px solid #f59e0b' : '1px solid var(--line)',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '2px',
                    }}
                    title={ev.summary}
                  >
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', fontSize: '10px' }}>
                      <span style={{ fontWeight: 700, color: isSelected ? '#fbbf24' : 'var(--text-2)' }}>
                        {idx + 1}. {ev.regime_badge}
                      </span>
                      <span class="num" style={{ color: 'var(--text-3)' }}>
                        t={ev.time_s.toFixed(2)}s
                      </span>
                    </div>
                    <div style={{ fontSize: '10.5px', fontWeight: 600, color: 'var(--text-1)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                      {ev.title}
                    </div>
                    <div class="num" style={{ fontSize: '10px', color: '#38bdf8' }}>
                      {ev.key_metric_label}: <strong>{ev.key_metric_value}</strong>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* Inline Real-Time Rendered Video Player */}
          {videoUrl && (
            <div
              data-testid="transient-video-container"
              style={{
                marginTop: '4px',
                padding: '8px',
                background: 'rgba(9, 13, 22, 0.9)',
                borderRadius: '6px',
                border: '1px solid #f97316',
                display: 'flex',
                alignItems: 'center',
                gap: '12px',
              }}
            >
              <video
                src={videoUrl}
                controls
                autoPlay
                loop
                muted
                data-testid="transient-video-player"
                style={{
                  width: '240px',
                  height: '302px',
                  aspectRatio: '560 / 704',
                  objectFit: 'contain',
                  flexShrink: 0,
                  borderRadius: '4px',
                  background: '#090d16',
                  border: '1px solid var(--line)',
                }}
              />
              <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', fontSize: '11.5px' }}>
                <strong style={{ color: '#fbbf24' }}>Real-Time Rendered Multiphysics Video (30 fps .webm)</strong>
                <span style={{ color: 'var(--text-2)' }}>
                  Synthesised from all {frames.length} transient keyframes (`t = 0.00 s → {frames[frames.length - 1]?.time_s.toFixed(2)} s`), combining 3D ParaView volumetric flame &amp; smoke rendering with synchronized telemetry HUD.
                </span>
                <div>
                  <a
                    href={videoUrl}
                    download="solar-candle-ignition-transient.webm"
                    class="btn small primary"
                    data-testid="download-transient-video"
                    style={{ textDecoration: 'none', display: 'inline-block' }}
                  >
                    ⬇ Download Real-Time Video (.webm)
                  </a>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ---- Live Shared Workspace Synchronisation (MCP + REST + Browser Co-Pilot) ----

interface WorkspaceSnapshot {
  revision: number;
  updated_at_ms: number;
  actor: string;
  last_tool: string;
  doc_fsm: string | null;
  coupled_system_id: string | null;
  design_p: number[] | null;
  epistemic_theta: number[] | null;
}

let lastSeenWorkspaceRevision = -1;

export function markWorkspaceRevisionSeen(rev: number): void {
  if (typeof rev === 'number' && Number.isFinite(rev)) {
    lastSeenWorkspaceRevision = Math.max(lastSeenWorkspaceRevision, rev);
  }
}

async function pushWorkspaceState(payload: {
  actor?: string;
  last_tool?: string;
  summary?: string;
  doc_fsm?: string;
  coupled_system_id?: string;
  design_p?: number[];
  epistemic_theta?: number[];
}): Promise<void> {
  try {
    const resp = await fetch(apiUrl('/api/workspace'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ actor: 'web-ui', ...payload }),
    });
    if (resp.ok) {
      const ws = (await resp.json()) as WorkspaceSnapshot;
      if (typeof ws.revision === 'number') {
        lastSeenWorkspaceRevision = Math.max(lastSeenWorkspaceRevision, ws.revision);
      }
    }
  } catch {
    // Non-fatal when running as a static bundle without the dev/MCP server
  }
}

// ---- Main 3-Column Coupled Systems Workbench Modal ----

export function matchedCoupledSystemId(): string | null {
  const sheetId = ui.tutorialSheetId;
  const docName = (store.doc.name || '').toLowerCase();
  const hasLet = (k: string) => store.doc.lets?.some((l) => l.name === k) ?? false;
  if (sheetId === 'T13' || isSolarCandleDocument() || hasLet('i_solar')) {
    return 'solar_candle_ignition_v1';
  }
  if (sheetId === 'T8' || isFloatBathDocument() || hasLet('h_sill')) {
    return 'pilkington_float_bath_cvd_coater_v1';
  }
  if (sheetId === 'T17' || docName.includes('rocket') || docName.includes('regen') || hasLet('throat_ch_mm')) {
    return 'regen_nozzle_h2_v1';
  }
  if (sheetId === 'T14' || sheetId === 'A13' || docName.includes('fixing') || docName.includes('fastener') || hasLet('torque_nm')) {
    return 'jwst_fastener_fixing_envelope_v1';
  }
  if (sheetId === 'T16' || sheetId === 'A14' || docName.includes('deadcat') || docName.includes('antenna') || hasLet('cfrp_standoff_mm')) {
    return 'deadcat_4in_gemini_antenna_v1';
  }
  if (sheetId === 'T9' || docName.includes('induction')) {
    return 'vacuum_induction_melt_v1';
  }
  if (sheetId === 'T10' || docName.includes('czochralski') || docName.includes('crystal')) {
    return 'czochralski_si_crystal_v1';
  }
  if (sheetId === 'T11' || docName.includes('caster') || docName.includes('slab')) {
    return 'continuous_steel_slab_caster_v1';
  }
  if (sheetId === 'T12' || docName.includes('pem') || docName.includes('electroly')) {
    return 'pem_water_electrolyser_v1';
  }
  if (isSwirlCombustorDocument() || docName.includes('swirl') || docName.includes('combustor')) {
    return 'annular_swirl_spray_combustor_v1';
  }
  return null;
}

/** Extract `design_p` from the active `.fsm` document's `let` variables when present (`Document → CoupledSystem`). */
function extractCoupledDesignPFromDoc(systemId: string, fallbackP: number[]): number[] {
  const getLet = (k: string): number | undefined => store.doc.lets?.find((l) => l.name === k)?.value;
  const p = [...fallbackP];
  if (systemId === 'pilkington_float_bath_cvd_coater_v1' && p.length >= 6) {
    if (getLet('h_sill') !== undefined) p[0] = getLet('h_sill')!;
    if (getLet('q_beam') !== undefined) p[2] = getLet('q_beam')!;
    if (getLet('p_h2') !== undefined) p[3] = getLet('p_h2')!;
    if (getLet('i_motor') !== undefined) p[4] = getLet('i_motor')!;
    if (getLet('roll_cant') !== undefined) p[5] = getLet('roll_cant')!;
  } else if (systemId === 'regen_nozzle_h2_v1' && p.length >= 4) {
    if (getLet('throat_ch_mm') !== undefined) p[0] = getLet('throat_ch_mm')!;
    if (getLet('liner_wall_mm') !== undefined) p[1] = getLet('liner_wall_mm')!;
    if (getLet('tbc_um') !== undefined) p[2] = getLet('tbc_um')!;
    if (getLet('coolant_mdot_kg_s') !== undefined) p[3] = getLet('coolant_mdot_kg_s')!;
  } else if (systemId === 'solar_candle_ignition_v1' && p.length >= 5) {
    if (getLet('d_lens') !== undefined) p[0] = getLet('d_lens')!;
    if (getLet('i_solar') !== undefined) p[3] = getLet('i_solar')!;
    if (getLet('wick_len') !== undefined) p[4] = getLet('wick_len')!;
  } else if (systemId === 'jwst_fastener_fixing_envelope_v1' && p.length >= 5) {
    if (getLet('torque_nm') !== undefined) p[0] = getLet('torque_nm')!;
    if (getLet('screw_d') !== undefined) p[1] = getLet('screw_d')!;
    if (getLet('screw_pitch') !== undefined) p[2] = getLet('screw_pitch')!;
    if (getLet('eng_len') !== undefined) p[3] = getLet('eng_len')!;
    if (getLet('boss_r') !== undefined) p[4] = 2.0 * getLet('boss_r')!;
  } else if (systemId === 'deadcat_4in_gemini_antenna_v1' && p.length >= 8) {
    if (getLet('cfrp_standoff_mm') !== undefined) p[0] = getLet('cfrp_standoff_mm')!;
    if (getLet('gemini_l900_mm') !== undefined) p[1] = getLet('gemini_l900_mm')!;
    if (getLet('gemini_l2400_mm') !== undefined) p[2] = getLet('gemini_l2400_mm')!;
    if (getLet('gemini_skew_deg') !== undefined) p[3] = getLet('gemini_skew_deg')!;
    if (getLet('antenna_elev_mm') !== undefined) p[4] = getLet('antenna_elev_mm')!;
    if (getLet('tpu_pod_wall_mm') !== undefined) p[5] = getLet('tpu_pod_wall_mm')!;
    if (getLet('match_series_nh') !== undefined) p[6] = getLet('match_series_nh')!;
    if (getLet('match_shunt_pf') !== undefined) p[7] = getLet('match_shunt_pf')!;
  }
  return p;
}

export function SystemsWorkbench() {
  useApp();
  const [systems, setSystems] = useState<CoupledSystemSummary[]>([]);
  const [activeId, setActiveId] = useState<string>(() => matchedCoupledSystemId() ?? '');
  const [designP, setDesignP] = useState<number[]>([]);
  const [epistemicTheta, setEpistemicTheta] = useState<number[]>([]);
  const [bundle, setBundle] = useState<CoupledSolveBundle | null>(null);
  const [bathField, setBathField] = useState<BathFieldMode>('glass_temp');
  const [wsRev, setWsRev] = useState<number>(lastSeenWorkspaceRevision);
  const [wsActor, setWsActor] = useState<string>('ready');
  const [error, setError] = useState<string | null>(null);

  // Background poll of `/api/workspace` so MCP / REST edits immediately sync into the live Web UI
  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const resp = await fetch(apiUrl('/api/workspace'));
        if (!resp.ok || !active) return;
        const ws = (await resp.json()) as WorkspaceSnapshot;
        if (lastSeenWorkspaceRevision < 0) {
          lastSeenWorkspaceRevision = typeof ws.revision === 'number' ? ws.revision : 0;
          return;
        }
        if (typeof ws.revision === 'number' && ws.revision > lastSeenWorkspaceRevision) {
          lastSeenWorkspaceRevision = ws.revision;
          setWsRev(ws.revision);
          setWsActor(`${ws.actor}:${ws.last_tool}`);
          if (ws.actor === 'mcp-agent') {
            if (ws.coupled_system_id && Array.isArray(ws.design_p)) {
              setActiveId(ws.coupled_system_id);
              setDesignP(ws.design_p);
              const th = Array.isArray(ws.epistemic_theta) ? ws.epistemic_theta : epistemicTheta;
              setEpistemicTheta(th);
              try {
                const res = solveCoupledSystem(ws.coupled_system_id, ws.design_p, th);
                setBundle(res);
                if (ws.coupled_system_id === 'pilkington_float_bath_cvd_coater_v1') {
                  coupledShared.designP = ws.design_p;
                  coupledShared.epistemicTheta = th;
                  coupledShared.bundle = res;
                  if (liveViewport) syncCoupledViewportOverlay(liveViewport);
                } else if (ws.coupled_system_id === 'solar_candle_ignition_v1') {
                  candleShared.designP = ws.design_p;
                  candleShared.epistemicTheta = th;
                  candleShared.bundle = res;
                  if (liveViewport) syncCoupledViewportOverlay(liveViewport);
                }
              } catch {
                // ignore
              }
            }
            if (typeof ws.doc_fsm === 'string' && ws.doc_fsm.trim().length > 0) {
              openText(ws.doc_fsm, 'mcp-shared-workspace.fsm');
              viewApi.fit(false);
              ui.notify(`Synced MCP Co-Pilot update (rev #${ws.revision}: ${ws.last_tool})`);
            }
          }
        }
      } catch {
        // ignore offline
      }
    };
    const timer = window.setInterval(poll, 1000);
    poll();
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (!ui.systemsOpen) return;
    try {
      const cat = listCoupledSystems();
      setSystems(cat);
      const targetId = matchedCoupledSystemId();
      if (!targetId) {
        setActiveId('');
        setBundle(null);
        setError(null);
        return;
      }
      const sys = cat.find((s) => s.system_id === targetId);
      if (sys) {
        setActiveId(sys.system_id);
        const baseP =
          sys.system_id === 'solar_candle_ignition_v1' && candleShared.designP.length === sys.design_parameters.length
            ? [...candleShared.designP]
            : sys.design_parameters.map((p) => p.nominal);
        const p0 = extractCoupledDesignPFromDoc(sys.system_id, baseP);
        const th0 =
          sys.system_id === 'solar_candle_ignition_v1' && candleShared.epistemicTheta.length === sys.epistemic_parameters.length
            ? [...candleShared.epistemicTheta]
            : sys.epistemic_parameters.map((t) => t.nominal);
        setDesignP(p0);
        setEpistemicTheta(th0);
        const res = solveCoupledSystem(sys.system_id, p0, th0);
        setBundle(res);
        if (sys.system_id === 'pilkington_float_bath_cvd_coater_v1') {
          coupledShared.summary = sys;
          coupledShared.designP = p0;
          coupledShared.epistemicTheta = th0;
          coupledShared.bundle = res;
        } else if (sys.system_id === 'solar_candle_ignition_v1') {
          candleShared.summary = sys;
          candleShared.designP = p0;
          candleShared.epistemicTheta = th0;
          candleShared.bundle = res;
        }
        setError(null);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [ui.systemsOpen]);

  if (!ui.systemsOpen) return null;

  const activeSummary = activeId ? (systems.find((s) => s.system_id === activeId) ?? null) : null;

  const runSolve = (sysId: string, nextP: number[], nextTheta: number[]) => {
    try {
      const res = solveCoupledSystem(sysId, nextP, nextTheta);
      setBundle(res);
      if (sysId === 'pilkington_float_bath_cvd_coater_v1') {
        coupledShared.designP = nextP;
        coupledShared.epistemicTheta = nextTheta;
        coupledShared.bundle = res;
        if (liveViewport) syncCoupledViewportOverlay(liveViewport);
      } else if (sysId === 'solar_candle_ignition_v1') {
        candleShared.designP = nextP;
        candleShared.epistemicTheta = nextTheta;
        candleShared.bundle = res;
        if (liveViewport) syncCoupledViewportOverlay(liveViewport);
      }
      void pushWorkspaceState({
        last_tool: 'solveCoupledSystem',
        summary: `Solved ${sysId} in Web Workbench`,
        coupled_system_id: sysId,
        design_p: nextP,
        epistemic_theta: nextTheta,
        ...(res.fsm_3d ? { doc_fsm: res.fsm_3d } : {}),
      });
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const switchSystem = (sysId: string) => {
    if (!sysId) {
      setActiveId('');
      setBundle(null);
      setError(null);
      return;
    }
    const sys = systems.find((s) => s.system_id === sysId);
    if (!sys) return;
    setActiveId(sysId);
    const p0 = extractCoupledDesignPFromDoc(sysId, sys.design_parameters.map((p) => p.nominal));
    const th0 = sys.epistemic_parameters.map((t) => t.nominal);
    setDesignP(p0);
    setEpistemicTheta(th0);
    runSolve(sysId, p0, th0);
  };

  const applyPreset = (pValues: number[]) => {
    setDesignP(pValues);
    runSolve(activeId, pValues, epistemicTheta);
  };

  const stepAdjointOptimiser = () => {
    if (!bundle || !activeSummary) return;
    const { analysis } = bundle;
    const { design_parameters, objectives } = activeSummary;
    const nextP = [...designP];
    for (let k = 0; k < design_parameters.length; k++) {
      const dp = design_parameters[k];
      const span = dp.max_bound - dp.min_bound;
      let gradNorm = 0;
      for (let m = 0; m < objectives.length; m++) {
        const obj = objectives[m];
        const val = analysis.objectives[m];
        const scale = Math.max(Math.abs(val), 1.0);
        const g = (analysis.design_sensitivities[m]?.[k] ?? 0) * (span / scale);
        if (obj.kind === 'Minimize') {
          gradNorm += g;
        } else if (obj.kind === 'Maximize') {
          gradNorm -= g;
        } else if (typeof obj.kind === 'object' && 'UpperLimit' in obj.kind) {
          if (val > obj.kind.UpperLimit) gradNorm += 2.5 * g;
          else gradNorm += 0.35 * g;
        } else if (typeof obj.kind === 'object' && 'LowerLimit' in obj.kind) {
          if (val < obj.kind.LowerLimit) gradNorm -= 2.5 * g;
        } else if (typeof obj.kind === 'object' && 'TargetBand' in obj.kind) {
          const { min, max } = obj.kind.TargetBand;
          const mid = 0.5 * (min + max);
          if (val < min) gradNorm -= 2.5 * g;
          else if (val > max) gradNorm += 2.5 * g;
          else gradNorm += 0.25 * Math.sign(val - mid) * g;
        }
      }
      if (Math.abs(gradNorm) > 1e-9) {
        const step = -0.08 * span * Math.max(-1, Math.min(1, gradNorm));
        nextP[k] = Math.max(dp.min_bound, Math.min(dp.max_bound, nextP[k] + step));
      }
    }
    setDesignP(nextP);
    runSolve(activeId, nextP, epistemicTheta);
  };

  const syncTo3DViewport = (jumpToResults = false) => {
    if (!bundle) return;
    const fsm = bundle.fsm_3d ?? buildFloatBathFsm(designP);
    const filename =
      activeId === 'regen_nozzle_h2_v1'
        ? 'T17-regenerative-rocket-nozzle.fsm'
        : activeId === 'solar_candle_ignition_v1'
          ? 'T13-solar-parabolic-candle-igniter.fsm'
          : activeId === 'jwst_fastener_fixing_envelope_v1'
            ? 'T14-fastener-torque-weibull-preload.fsm'
            : activeId === 'deadcat_4in_gemini_antenna_v1'
              ? 'T16-deadcat-fpv-drone-gemini-antenna.fsm'
              : 'T8-float-bath-cvd-coater.fsm';
    if (activeId === 'pilkington_float_bath_cvd_coater_v1' && bundle.float_bath_visual) {
      coupledShared.designP = [...designP];
      coupledShared.epistemicTheta = [...epistemicTheta];
      coupledShared.bundle = bundle;
      coupledShared.overlay.enabled = true;
      coupledShared.overlay.mode = bathField;
    } else if (activeId === 'solar_candle_ignition_v1' && bundle.candle_flame_visual) {
      candleShared.designP = [...designP];
      candleShared.epistemicTheta = [...epistemicTheta];
      candleShared.bundle = bundle;
      candleShared.overlay.enabled = true;
    } else {
      coupledShared.overlay.enabled = false;
    }
    if (openText(fsm, filename)) {
      ui.set('resultsCase', 0);
      ui.set('systemsOpen', false);
      if (jumpToResults) {
        store.setLens('results');
      }
      viewApi.fit(false);
      if (liveViewport) syncCoupledViewportOverlay(liveViewport);
      void pushWorkspaceState({
        last_tool: 'syncTo3DViewport',
        summary: `Loaded 3D CAD Twin (${filename}) into Viewport`,
        doc_fsm: fsm,
        coupled_system_id: activeId,
        design_p: designP,
        epistemic_theta: epistemicTheta,
      });
      ui.notify(`Loaded 3D Parametric Twin (${filename}) into Main 3D Viewport`);
    }
  };

  const track = bundle?.analysis.causal_tracks[0] ?? null;

  const routeGroups: { title: string; subtitle: string; ids: string[] }[] = [
    {
      title: '1. Propulsion & High-Heat-Flux Thermal-Fluids',
      subtitle: '1D compressible/reacting channel flow coupled to 3D wall conduction & coolant convection',
      ids: ['regen_nozzle_h2_v1', 'annular_swirl_spray_combustor_v1'],
    },
    {
      title: '2. Mechanical Joints & Structural Preload',
      subtitle: 'VDI 2230 torque-preload, thread shear & cryogenic Weibull reliability envelopes',
      ids: ['jwst_fastener_fixing_envelope_v1'],
    },
    {
      title: '3. Electromagnetics, Induction & RF',
      subtitle: 'Harmonic Maxwell / eddy-current heating and near-field antenna impedance coupling',
      ids: ['deadcat_4in_gemini_antenna_v1', 'vacuum_induction_melt_v1'],
    },
    {
      title: '4. Process Transport, Electrochemistry & Radiation',
      subtitle: 'Multi-field industrial transport, solidification, electrolysis & concentrated radiative flux',
      ids: [
        'pilkington_float_bath_cvd_coater_v1',
        'czochralski_si_crystal_v1',
        'continuous_steel_slab_caster_v1',
        'pem_water_electrolyser_v1',
        'solar_candle_ignition_v1',
      ],
    },
  ];

  return (
    <div class="sys-scrim" data-testid="systems-workbench">
      <div class="sys-modal">
        {/* Top bar */}
        <div class="sys-header">
          <div class="sys-header-left">
            <span class="sys-badge">MONOLITHIC PDE + ADJOINT + UQ</span>
            {activeId && (
              <button
                class="btn small ghost"
                data-testid="systems-back-to-router"
                title="Return to 5S Coupled-Physics Abstraction Directory"
                onClick={() => switchSystem('')}
              >
                ← 5S Route
              </button>
            )}
            <select
              key={systems.length}
              class="select sys-select"
              value={activeId}
              data-testid="systems-domain-select"
              onChange={(e) => switchSystem((e.target as HTMLSelectElement).value)}
            >
              <option value="">— Select Coupled PDE Abstraction (5S Route) —</option>
              {systems.map((s) => (
                <option key={s.system_id} value={s.system_id}>
                  [{s.industry_sector}] {s.system_title} ({s.num_dofs} DOFs)
                </option>
              ))}
            </select>
          </div>
          {activeSummary && activeSummary.scenarios.length > 0 && (
            <div class="sys-presets">
              {activeSummary.scenarios.map((pr, idx) => (
                <button
                  key={pr.name}
                  class="btn small"
                  title={pr.description}
                  data-testid={`preset-${idx}`}
                  onClick={() => applyPreset(pr.design_p)}
                >
                  {pr.name}
                </button>
              ))}
            </div>
          )}
          <div class="sys-header-right">
            <span
              class="num"
              data-testid="shared-workspace-badge"
              style={{
                fontSize: '10px',
                padding: '2px 7px',
                borderRadius: '4px',
                background: 'var(--bg-3)',
                border: '1px solid var(--line)',
                color: 'var(--text-2)',
              }}
              title="Shared MCP / REST / Browser Workspace State (`/api/workspace` & `/api/mcp`)"
            >
              MCP Workspace rev #{wsRev} ({wsActor})
            </span>
            {bundle && (
              <>
                <button
                  class="btn small primary"
                  data-testid="sync-3d-bath"
                  onClick={() => syncTo3DViewport(false)}
                  title="Generate a 3D parametric .fsm CAD twin + thermo-mechanical FEA setup in the main 3D viewport"
                >
                  {activeId === 'regen_nozzle_h2_v1'
                    ? 'Load 3D Nozzle into Viewport'
                    : activeId === 'solar_candle_ignition_v1'
                      ? 'Load 3D Solar Candle & Flame into Viewport'
                      : 'Load 3D Bath into Viewport'}
                </button>
                <button
                  class="btn small"
                  data-testid="sync-3d-and-solve"
                  onClick={() => syncTo3DViewport(true)}
                  style={{ borderColor: 'rgba(16, 185, 129, 0.45)', color: '#6ee7b7' }}
                  title="Project solved 1D/2D boundary profiles onto the 3D .fsm twin and jump directly to 3. Results"
                >
                  🚀 Sync 3D & Solve Results →
                </button>
              </>
            )}
            <button class="btn small" data-testid="close-systems-workbench" onClick={() => ui.set('systemsOpen', false)}>
              Close (Esc)
            </button>
          </div>
        </div>

        {/* Status ribbon */}
        {bundle && (
          <div class="sys-status-bar num" data-testid="systems-solve-status">
            <span class="ok-pill">CONVERGED</span>
            <span>DOFs: {bundle.analysis.num_dofs} (nnz: {bundle.analysis.jacobian_nnz}, cross-physics: {bundle.analysis.cross_physics_nnz})</span>
            <span>Newton steps: {bundle.analysis.steady_newton_iters}</span>
            <span>||R(W*)||₂ = {bundle.analysis.final_residual_l2.toExponential(2)}</span>
            <span>Adjoint vs Tangent max err: {bundle.analysis.adjoint_vs_tangent_max_rel_err.toExponential(2)}</span>
            <span>Constraints: {bundle.constraints_pass_robust_p90 ? 'PASS (Robust p90)' : bundle.constraints_pass_p50 ? 'PASS (Nominal p50)' : 'VIOLATED'}</span>
            <span>Solve + Adjoint + UQ: {bundle.wall_ms.toFixed(1)} ms</span>
            {bundle.fsm_3d && (() => {
              const qThroat = bundle.fsm_3d.match(/heatflux\s+throat_q\s+\S+\s+q=([0-9.]+)/)?.[1];
              const qChamber = bundle.fsm_3d.match(/heatflux\s+chamber_q\s+\S+\s+q=([0-9.]+)/)?.[1];
              const hCool = bundle.fsm_3d.match(/convection\s+ethanol_cool\s+\S+\s+h=([0-9.]+)\s+tinf=([0-9.]+)/);
              if (!qThroat && !hCool) return null;
              return (
                <span
                  data-testid="systems-1d-3d-projection-summary"
                  style={{ color: '#6ee7b7', fontWeight: 600 }}
                  title="Live 1D channel state projected onto 3D .fsm surface regions"
                >
                  1D→3D Projection: q_throat={qThroat ?? '—'} · q_chamber={qChamber ?? '—'} MW/m² · h_cool={hCool?.[1] ?? '—'} W/(mm²·K) @ T_∞={hCool?.[2] ?? '—'} °C
                </span>
              );
            })()}
          </div>
        )}

        {error && <div class="error-box">{error}</div>}

        {/* 5S Coupled-Physics Abstraction Router when no coupled system is bound to the active document */}
        {(!bundle || !activeSummary) && (
          <div
            data-testid="systems-5s-router"
            style={{
              padding: '20px 24px',
              overflowY: 'auto',
              display: 'flex',
              flexDirection: 'column',
              gap: '16px',
            }}
          >
            <div
              style={{
                padding: '12px 16px',
                borderRadius: '6px',
                background: 'rgba(59, 130, 246, 0.08)',
                border: '1px solid rgba(59, 130, 246, 0.25)',
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: '12px',
                flexWrap: 'wrap',
              }}
            >
              <div>
                <div style={{ fontWeight: 700, fontSize: '13px', color: 'var(--text-1)', marginBottom: '3px' }}>
                  5S Coupled-Physics Abstraction Directory
                </div>
                <div style={{ fontSize: '11.5px', color: 'var(--text-2)' }}>
                  Current document (<code>{store.doc.name || 'Untitled'}</code>) is not bound to a coupled 1D/2D PDE system. Select the specific physical abstraction you want to open below, or stay in standard 3D CAD / P2 FEM.
                </div>
              </div>
              <button class="btn small" onClick={() => ui.set('systemsOpen', false)}>
                Stay in 3D CAD / FEM
              </button>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
                gap: '14px',
              }}
            >
              {routeGroups.map((grp) => {
                const items = grp.ids
                  .map((id) => systems.find((s) => s.system_id === id))
                  .filter((s): s is CoupledSystemSummary => !!s);
                return (
                  <div
                    key={grp.title}
                    style={{
                      padding: '12px 14px',
                      borderRadius: '6px',
                      background: 'var(--bg-2)',
                      border: '1px solid var(--line)',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: '8px',
                    }}
                  >
                    <div>
                      <div style={{ fontWeight: 700, fontSize: '12px', color: 'var(--text-1)' }}>{grp.title}</div>
                      <div style={{ fontSize: '10.5px', color: 'var(--text-3)', marginTop: '2px' }}>{grp.subtitle}</div>
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
                      {items.map((s) => (
                        <button
                          key={s.system_id}
                          class="btn"
                          data-testid={`route-system-${s.system_id}`}
                          onClick={() => switchSystem(s.system_id)}
                          style={{
                            textAlign: 'left',
                            padding: '8px 10px',
                            display: 'flex',
                            flexDirection: 'column',
                            alignItems: 'flex-start',
                            gap: '3px',
                            height: 'auto',
                          }}
                        >
                          <div style={{ display: 'flex', justifyContent: 'space-between', width: '100%', gap: '8px' }}>
                            <span style={{ fontWeight: 600, fontSize: '11.5px', color: 'var(--text-1)' }}>{s.system_title}</span>
                            <span class="num" style={{ fontSize: '10px', color: 'var(--text-3)' }}>
                              {s.num_dofs} DOFs
                            </span>
                          </div>
                          <div style={{ fontSize: '10px', color: 'var(--text-2)' }}>
                            [{s.industry_sector}] {s.design_parameters.length} design levers · {s.objectives.length} objectives
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* 3-Column Engineering Workbench */}
        {bundle && activeSummary && (
          <div class="sys-grid">
            {/* LEFT COLUMN: Design Parameters p_k & Epistemic Uncertainties theta_l */}
            <div class="sys-col sys-col-left">
              <div class="sys-section-head">
                <span>Design Levers (p_k)</span>
                <button
                  class="btn small primary"
                  data-testid="adjoint-step-btn"
                  onClick={stepAdjointOptimiser}
                  title="Take an automatic design step along the sparse Adjoint sensitivity −dΦ/dp"
                >
                  Adjoint Step (−dΦ/dp)
                </button>
              </div>
              <div class="sys-param-list">
                {activeSummary.design_parameters.map((dp, k) => {
                  const val = designP[k] ?? dp.nominal;
                  const step = (dp.max_bound - dp.min_bound) / 200;
                  return (
                    <div key={dp.name} class="sys-param-card" title={dp.description}>
                      <div class="sys-param-top">
                        <span class="sys-param-label">
                          {dp.symbol} — {dp.name}
                        </span>
                        <span class="sys-param-val num">
                          {val.toFixed(dp.max_bound - dp.min_bound < 5 ? 2 : 1)} {dp.unit}
                        </span>
                      </div>
                      <input
                        type="range"
                        min={dp.min_bound}
                        max={dp.max_bound}
                        step={step}
                        value={val}
                        data-testid={`design-slider-${dp.name}`}
                        onInput={(e) => {
                          const v = parseFloat((e.target as HTMLInputElement).value);
                          const next = [...designP];
                          next[k] = v;
                          setDesignP(next);
                          runSolve(activeId, next, epistemicTheta);
                        }}
                      />
                      <div class="sys-param-meta">
                        <span class="sys-cat">{dp.category}</span>
                        <span class="num">
                          [{dp.min_bound} .. {dp.max_bound} {dp.unit}]
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>

              <div class="sys-section-head" style={{ marginTop: '10px' }}>
                <span>Epistemic Uncertainties (θ_l ± 1σ)</span>
              </div>
              <div class="sys-param-list">
                {activeSummary.epistemic_parameters.map((ep, l) => {
                  const val = epistemicTheta[l] ?? ep.nominal;
                  const step = (ep.max_bound - ep.min_bound) / 200;
                  return (
                    <div key={ep.name} class="sys-param-card epistemic" title={ep.physical_mechanism}>
                      <div class="sys-param-top">
                        <span class="sys-param-label">
                          {ep.symbol} — {ep.name}
                        </span>
                        <span class="sys-param-val num">
                          {val.toFixed(ep.max_bound - ep.min_bound < 5 ? 2 : 1)} {ep.unit} (±{ep.std_dev_sigma})
                        </span>
                      </div>
                      <input
                        type="range"
                        min={ep.min_bound}
                        max={ep.max_bound}
                        step={step}
                        value={val}
                        data-testid={`epistemic-slider-${ep.name}`}
                        onInput={(e) => {
                          const v = parseFloat((e.target as HTMLInputElement).value);
                          const next = [...epistemicTheta];
                          next[l] = v;
                          setEpistemicTheta(next);
                          runSolve(activeId, designP, next);
                        }}
                      />
                      <div class="sys-param-meta">
                        <span>Prior: {ep.nominal} ± {ep.std_dev_sigma} {ep.unit}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* CENTRE COLUMN: Interactive 3D Multi-Physics Twin / 2D Plan View + 3-Strip Longitudinal Causal Attribution */}
            <div class="sys-col sys-col-center">
              {bundle.float_bath_visual && (
                <div class="sys-heatmap-wrap">
                  <FloatBathCanvas
                    vis={bundle.float_bath_visual}
                    mode={bathField}
                    onChangeMode={setBathField}
                    designP={designP}
                  />
                </div>
              )}

              {activeId === 'regen_nozzle_h2_v1' && (
                <RocketNozzle3DTwin state={bundle.analysis.state} designP={designP} />
              )}

              {activeId === 'solar_candle_ignition_v1' && bundle.candle_flame_visual && (
                <SolarCandle3DTwin vis={bundle.candle_flame_visual} designP={designP} />
              )}

              {track && (
                <div class="sys-attribution-card">
                  <div class="sys-card-bar">
                    <span class="sys-card-title">{track.track_name}</span>
                    <span class="num" style={{ color: 'var(--text-3)' }}>
                      Birth-to-Exit Causal Attribution ({track.coord_unit})
                    </span>
                  </div>
                  <CausalAttributionSvg track={track} vis={bundle.float_bath_visual} />
                </div>
              )}

              {/* Global Conservation Audits */}
              <div class="sys-audit-card">
                <div class="sys-card-bar">
                  <span class="sys-card-title">Global Conservation Balance Audits</span>
                </div>
                <table class="sys-table num">
                  <thead>
                    <tr>
                      <th>Balance Law</th>
                      <th>Inflow</th>
                      <th>Outflow</th>
                      <th>Source</th>
                      <th>Imbalance</th>
                      <th>Rel. Error</th>
                    </tr>
                  </thead>
                  <tbody>
                    {bundle.analysis.conservation_audits.map((a) => (
                      <tr key={a.name}>
                        <td style={{ fontFamily: 'var(--sans)', fontWeight: 500 }}>
                          {a.name} ({a.unit})
                        </td>
                        <td>{a.inflow.toFixed(3)}</td>
                        <td>{a.outflow.toFixed(3)}</td>
                        <td>{a.source_or_sink.toFixed(3)}</td>
                        <td>{a.net_imbalance.toFixed(4)}</td>
                        <td>{(a.relative_error * 100).toFixed(3)}%</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            {/* RIGHT COLUMN: Objectives with Epistemic p10/p50/p90 & Sparse Adjoint Sensitivity Matrix */}
            <div class="sys-col sys-col-right">
              <div class="sys-section-head">
                <span>Objectives &amp; Epistemic Confidence (p10 / p50 / p90)</span>
              </div>
              <div class="sys-obj-list">
                {activeSummary.objectives.map((obj, m) => {
                  const uq = bundle.analysis.objective_uncertainties[m];
                  const val = bundle.analysis.objectives[m];
                  let statusLabel = 'MINIMIZE';
                  let statusClass = 'neutral';
                  if (obj.kind === 'Maximize') {
                    statusLabel = 'MAXIMIZE';
                  } else if (typeof obj.kind === 'object' && 'UpperLimit' in obj.kind) {
                    const lim = obj.kind.UpperLimit;
                    const inWin = val <= lim;
                    const p90InWin = uq.p90 <= lim;
                    statusLabel = inWin ? (p90InWin ? `PASS ≤ ${lim}` : `MARGINAL p90 ≤ ${lim}`) : `EXCEEDS ${lim}`;
                    statusClass = inWin ? (p90InWin ? 'ok' : 'warn') : 'bad';
                  } else if (typeof obj.kind === 'object' && 'LowerLimit' in obj.kind) {
                    const lim = obj.kind.LowerLimit;
                    const inWin = val >= lim;
                    statusLabel = inWin ? `PASS ≥ ${lim}` : `BELOW ${lim}`;
                    statusClass = inWin ? 'ok' : 'bad';
                  } else if (typeof obj.kind === 'object' && 'TargetBand' in obj.kind) {
                    const { min, max } = obj.kind.TargetBand;
                    const inWin = val >= min && val <= max;
                    const p90InWin = uq.p10 >= min && uq.p90 <= max;
                    statusLabel = inWin ? (p90InWin ? `IN SPEC [${min}..${max}]` : `MARGINAL [${min}..${max}]`) : `OUT OF SPEC [${min}..${max}]`;
                    statusClass = inWin ? (p90InWin ? 'ok' : 'warn') : 'bad';
                  }

                  let domIdx = 0;
                  let domShare = 0;
                  uq.variance_shares.forEach((sh, idx) => {
                    if (sh > domShare) {
                      domShare = sh;
                      domIdx = idx;
                    }
                  });
                  const domEp = activeSummary.epistemic_parameters[domIdx]?.symbol ?? '—';

                  return (
                    <div key={obj.name} class="sys-obj-card" data-testid={`objective-card-${obj.name}`} title={obj.description}>
                      <div class="sys-obj-top">
                        <span class="sys-obj-title">
                          {obj.symbol} — {obj.name}
                        </span>
                        <span class={`sys-pill ${statusClass}`}>{statusLabel}</span>
                      </div>
                      <div class="sys-obj-main num">
                        <span class="sys-obj-val">
                          {val.toFixed(Math.abs(val) < 10 ? 3 : 1)} {obj.unit}
                        </span>
                        <span class="sys-obj-band">
                          p10: {uq.p10.toFixed(Math.abs(val) < 10 ? 3 : 1)} · p90: {uq.p90.toFixed(Math.abs(val) < 10 ? 3 : 1)} {obj.unit}
                        </span>
                      </div>
                      <div class="sys-obj-driver">
                        Dominant epistemic driver: <strong>{domEp}</strong> ({(domShare * 100).toFixed(0)}% variance)
                      </div>
                    </div>
                  );
                })}
              </div>

              <div class="sys-section-head" style={{ marginTop: '10px' }}>
                <span>Sparse Adjoint Jacobian (dΦ_m / dp_k)</span>
              </div>
              <div class="sys-adjoint-wrap">
                <table class="sys-table num" data-testid="adjoint-matrix-table">
                  <thead>
                    <tr>
                      <th>Objective \ Lever</th>
                      {activeSummary.design_parameters.map((dp) => (
                        <th key={dp.name} title={`${dp.name} (${dp.unit})`}>
                          {dp.symbol}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {activeSummary.objectives.map((obj, m) => (
                      <tr key={obj.name}>
                        <td style={{ fontFamily: 'var(--sans)', fontWeight: 500 }} title={`${obj.name} (${obj.unit})`}>
                          {obj.symbol}
                        </td>
                        {activeSummary.design_parameters.map((dp, k) => {
                          const raw = bundle.analysis.design_sensitivities[m]?.[k] ?? 0;
                          const per10Pct = raw * 0.1 * (dp.max_bound - dp.min_bound);
                          const cls = Math.abs(per10Pct) < 1e-6 ? 'zero' : per10Pct > 0 ? 'pos' : 'neg';
                          return (
                            <td
                              key={dp.name}
                              class={`adj-cell ${cls}`}
                              title={`d(${obj.symbol})/d(${dp.symbol}) = ${raw.toExponential(3)} ${obj.unit}/${dp.unit} (${per10Pct >= 0 ? '+' : ''}${per10Pct.toFixed(3)} ${obj.unit} per +10% lever span)`}
                            >
                              {Math.abs(per10Pct) < 1e-4 ? '0.00' : (per10Pct >= 0 ? '+' : '') + per10Pct.toFixed(2)}
                            </td>
                          );
                        })}
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div class="sys-hint">
                  Values show ΔΦ per +10% span of each design lever p_k (computed via 1 sparse Adjoint solve Jᵀλ = ∂Φ/∂Wᵀ per objective). Hover any cell for exact physical derivative.
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function CausalAttributionSvg({
  track,
  vis,
}: {
  track: CoupledSolveBundle['analysis']['causal_tracks'][0];
  vis: FloatBathVisualFields | null;
}) {
  const pts = track.points;
  if (!pts.length) return null;
  const W = 760;
  const H = 195;
  const padL = 48;
  const padR = 18;
  const plotW = W - padL - padR;

  const sMin = pts[0].coord_m;
  const sMax = pts[pts.length - 1].coord_m || 1;
  const toX = (s: number) => padL + ((s - sMin) / Math.max(sMax - sMin, 1e-6)) * plotW;

  const mMax = Math.max(
    ...pts.map((p) => p.surviving_exit_defect),
    ...(vis ? vis.varicose_points.map((p) => Math.abs(p.optical_power_mdpt)) : []),
    1,
  );
  const mMin = 0;
  const toY1 = (v: number) => 58 - ((v - mMin) / Math.max(mMax - mMin, 1e-6)) * 42;

  const fMax = Math.max(...pts.map((p) => Math.max(p.forward_transport_kw, p.return_or_loss_transport_kw)), 1);
  const fMin = Math.min(...pts.map((p) => Math.min(p.forward_transport_kw, p.return_or_loss_transport_kw)), 0);
  const toY2 = (v: number) => 122 - ((v - fMin) / Math.max(fMax - fMin, 1e-6)) * 42;

  const hMax = Math.max(...pts.map((p) => p.geometry_profile_m), 1e-3);
  const hMin = Math.min(...pts.map((p) => p.geometry_profile_m), 0);
  const toY3 = (v: number) => 180 - ((v - hMin) / Math.max(hMax - hMin, 1e-6)) * 36;

  const poly = (fnX: (p: (typeof pts)[0]) => number, fnY: (p: (typeof pts)[0]) => number) =>
    pts.map((p) => `${fnX(p).toFixed(1)},${fnY(p).toFixed(1)}`).join(' ');

  return (
    <svg viewBox={`0 0 ${W} ${H}`} class="sys-attr-svg">
      <rect x={padL} y={12} width={plotW} height={48} fill="rgba(255,255,255,0.03)" rx={3} />
      <rect x={padL} y={76} width={plotW} height={48} fill="rgba(255,255,255,0.03)" rx={3} />
      <rect x={padL} y={140} width={plotW} height={42} fill="rgba(255,255,255,0.03)" rx={3} />

      <polyline fill="none" stroke="#60a5fa" stroke-width="2" points={poly((p) => toX(p.coord_m), (p) => toY1(p.surviving_exit_defect))} />
      {vis && vis.sinuous_points.length > 0 && (
        <polyline
          fill="none"
          stroke="#f472b6"
          stroke-width="1.6"
          stroke-dasharray="4 2"
          points={vis.sinuous_points.map((pt) => `${toX(pt.x_m).toFixed(1)},${toY1(Math.abs(pt.optical_power_mdpt)).toFixed(1)}`).join(' ')}
        />
      )}
      <text x={padL + 6} y={24} class="sys-svg-label">
        1. {track.defect_label} (max {mMax.toFixed(1)} {track.defect_unit}{vis ? ' · Solid Blue = Varicose, Dashed Pink = Sinuous' : ''})
      </text>

      <polyline fill="none" stroke="#34d399" stroke-width="1.8" points={poly((p) => toX(p.coord_m), (p) => toY2(p.forward_transport_kw))} />
      <polyline fill="none" stroke="#f87171" stroke-width="1.8" points={poly((p) => toX(p.coord_m), (p) => toY2(p.return_or_loss_transport_kw))} />
      <text x={padL + 6} y={88} class="sys-svg-label">
        2. Enthalpy Transport: Green = Forward Stream, Red = Return / Wall Loss (max {fMax.toFixed(0)} kW)
      </text>

      <polyline fill="none" stroke="#fbbf24" stroke-width="2" points={poly((p) => toX(p.coord_m), (p) => toY3(p.geometry_profile_m))} />
      <text x={padL + 6} y={152} class="sys-svg-label">
        3. {track.geometry_label}: {(hMax * 1000).toFixed(1)} mm max → {(hMin * 1000).toFixed(1)} mm min
      </text>
    </svg>
  );
}

// Register 3D Coupled Physics HUD in viewport + command palette (no floating top-left pill)
viewOverlays.push(function SystemsOverlayHost() {
  useApp();
  return (
    <>
      <CoupledViewportHud />
      <SystemsWorkbench />
    </>
  );
});

extraCommands.push(() => [
  {
    id: 'systems.open',
    group: 'Systems',
    label: 'Open Coupled Multiphysics Systems Workbench…',
    run: () => ui.set('systemsOpen', true),
  },
]);
