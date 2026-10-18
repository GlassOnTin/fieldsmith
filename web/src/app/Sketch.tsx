// Sketch: the closed profile of an Extrude node drawn in its local XY plane.
// The polygon drawn is the document's own data — the same arc maths as the field
// itself (fieldsmith-field prim.rs `arc_geom`): the centre sits on the left
// normal by (1−k²)/(2k)·l/2 and the arc mid on the right normal by k·l/2, so a
// positive bulge bows to the right of a→b. Inserts commit through
// store.transact and drags through transient/commitTransient — the same paths
// the numeric table uses, so a sketched edit and a typed edit replay identically.
import { useEffect, useRef, useState } from 'preact/hooks';
import { store } from '../doc/store';
import type { Document, Node } from '../doc/types';
import { useApp } from './ui';

/** Geometry the e2e pins: intrinsic canvas size, fitting pad and the vertex
 * grab radius (CSS px). Changing any of these moves sketch clicks in V31. */
export const SKETCH_W = 240;
export const SKETCH_H = 180;
export const SKETCH_PAD = 20;
export const SKETCH_GRAB = 9;
/** Arc samples per bulged segment when drawing and hit-testing. */
const ARC_SAMPLES = 32;

type Pt = { x: number; y: number; bulge: number };
type View = { s: number; ox: number; oy: number };

/** The profile flattened into a model-space polyline: each straight segment
 * contributes its end, each bulged segment its arc sampled at ARC_SAMPLES
 * points. The closed edge back to vertex 0 is included, so the last sample is
 * vertex 0 again. `ends[i]` is the polyline index of vertex i. */
function sampleProfile(pts: Pt[]): { line: [number, number][]; ends: number[] } {
  const line: [number, number][] = [[pts[0].x, pts[0].y]];
  const ends: number[] = [0];
  const n = pts.length;
  for (let i = 0; i < n; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % n];
    if (a.bulge === 0.0) {
      line.push([b.x, b.y]);
      ends.push(line.length - 1);
    } else {
      const cx = b.x - a.x;
      const cy = b.y - a.y;
      const l = Math.hypot(cx, cy);
      if (l < 1e-12) {
        line.push([b.x, b.y]);
        ends.push(line.length - 1);
        continue;
      }
      const d0 = cx / l;
      const d1 = cy / l;
      const mx = (a.x + b.x) / 2;
      const my = (a.y + b.y) / 2;
      // prim.rs arc_geom, verbatim.
      const h = 0.5 * l * (1.0 - a.bulge * a.bulge) / (2.0 * a.bulge);
      const ccx = mx + -d1 * h;
      const ccy = my + d0 * h;
      const r = 0.5 * l * (1.0 + a.bulge * a.bulge) / (2.0 * Math.abs(a.bulge));
      // The arc bows through mid = m − n̂·sag; pick the sweep towards that mid.
      // The centre, chord midpoint and arc mid are collinear, so mid is the
      // sweep's angular midpoint and the full sweep is 2·|Δθ| through it.
      const sag = a.bulge * l * 0.5;
      const tA = Math.atan2(a.y - ccy, a.x - ccx);
      let dT = Math.atan2(my - d0 * sag - ccy, mx + d1 * sag - ccx) - tA;
      if (dT > Math.PI) dT -= 2 * Math.PI;
      if (dT <= -Math.PI) dT += 2 * Math.PI;
      const sweep = Math.sign(dT) * 2.0 * Math.abs(dT);
      ends.push(line.length);
      for (let t = 1; t <= ARC_SAMPLES; t++) {
        if (t < ARC_SAMPLES) {
          const th = tA + sweep * (t / ARC_SAMPLES);
          line.push([ccx + r * Math.cos(th), ccy + r * Math.sin(th)]);
        } else {
          // Land the exact end: the last sample would only round to it.
          line.push([b.x, b.y]);
        }
      }
    }
  }
  return { line, ends };
}

function findExtrude(list: Node[], id: string): Node | null {
  for (const nd of list) {
    if (nd.id === id) return nd;
    if (nd.kind.type === 'group') {
      const f = findExtrude(nd.kind.children, id);
      if (f) return f;
    }
  }
  return null;
}

/** Deterministic fit (the e2e replicates this): sample the drawn polyline,
 * take the min-scale span fit with SKETCH_PAD on all sides, and centre the
 * spanned box. */
function sketchView(pts: Pt[]): { view: View; pxLine: [number, number][]; ends: number[] } {
  const { line, ends } = sampleProfile(pts);
  let minX = line[0][0];
  let maxX = line[0][0];
  let minY = line[0][1];
  let maxY = line[0][1];
  for (const [x, y] of line) {
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
  }
  const spanX = Math.max(maxX - minX, 1e-9);
  const spanY = Math.max(maxY - minY, 1e-9);
  const s = Math.min((SKETCH_W - 2 * SKETCH_PAD) / spanX, (SKETCH_H - 2 * SKETCH_PAD) / spanY);
  const ox = SKETCH_PAD + (SKETCH_W - 2 * SKETCH_PAD - spanX * s) / 2 - minX * s;
  const oy = SKETCH_H - SKETCH_PAD - (SKETCH_H - 2 * SKETCH_PAD - spanY * s) / 2 + minY * s;
  const view = { s, ox, oy };
  const pxLine = line.map(([x, y]) => [view.ox + x * view.s, view.oy - y * view.s] as [number, number]);
  return { view, pxLine, ends };
}

/** Nearest part: a vertex within SKETCH_GRAB px, else the edge index whose
 * sampled segment carries the closest point (insert after that vertex). */
function locate(
  pts: Pt[],
  pxLine: [number, number][],
  ends: number[],
  px: number,
  py: number,
): { kind: 'vertex'; i: number } | { kind: 'edge'; i: number } | null {
  let vi = -1;
  let vd = SKETCH_GRAB;
  for (let k = 0; k < pts.length; k++) {
    const [x, y] = pxLine[ends[k]];
    const d = Math.hypot(px - x, py - y);
    if (d <= vd) {
      vd = d;
      vi = k;
    }
  }
  if (vi >= 0) return { kind: 'vertex', i: vi };
  let eg = -1;
  let ed = Infinity;
  for (let k = 0; k < pts.length; k++) {
    const from = ends[k];
    const to = k + 1 < ends.length ? ends[k + 1] : pxLine.length;
    for (let j = from + 1; j <= to; j++) {
      const [x, y] = pxLine[j];
      const d = Math.hypot(px - x, py - y);
      if (d < ed) {
        ed = d;
        eg = k;
      }
    }
  }
  return eg >= 0 ? { kind: 'edge', i: eg } : null;
}

export function SketchCanvas({ id }: { id: string }) {
  useApp();
  const ref = useRef<HTMLCanvasElement | null>(null);
  const drag = useRef<number>(-1);
  /** View frozen at drag start, so the mapping the cursor reads does not zoom
   * under it while the vertex moves. */
  const dragView = useRef<View | null>(null);
  const [hot, setHot] = useState<{ kind: 'vertex'; i: number } | null>(null);

  const draw = () => {
    const canvas = ref.current;
    if (!canvas) return;
    const nd = findExtrude(store.doc.root, id);
    if (!nd || nd.kind.type !== 'extrude') return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== SKETCH_W * dpr || canvas.height !== SKETCH_H * dpr) {
      canvas.width = SKETCH_W * dpr;
      canvas.height = SKETCH_H * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, SKETCH_W, SKETCH_H);
    const css = getComputedStyle(document.documentElement);
    const accent = css.getPropertyValue('--accent').trim() || '#336fd1';
    const ink = css.getPropertyValue('--line-strong').trim() || '#cfcfca';
    const text = css.getPropertyValue('--text-3').trim() || '#8b8b85';

    const { view, pxLine, ends } = sketchView(nd.kind.profile);

    // Local axes through the origin.
    ctx.strokeStyle = ink;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, Math.round(view.oy) + 0.5);
    ctx.lineTo(SKETCH_W, Math.round(view.oy) + 0.5);
    ctx.moveTo(Math.round(view.ox) + 0.5, 0);
    ctx.lineTo(Math.round(view.ox) + 0.5, SKETCH_H);
    ctx.stroke();

    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    pxLine.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.stroke();

    for (let i = 0; i < nd.kind.profile.length; i++) {
      const [x, y] = pxLine[ends[i]];
      ctx.fillStyle = text;
      ctx.beginPath();
      ctx.arc(x, y, i === 0 ? 3.5 : 2.5, 0, Math.PI * 2);
      ctx.fill();
      if (hot?.i === i) {
        ctx.strokeStyle = accent;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(x, y, 6, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
  };
  // The component re-renders on every store/ui emit (useApp), so a dep-less
  // effect redraws exactly when the document or the hover state changed.
  useEffect(draw);

  const local = (e: PointerEvent) => {
    const r = (e.target as HTMLCanvasElement).getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as [number, number];
  };

  const down = (e: PointerEvent) => {
    const nd = findExtrude(store.doc.root, id);
    if (!nd || nd.kind.type !== 'extrude') return;
    const [px, py] = local(e);
    const { view, pxLine, ends } = sketchView(nd.kind.profile);
    const hit = locate(nd.kind.profile, pxLine, ends, px, py);
    if (!hit) return;
    if (hit.kind === 'vertex') {
      drag.current = hit.i;
      dragView.current = view;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      setHot(hit);
      e.preventDefault();
      return;
    }
    // Click insert: a new point at the clicked position, after the nearest edge.
    const mx = (px - view.ox) / view.s;
    const my = (view.oy - py) / view.s;
    const at = hit.i;
    store.transact('Add profile point', (d: Document) => {
      const ex = findExtrude(d.root, id);
      if (ex && ex.kind.type === 'extrude') ex.kind.profile.splice(at + 1, 0, { x: mx, y: my, bulge: 0 });
    });
  };

  const move = (e: PointerEvent) => {
    if (drag.current < 0) {
      if (e.buttons !== 0) return;
      const [px, py] = local(e);
      const nd = findExtrude(store.doc.root, id);
      if (!nd || nd.kind.type !== 'extrude') {
        setHot(null);
        return;
      }
      const { pxLine, ends } = sketchView(nd.kind.profile);
      const hit = locate(nd.kind.profile, pxLine, ends, px, py);
      setHot(hit?.kind === 'vertex' ? hit : null);
      return;
    }
    const [px, py] = local(e);
    const view = dragView.current;
    if (!view) return;
    const mx = (px - view.ox) / view.s;
    const my = (view.oy - py) / view.s;
    const i = drag.current;
    store.transient((d: Document) => {
      const ex = findExtrude(d.root, id);
      if (ex && ex.kind.type === 'extrude') {
        ex.kind.profile[i].x = mx;
        ex.kind.profile[i].y = my;
      }
    });
  };

  const up = () => {
    if (drag.current < 0) return;
    drag.current = -1;
    dragView.current = null;
    store.commitTransient('Move profile point');
  };

  const cancel = () => {
    if (drag.current < 0) return;
    drag.current = -1;
    dragView.current = null;
    store.cancelTransient();
  };

  return (
    <canvas
      ref={ref}
      style={{ width: SKETCH_W, height: SKETCH_H, cursor: 'crosshair', display: 'block' }}
      data-testid="profile-sketch"
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={cancel}
      onPointerLeave={() => setHot(null)}
    />
  );
}