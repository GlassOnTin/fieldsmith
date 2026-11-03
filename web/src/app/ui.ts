// Ephemeral UI state (not part of the document, not undoable).
import { useEffect, useReducer } from 'preact/hooks';
import { store } from '../doc/store';

type Listener = () => void;

export type GizmoMode = 'translate' | 'rotate';
export type ResultMode = 'stress' | 'safety' | 'peel' | 'deflection' | 'vectors' | 'temperature';
export type ResultColormap = 'viridis' | 'flame' | 'turbo' | 'coolwarm';

/** Deformed-shape exaggeration the panel starts at (the ×5 reset target). */
export const RESULTS_SCALE_DEFAULT = 5;

export interface Probe {
  value: number;
  unit: string;
  point: [number, number, number];
}

class Ui {
  paletteOpen = false;
  /** Coupled Engineering Systems Workbench modal/overlay (V52). */
  systemsOpen = typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('systems');
  /** Interactive 3-Column Tutorials & Theoretical Manual Drawer (T1–T9). */
  tutorialsOpen = typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('tutorials');
  tutorialSheetId = 'T1';
  /** MCP & REST API Co-Pilot Setup guide modal from the footer. */
  mcpSetupOpen = false;
  /** Narrow-screen drawer state. Only meaningful under the ≤900 px layout, where the tree and
   * inspector panels overlay the viewport as slide-in drawers instead of grid columns; at
   * desktop widths the toggles and scrim are hidden by CSS and these fields are inert. */
  treeOpen = false;
  inspOpen = false;
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
  /** ParaView / VTK scalar transfer function colourmap. */
  resultsColormap: ResultColormap = 'viridis';
  /** Render translucent 3D volumetric iso-shells / glowing scalar cloud in Results. */
  resultsVolumeOn = false;
  /** Case index, −1 = envelope. */
  resultsCase = -1;
  /** Deformed-shape exaggeration factor. */
  resultsScale = RESULTS_SCALE_DEFAULT;
  /** Legend range the user set for one colour mode. Keyed to that mode and the rung it was
   * set on (doc version + rung label), so a mode switch or a new solve falls back to the
   * auto bounds instead of miscolouring a different result with a stale range. The auto
   * lower bound is 0 for stress/deflection and the data minimum for temperature. */
  resultsRange: { mode: 'stress' | 'peel' | 'deflection' | 'temperature'; levelKey: string; lo: number; hi: number } | null =
    null;
  // Mode-shape animation (a modal run's shapes in the Results lens; not undoable).
  /** Which mode animates, 0-based; −1 = none — the mesh shows the static results.
   * Reset to −1 after any solve whose run carries fewer shapes. */
  resultsModeShape = -1;
  /** The animation phase in radians; cycles 0 → 2π. The shown displacement is
   * φ_k · cos(phase) and the colour is |φ_k · cos(phase)|. */
  resultsModePhase = 0;
  /** Play the phase forward (a rAF ticker in results.tsx advances it per frame). */
  resultsModePlay = false;
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
  makeSliceOn = false;
  /** Whether the exact-gradient overhang overlay (> 45° from +Z) is shown in Make. */
  makeOverhangOn = false;
  // Sweep panel display state (not undoable). The parameter defaults to the document's
  // first sweepable one (null until the user picks); from/to fall back to that
  // parameter's own value — the question is the user's to fill.
  sweepParam: string | null = null;
  /** The sweep's from value; null = the parameter's current value. */
  sweepFrom: number | null = null;
  /** The sweep's to value; null = the parameter's current value. */
  sweepTo: number | null = null;
  /** How many values the sweep steps from → to. */
  sweepCount = 5;
  /** Optional second parameter for 2D grid sweeps (M25); null or '' = 1D sweep. */
  sweepParam2: string | null = null;
  sweepFrom2: number | null = null;
  sweepTo2: number | null = null;
  sweepCount2 = 2;
  /** Y-axis metric for the Results Pareto frontier explorer (M25). */
  paretoMetric: 'vm' | 'safety' | 'compliance' | 'peel' | 'deflection' | 'f1' = 'vm';
  // Viewport display state (not undoable, session-only).
  /** Body opacity: 1 = opaque, < 1 renders the body translucent and shows the lattice
   * through it. Resets on load — UI state is not persisted. */
  bodyAlpha = 1;
  /** Section plane: axis-aligned cut sweeping through the part. `t` ∈ [0, 1] sweeps the
   * plane along the axis inside the field bounds; t = 0 restores the intact body in both
   * directions. Display-only like bodyAlpha — resets on load. */
  clip: { on: boolean; axis: 0 | 1 | 2; dir: 1 | -1; t: number } = { on: false, axis: 0, dir: 1, t: 0 };
  /** A rendering failure the viewport cannot show (loud rendering failures, VISION §7): a
   * shader whose program will not link, a document whose field has no surface inside its
   * own bounds, or a results mesh whose build threw. Persistent until the next state
   * change clears it — unlike the toast, it stays while the blank viewport stays. */
  renderError: { kind: 'shader' | 'surface' | 'results'; text: string } | null = null;
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
