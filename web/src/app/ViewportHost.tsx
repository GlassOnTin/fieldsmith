// Hosts the 3D viewport: keeps the field shader in sync with the document, drives highlights,
// picking, hover and the transform gizmo.
import { useEffect, useRef } from 'preact/hooks';
import * as THREE from 'three';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
import { Viewport } from '../view/viewport';
import type { FieldSpec, Highlight, MeshSlotSpec } from '../view/viewport';
import { store } from '../doc/store';
import { meshcache } from '../doc/meshcache';
import { bodyShader, shapeShader, pick, previewFillParamData, FACE_CODES } from '../doc/wasm';
import type { ShaderInfo } from '../doc/wasm';
import { findNode, walk } from '../doc/types';
import type { Document, Node, Vec3 } from '../doc/types';
import { effectiveLeafColors } from '../doc/ops';
import { parentMatrix, toTransform, worldMatrix, clean } from '../doc/xform';
import { ui } from './ui';
import { analysis } from './analysis';
import { viewApi } from './commands';

/** What an empty document frames: a 80 mm patch of the build plate. */
const EMPTY_VIEW: [Vec3, Vec3] = [
  [-40, -40, 0],
  [40, 40, 30],
];
const ACCENT: [number, number, number] = [0.2, 0.44, 0.86];
const HOVER: [number, number, number] = [0.55, 0.68, 0.9];

/** Leaf descendants (or the node itself) — what gets highlighted for a selection. */
function leavesOf(n: Node): string[] {
  const out: string[] = [];
  walk([n], (m) => {
    if (m.kind.type !== 'group') out.push(m.id);
  });
  return out;
}

export interface ViewportExtension {
  /** Called after each document change with the live viewport. */
  sync?(vp: Viewport, doc: Document): void;
  /** Extra highlights (e.g. regions in the Setup lens). */
  highlights?(leafIndex: (id: string) => number): Highlight[];
  /** Handle a click on the model; return true if consumed. */
  click?(hit: ReturnType<typeof pick>, e: PointerEvent): boolean;
}

/** Lens modules register extensions here. */
export const viewportExtensions: ViewportExtension[] = [];
export let liveViewport: Viewport | null = null;

export function ViewportHost() {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const vp = new Viewport(host.current!);
    liveViewport = vp;
    const hook = (window as unknown as { fieldsmith?: Record<string, unknown> }).fieldsmith;
    if (hook) hook.viewport = vp;
    let shader: ShaderInfo | null = null;
    /** Mesh hashes we already reported as not loaded — the hint repeats after the grid
     * returns (e.g. its .fsmz sidecar is opened), not on every sync before that. */
    const missingNotified = new Set<string>();
    /** Resolve a shader's mesh slots against the hydrated grid registry. A slot whose grid
     * is absent goes to `missing` rather than into the spec: an unbound sampler reads 0,
     * which every ray hits, not a vacuum. */
    const resolveMeshes = (sh: ShaderInfo): { slots: MeshSlotSpec[]; missing: string[] } => {
      const slots: MeshSlotSpec[] = [];
      const missing: string[] = [];
      for (const m of sh.meshes) {
        const g = meshcache.slotGrid(m.key_hex);
        if (g && g.dims[0] === m.dims[0] && g.dims[1] === m.dims[1] && g.dims[2] === m.dims[2]) {
          slots.push({ slot: m.slot, dist: g.dist, patch: g.patch, dims: m.dims });
          missingNotified.delete(m.key_hex);
        } else {
          missing.push(m.key_hex);
        }
      }
      return { slots, missing };
    };
    let lastDoc: Document | null = null;
    let fittedLoad = -1;
    let lastSel = '';
    let lastLens = store.lens;
    let pending = 0;
    /** The field spec the viewport is running (last `setField` launch wins), so the
     * analysis-arrival path can compare/swap the fill grid without touching `vp`'s privates. */
    let boundSpec: FieldSpec | null = null;
    /** What the bound param-grid payload holds (identity/contents), skip the swap when a
     * level re-reports the same values. */
    let appliedData: Float32Array | null = null;
    const sameData = (a: Float32Array | null, b: Float32Array): boolean => {
      if (a === null || a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
      return true;
    };

    const leafIndex = (id: string) => (shader ? shader.leaves.indexOf(id) : -1);

    const dark = window.matchMedia('(prefers-color-scheme: dark)');
    const applyTheme = () => {
      vp.setGridColor(dark.matches);
      vp.setBaseColor(dark.matches ? [0.62, 0.62, 0.6] : [0.74, 0.73, 0.7]);
    };
    dark.addEventListener('change', applyTheme);
    applyTheme();

    // ---- transform gizmo -------------------------------------------------------------
    const proxy = new THREE.Object3D();
    vp.scene.add(proxy);
    const tc = new TransformControls(vp.camera, vp.canvas);
    const helper = tc.getHelper();
    vp.scene.add(helper);
    tc.setSize(0.8);
    let gizmoNode: string | null = null;
    let dragging = false;
    tc.addEventListener('change', () => vp.invalidate());
    viewApi.gizmoAxis = () => tc.axis;
    tc.addEventListener('dragging-changed', (e) => {
      dragging = !!(e as unknown as { value: boolean }).value;
      vp.controls.enabled = !dragging;
      if (!dragging && gizmoNode) store.commitTransient(ui.gizmo === 'rotate' ? `Rotate ${gizmoNode}` : `Move ${gizmoNode}`);
    });
    tc.addEventListener('objectChange', () => {
      if (!gizmoNode || !dragging) return;
      const id = gizmoNode;
      proxy.updateMatrixWorld();
      const world = proxy.matrixWorld.clone();
      if (ui.snap && ui.gizmo === 'translate') {
        const p = new THREE.Vector3().setFromMatrixPosition(world);
        const snapped = featureSnap(store.doc, id, p.toArray() as Vec3);
        world.setPosition(...snapped);
      }
      const local = parentMatrix(store.doc, id).invert().multiply(world);
      const t = toTransform(local);
      store.transient((d) => {
        const n = findNode(d, id);
        if (n) n.transform = t;
      });
    });

    const placeGizmo = () => {
      const s = store.selection;
      const n = s && s.kind === 'node' && store.lens === 'shape' ? findNode(store.doc, s.id) : null;
      if (!n) {
        tc.detach();
        gizmoNode = null;
        helper.visible = false;
        vp.invalidate();
        return;
      }
      gizmoNode = n.id;
      if (!dragging) {
        const m = worldMatrix(store.doc, n);
        m.decompose(proxy.position, proxy.quaternion, proxy.scale);
        proxy.updateMatrixWorld();
      }
      tc.attach(proxy);
      helper.visible = true;
      tc.setMode(ui.gizmo);
      tc.setSpace(ui.gizmo === 'rotate' ? 'local' : 'world');
      tc.setTranslationSnap(ui.snap ? ui.gridStep : null);
      tc.setRotationSnap(ui.snap ? THREE.MathUtils.degToRad(ui.angleStep) : null);
      vp.invalidate();
    };

    // ---- document → shader ------------------------------------------------------------
    const updateHighlights = () => {
      const hl: Highlight[] = [];
      const s = store.selection;
      if (store.lens === 'shape') {
        if (ui.hover?.shape && (!s || s.kind !== 'node' || s.id !== ui.hover.shape)) {
          const i = leafIndex(ui.hover.shape);
          if (i >= 0) hl.push({ leaf: i, face: -1, color: HOVER });
        }
        if (s && s.kind === 'node') {
          const n = findNode(store.doc, s.id);
          if (n) for (const id of leavesOf(n)) {
            const i = leafIndex(id);
            if (i >= 0) hl.push({ leaf: i, face: -1, color: ACCENT });
          }
        }
      }
      for (const ext of viewportExtensions) if (ext.highlights) hl.push(...ext.highlights(leafIndex));
      vp.setHighlights(hl);
    };

    const updateGhost = () => {
      const s = store.selection;
      const n = s && s.kind === 'node' && store.lens === 'shape' ? findNode(store.doc, s.id) : null;
      if (n && (n.op !== 'add' || n.suppressed)) {
        const g = shapeShader(store.doc, n.id);
        const gm = resolveMeshes(g);
        // A mesh ghost whose grids are missing reads as a vacuum field anyway; hide it.
        vp.setGhost(gm.missing.length ? null : { source: g.source, bounds: g.bounds, meshes: gm.slots });
      } else vp.setGhost(null);
    };

    const sync = () => {
      pending = 0;
      const doc = store.doc;
      const selKey = JSON.stringify(store.selection);
      if (doc !== lastDoc) {
        const wasEmpty = !shader?.bounds;
        lastDoc = doc;
        shader = bodyShader(doc);
        const refit = store.loads !== fittedLoad || (wasEmpty && !!shader.bounds);
        fittedLoad = store.loads;
        // Grid resolution per document change: mesh leaves without loaded grids leave the
        // viewport empty and cannot be solved — the worker says the same at run time.
        const { slots, missing } = resolveMeshes(shader);
        for (const key of missing) {
          if (!missingNotified.has(key)) {
            missingNotified.add(key);
            ui.notify(
              `Mesh grid ${key} is not loaded — open the .fsmz sidecar (File → Open) to see and solve it`,
              'info',
              10000,
            );
          }
        }
        // Filled parts: the pre-analysis lattice preview rides `shader.grid`'s geometry;
        // its payload is re-derived per document change (uniform at the max density).
        const fillGrid =
          shader.grid && shader.source.includes('u_param')
            ? { ...shader.grid, data: previewFillParamData(doc) }
            : null;
        const spec = missing.length
          ? { source: shader.source, bounds: null, ident: '' }
          : { source: shader.source, bounds: shader.bounds, ident: shader.ident, meshes: slots, grid: fillGrid };
        boundSpec = spec;
        appliedData = fillGrid ? fillGrid.data : null;
        void vp.setField(spec).then(() => {
          ui.compileMs = vp.lastCompileMs;
          updateHighlights();
          if (refit) vp.fit(shader?.bounds ?? EMPTY_VIEW);
        });
        // Interior pass of a filled part: mounted whenever its part renders (the quad only
        // draws once the body goes sub-1-α), at a slower march step — the lattice field
        // over-estimates distances between grid nodes and would tunnel thin struts.
        vp.setCore(
          !missing.length && shader.core && shader.bounds
            ? {
                source: shader.source,
                bounds: shader.bounds,
                ident: shader.ident,
                meshes: slots,
                grid: fillGrid,
                stepScale: 0.35,
              }
            : null,
        );
        vp.setPartColors(effectiveLeafColors(doc), shader.leaves);
        updateGhost();
      } else if (selKey !== lastSel || store.lens !== lastLens) {
        updateGhost();
      }
      lastSel = selKey;
      lastLens = store.lens;
      updateHighlights();
      placeGizmo();
      // Extensions re-sync on every pass (not only on doc changes) so lens and selection
      // switches reach them too.
      for (const ext of viewportExtensions) ext.sync?.(vp, doc);
    };
    const schedule = () => {
      if (!pending) pending = requestAnimationFrame(sync);
    };
    const unsubStore = store.subscribe(schedule);
    let lastUi = { gizmo: ui.gizmo, snap: ui.snap, hover: ui.hover?.shape, alpha: ui.bodyAlpha };
    const unsubUi = ui.subscribe(() => {
      const now = { gizmo: ui.gizmo, snap: ui.snap, hover: ui.hover?.shape, alpha: ui.bodyAlpha };
      if (now.alpha !== lastUi.alpha) {
        // The opaque↔translucent regime flip rebuilds the material asynchronously — expose
        // its compile time like setField's callers do.
        void Promise.resolve(vp.setAlpha(now.alpha)).then(() => {
          ui.compileMs = vp.lastCompileMs;
        });
      }
      if (now.gizmo !== lastUi.gizmo || now.snap !== lastUi.snap) placeGizmo();
      if (now.hover !== lastUi.hover) updateHighlights();
      lastUi = now;
      // Data arrivals that bypass the store (analysis level completions bridge through ui)
      // still need a synced pass: extension overlays pick them up only via ext.sync.
      schedule();
    });
    // ---- analysis arrivals: the graded fill grid replaces the per-max preview -----------
    let lastArrival: unknown = null;
    const unsubAnalysis = analysis.subscribe(() => {
      const fp = analysis.latest?.fillParam;
      if (!fp || fp === lastArrival || !shader?.core || !boundSpec?.bounds) return;
      // Levels of a run launched for a superseded document edition never enter the shader.
      if (analysis.launchedFor !== store.docVersion) return;
      lastArrival = fp;
      const dims: [number, number, number] = [fp.dims[0], fp.dims[1], fp.dims[2]];
      let swapped = false;
      const g = boundSpec.grid;
      if (g && dims[0] === g.dims[0] && dims[1] === g.dims[1] && dims[2] === g.dims[2]) {
        if (sameData(appliedData, fp.data)) return;
        appliedData = fp.data;
        // Grid matching the compiled spec: texture payloads only, no recompile.
        swapped = vp.updateParamGrid(fp.data, dims);
      }
      if (!swapped) {
        // Dims differ (or the swap raced the compile): one full re-binds — after this the
        // grid matches the spec and later levels texture-swap again.
        const spec: FieldSpec = {
          ...boundSpec,
          grid: { origin: [fp.spec[0], fp.spec[1], fp.spec[2]], spacing: fp.spec[3], dims, data: fp.data },
        };
        boundSpec = spec;
        appliedData = fp.data;
        void vp.setField(spec).then(() => {
          ui.compileMs = vp.lastCompileMs;
        });
      }
    });
    sync();

    // ---- camera API for commands --------------------------------------------------------
    viewApi.fit = (animate = true) => {
      const b = shader?.bounds;
      if (b) vp.fit(b, animate);
    };
    viewApi.view = (dir, up) => vp.view(dir, up);
    viewApi.insertionPoint = () => {
      if (!shader?.bounds) return [0, 0, 0];
      const t = vp.controls.target;
      return [Math.round(t.x), Math.round(t.y), 0];
    };
    viewApi.project = (p) => {
      vp.camera.updateMatrixWorld();
      const v = new THREE.Vector3(p[0], p[1], p[2]).project(vp.camera);
      const r = vp.canvas.getBoundingClientRect();
      return { x: r.left + ((v.x + 1) / 2) * r.width, y: r.top + ((1 - v.y) / 2) * r.height };
    };

    // ---- picking & hover ------------------------------------------------------------------
    let down: { x: number; y: number; button: number } | null = null;
    const hitAt = (x: number, y: number) => {
      const r = vp.ray(x, y);
      return pick(store.doc, r.origin, r.dir);
    };
    const onDown = (e: PointerEvent) => {
      down = (tc as unknown as { axis: string | null }).axis
        ? null
        : { x: e.clientX, y: e.clientY, button: e.button };
    };
    const onUp = (e: PointerEvent) => {
      if (!down || e.button !== down.button) return;
      const moved = Math.hypot(e.clientX - down.x, e.clientY - down.y);
      down = null;
      if (moved > 4) return; // drag = orbit/pan/rotate, never a pick
      const hit = hitAt(e.clientX, e.clientY);
      if (e.button === 0) {
        for (const ext of viewportExtensions) if (ext.click?.(hit, e)) return;
        // Empty space deselects in every lens; a shape click selects only in the
        // shape lens, where the other lens extensions own the other clicks.
        if (hit?.shape) {
          if (store.lens === 'shape') store.select({ kind: 'node', id: hit.shape });
        } else store.select(null);
        return;
      }
      if (e.button === 2) store.select(null); // right-click deselects (drag pans)
    };
    let hoverPending = 0;
    let lastMove: PointerEvent | null = null;
    const onMove = (e: PointerEvent) => {
      lastMove = e;
      if (hoverPending || dragging || e.buttons) return;
      hoverPending = requestAnimationFrame(() => {
        hoverPending = 0;
        const m = lastMove!;
        const hit = hitAt(m.clientX, m.clientY);
        const next = hit ? { shape: hit.shape, face: hit.face, point: hit.point } : null;
        if (JSON.stringify(next?.shape) !== JSON.stringify(ui.hover?.shape) || JSON.stringify(next?.face) !== JSON.stringify(ui.hover?.face)) {
          ui.hover = next;
          ui.emit();
        } else ui.hover = next;
      });
    };
    const onLeave = () => {
      if (ui.hover) {
        ui.hover = null;
        ui.emit();
      }
    };
    vp.canvas.addEventListener('pointerdown', onDown);
    vp.canvas.addEventListener('pointerup', onUp);
    vp.canvas.addEventListener('pointermove', onMove);
    vp.canvas.addEventListener('pointerleave', onLeave);

    const fpsTimer = window.setInterval(() => {
      const ms = vp.frameTime();
      if (Math.abs(ms - ui.frameMs) > 0.5) ui.set('frameMs', ms);
    }, 1000);

    return () => {
      unsubStore();
      unsubUi();
      unsubAnalysis();
      clearInterval(fpsTimer);
      tc.dispose();
      vp.renderer.dispose();
      liveViewport = null;
    };
  }, []);

  return <div class="viewport-host" ref={host} style={{ position: 'absolute', inset: 0 }} />;
}

/** Per-axis snap of a dragged world position to other shapes' centres (within 1.5 mm). */
function featureSnap(doc: Document, id: string, p: Vec3): Vec3 {
  const cands: number[][] = [[0], [0], [0]];
  walk(doc.root, (n) => {
    if (n.id === id) return;
    const c = new THREE.Vector3().setFromMatrixPosition(worldMatrix(doc, n));
    cands[0].push(c.x);
    cands[1].push(c.y);
    cands[2].push(c.z);
  });
  return p.map((v, a) => {
    let best = v;
    let bd = 1.5;
    for (const c of cands[a]) {
      const d = Math.abs(c - v);
      if (d < bd) {
        bd = d;
        best = c;
      }
    }
    return clean(best);
  }) as Vec3;
}

export { FACE_CODES };
