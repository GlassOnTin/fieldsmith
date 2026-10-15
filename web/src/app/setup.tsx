// Setup lens: regions, supports, loads and temperatures on top of the shape model. Clicking
// a surface makes a region (with the region selected, further clicks join its selector
// list); supports, loads and temperatures attach to regions and supports/loads draw as
// glyphs where the analysed mesh actually supports them.
import { Fragment } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import * as THREE from 'three';
import { store } from '../doc/store';
import { FACE_LABEL, findNode, freshId, walk } from '../doc/types';
import type { Document, Face, Load, Node, Region, Support, Temperature } from '../doc/types';
import { FACE_CODES, evalBody, loadFnProbe } from '../doc/wasm';
import type { LevelResult, LoadFnProbe, PickHit } from '../doc/wasm';
import { viewportExtensions } from './ViewportHost';
import type { Viewport, Highlight } from '../view/viewport';
import { inspectors, NameField } from './Inspector';
import { treeSections } from './Tree';
import { NumRow, VecRow, Seg } from './fields';
import { ui, useApp } from './ui';
import { Icon } from './icons';
import { analysis } from './analysis';
import { extraCommands } from './commands';

const REGION_COLORS: [number, number, number][] = [
  [0.25, 0.6, 0.36],
  [0.22, 0.48, 0.86],
  [0.86, 0.52, 0.18],
  [0.7, 0.33, 0.64],
  [0.2, 0.62, 0.62],
];
const SUPPORT_COLOR = 0x2f855a;
const LOAD_COLOR = 0xd0453b;
const AXIS_COLORS: [number, number, number][] = [
  [0.82, 0.27, 0.23],
  [0.3, 0.68, 0.31],
  [0.27, 0.45, 0.83],
];
// Keep in step with viewport.MAX_HIGHLIGHTS so region colouring never thrashes the shader.
const MAX_HIGHLIGHTS = 16;

const REGION_COLORS_CSS = REGION_COLORS.map(
  (c) => `rgb(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)})`,
);

function regionIn(d: Document, id: string): Region {
  const r = d.regions.find((x) => x.id === id);
  if (!r) throw new Error(`missing region ${id}`);
  return r;
}
function supportIn(d: Document, id: string): Support {
  const s = d.supports.find((x) => x.id === id);
  if (!s) throw new Error(`missing support ${id}`);
  return s;
}
function loadIn(d: Document, id: string): Load {
  const l = d.loads.find((x) => x.id === id);
  if (!l) throw new Error(`missing load ${id}`);
  return l;
}

function temperatureIn(d: Document, id: string): Temperature {
  const t = d.temperatures.find((x) => x.id === id);
  if (!t) throw new Error(`missing temperature ${id}`);
  return t;
}

/** Region id the current selection refers to (a region, or its support/load/temperature). */
function selectedRegion(doc: Document): string | null {
  const s = store.selection;
  if (!s) return null;
  if (s.kind === 'region') return s.id;
  try {
    if (s.kind === 'support') return supportIn(doc, s.id).region;
    if (s.kind === 'load') return loadIn(doc, s.id).region;
    if (s.kind === 'temperature') return temperatureIn(doc, s.id).region;
  } catch {
    return null;
  }
  return null;
}

// ---- viewport overlay ----------------------------------------------------------------

function regionHighlights(leafIndex: (id: string) => number): Highlight[] {
  if (store.lens !== 'setup') return [];
  const out: Highlight[] = [];
  for (const [ri, r] of store.doc.regions.entries()) {
    const color = REGION_COLORS[ri % REGION_COLORS.length];
    for (const sl of r.selectors) {
      if (out.length >= MAX_HIGHLIGHTS) return out;
      const li = leafIndex(sl.shape);
      if (li < 0) continue;
      // Mesh patches surface as face codes PATCH_CODE0 + id (10 + n) in the generated
      // GLSL's ident slot; 'all' is face code 0 and the shader treats key.y < 0 as any face.
      const code = sl.patch != null ? 10 + sl.patch : sl.face === 'all' ? -1 : FACE_CODES.indexOf(sl.face);
      out.push({ leaf: li, face: code, color });
    }
  }
  return out;
}

/** Tree/inspector label for one selector: a mesh patch reads `patch 3`, prim faces keep
 * their FACE_LABEL name. */
function selectorLabel(sl: { face: Face; patch?: number }): string {
  return sl.patch != null ? `patch ${sl.patch}` : FACE_LABEL[sl.face];
}

const sameSelector = (
  a: { shape: string; face: Face; patch?: number },
  b: { shape: string; face: Face; patch?: number },
): boolean => a.shape === b.shape && a.face === b.face && a.patch === b.patch;

function onClickRegion(hit: PickHit | null, e: PointerEvent): boolean {
  if (store.lens !== 'setup' || e.button !== 0) return false;
  if (!hit) return false;
  // An imported mesh carries its patch segmentation: a hit on a patch names `mesh1.p3`
  // (face stays 'all'); prim faces keep the face selector.
  const shape = hit.shape;
  const sel: { shape: string; face: Face; patch?: number } | null =
    shape && hit.patch != null
      ? { shape, face: 'all', patch: hit.patch }
      : shape && hit.face
        ? { shape, face: hit.face }
        : null;
  if (!sel) {
    ui.notify('This surface is not a shape face and cannot join a region.', 'error');
    return true;
  }
  const label = selectorLabel(sel);
  const target = store.selection?.kind === 'region' ? store.selection.id : null;
  if (target) {
    if (!store.doc.regions.some((x) => x.id === target)) return false;
    if (!regionIn(store.doc, target).selectors.some((sl) => sameSelector(sl, sel))) {
      store.transact(`Add face to ${target}`, (d) => {
        regionIn(d, target).selectors.push(sel);
      });
    }
  } else {
    let id = '';
    store.transact('Add region', (d) => {
      id = freshId(d, 'region');
      d.regions.push({ id, selectors: [sel] });
    });
    store.select({ kind: 'region', id });
    ui.notify(`${id} — ${label} of ${hit.shape}. Click more surfaces to add faces.`, 'info', 4500);
  }
  return true;
}

// ---- cluster geometry ----------------------------------------------------------------

const A = new THREE.Vector3();
const B = new THREE.Vector3();
const C = new THREE.Vector3();
const E1 = new THREE.Vector3();
const E2 = new THREE.Vector3();
const UP = new THREE.Vector3(0, 0, 1);

interface Cluster {
  pos: THREE.Vector3;
  normal: THREE.Vector3;
  /** Facet centroids in this bucket, flat xyz — bounds the support patch to the
   * surface the region actually claims, so markers never stick out past a face. */
  pts: number[];
}

/** Surface facets of one region, bucketed on a 2 mm grid and combined face-normal. Reads the
 * analysis's per-region facet sets, so a region overlapped by a later region (which the
 * shared surfaceRegion map reassigns) still yields its own facets. */
function clusters(lvl: LevelResult, ri: number): Cluster[] {
  const sets = lvl.regionFacetSets;
  const offs = lvl.regionFacetOffsets;
  if (!sets || !offs || ri < 0 || ri >= offs.length - 1) return [];
  const sums = new Map<
    string,
    { x: number; y: number; z: number; nx: number; ny: number; nz: number; pts: number[] }
  >();
  const f = lvl.surfaceFacets;
  const nodes = lvl.nodes;
  for (let k = offs[ri]; k < offs[ri + 1]; k++) {
    const i = sets[k];
    const a = f[6 * i], b = f[6 * i + 1], c = f[6 * i + 2];
    A.set(nodes[3 * a], nodes[3 * a + 1], nodes[3 * a + 2]);
    B.set(nodes[3 * b], nodes[3 * b + 1], nodes[3 * b + 2]);
    C.set(nodes[3 * c], nodes[3 * c + 1], nodes[3 * c + 2]);
    TMP.copy(A).add(B).add(C).multiplyScalar(1 / 3);
    E1.subVectors(B, A);
    E2.subVectors(C, A);
    E1.cross(E2);
    const key = `${Math.round(TMP.x / 2)},${Math.round(TMP.y / 2)},${Math.round(TMP.z / 2)}`;
    const acc = sums.get(key) ?? { x: 0, y: 0, z: 0, nx: 0, ny: 0, nz: 0, pts: [] };
    acc.x = TMP.x; acc.y = TMP.y; acc.z = TMP.z;
    acc.pts.push(TMP.x, TMP.y, TMP.z);
    acc.nx += E1.x; acc.ny += E1.y; acc.nz += E1.z;
    sums.set(key, acc);
  }
  const out: Cluster[] = [];
  for (const acc of sums.values()) {
    const pos = new THREE.Vector3(acc.x, acc.y, acc.z);
    const normal = new THREE.Vector3(acc.nx, acc.ny, acc.nz);
    if (normal.lengthSq() < 1e-9) normal.set(0, 0, 1);
    normal.normalize();
    out.push({ pos, normal, pts: acc.pts });
  }
  return out;
}
const TMP = new THREE.Vector3();

/** Cluster nearest the centroid of a cluster list — the anchor for the single glyph that
 * stands for a support or a force load, whose fixed axes and direction don't vary per
 * cluster. */
function centroidAnchor(cs: Cluster[]): Cluster {
  const mean = cs
    .reduce((acc, c) => acc.add(c.pos), new THREE.Vector3())
    .multiplyScalar(1 / cs.length);
  let best = cs[0];
  let bd = Infinity;
  for (const c of cs) {
    const dd = c.pos.distanceToSquared(mean);
    if (dd < bd) {
      bd = dd;
      best = c;
    }
  }
  return best;
}

/** Outward-oriented cluster normals: the field crosses zero at the surface, so a point a
 * half-millimetre along +normal must read positive. */
function orientOutward(d: Document, cls: Cluster[]) {
  if (cls.length === 0) return;
  const pts = new Float64Array(cls.length * 3);
  cls.forEach((c, i) => {
    pts[3 * i] = c.pos.x + 0.5 * c.normal.x;
    pts[3 * i + 1] = c.pos.y + 0.5 * c.normal.y;
    pts[3 * i + 2] = c.pos.z + 0.5 * c.normal.z;
  });
  const f = evalBody(d, pts);
  cls.forEach((c, i) => {
    if (f[i] < 0) c.normal.negate();
  });
}

function disposeDeep(o: THREE.Object3D) {
  o.traverse((n) => {
    const m = n as THREE.Mesh & { material?: THREE.Material | THREE.Material[] };
    m.geometry?.dispose();
    if (Array.isArray(m.material)) m.material.forEach((x) => x.dispose());
    else m.material?.dispose();
  });
}

/** How many surface facets the latest analysis matched for region `ri` (null while no
 * analysis has run or the region is not in the document order the analysis used). */
function matchedFaceCount(ri: number): number | null {
  const lvl = analysis.latest;
  if (!lvl) return null;
  if (ri < 0 || ri >= lvl.regionFacetOffsets.length - 1) return null;
  return lvl.regionFacetOffsets[ri + 1] - lvl.regionFacetOffsets[ri];
}

/** Tree/inspector face label: the matched facet count once the analysis has run (that is
 * what supports and loads act on), otherwise the count of clicked selectors. */
function faceCountLabel(ri: number, selectors: number): string {
  const n = matchedFaceCount(ri) ?? selectors;
  return `${n} ${n === 1 ? 'face' : 'faces'}`;
}

const glyphs = new THREE.Group();
let glyphHost: Viewport | null = null;
let glyphSig = '';

function clearGlyphs() {
  while (glyphs.children.length) {
    const c = glyphs.children.pop()!;
    disposeDeep(c);
  }
}

/** Model diagonal from the analysed nodes — sizes glyph marks relative to the part. */
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

function buildGlyphs(lvl: LevelResult) {
  const d = store.doc;
  const base = 0.22 * (modelDiag(lvl) || 40);
  const forceVecs = d.loads.flatMap((l) => (l.kind.type === 'force' ? [l.kind.vector] : []));
  const maxMag = Math.max(1e-9, ...forceVecs.map((v) => Math.hypot(v[0], v[1], v[2])));

  // Per-region clusters, oriented outward once and shared between supports and loads.
  const cache = new Map<number, Cluster[]>();
  const regionClusters = (ri: number) => {
    let cs = cache.get(ri);
    if (!cs) {
      cs = clusters(lvl, ri);
      orientOutward(d, cs);
      cache.set(ri, cs);
    }
    return cs;
  };

  const size = Math.min(14, Math.max(3, 4 * (lvl.meta.provenance.element_size || 4)));
  for (const s of d.supports) {
    const ri = d.regions.findIndex((r) => r.id === s.region);
    if (ri < 0) continue;
    const cs = regionClusters(ri);
    const discMat = new THREE.MeshBasicMaterial({
      color: SUPPORT_COLOR,
      transparent: true,
      opacity: 0.45,
      side: THREE.DoubleSide,
      depthWrite: false,
    });
    const linePos: number[] = [];
    const lineCol: number[] = [];
    for (const c of cs) {
      // Support patch bounded by the bucket's own facet centroids, rotated into the patch
      // plane, and floated just off the surface on the non-material side. A patch bigger
      // than the bucket sticks out past the part's faces; one sunk into the material is
      // hidden by the surface and only shows where it pokes out.
      const quat = new THREE.Quaternion().setFromUnitVectors(UP, c.normal);
      const ux = new THREE.Vector3(1, 0, 0).applyQuaternion(quat);
      const uy = new THREE.Vector3(0, 1, 0).applyQuaternion(quat);
      let uMin = Infinity, uMax = -Infinity, vMin = Infinity, vMax = -Infinity;
      for (let i = 0; i < c.pts.length; i += 3) {
        const dx = c.pts[i] - c.pos.x;
        const dy = c.pts[i + 1] - c.pos.y;
        const dz = c.pts[i + 2] - c.pos.z;
        const u = dx * ux.x + dy * ux.y + dz * ux.z;
        const v = dx * uy.x + dy * uy.y + dz * uy.z;
        uMin = Math.min(uMin, u);
        uMax = Math.max(uMax, u);
        vMin = Math.min(vMin, v);
        vMax = Math.max(vMax, v);
      }
      const mesh = new THREE.Mesh(
        new THREE.PlaneGeometry(Math.max(0.6, uMax - uMin), Math.max(0.6, vMax - vMin)),
        discMat,
      );
      mesh.quaternion.copy(quat);
      mesh.position
        .copy(c.pos)
        .addScaledVector(c.normal, -0.15)
        .addScaledVector(ux, (uMin + uMax) / 2)
        .addScaledVector(uy, (vMin + vMax) / 2);
      mesh.renderOrder = 2;
      glyphs.add(mesh);
    }
    // One axis cross per support, not per cluster: the fixed axes are a property of the
    // support, so a cross per cluster stacks identical x-ray streaks. Anchor it at the
    // cluster nearest the support's centroid.
    if (cs.length) {
      const anchorC = centroidAnchor(cs);
      const anchor = anchorC.pos.clone().addScaledVector(anchorC.normal, 0.4);
      const L = Math.max(2.5, Math.min(7, size * 0.55));
      for (let ax = 0; ax < 3; ax++) {
        if (!s.fixed[ax]) continue;
        linePos.push(anchor.x, anchor.y, anchor.z);
        linePos.push(anchor.x + (ax === 0 ? L : 0), anchor.y + (ax === 1 ? L : 0), anchor.z + (ax === 2 ? L : 0));
        const col = AXIS_COLORS[ax];
        lineCol.push(col[0], col[1], col[2], col[0], col[1], col[2]);
      }
    }
    if (linePos.length) {
      const geo = new THREE.BufferGeometry();
      geo.setAttribute('position', new THREE.Float32BufferAttribute(linePos, 3));
      geo.setAttribute('color', new THREE.Float32BufferAttribute(lineCol, 3));
      const lines = new THREE.LineSegments(
        geo,
        new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthTest: false }),
      );
      lines.renderOrder = 3;
      glyphs.add(lines);
    }
  }

  for (const l of d.loads) {
    const ri = d.regions.findIndex((r) => r.id === l.region);
    if (ri < 0) continue;
    const cs = regionClusters(ri);
    if (l.kind.type === 'force') {
      const v = l.kind.vector;
      const mag = Math.hypot(v[0], v[1], v[2]);
      if (mag < 1e-9) continue;
      const dir = new THREE.Vector3(v[0] / mag, v[1] / mag, v[2] / mag);
      const len = base * (0.35 + 0.65 * (mag / maxMag));
      // One arrow per force load: the direction is constant across the region, so
      // per-cluster arrows stack identical glyphs. Pressure keeps per-patch arrows
      // because its directions genuinely vary over the surface.
      if (cs.length) {
        const anchorC = centroidAnchor(cs);
        const arrow = new THREE.ArrowHelper(
          dir,
          anchorC.pos.clone().addScaledVector(dir, 0.2),
          len,
          LOAD_COLOR,
          Math.min(2.2, len * 0.3),
          Math.min(1.3, len * 0.15),
        );
        arrow.renderOrder = 3;
        glyphs.add(arrow);
      }
    } else if (l.kind.type === 'distributed') {
      // The traction direction is constant per load (per-facet magnitude varies, which the
      // browser cannot compute — it never rescales), so one arrow at the centroid anchor.
      // A frame-local direction needs the solve-fitted face frame, which is not carried
      // here; the inspector's fn probe and the provenance report carry the numbers instead
      // of a guessed arrow.
      if (l.kind.dir.form !== 'global') continue;
      const v = l.kind.dir.v;
      const mag = Math.hypot(v[0], v[1], v[2]);
      if (mag < 1e-9) continue;
      if (!cs.length) continue;
      const anchorC = centroidAnchor(cs);
      const len = base * 0.5;
      const arrow = new THREE.ArrowHelper(
        new THREE.Vector3(v[0] / mag, v[1] / mag, v[2] / mag),
        anchorC.pos.clone().addScaledVector(anchorC.normal, 0.2),
        len,
        LOAD_COLOR,
        Math.min(2.2, len * 0.3),
        Math.min(1.3, len * 0.15),
      );
      arrow.renderOrder = 3;
      glyphs.add(arrow);
    } else {
      // Pressure pushes into the face: a short arrow along −normal at each patch.
      const len = base * 0.5;
      for (const c of cs) {
        const arrow = new THREE.ArrowHelper(
          c.normal.clone().negate(),
          c.pos.clone().addScaledVector(c.normal, -0.2),
          len,
          LOAD_COLOR,
          Math.min(2.2, len * 0.3),
          Math.min(1.3, len * 0.15),
        );
        arrow.renderOrder = 3;
        glyphs.add(arrow);
      }
    }
  }
}

function syncGlyphs(vp: Viewport) {
  if (!glyphHost) {
    glyphHost = vp;
    vp.overlays.add(glyphs);
  }
  const lvl =
    store.lens === 'setup' && analysis.launchedFor === store.docVersion ? analysis.latest : null;
  const lfp = lvl ? `${lvl.label}:${lvl.nodes.length}:${lvl.surfaceRegion.length}` : 'none';
  const sig = store.lens === 'setup' ? `${store.docVersion}|${lfp}` : 'off';
  if (sig === glyphSig) return;
  glyphSig = sig;
  const had = glyphs.children.length > 0;
  clearGlyphs();
  if (store.lens !== 'setup' || !lvl) {
    if (had) vp.invalidate();
    return;
  }
  try {
    buildGlyphs(lvl);
  } catch (err) {
    clearGlyphs();
    console.error('glyph build failed', err);
  }
  vp.invalidate();
}

// Level arrivals don't touch the store, so bridge them into a UI re-render (and a sync).
analysis.subscribe(() => ui.emit());

// ---- add / select helpers -------------------------------------------------------------

function addAnalysis(kind: 'support' | 'load' | 'temperature') {
  const rid = selectedRegion(store.doc);
  if (!rid) {
    ui.notify('Create a region first: click a surface in the model.', 'error');
    return;
  }
  let id = '';
  store.transact(
    kind === 'support' ? 'Add support' : kind === 'load' ? 'Add load' : 'Add temperature',
    (d) => {
      id = freshId(d, kind);
      if (kind === 'support') d.supports.push({ id, region: rid, fixed: [true, true, true] });
      else if (kind === 'load')
        d.loads.push({ id, region: rid, case: 'main', kind: { type: 'force', vector: [0, 0, -100] } });
      else d.temperatures.push({ id, region: rid, celsius: 80, case: 'Thermal 1' });
    }
  );
  store.select({ kind, id });
}

// ---- tree -----------------------------------------------------------------------------

function SectionHead(props: { label: string; add?: () => void; title?: string; testId?: string }) {
  return (
    <div class="section-label">
      <span>{props.label}</span>
      {props.add && (
        <button class="btn ghost icon small" title={props.title} data-testid={props.testId} onClick={props.add}>
          <Icon.plus />
        </button>
      )}
    </div>
  );
}

function AnalysisRow(props: { kind: 'region' | 'support' | 'load' | 'temperature'; id: string; swatch?: string; meta: string }) {
  const selected = store.selection?.kind === props.kind && store.selection.id === props.id;
  return (
    <div
      class={`row ${selected ? 'selected' : ''}`}
      role="treeitem"
      aria-selected={selected}
      data-testid={`${props.kind}-${props.id}`}
      onClick={() => store.select({ kind: props.kind, id: props.id })}
    >
      {props.swatch !== undefined ? (
        <span class="swatch" style={{ background: props.swatch }} />
      ) : (
        <span class="glyph">{props.kind === 'support' ? '⛨︎' : props.kind === 'temperature' ? '°' : '↓'}</span>
      )}
      <span class="row-name">{props.id}</span>
      <span class="meta">{props.meta}</span>
    </div>
  );
}

function SetupTree() {
  useApp();
  if (store.lens !== 'setup') return null;
  const doc = store.doc;
  return (
    <>
      <SectionHead label="Regions" />
      {doc.regions.length === 0 && <div class="hint">Click a surface of the model to make a region.</div>}
      {doc.regions.map((r, ri) => (
        <AnalysisRow
          key={r.id}
          kind="region"
          id={r.id}
          swatch={REGION_COLORS_CSS[ri % REGION_COLORS_CSS.length]}
          meta={faceCountLabel(ri, r.selectors.length)}
        />
      ))}
      <SectionHead label="Supports" add={() => addAnalysis('support')} title="Add support on the selected region" testId="add-support" />
      {doc.supports.length === 0 && <div class="hint">Select a region, then add a support.</div>}
      {doc.supports.map((s) => (
        <AnalysisRow key={s.id} kind="support" id={s.id} meta={s.region} />
      ))}
      <SectionHead label="Loads" add={() => addAnalysis('load')} title="Add load on the selected region" testId="add-load" />
      {doc.loads.length === 0 && <div class="hint">Select a region, then add a load.</div>}
      {doc.loads.map((l) => (
        <AnalysisRow
          key={l.id}
          kind="load"
          id={l.id}
          meta={`${l.kind.type} · ${l.case}`}
        />
      ))}
      <SectionHead
        label="Temperatures"
        add={() => addAnalysis('temperature')}
        title="Add fixed temperature on the selected region"
        testId="add-temperature"
      />
      {doc.temperatures.length === 0 && (
        <div class="hint">Select a region, then add a fixed temperature.</div>
      )}
      {doc.temperatures.map((t) => (
        <AnalysisRow key={t.id} kind="temperature" id={t.id} meta={`${t.celsius} °C · ${t.case}`} />
      ))}
    </>
  );
}
treeSections.push(SetupTree);

// ---- shared rows ----------------------------------------------------------------------

function TextInputRow(props: { label: string; value: string; commit: (v: string) => void; testId?: string; title?: string }) {
  const [v, setV] = useState(props.value);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (!dirty) setV(props.value);
  }, [props.value, dirty]);
  const commitText = () => {
    const t = v.trim();
    if (dirty && t !== props.value) props.commit(t);
    setDirty(false);
  };
  return (
    <>
      <label class="plain">{props.label}</label>
      <div class="numbox">
        <input
          class="num"
          value={v}
          data-testid={props.testId}
          title={props.title}
          spellcheck={false}
          onInput={(e) => {
            setV((e.target as HTMLInputElement).value);
            setDirty(true);
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
          }}
          onBlur={commitText}
        />
      </div>
    </>
  );
}

function RegionPicker(props: { value: string; testId: string; commit: (region: string) => void }) {
  const doc = store.doc;
  return (
    <select
      class="select"
      data-testid={props.testId}
      value={props.value}
      onChange={(e) => props.commit((e.target as HTMLSelectElement).value)}
    >
      {doc.regions.map((r) => (
        <option key={r.id} value={r.id}>{r.id}</option>
      ))}
    </select>
  );
}

// ---- inspectors -----------------------------------------------------------------------

function removeFace(regionId: string, index: number) {
  store.transact(`Remove face from ${regionId}`, (d) => {
    const r = d.regions.find((x) => x.id === regionId);
    if (!r) return;
    r.selectors.splice(index, 1);
    if (r.selectors.length === 0) {
      d.regions = d.regions.filter((x) => x.id !== regionId);
      d.supports = d.supports.filter((s) => s.region !== regionId);
      d.loads = d.loads.filter((l) => l.region !== regionId);
      d.temperatures = d.temperatures.filter((t) => t.region !== regionId);
      if (d.fill) d.fill.protect = d.fill.protect.filter((p) => p !== regionId);
    }
  });
}

/** Leaf shapes a region selector can name: the field's prims, meshes and extends. A
 * group is left out (it is a container, not a surface); an extend is in — it exposes
 * its swept surface's face (M7), so `skirt.nz` labels the skirt wall. */
function selectableShapes(doc: Document): Node[] {
  const out: Node[] = [];
  walk(doc.root, (n) => {
    if (n.kind.type !== 'group') out.push(n);
  });
  return out;
}

/** The face options the .fsm actually accepts for one shape. A mesh lists its face names
 * without `side` (validate refuses it: a mesh has no side face) plus one patch entry per
 * segmented patch; prims keep the full face list — a prim without the named face simply
 * claims nothing at the solve. An extend lists its axis faces without `all` or `side`
 * (validate refuses those on a derived solid: the swept surface has no side wall). */
function faceOptions(node: Node | null): { value: string; label: string }[] {
  if (!node) return [];
  if (node.kind.type === 'extend') {
    return FACE_CODES.filter((f) => f !== 'all' && f !== 'side').map((f) => ({ value: `f:${f}`, label: FACE_LABEL[f] }));
  }
  if (node.kind.type !== 'mesh') {
    return FACE_CODES.map((f) => ({ value: `f:${f}`, label: FACE_LABEL[f] }));
  }
  const opts = FACE_CODES.filter((f) => f !== 'side').map((f) => ({ value: `f:${f}`, label: FACE_LABEL[f] }));
  for (let p = 1; p <= node.kind.patches; p++) opts.push({ value: `p:${p}`, label: `Patch ${p}` });
  return opts;
}

/** Hand-build a selector: shape and face chosen from menus instead of a surface click. */
function AddFaceRow({ regionId }: { regionId: string }) {
  const shapes = selectableShapes(store.doc);
  const [shapeId, setShapeId] = useState('');
  const sid = shapes.some((n) => n.id === shapeId) ? shapeId : shapes[0]?.id ?? '';
  const opts = faceOptions(sid ? findNode(store.doc, sid) : null);
  const [faceVal, setFaceVal] = useState('');
  const fv = opts.some((o) => o.value === faceVal) ? faceVal : opts[0]?.value ?? '';
  if (!shapes.length) return null;
  const add = () => {
    const [kind, id] = fv.split(':');
    // A patch entry keeps `face` 'all' (validate: a patch selector on a mesh); a face entry
    // names the face of the chosen shape. Both forms are exactly what the .fsm parser takes.
    const sel =
      kind === 'p'
        ? { shape: sid, face: 'all' as Face, patch: Number(id) }
        : { shape: sid, face: id as Face };
    if (regionIn(store.doc, regionId).selectors.some((sl) => sameSelector(sl, sel))) {
      ui.notify('That face is already in this region.', 'info');
      return;
    }
    store.transact(`Add face to ${regionId}`, (d) => {
      regionIn(d, regionId).selectors.push(sel);
    });
  };
  return (
    <div style={{ display: 'flex', gap: 4, marginTop: 8 }}>
      <select
        class="select"
        data-testid="add-face-shape"
        value={sid}
        onChange={(e) => setShapeId((e.target as HTMLSelectElement).value)}
      >
        {shapes.map((n) => (
          <option key={n.id} value={n.id}>
            {n.id}
          </option>
        ))}
      </select>
      <select
        class="select"
        data-testid="add-face-face"
        value={fv}
        onChange={(e) => setFaceVal((e.target as HTMLSelectElement).value)}
      >
        {opts.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <button class="btn ghost small" data-testid="add-face" onClick={add}>
        Add
      </button>
    </div>
  );
}

function RegionInspector({ id }: { id: string }) {
  useApp();
  const r = store.doc.regions.find((x) => x.id === id);
  if (!r) return null;
  return (
    <div class="insp" data-inspector="region">
      <div class="insp-heading">
        <span class="kind">Region</span>
        <span class="row-name">{r.id}</span>
      </div>
      <NameField id={r.id} />
      <div class="field-grid">
        {r.selectors.map((sl, i) => (
          <Fragment key={i}>
            <span class="plain">{selectorLabel(sl)}</span>
            <span class="sel-face">
              <span>{sl.shape}</span>
              <button class="mini icon" title="Remove this face" onClick={() => removeFace(id, i)}>
                <Icon.trash />
              </button>
            </span>
          </Fragment>
        ))}
      </div>
      <AddFaceRow regionId={id} />
      <div class="hint">
        {r.selectors.length === 1
          ? 'Click surfaces to add faces, or pick a shape and face above. Removing the last face removes the region and its supports, loads and temperatures.'
          : 'Click surfaces in the model, or pick a shape and face above.'}
      </div>
    </div>
  );
}

function SupportInspector({ id }: { id: string }) {
  useApp();
  const s = store.doc.supports.find((x) => x.id === id);
  if (!s) return null;
  const region = store.doc.regions.find((x) => x.id === s.region);
  const setFixed = (i: number, v: boolean) => {
    store.transact('Set support', (d) => {
      const t = d.supports.find((x) => x.id === id);
      if (t) t.fixed[i] = v;
    });
  };
  return (
    <div class="insp" data-inspector="support">
      <div class="insp-heading">
        <span class="kind">Support</span>
        <span class="row-name">{s.id}</span>
      </div>
      <NameField id={s.id} />
      <div class="field-grid">
        <label class="plain">Region</label>
        <RegionPicker
          value={s.region}
          testId="support-region"
          commit={(rid) => store.transact('Set support region', (d) => void (supportIn(d, id).region = rid))}
        />
        <label class="plain">Fix</label>
        <div class="axis-checks">
          {([0, 1, 2] as const).map((i) => (
            <label class="check" key={i}>
              <input
                type="checkbox"
                data-testid={`fixed-${'xyz'[i]}`}
                checked={s.fixed[i]}
                onChange={(e) => setFixed(i, (e.target as HTMLInputElement).checked)}
              />
              {'XYZ'[i]}
            </label>
          ))}
        </div>
      </div>
      <div class="hint">
        {region
          ? `${faceCountLabel(store.doc.regions.findIndex((r) => r.id === region.id), region.selectors.length)} constrained where they touch.`
          : 'Region missing.'}
      </div>
    </div>
  );
}

/** Curved preview of one live-compiled fn (M11): the q(x) samples from the wasm probe,
 * band edges as hairlines, and the per-1-mm integral. The probe runs on each keystroke —
 * compiling a short expression is microseconds. y-dependence previews its y = 0 slice. */
function FnCurve(p: LoadFnProbe) {
  if (!p.ok || !p.xs || !p.q || !p.domain) {
    return <div class="error-box">{p.error ?? 'nothing to draw'}</div>;
  }
  const { xs, q, domain, integral = 0 } = p;
  const W = 220;
  const H = 64;
  const PAD = 5;
  const xr = domain.hi - domain.lo || 1;
  const qmax = Math.max(1e-12, ...q.map(Math.abs));
  const y0 = H * 0.72;
  const amp = (H * 0.25) / qmax;
  const px = (x: number) => PAD + ((x - domain.lo) / xr) * (W - 2 * PAD);
  const pts = xs.map((x, i) => `${px(x).toFixed(1)},${(y0 - q[i] * amp).toFixed(1)}`).join(' ');
  return (
    <div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        data-testid="fn-curve"
        style={{ width: '100%', display: 'block', marginTop: 4 }}
      >
        {p.bands?.map((b, i) => (
          <Fragment key={i}>
            <line x1={px(b.lo)} y1={0} x2={px(b.lo)} y2={H} stroke="var(--line-strong)" strokeWidth="1" />
            <line x1={px(b.hi)} y1={0} x2={px(b.hi)} y2={H} stroke="var(--line-strong)" strokeWidth="1" />
          </Fragment>
        ))}
        <line x1={PAD} y1={y0} x2={W - PAD} y2={y0} stroke="var(--line)" strokeWidth="1" />
        <polyline points={pts} fill="none" stroke="var(--accent)" strokeWidth="1.5" />
        <text x={PAD} y={H - 1} fontSize="9" fill="var(--text-3)" fontFamily="var(--mono)">
          x {domain.lo.toFixed(1)} … {domain.hi.toFixed(1)} mm ({domain.from === 'bands' ? 'fn bands' : 'default'}) · from the face's centroid
        </text>
      </svg>
      <div class="hint">
        ∫ q dx ≈ {integral.toFixed(3)} N per 1 mm of face width (y = 0 slice). The applied
        total uses the real facets; the solve report names it.
      </div>
    </div>
  );
}

/** One distributed-load fn editor: live-compiled text plus the probe's curve, integral,
 * and validate strings. Commits to the document on blur, like the other text rows. */
function FnField({ id, expr }: { id: string; expr: string }) {
  const [v, setV] = useState(expr);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    if (!dirty) setV(expr);
  }, [expr, dirty]);
  const probe = loadFnProbe(v, -20, 20);
  const commit = () => {
    const t = v.trim();
    if (dirty && t !== expr) {
      store.transact('Set load fn', (d) => {
        const k = loadIn(d, id).kind;
        if (k.type === 'distributed') k.expr = t;
      });
    }
    setDirty(false);
  };
  return (
    <div style={{ gridColumn: '1 / -1', display: 'flex', flexDirection: 'column', gap: 6 }}>
      <div class="numbox">
        <input
          class="num"
          value={v}
          data-testid="load-fn"
          spellcheck={false}
          title={'Bands `lo < x < hi -> value` separated by ; at the face frame: x from the centroid, y across, n outward'}
          onInput={(e) => {
            setV((e.target as HTMLInputElement).value);
            setDirty(true);
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
          }}
          onBlur={commit}
        />
      </div>
      {probe.checks && probe.checks.length > 0 && (
        <div class="error-box">
          {probe.checks.map((c) => (
            <div key={c}>{c}</div>
          ))}
        </div>
      )}
      <FnCurve {...probe} />
    </div>
  );
}

function LoadInspector({ id }: { id: string }) {
  useApp();
  const l = store.doc.loads.find((x) => x.id === id);
  if (!l) return null;
  const setKind = (t: 'force' | 'pressure' | 'distributed') => {
    if (t === l.kind.type) return;
    store.transact('Set load kind', (d) => {
      const load = loadIn(d, id);
      load.kind =
        t === 'force'
          ? { type: 'force', vector: [0, 0, -100] }
          : t === 'pressure'
            ? { type: 'pressure', mpa: 1 }
            : { type: 'distributed', expr: '0.01', dir: { form: 'local', v: [0, 0, -1] } };
    });
  };
  return (
    <div class="insp" data-inspector="load">
      <div class="insp-heading">
        <span class="kind">Load</span>
        <span class="row-name">{l.id}</span>
      </div>
      <NameField id={l.id} />
      <div class="field-grid">
        <label class="plain">Region</label>
        <RegionPicker
          value={l.region}
          testId="load-region"
          commit={(rid) => store.transact('Set load region', (d) => void (loadIn(d, id).region = rid))}
        />
        <label class="plain">Type</label>
        <Seg<'force' | 'pressure' | 'distributed'>
          value={l.kind.type}
          options={[
            { value: 'force', label: 'Force' },
            { value: 'pressure', label: 'Pressure' },
            { value: 'distributed', label: 'Fn', title: 'Force per unit area from a compiled fn over the face' },
          ]}
          onChange={setKind}
          testId="load-kind"
        />
        {l.kind.type === 'force' && (
          <>
            <VecRow
              label="Force"
              unit="N"
              get={(d) => {
                const k = loadIn(d, id).kind;
                return k.type === 'force' ? k.vector : [0, 0, 0];
              }}
              set={(d, v) => {
                const k = loadIn(d, id).kind;
                if (k.type === 'force') k.vector = v;
              }}
              undoLabel="Set force"
              testId="force"
              step={10}
            />
            <TextInputRow
              label="Case"
              value={l.case}
              commit={(v) => store.transact('Set load case', (d) => void (loadIn(d, id).case = v))}
              testId="load-case"
              title="Named load case; the Results lens can switch between cases"
            />
          </>
        )}
        {l.kind.type === 'pressure' && (
          <NumRow
            label="Pressure"
            unit="MPa"
            min={0}
            get={(d) => {
              const k = loadIn(d, id).kind;
              return k.type === 'pressure' ? k.mpa : 0;
            }}
            set={(d, v) => {
              const k = loadIn(d, id).kind;
              if (k.type === 'pressure') k.mpa = v;
            }}
            undoLabel="Set pressure"
            testId="pressure"
            title="Positive pushes into the face"
          />
        )}
      </div>
      {l.kind.type === 'distributed' && (
        <div class="field-grid" style={{ marginTop: 4 }}>
          <FnField id={id} expr={l.kind.expr} />
          <label class="plain">Direction</label>
          <Seg<'global' | 'local'>
            value={l.kind.dir.form}
            options={[
              { value: 'global', label: 'World', title: 'Fixed world vector (x, y, z); normalised at the solve' },
              { value: 'local', label: 'Face frame', title: 'x, y, then outward normal n of the face frame' },
            ]}
            onChange={(t) =>
              store.transact('Set load direction', (d) => {
                const k = loadIn(d, id).kind;
                if (k.type !== 'distributed') return;
                k.dir = t === 'global' ? { form: 'global', v: [0, 0, -1] } : { form: 'local', v: [0, 0, -1] };
              })
            }
            testId="load-dir-form"
          />
          {l.kind.dir.form === 'global' ? (
            <VecRow
              label="Direction"
              get={(d) => {
                const k = loadIn(d, id).kind;
                return k.type === 'distributed' && k.dir.form === 'global' ? k.dir.v : [0, 0, -1];
              }}
              set={(d, v) => {
                const k = loadIn(d, id).kind;
                if (k.type === 'distributed' && k.dir.form === 'global') k.dir = { form: 'global', v };
              }}
              undoLabel="Set load direction"
              testId="load-dir"
              step={1}
            />
          ) : (
            <VecRow
              label="Frame dir"
              get={(d) => {
                const k = loadIn(d, id).kind;
                return k.type === 'distributed' && k.dir.form === 'local' ? k.dir.v : [0, 0, -1];
              }}
              set={(d, v) => {
                const k = loadIn(d, id).kind;
                if (k.type === 'distributed' && k.dir.form === 'local') k.dir = { form: 'local', v };
              }}
              undoLabel="Set load frame direction"
              testId="load-dir"
              step={1}
            />
          )}
          <NumRow
            label="Total"
            unit="N"
            get={(d) => {
              const k = loadIn(d, id).kind;
              return k.type === 'distributed' ? k.total ?? 0 : 0;
            }}
            set={(d, v) => {
              const k = loadIn(d, id).kind;
              if (k.type !== 'distributed') return;
              if (v === 0) delete k.total;
              else k.total = v;
            }}
            undoLabel="Set load total"
            testId="load-total"
            step={10}
            title="Optional: rescale the fn so its resultant is this many newtons (0 keeps the fn as written)"
          />
          <div class="hint" style={{ gridColumn: '1 / -1' }}>
            Density in N/mm² at the face's frame: x runs from the face centroid, y across it,
            n out. The solve fits the frame and reports the applied resultant; a face that
            curves away from its plane (fit RMS over 10% of extent) is refused — a swept
            skirt needs dir= instead of the frame.
          </div>
        </div>
      )}
    </div>
  );
}

function TemperatureInspector({ id }: { id: string }) {
  useApp();
  const t = store.doc.temperatures.find((x) => x.id === id);
  if (!t) return null;
  return (
    <div class="insp" data-inspector="temperature">
      <div class="insp-heading">
        <span class="kind">Temperature</span>
        <span class="row-name">{t.id}</span>
      </div>
      <NameField id={t.id} />
      <div class="field-grid">
        <label class="plain">Region</label>
        <RegionPicker
          value={t.region}
          testId="temperature-region"
          commit={(rid) =>
            store.transact('Set temperature region', (d) => void (temperatureIn(d, id).region = rid))
          }
        />
        <NumRow
          label="Fixed value"
          unit="°C"
          get={(d) => temperatureIn(d, id).celsius}
          set={(d, v) => {
            temperatureIn(d, id).celsius = v;
          }}
          undoLabel="Set temperature"
          testId="temperature-celsius"
          step={10}
          title="Fixed boundary temperature for the conduction solve; faces without one stay insulated"
        />
        <TextInputRow
          label="Case"
          value={t.case}
          commit={(v) => store.transact('Set temperature case', (d) => void (temperatureIn(d, id).case = v))}
          testId="temperature-case"
          title="Named thermal case; a case name cannot also be a load case"
        />
      </div>
      <div class="hint" style={{ gridColumn: '1 / -1' }}>
        The region's facet nodes take this value Dirichlet-style in the conduction solve and the
        whole part expands by α·(T − tref) from it; two temperatures sharing nodes inside one
        case must agree, and the reference is the solve statement's tref (20 °C when unstated).
      </div>
    </div>
  );
}

// ---- registration ---------------------------------------------------------------------

viewportExtensions.push({
  sync: syncGlyphs,
  highlights: regionHighlights,
  click: onClickRegion,
});

inspectors['region'] = (id) => <RegionInspector id={id} />;
inspectors['support'] = (id) => <SupportInspector id={id} />;
inspectors['load'] = (id) => <LoadInspector id={id} />;
inspectors['temperature'] = (id) => <TemperatureInspector id={id} />;

extraCommands.push(() => {
  if (store.lens !== 'setup') return [];
  return [
    {
      id: 'setup.add-support',
      group: 'Setup',
      label: 'Add support',
      enabled: () => !!selectedRegion(store.doc),
      run: () => addAnalysis('support'),
    },
    {
      id: 'setup.add-load',
      group: 'Setup',
      label: 'Add load',
      enabled: () => !!selectedRegion(store.doc),
      run: () => addAnalysis('load'),
    },
    {
      id: 'setup.add-temperature',
      group: 'Setup',
      label: 'Add temperature',
      enabled: () => !!selectedRegion(store.doc),
      run: () => addAnalysis('temperature'),
    },
  ];
});