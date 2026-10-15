// Make lens: export the part — binary STL and 3MF (millimetre units, part name and material
// recorded) at a chosen mesh spacing, with a health line under it (mesh volume vs field
// volume, defect counts, timings). The layer preview slices the exported mesh itself, so the
// contours are what a slicer would see, and scrubs through latest-wins worker answers.
import { useEffect } from 'preact/hooks';
import { store } from '../doc/store';
import { bodyBounds } from '../doc/wasm';
import type { ExportMeta } from '../doc/wasm';
import { Seg, fmt } from './fields';
import { viewOverlays } from './App';
import { viewportExtensions } from './ViewportHost';
import type { Viewport } from '../view/viewport';
import { analysis } from './analysis';
import { ui, useApp } from './ui';
import * as THREE from 'three';

// ---- slice plane helpers -------------------------------------------------------------

/** Z range of the model body (mm): null when the model is empty. */
function zRange(): [number, number] | null {
  if (store.doc.root.length === 0) return null;
  const b = bodyBounds(store.doc);
  if (!b) return null;
  return [Math.min(b[0][2], b[1][2]), Math.max(b[0][2], b[1][2])];
}

/** Number of preview layers at the chosen layer height. */
function layerCount(lo: number, hi: number): number {
  return Math.max(1, Math.ceil((hi - lo) / ui.makeLayerH));
}

/** Z of the wanted preview layer: centres of the layer so a plane never lands on a mesh
 * vertex row of the export grid spacing. */
function sliceWantedZ(): number | null {
  const zr = zRange();
  if (!zr) return null;
  const [lo, hi] = zr;
  const n = layerCount(lo, hi);
  const layer = Math.min(Math.max(0, ui.makeLayer), n - 1);
  return Math.min(hi, lo + (layer + 0.5) * ui.makeLayerH);
}

// ---- viewport overlay: slice contours --------------------------------------------------

function disposeDeep(o: THREE.Object3D) {
  o.traverse((n) => {
    const m = n as THREE.Mesh & { material?: THREE.Material | THREE.Material[] };
    m.geometry?.dispose();
    if (Array.isArray(m.material)) m.material.forEach((x) => x.dispose());
    else m.material?.dispose();
  });
}

const sliceGroup = new THREE.Group();
let sliceHost: Viewport | null = null;
let sliceSig = '';

function syncSlices(vp: Viewport) {
  if (!sliceHost) {
    sliceHost = vp;
    vp.overlays.add(sliceGroup);
  }
  const s = analysis.state;
  const ex = s.export;
  const on = store.lens === 'make' && ui.makeSliceOn;
  // Contours belong to one export (doc edition + mesh); they persist through a scrub until
  // the fresh layer's answer arrives.
  if (!on || !(ex instanceof Object) || !s.slice || s.slice.z !== sliceWantedZ()) {
    if (sliceSig !== 'off') {
      sliceSig = 'off';
      while (sliceGroup.children.length) {
        const c = sliceGroup.children.pop()!;
        disposeDeep(c);
      }
      vp.invalidate();
    }
    return;
  }
  const key = `${ex.version}|${ex.meta.h}|${ex.meta.verts}|${s.slice.z}|${s.slice.segs.length}`;
  if (key === sliceSig) return;
  sliceSig = key;
  while (sliceGroup.children.length) {
    const c = sliceGroup.children.pop()!;
    disposeDeep(c);
  }
  const segs = s.slice.segs;
  const z = s.slice.z;
  const pos = new Float32Array((segs.length / 4) * 6);
  for (let i = 0, o = 0; i < segs.length; i += 4, o += 6) {
    pos[o] = segs[i];
    pos[o + 1] = segs[i + 1];
    pos[o + 2] = z;
    pos[o + 3] = segs[i + 2];
    pos[o + 4] = segs[i + 3];
    pos[o + 5] = z;
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  const lines = new THREE.LineSegments(
    geo,
    new THREE.LineBasicMaterial({ color: 0xff2d95, transparent: true, opacity: 0.95 }),
  );
  lines.renderOrder = 4;
  sliceGroup.add(lines);
  vp.invalidate();
}

viewportExtensions.push({ sync: syncSlices });

// ---- panel ----------------------------------------------------------------------------

function defectWord(n: number): string {
  return n === 0 ? '0' : String(n);
}

function healthLines(m: ExportMeta): string[] {
  const defects =
    [m.open_edges, m.non_manifold_edges, m.orientation_defects, m.degenerate_triangles, m.self_intersections]
      .reduce((a, b) => a + b, 0) === 0
      ? 'no defects'
      : `open ${defectWord(m.open_edges)} · non-manifold ${defectWord(m.non_manifold_edges)} · orient ${defectWord(m.orientation_defects)} · degen ${defectWord(m.degenerate_triangles)} · self-x ${defectWord(m.self_intersections)}`;
  return [
    `${m.what} at h ${m.h} mm · ${m.verts.toLocaleString()} verts · ${m.tris.toLocaleString()} tris`,
    `mesh volume ${fmt(m.mesh_volume, 1)} mm³ vs field ${fmt(m.field_volume, 1)} ± ${fmt(m.field_err, 1)} · Δ ${
      m.delta_pct >= 0 ? '+' : ''
    }${fmt(m.delta_pct, 3)}%`,
    `${defects}`,
    `mesh ${fmt(m.mesh_ms / 1000, 2)} s · field ref ${fmt(m.ref_ms / 1000, 2)} s · defect scan ${fmt(m.scan_ms / 1000, 2)} s` +
      (m.build_ms > 0 ? ` · fill build ${fmt(m.build_ms / 1000, 2)} s` : ''),
  ];
}

/** H spacing choices for the export mesh (mm). The export path is adaptive dual contouring:
 * it refines to h only near the surface, so 0.1 stays inside the 15 s budget where a uniform
 * mesh of the full part volume would cost minutes. */
const SPACINGS = [0.4, 0.25, 0.15, 0.1];

function MakePanel() {
  useApp();
  if (store.lens !== 'make') return null;
  // The slice request piggybacks on every render: when the wanted plane has no answer yet
  // (new export, scrub, layer-height change), ask once; the worker answers latest-wins.
  const z = sliceWantedZ();
  useEffect(() => {
    if (store.lens !== 'make' || !ui.makeSliceOn) return;
    if (!z) return;
    if (analysis.state.slice?.z !== z) analysis.sliceAt(z);
  });

  const doc = store.doc;
  if (!doc.root.length) {
    return (
      <div class="results-panel" data-testid="make-panel">
        <div class="card-head">
          <span class="title">Make</span>
        </div>
        <div class="hint">Add shapes to the model first.</div>
      </div>
    );
  }

  const s = analysis.state;
  const ex = s.export instanceof Object ? s.export : null;
  const fill = doc.fill !== null;
  const zr = zRange();
  const nLayers = zr ? layerCount(zr[0], zr[1]) : 1;
  const layer = Math.min(Math.max(0, ui.makeLayer), nLayers - 1);

  return (
    <div class="results-panel" data-testid="make-panel">
      <div class="card-head">
        <span class="title">Make</span>
        {ex && <span class="meta">{ex.meta.what} · h {fmt(ex.meta.h)} mm</span>}
      </div>

      <div class="field-grid">
        {fill && (
          <>
            <label class="plain">Part</label>
            <Seg<'f' | 's'>
              value={ui.makeSolid ? 's' : 'f'}
              options={[
                { value: 'f', label: 'Fill' },
                { value: 's', label: 'Solid' },
              ]}
              onChange={(v) => ui.set('makeSolid', v === 's')}
              testId="make-part"
            />
          </>
        )}
        <label class="plain">Mesh spacing</label>
        <Seg<string>
          value={String(ui.makeH)}
          options={SPACINGS.map((h) => ({ value: String(h), label: fmt(h), title: `${fmt(h)} mm` }))}
          onChange={(v) => ui.set('makeH', Number.parseFloat(v))}
          testId="make-h"
        />
        <button
          class={`btn${ex ? '' : ' primary'}`}
          data-testid="make-export"
          disabled={s.export === 'running'}
          onClick={() => analysis.exportMesh(ui.makeH, ui.makeSolid)}
        >
          {s.export === 'running' ? 'exporting…' : ex ? 'Re-build export' : 'Build export'}
        </button>
      </div>

      {s.exportError && (
        <div class="error-box" data-testid="make-error">
          {s.exportError}
        </div>
      )}
      {s.export === 'running' && <div class="hint">exporting the {ui.makeSolid || !fill ? 'solid body' : 'graded fill'}…</div>}

      {ex && (
        <>
          <div class="field-grid" data-testid="make-meta">
            {healthLines(ex.meta).map((l, i) => (
              <span class={i < 3 ? 'plain' : 'meta'} key={i}>
                {l}
              </span>
            ))}
          </div>
          <div class="chips">
            <button class="btn primary" data-testid="make-download-3mf" onClick={() => analysis.downloadMesh('3mf')}>
              Download 3MF
            </button>
            <button class="btn" data-testid="make-download-stl" onClick={() => analysis.downloadMesh('stl')}>
              Download STL
            </button>
          </div>

          <div class="field-grid">
            <label class="plain">Layer height</label>
            <Seg<string>
              value={String(ui.makeLayerH)}
              options={[
                { value: '0.5', label: '0.5' },
                { value: '1', label: '1' },
                { value: '2', label: '2' },
              ]}
              onChange={(v) => ui.set('makeLayerH', Number.parseFloat(v))}
              testId="make-layer-h"
            />
            <label class="plain">Layer</label>
            <div class="chips" data-testid="make-slice">
              <input
                type="range"
                min={0}
                max={nLayers - 1}
                step={1}
                value={layer}
                onInput={(e) => {
                  const v = Number.parseFloat((e.target as HTMLInputElement).value);
                  ui.set('makeLayer', Math.round(v));
                }}
                title={`z ${fmt(zr ? zr[0] + (layer + 0.5) * ui.makeLayerH : 0)} mm of ${fmt(zr ? zr[1] - zr[0] : 0)} mm`}
              />
              <button
                class={ui.makeSliceOn ? 'on' : ''}
                title="Slice preview of the exported mesh"
                onClick={() => ui.set('makeSliceOn', !ui.makeSliceOn)}
              >
                {ui.makeSliceOn ? 'hide' : 'show'}
              </button>
            </div>
            <span class="plain">at z</span>
            <span class="num" data-testid="make-layer-z">
              {fmt(zr ? zr[0] + (layer + 0.5) * ui.makeLayerH : 0)} mm · layer {layer + 1}/{nLayers}
            </span>
          </div>
        </>
      )}

      {!ex && s.export !== 'running' && <div class="hint">No export yet: pick a mesh spacing and build one.</div>}
    </div>
  );
}
viewOverlays.push(MakePanel);