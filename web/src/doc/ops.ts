// Document edits used by the UI. All functions mutate a draft inside `store.transact`.
import type { Document, Face, Modifier, Node, NodeKind, Op, Vec3 } from './types';
import { allIds, containerOf, findNode, freshId, walk } from './types';
import { pruneConsumers } from './param';
import { upsertLetParam } from './continuum';

/** Shape types the user creates from scratch; `mesh`/`extend` arrive via import and
 *  sidecar load only, so they are not in the Add menu. */
export type ShapeType = Exclude<NodeKind['type'], 'group' | 'mesh' | 'extend'> | 'group';

export const SHAPE_TYPES: { type: ShapeType; label: string; key?: string }[] = [
  { type: 'box', label: 'Box', key: 'B' },
  { type: 'cylinder', label: 'Cylinder', key: 'C' },
  { type: 'sphere', label: 'Sphere' },
  { type: 'capsule', label: 'Capsule' },
  { type: 'torus', label: 'Torus' },
  { type: 'extrude', label: 'Extruded profile' },
  { type: 'revolve', label: 'Revolved profile' },
  { type: 'group', label: 'Group', key: 'G' },
];

const PREFIX: Record<ShapeType, string> = {
  box: 'box',
  cylinder: 'cyl',
  sphere: 'sphere',
  capsule: 'capsule',
  torus: 'torus',
  extrude: 'profile',
  revolve: 'revolve',
  group: 'group',
};

export function defaultKind(type: ShapeType): NodeKind {
  switch (type) {
    case 'box':
      return { type: 'box', size: [20, 20, 20], round: 0 };
    case 'cylinder':
      return { type: 'cylinder', radius: 10, height: 20, round: 0 };
    case 'sphere':
      return { type: 'sphere', radius: 10 };
    case 'capsule':
      return { type: 'capsule', radius: 5, length: 20 };
    case 'torus':
      return { type: 'torus', major: 12, minor: 4 };
    case 'extrude':
      return {
        type: 'extrude',
        height: 10,
        profile: [
          { x: -10, y: -10, bulge: 0 },
          { x: 10, y: -10, bulge: 0 },
          { x: 10, y: 10, bulge: 0.4 },
          { x: -10, y: 10, bulge: 0 },
        ],
      };
    case 'revolve':
      return {
        type: 'revolve',
        angle: 360,
        profile: [
          { x: 6, y: 0, bulge: 0 },
          { x: 16, y: 0, bulge: 0 },
          { x: 16, y: 20, bulge: 0 },
          { x: 6, y: 20, bulge: 0 },
        ],
      };
    case 'group':
      return { type: 'group', children: [], modifiers: [] };
  }
}

export function newNode(doc: Document, type: ShapeType, at: Vec3 = [0, 0, 0], op: Op = 'add'): Node {
  return {
    id: freshId(doc, PREFIX[type]),
    op,
    blend: 0,
    suppressed: false,
    transform: { translate: at, rotate: [0, 0, 0] },
    kind: defaultKind(type),
  };
}

/** Insert after `afterId` (same list), inside group `afterId` if it is a group and `inside`, else at the end of root. */
export function insertNode(doc: Document, node: Node, afterId: string | null, inside = false) {
  if (afterId) {
    const target = findNode(doc, afterId);
    if (inside && target && target.kind.type === 'group') {
      target.kind.children.push(node);
      return;
    }
    const c = containerOf(doc, afterId);
    if (c) {
      c.list.splice(c.index + 1, 0, node);
      return;
    }
  }
  doc.root.push(node);
}

export function removeNode(doc: Document, id: string): Node | null {
  const c = containerOf(doc, id);
  if (!c) return null;
  const [n] = c.list.splice(c.index, 1);
  // Drop references to removed shapes (and their descendants) from regions and fill.
  const gone = new Set<string>();
  walk([n], (m) => gone.add(m.id));
  for (const r of doc.regions) r.selectors = r.selectors.filter((s) => !gone.has(s.shape));
  if (doc.fill) doc.fill.protect = doc.fill.protect.filter((p) => !gone.has(p));
  pruneConsumers(doc, gone);
  return n;
}

function isAncestor(doc: Document, ancestorId: string, id: string): boolean {
  const a = findNode(doc, ancestorId);
  if (!a) return false;
  let found = false;
  walk([a], (n) => {
    if (n.id === id && n !== a) found = true;
  });
  return found;
}

/** Move node `id` relative to `targetId`: before, after, or inside (group). Returns false if illegal. */
export function moveNode(doc: Document, id: string, targetId: string, where: 'before' | 'after' | 'inside'): boolean {
  if (id === targetId || isAncestor(doc, id, targetId)) return false;
  const target = findNode(doc, targetId);
  if (!target || (where === 'inside' && target.kind.type !== 'group')) return false;
  const c = containerOf(doc, id);
  if (!c) return false;
  const [n] = c.list.splice(c.index, 1);
  if (where === 'inside' && target.kind.type === 'group') {
    target.kind.children.push(n);
    return true;
  }
  const t = containerOf(doc, targetId)!;
  t.list.splice(where === 'before' ? t.index : t.index + 1, 0, n);
  return true;
}

/** Move up/down within its list. */
export function nudgeNode(doc: Document, id: string, delta: -1 | 1): boolean {
  const c = containerOf(doc, id);
  if (!c) return false;
  const j = c.index + delta;
  if (j < 0 || j >= c.list.length) return false;
  [c.list[c.index], c.list[j]] = [c.list[j], c.list[c.index]];
  return true;
}

export const ID_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** Rename any id (node, region, support, load, temperature, convection, heat flux),
 * updating every reference. */
export function renameId(doc: Document, from: string, to: string): string | null {
  if (!ID_RE.test(to)) return 'Names start with a letter and use letters, digits and _ only.';
  if (from === to) return null;
  // One name space over statements and lets (allIds), in either direction: a rename to
  // a let name would produce a .fsm the parser refuses ("used more than once").
  const taken = allIds(doc);
  if (taken.has(to)) return `“${to}” is already used.`;
  const rewire = () => {
    for (const c of doc.consumed ?? []) if (c.target === from) c.target = to;
  };
  const node = findNode(doc, from);
  if (node) {
    node.id = to;
    for (const r of doc.regions) for (const s of r.selectors) if (s.shape === from) s.shape = to;
    if (doc.fill) doc.fill.protect = doc.fill.protect.map((p) => (p === from ? to : p));
    rewire();
    return null;
  }
  const region = doc.regions.find((r) => r.id === from);
  if (region) {
    region.id = to;
    for (const s of doc.supports) if (s.region === from) s.region = to;
    for (const l of doc.loads) if (l.region === from) l.region = to;
    for (const t of doc.temperatures) if (t.region === from) t.region = to;
    for (const c of doc.convections) if (c.region === from) c.region = to;
    for (const f of doc.heat_fluxes) if (f.region === from) f.region = to;
    return null;
  }
  const sup = doc.supports.find((r) => r.id === from);
  if (sup) {
    sup.id = to;
    return null;
  }
  const load = doc.loads.find((r) => r.id === from);
  if (load) {
    load.id = to;
    rewire();
    return null;
  }
  const temp = doc.temperatures.find((r) => r.id === from);
  if (temp) {
    temp.id = to;
    rewire();
    return null;
  }
  const conv = doc.convections.find((r) => r.id === from);
  if (conv) {
    conv.id = to;
    rewire();
    return null;
  }
  const flux = doc.heat_fluxes.find((r) => r.id === from);
  if (flux) {
    flux.id = to;
    rewire();
    return null;
  }
  return `Nothing is called “${from}”.`;
}

/** Duplicate a node (deep copy with fresh ids), inserted after the original. */
export function duplicateNode(doc: Document, id: string): Node | null {
  const n = findNode(doc, id);
  if (!n) return null;
  const copy: Node = structuredClone(n);
  const taken = allIds(doc);
  const reid = (m: Node) => {
    const base = m.id.replace(/\d+$/, '') || 'shape';
    let i = 1;
    while (taken.has(`${base}${i}`)) i++;
    m.id = `${base}${i}`;
    taken.add(m.id);
    if (m.kind.type === 'group') m.kind.children.forEach(reid);
  };
  reid(copy);
  insertNode(doc, copy, id);
  return copy;
}

/** Wrap a node in a new group in place. */
export function groupNode(doc: Document, id: string): Node | null {
  const c = containerOf(doc, id);
  if (!c) return null;
  const n = c.list[c.index];
  const g: Node = {
    id: freshId(doc, 'group'),
    op: n.op,
    blend: 0,
    suppressed: false,
    transform: { translate: [0, 0, 0], rotate: [0, 0, 0] },
    kind: { type: 'group', children: [{ ...n, op: 'add' }], modifiers: [] },
  };
  c.list[c.index] = g;
  return g;
}

export function defaultModifier(type: Modifier['type']): Modifier {
  switch (type) {
    case 'shell':
      return { type: 'shell', thickness: 2, side: 'inside' };
    case 'offset':
      return { type: 'offset', distance: 1 };
    case 'mirror':
      return { type: 'mirror', axis: 'x' };
    case 'linear':
      return { type: 'linear', count: 3, step: [20, 0, 0] };
    case 'circular':
      return { type: 'circular', count: 6, axis: 'z', angle: 360 };
  }
}

export const MODIFIER_LABEL: Record<Modifier['type'], string> = {
  shell: 'Shell',
  offset: 'Offset',
  mirror: 'Mirror',
  linear: 'Linear pattern',
  circular: 'Circular pattern',
};

// ---- read helpers -------------------------------------------------------------------

/** Effective display colour per leaf volume id: the node's own colour, else the nearest
 * ancestor group's. Leaves with no colour anywhere above them are omitted (they render in
 * the document base colour). The viewport walks this with the shader's leaf index order. */
export function effectiveLeafColors(doc: Document): Record<string, string> {
  const out: Record<string, string> = {};
  const rec = (nodes: Node[], inherited?: string) => {
    for (const n of nodes) {
      const colour = n.color ?? inherited;
      if (n.kind.type === 'group') rec(n.kind.children, colour);
      else if (colour) out[n.id] = colour;
    }
  };
  rec(doc.root);
  return out;
}

/** The nearest ancestor group that carries its own colour, and what it is — the Inspector's
 * inherited hint and the rule `effectiveLeafColors` applies. `undefined` when nothing above
 * the node has a colour. */
export function inheritedColor(doc: Document, id: string): { from: string; color: string } | undefined {
  let parent = containerOf(doc, id)?.parent ?? null;
  while (parent) {
    if (parent.color) return { from: parent.id, color: parent.color };
    parent = containerOf(doc, parent.id)?.parent ?? null;
  }
  return undefined;
}

// ---- Surface Frame, Edge-Datum Dimensioning & Hierarchical Object Tree Association ----

export function nodeHalfExtents(kind: NodeKind): Vec3 {
  switch (kind.type) {
    case 'box':
      return [0.5 * kind.size[0], 0.5 * kind.size[1], 0.5 * kind.size[2]];
    case 'cylinder':
      return [kind.radius, kind.radius, 0.5 * kind.height];
    case 'sphere':
      return [kind.radius, kind.radius, kind.radius];
    case 'capsule':
      return [kind.radius, kind.radius, 0.5 * kind.length + kind.radius];
    case 'torus':
      return [kind.major + kind.minor, kind.major + kind.minor, kind.minor];
    case 'extrude': {
      let rx = 8;
      let ry = 8;
      for (const v of kind.profile) {
        rx = Math.max(rx, Math.abs(v.x));
        ry = Math.max(ry, Math.abs(v.y));
      }
      return [rx, ry, 0.5 * kind.height];
    }
    case 'revolve': {
      let rMax = 8;
      let zMax = 8;
      for (const v of kind.profile) {
        rMax = Math.max(rMax, Math.abs(v.x));
        zMax = Math.max(zMax, Math.abs(v.y));
      }
      return [rMax, rMax, zMax];
    }
    case 'mesh': {
      const [x0, y0, z0, x1, y1, z1] = kind.bbox;
      return [0.5 * Math.abs(x1 - x0), 0.5 * Math.abs(y1 - y0), 0.5 * Math.abs(z1 - z0)];
    }
    case 'group': {
      const addKids = kind.children.filter((c) => c.op === 'add' && !c.suppressed);
      const list = addKids.length > 0 ? addKids : kind.children;
      if (list.length === 0) return [10, 10, 5];
      let maxX = 4;
      let maxY = 4;
      let maxZ = 2;
      for (const c of list) {
        const [cx, cy, cz] = nodeHalfExtents(c.kind);
        maxX = Math.max(maxX, Math.abs(c.transform.translate[0]) + cx);
        maxY = Math.max(maxY, Math.abs(c.transform.translate[1]) + cy);
        maxZ = Math.max(maxZ, Math.abs(c.transform.translate[2]) + cz);
      }
      return [maxX, maxY, maxZ];
    }
    default:
      return [15, 15, 8];
  }
}

export interface HostSurfaceFrame {
  hostId: string;
  face: Face;
  hostCenter: Vec3;
  halfExtents: Vec3;
  faceCenter: Vec3;
  outwardNormal: Vec3;
  uAxis: Vec3;
  vAxis: Vec3;
  uAxisIndex: 0 | 1 | 2;
  vAxisIndex: 0 | 1 | 2;
  nAxisIndex: 0 | 1 | 2;
  uLabel: 'X' | 'Y' | 'Z';
  vLabel: 'X' | 'Y' | 'Z';
  nLabel: string;
  faceWidth: number;
  faceHeight: number;
  hostThickness: number;
  rotateDeg: Vec3;
  /** Convert face-centre coordinates `(u, v)` and normal offset `nOff` into 3D position. */
  pointFromUV: (u: number, v: number, nOff?: number) => Vec3;
  /** Convert edge-inset distances `(dx, dy)` from the `-U` and `-V` face edges into 3D position. */
  pointFromEdge: (dx: number, dy: number, nOff?: number) => Vec3;
  /** Recover `(u, v, dx, dy, normalOffset)` on this face from any 3D point. */
  uvFromPoint: (pt: Vec3) => {
    u: number;
    v: number;
    dx: number;
    dy: number;
    normalOffset: number;
  };
}

export function resolveHostSurfaceFrame(
  doc: Document,
  hostId: string,
  face: Face = 'top',
): HostSurfaceFrame {
  const host = findNode(doc, hostId) ?? doc.root.find((n) => n.op === 'add') ?? null;
  let effectiveTarget: Node | null = host;
  if (host && host.kind.type === 'group') {
    const primaryChild = host.kind.children.find((c) => c.op === 'add' && c.kind.type !== 'group');
    if (primaryChild && host.transform.translate.every((x) => x === 0)) {
      effectiveTarget = primaryChild;
    }
  }
  const at: Vec3 = effectiveTarget ? [...effectiveTarget.transform.translate] : [0, 0, 0];
  const [hx, hy, hz] = effectiveTarget ? nodeHalfExtents(effectiveTarget.kind) : [20, 20, 10];

  let outwardNormal: Vec3 = [0, 0, 1];
  let uAxis: Vec3 = [1, 0, 0];
  let vAxis: Vec3 = [0, 1, 0];
  let uAxisIndex: 0 | 1 | 2 = 0;
  let vAxisIndex: 0 | 1 | 2 = 1;
  let nAxisIndex: 0 | 1 | 2 = 2;
  let uLabel: 'X' | 'Y' | 'Z' = 'X';
  let vLabel: 'X' | 'Y' | 'Z' = 'Y';
  let nLabel = '+Z (Top)';
  let faceWidth = 2 * hx;
  let faceHeight = 2 * hy;
  let hostThickness = 2 * hz;
  let dist = hz;
  let rotateDeg: Vec3 = [0, 0, 0];

  switch (face) {
    case 'top':
    case 'pz':
    case 'all':
      outwardNormal = [0, 0, 1];
      uAxis = [1, 0, 0];
      vAxis = [0, 1, 0];
      uAxisIndex = 0;
      vAxisIndex = 1;
      nAxisIndex = 2;
      uLabel = 'X';
      vLabel = 'Y';
      nLabel = '+Z (Top)';
      faceWidth = 2 * hx;
      faceHeight = 2 * hy;
      hostThickness = 2 * hz;
      dist = hz;
      rotateDeg = [0, 0, 0];
      break;
    case 'bottom':
    case 'nz':
      outwardNormal = [0, 0, -1];
      uAxis = [1, 0, 0];
      vAxis = [0, 1, 0];
      uAxisIndex = 0;
      vAxisIndex = 1;
      nAxisIndex = 2;
      uLabel = 'X';
      vLabel = 'Y';
      nLabel = '−Z (Bottom)';
      faceWidth = 2 * hx;
      faceHeight = 2 * hy;
      hostThickness = 2 * hz;
      dist = hz;
      rotateDeg = [180, 0, 0];
      break;
    case 'px':
    case 'side':
      outwardNormal = [1, 0, 0];
      uAxis = [0, 1, 0];
      vAxis = [0, 0, 1];
      uAxisIndex = 1;
      vAxisIndex = 2;
      nAxisIndex = 0;
      uLabel = 'Y';
      vLabel = 'Z';
      nLabel = '+X (Right)';
      faceWidth = 2 * hy;
      faceHeight = 2 * hz;
      hostThickness = 2 * hx;
      dist = hx;
      rotateDeg = [0, 90, 0];
      break;
    case 'nx':
      outwardNormal = [-1, 0, 0];
      uAxis = [0, 1, 0];
      vAxis = [0, 0, 1];
      uAxisIndex = 1;
      vAxisIndex = 2;
      nAxisIndex = 0;
      uLabel = 'Y';
      vLabel = 'Z';
      nLabel = '−X (Left)';
      faceWidth = 2 * hy;
      faceHeight = 2 * hz;
      hostThickness = 2 * hx;
      dist = hx;
      rotateDeg = [0, -90, 0];
      break;
    case 'py':
      outwardNormal = [0, 1, 0];
      uAxis = [1, 0, 0];
      vAxis = [0, 0, 1];
      uAxisIndex = 0;
      vAxisIndex = 2;
      nAxisIndex = 1;
      uLabel = 'X';
      vLabel = 'Z';
      nLabel = '+Y (Back)';
      faceWidth = 2 * hx;
      faceHeight = 2 * hz;
      hostThickness = 2 * hy;
      dist = hy;
      rotateDeg = [-90, 0, 0];
      break;
    case 'ny':
      outwardNormal = [0, -1, 0];
      uAxis = [1, 0, 0];
      vAxis = [0, 0, 1];
      uAxisIndex = 0;
      vAxisIndex = 2;
      nAxisIndex = 1;
      uLabel = 'X';
      vLabel = 'Z';
      nLabel = '−Y (Front)';
      faceWidth = 2 * hx;
      faceHeight = 2 * hz;
      hostThickness = 2 * hy;
      dist = hy;
      rotateDeg = [90, 0, 0];
      break;
  }

  const faceCenter: Vec3 = [
    at[0] + outwardNormal[0] * dist,
    at[1] + outwardNormal[1] * dist,
    at[2] + outwardNormal[2] * dist,
  ];

  const round3 = (v: number) => Math.round(v * 1000) / 1000;

  const pointFromUV = (u: number, v: number, nOff = 0): Vec3 => [
    round3(faceCenter[0] + uAxis[0] * u + vAxis[0] * v + outwardNormal[0] * nOff),
    round3(faceCenter[1] + uAxis[1] * u + vAxis[1] * v + outwardNormal[1] * nOff),
    round3(faceCenter[2] + uAxis[2] * u + vAxis[2] * v + outwardNormal[2] * nOff),
  ];

  const pointFromEdge = (dx: number, dy: number, nOff = 0): Vec3 => {
    const u = -0.5 * faceWidth + dx;
    const v = -0.5 * faceHeight + dy;
    return pointFromUV(u, v, nOff);
  };

  const uvFromPoint = (pt: Vec3) => {
    const rel: Vec3 = [pt[0] - faceCenter[0], pt[1] - faceCenter[1], pt[2] - faceCenter[2]];
    const u = round3(rel[0] * uAxis[0] + rel[1] * uAxis[1] + rel[2] * uAxis[2]);
    const v = round3(rel[0] * vAxis[0] + rel[1] * vAxis[1] + rel[2] * vAxis[2]);
    const normalOffset = round3(
      rel[0] * outwardNormal[0] + rel[1] * outwardNormal[1] + rel[2] * outwardNormal[2],
    );
    const dx = round3(0.5 * faceWidth + u);
    const dy = round3(0.5 * faceHeight + v);
    return { u, v, dx, dy, normalOffset };
  };

  return {
    hostId: effectiveTarget?.id ?? hostId,
    face,
    hostCenter: at,
    halfExtents: [hx, hy, hz],
    faceCenter,
    outwardNormal,
    uAxis,
    vAxis,
    uAxisIndex,
    vAxisIndex,
    nAxisIndex,
    uLabel,
    vLabel,
    nLabel,
    faceWidth,
    faceHeight,
    hostThickness,
    rotateDeg,
    pointFromUV,
    pointFromEdge,
    uvFromPoint,
  };
}

/** Find a companion node ID for a fixing pair (`<prefix>` <-> `<prefix>_bore`). */
export function findCompanionFixingNode(doc: Document, id: string): Node | null {
  if (id.endsWith('_bore')) {
    const base = id.slice(0, -5);
    return findNode(doc, base);
  }
  return findNode(doc, `${id}_bore`);
}

/** Associate newly created child nodes (such as a fixing's `<prefix>_bore` and `<prefix>` groups,
 * or a surface-sketch feature) with a host part so a natural hierarchical Object Tree develops.
 * - If `host` is already a `group`, appends `childNodes` to `host.kind.children`.
 * - If `host` is inside a parent `group` (e.g. `base_assy`), appends `childNodes` to that parent `group`.
 * - If `host` is a root primitive and `wrapHostInAssyGroup` is true, wraps `host` + `childNodes`
 *   into a clean `<hostId>_assy` assembly group in `doc.root`. */
export function associateNodesWithHost(
  doc: Document,
  hostId: string | undefined,
  childNodes: Node[],
  wrapHostInAssyGroup = false,
): Node | null {
  if (!hostId) {
    doc.root.push(...childNodes);
    return null;
  }
  const host = findNode(doc, hostId);
  if (!host) {
    doc.root.push(...childNodes);
    return null;
  }
  if (host.kind.type === 'group') {
    host.kind.children.push(...childNodes);
    return host;
  }
  const cont = containerOf(doc, host.id);
  if (cont?.parent && cont.parent.kind.type === 'group') {
    cont.parent.kind.children.push(...childNodes);
    return cont.parent;
  }
  if (wrapHostInAssyGroup && cont) {
    const baseAssy = `${host.id}_assy`;
    const assyId = allIds(doc).has(baseAssy) ? freshId(doc, baseAssy) : baseAssy;
    const assyGroup: Node = {
      id: assyId,
      op: host.op,
      blend: 0,
      suppressed: false,
      transform: { translate: [0, 0, 0], rotate: [0, 0, 0] },
      kind: {
        type: 'group',
        children: [{ ...host, op: 'add' }, ...childNodes],
        modifiers: [],
      },
    };
    cont.list[cont.index] = assyGroup;
    return assyGroup;
  }
  if (cont) {
    cont.list.splice(cont.index + 1, 0, ...childNodes);
  } else {
    doc.root.push(...childNodes);
  }
  return null;
}

/** Nest an existing node (and its companion `_bore` node, if any) under a host part's assembly group. */
export function nestExistingNodeWithHost(
  doc: Document,
  nodeId: string,
  hostId: string,
): Node | null {
  if (nodeId === hostId || isAncestor(doc, nodeId, hostId)) return null;
  const node = findNode(doc, nodeId);
  const host = findNode(doc, hostId);
  if (!node || !host) return null;

  const companion = findCompanionFixingNode(doc, nodeId);
  const idsToMove = companion
    ? companion.op === 'subtract'
      ? [companion.id, node.id]
      : [node.id, companion.id]
    : [node.id];

  // Ensure host is or resides in an assembly group
  let targetGroup: Node | null = null;
  if (host.kind.type === 'group') {
    targetGroup = host;
  } else {
    const hCont = containerOf(doc, host.id);
    if (hCont?.parent && hCont.parent.kind.type === 'group') {
      targetGroup = hCont.parent;
    } else if (hCont) {
      const baseAssy = `${host.id}_assy`;
      const assyId = allIds(doc).has(baseAssy) ? freshId(doc, baseAssy) : baseAssy;
      const assyGroup: Node = {
        id: assyId,
        op: host.op,
        blend: 0,
        suppressed: false,
        transform: { translate: [0, 0, 0], rotate: [0, 0, 0] },
        kind: {
          type: 'group',
          children: [{ ...host, op: 'add' }],
          modifiers: [],
        },
      };
      hCont.list[hCont.index] = assyGroup;
      targetGroup = assyGroup;
    }
  }
  if (!targetGroup || targetGroup.kind.type !== 'group') return null;

  for (const mid of idsToMove) {
    if (mid === targetGroup.id) continue;
    const c = containerOf(doc, mid);
    if (!c || c.parent?.id === targetGroup.id) continue;
    const [removed] = c.list.splice(c.index, 1);
    targetGroup.kind.children.push(removed);
  }
  return targetGroup;
}

/** Un-nest a node from its parent group back up to `doc.root` (or the parent group's container). */
export function promoteNodeOutOfGroup(doc: Document, nodeId: string): boolean {
  const c = containerOf(doc, nodeId);
  if (!c || !c.parent) return false;
  const parentCont = containerOf(doc, c.parent.id);
  if (!parentCont) return false;
  const [removed] = c.list.splice(c.index, 1);
  parentCont.list.splice(parentCont.index + 1, 0, removed);
  return true;
}

function rotateVec3EulerDeg(v: Vec3, rotDeg: Vec3): Vec3 {
  const rx = (rotDeg[0] * Math.PI) / 180;
  const ry = (rotDeg[1] * Math.PI) / 180;
  const rz = (rotDeg[2] * Math.PI) / 180;
  const cx = Math.cos(rx), sx = Math.sin(rx);
  const cy = Math.cos(ry), sy = Math.sin(ry);
  const cz = Math.cos(rz), sz = Math.sin(rz);
  // Rx
  const y1 = v[1] * cx - v[2] * sx;
  const z1 = v[1] * sx + v[2] * cx;
  const x1 = v[0];
  // Ry
  const x2 = x1 * cy + z1 * sy;
  const z2 = -x1 * sy + z1 * cy;
  const y2 = y1;
  // Rz
  const x3 = x2 * cz - y2 * sz;
  const y3 = x2 * sz + y2 * cz;
  const z3 = z2;
  const r3 = (n: number) => Math.round(n * 1000) / 1000;
  return [r3(x3), r3(y3), r3(z3)];
}

/** Dissolve a group node in-place and keep all of its child objects at the group's position in the tree,
 * preserving each child's world transform, boolean operation (e.g. `subtract` for bore groups), colour, and material. */
export function ungroupNode(doc: Document, groupId: string): Node[] {
  const c = containerOf(doc, groupId);
  if (!c) return [];
  const g = c.list[c.index];
  if (!g || g.kind.type !== 'group') return [];
  const [gx, gy, gz] = g.transform.translate;
  const gRot = g.transform.rotate;
  const hasRot = Math.abs(gRot[0]) > 1e-6 || Math.abs(gRot[1]) > 1e-6 || Math.abs(gRot[2]) > 1e-6;
  const r3 = (n: number) => Math.round(n * 1000) / 1000;

  const promotedChildren: Node[] = g.kind.children.map((ch) => {
    const copy = structuredClone(ch);
    const rel = hasRot ? rotateVec3EulerDeg(copy.transform.translate, gRot) : copy.transform.translate;
    copy.transform.translate = [r3(gx + rel[0]), r3(gy + rel[1]), r3(gz + rel[2])];
    if (hasRot) {
      copy.transform.rotate = [
        r3(copy.transform.rotate[0] + gRot[0]),
        r3(copy.transform.rotate[1] + gRot[1]),
        r3(copy.transform.rotate[2] + gRot[2]),
      ];
    }
    if (g.op === 'subtract') {
      copy.op = 'subtract';
    }
    if (!copy.color && g.color) copy.color = g.color;
    if (!copy.material && g.material) copy.material = g.material;
    return copy;
  });

  c.list.splice(c.index, 1, ...promotedChildren);
  pruneConsumers(doc, new Set([groupId]));
  return promotedChildren;
}

/** Delete a node (or group and all its descendants) and optionally its companion fixing node (`<id>_bore` <-> `<id>`). */
export function removeNodeAndCompanion(
  doc: Document,
  id: string,
  includeCompanion = false,
): string[] {
  const removedIds: string[] = [];
  const companion = includeCompanion ? findCompanionFixingNode(doc, id) : null;
  const r1 = removeNode(doc, id);
  if (r1) removedIds.push(r1.id);
  if (companion) {
    const r2 = removeNode(doc, companion.id);
    if (r2) removedIds.push(r2.id);
  }
  return removedIds;
}

/** Upsert a `Consumer` expression in `doc.consumed` so `target.attr[.comp]` is driven by `expr`. */
export function upsertConsumerExpr(
  doc: Document,
  target: string,
  attr: string,
  comp: number | null,
  expr: string,
): void {
  if (!doc.consumed) doc.consumed = [];
  const existing = doc.consumed.find(
    (c) => c.target === target && c.attr === attr && c.comp === comp,
  );
  if (existing) {
    existing.expr = expr;
  } else {
    doc.consumed.push({ target, attr, comp, expr });
  }
}

/** Position any shape or group onto a specific face of a host part using either
 * edge-inset dimensions `(dx, dy)` or face-centre offsets `(u, v)`, with optional
 * parametric `let` binding and hierarchical Object Tree association. */
export function positionNodeOnHostSurface(
  doc: Document,
  nodeId: string,
  opts: {
    hostId: string;
    face: Face;
    mode: 'edge' | 'center';
    dx?: number;
    dy?: number;
    u?: number;
    v?: number;
    seating: 'flush' | 'centered' | 'recessed';
    normalOffset?: number;
    bindLets?: boolean;
    associateInTree?: boolean;
  },
): { pt: Vec3; rotateDeg: Vec3; frame: HostSurfaceFrame } | null {
  const node = findNode(doc, nodeId);
  if (!node) return null;
  const frame = resolveHostSurfaceFrame(doc, opts.hostId, opts.face);
  const [, , childHz] = nodeHalfExtents(node.kind);

  const companion = findCompanionFixingNode(doc, nodeId);
  const isFixingPair = companion !== null;

  let nOff = opts.normalOffset ?? 0;
  if (!isFixingPair) {
    if (opts.seating === 'flush') nOff += childHz;
    else if (opts.seating === 'recessed') nOff -= childHz;
  }

  const dx = opts.dx ?? 0.5 * frame.faceWidth;
  const dy = opts.dy ?? 0.5 * frame.faceHeight;
  const u = opts.u ?? 0;
  const v = opts.v ?? 0;

  const pt =
    opts.mode === 'edge'
      ? frame.pointFromEdge(dx, dy, nOff)
      : frame.pointFromUV(u, v, nOff);

  const targets = companion ? [node, companion] : [node];
  for (const t of targets) {
    t.transform.translate = [...pt];
    t.transform.rotate = [...frame.rotateDeg];
  }

  if (opts.bindLets) {
    const cleanPrefix = (nodeId.endsWith('_bore') ? nodeId.slice(0, -5) : nodeId).replace(
      /[^a-zA-Z0-9_]/g,
      '_',
    );
    const fmtNum = (val: number) => Number(val.toFixed(4));
    if (opts.mode === 'edge') {
      const dxName = `${cleanPrefix}_dx`;
      const dyName = `${cleanPrefix}_dy`;
      upsertLetParam(doc, dxName, fmtNum(dx));
      upsertLetParam(doc, dyName, fmtNum(dy));
      const baseU = fmtNum(frame.faceCenter[frame.uAxisIndex] - 0.5 * frame.faceWidth);
      const baseV = fmtNum(frame.faceCenter[frame.vAxisIndex] - 0.5 * frame.faceHeight);
      const exprU = baseU === 0 ? dxName : `${baseU} + ${dxName}`;
      const exprV = baseV === 0 ? dyName : `${baseV} + ${dyName}`;
      for (const t of targets) {
        upsertConsumerExpr(doc, t.id, 'at', frame.uAxisIndex, exprU);
        upsertConsumerExpr(doc, t.id, 'at', frame.vAxisIndex, exprV);
      }
    } else {
      const uName = `${cleanPrefix}_u`;
      const vName = `${cleanPrefix}_v`;
      upsertLetParam(doc, uName, fmtNum(u));
      upsertLetParam(doc, vName, fmtNum(v));
      const baseU = fmtNum(frame.faceCenter[frame.uAxisIndex]);
      const baseV = fmtNum(frame.faceCenter[frame.vAxisIndex]);
      const exprU = baseU === 0 ? uName : `${baseU} + ${uName}`;
      const exprV = baseV === 0 ? vName : `${baseV} + ${vName}`;
      for (const t of targets) {
        upsertConsumerExpr(doc, t.id, 'at', frame.uAxisIndex, exprU);
        upsertConsumerExpr(doc, t.id, 'at', frame.vAxisIndex, exprV);
      }
    }
  }

  if (opts.associateInTree) {
    nestExistingNodeWithHost(doc, nodeId, opts.hostId);
  }

  return { pt, rotateDeg: frame.rotateDeg, frame };
}
