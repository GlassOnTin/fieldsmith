// The analysis ladder worker. Runs the wasm pipeline off the main thread and streams each
// level back as it completes; the parent kills the worker to cancel an in-flight run.
// Fill documents run the graded path: analyseGraded delivers the solid mesh, the graded
// re-solve and the fill summary in one call, so each level packs both. The fine level's
// graded handle is kept alive after the run so the Make lens can export the stored fill
// without a re-solve; it dies with the worker on the next run or edit.
import type { Document } from '../doc/types';
import { loadWasm, analyse, analyseGraded, exportSolid, exportGraded } from '../doc/wasm';
import type { AnalysisHandle, GradedHandleWithExport, LevelResult, MeshHandle, WorkerIn, WorkerOut } from '../doc/wasm';

export const COARSE_DOFS = 30_000;
export const FINE_DOFS = 150_000;

let ready: Promise<void> | null = null;

/** The fine (or, when the fine level skipped, the coarse) graded handle of the last run.
 * `null` for solid documents and after this worker has never run a fill document. */
let graded: GradedHandleWithExport | null = null;
/** The export mesh of the last `export` message (both solid and fill exports). */
let mesh: MeshHandle | null = null;

function post(m: WorkerOut, transfers?: ArrayBuffer[]) {
  (self as unknown as { postMessage(m: WorkerOut, transfers: ArrayBuffer[]): void }).postMessage(m, transfers ?? []);
}

/** Copy every array out of the wasm handle, then release it (keep=false keeps it for the
 * Make lens export path). */
function pack(label: string, out: AnalysisHandle & { class?: () => Uint8Array }, keep = false): LevelResult {
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
    regionFacetSets: out.regionFacetSets().slice(),
    regionFacetOffsets: out.regionFacetOffsets().slice(),
    u,
    vmElem,
    vmElemEnvelope: out.vmElemEnvelope().slice(),
    class: out.class ? out.class().slice() : null,
  };
  if (!keep) out.free();
  return r;
}

/** The message carries the buffers by transfer, so nothing is copied across the boundary. */
function transfersOf(r: LevelResult): ArrayBuffer[] {
  return [r.nodes, r.surfaceFacets, r.surfaceOwner, r.surfaceRegion, r.regionFacetSets, r.regionFacetOffsets, r.vmElemEnvelope, ...r.u, ...r.vmElem]
    .map((a) => a.buffer as ArrayBuffer);
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The export mesh is being sliced (wasm runs on this thread to completion). */
let slicing = false;
/** A scrub that arrived mid-slice: answered with the newest z once the running one returns. */
let sliceQueued: number | null = null;

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
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  const fill = doc.fill !== null;
  const size = doc.solve?.size ?? null;

  /** One runLevel: meshes, solves, packs and posts the level; its packed result (or null on
   * failure, with the error already posted). */
  const runLevel = (opts: { h?: number; dofs?: number }, label: string, keep = false): LevelResult | null => {
    post({ type: 'level', label });
    try {
      const out = fill ? analyseGraded(doc, opts) : analyse(doc, opts);
      const level = pack(label, out, keep && fill);
      if (keep && fill) {
        if (graded) graded.free();
        graded = out as GradedHandleWithExport;
      }
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

  const coarse = runLevel({ dofs: COARSE_DOFS }, 'coarse mesh', true);
  if (!coarse) return;
  const skip = skipFine(coarse.meta.provenance.element_size);
  if (skip !== null) {
    post({ type: 'done', skippedFine: skip });
    return;
  }
  const label = size !== null ? `refined · h ${size.toFixed(2)} mm` : `refined · ${Math.round(FINE_DOFS / 1000)}k DOF`;
  if (!runLevel(size !== null ? { h: size } : { dofs: FINE_DOFS }, label, true)) return;
  post({ type: 'done', skippedFine: null });
}

/** Run the Make-lens export. Solid documents (or an explicit solid export) mesh the body
 * field; fill documents reuse the run's stored graded handle when one exists, else build
 * the graded fill on the spot. */
async function doExport(doc: Document, h: number, solid: boolean) {
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  post({ type: 'exportStart', h, solid });
  try {
    let out: MeshHandle;
    if (solid || doc.fill === null) {
      out = exportSolid(doc, h);
    } else if (graded) {
      out = graded.export(h);
    } else {
      out = exportGraded(doc, h);
    }
    if (mesh) mesh.free();
    mesh = out;
    post({ type: 'exportDone', mesh: { meta: JSON.parse(out.metaJson()) } });
  } catch (e) {
    // A trap (out of memory surfaces as a bare `unreachable`) leaves this wasm instance
    // unusable: say so in words and let the engine replace the worker.
    if (e instanceof WebAssembly.RuntimeError) {
      post({
        type: 'exportError',
        trapped: true,
        message: `the export at ${h} mm stopped inside the wasm module (${e.message}) — most likely out of browser memory; try a coarser mesh spacing`,
      });
      return;
    }
    post({ type: 'exportError', message: errText(e) });
  }
}

/** Layer preview: one slice of the export mesh. A scrub posts faster than the wasm answer
 * returns, so only the newest request survives — the older computation is dropped. */
function doSlice(z: number) {
  if (slicing) {
    sliceQueued = z;
    return;
  }
  if (!mesh) {
    post({ type: 'sliceDone', z, segs: null });
    return;
  }
  slicing = true;
  try {
    let segs = mesh.sliceAt(z).slice();
    slicing = false;
    if (sliceQueued !== null) {
      const z2 = sliceQueued;
      sliceQueued = null;
      doSlice(z2);
      return; // newer request already answered the panel
    }
    post({ type: 'sliceDone', z, segs }, [segs.buffer as ArrayBuffer]);
  } catch (e) {
    slicing = false;
    sliceQueued = null;
    post({ type: 'exportError', message: errText(e) });
  }
}

function doBytes(what: 'stl' | '3mf', name: string) {
  if (!mesh) {
    post({ type: 'exportError', message: 'export the mesh first' });
    return;
  }
  // The Uint8Array is a view into wasm memory; copy so the buffer can transfer.
  const bytes = (what === 'stl' ? mesh.stlBytes() : mesh.threeMfBytes()).slice();
  post({ type: 'bytesDone', what, name, bytes: bytes.buffer as ArrayBuffer }, [bytes.buffer as ArrayBuffer]);
}

(self as unknown as { onmessage: ((e: MessageEvent<WorkerIn>) => void) | null }).onmessage = (e) => {
  const m = e.data;
  if (m.type === 'run') void run(m.doc);
  else if (m.type === 'export') void doExport(m.doc, m.h, m.solid);
  else if (m.type === 'slice') doSlice(m.z);
  else if (m.type === 'bytes') doBytes(m.what, m.name);
};