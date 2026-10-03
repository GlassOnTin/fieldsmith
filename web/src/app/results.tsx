// Results lens: the analysed surface drawn as the deformed triangle mesh, coloured per facet
// by the chosen quantity, plus a status badge and a control panel. Colour comes from the
// element that owns each boundary facet (element-centroid von Mises), so peaks read honestly.
import { Fragment } from 'preact';
import { ui, useApp } from './ui';
import type { ResultMode } from './ui';
import { store } from '../doc/store';
import { Seg, NumberInput } from './fields';
import { viewOverlays, statusItems } from './App';
import { viewportExtensions } from './ViewportHost';
import type { Viewport } from '../view/viewport';
import { analysis } from './analysis';
import { fmt } from './fields';
import * as THREE from 'three';
import type { LevelResult } from '../doc/wasm';

// ---- colour ramp ---------------------------------------------------------------------

/** 0..1 → colour. A five-stop blue→cyan→green→yellow→red ramp. */
const RAMP: [number, number, number][] = [
  [0.22, 0.27, 0.75],
  [0.1, 0.66, 0.76],
  [0.35, 0.77, 0.3],
  [0.97, 0.78, 0.12],
  [0.79, 0.11, 0.12],
];

export function rampColor(t: number, out: [number, number, number]) {
  const x = Math.min(1, Math.max(0, t));
  const n = RAMP.length - 1;
  const i = Math.min(n - 1, Math.floor(x * n));
  const f = x * n - i;
  for (let k = 0; k < 3; k++) out[k] = RAMP[i][k] + f * (RAMP[i + 1][k] - RAMP[i][k]);
}

function gradientCss(): string {
  const parts = RAMP.map((c, i) => `rgb(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)}) ${(i / (RAMP.length - 1)) * 100}%`);
  return `linear-gradient(90deg, ${parts.join(', ')})`;
}

// ---- values --------------------------------------------------------------------------

/** Effective case index; −1 for the envelope. */
function effCase(lvl: LevelResult): number {
  const c = ui.resultsCase;
  return c >= 0 && c < lvl.meta.cases.length ? c : -1;
}

/** Quantity of one facet for the current display mode. */
function facetValue(lvl: LevelResult, mode: ResultMode, ci: number, f: number): { value: number; unit: string } {
  const owner = lvl.surfaceOwner[f];
  if (mode === 'deflection' || mode === 'vectors') {
    const u = lvl.u[ci >= 0 ? ci : 0];
    const n0 = lvl.surfaceFacets[6 * f], n1 = lvl.surfaceFacets[6 * f + 1], n2 = lvl.surfaceFacets[6 * f + 2];
    let m = 0;
    for (const n of [n0, n1, n2]) {
      m += Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
    }
    return { value: m / 3, unit: 'mm' };
  }
  const vm = (ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope)[owner];
  if (mode === 'safety') return { value: vm > 0 ? lvl.meta.yield_strength / vm : Infinity, unit: 'SF' };
  return { value: vm, unit: 'MPa' };
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
  return Math.max(
    1e-9,
    ...(ci >= 0 ? [cases[ci].max_vm_elem] : cases.map((c) => c.max_vm_elem)),
  );
}

function legendLabels(mode: ResultMode, max: number, yieldMpa: number): { min: string; max: string; caption: string } {
  if (mode === 'stress') return { min: '0', max: `${fmt(max)} MPa`, caption: 'von Mises stress, element centroids' };
  if (mode === 'safety') return { min: `≥ ${fmt(max)}`, max: '0', caption: `safety factor = yield ÷ stress (yield ${fmt(yieldMpa)} MPa); red below 1.0` };
  if (mode === 'deflection') return { min: '0', max: `${fmt(max)} mm`, caption: 'displacement magnitude' };
  return { min: '', max: '', caption: 'displacement direction arrows on the deformed shape' };
}

// ---- mesh ----------------------------------------------------------------------------

function disposeDeep(o: THREE.Object3D) {
  o.traverse((n) => {
    const m = n as THREE.Mesh & { material?: THREE.Material | THREE.Material[] };
    m.geometry?.dispose();
    if (Array.isArray(m.material)) m.material.forEach((x) => x.dispose());
    else m.material?.dispose();
  });
}

const resultGroup = new THREE.Group();
let resultHost: Viewport | null = null;
let resultMesh: THREE.Mesh | null = null;
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
  deflection: 'deflection',
  vectors: 'vectors',
};

function rebuild(lvl: LevelResult) {
  const ci = effCase(lvl);
  const mode = ui.resultsMode;
  const scale = ui.resultsScale;
  const nFacets = lvl.surfaceRegion.length;
  const u = lvl.u[ci >= 0 ? ci : 0];
  const vmArr = ci >= 0 ? lvl.vmElem[ci] : lvl.vmElemEnvelope;
  const vmax = modeMax(lvl, mode, ci);
  const yieldMpa = lvl.meta.yield_strength;

  const positions = new Float32Array(nFacets * 9);
  const colors = new Float32Array(nFacets * 9);
  const col: [number, number, number] = [0, 0, 0];
  for (let f = 0; f < nFacets; f++) {
    const owner = lvl.surfaceOwner[f];
    if (mode === 'stress') rampColor(vmArr[owner] / vmax, col);
    else if (mode === 'safety') rampColor(vmArr[owner] > 0 ? 1 - Math.min(1, (yieldMpa / vmArr[owner]) / vmax) : 0, col);
    else if (mode === 'deflection') {
      let m = 0;
      for (let j = 0; j < 3; j++) {
        const n = lvl.surfaceFacets[6 * f + j];
        m += Math.hypot(u[3 * n], u[3 * n + 1], u[3 * n + 2]);
      }
      rampColor(m / 3 / vmax, col);
    } else {
      col[0] = 0.55; col[1] = 0.55; col[2] = 0.55;
    }
    for (let j = 0; j < 3; j++) {
      const n = lvl.surfaceFacets[6 * f + j];
      const o = 9 * f + 3 * j;
      positions[o] = lvl.nodes[3 * n] + scale * u[3 * n];
      positions[o + 1] = lvl.nodes[3 * n + 1] + scale * u[3 * n + 1];
      positions[o + 2] = lvl.nodes[3 * n + 2] + scale * u[3 * n + 2];
      colors[o] = col[0];
      colors[o + 1] = col[1];
      colors[o + 2] = col[2];
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geo.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  const mesh = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide }));
  mesh.renderOrder = 1;
  resultGroup.add(mesh);
  resultMesh = mesh;

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

function syncResults(vp: Viewport) {
  if (!resultHost) {
    resultHost = vp;
    vp.overlays.add(resultGroup);
  }
  vp.showField = store.lens !== 'results';
  const lvl = analysis.latest;
  const on = store.lens === 'results' && !!lvl;
  const sig = on
    ? `${lvl && lvl.label}|${lvl && lvl.nodes.length}${lvl && lvl.meta.provenance.total_ms}|${ui.resultsMode}|${ui.resultsCase}|${ui.resultsScale}`
    : 'off';
  if (sig === resultSig) return;
  resultSig = sig;
  while (resultGroup.children.length) {
    const c = resultGroup.children.pop()!;
    disposeDeep(c);
  }
  resultMesh = null;
  if (!on || !lvl) return;
  try {
    rebuild(lvl);
  } catch (err) {
    while (resultGroup.children.length) {
      const c = resultGroup.children.pop()!;
      disposeDeep(c);
    }
    console.error('results mesh build failed', err);
  }
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
  const f = hits[0].faceIndex ?? -1;
  if (f < 0 || f >= lvl.surfaceRegion.length) return false;
  const { value, unit } = facetValue(lvl, ui.resultsMode, effCase(lvl), f);
  const p = hits[0].point;
  ui.set('probe', { value, unit, point: [p.x, p.y, p.z] });
  return true;
}

viewportExtensions.push({
  sync: syncResults,
  click: onClickResults,
});

// ---- badge ---------------------------------------------------------------------------

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
  const peak = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem));
  const defl = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement));
  const p = lvl.meta.provenance;
  // Provenance: refinement level, order, counts, timing, and the change since the previous level.
  const lines = [
    lvl.label,
    `${p.elements} elements · order P${p.order} · ${p.dofs} dofs · ${p.iterations} iterations`,
    `mesh ${p.mesh_ms} ms · solve ${p.solve_ms} ms · total ${p.total_ms} ms`,
    `quality min ${p.quality.min_dihedral_deg.toFixed(1)}° dihedral`,
  ];
  if (prev) {
    const pv = Math.max(1e-9, ...prev.meta.cases.map((c) => c.max_vm_elem));
    const pd = Math.max(1e-9, ...prev.meta.cases.map((c) => c.max_displacement));
    const sv = (peak - pv) / pv, sd = (defl - pd) / pd;
    lines.push(`peak ${sv >= 0 ? '+' : ''}${(sv * 100).toFixed(1)}% · deflect ${sd >= 0 ? '+' : ''}${(sd * 100).toFixed(1)}% vs previous level`);
  }
  const title = lines.join('\n');
  return (
    <span class="num" data-testid="analysis-badge" title={title}>
      peak {fmt(peak)} MPa · deflect {fmt(defl)} mm
    </span>
  );
}
statusItems.push(AnalysisBadge);

// ---- panel ---------------------------------------------------------------------------

function ResultsPanel() {
  useApp();
  if (store.lens !== 'results') return null;
  const s = analysis.state;
  const lvl = s.levels[s.levels.length - 1];
  const ci = lvl ? effCase(lvl) : -1;
  const mode = ui.resultsMode;
  const legend = lvl ? legendLabels(mode, modeMax(lvl, mode, ci), lvl.meta.yield_strength) : null;
  return (
    <div class="results-panel" data-testid="results-panel">
      <div class="card-head">
        <span class="title">Results</span>
        {lvl && <span class="meta">{lvl.label}</span>}
      </div>
      {s.status === 'error' && (s.errorKind === 'setup' ? <div class="hint">{s.error}</div> : <div class="error-box">{s.error}</div>)}
      {s.status === 'running' && <div class="hint">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</div>}
      {!lvl && s.status === 'ready' && <div class="hint">No results yet.</div>}
      {lvl && (
        <>
          <div class="field-grid">
            <span class="plain">Show</span>
            <Seg<ResultMode>
              value={mode}
              options={(Object.keys(MODE_NAMES) as ResultMode[]).map((m) => ({ value: m, label: m[0].toUpperCase() + m.slice(1) }))}
              onChange={(v) => ui.set('resultsMode', v)}
              testId="results-mode"
            />
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
            <NumberInput
              value={ui.resultsScale}
              min={0}
              max={200}
              step={1}
              onChange={(v) => ui.set('resultsScale', Math.round(v * 10) / 10)}
              testId="results-scale"
            />
          </div>
          {legend && legend.caption && (
            <div class="legend-block" data-testid="legend">
              {legend.max !== '' && (
                <div class="legend-range">
                  <span class="num">{legend.min}</span>
                  <span class="legend-gradient" style={{ background: gradientCss() }} />
                  <span class="num">{legend.max}</span>
                </div>
              )}
              <div class="meta">{legend.caption}</div>
            </div>
          )}
          <div class="field-grid" data-testid="results-summary">
            <span class="plain">Peak von Mises</span>
            <span class="num">{fmt(Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem)))} MPa</span>
            <span class="plain">Max deflection</span>
            <span class="num">{fmt(Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement)))} mm</span>
            <span class="plain">Elements</span>
            <span class="num">{lvl.meta.provenance.elements} tet · {lvl.meta.provenance.dofs} dofs</span>
            <span class="plain">Timing</span>
            <span class="num">mesh {lvl.meta.provenance.mesh_ms} ms · solve {lvl.meta.provenance.solve_ms} ms</span>
            {lvl.meta.cases.slice(0, 4).map((c) => (
              <Fragment key={c.name}>
                <span class="plain">{c.name}</span>
                <span class="num">{fmt(c.max_vm_elem)} MPa · {fmt(c.max_displacement)} mm</span>
              </Fragment>
            ))}
          </div>
          {s.skippedFine && <div class="hint" data-testid="skipped-fine">{s.skippedFine}</div>}
          {ui.probe && (
            <div class="probe" data-testid="probe">
              {fmt(ui.probe.value)} {ui.probe.unit} at ({ui.probe.point.map((v) => v.toFixed(1)).join(', ')}) mm
            </div>
          )}
        </>
      )}
    </div>
  );
}
viewOverlays.push(ResultsPanel);