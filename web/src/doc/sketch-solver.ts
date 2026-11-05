// Parametric 2D Sketch & Constraint Solver + Surface-Sketch 3D Feature Engine (Stage 1 + Stage 2).
// Mirrors `crates/fieldsmith-doc/src/sketch.rs` in TypeScript so the browser workbench can run
// interactive nullspace dragging and constraint diagnostics at 60+ fps and commit directly into
// the live `store.doc` (or export JSON payloads for the `solve_sketch` / `apply_sketch_feature` MCP tools).

import type { Document, Face, Node, ProfileVertex } from './types';
import { associateNodesWithHost, resolveHostSurfaceFrame } from './ops';

export type SketchPlane =
  | { kind: 'xy'; offset?: number }
  | { kind: 'xz'; offset?: number }
  | { kind: 'yz'; offset?: number }
  | { kind: 'on_face'; host_id: string; face: Face; offset?: number };

export interface SketchPoint {
  id: string;
  x: number;
  y: number;
  fixed?: boolean;
  construction?: boolean;
}

export interface SketchSegment {
  id: string;
  p1: string;
  p2: string;
  bulge?: number;
  construction?: boolean;
}

export interface SketchCircle {
  id: string;
  center: string;
  radius: number;
  construction?: boolean;
}

export type SketchConstraint =
  | { id: string; kind: 'coincident'; p1: string; p2: string }
  | { id: string; kind: 'fixed'; point: string; x: number; y: number }
  | { id: string; kind: 'horizontal'; segment?: string; p1?: string; p2?: string }
  | { id: string; kind: 'vertical'; segment?: string; p1?: string; p2?: string }
  | { id: string; kind: 'parallel'; seg1: string; seg2: string }
  | { id: string; kind: 'perpendicular'; seg1: string; seg2: string }
  | { id: string; kind: 'equal_length'; seg1: string; seg2: string }
  | { id: string; kind: 'tangent'; seg: string; target: string }
  | { id: string; kind: 'concentric'; c1: string; c2: string }
  | { id: string; kind: 'midpoint'; point: string; segment?: string; p1?: string; p2?: string }
  | { id: string; kind: 'symmetric'; p1: string; p2: string; axis: string }
  | { id: string; kind: 'point_on_line'; point: string; segment: string }
  | { id: string; kind: 'distance'; segment?: string; p1?: string; p2?: string; value: number; expr?: string }
  | { id: string; kind: 'horizontal_distance'; segment?: string; p1?: string; p2?: string; value: number; expr?: string }
  | { id: string; kind: 'vertical_distance'; segment?: string; p1?: string; p2?: string; value: number; expr?: string }
  | { id: string; kind: 'angle'; seg1: string; seg2?: string; degrees: number; expr?: string }
  | { id: string; kind: 'radius'; target: string; value: number; expr?: string }
  | { id: string; kind: 'diameter'; target: string; value: number; expr?: string };

export interface ParametricSketch {
  id: string;
  plane: SketchPlane;
  points: SketchPoint[];
  segments: SketchSegment[];
  circles: SketchCircle[];
  constraints: SketchConstraint[];
}

export type SketchStatus =
  | 'fully_constrained'
  | 'under_constrained'
  | 'redundant_constraints'
  | 'over_constrained_conflict';

export interface PointFreedom {
  id: string;
  x: number;
  y: number;
  dof: number;
  status: SketchStatus;
}

export interface SketchSolveReport {
  status: SketchStatus;
  converged: boolean;
  iterations: number;
  max_residual: number;
  l2_residual: number;
  total_variables: number;
  total_equations: number;
  jacobian_rank: number;
  dof: number;
  redundant_constraints: string[];
  conflicting_constraints: string[];
  diagnosis: string;
  point_freedom: PointFreedom[];
  outer_profile: ProfileVertex[];
  hole_profiles: ProfileVertex[][];
  solved_sketch: ParametricSketch;
}

export type SketchFeatureOp =
  | { kind: 'boss_extrude'; height: number; blend?: number; symmetric?: boolean }
  | { kind: 'pocket_cut'; depth: number; through_all?: boolean; blend?: number }
  | { kind: 'revolve_boss'; angle_deg: number; blend?: number }
  | { kind: 'revolve_cut'; angle_deg: number; blend?: number };

export interface SketchPresetDescriptor {
  id: string;
  title: string;
  subtitle: string;
  defaultOp: SketchFeatureOp;
  build: () => ParametricSketch;
}

// ---------------------------------------------------------------------------------------------
// High-Level Shape Macros & Engineering Presets
// ---------------------------------------------------------------------------------------------

export function addRectToSketch(
  sk: ParametricSketch,
  id: string,
  x: number,
  y: number,
  width: number,
  height: number,
  fillet = 0,
  construction = false,
): void {
  const p0 = `${id}_p0`;
  const p1 = `${id}_p1`;
  const p2 = `${id}_p2`;
  const p3 = `${id}_p3`;
  sk.points.push(
    { id: p0, x, y, fixed: false, construction },
    { id: p1, x: x + width, y, fixed: false, construction },
    { id: p2, x: x + width, y: y + height, fixed: false, construction },
    { id: p3, x, y: y + height, fixed: false, construction },
  );
  const sBot = `${id}_bot`;
  const sRight = `${id}_right`;
  const sTop = `${id}_top`;
  const sLeft = `${id}_left`;
  sk.segments.push(
    { id: sBot, p1: p0, p2: p1, bulge: 0, construction },
    { id: sRight, p1: p1, p2: p2, bulge: 0, construction },
    { id: sTop, p1: p2, p2: p3, bulge: 0, construction },
    { id: sLeft, p1: p3, p2: p0, bulge: 0, construction },
  );
  sk.constraints.push(
    { id: `${id}_fix_origin`, kind: 'fixed', point: p0, x, y },
    { id: `${id}_horiz_bot`, kind: 'horizontal', segment: sBot },
    { id: `${id}_horiz_top`, kind: 'horizontal', segment: sTop },
    { id: `${id}_vert_right`, kind: 'vertical', segment: sRight },
    { id: `${id}_vert_left`, kind: 'vertical', segment: sLeft },
    { id: `${id}_width`, kind: 'distance', segment: sBot, value: width },
    { id: `${id}_height`, kind: 'distance', segment: sRight, value: height },
  );
  if (fillet > 1e-6) {
    for (const c of [p0, p1, p2, p3]) {
      filletSketchCorner(sk, c, fillet);
    }
  }
}

export function addCenterRectToSketch(
  sk: ParametricSketch,
  id: string,
  cx: number,
  cy: number,
  width: number,
  height: number,
  fillet = 0,
  construction = false,
): void {
  addRectToSketch(sk, id, cx - 0.5 * width, cy - 0.5 * height, width, height, fillet, construction);
}

export function addCircleToSketch(
  sk: ParametricSketch,
  id: string,
  cx: number,
  cy: number,
  radius: number,
  construction = false,
): void {
  const pc = `${id}_center`;
  sk.points.push({ id: pc, x: cx, y: cy, fixed: false, construction: true });
  sk.circles.push({ id, center: pc, radius, construction });
  sk.constraints.push(
    { id: `${id}_fix_center`, kind: 'fixed', point: pc, x: cx, y: cy },
    { id: `${id}_radius`, kind: 'radius', target: id, value: radius },
  );
}

export function addSlotToSketch(
  sk: ParametricSketch,
  id: string,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  radius: number,
): void {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.hypot(dx, dy);
  const ux = len > 1e-9 ? dx / len : 1;
  const uy = len > 1e-9 ? dy / len : 0;
  const nx = uy;
  const ny = -ux;
  const coords: [string, number, number][] = [
    [`${id}_p0`, x1 + nx * radius, y1 + ny * radius],
    [`${id}_p1`, x2 + nx * radius, y2 + ny * radius],
    [`${id}_p2`, x2 - nx * radius, y2 - ny * radius],
    [`${id}_p3`, x1 - nx * radius, y1 - ny * radius],
  ];
  for (const [pid, px, py] of coords) {
    sk.points.push({ id: pid, x: px, y: py, fixed: false, construction: false });
    sk.constraints.push({ id: `${pid}_fix`, kind: 'fixed', point: pid, x: px, y: py });
  }
  sk.segments.push(
    { id: `${id}_side1`, p1: `${id}_p0`, p2: `${id}_p1`, bulge: 0, construction: false },
    { id: `${id}_cap2`, p1: `${id}_p1`, p2: `${id}_p2`, bulge: 1.0, construction: false },
    { id: `${id}_side2`, p1: `${id}_p2`, p2: `${id}_p3`, bulge: 0, construction: false },
    { id: `${id}_cap1`, p1: `${id}_p3`, p2: `${id}_p0`, bulge: 1.0, construction: false },
  );
}

export function addPolygonToSketch(
  sk: ParametricSketch,
  id: string,
  cx: number,
  cy: number,
  radius: number,
  sides: number,
  angleDeg = 0,
  construction = false,
): void {
  const n = Math.max(3, Math.min(32, Math.round(sides)));
  const pids: string[] = [];
  for (let i = 0; i < n; i++) {
    const th = (angleDeg * Math.PI) / 180 + (2 * Math.PI * i) / n;
    const px = cx + radius * Math.cos(th);
    const py = cy + radius * Math.sin(th);
    const pid = `${id}_v${i}`;
    sk.points.push({ id: pid, x: px, y: py, fixed: false, construction });
    sk.constraints.push({ id: `${pid}_fix`, kind: 'fixed', point: pid, x: px, y: py });
    pids.push(pid);
  }
  for (let i = 0; i < n; i++) {
    sk.segments.push({
      id: `${id}_e${i}`,
      p1: pids[i],
      p2: pids[(i + 1) % n],
      bulge: 0,
      construction,
    });
  }
}

export function addBoltCircleToSketch(
  sk: ParametricSketch,
  id: string,
  cx: number,
  cy: number,
  pcd: number,
  count: number,
  holeDiameter: number,
  startAngleDeg = 0,
): void {
  addCircleToSketch(sk, `${id}_pcd`, cx, cy, 0.5 * pcd, true);
  const rPcd = 0.5 * pcd;
  const rHole = 0.5 * holeDiameter;
  for (let i = 0; i < count; i++) {
    const th = (startAngleDeg * Math.PI) / 180 + (2 * Math.PI * i) / count;
    addCircleToSketch(sk, `${id}_h${i + 1}`, cx + rPcd * Math.cos(th), cy + rPcd * Math.sin(th), rHole, false);
  }
}

export function filletSketchCorner(sk: ParametricSketch, pointId: string, radius: number): boolean {
  if (radius <= 0) return false;
  const cIdx = sk.points.findIndex((p) => p.id === pointId);
  if (cIdx < 0) return false;
  const corner = sk.points[cIdx];
  const inIdx = sk.segments.findIndex((s) => !s.construction && s.p2 === pointId && Math.abs(s.bulge ?? 0) < 1e-9);
  const outIdx = sk.segments.findIndex((s) => !s.construction && s.p1 === pointId && Math.abs(s.bulge ?? 0) < 1e-9);
  if (inIdx < 0 || outIdx < 0) return false;

  const pPrev = sk.points.find((p) => p.id === sk.segments[inIdx].p1);
  const pNext = sk.points.find((p) => p.id === sk.segments[outIdx].p2);
  if (!pPrev || !pNext) return false;

  const vIn = [pPrev.x - corner.x, pPrev.y - corner.y];
  const vOut = [pNext.x - corner.x, pNext.y - corner.y];
  const lIn = Math.hypot(vIn[0], vIn[1]);
  const lOut = Math.hypot(vOut[0], vOut[1]);
  if (lIn < 1e-6 || lOut < 1e-6) return false;

  const uIn = [vIn[0] / lIn, vIn[1] / lIn];
  const uOut = [vOut[0] / lOut, vOut[1] / lOut];
  const cosAlpha = Math.max(-0.9999, Math.min(0.9999, uIn[0] * uOut[0] + uIn[1] * uOut[1]));
  const alpha = Math.acos(cosAlpha);
  const tanDist = radius / Math.tan(0.5 * alpha);
  if (tanDist >= 0.95 * Math.min(lIn, lOut)) return false;

  const t1x = corner.x + uIn[0] * tanDist;
  const t1y = corner.y + uIn[1] * tanDist;
  const t2x = corner.x + uOut[0] * tanDist;
  const t2y = corner.y + uOut[1] * tanDist;
  const cross = -uIn[0] * uOut[1] - -uIn[1] * uOut[0];
  const sweep = Math.PI - alpha;
  const bulge = Math.tan(0.25 * sweep) * (cross >= 0 ? 1 : -1);

  const t1Id = `${pointId}_f1`;
  const t2Id = `${pointId}_f2`;
  const inSegId = sk.segments[inIdx].id;
  const outSegId = sk.segments[outIdx].id;
  sk.constraints = sk.constraints.filter((c) => {
    if (c.kind === 'fixed' && c.point === pointId) return false;
    if (c.kind === 'distance' && (c.segment === inSegId || c.segment === outSegId)) return false;
    return true;
  });

  sk.points.splice(cIdx, 1);
  sk.points.push(
    { id: t1Id, x: t1x, y: t1y, fixed: false, construction: false },
    { id: t2Id, x: t2x, y: t2y, fixed: false, construction: false },
  );
  sk.constraints.push(
    { id: `${t1Id}_fix`, kind: 'fixed', point: t1Id, x: t1x, y: t1y },
    { id: `${t2Id}_fix`, kind: 'fixed', point: t2Id, x: t2x, y: t2y },
  );
  sk.segments[inIdx].p2 = t1Id;
  sk.segments[outIdx].p1 = t2Id;
  sk.segments.splice(inIdx + 1, 0, {
    id: `${pointId}_arc`,
    p1: t1Id,
    p2: t2Id,
    bulge,
    construction: false,
  });
  return true;
}

export const SKETCH_PRESETS: SketchPresetDescriptor[] = [
  {
    id: 'flanged_mounting_bracket',
    title: 'L-Flanged Mounting Bracket (0 DOF)',
    subtitle: '6-segment orthogonal L-profile + ⌀6.4 mm mounting hole with Horizontal, Vertical & Distance constraints',
    defaultOp: { kind: 'boss_extrude', height: 18, blend: 0 },
    build: () => ({
      id: 'flanged_mounting_bracket',
      plane: { kind: 'xy', offset: 0 },
      points: [
        { id: 'p0', x: 0, y: 0 },
        { id: 'p1', x: 60, y: 0 },
        { id: 'p2', x: 60, y: 12 },
        { id: 'p3', x: 14, y: 12 },
        { id: 'p4', x: 14, y: 45 },
        { id: 'p5', x: 0, y: 45 },
        { id: 'hc', x: 42, y: 6, construction: true },
      ],
      segments: [
        { id: 's0', p1: 'p0', p2: 'p1', bulge: 0 },
        { id: 's1', p1: 'p1', p2: 'p2', bulge: 0 },
        { id: 's2', p1: 'p2', p2: 'p3', bulge: 0 },
        { id: 's3', p1: 'p3', p2: 'p4', bulge: 0 },
        { id: 's4', p1: 'p4', p2: 'p5', bulge: 0 },
        { id: 's5', p1: 'p5', p2: 'p0', bulge: 0 },
      ],
      circles: [{ id: 'hole1', center: 'hc', radius: 3.2 }],
      constraints: [
        { id: 'c_origin', kind: 'fixed', point: 'p0', x: 0, y: 0 },
        { id: 'c_h0', kind: 'horizontal', segment: 's0' },
        { id: 'c_v1', kind: 'vertical', segment: 's1' },
        { id: 'c_h2', kind: 'horizontal', segment: 's2' },
        { id: 'c_v3', kind: 'vertical', segment: 's3' },
        { id: 'c_h4', kind: 'horizontal', segment: 's4' },
        { id: 'c_v5', kind: 'vertical', segment: 's5' },
        { id: 'd_base_w', kind: 'distance', segment: 's0', value: 60 },
        { id: 'd_flange_t', kind: 'distance', segment: 's1', value: 12 },
        { id: 'd_web_t', kind: 'distance', segment: 's4', value: 14 },
        { id: 'd_total_h', kind: 'distance', segment: 's5', value: 45 },
        { id: 'c_hole_pos', kind: 'fixed', point: 'hc', x: 42, y: 6 },
        { id: 'd_hole_dia', kind: 'diameter', target: 'hole1', value: 6.4 },
      ],
    }),
  },
  {
    id: 'bolt_flange_plate',
    title: 'Filleted Bolt-Circle Flange Plate (Macro)',
    subtitle: 'Center-rectangle (64×64 mm, R8 fillets) + ⌀28 mm center bore + 4× ⌀5.5 mm holes on ⌀44 mm PCD',
    defaultOp: { kind: 'boss_extrude', height: 10, blend: 1.5 },
    build: () => {
      const sk: ParametricSketch = {
        id: 'bolt_flange_plate',
        plane: { kind: 'xy', offset: 0 },
        points: [],
        segments: [],
        circles: [],
        constraints: [],
      };
      addCenterRectToSketch(sk, 'plate', 0, 0, 64, 64, 8, false);
      addCircleToSketch(sk, 'center_bore', 0, 0, 14, false);
      addBoltCircleToSketch(sk, 'bolts', 0, 0, 44, 4, 5.5, 45);
      return sk;
    },
  },
  {
    id: 'slotted_adjustment_link',
    title: 'Slotted Tangent-Arc Adjustment Link',
    subtitle: 'Stadium slot (50 mm center-to-center, R12 caps) + dual ⌀10 mm pin bores',
    defaultOp: { kind: 'boss_extrude', height: 8, blend: 0 },
    build: () => {
      const sk: ParametricSketch = {
        id: 'slotted_adjustment_link',
        plane: { kind: 'xy', offset: 0 },
        points: [],
        segments: [],
        circles: [],
        constraints: [],
      };
      addSlotToSketch(sk, 'outer_slot', 0, 0, 50, 0, 12);
      addCircleToSketch(sk, 'left_bore', 0, 0, 5, false);
      addCircleToSketch(sk, 'right_bore', 50, 0, 5, false);
      return sk;
    },
  },
  {
    id: 'turned_aerospace_spool',
    title: 'Turned Aerospace Flanged Spool (360° Revolve)',
    subtitle: 'Meridian (R, Z) profile revolved 360° around Z axis (bore R=6 mm, web R=11 mm, flange R=18 mm)',
    defaultOp: { kind: 'revolve_boss', angle_deg: 360, blend: 0 },
    build: () => {
      const pts: SketchPoint[] = [
        { id: 'p0', x: 6, y: 0 },
        { id: 'p1', x: 18, y: 0 },
        { id: 'p2', x: 18, y: 4 },
        { id: 'p3', x: 11, y: 4 },
        { id: 'p4', x: 11, y: 20 },
        { id: 'p5', x: 18, y: 20 },
        { id: 'p6', x: 18, y: 24 },
        { id: 'p7', x: 6, y: 24 },
      ];
      return {
        id: 'turned_aerospace_spool',
        plane: { kind: 'xz', offset: 0 },
        points: pts,
        segments: pts.map((p, i) => ({
          id: `s${i}`,
          p1: p.id,
          p2: pts[(i + 1) % pts.length].id,
          bulge: 0,
        })),
        circles: [],
        constraints: pts.map((p) => ({
          id: `fix_${p.id}`,
          kind: 'fixed' as const,
          point: p.id,
          x: p.x,
          y: p.y,
        })),
      };
    },
  },
  {
    id: 'under_constrained_demo',
    title: 'Under-Constrained Four-Bar Corner (2 DOF Free)',
    subtitle: 'Interactive DOF demo: p2 is free to drag in 2D until you add Horizontal/Vertical or Distance constraints',
    defaultOp: { kind: 'boss_extrude', height: 12, blend: 0 },
    build: () => ({
      id: 'under_constrained_demo',
      plane: { kind: 'xy', offset: 0 },
      points: [
        { id: 'p0', x: 0, y: 0 },
        { id: 'p1', x: 45, y: 0 },
        { id: 'p2', x: 38, y: 28 },
        { id: 'p3', x: 0, y: 30 },
      ],
      segments: [
        { id: 's0', p1: 'p0', p2: 'p1', bulge: 0 },
        { id: 's1', p1: 'p1', p2: 'p2', bulge: 0 },
        { id: 's2', p1: 'p2', p2: 'p3', bulge: 0 },
        { id: 's3', p1: 'p3', p2: 'p0', bulge: 0 },
      ],
      circles: [],
      constraints: [
        { id: 'c_origin', kind: 'fixed', point: 'p0', x: 0, y: 0 },
        { id: 'c_h0', kind: 'horizontal', segment: 's0' },
        { id: 'c_v3', kind: 'vertical', segment: 's3' },
        { id: 'd_w', kind: 'distance', segment: 's0', value: 45 },
        { id: 'd_h', kind: 'distance', segment: 's3', value: 30 },
      ],
    }),
  },
];

// ---------------------------------------------------------------------------------------------
// Stage 1 Solver: Damped Levenberg–Marquardt + Homotopy + Column-Pivoted QR Rank Analysis
// ---------------------------------------------------------------------------------------------

interface VarMap {
  pointVars: (readonly [number, number] | null)[];
  circleVars: number[];
  nVars: number;
}

function buildVarMap(sk: ParametricSketch): VarMap {
  let nVars = 0;
  const pointVars: (readonly [number, number] | null)[] = [];
  for (const p of sk.points) {
    if (p.fixed) {
      pointVars.push(null);
    } else {
      pointVars.push([nVars, nVars + 1] as const);
      nVars += 2;
    }
  }
  const circleVars: number[] = [];
  for (let k = 0; k < sk.circles.length; k++) {
    circleVars.push(nVars++);
  }
  return { pointVars, circleVars, nVars };
}

function packVars(sk: ParametricSketch, vmap: VarMap): number[] {
  const x = new Array<number>(vmap.nVars).fill(0);
  sk.points.forEach((p, i) => {
    const pv = vmap.pointVars[i];
    if (pv) {
      x[pv[0]] = p.x;
      x[pv[1]] = p.y;
    }
  });
  sk.circles.forEach((c, k) => {
    x[vmap.circleVars[k]] = c.radius;
  });
  return x;
}

function unpackVars(sk: ParametricSketch, vmap: VarMap, x: number[]): void {
  sk.points.forEach((p, i) => {
    const pv = vmap.pointVars[i];
    if (pv) {
      p.x = x[pv[0]];
      p.y = x[pv[1]];
    }
  });
  sk.circles.forEach((c, k) => {
    c.radius = Math.max(1e-6, x[vmap.circleVars[k]]);
  });
}

function evalSimpleExpr(expr: string, letsMap: Record<string, number>): number | null {
  const clean = expr.trim();
  if (!clean) return null;
  // Safe arithmetic evaluator supporting numbers, identifiers from letsMap, +, -, *, /, (, )
  const tokens = clean.match(/[A-Za-z_][A-Za-z0-9_]*|\d+(?:\.\d+)?(?:e[+-]?\d+)?|[+\-*/()]/gi);
  if (!tokens) return null;
  let pos = 0;
  const parsePrimary = (): number => {
    const tok = tokens[pos++];
    if (!tok) return NaN;
    if (tok === '-') return -parsePrimary();
    if (tok === '+') return parsePrimary();
    if (tok === '(') {
      const val = parseSum();
      if (tokens[pos] === ')') pos++;
      return val;
    }
    if (/^[A-Za-z_]/.test(tok)) {
      return tok in letsMap ? letsMap[tok] : NaN;
    }
    return Number(tok);
  };
  const parseProduct = (): number => {
    let left = parsePrimary();
    while (pos < tokens.length && (tokens[pos] === '*' || tokens[pos] === '/')) {
      const op = tokens[pos++];
      const right = parsePrimary();
      left = op === '*' ? left * right : left / right;
    }
    return left;
  };
  const parseSum = (): number => {
    let left = parseProduct();
    while (pos < tokens.length && (tokens[pos] === '+' || tokens[pos] === '-')) {
      const op = tokens[pos++];
      const right = parseProduct();
      left = op === '+' ? left + right : left - right;
    }
    return left;
  };
  const out = parseSum();
  return Number.isFinite(out) ? out : null;
}

function ptXY(sk: ParametricSketch, x: number[], vmap: VarMap, id: string): [number, number] {
  const idx = sk.points.findIndex((p) => p.id === id);
  if (idx < 0) return [0, 0];
  const pv = vmap.pointVars[idx];
  return pv ? [x[pv[0]], x[pv[1]]] : [sk.points[idx].x, sk.points[idx].y];
}

function pairXY(
  sk: ParametricSketch,
  x: number[],
  vmap: VarMap,
  segment?: string,
  p1?: string,
  p2?: string,
): [[number, number], [number, number]] {
  if (segment) {
    const s = sk.segments.find((seg) => seg.id === segment);
    if (s) return [ptXY(sk, x, vmap, s.p1), ptXY(sk, x, vmap, s.p2)];
  }
  if (p1 && p2) {
    return [ptXY(sk, x, vmap, p1), ptXY(sk, x, vmap, p2)];
  }
  return [
    [0, 0],
    [1, 0],
  ];
}

function circleOrArcGeom(
  sk: ParametricSketch,
  x: number[],
  vmap: VarMap,
  targetId: string,
): { center: [number, number]; r: number } {
  const cIdx = sk.circles.findIndex((c) => c.id === targetId);
  if (cIdx >= 0) {
    return {
      center: ptXY(sk, x, vmap, sk.circles[cIdx].center),
      r: Math.abs(x[vmap.circleVars[cIdx]]),
    };
  }
  const seg = sk.segments.find((s) => s.id === targetId);
  if (seg && Math.abs(seg.bulge ?? 0) > 1e-9) {
    const a = ptXY(sk, x, vmap, seg.p1);
    const b = ptXY(sk, x, vmap, seg.p2);
    const k = seg.bulge ?? 0;
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const chord = Math.max(1e-12, Math.hypot(dx, dy));
    const r = (chord * (1 + k * k)) / (4 * Math.abs(k));
    const ux = dx / chord;
    const uy = dy / chord;
    const nx = -uy;
    const ny = ux;
    const offset = (chord * (1 - k * k)) / (4 * k);
    return {
      center: [0.5 * (a[0] + b[0]) + nx * offset, 0.5 * (a[1] + b[1]) + ny * offset],
      r,
    };
  }
  return { center: [0, 0], r: 1 };
}

function evalResiduals(
  sk: ParametricSketch,
  x: number[],
  vmap: VarMap,
  alpha: number,
  initX: number[],
): { r: number[]; rowIds: string[] } {
  const r: number[] = [];
  const rowIds: string[] = [];
  const blend = (target: number, initVal: number) =>
    alpha >= 0.999999 ? target : initVal + alpha * (target - initVal);

  for (const c of sk.constraints) {
    switch (c.kind) {
      case 'coincident': {
        const a = ptXY(sk, x, vmap, c.p1);
        const b = ptXY(sk, x, vmap, c.p2);
        r.push(a[0] - b[0], a[1] - b[1]);
        rowIds.push(c.id, c.id);
        break;
      }
      case 'fixed': {
        const p = ptXY(sk, x, vmap, c.point);
        const p0 = ptXY(sk, initX, vmap, c.point);
        r.push(p[0] - blend(c.x, p0[0]), p[1] - blend(c.y, p0[1]));
        rowIds.push(c.id, c.id);
        break;
      }
      case 'horizontal': {
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        r.push(b[1] - a[1]);
        rowIds.push(c.id);
        break;
      }
      case 'vertical': {
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        r.push(b[0] - a[0]);
        rowIds.push(c.id);
        break;
      }
      case 'parallel': {
        const [a1, b1] = pairXY(sk, x, vmap, c.seg1);
        const [a2, b2] = pairXY(sk, x, vmap, c.seg2);
        const d1x = b1[0] - a1[0];
        const d1y = b1[1] - a1[1];
        const d2x = b2[0] - a2[0];
        const d2y = b2[1] - a2[1];
        const l2 = Math.max(1e-9, Math.hypot(d2x, d2y));
        r.push((d1x * d2y - d1y * d2x) / l2);
        rowIds.push(c.id);
        break;
      }
      case 'perpendicular': {
        const [a1, b1] = pairXY(sk, x, vmap, c.seg1);
        const [a2, b2] = pairXY(sk, x, vmap, c.seg2);
        const d1x = b1[0] - a1[0];
        const d1y = b1[1] - a1[1];
        const d2x = b2[0] - a2[0];
        const d2y = b2[1] - a2[1];
        const l2 = Math.max(1e-9, Math.hypot(d2x, d2y));
        r.push((d1x * d2x + d1y * d2y) / l2);
        rowIds.push(c.id);
        break;
      }
      case 'equal_length': {
        if (sk.circles.some((ci) => ci.id === c.seg1) && sk.circles.some((ci) => ci.id === c.seg2)) {
          const g1 = circleOrArcGeom(sk, x, vmap, c.seg1);
          const g2 = circleOrArcGeom(sk, x, vmap, c.seg2);
          r.push(g1.r - g2.r);
          rowIds.push(c.id);
        } else {
          const [a1, b1] = pairXY(sk, x, vmap, c.seg1);
          const [a2, b2] = pairXY(sk, x, vmap, c.seg2);
          const l1 = Math.hypot(b1[0] - a1[0], b1[1] - a1[1]);
          const l2 = Math.hypot(b2[0] - a2[0], b2[1] - a2[1]);
          r.push(l1 - l2);
          rowIds.push(c.id);
        }
        break;
      }
      case 'tangent': {
        const [a, b] = pairXY(sk, x, vmap, c.seg);
        const { center, r: rad } = circleOrArcGeom(sk, x, vmap, c.target);
        const dx = b[0] - a[0];
        const dy = b[1] - a[1];
        const len = Math.max(1e-9, Math.hypot(dx, dy));
        const signedDist = ((center[0] - a[0]) * -dy + (center[1] - a[1]) * dx) / len;
        r.push(Math.abs(signedDist) - rad);
        rowIds.push(c.id);
        break;
      }
      case 'concentric': {
        const g1 = circleOrArcGeom(sk, x, vmap, c.c1);
        const g2 = circleOrArcGeom(sk, x, vmap, c.c2);
        r.push(g1.center[0] - g2.center[0], g1.center[1] - g2.center[1]);
        rowIds.push(c.id, c.id);
        break;
      }
      case 'midpoint': {
        const pm = ptXY(sk, x, vmap, c.point);
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        r.push(pm[0] - 0.5 * (a[0] + b[0]), pm[1] - 0.5 * (a[1] + b[1]));
        rowIds.push(c.id, c.id);
        break;
      }
      case 'symmetric': {
        const a = ptXY(sk, x, vmap, c.p1);
        const b = ptXY(sk, x, vmap, c.p2);
        const ax = c.axis.toLowerCase();
        if (ax === 'y_axis' || ax === 'y' || ax === 'vertical') {
          r.push(a[0] + b[0], a[1] - b[1]);
          rowIds.push(c.id, c.id);
        } else if (ax === 'x_axis' || ax === 'x' || ax === 'horizontal') {
          r.push(a[0] - b[0], a[1] + b[1]);
          rowIds.push(c.id, c.id);
        } else {
          const [l1, l2] = pairXY(sk, x, vmap, c.axis);
          const dx = l2[0] - l1[0];
          const dy = l2[1] - l1[1];
          const len = Math.max(1e-9, Math.hypot(dx, dy));
          const ux = dx / len;
          const uy = dy / len;
          const mx = 0.5 * (a[0] + b[0]) - l1[0];
          const my = 0.5 * (a[1] + b[1]) - l1[1];
          r.push(mx * uy - my * ux, (b[0] - a[0]) * ux + (b[1] - a[1]) * uy);
          rowIds.push(c.id, c.id);
        }
        break;
      }
      case 'point_on_line': {
        const p = ptXY(sk, x, vmap, c.point);
        const [a, b] = pairXY(sk, x, vmap, c.segment);
        const dx = b[0] - a[0];
        const dy = b[1] - a[1];
        const len = Math.max(1e-9, Math.hypot(dx, dy));
        r.push(((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / len);
        rowIds.push(c.id);
        break;
      }
      case 'distance': {
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        const [a0, b0] = pairXY(sk, initX, vmap, c.segment, c.p1, c.p2);
        const dist = Math.hypot(b[0] - a[0], b[1] - a[1]);
        const dist0 = Math.hypot(b0[0] - a0[0], b0[1] - a0[1]);
        r.push(dist - blend(c.value, dist0));
        rowIds.push(c.id);
        break;
      }
      case 'horizontal_distance': {
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        const [a0, b0] = pairXY(sk, initX, vmap, c.segment, c.p1, c.p2);
        const sign = b0[0] >= a0[0] ? 1 : -1;
        r.push((b[0] - a[0]) * sign - blend(c.value, Math.abs(b0[0] - a0[0])));
        rowIds.push(c.id);
        break;
      }
      case 'vertical_distance': {
        const [a, b] = pairXY(sk, x, vmap, c.segment, c.p1, c.p2);
        const [a0, b0] = pairXY(sk, initX, vmap, c.segment, c.p1, c.p2);
        const sign = b0[1] >= a0[1] ? 1 : -1;
        r.push((b[1] - a[1]) * sign - blend(c.value, Math.abs(b0[1] - a0[1])));
        rowIds.push(c.id);
        break;
      }
      case 'angle': {
        const [a1, b1] = pairXY(sk, x, vmap, c.seg1);
        const d1x = b1[0] - a1[0];
        const d1y = b1[1] - a1[1];
        const l1 = Math.max(1, Math.hypot(d1x, d1y));
        const ang1 = Math.atan2(d1y, d1x);
        let actual = ang1;
        if (c.seg2) {
          const [a2, b2] = pairXY(sk, x, vmap, c.seg2);
          actual = Math.atan2(b2[1] - a2[1], b2[0] - a2[0]) - ang1;
        }
        const targetRad = (c.degrees * Math.PI) / 180;
        const diff = Math.atan2(Math.sin(actual - targetRad), Math.cos(actual - targetRad));
        r.push(diff * l1);
        rowIds.push(c.id);
        break;
      }
      case 'radius': {
        const { r: rad } = circleOrArcGeom(sk, x, vmap, c.target);
        const { r: rad0 } = circleOrArcGeom(sk, initX, vmap, c.target);
        r.push(rad - blend(c.value, rad0));
        rowIds.push(c.id);
        break;
      }
      case 'diameter': {
        const { r: rad } = circleOrArcGeom(sk, x, vmap, c.target);
        const { r: rad0 } = circleOrArcGeom(sk, initX, vmap, c.target);
        r.push(2 * rad - blend(c.value, 2 * rad0));
        rowIds.push(c.id);
        break;
      }
    }
  }
  return { r, rowIds };
}

function computeJacobian(
  sk: ParametricSketch,
  x: number[],
  vmap: VarMap,
  alpha: number,
  initX: number[],
  m: number,
): number[] {
  const n = vmap.nVars;
  const J = new Array<number>(m * n).fill(0);
  if (m === 0 || n === 0) return J;
  const h = 1e-7;
  const inv2h = 0.5 / h;
  const xWork = x.slice();
  for (let col = 0; col < n; col++) {
    const orig = xWork[col];
    xWork[col] = orig + h;
    const rp = evalResiduals(sk, xWork, vmap, alpha, initX).r;
    xWork[col] = orig - h;
    const rm = evalResiduals(sk, xWork, vmap, alpha, initX).r;
    xWork[col] = orig;
    for (let row = 0; row < m; row++) {
      J[row * n + col] = (rp[row] - rm[row]) * inv2h;
    }
  }
  return J;
}

function solveLinearSystem(A: number[], b: number[], n: number): number[] | null {
  const a = A.slice();
  const rhs = b.slice();
  for (let k = 0; k < n; k++) {
    let piv = k;
    let maxVal = Math.abs(a[k * n + k]);
    for (let i = k + 1; i < n; i++) {
      const v = Math.abs(a[i * n + k]);
      if (v > maxVal) {
        maxVal = v;
        piv = i;
      }
    }
    if (maxVal < 1e-14) return null;
    if (piv !== k) {
      for (let c = k; c < n; c++) {
        const tmp = a[k * n + c];
        a[k * n + c] = a[piv * n + c];
        a[piv * n + c] = tmp;
      }
      const tb = rhs[k];
      rhs[k] = rhs[piv];
      rhs[piv] = tb;
    }
    const diag = a[k * n + k];
    for (let i = k + 1; i < n; i++) {
      const f = a[i * n + k] / diag;
      a[i * n + k] = 0;
      for (let c = k + 1; c < n; c++) {
        a[i * n + c] -= f * a[k * n + c];
      }
      rhs[i] -= f * rhs[k];
    }
  }
  const x = new Array<number>(n).fill(0);
  for (let i = n - 1; i >= 0; i--) {
    let s = rhs[i];
    for (let j = i + 1; j < n; j++) {
      s -= a[i * n + j] * x[j];
    }
    x[i] = s / a[i * n + i];
  }
  return x;
}

function analyzeJacobianRank(J: number[], m: number, n: number) {
  if (m === 0 || n === 0) {
    return {
      rank: 0,
      dependentRows: Array.from({ length: m }, (_, i) => i),
      constrainedCols: new Set<number>(),
    };
  }
  const a = J.slice();
  const rowPerm = Array.from({ length: m }, (_, i) => i);
  const colPerm = Array.from({ length: n }, (_, i) => i);
  const maxK = Math.min(m, n);
  let rank = 0;

  for (let k = 0; k < maxK; k++) {
    let bestVal = 0;
    let bestR = k;
    let bestC = k;
    for (let r = k; r < m; r++) {
      for (let c = k; c < n; c++) {
        const v = Math.abs(a[r * n + c]);
        if (v > bestVal) {
          bestVal = v;
          bestR = r;
          bestC = c;
        }
      }
    }
    if (bestVal < 1e-5) break;
    if (bestR !== k) {
      for (let c = 0; c < n; c++) {
        const tmp = a[k * n + c];
        a[k * n + c] = a[bestR * n + c];
        a[bestR * n + c] = tmp;
      }
      const tr = rowPerm[k];
      rowPerm[k] = rowPerm[bestR];
      rowPerm[bestR] = tr;
    }
    if (bestC !== k) {
      for (let r = 0; r < m; r++) {
        const tmp = a[r * n + k];
        a[r * n + k] = a[r * n + bestC];
        a[r * n + bestC] = tmp;
      }
      const tc = colPerm[k];
      colPerm[k] = colPerm[bestC];
      colPerm[bestC] = tc;
    }
    const piv = a[k * n + k];
    for (let r = k + 1; r < m; r++) {
      const f = a[r * n + k] / piv;
      a[r * n + k] = 0;
      for (let c = k + 1; c < n; c++) {
        a[r * n + c] -= f * a[k * n + c];
      }
    }
    rank++;
  }

  for (let k = rank - 1; k >= 0; k--) {
    const piv = a[k * n + k];
    for (let c = k; c < n; c++) a[k * n + c] /= piv;
    for (let r = 0; r < k; r++) {
      const f = a[r * n + k];
      a[r * n + k] = 0;
      for (let c = k; c < n; c++) a[r * n + c] -= f * a[k * n + c];
    }
  }

  const constrainedCols = new Set<number>();
  for (let k = 0; k < rank; k++) {
    let coupledToFree = false;
    for (let c = rank; c < n; c++) {
      if (Math.abs(a[k * n + c]) > 1e-4) {
        coupledToFree = true;
        break;
      }
    }
    if (!coupledToFree) constrainedCols.add(colPerm[k]);
  }
  return {
    rank,
    dependentRows: rowPerm.slice(rank),
    constrainedCols,
  };
}

function signedLoopArea(verts: ProfileVertex[]): number {
  const n = verts.length;
  if (n < 2) return 0;
  let area = 0;
  for (let i = 0; i < n; i++) {
    const a = verts[i];
    const b = verts[(i + 1) % n];
    area += 0.5 * (a.x * b.y - b.x * a.y);
    if (Math.abs(a.bulge) > 1e-9) {
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const c2 = dx * dx + dy * dy;
      const theta = 4 * Math.atan(a.bulge);
      const r2 = (c2 * Math.pow(1 + a.bulge * a.bulge, 2)) / (16 * a.bulge * a.bulge);
      area += 0.5 * r2 * (theta - Math.sin(theta));
    }
  }
  return area;
}

function ensureCcwLoop(verts: ProfileVertex[]): ProfileVertex[] {
  if (signedLoopArea(verts) >= 0) return verts;
  const n = verts.length;
  const rev: ProfileVertex[] = [];
  for (let i = 0; i < n; i++) {
    const curr = verts[(n - 1 - i + 1) % n];
    const prev = verts[n - 1 - i];
    rev.push({ x: curr.x, y: curr.y, bulge: -(prev.bulge ?? 0) });
  }
  return rev;
}

export function compileSketchProfiles(sk: ParametricSketch): {
  outer_profile: ProfileVertex[];
  hole_profiles: ProfileVertex[][];
} {
  const ptMap = new Map<string, [number, number]>();
  for (const p of sk.points) ptMap.set(p.id, [p.x, p.y]);

  const loops: ProfileVertex[][] = [];
  const activeSegs = sk.segments.filter((s) => !s.construction);
  const visited = new Array<boolean>(activeSegs.length).fill(false);

  for (let startIdx = 0; startIdx < activeSegs.length; startIdx++) {
    if (visited[startIdx]) continue;
    visited[startIdx] = true;
    const first = activeSegs[startIdx];
    const p1 = ptMap.get(first.p1);
    const p2 = ptMap.get(first.p2);
    if (!p1 || !p2) continue;

    const chain: ProfileVertex[] = [{ x: p1[0], y: p1[1], bulge: first.bulge ?? 0 }];
    const startXY = p1;
    let curXY = p2;
    let curId = first.p2;

    while (true) {
      if (Math.hypot(curXY[0] - startXY[0], curXY[1] - startXY[1]) < 1e-4 && chain.length >= 2) {
        loops.push(ensureCcwLoop(chain));
        break;
      }
      let foundNext: {
        idx: number;
        start: [number, number];
        end: [number, number];
        nextId: string;
        bulge: number;
      } | null = null;
      for (let idx = 0; idx < activeSegs.length; idx++) {
        if (visited[idx]) continue;
        const seg = activeSegs[idx];
        const sa = ptMap.get(seg.p1);
        const sb = ptMap.get(seg.p2);
        if (!sa || !sb) continue;
        if (seg.p1 === curId || Math.hypot(sa[0] - curXY[0], sa[1] - curXY[1]) < 1e-4) {
          foundNext = { idx, start: sa, end: sb, nextId: seg.p2, bulge: seg.bulge ?? 0 };
          break;
        } else if (seg.p2 === curId || Math.hypot(sb[0] - curXY[0], sb[1] - curXY[1]) < 1e-4) {
          foundNext = { idx, start: sb, end: sa, nextId: seg.p1, bulge: -(seg.bulge ?? 0) };
          break;
        }
      }
      if (foundNext) {
        visited[foundNext.idx] = true;
        chain.push({ x: foundNext.start[0], y: foundNext.start[1], bulge: foundNext.bulge });
        curXY = foundNext.end;
        curId = foundNext.nextId;
      } else {
        break;
      }
    }
  }

  for (const c of sk.circles) {
    if (c.construction) continue;
    const center = ptMap.get(c.center);
    if (center) {
      const r = Math.max(1e-6, Math.abs(c.radius));
      loops.push([
        { x: center[0] - r, y: center[1], bulge: 1.0 },
        { x: center[0] + r, y: center[1], bulge: 1.0 },
      ]);
    }
  }

  if (loops.length === 0) return { outer_profile: [], hole_profiles: [] };
  loops.sort((a, b) => Math.abs(signedLoopArea(b)) - Math.abs(signedLoopArea(a)));
  const outer = loops.shift()!;
  return { outer_profile: outer, hole_profiles: loops };
}

export function solveParametricSketch(
  inputSketch: ParametricSketch,
  letsMap: Record<string, number> = {},
): SketchSolveReport {
  const sk: ParametricSketch = structuredClone(inputSketch);

  // Evaluate any `.fsm` `let` expressions on dimensional constraints
  for (const c of sk.constraints) {
    if ('expr' in c && c.expr) {
      const val = evalSimpleExpr(c.expr, letsMap);
      if (val !== null) {
        if (c.kind === 'angle') c.degrees = val;
        else c.value = val;
      }
    }
  }

  const vmap = buildVarMap(sk);
  const initX = packVars(sk, vmap);
  let x = initX.slice();
  const n = vmap.nVars;

  const initEval = evalResiduals(sk, x, vmap, 1.0, initX);
  const m = initEval.r.length;
  const rowIds = initEval.rowIds;
  const initMaxR = initEval.r.reduce((acc, v) => Math.max(acc, Math.abs(v)), 0);

  const alphas = initMaxR > 5.0 && m > 0 && n > 0 ? [0.25, 0.5, 0.75, 1.0] : [1.0];
  let totalIters = 0;

  for (const alpha of alphas) {
    let lambda = 1e-4;
    for (let iter = 0; iter < 50; iter++) {
      const { r } = evalResiduals(sk, x, vmap, alpha, initX);
      const maxR = r.reduce((acc, v) => Math.max(acc, Math.abs(v)), 0);
      if (maxR <= 1e-9 || m === 0 || n === 0) break;
      totalIters++;
      const cost = r.reduce((acc, v) => acc + v * v, 0);
      const J = computeJacobian(sk, x, vmap, alpha, initX, m);

      const jtj = new Array<number>(n * n).fill(0);
      const jtr = new Array<number>(n).fill(0);
      for (let row = 0; row < m; row++) {
        const rv = r[row];
        for (let c1 = 0; c1 < n; c1++) {
          const j1 = J[row * n + c1];
          jtr[c1] -= j1 * rv;
          for (let c2 = c1; c2 < n; c2++) {
            jtj[c1 * n + c2] += j1 * J[row * n + c2];
          }
        }
      }
      for (let c1 = 0; c1 < n; c1++) {
        for (let c2 = 0; c2 < c1; c2++) jtj[c1 * n + c2] = jtj[c2 * n + c1];
        jtj[c1 * n + c1] += lambda;
      }

      const dx = solveLinearSystem(jtj, jtr, n);
      if (dx) {
        const stepNorm = Math.hypot(...dx);
        const scale = stepNorm > 25.0 ? 25.0 / stepNorm : 1.0;
        const xCand = x.map((xi, idx) => xi + scale * dx[idx]);
        const rCand = evalResiduals(sk, xCand, vmap, alpha, initX).r;
        const costCand = rCand.reduce((acc, v) => acc + v * v, 0);
        if (costCand < cost) {
          x = xCand;
          lambda = Math.max(1e-9, lambda * 0.3);
          if (stepNorm < 1e-9) break;
        } else {
          lambda = Math.min(1e6, lambda * 5.0);
        }
      } else {
        lambda = Math.min(1e6, lambda * 10.0);
      }
    }
  }

  unpackVars(sk, vmap, x);
  const finalR = evalResiduals(sk, x, vmap, 1.0, initX).r;
  const maxResidual = finalR.reduce((acc, v) => Math.max(acc, Math.abs(v)), 0);
  const l2Residual = Math.hypot(...finalR);
  const finalJ = computeJacobian(sk, x, vmap, 1.0, initX, m);
  const rankInfo = analyzeJacobianRank(finalJ, m, n);
  const dof = Math.max(0, n - rankInfo.rank);

  const conflictSet = new Set<string>();
  const redundantSet = new Set<string>();
  finalR.forEach((res, rowIdx) => {
    if (Math.abs(res) > 1e-4) conflictSet.add(rowIds[rowIdx]);
  });
  for (const depRow of rankInfo.dependentRows) {
    const cid = rowIds[depRow];
    if (Math.abs(finalR[depRow]) <= 1e-4 && !conflictSet.has(cid)) {
      redundantSet.add(cid);
    }
  }

  const redundant_constraints = Array.from(redundantSet);
  const conflicting_constraints = Array.from(conflictSet);
  const converged = conflicting_constraints.length === 0;

  let status: SketchStatus = 'under_constrained';
  if (conflicting_constraints.length > 0) status = 'over_constrained_conflict';
  else if (redundant_constraints.length > 0) status = 'redundant_constraints';
  else if (dof === 0) status = 'fully_constrained';

  const point_freedom: PointFreedom[] = sk.points.map((p, i) => {
    const pv = vmap.pointVars[i];
    let pDof = 0;
    if (pv) {
      pDof =
        (rankInfo.constrainedCols.has(pv[0]) ? 0 : 1) +
        (rankInfo.constrainedCols.has(pv[1]) ? 0 : 1);
    }
    const pStatus: SketchStatus =
      conflicting_constraints.length > 0
        ? 'over_constrained_conflict'
        : pDof === 0
          ? 'fully_constrained'
          : 'under_constrained';
    return { id: p.id, x: p.x, y: p.y, dof: pDof, status: pStatus };
  });

  let diagnosis = '';
  if (status === 'fully_constrained') {
    diagnosis = `Sketch '${sk.id}' is fully constrained (DOF = 0, rank = ${rankInfo.rank}/${n}, max residual = ${maxResidual.toExponential(2)} mm).`;
  } else if (status === 'under_constrained') {
    const freeList = point_freedom
      .filter((pf) => pf.dof > 0)
      .map((pf) => `${pf.id} (${pf.dof} DOF)`)
      .join(', ');
    diagnosis = `Sketch '${sk.id}' is under-constrained with ${dof} DOF remaining (rank = ${rankInfo.rank}/${n}). Free points: [${freeList}].`;
  } else if (status === 'redundant_constraints') {
    diagnosis = `Sketch '${sk.id}' converged (DOF = ${dof}), but has ${redundant_constraints.length} redundant constraint(s): [${redundant_constraints.join(', ')}].`;
  } else {
    diagnosis = `Sketch '${sk.id}' is over-constrained with conflicting constraint(s): [${conflicting_constraints.join(', ')}] (max residual = ${maxResidual.toFixed(3)} mm).`;
  }

  const { outer_profile, hole_profiles } = compileSketchProfiles(sk);

  return {
    status,
    converged,
    iterations: totalIters,
    max_residual: maxResidual,
    l2_residual: l2Residual,
    total_variables: n,
    total_equations: m,
    jacobian_rank: rankInfo.rank,
    dof,
    redundant_constraints,
    conflicting_constraints,
    diagnosis,
    point_freedom,
    outer_profile,
    hole_profiles,
    solved_sketch: sk,
  };
}

// ---------------------------------------------------------------------------------------------
// Stage 2: Surface-Sketch 3D Feature Compiler (`BossExtrude`, `PocketCut`, `Revolve`)
// ---------------------------------------------------------------------------------------------

export function resolveSketchPlaneFrame(doc: Document, plane: SketchPlane): {
  planeOrigin: [number, number, number];
  outwardNormal: [number, number, number];
  rotateDeg: [number, number, number];
  hostThickness: number;
  faceWidth: number;
  faceHeight: number;
  description: string;
} {
  if (plane.kind === 'xy') {
    const off = plane.offset ?? 0;
    return {
      planeOrigin: [0, 0, off],
      outwardNormal: [0, 0, 1],
      rotateDeg: [0, 0, 0],
      hostThickness: 50,
      faceWidth: 80,
      faceHeight: 60,
      description: `World XY plane (z = ${off.toFixed(1)} mm, normal +Z)`,
    };
  }
  if (plane.kind === 'xz') {
    const off = plane.offset ?? 0;
    return {
      planeOrigin: [0, off, 0],
      outwardNormal: [0, -1, 0],
      rotateDeg: [90, 0, 0],
      hostThickness: 50,
      faceWidth: 80,
      faceHeight: 60,
      description: `World XZ plane (y = ${off.toFixed(1)} mm)`,
    };
  }
  if (plane.kind === 'yz') {
    const off = plane.offset ?? 0;
    return {
      planeOrigin: [off, 0, 0],
      outwardNormal: [1, 0, 0],
      rotateDeg: [0, 90, 0],
      hostThickness: 50,
      faceWidth: 80,
      faceHeight: 60,
      description: `World YZ plane (x = ${off.toFixed(1)} mm, normal +X)`,
    };
  }

  const sf = resolveHostSurfaceFrame(doc, plane.host_id, plane.face);
  const off = plane.offset ?? 0;
  const n = sf.outwardNormal;
  const origin: [number, number, number] = [
    sf.faceCenter[0] + n[0] * off,
    sf.faceCenter[1] + n[1] * off,
    sf.faceCenter[2] + n[2] * off,
  ];
  return {
    planeOrigin: origin,
    outwardNormal: n,
    rotateDeg: sf.rotateDeg,
    hostThickness: Math.max(10, sf.hostThickness),
    faceWidth: sf.faceWidth,
    faceHeight: sf.faceHeight,
    description: `Surface '${plane.host_id}.${plane.face}' (${sf.faceWidth.toFixed(1)}×${sf.faceHeight.toFixed(1)} mm) at (${origin[0].toFixed(1)}, ${origin[1].toFixed(1)}, ${origin[2].toFixed(1)}) mm`,
  };
}

function upsertDocNode(doc: Document, node: Node): void {
  const idx = doc.root.findIndex((n) => n.id === node.id);
  if (idx >= 0) doc.root[idx] = node;
  else doc.root.push(node);
}

export function applySketchFeatureToDoc(
  doc: Document,
  featureId: string,
  report: SketchSolveReport,
  plane: SketchPlane,
  op: SketchFeatureOp,
  associateWithHostGroup = true,
): { createdIds: string[]; planeDescription: string } {
  const frame = resolveSketchPlaneFrame(doc, plane);
  const n = frame.outwardNormal;
  const createdIds: string[] = [];
  const createdNodes: Node[] = [];

  const pushFeatureNode = (node: Node) => {
    createdIds.push(node.id);
    if (plane.kind === 'on_face' && associateWithHostGroup) {
      createdNodes.push(node);
    } else {
      upsertDocNode(doc, node);
    }
  };

  if (op.kind === 'boss_extrude') {
    const h = Math.max(0.5, op.height);
    const shift = op.symmetric ? 0 : 0.5 * h;
    const origin: [number, number, number] = [
      frame.planeOrigin[0] + n[0] * shift,
      frame.planeOrigin[1] + n[1] * shift,
      frame.planeOrigin[2] + n[2] * shift,
    ];
    pushFeatureNode({
      id: featureId,
      op: 'add',
      blend: op.blend ?? 0,
      suppressed: false,
      transform: { translate: origin, rotate: frame.rotateDeg },
      kind: { type: 'extrude', profile: structuredClone(report.outer_profile), height: h },
    });

    report.hole_profiles.forEach((holeLoop, i) => {
      const holeId = `${featureId}_hole${i + 1}`;
      pushFeatureNode({
        id: holeId,
        op: 'subtract',
        blend: 0,
        suppressed: false,
        transform: { translate: origin, rotate: frame.rotateDeg },
        kind: { type: 'extrude', profile: structuredClone(holeLoop), height: h + 1.0 },
      });
    });
  } else if (op.kind === 'pocket_cut') {
    const cutDepth = op.through_all ? frame.hostThickness + 2.0 : Math.max(0.5, op.depth);
    const cutH = cutDepth + 0.5;
    const shift = -0.5 * cutDepth + 0.25;
    const origin: [number, number, number] = [
      frame.planeOrigin[0] + n[0] * shift,
      frame.planeOrigin[1] + n[1] * shift,
      frame.planeOrigin[2] + n[2] * shift,
    ];
    pushFeatureNode({
      id: featureId,
      op: 'subtract',
      blend: op.blend ?? 0,
      suppressed: false,
      transform: { translate: origin, rotate: frame.rotateDeg },
      kind: { type: 'extrude', profile: structuredClone(report.outer_profile), height: cutH },
    });
  } else if (op.kind === 'revolve_boss' || op.kind === 'revolve_cut') {
    pushFeatureNode({
      id: featureId,
      op: op.kind === 'revolve_boss' ? 'add' : 'subtract',
      blend: op.blend ?? 0,
      suppressed: false,
      transform: { translate: frame.planeOrigin, rotate: [0, 0, 0] },
      kind: {
        type: 'revolve',
        profile: structuredClone(report.outer_profile),
        angle: Math.max(1, Math.min(360, op.angle_deg)),
      },
    });
  }

  if (plane.kind === 'on_face' && associateWithHostGroup && createdNodes.length > 0) {
    associateNodesWithHost(doc, plane.host_id, createdNodes, true);
  }

  return { createdIds, planeDescription: frame.description };
}
