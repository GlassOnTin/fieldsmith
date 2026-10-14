// The results lens' parameter sweep (GOAL v0.4 section D): the panel drives the analysis
// worker's sweep arm (a mirror of the native Sweep question), the rows stream back per
// point, and the export writes the same CSV the CLI's `api --csv` writes — one row
// builder, two sources (the engine's own records and the envelope mirror below).
import { useEffect, useReducer } from 'preact/hooks';
import { store } from '../doc/store';
import { parseAddr, resolveAddr } from '../doc/param';
import { COARSE_DOFS, FINE_DOFS } from '../doc/wasm';
import type { SweepRecord, WorkerOut } from '../doc/wasm';
import { rustFloat } from '../doc/numfmt';
import { analysis } from './analysis';
import { download } from './commands';

export interface SweepPointRow {
  value: number;
  records: SweepRecord[];
}

export interface SweepState {
  /** The parameter address the current (or last) sweep ran on; '' before any run. */
  param: string;
  /** The points that arrived, in value order. A native sweep discards partial work on a
   * point error; the web stream keeps the ones that measured. */
  points: SweepPointRow[];
  /** Total points asked for — the panel's progress line. */
  total: number;
  running: boolean;
  /** The last sweep completed whole (the export button lives on this). */
  done: boolean;
  error: string | null;
  /** store.docVersion the sweep measured; the table clears when the edition moves on. */
  docVersion: number | null;
}

/** The CLI's own header (crates/fieldsmith-api/src/sweep_csv.rs::CSV_HEADER); the web
 * writer and the envelope mirror below produce the same first line byte for byte. */
export const SWEEP_CSV_HEADER = 'value,dofs,elements,wall_ms,deflection_mm,vm_mpa,deflection_change_rel,vm_change_rel,status';

/** One CSV row: the last rung's measured cells, printed the way Rust prints them
 * (`f64::to_string` — rustFloat reproduces it inside the decimal window both files name;
 * the e2e evidence checks the actual columns rather than assuming the window). The integer
 * columns print as integers on both sides. */
function csvRow(
  value: string,
  last: { dofs: number; elements: number; wall_ms: number; tracked: { deflection_mm: number; vm_mpa: number }; change: { deflection_rel: number; vm_rel: number } | null | undefined },
  status: string,
): string {
  return [
    value,
    String(last.dofs),
    String(last.elements),
    rustFloat(last.wall_ms),
    rustFloat(last.tracked.deflection_mm),
    rustFloat(last.tracked.vm_mpa),
    last.change ? rustFloat(last.change.deflection_rel) : '',
    last.change ? rustFloat(last.change.vm_rel) : '',
    status,
  ].join(',');
}

/** The sweep answer as the API envelope serializes it: `RungRecord` with the tracked
 * maxima and (null on rung 0, no baseline) the change estimate. */
interface EnvelopeRecord {
  dofs: number;
  elements: number;
  wall_ms: number;
  tracked: { deflection_mm: number; vm_mpa: number };
  change: { deflection_rel: number; vm_rel: number } | null | undefined;
}

interface SweepEnvelope {
  sweep: {
    param: string;
    points: { value: number; outcome: { records: EnvelopeRecord[]; met: unknown; unmet: unknown } }[];
  };
}

/** Byte-identical mirror of `sweep_csv::sweep_csv` (crates/fieldsmith-api/src/sweep_csv.rs):
 * one row per point, the last rung's measured numbers, change columns empty on a
 * single-rung point, the verdict column met/refusal/ok. The cells print from the envelope's
 * parsed numbers — JSON.parse is the correctly rounding client the CSV's byte promise is
 * written against, unlike serde_json's own parse (the measured 1-ulp drift pinned in
 * sweep_csv.rs's tests). Throws the native text when the envelope carries no sweep answer. */
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
    points: [],
    total: 0,
    running: false,
    done: false,
    error: null,
    docVersion: null,
  };

  private listeners = new Set<() => void>();
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
    // A document edition the sweep did not measure takes its table away — the same
    // staleness rule the analysis engine applies to lens exports, applied to the sweep's
    // measured points. (An edit mid-run also cancels the run itself: the analysis engine
    // restarts, and its cancel closes the sweep with words.)
    store.subscribe(() => {
      if (this.state.docVersion !== null && this.state.docVersion !== store.docVersion) {
        this.set({ param: '', points: [], total: 0, running: false, done: false, error: null, docVersion: null });
      }
    });
  }

  /** Run the sweep: the address resolves against the live document first (its request
   * error is the panel's own message), the values step `from`..`to`, the rung schedule is
   * the ladder's own coarse/refined constants (the worker mirrors the native default), and
   * every point runs on a document copy — the live document keeps its values. */
  start(param: string, from: number, to: number, count: number) {
    try {
      const addr = parseAddr(param);
      resolveAddr(store.doc, addr);
    } catch (e) {
      this.set({ error: e instanceof Error ? e.message : String(e), running: false });
      return;
    }
    const values = count === 1 ? [from] : Array.from({ length: count }, (_, i) => from + ((to - from) * i) / (count - 1));
    this.set({ param, points: [], total: values.length, running: true, done: false, error: null, docVersion: store.docVersion });
    analysis.runSweep((m) => this.accept(m), param, values, [COARSE_DOFS, FINE_DOFS]);
  }

  private accept(m: WorkerOut) {
    if (m.type === 'sweepPoint') {
      // Points stand even when a later point errors — the stream is the web sweep's
      // difference from the native one, which discards partial work (see the worker).
      const points = [...this.state.points, { value: m.value, records: m.records }];
      this.set({ points });
    } else if (m.type === 'sweepDone') {
      this.set({ running: false, done: true });
    } else if (m.type === 'sweepError') {
      this.set({ running: false, error: m.message });
    }
  }

  /** Export the finished sweep as the CLI's sweep CSV. The engine writes its own rows
   * with the shared builder; sweepCsvFromEnvelope prints the envelope's digits — the e2e
   * evidence holds the two paths to the same bytes. */
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