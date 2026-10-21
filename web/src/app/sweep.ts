// The results lens' parameter sweep (GOAL v0.4 section D & v0.7 M25): the panel drives
// the analysis worker's sweep arm (1D or 2D grid), streams points back with mass and
// structural metrics for the interactive Pareto frontier explorer, and exports the CLI's
// sweep CSV — one row builder, two sources (the engine's own records and the envelope mirror).
import { useEffect, useReducer } from 'preact/hooks';
import { store } from '../doc/store';
import { applyAddrs, parseAddr, readParam, resolveAddr } from '../doc/param';
import { COARSE_DOFS, FINE_DOFS } from '../doc/wasm';
import type { SweepRecord, WorkerOut } from '../doc/wasm';
import { rustFloat } from '../doc/numfmt';
import { analysis } from './analysis';
import { download } from './commands';

export type ParetoMetric = 'vm' | 'safety' | 'compliance' | 'peel' | 'deflection' | 'f1';

export interface SweepPointRow {
  value: number;
  value2?: number;
  values: Record<string, number>;
  mass_g: number;
  layer_peel: number;
  yield_strength: number;
  f1_hz: number | null;
  records: SweepRecord[];
}

export interface SweepState {
  /** The parameter address the current (or last) sweep ran on; '' before any run. */
  param: string;
  /** Optional second parameter address for a 2D grid sweep; '' for 1D. */
  param2: string;
  /** The points that arrived, in value order. A native sweep discards partial work on a
   * point error; the web stream keeps the ones that measured. */
  points: SweepPointRow[];
  /** Total points asked for — the panel's progress line. */
  total: number;
  running: boolean;
  /** The last sweep completed whole (the export button lives on this). */
  done: boolean;
  error: string | null;
  /** store.docVersion the sweep measured; the table clears when an unrelated edition moves on. */
  docVersion: number | null;
  /** Index of the Pareto point currently applied to the document, or null. */
  selectedIndex: number | null;
}

/** Extract the Y-axis metric value from a finished sweep point. */
export function metricValueOf(p: SweepPointRow, metric: ParetoMetric): number {
  const last = p.records[p.records.length - 1];
  const vm = last?.tracked.vm_mpa ?? 0;
  const defl = last?.tracked.deflection_mm ?? 0;
  const compl = last?.tracked.compliance_nmm ?? 0;
  if (metric === 'vm') return vm;
  if (metric === 'safety') return vm > 0 && p.yield_strength > 0 ? p.yield_strength / vm : 0;
  if (metric === 'compliance') return compl;
  if (metric === 'peel') return p.layer_peel;
  if (metric === 'deflection') return defl;
  if (metric === 'f1') return p.f1_hz ?? 0;
  return vm;
}

/** Whether higher values of `metric` are better (true for safety factor and f₁; false for
 * stress, compliance, layer-peel, and deflection). Mass on X is always minimized. */
export function isMaximizedMetric(metric: ParetoMetric): boolean {
  return metric === 'safety' || metric === 'f1';
}

/** Compute the set of non-dominated (Pareto-optimal) point indices for `(mass_g, metric)`. */
export function computeParetoIndices(points: readonly SweepPointRow[], metric: ParetoMetric): Set<number> {
  const maxY = isMaximizedMetric(metric);
  const ys = points.map((p) => metricValueOf(p, metric));
  const front = new Set<number>();
  const eps = 1e-9;
  for (let i = 0; i < points.length; i++) {
    const xi = points[i].mass_g;
    const yi = ys[i];
    let dominated = false;
    for (let j = 0; j < points.length; j++) {
      if (i === j) continue;
      const xj = points[j].mass_g;
      const yj = ys[j];
      const xNoWorse = xj <= xi + eps;
      const yNoWorse = maxY ? yj >= yi - eps : yj <= yi + eps;
      const xStrictlyBetter = xj < xi - eps;
      const yStrictlyBetter = maxY ? yj > yi + eps : yj < yi - eps;
      if (xNoWorse && yNoWorse && (xStrictlyBetter || yStrictlyBetter)) {
        dominated = true;
        break;
      }
    }
    if (!dominated) front.add(i);
  }
  return front;
}

/** The CLI's own header (crates/fieldsmith-api/src/sweep_csv.rs::CSV_HEADER); the web
 * writer and the envelope mirror below produce the same first line byte for byte. */
export const SWEEP_CSV_HEADER = 'value,dofs,elements,wall_ms,deflection_mm,vm_mpa,compliance_nmm,deflection_change_rel,vm_change_rel,compliance_change_rel,status';

/** One CSV row: the last rung's measured cells, printed the way Rust prints them
 * (`f64::to_string` — rustFloat reproduces it inside the decimal window both files name;
 * the e2e evidence checks the actual columns rather than assuming the window). The integer
 * columns print as integers on both sides. */
function csvRow(
  value: string,
  last: {
    dofs: number;
    elements: number;
    wall_ms: number;
    tracked: { deflection_mm: number; vm_mpa: number; compliance_nmm: number };
    change: { deflection_rel: number; vm_rel: number; compliance_rel: number } | null | undefined;
  },
  status: string,
): string {
  return [
    value,
    String(last.dofs),
    String(last.elements),
    rustFloat(last.wall_ms),
    rustFloat(last.tracked.deflection_mm),
    rustFloat(last.tracked.vm_mpa),
    rustFloat(last.tracked.compliance_nmm),
    last.change ? rustFloat(last.change.deflection_rel) : '',
    last.change ? rustFloat(last.change.vm_rel) : '',
    last.change ? rustFloat(last.change.compliance_rel) : '',
    status,
  ].join(',');
}

interface EnvelopeRecord {
  dofs: number;
  elements: number;
  wall_ms: number;
  tracked: { deflection_mm: number; vm_mpa: number; compliance_nmm: number };
  change: { deflection_rel: number; vm_rel: number; compliance_rel: number } | null | undefined;
}

interface SweepEnvelope {
  sweep: {
    param: string;
    points: { value: number; outcome: { records: EnvelopeRecord[]; met: unknown; unmet: unknown } }[];
  };
}

export function sweepCsvFromEnvelope(env: SweepEnvelope): string {
  const sweep = (env as { sweep?: SweepEnvelope['sweep'] }).sweep;
  if (!sweep) {
    throw new Error('sweep csv: the envelope carries no sweep answer (--csv is a sweep-question option)');
  }
  const rows = [SWEEP_CSV_HEADER];
  for (const p of sweep.points) {
    const last = p.outcome.records[p.outcome.records.length - 1];
    if (!last) {
      throw new Error(`sweep csv: a point of ${sweep.param} at value ${rustFloat(p.value)} has no rung records — nothing measured to write`);
    }
    const status = p.outcome.met ? 'met' : p.outcome.unmet ? 'refusal' : 'ok';
    rows.push(csvRow(rustFloat(p.value), last, status));
  }
  return rows.join('\n') + '\n';
}

class SweepEngine {
  state: SweepState = {
    param: '',
    param2: '',
    points: [],
    total: 0,
    running: false,
    done: false,
    error: null,
    docVersion: null,
    selectedIndex: null,
  };

  private listeners = new Set<() => void>();
  private validDocVersions = new Set<number>();
  private baseLoads = -1;
  private applyingPoint = false;

  subscribe(l: () => void): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
  private emit() {
    for (const l of this.listeners) l();
  }
  private set(patch: Partial<SweepState>) {
    this.state = { ...this.state, ...patch };
    this.emit();
  }

  constructor() {
    store.subscribe(() => {
      if (this.applyingPoint) return;
      if (this.state.docVersion !== null && this.state.docVersion !== store.docVersion) {
        if (
          store.loads === this.baseLoads &&
          (this.validDocVersions.has(store.docVersion) || store.lastAction.includes('Apply Pareto point'))
        ) {
          this.validDocVersions.add(store.docVersion);
          const matchIdx = this.findMatchingPointIndex();
          this.set({ docVersion: store.docVersion, selectedIndex: matchIdx });
          return;
        }
        this.validDocVersions.clear();
        this.set({
          param: '',
          param2: '',
          points: [],
          total: 0,
          running: false,
          done: false,
          error: null,
          docVersion: null,
          selectedIndex: null,
        });
      }
    });
  }

  private findMatchingPointIndex(): number | null {
    for (let i = 0; i < this.state.points.length; i++) {
      const entries = Object.entries(this.state.points[i].values);
      if (entries.length === 0) continue;
      let allMatch = true;
      for (const [k, v] of entries) {
        try {
          const cur = readParam(store.doc, parseAddr(k));
          if (cur === null || Math.abs(cur - v) > 1e-9) {
            allMatch = false;
            break;
          }
        } catch {
          allMatch = false;
          break;
        }
      }
      if (allMatch) return i;
    }
    return null;
  }

  /** Run a 1D or 2D parameter sweep on copies of the live document. */
  start(
    param: string,
    from: number,
    to: number,
    count: number,
    param2?: string | null,
    from2?: number,
    to2?: number,
    count2?: number,
  ) {
    const p2 = param2 && param2 !== '' && param2 !== param ? param2 : '';
    try {
      const addr = parseAddr(param);
      resolveAddr(store.doc, addr);
      if (p2) {
        const addr2 = parseAddr(p2);
        resolveAddr(store.doc, addr2);
      }
    } catch (e) {
      this.set({ error: e instanceof Error ? e.message : String(e), running: false });
      return;
    }
    const values =
      count === 1 ? [from] : Array.from({ length: count }, (_, i) => from + ((to - from) * i) / (count - 1));
    const n2 = Math.max(1, Math.round(count2 ?? 2));
    const values2 = p2
      ? n2 === 1
        ? [from2 ?? 0]
        : Array.from({ length: n2 }, (_, i) => (from2 ?? 0) + (((to2 ?? 0) - (from2 ?? 0)) * i) / (n2 - 1))
      : undefined;
    const total = values.length * (values2 ? values2.length : 1);
    this.baseLoads = store.loads;
    this.validDocVersions = new Set([store.docVersion]);
    this.set({
      param,
      param2: p2,
      points: [],
      total,
      running: true,
      done: false,
      error: null,
      docVersion: store.docVersion,
      selectedIndex: null,
    });
    analysis.runSweep((m) => this.accept(m), param, values, [COARSE_DOFS, FINE_DOFS], p2 || undefined, values2);
  }

  private accept(m: WorkerOut) {
    if (m.type === 'sweepPoint') {
      const values = m.values ?? { [this.state.param]: m.value };
      const points: SweepPointRow[] = [
        ...this.state.points,
        {
          value: m.value,
          value2: m.value2,
          values,
          mass_g: m.mass_g ?? 0,
          layer_peel: m.layer_peel ?? 0,
          yield_strength: m.yield_strength ?? 0,
          f1_hz: m.f1_hz ?? null,
          records: m.records,
        },
      ];
      this.set({ points });
    } else if (m.type === 'sweepDone') {
      this.set({ running: false, done: true });
    } else if (m.type === 'sweepError') {
      this.set({ running: false, error: m.message });
    }
  }

  /** Apply a plotted Pareto candidate's parameter values to the live document in a single
   * reversible `store.transact` transaction (`Ctrl+Z` restores the prior document state). */
  applyPoint(index: number) {
    const pt = this.state.points[index];
    if (!pt) return;
    const entries = Object.entries(pt.values);
    if (entries.length === 0) return;
    const pairs = entries.map(([k, v]) => [parseAddr(k), v] as const);
    const summary = entries.map(([k, v]) => `${k}=${rustFloat(v)}`).join(', ');
    this.applyingPoint = true;
    try {
      store.transact(`Apply Pareto point (${summary})`, (d) => {
        applyAddrs(d, pairs, true);
      });
    } finally {
      this.applyingPoint = false;
    }
    this.validDocVersions.add(store.docVersion);
    this.set({ docVersion: store.docVersion, selectedIndex: index });
  }

  /** Export the finished sweep as the CLI's sweep CSV. */
  exportCsv() {
    if (!this.state.done) return;
    const rows = [SWEEP_CSV_HEADER];
    for (const p of this.state.points) {
      const last = p.records[p.records.length - 1];
      if (!last) continue;
      rows.push(csvRow(rustFloat(p.value), last, 'ok'));
    }
    download(`${store.doc.name}-sweep.csv`, rows.join('\n') + '\n', 'text/csv');
  }
}

/** Global sweep engine — imported by the results lens. */
export const sweep = new SweepEngine();

export function useSweep(): SweepState {
  const [, bump] = useReducer((x: number, _: void) => x + 1, 0);
  useEffect(() => sweep.subscribe(bump), []);
  return sweep.state;
}
