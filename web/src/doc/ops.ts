// Document edits used by the UI. All functions mutate a draft inside `store.transact`.
import type { Document, Modifier, Node, NodeKind, Op, Vec3 } from './types';
import { allIds, containerOf, findNode, freshId, walk } from './types';
import { pruneConsumers } from './param';

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
  { type: 'group', label: 'Group', key: 'G' },
];

const PREFIX: Record<ShapeType, string> = {
  box: 'box',
  cylinder: 'cyl',
  sphere: 'sphere',
  capsule: 'capsule',
  torus: 'torus',
  extrude: 'profile',
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

/** Rename any id (node, region, support, load, temperature, convection), updating every
 * reference. */
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
