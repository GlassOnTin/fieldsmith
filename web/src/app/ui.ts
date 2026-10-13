// Ephemeral UI state (not part of the document, not undoable).
import { useEffect, useReducer } from 'preact/hooks';
import { store } from '../doc/store';

type Listener = () => void;

export type GizmoMode = 'translate' | 'rotate';
export type ResultMode = 'stress' | 'safety' | 'deflection' | 'vectors';

export interface Probe {
  value: number;
  unit: string;
  point: [number, number, number];
}

class Ui {
  paletteOpen = false;
  menu: { kind: 'add' | 'file' | 'modifier'; x: number; y: number; target?: string } | null = null;
  toast: { text: string; kind: 'info' | 'error' } | null = null;
  renaming: string | null = null;
  gizmo: GizmoMode = 'translate';
  snap = true;
  gridStep = 1;
  angleStep = 15;
  frameMs = 0;
  compileMs = 0;
  hover: { shape: string | null; face: string | null; point: [number, number, number] } | null = null;
  // Results lens display state (not undoable).
  resultsMode: ResultMode = 'stress';
  /** Case index, −1 = envelope. */
  resultsCase = -1;
  /** Deformed-shape exaggeration factor. */
  resultsScale = 5;
  /** Legend range the user set for one colour mode. Keyed to that mode and the rung it was
   * set on (doc version + rung label), so a mode switch or a new solve falls back to the
   * auto bounds instead of miscolouring a different result with a stale range. */
  resultsRange: { mode: 'stress' | 'deflection'; levelKey: string; lo: number; hi: number } | null =
    null;
  // Make lens display state (not undoable).
  /** Export mesh spacing (mm). */
  makeH = 0.25;
  /** Export the plain body instead of the stored graded fill. */
  makeSolid = false;
  /** Layer-height of the slice preview (mm); layers sit at (k + 0.5)·h within the body. */
  makeLayerH = 1;
  /** Preview layer index (0-based from the body's bottom). */
  makeLayer = 0;
  /** Whether the slice preview contour overlay is shown. */
  makeSliceOn = true;
  // Viewport display state (not undoable, session-only).
  /** Body opacity: 1 = opaque, < 1 renders the body translucent and shows the lattice
   * through it. Resets on load — UI state is not persisted. */
  bodyAlpha = 1;
  probe: Probe | null = null;
  collapsed = new Set<string>();
  version = 0;
  private listeners = new Set<Listener>();
  private toastTimer = 0;

  subscribe(l: Listener) {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
  set<K extends keyof this>(k: K, v: this[K]) {
    if (this[k] === v) return;
    this[k] = v;
    this.emit();
  }
  emit() {
    this.version++;
    for (const l of this.listeners) l();
  }
  notify(text: string, kind: 'info' | 'error' = 'info', ms = 3200) {
    this.toast = { text, kind };
    clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => {
      this.toast = null;
      this.emit();
    }, ms);
    this.emit();
  }
}

export const ui = new Ui();

/** Re-render the calling component when the document store or UI state changes. */
export function useApp() {
  const [, bump] = useReducer((x: number, _: void) => x + 1, 0);
  const force = () => bump(undefined);
  useEffect(() => {
    const a = store.subscribe(force);
    const b = ui.subscribe(force);
    return () => {
      a();
      b();
    };
  }, []);
  return { store, ui };
}
