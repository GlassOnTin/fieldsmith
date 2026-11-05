// Interactive Parametric 2D Sketch & Surface-Feature Workbench (Stage 1 + Stage 2).
// Combines Shapr3D/Onshape single-key Smart Dimension ('D'), SolveSpace/FreeCAD exact
// Jacobian QR rank & DOF diagnostics (Green = 0 DOF, Blue = Under-constrained, Amber = Redundant,
// Red = Conflict), nullspace interactive point dragging, high-level shape macros, and
// surface-attached 3D feature compilation (`BossExtrude`, `PocketCut`, `RevolveBoss`, `RevolveCut`).

import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { store } from '../doc/store';
import type { Face, Node } from '../doc/types';
import {
  FIXINGS_CATALOGUE,
  FIXING_SIZES,
  insertFixingIntoDocument,
  type FixingKindId,
} from '../doc/fixings-catalog';
import { resolveHostSurfaceFrame } from '../doc/ops';
import {
  SKETCH_PRESETS,
  addBoltCircleToSketch,
  addCenterRectToSketch,
  addCircleToSketch,
  addPolygonToSketch,
  addSlotToSketch,
  applySketchFeatureToDoc,
  filletSketchCorner,
  solveParametricSketch,
  type ParametricSketch,
  type SketchFeatureOp,
  type SketchPlane,
  type SketchSolveReport,
  type SketchStatus,
} from '../doc/sketch-solver';
import { Icon } from './icons';
import { ui, useApp } from './ui';

let sketchModalOpen = false;
const modalListeners = new Set<() => void>();

export function openSketchWorkbenchModal() {
  sketchModalOpen = true;
  modalListeners.forEach((fn) => fn());
}

export function closeSketchWorkbenchModal() {
  sketchModalOpen = false;
  modalListeners.forEach((fn) => fn());
}

const STATUS_META: Record<
  SketchStatus,
  { badge: string; color: string; bg: string; border: string }
> = {
  fully_constrained: {
    badge: '✓ Fully Constrained (0 DOF)',
    color: '#16a34a',
    bg: 'rgba(22, 163, 74, 0.12)',
    border: 'rgba(22, 163, 74, 0.45)',
  },
  under_constrained: {
    badge: '↔ Under-Constrained (Free DOF)',
    color: '#2563eb',
    bg: 'rgba(37, 99, 235, 0.12)',
    border: 'rgba(37, 99, 235, 0.45)',
  },
  redundant_constraints: {
    badge: '⚠ Redundant Constraint(s)',
    color: '#d97706',
    bg: 'rgba(217, 119, 6, 0.14)',
    border: 'rgba(217, 119, 6, 0.48)',
  },
  over_constrained_conflict: {
    badge: '✕ Conflicting Constraints',
    color: '#dc2626',
    bg: 'rgba(220, 38, 38, 0.14)',
    border: 'rgba(220, 38, 38, 0.5)',
  },
};

const CANVAS_W = 540;
const CANVAS_H = 340;
const PAD = 38;

interface ViewTransform {
  s: number;
  ox: number;
  oy: number;
}

function computeCanvasView(sk: ParametricSketch): ViewTransform {
  let minX = -20;
  let maxX = 60;
  let minY = -20;
  let maxY = 50;
  if (sk.points.length > 0 || sk.circles.length > 0) {
    minX = Infinity;
    maxX = -Infinity;
    minY = Infinity;
    maxY = -Infinity;
    for (const p of sk.points) {
      minX = Math.min(minX, p.x);
      maxX = Math.max(maxX, p.x);
      minY = Math.min(minY, p.y);
      maxY = Math.max(maxY, p.y);
    }
    for (const c of sk.circles) {
      const cp = sk.points.find((p) => p.id === c.center);
      if (cp) {
        minX = Math.min(minX, cp.x - c.radius);
        maxX = Math.max(maxX, cp.x + c.radius);
        minY = Math.min(minY, cp.y - c.radius);
        maxY = Math.max(maxY, cp.y + c.radius);
      }
    }
  }
  const spanX = Math.max(maxX - minX, 20);
  const spanY = Math.max(maxY - minY, 20);
  const s = Math.min((CANVAS_W - 2 * PAD) / spanX, (CANVAS_H - 2 * PAD) / spanY);
  const ox = PAD + (CANVAS_W - 2 * PAD - spanX * s) / 2 - minX * s;
  const oy = CANVAS_H - PAD - (CANVAS_H - 2 * PAD - spanY * s) / 2 + minY * s;
  return { s, ox, oy };
}

export function SketchWorkbench({
  embedded = false,
  onClose,
}: {
  embedded?: boolean;
  onClose?: () => void;
}) {
  useApp();
  const [presetId, setPresetId] = useState<string>(SKETCH_PRESETS[0].id);
  const [sketch, setSketch] = useState<ParametricSketch>(() => SKETCH_PRESETS[0].build());
  const [featureId, setFeatureId] = useState<string>('sketch_boss1');
  const [planeMode, setPlaneMode] = useState<'xy' | 'xz' | 'yz' | 'on_face'>('xy');
  const [hostShapeId, setHostShapeId] = useState<string>(() => store.doc.root[0]?.id ?? 'base');
  const [hostFace, setHostFace] = useState<Face>('top');
  const [planeOffset] = useState<number>(0);
  const [opKind, setOpKind] = useState<SketchFeatureOp['kind']>('boss_extrude');
  const [extrudeHeight, setExtrudeHeight] = useState<number>(16);
  const [pocketDepth, setPocketDepth] = useState<number>(8);
  const [throughAll, setThroughAll] = useState<boolean>(false);
  const [revolveAngle, setRevolveAngle] = useState<number>(360);
  const [featureBlend, setFeatureBlend] = useState<number>(0);
  const [associateInTree, setAssociateInTree] = useState<boolean>(true);
  const [sketchFixingKind, setSketchFixingKind] = useState<FixingKindId>('self_tapping_screw');
  const [sketchFixingSize, setSketchFixingSize] = useState<string>('M4');

  const allHostShapes = useMemo(() => {
    const out: Node[] = [];
    const collect = (nodes: Node[]) => {
      for (const n of nodes) {
        if (n.suppressed || n.op !== 'add') continue;
        if (n.kind.type === 'group') collect(n.kind.children);
        else out.push(n);
      }
    };
    collect(store.doc.root);
    return out;
  }, [store.doc.root]);
  const activeHostFrame = useMemo(
    () =>
      planeMode === 'on_face'
        ? resolveHostSurfaceFrame(store.doc, hostShapeId, hostFace)
        : null,
    [planeMode, hostShapeId, hostFace, store.doc],
  );

  // Interactive selection & Smart Dimension ('D') state
  const [selectedPointIds, setSelectedPointIds] = useState<string[]>([]);
  const [selectedSegIds, setSelectedSegIds] = useState<string[]>(['s0']);
  const selectedSegId = selectedSegIds[0] ?? null;
  const setSelectedSegId = (id: string | null) => setSelectedSegIds(id ? [id] : []);
  const [selectedCircleIds, setSelectedCircleIds] = useState<string[]>([]);
  const selectedCircleId = selectedCircleIds[0] ?? null;
  const setSelectedCircleId = (id: string | null) => setSelectedCircleIds(id ? [id] : []);
  const [smartDimValue, setSmartDimValue] = useState<string>('60');
  const [smartDimExpr, setSmartDimExpr] = useState<string>('');
  const [filletRadius, setFilletRadius] = useState<number>(4);
  const [showMcpJson, setShowMcpJson] = useState<boolean>(false);

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const dragPointRef = useRef<string | null>(null);
  const dragViewRef = useRef<ViewTransform | null>(null);

  // Map document `lets` for expression evaluation
  const letsMap = useMemo(() => {
    const m: Record<string, number> = {};
    for (const l of store.doc.lets ?? []) {
      m[l.name] = l.value;
    }
    return m;
  }, [store.doc.lets]);

  const report: SketchSolveReport = useMemo(
    () => solveParametricSketch(sketch, letsMap),
    [sketch, letsMap],
  );

  const solved = report.solved_sketch;

  const loadPreset = (id: string) => {
    const found = SKETCH_PRESETS.find((p) => p.id === id);
    if (!found) return;
    setPresetId(found.id);
    const built = found.build();
    setSketch(built);
    setOpKind(found.defaultOp.kind);
    if (found.defaultOp.kind === 'boss_extrude') setExtrudeHeight(found.defaultOp.height);
    if (found.defaultOp.kind === 'revolve_boss') setRevolveAngle(found.defaultOp.angle_deg);
    setSelectedSegId(built.segments[0]?.id ?? null);
    setSelectedPointIds(built.points[0] ? [built.points[0].id] : []);
    setSelectedCircleId(built.circles[0]?.id ?? null);
  };

  // Draw the 2D parametric sketch canvas with color-coded DOF freedom and constraint badges
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== CANVAS_W * dpr || canvas.height !== CANVAS_H * dpr) {
      canvas.width = CANVAS_W * dpr;
      canvas.height = CANVAS_H * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, CANVAS_W, CANVAS_H);

    const view = dragViewRef.current ?? computeCanvasView(solved);
    const toPx = (x: number, y: number): [number, number] => [
      view.ox + x * view.s,
      view.oy - y * view.s,
    ];

    // 1. Subtle engineering millimeter grid
    ctx.strokeStyle = 'rgba(148, 163, 184, 0.14)';
    ctx.lineWidth = 1;
    const stepMm = view.s * 10 >= 28 ? 10 : 20;
    for (let gx = -200; gx <= 200; gx += stepMm) {
      const [px] = toPx(gx, 0);
      if (px >= 0 && px <= CANVAS_W) {
        ctx.beginPath();
        ctx.moveTo(Math.round(px) + 0.5, 0);
        ctx.lineTo(Math.round(px) + 0.5, CANVAS_H);
        ctx.stroke();
      }
    }
    for (let gy = -200; gy <= 200; gy += stepMm) {
      const [, py] = toPx(0, gy);
      if (py >= 0 && py <= CANVAS_H) {
        ctx.beginPath();
        ctx.moveTo(0, Math.round(py) + 0.5);
        ctx.lineTo(CANVAS_W, Math.round(py) + 0.5);
        ctx.stroke();
      }
    }

    // 2. Datum axes through (0, 0)
    const [ox, oy] = toPx(0, 0);
    ctx.strokeStyle = 'rgba(239, 68, 68, 0.45)';
    ctx.beginPath();
    ctx.moveTo(0, Math.round(oy) + 0.5);
    ctx.lineTo(CANVAS_W, Math.round(oy) + 0.5);
    ctx.stroke();
    ctx.strokeStyle = 'rgba(34, 197, 94, 0.45)';
    ctx.beginPath();
    ctx.moveTo(Math.round(ox) + 0.5, 0);
    ctx.lineTo(Math.round(ox) + 0.5, CANVAS_H);
    ctx.stroke();
    if (opKind === 'revolve_boss' || opKind === 'revolve_cut') {
      ctx.save();
      ctx.setLineDash([6, 4]);
      ctx.strokeStyle = '#f59e0b';
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.moveTo(Math.round(ox) + 0.5, 0);
      ctx.lineTo(Math.round(ox) + 0.5, CANVAS_H);
      ctx.stroke();
      ctx.fillStyle = '#f59e0b';
      ctx.font = '600 10px ui-monospace, monospace';
      ctx.fillText('↻ Z Revolve Axis (R = 0)', ox + 6, 14);
      ctx.restore();
    }

    // 2b. Host Face Reference Boundary when sketching directly on a 3D part face
    if (activeHostFrame) {
      const hw = 0.5 * activeHostFrame.faceWidth;
      const hh = 0.5 * activeHostFrame.faceHeight;
      const [x0, y0] = toPx(-hw, hh);
      const [x1, y1] = toPx(hw, -hh);
      ctx.save();
      ctx.setLineDash([4, 3]);
      ctx.strokeStyle = 'rgba(56, 189, 248, 0.65)';
      ctx.lineWidth = 1.4;
      ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
      ctx.fillStyle = '#38bdf8';
      ctx.font = '600 10px ui-monospace, monospace';
      ctx.fillText(
        `Face ${hostShapeId}.${hostFace} (${activeHostFrame.faceWidth.toFixed(1)}×${activeHostFrame.faceHeight.toFixed(1)} mm)`,
        Math.max(8, x0 + 4),
        Math.max(12, y0 - 4),
      );
      ctx.restore();
    }

    // 3. Shaded preview of solved outer_profile & hole_profiles
    if (report.outer_profile.length >= 2) {
      ctx.save();
      ctx.fillStyle =
        report.status === 'over_constrained_conflict'
          ? 'rgba(220, 38, 38, 0.09)'
          : report.status === 'fully_constrained'
            ? 'rgba(22, 163, 74, 0.10)'
            : 'rgba(37, 99, 235, 0.10)';
      ctx.beginPath();
      report.outer_profile.forEach((v, idx) => {
        const [px, py] = toPx(v.x, v.y);
        if (idx === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      });
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    }

    // 4. Draw segments (lines & circular arcs) + inline constraint glyphs (H, V, L=...)
    const ptMap = new Map(solved.points.map((p) => [p.id, p]));
    const statusStroke = STATUS_META[report.status].color;

    for (const seg of solved.segments) {
      const a = ptMap.get(seg.p1);
      const b = ptMap.get(seg.p2);
      if (!a || !b) continue;
      const [ax, ay] = toPx(a.x, a.y);
      const [bx, by] = toPx(b.x, b.y);
      const isSel = selectedSegIds.includes(seg.id);

      ctx.save();
      if (seg.construction) ctx.setLineDash([5, 4]);
      ctx.strokeStyle = isSel ? '#f59e0b' : seg.construction ? '#64748b' : statusStroke;
      ctx.lineWidth = isSel ? 3.0 : seg.construction ? 1.2 : 2.1;

      ctx.beginPath();
      const k = seg.bulge ?? 0;
      if (Math.abs(k) < 1e-6) {
        ctx.moveTo(ax, ay);
        ctx.lineTo(bx, by);
      } else {
        // Sample arc in model coordinates
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const l = Math.hypot(dx, dy);
        const d0 = dx / l;
        const d1 = dy / l;
        const mx = 0.5 * (a.x + b.x);
        const my = 0.5 * (a.y + b.y);
        const h = (0.5 * l * (1 - k * k)) / (2 * k);
        const ccx = mx - d1 * h;
        const ccy = my + d0 * h;
        const r = (0.5 * l * (1 + k * k)) / (2 * Math.abs(k));
        const sag = 0.5 * k * l;
        const tA = Math.atan2(a.y - ccy, a.x - ccx);
        let dT = Math.atan2(my - d0 * sag - ccy, mx + d1 * sag - ccx) - tA;
        if (dT > Math.PI) dT -= 2 * Math.PI;
        if (dT <= -Math.PI) dT += 2 * Math.PI;
        const sweep = Math.sign(dT) * 2 * Math.abs(dT);
        ctx.moveTo(ax, ay);
        for (let step = 1; step <= 24; step++) {
          const th = tA + sweep * (step / 24);
          const [sx, sy] = toPx(ccx + r * Math.cos(th), ccy + r * Math.sin(th));
          ctx.lineTo(sx, sy);
        }
      }
      ctx.stroke();
      ctx.restore();

      // Draw midpoint constraint glyph badges (H, V, Distance)
      const midX = 0.5 * (ax + bx);
      const midY = 0.5 * (ay + by);
      const segConstraints = solved.constraints.filter(
        (c) => ('segment' in c && c.segment === seg.id) || ('seg1' in c && c.seg1 === seg.id),
      );
      if (segConstraints.length > 0) {
        const labels = segConstraints
          .map((c) => {
            if (c.kind === 'horizontal') return 'H';
            if (c.kind === 'vertical') return 'V';
            if (c.kind === 'distance') return c.expr ? `${c.expr}=${c.value.toFixed(1)}` : `${c.value.toFixed(1)}`;
            if (c.kind === 'parallel') return '∥';
            if (c.kind === 'perpendicular') return '⊥';
            if (c.kind === 'equal_length') return '=';
            return c.kind;
          })
          .join(' · ');
        ctx.save();
        ctx.font = '600 10px ui-monospace, monospace';
        const tw = ctx.measureText(labels).width + 8;
        ctx.fillStyle = 'rgba(15, 23, 42, 0.82)';
        ctx.fillRect(midX - tw / 2, midY - 17, tw, 14);
        ctx.fillStyle = isSel ? '#fbbf24' : '#e2e8f0';
        ctx.fillText(labels, midX - tw / 2 + 4, midY - 7);
        ctx.restore();
      }
    }

    // 5. Draw circles
    for (const c of solved.circles) {
      const cp = ptMap.get(c.center);
      if (!cp) continue;
      const [cx, cy] = toPx(cp.x, cp.y);
      const rPx = c.radius * view.s;
      const isSel = selectedCircleIds.includes(c.id);
      ctx.save();
      if (c.construction) ctx.setLineDash([4, 4]);
      ctx.strokeStyle = isSel ? '#f59e0b' : c.construction ? '#64748b' : statusStroke;
      ctx.lineWidth = isSel ? 2.8 : c.construction ? 1.2 : 2.0;
      ctx.beginPath();
      ctx.arc(cx, cy, Math.max(2, rPx), 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();

      if (!c.construction) {
        ctx.save();
        ctx.font = '600 10px ui-monospace, monospace';
        ctx.fillStyle = '#e2e8f0';
        ctx.fillText(`⌀${(2 * c.radius).toFixed(1)}`, cx + rPx * 0.7 + 3, cy - rPx * 0.7);
        ctx.restore();
      }
    }

    // 6. Draw points color-coded by per-point DOF freedom (`0 DOF` = green, `>0 DOF` = blue, conflict = red)
    for (const pf of report.point_freedom) {
      const [px, py] = toPx(pf.x, pf.y);
      const isSel = selectedPointIds.includes(pf.id);
      const ptColor =
        pf.status === 'over_constrained_conflict'
          ? '#dc2626'
          : pf.dof === 0
            ? '#16a34a'
            : '#3b82f6';
      ctx.save();
      ctx.fillStyle = ptColor;
      ctx.beginPath();
      ctx.arc(px, py, isSel ? 5.5 : 4.2, 0, Math.PI * 2);
      ctx.fill();
      if (isSel) {
        ctx.strokeStyle = '#f59e0b';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(px, py, 8.5, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.font = '600 10px ui-monospace, monospace';
      ctx.fillStyle = isSel ? '#fbbf24' : '#94a3b8';
      ctx.fillText(pf.dof > 0 ? `${pf.id}(${pf.dof}DOF)` : pf.id, px + 6, py + 4);
      ctx.restore();
    }
  }, [solved, report, selectedPointIds, selectedSegIds, selectedCircleIds, opKind]);

  // Canvas pointer interaction: click to select point/segment/circle, or drag any point in nullspace!
  const onCanvasPointerDown = (e: PointerEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const scaleX = CANVAS_W / rect.width;
    const scaleY = CANVAS_H / rect.height;
    const px = (e.clientX - rect.left) * scaleX;
    const py = (e.clientY - rect.top) * scaleY;
    const view = computeCanvasView(solved);
    const toPx = (x: number, y: number): [number, number] => [
      view.ox + x * view.s,
      view.oy - y * view.s,
    ];

    // 1. Hit-test points first
    for (const p of solved.points) {
      const [vx, vy] = toPx(p.x, p.y);
      if (Math.hypot(px - vx, py - vy) <= 12) {
        dragPointRef.current = p.id;
        dragViewRef.current = view;
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        if (e.shiftKey) {
          setSelectedPointIds((prev) =>
            prev.includes(p.id) ? prev.filter((id) => id !== p.id) : [...prev.slice(-1), p.id],
          );
        } else {
          setSelectedPointIds([p.id]);
        }
        e.preventDefault();
        return;
      }
    }

    // 2. Hit-test circles
    const ptMap = new Map(solved.points.map((p) => [p.id, p]));
    for (const c of solved.circles) {
      const cp = ptMap.get(c.center);
      if (!cp) continue;
      const [cx, cy] = toPx(cp.x, cp.y);
      const rPx = c.radius * view.s;
      if (Math.abs(Math.hypot(px - cx, py - cy) - rPx) <= 10) {
        if (e.shiftKey) {
          setSelectedCircleIds((prev) =>
            prev.includes(c.id) ? prev.filter((id) => id !== c.id) : [...prev.slice(-1), c.id],
          );
        } else {
          setSelectedCircleIds([c.id]);
          setSelectedSegIds([]);
        }
        setSmartDimValue((2 * c.radius).toFixed(2));
        return;
      }
    }

    // 3. Hit-test segments
    for (const seg of solved.segments) {
      const a = ptMap.get(seg.p1);
      const b = ptMap.get(seg.p2);
      if (!a || !b) continue;
      const [ax, ay] = toPx(a.x, a.y);
      const [bx, by] = toPx(b.x, b.y);
      const ex = bx - ax;
      const ey = by - ay;
      const ee = ex * ex + ey * ey;
      const t = ee > 0 ? Math.max(0, Math.min(1, ((px - ax) * ex + (py - ay) * ey) / ee)) : 0;
      const d = Math.hypot(px - (ax + t * ex), py - (ay + t * ey));
      if (d <= 9) {
        if (e.shiftKey) {
          setSelectedSegIds((prev) =>
            prev.includes(seg.id) ? prev.filter((id) => id !== seg.id) : [...prev.slice(-1), seg.id],
          );
        } else {
          setSelectedSegIds([seg.id]);
          setSelectedCircleIds([]);
        }
        const len = Math.hypot(b.x - a.x, b.y - a.y);
        setSmartDimValue(len.toFixed(2));
        return;
      }
    }
  };

  const onCanvasPointerMove = (e: PointerEvent) => {
    const pid = dragPointRef.current;
    const view = dragViewRef.current;
    const canvas = canvasRef.current;
    if (!pid || !view || !canvas) return;
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) * (CANVAS_W / rect.width);
    const py = (e.clientY - rect.top) * (CANVAS_H / rect.height);
    const mx = Number(((px - view.ox) / view.s).toFixed(2));
    const my = Number(((view.oy - py) / view.s).toFixed(2));

    setSketch((prev) => {
      const next = structuredClone(prev);
      const pt = next.points.find((p) => p.id === pid);
      if (pt) {
        pt.x = mx;
        pt.y = my;
      }
      // If the point has an explicit `fixed` constraint, dragging it updates its anchor coordinates
      // so the user can interactively slide anchored points as well as under-constrained points!
      for (const c of next.constraints) {
        if (c.kind === 'fixed' && c.point === pid) {
          c.x = mx;
          c.y = my;
        }
      }
      return next;
    });
  };

  const onCanvasPointerUp = () => {
    if (!dragPointRef.current) return;
    dragPointRef.current = null;
    dragViewRef.current = null;
    // Persist solved coordinates back into sketch seed points
    setSketch(structuredClone(solved));
  };

  // Context-Sensitive & FreeCAD-Style Icon Constraint Actions
  const addQuickConstraint = (
    kind:
      | 'horizontal'
      | 'vertical'
      | 'fix_point'
      | 'coincident'
      | 'parallel'
      | 'perpendicular'
      | 'equal_length'
      | 'tangent'
      | 'concentric'
      | 'symmetric'
      | 'midpoint'
      | 'angle'
      | 'diameter'
      | 'smart_dim',
  ) => {
    setSketch(() => {
      const next = structuredClone(solved);
      const cid = `c_${kind}_${next.constraints.length + 1}`;
      const segA = selectedSegIds[0] ?? next.segments[0]?.id;
      const segB = selectedSegIds[1] ?? next.segments[1]?.id;
      const ptA = selectedPointIds[0] ?? next.points[0]?.id;
      const ptB = selectedPointIds[1] ?? next.points[1]?.id;
      const cirA = selectedCircleIds[0] ?? next.circles[0]?.id;
      const cirB = selectedCircleIds[1] ?? next.circles[1]?.id;

      if ((kind === 'horizontal' || kind === 'vertical') && segA) {
        next.constraints.push({ id: cid, kind, segment: segA });
      } else if (kind === 'fix_point' && ptA) {
        const pt = next.points.find((p) => p.id === ptA);
        if (pt) {
          next.constraints.push({ id: cid, kind: 'fixed', point: pt.id, x: pt.x, y: pt.y });
        }
      } else if (kind === 'coincident' && ptA && ptB && ptA !== ptB) {
        next.constraints.push({ id: cid, kind: 'coincident', p1: ptA, p2: ptB });
      } else if (kind === 'parallel' && segA && segB && segA !== segB) {
        next.constraints.push({ id: cid, kind: 'parallel', seg1: segA, seg2: segB });
      } else if (kind === 'perpendicular' && segA && segB && segA !== segB) {
        next.constraints.push({ id: cid, kind: 'perpendicular', seg1: segA, seg2: segB });
      } else if (kind === 'equal_length' && segA && segB && segA !== segB) {
        next.constraints.push({ id: cid, kind: 'equal_length', seg1: segA, seg2: segB });
      } else if (kind === 'tangent' && segA && cirA) {
        next.constraints.push({ id: cid, kind: 'tangent', seg: segA, target: cirA });
      } else if (kind === 'concentric' && cirA && cirB && cirA !== cirB) {
        next.constraints.push({ id: cid, kind: 'concentric', c1: cirA, c2: cirB });
      } else if (kind === 'symmetric' && ptA && ptB && segA) {
        next.constraints.push({ id: cid, kind: 'symmetric', p1: ptA, p2: ptB, axis: segA });
      } else if (kind === 'midpoint' && ptA && segA) {
        next.constraints.push({ id: cid, kind: 'midpoint', point: ptA, segment: segA });
      } else if (kind === 'angle' && segA) {
        const deg = Number(smartDimValue) || 45;
        next.constraints.push({
          id: cid,
          kind: 'angle',
          seg1: segA,
          seg2: segB && segB !== segA ? segB : undefined,
          degrees: deg,
          expr: smartDimExpr.trim() || undefined,
        });
      } else if (kind === 'diameter' && cirA) {
        const num = Number(smartDimValue) || 12;
        const expr = smartDimExpr.trim() || undefined;
        const existing = next.constraints.find(
          (c) => (c.kind === 'diameter' || c.kind === 'radius') && c.target === cirA,
        );
        if (existing && (existing.kind === 'diameter' || existing.kind === 'radius')) {
          existing.kind = 'diameter';
          existing.value = num;
          existing.expr = expr;
        } else {
          next.constraints.push({ id: `d_${cirA}`, kind: 'diameter', target: cirA, value: num, expr });
        }
      } else if (kind === 'smart_dim') {
        const num = Number(smartDimValue) || 20;
        const expr = smartDimExpr.trim() || undefined;
        if (selectedCircleId) {
          const existing = next.constraints.find(
            (c) => (c.kind === 'diameter' || c.kind === 'radius') && c.target === selectedCircleId,
          );
          if (existing && (existing.kind === 'diameter' || existing.kind === 'radius')) {
            existing.kind = 'diameter';
            existing.value = num;
            existing.expr = expr;
          } else {
            next.constraints.push({
              id: `d_${selectedCircleId}`,
              kind: 'diameter',
              target: selectedCircleId,
              value: num,
              expr,
            });
          }
        } else if (selectedSegId) {
          const existing = next.constraints.find(
            (c) => c.kind === 'distance' && c.segment === selectedSegId,
          );
          if (existing && existing.kind === 'distance') {
            existing.value = num;
            existing.expr = expr;
          } else {
            next.constraints.push({
              id: `d_${selectedSegId}`,
              kind: 'distance',
              segment: selectedSegId,
              value: num,
              expr,
            });
          }
        } else if (selectedPointIds.length === 2) {
          next.constraints.push({
            id: `d_${selectedPointIds[0]}_${selectedPointIds[1]}`,
            kind: 'distance',
            p1: selectedPointIds[0],
            p2: selectedPointIds[1],
            value: num,
            expr,
          });
        }
      }
      return next;
    });
  };

  const removeConstraint = (cid: string) => {
    setSketch(() => {
      const next = structuredClone(solved);
      next.constraints = next.constraints.filter((c) => c.id !== cid);
      return next;
    });
  };

  const injectConflictDemo = () => {
    setSketch(() => {
      const next = structuredClone(solved);
      const seg = next.segments[0]?.id ?? 's0';
      next.constraints.push({
        id: 'conflict_override_dim',
        kind: 'distance',
        segment: seg,
        value: 99.0,
      });
      return next;
    });
  };

  const applyTo3dDocument = () => {
    const plane: SketchPlane =
      planeMode === 'on_face'
        ? { kind: 'on_face', host_id: hostShapeId, face: hostFace, offset: planeOffset }
        : { kind: planeMode, offset: planeOffset };

    const op: SketchFeatureOp =
      opKind === 'boss_extrude'
        ? { kind: 'boss_extrude', height: extrudeHeight, blend: featureBlend }
        : opKind === 'pocket_cut'
          ? { kind: 'pocket_cut', depth: pocketDepth, through_all: throughAll, blend: featureBlend }
          : opKind === 'revolve_boss'
            ? { kind: 'revolve_boss', angle_deg: revolveAngle, blend: featureBlend }
            : { kind: 'revolve_cut', angle_deg: revolveAngle, blend: featureBlend };

    let createdIds: string[] = [];
    let planeDesc = '';
    store.transact(`Apply 2D Sketch Feature ${featureId}`, (d) => {
      const res = applySketchFeatureToDoc(
        d,
        featureId.trim() || 'sketch_feat1',
        report,
        plane,
        op,
        associateInTree,
      );
      createdIds = res.createdIds;
      planeDesc = res.planeDescription;
    });
    if (createdIds.length > 0) {
      store.select({ kind: 'node', id: createdIds[0] });
      ui.notify(
        `Applied ${op.kind} '${createdIds[0]}' (${createdIds.length} node${createdIds.length > 1 ? 's' : ''}) on ${planeDesc}`,
        'info',
      );
      if (onClose) onClose();
    }
  };

  const injectHostFaceEdgeRefs = () => {
    const sf = resolveHostSurfaceFrame(store.doc, hostShapeId, hostFace);
    const hw = Number((0.5 * sf.faceWidth).toFixed(3));
    const hh = Number((0.5 * sf.faceHeight).toFixed(3));
    setPlaneMode('on_face');
    setSketch(() => {
      const next = structuredClone(solved);
      const corners: Array<{ id: string; x: number; y: number }> = [
        { id: 'face_sw', x: -hw, y: -hh },
        { id: 'face_se', x: hw, y: -hh },
        { id: 'face_ne', x: hw, y: hh },
        { id: 'face_nw', x: -hw, y: hh },
      ];
      for (const c of corners) {
        const ex = next.points.find((p) => p.id === c.id);
        if (ex) {
          ex.x = c.x;
          ex.y = c.y;
          ex.fixed = true;
          ex.construction = true;
        } else {
          next.points.push({ id: c.id, x: c.x, y: c.y, fixed: true, construction: true });
        }
      }
      const edges: Array<[string, string, string]> = [
        ['face_edge_s', 'face_sw', 'face_se'],
        ['face_edge_e', 'face_se', 'face_ne'],
        ['face_edge_n', 'face_ne', 'face_nw'],
        ['face_edge_w', 'face_nw', 'face_sw'],
      ];
      for (const [sid, p1, p2] of edges) {
        if (!next.segments.some((s) => s.id === sid)) {
          next.segments.push({ id: sid, p1, p2, construction: true });
        }
      }
      // If there isn't a hole circle yet, add one constrained by dx=8, dy=6 from face_sw so the user can immediately edit it
      if (!next.circles.some((ci) => !ci.construction)) {
        const cid = `hole_${next.circles.length + 1}`;
        const pid = `${cid}_c`;
        const x0 = Number((-hw + Math.min(8, hw * 0.5)).toFixed(2));
        const y0 = Number((-hh + Math.min(6, hh * 0.5)).toFixed(2));
        next.points.push({ id: pid, x: x0, y: y0 });
        next.circles.push({ id: cid, center: pid, radius: 2.0 });
        next.constraints.push(
          {
            id: `${cid}_dx_edge`,
            kind: 'horizontal_distance',
            p1: 'face_sw',
            p2: pid,
            value: Number((x0 - -hw).toFixed(2)),
          },
          {
            id: `${cid}_dy_edge`,
            kind: 'vertical_distance',
            p1: 'face_sw',
            p2: pid,
            value: Number((y0 - -hh).toFixed(2)),
          },
          {
            id: `${cid}_dia`,
            kind: 'diameter',
            target: cid,
            value: 4.0,
          },
        );
      }
      return next;
    });
    ui.notify(
      `Added fixed reference corners (face_sw..face_nw) for ${hostShapeId}.${hostFace} (${sf.faceWidth.toFixed(1)}×${sf.faceHeight.toFixed(1)} mm)`,
      'info',
    );
  };

  const placeFixingsAtSketchHoles = () => {
    const ptMap = new Map(solved.points.map((p) => [p.id, p]));
    const holePoints: { u: number; v: number }[] = [];
    for (const ci of solved.circles) {
      if (ci.construction) continue;
      const cp = ptMap.get(ci.center);
      if (cp) holePoints.push({ u: Number(cp.x.toFixed(3)), v: Number(cp.y.toFixed(3)) });
    }
    if (holePoints.length === 0) {
      for (const p of solved.points) {
        if (!p.fixed && !p.construction) {
          holePoints.push({ u: Number(p.x.toFixed(3)), v: Number(p.y.toFixed(3)) });
        }
      }
    }
    if (holePoints.length === 0) {
      holePoints.push({ u: 0, v: 0 });
    }
    let summary = '';
    let firstId = '';
    store.transact(`Place ${sketchFixingKind} at ${holePoints.length} Sketch Hole(s)`, (d) => {
      const res = insertFixingIntoDocument(d, {
        kind: sketchFixingKind,
        size: sketchFixingSize,
        engagementMm: 8,
        hostId: hostShapeId,
        hostFace: planeMode === 'on_face' ? hostFace : 'top',
        placementMode: 'sketch_points',
        customPointsUV: holePoints,
        groupInTree: true,
        associateWithHostGroup: associateInTree,
        bindSurfaceLets: true,
        fastenerMaterial: 'steel_304',
        substrateMaterial: d.material ?? 'petg',
        vibrationG: 2.5,
        payloadMassKg: 0.4,
      });
      firstId = res.firstNewId;
      summary = `Placed ${holePoints.length}× ${sketchFixingSize} ${sketchFixingKind} (${res.createdGroupIds.join(', ')}) on ${hostShapeId}.${planeMode === 'on_face' ? hostFace : 'top'}`;
    });
    if (firstId) store.select({ kind: 'node', id: firstId });
    ui.notify(summary, 'info');
    if (onClose) onClose();
  };

  const statusStyle = STATUS_META[report.status];
  const mcpToolPayload = useMemo(
    () =>
      JSON.stringify(
        {
          tool: 'apply_sketch_feature',
          arguments: {
            feature_id: featureId,
            preset: presetId,
            on: planeMode === 'on_face' ? `${hostShapeId}.${hostFace}` : planeMode,
            operation:
              opKind === 'boss_extrude'
                ? { kind: 'boss_extrude', height: extrudeHeight, blend: featureBlend }
                : opKind === 'pocket_cut'
                  ? { kind: 'pocket_cut', depth: pocketDepth, through_all: throughAll, blend: featureBlend }
                  : { kind: opKind, angle_deg: revolveAngle, blend: featureBlend },
          },
        },
        null,
        2,
      ),
    [featureId, presetId, planeMode, hostShapeId, hostFace, opKind, extrudeHeight, pocketDepth, throughAll, revolveAngle, featureBlend],
  );

  return (
    <div
      data-testid="parametric-sketch-workbench"
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: '10px',
        padding: embedded ? '10px' : '14px',
        background: 'var(--panel)',
        border: '1px solid var(--line-strong)',
        borderRadius: '8px',
        color: 'var(--text)',
      }}
    >
      {/* Header Bar + Live DOF Status Pill */}
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: '8px',
          borderBottom: '1px solid var(--line)',
          paddingBottom: '8px',
        }}
      >
        <div>
          <div style={{ fontSize: '13px', fontWeight: 700 }}>
            ✏️ Parametric 2D Sketcher, Jacobian QR Constraint Solver & Surface-Feature Engine
          </div>
          <div style={{ fontSize: '11px', color: 'var(--text-2)' }}>
            Damped Levenberg–Marquardt + Homotopy Continuation · Column-Pivoted QR Rank (`DOF = N_vars − rank(J)`) · MCP `solve_sketch` / `apply_sketch_feature`
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
          <span
            data-testid="sketch-dof-status-badge"
            style={{
              fontSize: '11px',
              fontWeight: 700,
              padding: '3px 9px',
              borderRadius: '999px',
              color: statusStyle.color,
              background: statusStyle.bg,
              border: `1px solid ${statusStyle.border}`,
            }}
          >
            {statusStyle.badge} · DOF={report.dof} · rank={report.jacobian_rank}/{report.total_variables}
          </span>
          {onClose && (
            <button type="button" class="btn ghost small" onClick={onClose} data-testid="close-sketch-modal">
              ✕ Close
            </button>
          )}
        </div>
      </div>

      {/* FreeCAD-Style Visual Icon Toolbars: 1) Sketch Geometry Palette  2) Constraint Palette */}
      <div
        data-testid="sketch-icon-toolbars"
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: '6px',
          background: 'var(--panel-2)',
          padding: '8px 10px',
          borderRadius: '6px',
          border: '1px solid var(--line-strong)',
        }}
      >
        {/* Row 1: Preset Selector + 2D Sketch Geometry Icon Dock (Cyan-Blue CAD Palette) */}
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '8px', alignItems: 'center', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
            <label style={{ fontSize: '11px', fontWeight: 700, color: 'var(--text-2)' }}>Template:</label>
            <select
              class="select"
              data-testid="sketch-preset-select"
              value={presetId}
              onChange={(e) => loadPreset((e.target as HTMLSelectElement).value)}
              style={{ fontSize: '11px', maxWidth: '250px' }}
            >
              {SKETCH_PRESETS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.title}
                </option>
              ))}
            </select>
          </div>

          <div
            data-testid="sketch-geometry-icon-bar"
            style={{ display: 'flex', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}
          >
            <span style={{ fontSize: '10px', fontWeight: 700, color: '#38bdf8', textTransform: 'uppercase', letterSpacing: '0.04em', marginRight: '2px' }}>
              Geometry:
            </span>
            {(
              [
                {
                  id: 'macro-add-line',
                  label: 'Line',
                  title: 'Add a 2D line segment (L)',
                  icon: Icon.geomLine,
                  run: () =>
                    setSketch(() => {
                      const next = structuredClone(solved);
                      const idx = next.points.length + 1;
                      const p1 = `p${idx}`;
                      const p2 = `p${idx + 1}`;
                      next.points.push({ id: p1, x: -12, y: 12 }, { id: p2, x: 18, y: 12 });
                      next.segments.push({ id: `s_${idx}`, p1, p2 });
                      return next;
                    }),
                },
                {
                  id: 'macro-add-rect',
                  label: 'Center Rect',
                  title: 'Add a symmetric Center Rectangle macro (R)',
                  icon: Icon.geomRect,
                  run: () =>
                    setSketch(() => {
                      const next = structuredClone(solved);
                      addCenterRectToSketch(next, `rect${next.segments.length + 1}`, 0, 0, 48, 30, 0, false);
                      return next;
                    }),
                },
                {
                  id: 'macro-add-hole',
                  label: 'Circle / Hole',
                  title: 'Add a parametric Circle / Interior Hole (C)',
                  icon: Icon.geomCircle,
                  run: () =>
                    setSketch(() => {
                      const next = structuredClone(solved);
                      addCircleToSketch(next, `hole_${next.circles.length + 1}`, 20, 15, 3.2, false);
                      return next;
                    }),
                },
                {
                  id: 'macro-add-slot',
                  label: 'Obround Slot',
                  title: 'Add a parametric Obround Adjustment Slot macro (S)',
                  icon: Icon.geomSlot,
                  run: () =>
                    setSketch(() => {
                      const next = structuredClone(solved);
                      addSlotToSketch(next, `slot${next.segments.length + 1}`, -10, 0, 10, 0, 4.5);
                      return next;
                    }),
                },
                {
                  id: 'macro-add-bolt-circle',
                  label: 'Bolt Circle',
                  title: 'Add a Bolt-Hole Circle pattern (construction PCD + N equal holes)',
                  icon: Icon.geomBoltCircle,
                  run: () =>
                    setSketch(() => {
                      const next = structuredClone(solved);
                      addBoltCircleToSketch(next, `bc${next.circles.length + 1}`, 0, 0, 36, 4, 4.2, 0);
                      return next;
                    }),
                },
                {
                  id: 'macro-add-hex',
                  label: 'Hex Polygon',
                  title: 'Build a regular 6-sided Hex Nut profile + center bore',
                  icon: Icon.geomPolygon,
                  run: () => {
                    const sk: ParametricSketch = {
                      id: 'hex_profile',
                      plane: sketch.plane,
                      points: [],
                      segments: [],
                      circles: [],
                      constraints: [],
                    };
                    addPolygonToSketch(sk, 'hex', 0, 0, 22, 6, 0, false);
                    addCircleToSketch(sk, 'bore', 0, 0, 8, false);
                    setSketch(sk);
                  },
                },
              ] as const
            ).map((btn) => (
              <button
                key={btn.id}
                type="button"
                class="btn small"
                data-testid={btn.id}
                title={btn.title}
                onClick={btn.run}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '5px',
                  padding: '4px 8px',
                  fontSize: '10.5px',
                  fontWeight: 600,
                  background: 'rgba(56, 189, 248, 0.09)',
                  borderColor: 'rgba(56, 189, 248, 0.35)',
                  color: 'var(--text)',
                }}
              >
                <span style={{ width: '15px', height: '15px', display: 'inline-flex', color: '#38bdf8' }}>
                  <btn.icon />
                </span>
                <span>{btn.label}</span>
              </button>
            ))}

            {/* Modify Sub-Group: Parametric Corner Fillet with Editable Radius */}
            <div
              data-testid="sketch-modify-group"
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                paddingLeft: '6px',
                borderLeft: '1px solid var(--line-strong)',
              }}
            >
              <span style={{ fontSize: '10px', fontWeight: 700, color: '#a78bfa', textTransform: 'uppercase' }}>
                Modify:
              </span>
              <button
                type="button"
                class="btn small"
                data-testid="macro-fillet-corner"
                title={`Replace selected corner vertex with a tangent circular arc of radius R = ${filletRadius} mm`}
                onClick={() => {
                  const targetPt = selectedPointIds[0] || solved.points[0]?.id;
                  if (!targetPt) return;
                  setSketch(() => {
                    const next = structuredClone(solved);
                    filletSketchCorner(next, targetPt, filletRadius);
                    return next;
                  });
                }}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: '4px',
                  padding: '4px 7px',
                  fontSize: '10.5px',
                  fontWeight: 600,
                  background: 'rgba(167, 139, 250, 0.11)',
                  borderColor: 'rgba(167, 139, 250, 0.4)',
                  color: 'var(--text)',
                }}
              >
                <span style={{ width: '15px', height: '15px', display: 'inline-flex', color: '#a78bfa' }}>
                  <Icon.geomFillet />
                </span>
                <span>Fillet Corner</span>
              </button>
              <input
                type="number"
                class="text-input"
                data-testid="sketch-fillet-radius-input"
                title="Corner fillet radius R (mm)"
                value={filletRadius}
                min={0.5}
                max={50}
                step={0.5}
                onInput={(e) => setFilletRadius(Math.max(0.5, Number((e.target as HTMLInputElement).value) || 4))}
                style={{ width: '48px', fontSize: '10.5px', padding: '2px 4px' }}
              />
              <span style={{ fontSize: '10px', color: 'var(--text-2)' }}>mm</span>
            </div>
          </div>
        </div>

        {/* Row 2: FreeCAD-Style Visual Constraint Icon Toolbar (Crimson Geometric + Amber Dimensional) */}
        <div
          data-testid="sketch-constraint-icon-bar"
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            alignItems: 'center',
            gap: '4px',
            paddingTop: '6px',
            borderTop: '1px solid var(--line)',
          }}
        >
          <span style={{ fontSize: '10px', fontWeight: 700, color: '#f87171', textTransform: 'uppercase', letterSpacing: '0.04em', marginRight: '2px' }}>
            Constraints:
          </span>
          {(
            [
              { id: 'btn-constraint-coincident', kind: 'coincident', label: 'Coincident', key: 'C', icon: Icon.conCoincident, dim: false, tip: 'Merge two points (Shift+Click two points, then click Coincident)' },
              { id: 'btn-constraint-fix', kind: 'fix_point', label: 'Fix Point', key: 'X', icon: Icon.conFixed, dim: false, tip: 'Ground/Fix selected point coordinates (x, y) in sketch plane (FreeCAD Sketcher::Fix)' },
              { id: 'btn-constraint-horizontal', kind: 'horizontal', label: 'Horiz', key: 'H', icon: Icon.conHorizontal, dim: false, tip: 'Constrain selected line segment Horizontal (y₁ = y₂)' },
              { id: 'btn-constraint-vertical', kind: 'vertical', label: 'Vert', key: 'V', icon: Icon.conVertical, dim: false, tip: 'Constrain selected line segment Vertical (x₁ = x₂)' },
              { id: 'btn-constraint-parallel', kind: 'parallel', label: 'Parallel', key: '∥', icon: Icon.conParallel, dim: false, tip: 'Constrain two segments Parallel (Shift+Click two segments)' },
              { id: 'btn-constraint-perpendicular', kind: 'perpendicular', label: 'Perp', key: '⊥', icon: Icon.conPerpendicular, dim: false, tip: 'Constrain two segments Perpendicular 90° (Shift+Click two segments)' },
              { id: 'btn-constraint-equal', kind: 'equal_length', label: 'Equal', key: '=', icon: Icon.conEqual, dim: false, tip: 'Constrain two segments or circles to Equal Length / Radius' },
              { id: 'btn-constraint-tangent', kind: 'tangent', label: 'Tangent', key: 'T', icon: Icon.conTangent, dim: false, tip: 'Constrain line segment Tangent to circle/arc' },
              { id: 'btn-constraint-concentric', kind: 'concentric', label: 'Concentric', key: '◎', icon: Icon.conConcentric, dim: false, tip: 'Constrain two circles/arcs Concentric' },
              { id: 'btn-constraint-symmetric', kind: 'symmetric', label: 'Symm', key: '><', icon: Icon.conSymmetric, dim: false, tip: 'Constrain two points Symmetric across an axis segment' },
              { id: 'btn-constraint-midpoint', kind: 'midpoint', label: 'Midpoint', key: '•', icon: Icon.conMidpoint, dim: false, tip: 'Constrain point to the Midpoint of a segment' },
              { id: 'btn-constraint-distance', kind: 'smart_dim', label: 'Distance', key: 'D', icon: Icon.conDistance, dim: true, tip: 'Apply Smart Distance dimension (mm or let expression)' },
              { id: 'btn-constraint-diameter', kind: 'diameter', label: 'Diameter', key: '⌀', icon: Icon.conDiameter, dim: true, tip: 'Constrain circle/hole Diameter ⌀ (mm or let expression)' },
              { id: 'btn-constraint-angle', kind: 'angle', label: 'Angle', key: '∠', icon: Icon.conAngle, dim: true, tip: 'Constrain Segment Angle (°)' },
            ] as const
          ).map((cbtn) => (
            <button
              key={cbtn.id}
              type="button"
              class="btn small"
              data-testid={cbtn.id}
              title={`${cbtn.tip} [${cbtn.key}]`}
              onClick={() => addQuickConstraint(cbtn.kind)}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                padding: '3px 7px',
                fontSize: '10.5px',
                fontWeight: 600,
                background: cbtn.dim ? 'rgba(245, 158, 11, 0.11)' : 'rgba(239, 68, 68, 0.10)',
                borderColor: cbtn.dim ? 'rgba(245, 158, 11, 0.42)' : 'rgba(239, 68, 68, 0.35)',
                color: 'var(--text)',
              }}
            >
              <span
                style={{
                  width: '14px',
                  height: '14px',
                  display: 'inline-flex',
                  color: cbtn.dim ? '#fbbf24' : '#f87171',
                }}
              >
                <cbtn.icon />
              </span>
              <span>{cbtn.label}</span>
            </button>
          ))}

          <button
            type="button"
            class="btn ghost small"
            data-testid="sketch-inject-conflict"
            onClick={injectConflictDemo}
            style={{ fontSize: '10.5px', padding: '3px 7px', color: '#dc2626', marginLeft: 'auto', fontWeight: 700 }}
            title=" Solver runs continuously at 60 Hz; click this demo button to inject a contradictory 99 mm dimension on s0 and watch live RREF conflict isolation"
          >
            ⚡ Demo Conflict (+99 mm)
          </button>
        </div>
      </div>

      {/* Main Split: Left = 2D Interactive Canvas + Context Constraint Pill; Right = Constraints & 3D Surface Feature */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
          gap: '12px',
          alignItems: 'start',
        }}
      >
        {/* Left Column: 2D Canvas + Context-Sensitive Smart Dimension Pill */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
          <div
            style={{
              background: '#0f172a',
              borderRadius: '7px',
              border: `1.5px solid ${statusStyle.border}`,
              overflow: 'hidden',
              position: 'relative',
            }}
          >
            <canvas
              ref={canvasRef}
              data-testid="parametric-sketch-canvas"
              style={{ width: '100%', height: 'auto', aspectRatio: `${CANVAS_W} / ${CANVAS_H}`, display: 'block', cursor: 'crosshair', touchAction: 'none' }}
              onPointerDown={onCanvasPointerDown}
              onPointerMove={onCanvasPointerMove}
              onPointerUp={onCanvasPointerUp}
              onPointerCancel={onCanvasPointerUp}
            />
          </div>

          {/* Context-Sensitive Smart Dimension ('D') & Selection Bar */}
          <div
            data-testid="sketch-context-pill"
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: '6px',
              padding: '7px 9px',
              borderRadius: '6px',
              background: 'var(--panel-2)',
              border: '1px solid var(--line-strong)',
              fontSize: '11px',
            }}
          >
            <span style={{ fontWeight: 700, color: 'var(--accent)' }}>
              Selected:{' '}
              {selectedCircleIds.length > 0
                ? `Circle ${selectedCircleIds.join(', ')}`
                : selectedSegIds.length > 0
                  ? `Segment ${selectedSegIds.join(', ')}`
                  : selectedPointIds.length > 0
                    ? `Point ${selectedPointIds.join(', ')}`
                    : 'None (Click or Shift+Click canvas)'}
            </span>

            <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
              <span style={{ width: '14px', height: '14px', display: 'inline-flex', color: '#fbbf24' }}>
                <Icon.conDistance />
              </span>
              <span style={{ fontSize: '10.5px', color: 'var(--text-2)', fontWeight: 600 }}>Smart Dim (D):</span>
              <input
                type="number"
                class="text-input"
                data-testid="smart-dim-value-input"
                value={smartDimValue}
                step={1}
                onInput={(e) => setSmartDimValue((e.target as HTMLInputElement).value)}
                style={{ width: '62px', fontSize: '11px', padding: '2px 5px' }}
              />
              <input
                type="text"
                class="text-input"
                data-testid="smart-dim-expr-input"
                placeholder="or let expr (e.g. base_w)"
                value={smartDimExpr}
                onInput={(e) => setSmartDimExpr((e.target as HTMLInputElement).value)}
                style={{ width: '125px', fontSize: '11px', padding: '2px 5px' }}
              />
              <button
                type="button"
                class="btn primary small"
                data-testid="btn-apply-smart-dim"
                onClick={() => addQuickConstraint('smart_dim')}
                style={{ fontSize: '10.5px', padding: '2px 8px' }}
              >
                Set Dim
              </button>
            </div>
          </div>

          {/* Solver Diagnosis Callout */}
          <div
            data-testid="sketch-solver-diagnosis"
            style={{
              fontSize: '11px',
              lineHeight: 1.4,
              padding: '7px 9px',
              borderRadius: '6px',
              background: statusStyle.bg,
              border: `1px solid ${statusStyle.border}`,
              color: 'var(--text)',
              fontFamily: 'var(--mono)',
            }}
          >
            {report.diagnosis}
            <div style={{ marginTop: '4px', fontSize: '10.5px', color: 'var(--text-2)' }}>
              Loops compiled: {report.outer_profile.length > 0 ? `1 outer (${report.outer_profile.length} verts)` : '0 outer'} · {report.hole_profiles.length} interior hole(s) · LM iters={report.iterations} · ‖r‖∞={report.max_residual.toExponential(2)} mm
            </div>
          </div>
        </div>

        {/* Right Column: Active Constraints Table + Stage 2 Surface-Attached 3D Feature Builder */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
          {/* Active Constraints List */}
          <div
            style={{
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              borderRadius: '6px',
              padding: '8px 10px',
              maxHeight: '175px',
              overflowY: 'auto',
            }}
          >
            <div style={{ fontSize: '11px', fontWeight: 700, marginBottom: '5px', display: 'flex', justifyContent: 'space-between' }}>
              <span>Active Geometric & Dimensional Constraints ({solved.constraints.length})</span>
              <span style={{ fontSize: '10px', color: 'var(--text-2)' }}>Click ✕ to relax/remove</span>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
              {solved.constraints.map((c) => {
                const isConflict = report.conflicting_constraints.includes(c.id);
                const isRedundant = report.redundant_constraints.includes(c.id);
                const isDim =
                  c.kind === 'distance' ||
                  c.kind === 'horizontal_distance' ||
                  c.kind === 'vertical_distance' ||
                  c.kind === 'radius' ||
                  c.kind === 'diameter' ||
                  c.kind === 'angle';
                const rowIcon =
                  c.kind === 'horizontal'
                    ? Icon.conHorizontal
                    : c.kind === 'vertical'
                      ? Icon.conVertical
                      : c.kind === 'fixed'
                        ? Icon.conFixed
                        : c.kind === 'coincident'
                          ? Icon.conCoincident
                          : c.kind === 'parallel'
                            ? Icon.conParallel
                            : c.kind === 'perpendicular'
                              ? Icon.conPerpendicular
                              : c.kind === 'equal_length'
                                ? Icon.conEqual
                                : c.kind === 'tangent'
                                  ? Icon.conTangent
                                  : c.kind === 'concentric'
                                    ? Icon.conConcentric
                                    : c.kind === 'symmetric'
                                      ? Icon.conSymmetric
                                      : c.kind === 'midpoint'
                                        ? Icon.conMidpoint
                                        : c.kind === 'diameter' || c.kind === 'radius'
                                          ? Icon.conDiameter
                                          : c.kind === 'angle'
                                            ? Icon.conAngle
                                            : Icon.conDistance;
                return (
                  <div
                    key={c.id}
                    data-testid={`constraint-row-${c.id}`}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      gap: '6px',
                      fontSize: '10.5px',
                      padding: '2px 6px',
                      borderRadius: '4px',
                      background: isConflict
                        ? 'rgba(220, 38, 38, 0.16)'
                        : isRedundant
                          ? 'rgba(217, 119, 6, 0.15)'
                          : 'var(--panel)',
                      border: `1px solid ${isConflict ? '#dc2626' : isRedundant ? '#d97706' : 'var(--line)'}`,
                    }}
                  >
                    <span style={{ fontFamily: 'var(--mono)', display: 'inline-flex', alignItems: 'center', gap: '5px' }}>
                      <span style={{ width: '13px', height: '13px', display: 'inline-flex', color: isDim ? '#fbbf24' : '#f87171' }}>
                        {rowIcon()}
                      </span>
                      <b>{c.id}</b>: {c.kind}
                      {'segment' in c && c.segment ? `(${c.segment})` : ''}
                      {'point' in c && c.point ? `(${c.point})` : ''}
                      {'target' in c && c.target ? `(${c.target})` : ''}
                    </span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                      {'value' in c && (
                        <input
                          type="number"
                          class="text-input"
                          value={c.value}
                          step={1}
                          style={{ width: '56px', fontSize: '10.5px', padding: '1px 4px' }}
                          onChange={(e) => {
                            const val = Number((e.target as HTMLInputElement).value);
                            if (!Number.isFinite(val)) return;
                            setSketch(() => {
                              const next = structuredClone(solved);
                              const target = next.constraints.find((item) => item.id === c.id);
                              if (target && 'value' in target) target.value = val;
                              return next;
                            });
                          }}
                        />
                      )}
                      <button
                        type="button"
                        class="btn ghost small"
                        data-testid={`remove-constraint-${c.id}`}
                        onClick={() => removeConstraint(c.id)}
                        style={{ padding: '0 4px', fontSize: '10px', color: isConflict ? '#dc2626' : 'var(--text-2)' }}
                        title="Remove constraint"
                      >
                        ✕
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Stage 2: Surface-Attached 3D Feature Panel */}
          <div
            style={{
              background: 'var(--panel-2)',
              border: '1px solid var(--line-strong)',
              borderRadius: '6px',
              padding: '9px 10px',
              display: 'flex',
              flexDirection: 'column',
              gap: '7px',
            }}
          >
            <div style={{ fontSize: '11.5px', fontWeight: 700 }}>
              Stage 2 — Surface-Attached 3D Feature (`BossExtrude`, `PocketCut`, `Revolve`)
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '110px 1fr', gap: '5px 8px', alignItems: 'center', fontSize: '11px' }}>
              <label>Feature Name:</label>
              <input
                type="text"
                class="text-input"
                data-testid="sketch-feature-id-input"
                value={featureId}
                onInput={(e) => setFeatureId((e.target as HTMLInputElement).value)}
                style={{ fontSize: '11px', padding: '3px 6px' }}
              />

              <label>Sketch Plane:</label>
              <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
                <select
                  class="select"
                  data-testid="sketch-plane-mode-select"
                  value={planeMode}
                  onChange={(e) => setPlaneMode((e.target as HTMLSelectElement).value as typeof planeMode)}
                  style={{ fontSize: '11px' }}
                >
                  <option value="xy">World XY (Top / +Z)</option>
                  <option value="xz">World XZ (Front / Revolve)</option>
                  <option value="yz">World YZ (Right / +X)</option>
                  <option value="on_face">Attach to 3D Shape Face…</option>
                </select>
                {planeMode === 'on_face' && (
                  <>
                    <select
                      class="select"
                      data-testid="sketch-host-shape-select"
                      value={hostShapeId}
                      onChange={(e) => setHostShapeId((e.target as HTMLSelectElement).value)}
                      style={{ fontSize: '11px' }}
                    >
                      {(allHostShapes.length > 0 ? allHostShapes : store.doc.root).map((n) => (
                        <option key={n.id} value={n.id}>
                          {n.id}
                        </option>
                      ))}
                    </select>
                    <select
                      class="select"
                      data-testid="sketch-host-face-select"
                      value={hostFace}
                      onChange={(e) => setHostFace((e.target as HTMLSelectElement).value as Face)}
                      style={{ fontSize: '11px' }}
                    >
                      <option value="top">.top (+Z)</option>
                      <option value="bottom">.bottom (-Z)</option>
                      <option value="px">.px (+X)</option>
                      <option value="nx">.nx (-X)</option>
                      <option value="py">.py (+Y)</option>
                      <option value="ny">.ny (-Y)</option>
                    </select>
                    <button
                      type="button"
                      class="btn small"
                      data-testid="sketch-add-face-edges"
                      onClick={injectHostFaceEdgeRefs}
                      title="Insert fixed reference corner points (face_sw..face_nw) + edge-dimensioned hole on the selected face"
                      style={{ fontSize: '10px', padding: '2px 6px' }}
                    >
                      + Face Edge Ref Points
                    </button>
                  </>
                )}
              </div>

              <label>3D Operation:</label>
              <div
                data-testid="sketch-op-icon-cards"
                style={{
                  display: 'grid',
                  gridTemplateColumns: 'repeat(4, 1fr)',
                  gap: '5px',
                }}
              >
                {(
                  [
                    { id: 'boss_extrude', label: 'Pad / Boss', sub: '+ Solid & Holes', icon: Icon.featBossExtrude, color: '#22c55e' },
                    { id: 'pocket_cut', label: 'Pocket Cut', sub: '− Cut Face', icon: Icon.featPocketCut, color: '#f97316' },
                    { id: 'revolve_boss', label: 'Revolve', sub: '↻ 360° Solid', icon: Icon.featRevolveBoss, color: '#38bdf8' },
                    { id: 'revolve_cut', label: 'Groove Cut', sub: '↻ − Cut Turn', icon: Icon.featRevolveCut, color: '#ec4899' },
                  ] as const
                ).map((item) => {
                  const active = opKind === item.id;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      data-testid={`sketch-op-card-${item.id}`}
                      onClick={() => setOpKind(item.id)}
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        alignItems: 'center',
                        gap: '2px',
                        padding: '5px 4px',
                        borderRadius: '6px',
                        cursor: 'pointer',
                        background: active ? `color-mix(in srgb, ${item.color} 18%, var(--panel))` : 'var(--panel)',
                        border: `1.5px solid ${active ? item.color : 'var(--line)'}`,
                        color: 'var(--text)',
                      }}
                    >
                      <span style={{ width: '18px', height: '18px', display: 'inline-flex', color: item.color }}>
                        <item.icon />
                      </span>
                      <span style={{ fontSize: '10px', fontWeight: 700, lineHeight: 1.1 }}>{item.label}</span>
                      <span style={{ fontSize: '9px', color: 'var(--text-2)', lineHeight: 1.1 }}>{item.sub}</span>
                    </button>
                  );
                })}
              </div>

              {opKind === 'boss_extrude' && (
                <>
                  <label>Extrude Height:</label>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                    <input
                      type="number"
                      class="text-input"
                      data-testid="sketch-extrude-height"
                      value={extrudeHeight}
                      min={0.5}
                      step={1}
                      onInput={(e) => setExtrudeHeight(Number((e.target as HTMLInputElement).value) || 10)}
                      style={{ width: '75px', fontSize: '11px' }}
                    />
                    <span style={{ color: 'var(--text-2)' }}>mm · Blend R:</span>
                    <input
                      type="number"
                      class="text-input"
                      value={featureBlend}
                      min={0}
                      step={0.5}
                      onInput={(e) => setFeatureBlend(Number((e.target as HTMLInputElement).value) || 0)}
                      style={{ width: '55px', fontSize: '11px' }}
                    />
                  </div>
                </>
              )}

              {opKind === 'pocket_cut' && (
                <>
                  <label>Pocket Depth:</label>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                    <input
                      type="number"
                      class="text-input"
                      data-testid="sketch-pocket-depth"
                      value={pocketDepth}
                      disabled={throughAll}
                      min={0.5}
                      step={1}
                      onInput={(e) => setPocketDepth(Number((e.target as HTMLInputElement).value) || 5)}
                      style={{ width: '70px', fontSize: '11px' }}
                    />
                    <label style={{ display: 'flex', alignItems: 'center', gap: '4px', cursor: 'pointer' }}>
                      <input
                        type="checkbox"
                        checked={throughAll}
                        onChange={(e) => setThroughAll((e.target as HTMLInputElement).checked)}
                      />
                      Through-All
                    </label>
                  </div>
                </>
              )}

              {(opKind === 'revolve_boss' || opKind === 'revolve_cut') && (
                <>
                  <label>Revolve Angle:</label>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                    <input
                      type="number"
                      class="text-input"
                      data-testid="sketch-revolve-angle"
                      value={revolveAngle}
                      min={1}
                      max={360}
                      step={15}
                      onInput={(e) => setRevolveAngle(Number((e.target as HTMLInputElement).value) || 360)}
                      style={{ width: '75px', fontSize: '11px' }}
                    />
                    <span style={{ color: 'var(--text-2)' }}>deg (360° = full turn)</span>
                  </div>
                </>
              )}
            </div>

            <div style={{ display: 'flex', flexWrap: 'wrap', justifyContent: 'space-between', alignItems: 'center', gap: '8px', marginTop: '4px' }}>
              <label
                style={{ display: 'inline-flex', alignItems: 'center', gap: '5px', fontSize: '10.5px', cursor: 'pointer' }}
                title="Associate created 3D feature or fixing inside the host part's assembly group in the Object Tree"
              >
                <input
                  type="checkbox"
                  data-testid="sketch-associate-host-checkbox"
                  checked={associateInTree}
                  onChange={(e) => setAssociateInTree((e.target as HTMLInputElement).checked)}
                />
                <span>🔗 Nest under `{hostShapeId}_assy` in Object Tree</span>
              </label>
              <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
                <button
                  type="button"
                  class="btn ghost small"
                  data-testid="toggle-sketch-mcp-json"
                  onClick={() => setShowMcpJson((v) => !v)}
                  style={{ fontSize: '10.5px' }}
                >
                  {showMcpJson ? 'Hide MCP JSON' : '🤖 View MCP Tool Call'}
                </button>
                <button
                  type="button"
                  class="btn primary"
                  data-testid="apply-sketch-feature-btn"
                  onClick={applyTo3dDocument}
                  style={{ fontSize: '11.5px', fontWeight: 700, padding: '5px 12px' }}
                >
                  🚀 Build 3D Feature in Document
                </button>
              </div>
            </div>

            {/* Direct Sketch-Driven Fixing Placement at Constrained Sketch Circles */}
            <div
              data-testid="sketch-fixing-placement-bar"
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: '6px',
                paddingTop: '6px',
                borderTop: '1px dashed var(--line)',
                fontSize: '10.5px',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: '5px', flexWrap: 'wrap' }}>
                <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>🔩 Sketch-to-Fixing:</span>
                <select
                  class="select"
                  data-testid="sketch-fixing-kind-select"
                  value={sketchFixingKind}
                  onChange={(e) => {
                    const nextKind = (e.target as HTMLSelectElement).value as FixingKindId;
                    setSketchFixingKind(nextKind);
                  }}
                  style={{ fontSize: '10.5px' }}
                >
                  {FIXINGS_CATALOGUE.map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.icon} {f.shortLabel}
                    </option>
                  ))}
                </select>
                <select
                  class="select"
                  data-testid="sketch-fixing-size-select"
                  value={sketchFixingSize}
                  onChange={(e) => setSketchFixingSize((e.target as HTMLSelectElement).value)}
                  style={{ fontSize: '10.5px' }}
                >
                  {FIXING_SIZES.map((s) => (
                    <option key={s.name} value={s.name}>
                      {s.name} (⌀{s.major_d}×{s.pitch})
                    </option>
                  ))}
                </select>
              </div>
              <button
                type="button"
                class="btn small"
                data-testid="sketch-place-fixings"
                onClick={placeFixingsAtSketchHoles}
                title="Insert auto-tapped 3D fixing(s) at the solved (u, v) centres of all non-construction sketch circles on the target face"
                style={{
                  fontSize: '10.5px',
                  fontWeight: 700,
                  padding: '4px 9px',
                  background: 'rgba(56, 189, 248, 0.14)',
                  borderColor: 'rgba(56, 189, 248, 0.45)',
                  color: 'var(--text)',
                }}
              >
                🔩 Place Fixing at Sketch Circle(s) ({solved.circles.filter((c) => !c.construction).length || 1})
              </button>
            </div>

            {showMcpJson && (
              <pre
                data-testid="sketch-mcp-json-preview"
                style={{
                  margin: '4px 0 0',
                  padding: '7px 8px',
                  background: '#0f172a',
                  color: '#e2e8f0',
                  borderRadius: '5px',
                  fontSize: '10px',
                  overflowX: 'auto',
                }}
              >
                {mcpToolPayload}
              </pre>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export function SketchWorkbenchModal() {
  const [open, setOpen] = useState(sketchModalOpen);
  useEffect(() => {
    const fn = () => setOpen(sketchModalOpen);
    modalListeners.add(fn);
    return () => {
      modalListeners.delete(fn);
    };
  }, []);

  if (!open) return null;

  return (
    <div
      class="modal-backdrop"
      data-testid="sketch-workbench-modal"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeSketchWorkbenchModal();
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.55)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 999,
        padding: '16px',
      }}
    >
      <div style={{ width: 'min(980px, 96vw)', maxHeight: '92vh', overflowY: 'auto' }}>
        <SketchWorkbench onClose={() => closeSketchWorkbenchModal()} />
      </div>
    </div>
  );
}
