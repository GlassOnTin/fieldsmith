// The analysis engine: owns the ladder worker, restarts it on document edits, and exposes
// ladder state to the status bar, viewport and results panel. Edits cancel and restart the
// run; the worker dies so a long solve never wedges the session.
import type { LevelResult, WorkerOut } from '../doc/wasm';
import { store } from '../doc/store';
import { ui } from './ui';

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
  };

  private listeners = new Set<() => void>();
  private worker: Worker | null = null;
  private gen = 0;
  private timer = 0;

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

  /** A run is wanted when the lens shows setup or results side. */
  private wanted(): boolean {
    return store.lens === 'setup' || store.lens === 'results' || store.lens === 'fill';
  }
  private sync() {
    if (!this.wanted() || this.state.resultsFor === store.docVersion) return;
    this.launch();
  }
  private launch() {
    this.cancel();
    this.gen++;
    const gen = this.gen;
    const docVersion = store.docVersion;
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
    });
    const w = new Worker(new URL('../worker/analysis.worker.ts', import.meta.url), { type: 'module' });
    this.worker = w;
    w.onerror = () => {
      if (gen !== this.gen) return;
      this.set({ status: 'error', error: 'the analysis worker failed to start', resultsFor: store.docVersion });
    };
    w.onmessage = (e: MessageEvent<WorkerOut>) => {
      if (gen !== this.gen) return;
      const m = e.data;
      if (m.type === 'level') this.set({ levelLabel: m.label });
      else if (m.type === 'levelDone') {
        const levels = [...this.state.levels, m.result];
        this.set({ levels, levelArrived: [...this.state.levelArrived, performance.now()], levelLabel: '' });
      } else if (m.type === 'done') {
        this.set({ status: 'ready', levelLabel: '', resultsFor: docVersion, skippedFine: m.skippedFine });
      } else if (m.type === 'error') {
        this.set({ status: 'error', levelLabel: '', error: m.message, errorKind: m.kind ?? 'failure', resultsFor: docVersion });
      }
    };
    w.postMessage({ type: 'run', doc: store.doc });
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