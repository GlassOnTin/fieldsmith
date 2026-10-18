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
  | { type: 'pressure'; mpa: number }
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
  mode: FillMode;
  cell: number;
  skin: number;
  rho_min: number;
  rho_max: number;
  curve: number;
  min_wall: number;
  protect: string[];
  margin: number;
  case: string | null;
}

export interface Material {
  id: string;
  name: string;
  e: number;
  nu: number;
  yield_strength: number;
  density: number;
  min_wall: number;
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
  material: string;
  custom_materials: Material[];
  fill: Fill | null;
  solve: { size: number | null; modes: number | null; t_ref: number | null };
  /** Named scalars and the recorded expression reads. Optional because the Rust side
   * omits both when empty (serde skip_serializing_if) — plain documents carry neither
   * key in their JSON; backfilled by [`withLetTables`] at every intake site. */
  lets?: Let[];
  consumed?: Consumer[];
}

/** A fixed boundary temperature (°C) on a labelled region, one thermal case (M14): the
 * boundary condition of the conduction solve and the driver of thermal expansion from
 * the solve's `t_ref` reference. */
export interface Temperature {
  id: string;
  region: string;
  celsius: number;
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
    material: 'petg',
    custom_materials: [],
    fill: null,
    solve: { size: null, modes: null, t_ref: null },
    lets: [],
    consumed: [],
  };
}

/** Backfill the let tables on a document that crossed the wasm boundary (the Rust side
 * omits both when empty) and the convections list of a document saved before the
 * convection statements existed. Mutates and returns the doc. */
export function withLetTables<T extends Document>(doc: T): T {
  doc.lets ??= [];
  doc.consumed ??= [];
  doc.convections ??= [];
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
  for (const l of doc.lets ?? []) s.add(l.name);
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
    case 'mesh':
      return 'Mesh';
    case 'extend':
      return 'Extend';
    case 'group':
      return 'Group';
  }
}
