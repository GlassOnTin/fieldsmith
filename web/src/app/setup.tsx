// Setup lens: regions, supports and loads on top of the shape model. Clicking a surface makes
// a region (with the region selected, further clicks join its selector list); supports and
// loads attach to regions and draw as glyphs where the analysed mesh actually supports them.
import { Fragment } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import * as THREE from 'three';
import { store } from '../doc/store';
import { FACE_LABEL, freshId } from '../doc/types';
import type { Document, Face, Load, Region, Support } from '../doc/types';
import { FACE_CODES, evalBody } from '../doc/wasm';
import type { LevelResult, PickHit } from '../doc/wasm';
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

/** Region id the current selection refers to (a region, or its support/load). */
function selectedRegion(doc: Document): string | null {
  const s = store.selection;
  if (!s) return null;
  if (s.kind === 'region') return s.id;
  try {
    if (s.kind === 'support') return supportIn(doc, s.id).region;
    if (s.kind === 'load') return loadIn(doc, s.id).region;
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
      // 'all' is face code 0 in the generated GLSL; the shader treats key.y < 0 as any face.
      const code = sl.face === 'all' ? -1 : FACE_CODES.indexOf(sl.face);
      out.push({ leaf: li, face: code, color });
    }
  }
  return out;
}

function onClickRegion(hit: PickHit | null, e: PointerEvent): boolean {
  if (store.lens !== 'setup' || e.button !== 0) return false;
  if (!hit) return false;
  if (!hit.shape || !hit.face) {
    ui.notify('This surface is not a shape face and cannot join a region.', 'error');
    return true;
  }
  const target = store.selection?.kind === 'region' ? store.selection.id : null;
  if (target) {
    if (!store.doc.regions.some((x) => x.id === target)) return false;
    if (!regionIn(store.doc, target).selectors.some((sl) => sl.shape === hit.shape && sl.face === hit.face)) {
      store.transact(`Add face to ${target}`, (d) => {
        regionIn(d, target).selectors.push({ shape: hit.shape!, face: hit.face! as Face });
      });
    }
  } else {
    let id = '';
    store.transact('Add region', (d) => {
      id = freshId(d, 'region');
      d.regions.push({ id, selectors: [{ shape: hit.shape!, face: hit.face! as Face }] });
    });
    store.select({ kind: 'region', id });
    ui.notify(`${id} — ${FACE_LABEL[hit.face as Face]} of ${hit.shape}. Click more surfaces to add faces.`, 'info', 4500);
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
}

/** Surface facets of one region, bucketed on a 2 mm grid and combined face-normal. */
function clusters(lvl: LevelResult, ri: number): Cluster[] {
  const sums = new Map<string, { x: number; y: number; z: number; nx: number; ny: number; nz: number; n: number }>();
  const f = lvl.surfaceFacets;
  const reg = lvl.surfaceRegion;
  const nodes = lvl.nodes;
  for (let i = 0; i < reg.length; i++) {
    if (reg[i] !== ri) continue;
    const a = f[6 * i], b = f[6 * i + 1], c = f[6 * i + 2];
    A.set(nodes[3 * a], nodes[3 * a + 1], nodes[3 * a + 2]);
    B.set(nodes[3 * b], nodes[3 * b + 1], nodes[3 * b + 2]);
    C.set(nodes[3 * c], nodes[3 * c + 1], nodes[3 * c + 2]);
    TMP.copy(A).add(B).add(C).multiplyScalar(1 / 3);
    E1.subVectors(B, A);
    E2.subVectors(C, A);
    E1.cross(E2);
    const key = `${Math.round(TMP.x / 2)},${Math.round(TMP.y / 2)},${Math.round(TMP.z / 2)}`;
    const acc = sums.get(key) ?? { x: 0, y: 0, z: 0, nx: 0, ny: 0, nz: 0, n: 0 };
    acc.x = TMP.x; acc.y = TMP.y; acc.z = TMP.z;
    acc.nx += E1.x; acc.ny += E1.y; acc.nz += E1.z;
    acc.n++;
    sums.set(key, acc);
  }
  const out: Cluster[] = [];
  for (const acc of sums.values()) {
    const pos = new THREE.Vector3(acc.x, acc.y, acc.z);
    const normal = new THREE.Vector3(acc.nx, acc.ny, acc.nz);
    if (normal.lengthSq() < 1e-9) normal.set(0, 0, 1);
    normal.normalize();
    out.push({ pos, normal });
  }
  return out;
}
const TMP = new THREE.Vector3();

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
    const discGeo = new THREE.PlaneGeometry(size, size);
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
      const mesh = new THREE.Mesh(discGeo, discMat);
      mesh.quaternion.setFromUnitVectors(UP, c.normal);
      mesh.position.copy(c.pos).addScaledVector(c.normal, 0.3);
      mesh.renderOrder = 2;
      glyphs.add(mesh);
      const anchor = c.pos.clone().addScaledVector(c.normal, 0.4);
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
      for (const c of cs) {
        const arrow = new THREE.ArrowHelper(
          dir,
          c.pos.clone().addScaledVector(dir, 0.2),
          len,
          LOAD_COLOR,
          Math.min(2.2, len * 0.3),
          Math.min(1.3, len * 0.15),
        );
        arrow.renderOrder = 3;
        glyphs.add(arrow);
      }
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
  const lvl = analysis.latest;
  const lfp = lvl ? `${lvl.nodes.length}:${lvl.surfaceRegion.length}:${analysis.state.resultsFor}` : 'none';
  const sig = `${store.lens}|${store.docVersion}|${lfp}`;
  if (sig === glyphSig) return;
  glyphSig = sig;
  clearGlyphs();
  if (store.lens !== 'setup' || !lvl) return;
  try {
    buildGlyphs(lvl);
  } catch (err) {
    clearGlyphs();
    console.error('glyph build failed', err);
  }
}

// Level arrivals don't touch the store, so bridge them into a UI re-render (and a sync).
analysis.subscribe(() => ui.emit());

// ---- add / select helpers -------------------------------------------------------------

function addAnalysis(kind: 'support' | 'load') {
  const rid = selectedRegion(store.doc);
  if (!rid) {
    ui.notify('Create a region first: click a surface in the model.', 'error');
    return;
  }
  let id = '';
  store.transact(kind === 'support' ? 'Add support' : 'Add load', (d) => {
    id = freshId(d, kind);
    if (kind === 'support') d.supports.push({ id, region: rid, fixed: [true, true, true] });
    else d.loads.push({ id, region: rid, case: 'main', kind: { type: 'force', vector: [0, 0, -100] } });
  });
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

function AnalysisRow(props: { kind: 'region' | 'support' | 'load'; id: string; swatch?: string; meta: string }) {
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
        <span class="glyph">{props.kind === 'support' ? '⛨' : '↓'}</span>
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
          meta={`${r.selectors.length} ${r.selectors.length === 1 ? 'face' : 'faces'}`}
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
        <AnalysisRow key={l.id} kind="load" id={l.id} meta={`${l.kind.type === 'force' ? 'force' : 'pressure'} · ${l.case}`} />
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
      if (d.fill) d.fill.protect = d.fill.protect.filter((p) => p !== regionId);
    }
  });
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
            <span class="plain">{FACE_LABEL[sl.face]}</span>
            <span class="sel-face">
              <span>{sl.shape}</span>
              <button class="mini icon" title="Remove this face" onClick={() => removeFace(id, i)}>
                <Icon.trash />
              </button>
            </span>
          </Fragment>
        ))}
      </div>
      <div class="hint">
        {r.selectors.length === 1
          ? 'Click surfaces to add faces. Removing the last face removes the region and its supports and loads.'
          : 'Click surfaces in the model to add faces.'}
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
          ? `${region.selectors.length} ${region.selectors.length === 1 ? 'face' : 'faces'} constrained where they touch.`
          : 'Region missing.'}
      </div>
    </div>
  );
}

function LoadInspector({ id }: { id: string }) {
  useApp();
  const l = store.doc.loads.find((x) => x.id === id);
  if (!l) return null;
  const setKind = (t: 'force' | 'pressure') => {
    if (t === l.kind.type) return;
    store.transact('Set load kind', (d) => {
      const load = loadIn(d, id);
      load.kind = t === 'force' ? { type: 'force', vector: [0, 0, -100] } : { type: 'pressure', mpa: 1 };
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
        <Seg<'force' | 'pressure'>
          value={l.kind.type}
          options={[
            { value: 'force', label: 'Force' },
            { value: 'pressure', label: 'Pressure' },
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
  ];
});