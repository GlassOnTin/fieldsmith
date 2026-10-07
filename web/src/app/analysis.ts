// The analysis engine: owns the ladder worker, restarts it on document edits, and exposes
// ladder state to the status bar, viewport and results panel. Edits cancel and restart the
// run; the worker dies so a long solve never wedges the session. The Make lens shares this
// worker: export/slice/bytes round-trips go through it (the worker keeps the graded handle
// and the export mesh), replies guard on the same generation counter as the ladder.
import type { ExportMeta, ImportOut, LevelResult, WorkerOut } from '../doc/wasm';
import { store } from '../doc/store';
import { meshcache } from '../doc/meshcache';
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
    const w = this.ensureWorker();
    this.feed(w);
    w.postMessage({ type: 'run', doc: store.doc });
  }

  /** Register the document's cached mesh grids in this worker (copy + transfer: the cache
   * keeps its originals). Extend grids derive from the meshes in-process on the worker
   * side, so parent meshes are all the feed carries. */
  private feed(w: Worker) {
    const entries = meshcache.entriesFor(store.doc);
    if (!entries.length) return;
    const copies = entries.map((e) => ({ ...e, dist: e.dist.slice(), patch: e.patch ? e.patch.slice() : null }));
    w.postMessage(
      { type: 'hydrate', entries: copies },
      copies.flatMap((c) => [c.dist.buffer as ArrayBuffer, ...(c.patch ? [c.patch.buffer as ArrayBuffer] : [])]),
    );
  }

  /** Import a model file in the worker (the voxeliser runs on its rayon pool). The grids
   * arrive in `importDone` and go into the main-thread cache; the splice into the document
   * is the caller's. Rejects with the failure message. */
  importFile(bytes: ArrayBuffer, kind: 'glb' | 'stl' | 'obj', src: string, opts: { units?: string; up?: string; voxel?: number; ids?: string[] } = {}): Promise<ImportOut> {
    if (this.importWait) return Promise.reject('an import is already running');
    return new Promise<ImportOut>((resolve, reject) => {
      this.importWait = { resolve, reject };
      this.ensureWorker().postMessage({ type: 'import', bytes, kind, src, opts }, [bytes]);
    });
  }

  /** An import whose worker is gone mid-flight (edit cancel, poison, replace). */
  private rejectImport(message: string) {
    if (!this.importWait) return;
    const w = this.importWait;
    this.importWait = null;
    w.reject(message);
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
      if (gen !== this.gen) {
        // A replaced worker's last import can no longer be applied to this document —
        // answer the caller instead of leaving the promise hanging.
        if (e.data.type === 'importDone' || e.data.type === 'importError') {
          this.rejectImport('the worker was replaced during the import');
        }
        return;
      }
      this.dispatch(e.data);
    };
    return w;
  }

  /** docVersion the last ladder run was started for ('done'/'error' replies land on it). */
  private launchVersion = 0;

  /** Import call in flight (one at a time; a replaced worker rejects it — the wasm
   * instance it ran in died with the worker). */
  private importWait: { resolve: (r: ImportOut) => void; reject: (e: string) => void } | null = null;

  private dispatch(m: WorkerOut) {
    if (this.importWait && (m.type === 'importDone' || m.type === 'importError')) {
      const wait = this.importWait;
      this.importWait = null;
      if (m.type === 'importDone') {
        for (const g of m.grids) meshcache.put({ ...g, dist: g.dist.slice(), patch: g.patch ? g.patch.slice() : null });
        return wait.resolve(m.result);
      }
      return wait.reject(m.message);
    }
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
    const w = this.ensureWorker();
    this.feed(w);
    w.postMessage({ type: 'export', doc: store.doc, h, solid });
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

  /** Kill the in-flight run (an edit cancels and restarts); an import in flight dies with
   * the worker, so reject its promise here. */
  private cancel() {
    clearTimeout(this.timer);
    this.rejectImport('the import was cancelled');
    if (this.worker) {
      this.worker.terminate();
      this.worker = null;
    }
  }
}

/** Global engine — imported by the lens modules. */
export const analysis = new AnalysisEngine();