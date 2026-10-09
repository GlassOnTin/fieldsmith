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

export interface Document {
  name: string;
  root: Node[];
  regions: Region[];
  supports: Support[];
  loads: Load[];
  material: string;
  custom_materials: Material[];
  fill: Fill | null;
  solve: { size: number | null; modes: number | null };
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
    material: 'petg',
    custom_materials: [],
    fill: null,
    solve: { size: null, modes: null },
  };
}

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
