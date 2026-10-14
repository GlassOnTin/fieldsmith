// Application state: the document (with undo/redo), selection and UI lens.
import type { Document } from './types';
import { emptyDocument } from './types';

export type Lens = 'shape' | 'setup' | 'results' | 'fill' | 'make';
export const LENSES: { id: Lens; label: string; key: string }[] = [
  { id: 'shape', label: 'Shape', key: '1' },
  { id: 'setup', label: 'Setup', key: '2' },
  { id: 'results', label: 'Results', key: '3' },
  { id: 'fill', label: 'Fill', key: '4' },
  { id: 'make', label: 'Make', key: '5' },
];

/** What is selected: a model node, or an analysis item (region / support / load /
 * temperature). */
export type Selection = { kind: 'node'; id: string } | { kind: 'region'; id: string } | { kind: 'support'; id: string } | { kind: 'load'; id: string } | { kind: 'temperature'; id: string } | null;

type Listener = () => void;

const AUTOSAVE_KEY = 'fieldsmith.autosave.v1';
const HISTORY_LIMIT = 200;

export class Store {
  doc: Document = emptyDocument();
  selection: Selection = null;
  lens: Lens = 'shape';
  /** Incremented on every change; components re-render when it moves. */
  version = 0;
  /** Incremented only when the document changes (not selection / lens). */
  docVersion = 0;
  private past: { doc: Document; label: string }[] = [];
  private future: { doc: Document; label: string }[] = [];
  private listeners = new Set<Listener>();
  /** Snapshot taken at the start of a transient (drag) edit. */
  private transientBase: Document | null = null;
  lastAction = '';
  /** Incremented when a whole document is loaded (open / new / example) — the view refits. */
  loads = 0;

  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  private emit(docChanged: boolean) {
    this.version++;
    if (docChanged) {
      this.docVersion++;
      this.autosave();
    }
    for (const l of this.listeners) l();
  }

  /** Replace the whole document (open / new). Clears history. */
  load(doc: Document) {
    this.doc = doc;
    this.past = [];
    this.future = [];
    this.selection = null;
    this.transientBase = null;
    this.loads++;
    this.emit(true);
  }

  /** Apply an undoable edit. `fn` mutates a draft copy. */
  transact(label: string, fn: (draft: Document) => void) {
    const base = this.transientBase ?? this.doc;
    const draft = structuredClone(this.transientBase ? this.doc : this.doc);
    fn(draft);
    if (JSON.stringify(draft) === JSON.stringify(base) && !this.transientBase) return;
    this.past.push({ doc: base, label });
    if (this.past.length > HISTORY_LIMIT) this.past.shift();
    this.future = [];
    this.transientBase = null;
    this.doc = draft;
    this.lastAction = label;
    this.emit(true);
  }

  /** Live edit during a drag: updates the document without recording history until `commit`. */
  transient(fn: (draft: Document) => void) {
    if (!this.transientBase) this.transientBase = this.doc;
    const draft = structuredClone(this.doc);
    fn(draft);
    this.doc = draft;
    this.emit(true);
  }

  commitTransient(label: string) {
    if (!this.transientBase) return;
    const base = this.transientBase;
    this.transientBase = null;
    if (JSON.stringify(base) === JSON.stringify(this.doc)) return;
    this.past.push({ doc: base, label });
    this.future = [];
    this.lastAction = label;
    this.emit(true);
  }

  cancelTransient() {
    if (!this.transientBase) return;
    this.doc = this.transientBase;
    this.transientBase = null;
    this.emit(true);
  }

  canUndo() {
    return this.past.length > 0;
  }
  canRedo() {
    return this.future.length > 0;
  }
  undoLabel() {
    return this.past[this.past.length - 1]?.label ?? '';
  }
  redoLabel() {
    return this.future[this.future.length - 1]?.label ?? '';
  }

  undo() {
    const prev = this.past.pop();
    if (!prev) return;
    this.future.push({ doc: this.doc, label: prev.label });
    this.doc = prev.doc;
    this.lastAction = `Undo ${prev.label}`;
    this.fixSelection();
    this.emit(true);
  }

  redo() {
    const next = this.future.pop();
    if (!next) return;
    this.past.push({ doc: this.doc, label: next.label });
    this.doc = next.doc;
    this.lastAction = `Redo ${next.label}`;
    this.fixSelection();
    this.emit(true);
  }

  select(sel: Selection) {
    this.selection = sel;
    this.emit(false);
  }

  setLens(lens: Lens) {
    if (this.lens === lens) return;
    this.lens = lens;
    this.emit(false);
  }

  /** Drop a selection that no longer exists after undo/redo. */
  private fixSelection() {
    const s = this.selection;
    if (!s) return;
    const exists =
      s.kind === 'node'
        ? JSON.stringify(this.doc.root).includes(`"id":"${s.id}"`)
        : s.kind === 'region'
          ? this.doc.regions.some((r) => r.id === s.id)
          : s.kind === 'support'
            ? this.doc.supports.some((r) => r.id === s.id)
            : s.kind === 'temperature'
              ? this.doc.temperatures.some((r) => r.id === s.id)
              : this.doc.loads.some((r) => r.id === s.id);
    if (!exists) this.selection = null;
  }

  private autosave() {
    try {
      localStorage.setItem(AUTOSAVE_KEY, JSON.stringify(this.doc));
    } catch {
      /* storage unavailable or full: autosave is best-effort */
    }
  }

  static restore(): Document | null {
    try {
      const s = localStorage.getItem(AUTOSAVE_KEY);
      return s ? (JSON.parse(s) as Document) : null;
    } catch {
      return null;
    }
  }
}

export const store = new Store();
