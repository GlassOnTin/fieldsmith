// The analysis ladder worker. Runs the wasm pipeline off the main thread and streams each
// level back as it completes; the parent kills the worker to cancel an in-flight run.
// Fill documents run the graded path: analyseGraded delivers the solid mesh, the graded
// re-solve and the fill summary in one call, so each level packs both.
import type { Document } from '../doc/types';
import { loadWasm, analyse, analyseGraded } from '../doc/wasm';
import type { AnalysisHandle, LevelResult, WorkerIn, WorkerOut } from '../doc/wasm';

export const COARSE_DOFS = 30_000;
export const FINE_DOFS = 150_000;

let ready: Promise<void> | null = null;

function post(m: WorkerOut, transfers?: ArrayBuffer[]) {
  (self as unknown as { postMessage(m: WorkerOut, transfers: ArrayBuffer[]): void }).postMessage(m, transfers ?? []);
}

/** Copy every array out of the wasm handle, then release it. */
function pack(label: string, out: AnalysisHandle & { class?: () => Uint8Array }): LevelResult {
  const meta = JSON.parse(out.metaJson());
  const u: Float64Array[] = [];
  const vmElem: Float64Array[] = [];
  for (let c = 0; c < meta.cases.length; c++) {
    u.push(out.u(c).slice());
    vmElem.push(out.vmElem(c).slice());
  }
  const r: LevelResult = {
    label,
    meta,
    nodes: out.nodes().slice(),
    surfaceFacets: out.surfaceFacets().slice(),
    surfaceOwner: out.surfaceOwner().slice(),
    surfaceRegion: out.surfaceRegion().slice(),
    u,
    vmElem,
    vmElemEnvelope: out.vmElemEnvelope().slice(),
    class: out.class ? out.class().slice() : null,
  };
  out.free();
  return r;
}

/** The message carries the buffers by transfer, so nothing is copied across the boundary. */
function transfersOf(r: LevelResult): ArrayBuffer[] {
  return [r.nodes, r.surfaceFacets, r.surfaceOwner, r.surfaceRegion, r.vmElemEnvelope, ...r.u, ...r.vmElem]
    .map((a) => a.buffer as ArrayBuffer);
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function run(doc: Document) {
  // Setup validation before wasm: an incomplete study is not a failure, and the badge
  // renders these as a neutral hint rather than the red 'analysis failed'.
  if (!doc.supports.length || !doc.loads.length) {
    post({
      type: 'error',
      kind: 'setup',
      message: !doc.supports.length
        ? 'add a support: the part needs something to hold it'
        : 'add a load',
    });
    return;
  }
  if (!ready) ready = loadWasm();
  await ready;
  const fill = doc.fill !== null;
  const size = doc.solve?.size ?? null;

  /** One runLevel: meshes, solves, packs and posts the level; its packed result (or null on
   * failure, with the error already posted). */
  const runLevel = (opts: { h?: number; dofs?: number }, label: string): LevelResult | null => {
    post({ type: 'level', label });
    try {
      const level = pack(label, fill ? analyseGraded(doc, opts) : analyse(doc, opts));
      post({ type: 'levelDone', result: level }, transfersOf(level));
      return level;
    } catch (e) {
      // The coarse level is already useful — report the failure without discarding it.
      post({ type: 'error', message: errText(e) });
      return null;
    }
  };
  const skipFine = (coarseSize: number): string | null => {
    if (size !== null && size >= coarseSize - 0.01) {
      // The chosen element size is already coarser than the coarse pass delivered; a second
      // pass at that size would repeat the coarse result.
      return `chosen element size ${size.toFixed(2)} mm is coarser than the coarse pass (${coarseSize.toFixed(2)} mm)`;
    }
    return null;
  };

  const coarse = runLevel({ dofs: COARSE_DOFS }, 'coarse mesh');
  if (!coarse) return;
  const skip = skipFine(coarse.meta.provenance.element_size);
  if (skip !== null) {
    post({ type: 'done', skippedFine: skip });
    return;
  }
  const label = size !== null ? `refined · h ${size.toFixed(2)} mm` : `refined · ${Math.round(FINE_DOFS / 1000)}k DOF`;
  if (!runLevel(size !== null ? { h: size } : { dofs: FINE_DOFS }, label)) return;
  post({ type: 'done', skippedFine: null });
}

(self as unknown as { onmessage: ((e: MessageEvent<WorkerIn>) => void) | null }).onmessage = (e) => {
  const m = e.data;
  if (m.type === 'run') void run(m.doc);
};