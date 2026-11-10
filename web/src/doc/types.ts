// Mirrors the serde representation of fieldsmith_doc::Document (crates/fieldsmith-doc/src/model.rs).

export type Vec3 = [number, number, number];
export type Op = 'add' | 'subtract' | 'intersect';
export type Axis = 'x' | 'y' | 'z';

export interface Transform {
  translate: Vec3;
  rotate: Vec3;
}

export interface ProfileVertex {
  x: number;
  y: number;
  bulge: number;
}

export type Modifier =
  | { type: 'shell'; thickness: number; side: 'inside' | 'outside' | 'centre' }
  | { type: 'offset'; distance: number }
  | { type: 'mirror'; axis: Axis }
  | { type: 'linear'; count: number; step: Vec3 }
  | { type: 'circular'; count: number; axis: Axis; angle: number };

export type NodeKind =
  | { type: 'box'; size: Vec3; round: number }
  | { type: 'cylinder'; radius: number; height: number; round: number }
  | { type: 'sphere'; radius: number }
  | { type: 'capsule'; radius: number; length: number }
  | { type: 'torus'; major: number; minor: number }
  | { type: 'extrude'; profile: ProfileVertex[]; height: number }
  | { type: 'revolve'; profile: ProfileVertex[]; angle: number }
  | {
      type: 'mesh';
      src: string;
      hash: string;
      units: string;
      up: string;
      voxel: number;
      bbox: [number, number, number, number, number, number];
      tris: number;
      patches: number;
      open_edges: number;
    }
  | { type: 'extend'; region: string; dir: Vec3; len: number }
  | { type: 'group'; children: Node[]; modifiers: Modifier[] };

export interface Node {
  id: string;
  op: Op;
  blend: number;
  suppressed: boolean;
  transform: Transform;
  /** This volume's material id when the document assigns one; absent takes the document
   * material. Labelling only — the solve stays single-material, the label rides the 3MF
   * export (validation returns its solve caveat as a warning). */
  material?: string;
  /** This volume's display colour as `#RRGGBB[AA]` (CSS hex). Unset takes the nearest
   * ancestor group's colour. Colouring only — shown in the viewport and stamped as the
   * volume's 3MF `displaycolor`; the spec keeps it render-only, never printed-material
   * colour. */
  color?: string;
  kind: NodeKind;
}

export type Face = 'all' | 'px' | 'nx' | 'py' | 'ny' | 'pz' | 'nz' | 'side' | 'top' | 'bottom';

export interface Selector {
  shape: string;
  face: Face;
  /** Surface-patch id of a mesh node (segmentation ids start at 1). */
  patch?: number;
}

export interface Region {
  id: string;
  selectors: Selector[];
}

export interface Support {
  id: string;
  region: string;
  fixed: [boolean, boolean, boolean];
}

/** Direction a distributed load pushes in: a global vector, or frame components
 * (x, y, then outward normal n) of the load's region face. */
export type LoadDir = { form: 'global'; v: Vec3 } | { form: 'local'; v: Vec3 };

export type LoadKind =
  | { type: 'force'; vector: Vec3 }
  | { type: 'pressure'; mpa: number; expr?: string }
  | { type: 'preload'; newtons: number; axis?: Vec3 }
  | { type: 'distributed'; expr: string; dir: LoadDir; total?: number };

export interface Load {
  id: string;
  region: string;
  case: string;
  kind: LoadKind;
}

export type Family = 'gyroid' | 'primitive' | 'diamond';
export type FillMode = 'sheet' | 'network';

export interface Fill {
  family: Family;
  morph?: Family | null;
  mode: FillMode;
  cell: number;
  cell_min?: number | null;
  skin: number;
  skin_max?: number | null;
  erode?: number | null;
  rho_min: number;
  rho_max: number;
  curve: number;
  min_wall: number;
  protect: string[];
  margin: number;
  case: string | null;
  rotate?: Vec3;
}

export type MaterialRole = 'structural' | 'fluid';
export type PlasticityLaw = 'j2';

export interface LiterateAssert {
  label: string;
  metric: string;
  target: string;
  tolerance: string;
  unit: string;
}

export interface Material {
  id: string;
  name: string;
  e: number;
  nu: number;
  yield_strength: number;
  density: number;
  min_wall: number;
  alpha?: number | null;
  k?: number | null;
  yield_z?: number | null;
  shear_z?: number | null;
  hardening_n?: number | null;
  hardening_k?: number | null;
  weibull_m?: number | null;
  role?: MaterialRole;
  source: string;
}

/** One named scalar (M: .fsm let-variables): the expression as written (canonical text,
 * const-folded where the whole side was literal) and the value it evaluated to at parse.
 * A sweep rewrites `value` and keeps `expr`, so the applied value survives. */
export interface Let {
  name: string;
  expr: string;
  value: number;
}

/** One recorded expression read: the statement attribute an expression drives and the
 * expression to recompute when a swept `let` changes. `target` is "" for readers bound
 * to nothing addressable — fill and solve refuse expressions at parse, so in practice
 * every consumer names a node, load, temperature or convection. */
export interface Consumer {
  target: string;
  attr: string;
  comp: number | null;
  expr: string;
}

export interface Document {
  name: string;
  root: Node[];
  regions: Region[];
  supports: Support[];
  loads: Load[];
  temperatures: Temperature[];
  /** Convective (Robin) bands; the Rust side always serializes the key, and stale
   * intakes (old saved JSON) are backfilled by [withLetTables]. */
  convections: Convection[];
  /** Prescribed heat-flux (Neumann) bands; backfilled like the convections. */
  heat_fluxes: HeatFlux[];
  material: string;
  custom_materials: Material[];
  fill: Fill | null;
  solve: {
    size: number | null;
    modes: number | null;
    t_ref: number | null;
    build?: [number, number] | null;
    plasticity?: PlasticityLaw | null;
    hardening_n?: number | null;
  };
  /** Optional explicit reduced-order coupled system ID bound to this document (`coupled <id>`). */
  coupled_system?: string | null;
  /** Executable verification contracts (`assert ...` or `# @assert:`), preserved across round-trips. */
  asserts?: LiterateAssert[];
  /** Named scalars and the recorded expression reads. Optional because the Rust side
   * omits both when empty (serde skip_serializing_if) — plain documents carry neither
   * key in their JSON; backfilled by [`withLetTables`] at every intake site. */
  lets?: Let[];
  consumed?: Consumer[];
  /** Persistent 2D parametric sketches (`sketch <id> ... { ... }`) and 3D feature bindings (`sketchfeature ...`). */
  sketches?: ParametricSketch[];
  sketch_features?: SketchFeatureBinding[];
}

// ---- Parametric 2D Sketch & Authoritative DocCommand Types --------------------------

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
  | {
      id: string;
      kind: 'distance';
      segment?: string;
      p1?: string;
      p2?: string;
      value: number;
      expr?: string;
    }
  | {
      id: string;
      kind: 'horizontal_distance';
      segment?: string;
      p1?: string;
      p2?: string;
      value: number;
      expr?: string;
    }
  | {
      id: string;
      kind: 'vertical_distance';
      segment?: string;
      p1?: string;
      p2?: string;
      value: number;
      expr?: string;
    }
  | { id: string; kind: 'angle'; seg1: string; seg2?: string; degrees: number; expr?: string }
  | { id: string; kind: 'radius'; target: string; value: number; expr?: string }
  | { id: string; kind: 'diameter'; target: string; value: number; expr?: string };

export type SketchMacro =
  | {
      macro: 'rect';
      id: string;
      x: number;
      y: number;
      width: number;
      height: number;
      fillet?: number;
      construction?: boolean;
    }
  | {
      macro: 'center_rect';
      id: string;
      cx: number;
      cy: number;
      width: number;
      height: number;
      fillet?: number;
      construction?: boolean;
    }
  | { macro: 'circle'; id: string; cx: number; cy: number; radius: number; construction?: boolean }
  | { macro: 'slot'; id: string; x1: number; y1: number; x2: number; y2: number; radius: number }
  | {
      macro: 'polygon';
      id: string;
      cx: number;
      cy: number;
      radius: number;
      sides: number;
      angle_deg?: number;
      construction?: boolean;
    }
  | {
      macro: 'bolt_circle';
      id: string;
      cx: number;
      cy: number;
      pcd: number;
      count: number;
      hole_diameter: number;
      start_angle_deg?: number;
    }
  | { macro: 'fillet_corner'; point_id: string; radius: number };

export interface ParametricSketch {
  id: string;
  plane: SketchPlane;
  points: SketchPoint[];
  segments: SketchSegment[];
  circles: SketchCircle[];
  constraints: SketchConstraint[];
  macros?: SketchMacro[];
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
  point_freedom: PointFreedom[];
  solved_sketch: ParametricSketch;
  outer_profile: ProfileVertex[];
  hole_profiles: ProfileVertex[][];
  diagnosis: string;
}

export type SketchFeatureOp =
  | { kind: 'boss_extrude'; height: number; blend?: number; symmetric?: boolean }
  | { kind: 'pocket_cut'; depth: number; through_all?: boolean; blend?: number }
  | { kind: 'revolve_boss'; angle_deg?: number; blend?: number }
  | { kind: 'revolve_cut'; angle_deg?: number; blend?: number };

export interface SketchFeatureBinding {
  feature_id: string;
  sketch_id: string;
  operation: SketchFeatureOp;
  associate_with_host?: boolean;
}

export interface SketchFeatureRequest {
  feature_id: string;
  sketch_id?: string;
  sketch?: ParametricSketch;
  preset?: string;
  on?: string;
  operation: SketchFeatureOp;
  lets?: Record<string, number>;
  associate_with_host?: boolean;
}

export interface SketchFeatureSummary {
  feature_id: string;
  sketch_id: string;
  created_node_ids: string[];
  plane_description: string;
  world_origin: Vec3;
  world_rotation_deg: Vec3;
  solve_report: SketchSolveReport;
}

export interface SketchPresetMeta {
  id: string;
  title: string;
  subtitle: string;
  default_op: SketchFeatureOp;
  sketch: ParametricSketch;
}

export type DocCommand =
  | { cmd: 'set_material'; material: string; node?: string }
  | { cmd: 'upsert_custom_material'; material: Material }
  | { cmd: 'set_let'; name: string; value: number; expr?: string }
  | { cmd: 'delete_let'; name: string }
  | { cmd: 'upsert_node'; node: Node; parent_group_id?: string }
  | { cmd: 'delete_entity'; id: string }
  | { cmd: 'upsert_region'; id?: string; selectors: Selector[] }
  | { cmd: 'upsert_support'; id?: string; region: string; fixed: [boolean, boolean, boolean] }
  | { cmd: 'upsert_load'; id?: string; region: string; case?: string; kind: LoadKind }
  | {
      cmd: 'upsert_temperature';
      id?: string;
      region: string;
      celsius: number;
      expr?: string;
      case?: string;
    }
  | {
      cmd: 'upsert_convection';
      id?: string;
      region: string;
      h: number;
      t_inf: number;
      h_expr?: string;
      t_inf_expr?: string;
      case?: string;
    }
  | { cmd: 'upsert_heat_flux'; id?: string; region: string; q: number; expr?: string; case?: string }
  | { cmd: 'set_fill'; fill: Fill | null }
  | { cmd: 'set_solve'; solve: Document['solve'] }
  | { cmd: 'upsert_sketch'; sketch: ParametricSketch }
  | { cmd: 'delete_sketch'; id: string }
  | { cmd: 'apply_sketch_feature'; request: SketchFeatureRequest };

export interface DocCommandOutcome {
  entity_id?: string;
  entity_kind?: string;
  action?: string;
  sketch_report?: SketchSolveReport;
  sketch_feature?: SketchFeatureSummary;
  problems: string[];
}

/** A fixed boundary temperature (°C) on a labelled region, one thermal case (M14): the
 * boundary condition of the conduction solve and the driver of thermal expansion from
 * the solve's `t_ref` reference. */
export interface Temperature {
  id: string;
  region: string;
  celsius: number;
  expr?: string;
  case: string;
}

/** A convective (Robin) boundary: the region's facets lose heat as `h·(T − t_inf)` per
 * unit area, with `h` in W/(mm²·K) (5 W/(m²·K) natural convection in air is 5e-6 in
 * these units) read against the solver's pinned solid conductivity k = 1 W/(mm·K), and
 * the ambient or coolant temperature in °C. A case with no temperature region still
 * solves with one `h > 0` — the band lifts the conduction matrix's constant mode and
 * the body settles to `t_inf`. A region may also carry a Temperature in the same case:
 * on shared nodes the fixed value wins (its facets are driven, not convected). */
export interface Convection {
  id: string;
  region: string;
  h: number;
  t_inf: number;
  h_expr?: string;
  t_inf_expr?: string;
  case: string;
}

/** A prescribed heat-flux (Neumann) boundary: the region's facets gain heat at `q` per
 * unit area, positive into the solid, negative out — the only boundary data besides a
 * Temperature or a Convection. `q` is in W/mm² against the solver's pinned conductiv-
 * ity k = 1 W/(mm·K); 1 kW/m² of solar loading is 1e-3 here. A flux band carries no
 * matrix term, so a case whose only boundary data is heat flux has no steady solution
 * (the level is free); it needs a Temperature or an `h > 0` Convection somewhere.
 * Two flux bands on one region in one case superpose legally — their `q` adds. */
export interface HeatFlux {
  id: string;
  region: string;
  q: number;
  expr?: string;
  case: string;
}

export const FACE_LABEL: Record<Face, string> = {
  all: 'All faces',
  px: '+X face',
  nx: '−X face',
  py: '+Y face',
  ny: '−Y face',
  pz: '+Z face',
  nz: '−Z face',
  side: 'Side',
  top: 'Top',
  bottom: 'Bottom',
};

export function emptyDocument(): Document {
  return {
    name: 'Untitled',
    root: [],
    regions: [],
    supports: [],
    loads: [],
    temperatures: [],
    convections: [],
    heat_fluxes: [],
    material: 'petg',
    custom_materials: [],
    fill: null,
    solve: { size: null, modes: null, t_ref: null },
    lets: [],
    consumed: [],
    sketches: [],
    sketch_features: [],
  };
}

/** Backfill the let tables on a document that crossed the wasm boundary (the Rust side
 * omits both when empty), the convections list of a document saved before the
 * convection statements existed, and likewise the heat_fluxes list. Mutates and
 * returns the doc. */
export function withLetTables<T extends Document>(doc: T): T {
  doc.lets ??= [];
  doc.consumed ??= [];
  doc.convections ??= [];
  doc.heat_fluxes ??= [];
  doc.sketches ??= [];
  doc.sketch_features ??= [];
  if (doc.fill) doc.fill.rotate ??= [0, 0, 0];
  return doc;
}

/** Fixed display-colour palette for the Inspector (canonical sRGB `#RRGGBB`). Free-form
 * hex input is out of scope: this palette is the only source of colours, matching what
 * `.fsm` and 3MF carry. */
export const PART_COLORS: string[] = [
  '#21BB4C',
  '#B5432A',
  '#2C7BE5',
  '#F2C842',
  '#8E4FE3',
  '#18A0A8',
  '#E8823C',
  '#D0457F',
  '#6B7280',
  '#1D3B6E',
];

/** Depth-first walk over every node. */
export function walk(nodes: Node[], fn: (n: Node, parent: Node | null, depth: number) => void, parent: Node | null = null, depth = 0) {
  for (const n of nodes) {
    fn(n, parent, depth);
    if (n.kind.type === 'group') walk(n.kind.children, fn, n, depth + 1);
  }
}

export function findNode(doc: Document, id: string): Node | null {
  let found: Node | null = null;
  walk(doc.root, (n) => {
    if (n.id === id) found = n;
  });
  return found;
}

/** The list that directly contains the node with `id` (root or a group's children). */
export function containerOf(doc: Document, id: string): { list: Node[]; index: number; parent: Node | null } | null {
  const rec = (list: Node[], parent: Node | null): { list: Node[]; index: number; parent: Node | null } | null => {
    for (let i = 0; i < list.length; i++) {
      if (list[i].id === id) return { list, index: i, parent };
      const k = list[i].kind;
      if (k.type === 'group') {
        const r = rec(k.children, list[i]);
        if (r) return r;
      }
    }
    return null;
  };
  return rec(doc.root, null);
}

export function allIds(doc: Document): Set<string> {
  const s = new Set<string>();
  walk(doc.root, (n) => s.add(n.id));
  for (const r of doc.regions) s.add(r.id);
  for (const r of doc.supports) s.add(r.id);
  for (const r of doc.loads) s.add(r.id);
  // Temperatures missed this list before the let tables landed; an id is one name
  // space over statements and lets (either direction is "used more than once"), so
  // fresh ids avoid everything named.
  for (const t of doc.temperatures) s.add(t.id);
  for (const c of doc.convections) s.add(c.id);
  for (const f of doc.heat_fluxes) s.add(f.id);
  for (const l of doc.lets ?? []) s.add(l.name);
  for (const sk of doc.sketches ?? []) s.add(sk.id);
  for (const sf of doc.sketch_features ?? []) s.add(sf.feature_id);
  return s;
}

export function freshId(doc: Document, prefix: string): string {
  const taken = allIds(doc);
  for (let i = 1; ; i++) {
    const c = `${prefix}${i}`;
    if (!taken.has(c)) return c;
  }
}

export function kindLabel(k: NodeKind): string {
  switch (k.type) {
    case 'box':
      return 'Box';
    case 'cylinder':
      return 'Cylinder';
    case 'sphere':
      return 'Sphere';
    case 'capsule':
      return 'Capsule';
    case 'torus':
      return 'Torus';
    case 'extrude':
      return 'Extrude';
    case 'revolve':
      return 'Revolve';
    case 'mesh':
      return 'Mesh';
    case 'extend':
      return 'Extend';
    case 'group':
      return 'Group';
  }
}
