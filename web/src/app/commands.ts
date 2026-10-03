// Every user action, in one registry: the command palette, keyboard shortcuts and menus all use it.
import { store, LENSES } from '../doc/store';
import type { Lens } from '../doc/store';
import { findNode, emptyDocument } from '../doc/types';
import type { Document, Vec3 } from '../doc/types';
import { parseFsm, printFsm } from '../doc/wasm';
import { SHAPE_TYPES, duplicateNode, groupNode, insertNode, newNode, nudgeNode, removeNode } from '../doc/ops';
import type { ShapeType } from '../doc/ops';
import { ui } from './ui';
import bracketFsm from '../../../examples/bracket.fsm?raw';
import boxCylFsm from '../../../examples/box-minus-cylinder.fsm?raw';

export interface Command {
  id: string;
  group: string;
  label: string;
  /** Display form of the shortcut, e.g. "Ctrl Z". */
  keys?: string;
  enabled?: () => boolean;
  run: () => void;
}

/** Hooks the viewport registers so commands can drive the camera. */
export const viewApi = {
  fit: (_animate?: boolean) => {},
  view: (_dir: Vec3, _up?: Vec3) => {},
  /** A sensible insertion point for new shapes (on the ground under the view target). */
  insertionPoint: (): Vec3 => [0, 0, 0],
  /** Project a model point (mm) to client pixel coordinates. */
  project: (_p: Vec3): { x: number; y: number } => ({ x: 0, y: 0 }),
  /** Gizmo handle under the pointer ('X', 'XY', …) or null. */
  gizmoAxis: (): string | null => null,
};

export const EXAMPLES: { id: string; label: string; text: string }[] = [
  { id: 'bracket', label: 'L-bracket (the bracket test)', text: bracketFsm },
  { id: 'box-minus-cylinder', label: 'Box minus cylinder', text: boxCylFsm },
];

function selectedNodeId(): string | null {
  const s = store.selection;
  return s && s.kind === 'node' && findNode(store.doc, s.id) ? s.id : null;
}

export function addShape(type: ShapeType, opts: { at?: Vec3; op?: 'add' | 'subtract' | 'intersect'; inside?: string } = {}) {
  let id = '';
  const label = SHAPE_TYPES.find((s) => s.type === type)?.label ?? type;
  store.transact(`Add ${label.toLowerCase()}`, (d) => {
    const at = opts.at ?? viewApi.insertionPoint();
    const n = newNode(d, type, at, opts.op ?? 'add');
    if (type !== 'group') n.transform.translate = [at[0], at[1], at[2] + halfHeight(n.kind)];
    id = n.id;
    const sel = opts.inside ?? selectedNodeId();
    const target = sel ? findNode(d, sel) : null;
    insertNode(d, n, sel, !!target && target.kind.type === 'group' && !!opts.inside);
  });
  store.select({ kind: 'node', id });
  return id;
}

function halfHeight(k: ReturnType<typeof newNode>['kind']): number {
  switch (k.type) {
    case 'box':
      return k.size[2] / 2;
    case 'cylinder':
      return k.height / 2;
    case 'sphere':
      return k.radius;
    case 'capsule':
      return k.radius + k.length / 2;
    case 'torus':
      return k.minor;
    case 'extrude':
      return k.height / 2;
    default:
      return 0;
  }
}

export function openText(text: string, fileName?: string): boolean {
  const r = parseFsm(text);
  if ('error' in r) {
    ui.notify(`${fileName ?? 'File'}, line ${r.error.line}: ${r.error.message}`, 'error', 8000);
    return false;
  }
  store.load(r.ok);
  ui.notify(`Opened ${fileName ?? r.ok.name}`);
  return true;
}

export function openFile() {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = '.fsm,text/plain';
  input.onchange = async () => {
    const f = input.files?.[0];
    if (f) openText(await f.text(), f.name);
  };
  input.click();
}

export function fileName(doc: Document): string {
  const base = doc.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'untitled';
  return `${base}.fsm`;
}

export function download(name: string, data: BlobPart, type: string) {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 2000);
}

export function saveFile() {
  const text = printFsm(store.doc);
  download(fileName(store.doc), text, 'text/plain');
  ui.notify(`Saved ${fileName(store.doc)}`);
}

export function newDocument() {
  store.load(emptyDocument());
  ui.notify('New document');
}

function deleteSelection() {
  const s = store.selection;
  if (!s) return;
  if (s.kind === 'node') {
    store.transact(`Delete ${s.id}`, (d) => {
      removeNode(d, s.id);
    });
  } else if (s.kind === 'region') {
    store.transact(`Delete ${s.id}`, (d) => {
      d.regions = d.regions.filter((r) => r.id !== s.id);
      d.supports = d.supports.filter((r) => r.region !== s.id);
      d.loads = d.loads.filter((r) => r.region !== s.id);
    });
  } else if (s.kind === 'support') {
    store.transact(`Delete ${s.id}`, (d) => {
      d.supports = d.supports.filter((r) => r.id !== s.id);
    });
  } else {
    store.transact(`Delete ${s.id}`, (d) => {
      d.loads = d.loads.filter((r) => r.id !== s.id);
    });
  }
  store.select(null);
}

const hasNode = () => selectedNodeId() !== null;

export function commands(): Command[] {
  const list: Command[] = [
    { id: 'file.new', group: 'File', label: 'New document', run: newDocument },
    { id: 'file.open', group: 'File', label: 'Open .fsm file…', keys: 'Ctrl O', run: openFile },
    { id: 'file.save', group: 'File', label: 'Save as .fsm', keys: 'Ctrl S', run: saveFile },
    ...EXAMPLES.map((e) => ({ id: `file.example.${e.id}`, group: 'File', label: `Open example: ${e.label}`, run: () => openText(e.text, `${e.id}.fsm`) })),
    { id: 'edit.undo', group: 'Edit', label: 'Undo', keys: 'Ctrl Z', enabled: () => store.canUndo(), run: () => store.undo() },
    { id: 'edit.redo', group: 'Edit', label: 'Redo', keys: 'Ctrl Shift Z', enabled: () => store.canRedo(), run: () => store.redo() },
    { id: 'edit.delete', group: 'Edit', label: 'Delete selection', keys: 'Del', enabled: () => !!store.selection, run: deleteSelection },
    {
      id: 'edit.duplicate',
      group: 'Edit',
      label: 'Duplicate shape',
      keys: 'Ctrl D',
      enabled: hasNode,
      run: () => {
        let id = '';
        store.transact('Duplicate', (d) => {
          id = duplicateNode(d, selectedNodeId()!)?.id ?? '';
        });
        if (id) store.select({ kind: 'node', id });
      },
    },
    {
      id: 'edit.rename',
      group: 'Edit',
      label: 'Rename selection',
      keys: 'F2',
      enabled: () => !!store.selection,
      run: () => ui.set('renaming', store.selection!.id),
    },
    {
      id: 'edit.suppress',
      group: 'Edit',
      label: 'Suppress / unsuppress shape',
      keys: 'H',
      enabled: hasNode,
      run: () => {
        const id = selectedNodeId()!;
        store.transact(`Toggle ${id}`, (d) => {
          const n = findNode(d, id);
          if (n) n.suppressed = !n.suppressed;
        });
      },
    },
    {
      id: 'edit.group',
      group: 'Edit',
      label: 'Group shape',
      keys: 'Ctrl G',
      enabled: hasNode,
      run: () => {
        let id = '';
        store.transact('Group', (d) => {
          id = groupNode(d, selectedNodeId()!)?.id ?? '';
        });
        if (id) store.select({ kind: 'node', id });
      },
    },
    {
      id: 'edit.up',
      group: 'Edit',
      label: 'Move shape up',
      keys: 'Alt ↑',
      enabled: hasNode,
      run: () => store.transact('Reorder', (d) => void nudgeNode(d, selectedNodeId()!, -1)),
    },
    {
      id: 'edit.down',
      group: 'Edit',
      label: 'Move shape down',
      keys: 'Alt ↓',
      enabled: hasNode,
      run: () => store.transact('Reorder', (d) => void nudgeNode(d, selectedNodeId()!, 1)),
    },
    { id: 'edit.deselect', group: 'Edit', label: 'Clear selection', keys: 'Esc', enabled: () => !!store.selection, run: () => store.select(null) },
    ...SHAPE_TYPES.map((s) => ({ id: `add.${s.type}`, group: 'Add', label: `Add ${s.label.toLowerCase()}`, keys: s.key ? `Shift ${s.key}` : undefined, run: () => void addShape(s.type) })),
    ...(['box', 'cylinder'] as const).map((t) => ({ id: `add.cut.${t}`, group: 'Add', label: `Cut with ${t}`, run: () => void addShape(t, { op: 'subtract' }) })),
    { id: 'view.fit', group: 'View', label: 'Fit model in view', keys: 'F', run: () => viewApi.fit(true) },
    { id: 'view.iso', group: 'View', label: 'Isometric view', keys: '0', run: () => viewApi.view([1, -1, 0.8]) },
    { id: 'view.top', group: 'View', label: 'Top view', keys: '7', run: () => viewApi.view([0, 0, 1], [0, 1, 0]) },
    { id: 'view.front', group: 'View', label: 'Front view', keys: '8', run: () => viewApi.view([0, -1, 0]) },
    { id: 'view.right', group: 'View', label: 'Right view', keys: '9', run: () => viewApi.view([1, 0, 0]) },
    { id: 'tool.move', group: 'Tool', label: 'Move tool', keys: 'W', run: () => ui.set('gizmo', 'translate') },
    { id: 'tool.rotate', group: 'Tool', label: 'Rotate tool', keys: 'E', run: () => ui.set('gizmo', 'rotate') },
    { id: 'tool.snap', group: 'Tool', label: 'Toggle snapping', keys: 'S', run: () => ui.set('snap', !ui.snap) },
    ...LENSES.map((l) => ({ id: `lens.${l.id}`, group: 'Lens', label: `${l.label} lens`, keys: l.key, run: () => store.setLens(l.id as Lens) })),
    { id: 'app.palette', group: 'Help', label: 'Command palette', keys: 'Ctrl K', run: () => ui.set('paletteOpen', true) },
  ];
  return list.concat(extraCommands.flatMap((f) => f()));
}

/** Later milestones register lens-specific commands here. */
export const extraCommands: (() => Command[])[] = [];

export function runCommand(id: string) {
  const c = commands().find((c) => c.id === id);
  if (c && (!c.enabled || c.enabled())) c.run();
}

/** Global keyboard shortcuts. Ignored while typing in an input. */
export function installShortcuts() {
  window.addEventListener('keydown', (e) => {
    const t = e.target as HTMLElement;
    const typing = t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (ctrl && k === 'k') {
      e.preventDefault();
      ui.set('paletteOpen', !ui.paletteOpen);
      return;
    }
    if (typing || ui.paletteOpen) return;
    const map: Record<string, string> = {};
    if (ctrl) {
      if (k === 'z' && e.shiftKey) map.k = 'edit.redo';
      else if (k === 'z') map.k = 'edit.undo';
      else if (k === 'y') map.k = 'edit.redo';
      else if (k === 's') map.k = 'file.save';
      else if (k === 'o') map.k = 'file.open';
      else if (k === 'd') map.k = 'edit.duplicate';
      else if (k === 'g') map.k = 'edit.group';
    } else if (e.altKey) {
      if (e.key === 'ArrowUp') map.k = 'edit.up';
      if (e.key === 'ArrowDown') map.k = 'edit.down';
    } else if (e.shiftKey) {
      const s = SHAPE_TYPES.find((s) => s.key && s.key.toLowerCase() === k);
      if (s) map.k = `add.${s.type}`;
    } else {
      const plain: Record<string, string> = {
        delete: 'edit.delete',
        backspace: 'edit.delete',
        f2: 'edit.rename',
        h: 'edit.suppress',
        escape: 'edit.deselect',
        f: 'view.fit',
        '0': 'view.iso',
        '7': 'view.top',
        '8': 'view.front',
        '9': 'view.right',
        w: 'tool.move',
        e: 'tool.rotate',
        s: 'tool.snap',
      };
      LENSES.forEach((l) => (plain[l.key] = `lens.${l.id}`));
      if (plain[k]) map.k = plain[k];
    }
    if (map.k) {
      e.preventDefault();
      runCommand(map.k);
    }
  });
}
