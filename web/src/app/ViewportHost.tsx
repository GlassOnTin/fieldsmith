// Hosts the 3D viewport: keeps the field shader in sync with the document, drives highlights,
// picking, hover and the transform gizmo.
import { useEffect, useRef } from 'preact/hooks';
import * as THREE from 'three';
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js';
import { Viewport } from '../view/viewport';
import type { Highlight } from '../view/viewport';
import { store } from '../doc/store';
import { bodyShader, shapeShader, pick, FACE_CODES } from '../doc/wasm';
import type { ShaderInfo } from '../doc/wasm';
import { findNode, walk } from '../doc/types';
import type { Document, Node, Vec3 } from '../doc/types';
import { parentMatrix, toTransform, worldMatrix, clean } from '../doc/xform';
import { ui } from './ui';
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
    let lastDoc: Document | null = null;
    let fittedLoad = -1;
    let lastSel = '';
    let lastLens = store.lens;
    let pending = 0;

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
        vp.setGhost({ source: g.source, bounds: g.bounds });
      } else vp.setGhost(null);
    };

    const sync = () => {
      pending = 0;
      const doc = store.doc;
      const selKey = JSON.stringify(store.selection);
      if (doc !== lastDoc) {
        const refit = store.loads !== fittedLoad;
        fittedLoad = store.loads;
        lastDoc = doc;
        shader = bodyShader(doc);
        void vp.setField({ source: shader.source, bounds: shader.bounds, ident: shader.ident }).then(() => {
          ui.compileMs = vp.lastCompileMs;
          updateHighlights();
          if (refit) vp.fit(shader?.bounds ?? EMPTY_VIEW);
        });
        for (const ext of viewportExtensions) ext.sync?.(vp, doc);
        updateGhost();
      } else if (selKey !== lastSel || store.lens !== lastLens) {
        updateGhost();
      }
      lastSel = selKey;
      lastLens = store.lens;
      updateHighlights();
      placeGizmo();
    };
    const schedule = () => {
      if (!pending) pending = requestAnimationFrame(sync);
    };
    const unsubStore = store.subscribe(schedule);
    let lastUi = { gizmo: ui.gizmo, snap: ui.snap, hover: ui.hover?.shape };
    const unsubUi = ui.subscribe(() => {
      const now = { gizmo: ui.gizmo, snap: ui.snap, hover: ui.hover?.shape };
      if (now.gizmo !== lastUi.gizmo || now.snap !== lastUi.snap) placeGizmo();
      if (now.hover !== lastUi.hover) updateHighlights();
      lastUi = now;
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

    // ---- picking & hover ------------------------------------------------------------------
    let down: { x: number; y: number } | null = null;
    const hitAt = (x: number, y: number) => {
      const r = vp.ray(x, y);
      return pick(store.doc, r.origin, r.dir);
    };
    const onDown = (e: PointerEvent) => {
      down = (tc as unknown as { axis: string | null }).axis ? null : { x: e.clientX, y: e.clientY };
    };
    const onUp = (e: PointerEvent) => {
      if (!down || e.button !== 0) return;
      const moved = Math.hypot(e.clientX - down.x, e.clientY - down.y);
      down = null;
      if (moved > 4) return;
      const hit = hitAt(e.clientX, e.clientY);
      for (const ext of viewportExtensions) if (ext.click?.(hit, e)) return;
      if (store.lens !== 'shape') return;
      if (hit?.shape) store.select({ kind: 'node', id: hit.shape });
      else store.select(null);
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
