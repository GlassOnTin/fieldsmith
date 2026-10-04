// The analysis engine: owns the ladder worker, restarts it on document edits, and exposes
// ladder state to the status bar, viewport and results panel. Edits cancel and restart the
// run; the worker dies so a long solve never wedges the session. The Make lens shares this
// worker: export/slice/bytes round-trips go through it (the worker keeps the graded handle
// and the export mesh), replies guard on the same generation counter as the ladder.
import type { ExportMeta, LevelResult, WorkerOut } from '../doc/wasm';
import { store } from '../doc/store';
import { ui } from './ui';
import { download } from './commands';

export type AnalysisStatus = 'idle' | 'running' | 'ready' | 'error';

export interface AnalysisState {
  status: AnalysisStatus;
  /** Label of the level being computed, or ''. */
  levelLabel: string;
  /** Completed ladder levels, coarse first. */
  levels: LevelResult[];
  /** performance.now() when each level arrived, aligned with `levels`. */
  levelArrived: number[];
  error: string | null;
  /** 'setup': the study is not analysable yet; 'failure': the solve itself failed. */
  errorKind: 'setup' | 'failure' | null;
  /** Why the refined level was skipped (element-size choice), if it was. */
  skippedFine: string | null;
  /** performance.now() at the start of the current run. */
  startedMs: number;
  /** store.docVersion the finished (or failed) run corresponds to; null mid-run. */
  resultsFor: number | null;
  /** Make-lens export: 'running' while the worker builds, then the health meta with the
   * docVersion it was built for (stale exports are cleared, never shown as current). */
  export: null | 'running' | { meta: ExportMeta; version: number };
  exportError: string | null;
  /** Latest completed plane slice of the export mesh, for the layer preview. */
  slice: { z: number; segs: Float64Array } | null;
}

export class AnalysisEngine {
  state: AnalysisState = {
    status: 'idle',
    levelLabel: '',
    levels: [],
    levelArrived: [],
    error: null,
    errorKind: null,
    skippedFine: null,
    startedMs: 0,
    resultsFor: null,
    export: null,
    exportError: null,
    slice: null,
  };

  private listeners = new Set<() => void>();
  private worker: Worker | null = null;
  private gen = 0;
  private timer = 0;
  /** docVersion the in-flight export was requested for. */
  private makeVersion = 0;

  subscribe(l: () => void): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
  private emit() {
    for (const l of this.listeners) l();
  }
  private set(patch: Partial<AnalysisState>) {
    this.state = { ...this.state, ...patch };
    this.emit();
  }

  /** Newest finished level (refined when present, else coarse). */
  get latest(): LevelResult | null {
    const n = this.state.levels.length;
    return n ? this.state.levels[n - 1] : null;
  }

  /** Latest level's provenance the badge shows. */
  get provenance() {
    return this.latest?.meta.provenance ?? null;
  }

  constructor() {
    store.subscribe(() => {
      clearTimeout(this.timer);
      this.timer = window.setTimeout(() => this.sync(), 150);
    });
  }

  /** A run is wanted when the lens shows setup or results side. The Make lens never
   * triggers the ladder: its export path builds what it needs in the worker. */
  private wanted(): boolean {
    return store.lens === 'setup' || store.lens === 'results' || store.lens === 'fill';
  }
  private sync() {
    // An export built for a superseded document edition is shown nothing, so the stale
    // surface can never masquerade as this doc's export.
    const ex = this.state.export;
    if (ex && ex !== 'running' && ex.version !== store.docVersion) {
      this.set({ export: null, exportError: null, slice: null });
    }
    if (!this.wanted() || this.state.resultsFor === store.docVersion) return;
    this.launch();
  }
  private launch() {
    this.cancel();
    const docVersion = store.docVersion;
    this.launchVersion = docVersion;
    if (ui.probe) ui.set('probe', null);
    this.set({
      status: 'running',
      levelLabel: 'meshing',
      levels: [],
      levelArrived: [],
      error: null,
      errorKind: null,
      skippedFine: null,
      startedMs: performance.now(),
      resultsFor: null,
      export: null,
      exportError: null,
      slice: null,
    });
    this.ensureWorker().postMessage({ type: 'run', doc: store.doc });
  }

  /** A live worker: reuse it (its graded handle and export mesh still serve the Make
   * lens); create one otherwise. The handlers guard on the generation at creation. */
  private ensureWorker(): Worker {
    if (this.worker) return this.worker;
    this.gen++;
    const gen = this.gen;
    const w = new Worker(new URL('../worker/analysis.worker.ts', import.meta.url), { type: 'module' });
    this.worker = w;
    w.onerror = () => {
      if (gen !== this.gen) return;
      this.set({ status: 'error', error: 'the analysis worker failed to start', resultsFor: store.docVersion });
    };
    w.onmessage = (e: MessageEvent<WorkerOut>) => {
      if (gen !== this.gen) return;
      this.dispatch(e.data);
    };
    return w;
  }

  /** docVersion the last ladder run was started for ('done'/'error' replies land on it). */
  private launchVersion = 0;

  private dispatch(m: WorkerOut) {
    if (m.type === 'level') this.set({ levelLabel: m.label });
    else if (m.type === 'levelDone') {
      const levels = [...this.state.levels, m.result];
      this.set({ levels, levelArrived: [...this.state.levelArrived, performance.now()], levelLabel: '' });
    } else if (m.type === 'done') {
      this.set({ status: 'ready', levelLabel: '', resultsFor: this.launchVersion, skippedFine: m.skippedFine });
    } else if (m.type === 'error') {
      this.set({ status: 'error', levelLabel: '', error: m.message, errorKind: m.kind ?? 'failure', resultsFor: this.launchVersion });
    } else if (m.type === 'exportDone') {
      this.set({ export: { meta: m.mesh.meta, version: this.makeVersion } });
    } else if (m.type === 'exportError') {
      if (m.trapped && this.worker) {
        // The instance is poisoned: retire it (the next request spawns a fresh one; the
        // graded handle it held is rebuilt on demand by exportGraded).
        this.worker.terminate();
        this.worker = null;
        this.gen++;
      }
      this.set({ export: null, exportError: m.message });
    } else if (m.type === 'sliceDone') {
      this.set({ slice: m.segs ? { z: m.z, segs: m.segs } : null });
    } else if (m.type === 'bytesDone') {
      download(`${m.name}.${m.what}`, m.bytes, m.what === 'stl' ? 'model/stl' : 'model/3mf');
    }
  }

  /** Make lens: export the part (the body when `solid`, else the stored graded fill). */
  exportMesh(h: number, solid: boolean) {
    const version = store.docVersion;
    this.makeVersion = version;
    this.set({ export: 'running', exportError: null });
    this.ensureWorker().postMessage({ type: 'export', doc: store.doc, h, solid });
  }

  /** Make lens: slice the export mesh at z (latest request wins in the worker). */
  sliceAt(z: number) {
    if (!(this.state.export instanceof Object)) return;
    this.ensureWorker().postMessage({ type: 'slice', z });
  }

  /** Make lens: fetch the export's file bytes and trigger the browser download. */
  downloadMesh(what: 'stl' | '3mf') {
    if (!(this.state.export instanceof Object)) return;
    this.ensureWorker().postMessage({ type: 'bytes', what, name: store.doc.name });
  }

  /** Kill the in-flight run (an edit cancels and restarts). */
  private cancel() {
    clearTimeout(this.timer);
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }
  }
}

/** Global engine — imported by the lens modules. */
export const analysis = new AnalysisEngine();