// The analysis ladder worker. Runs the wasm pipeline off the main thread and streams each
// level back as it completes; the parent kills the worker to cancel an in-flight run.
// Fill documents run the graded path: analyseGraded delivers the solid mesh, the graded
// re-solve and the fill summary in one call, so each level packs both. The fine level's
// graded handle is kept alive after the run so the Make lens can export the stored fill
// without a re-solve; it dies with the worker on the next run or edit.
import type { Document, Node } from '../doc/types';
import { walk } from '../doc/types';
import { loadWasm, analyse, analyseGraded, exportSolid, exportGraded, importMesh, meshGrid, hydrateGrid, meshHydrated } from '../doc/wasm';
import type { AnalysisHandle, AnalysisMeta, GradedHandleWithExport, HydrateEntry, LevelResult, MeshHandle, WorkerIn, WorkerOut } from '../doc/wasm';
// This file once carried its own copies of the rung schedule; the sweep's mirror of the
// native question engine needs the same constants, and one definition is one number.
import { COARSE_DOFS, FINE_DOFS } from '../doc/wasm';
import { applyAddr, parseAddr, resolveAddr } from '../doc/param';
import { rustFloat } from '../doc/numfmt';
import type { SweepRecord } from '../doc/wasm';

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
 * Make lens export path). `t0` is when the level's leg began, so `runMs` — the host's one
 * measurement — covers the wasm mesh+solve and the copies to the main thread. */
function pack(
  label: string,
  out: AnalysisHandle & {
    class?: () => Uint8Array;
    fillParamSpec?: () => Float64Array;
    fillParamDims?: () => Uint32Array;
    fillParamData?: () => Float32Array;
    thermalU?: (i: number) => Float64Array;
    thermalVmElem?: (i: number) => Float64Array;
    thermalTemperature?: (i: number) => Float64Array;
  },
  keep = false,
  t0 = performance.now(),
): LevelResult {
  const meta = JSON.parse(out.metaJson());
  const u: Float64Array[] = [];
  const vmElem: Float64Array[] = [];
  const temperature: (Float64Array | null)[] = [];
  for (let c = 0; c < meta.cases.length; c++) {
    u.push(out.u(c).slice());
    vmElem.push(out.vmElem(c).slice());
    const t = out.temperature(c);
    temperature.push(t.length ? t : null);
  }
  // Fill levels: the wasm meta keeps the graded thermal cases in `thermal_cases` while
  // the solid level's thermal cases ride its own `cases`. Pack them into the same level
  // slots — mechanical first, thermal after, the solid path's own order — so the Results
  // lens sees one case list with the same index space and names on every level, and the
  // temperature slots carry the graded conduction fields (the per-node max across slots
  // keeps the envelope semantics). A level without thermal slots must not fall through
  // silently: an empty field would read as "no thermal stress" downstream.
  const thermalCases = meta.thermal_cases ?? [];
  for (let i = 0; i < thermalCases.length; i++) {
    if (!out.thermalU || !out.thermalVmElem || !out.thermalTemperature) {
      throw new Error(`level '${label}': meta carries thermal case '${thermalCases[i].name}' but the handle has no thermal getters`);
    }
    u.push(out.thermalU(i).slice());
    vmElem.push(out.thermalVmElem(i).slice());
    const t = out.thermalTemperature(i);
    temperature.push(t.length ? t : null);
  }
  if (thermalCases.length) {
    meta.cases = [...meta.cases, ...thermalCases];
    delete meta.thermal_cases;
  }
  const nodes = out.nodes().slice();
  // The mode shapes (solid handle only — the graded handle has none): one concatenated
  // buffer, block k the shape of mode k. Every shape matches the mesh's node count.
  let modeShapes: Float64Array | null = null;
  const modeCount = out.modeShapeCount?.() ?? 0;
  if (modeCount > 0) {
    const per = nodes.length;
    const all = new Float64Array(modeCount * per);
    for (let k = 0; k < modeCount; k++) {
      const s = out.modeShape ? out.modeShape(k) : new Float64Array(0);
      if (s.length !== per) {
        throw new Error(`mode shape ${k + 1}: ${s.length} components, expected ${per}`);
      }
      all.set(s, k * per);
    }
    modeShapes = all;
  }
  const fillParam =
    out.fillParamData && out.fillParamSpec && out.fillParamDims
      ? {
          spec: out.fillParamSpec().slice(),
          dims: out.fillParamDims().slice(),
          data: out.fillParamData().slice(),
        }
      : null;
  const r: LevelResult = {
    label,
    meta,
    nodes,
    surfaceFacets: out.surfaceFacets().slice(),
    surfaceOwner: out.surfaceOwner().slice(),
    surfaceRegion: out.surfaceRegion().slice(),
    regionFacetSets: out.regionFacetSets().slice(),
    regionFacetOffsets: out.regionFacetOffsets().slice(),
    u,
    vmElem,
    temperature,
    vmElemEnvelope: out.vmElemEnvelope().slice(),
    modeShapes,
    class: out.class ? out.class().slice() : null,
    fillParam,
    runMs: performance.now() - t0,
  };
  if (!keep) out.free();
  return r;
}

/** The message carries the buffers by transfer, so nothing is copied across the boundary. */
function transfersOf(r: LevelResult): ArrayBuffer[] {
  const ls: (Uint8Array | Float64Array | Uint32Array | Float32Array)[] = [r.nodes, r.surfaceFacets, r.surfaceOwner, r.surfaceRegion, r.regionFacetSets, r.regionFacetOffsets, r.vmElemEnvelope, ...r.u, ...r.vmElem];
  for (const t of r.temperature) if (t) ls.push(t);
  if (r.modeShapes) ls.push(r.modeShapes);
  if (r.fillParam) ls.push(r.fillParam.spec, r.fillParam.dims, r.fillParam.data);
  return ls.map((a) => a.buffer as ArrayBuffer);
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The export mesh is being sliced (wasm runs on this thread to completion). */
let slicing = false;
/** A scrub that arrived mid-slice: answered with the newest z once the running one returns. */
let sliceQueued: number | null = null;

/** The mesh hashes the document carries (its own and sidecar-loaded). */
function docMeshHashes(doc: Document): string[] {
  const out: string[] = [];
  walk(doc.root, (n: Node) => {
    if (n.kind.type === 'mesh') out.push(n.kind.hash);
  });
  return out;
}

/** Mesh runs need every mesh grid in the worker's registry. Unparented meshes compile to
 * vacuum and the export would silently be a hollow shell, so runs refuse instead. */
function unhydratedHash(doc: Document): string | null {
  for (const hash of docMeshHashes(doc)) {
    if (!meshHydrated(hash)) return hash;
  }
  return null;
}

/** Import: voxelise in the worker (rayon) and post the nodes/reports plus the grids for the
 * main-thread cache. The worker's own registry is hydrated by `importMesh` itself. */
async function doImport(bytes: ArrayBuffer, kind: 'glb' | 'stl' | 'obj' | '3mf', src: string, opts: { units?: string; up?: string; voxel?: number; ids?: string[] }) {
  // The import can be the first message a fresh worker sees, so wasm is not loaded yet.
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  try {
    const result = importMesh(new Uint8Array(bytes), kind, { ...opts, src });
    const grids: HydrateEntry[] = [];
    const transfers: ArrayBuffer[] = [];
    for (const p of result.parts) {
      const g = meshGrid(p.hash);
      const dist = g.dist();
      const patchData = g.patchData();
      const patch = patchData ? patchData : null;
      grids.push({
        key: g.keyHex(),
        origin: [...g.origin()] as [number, number, number],
        spacing: g.spacing(),
        dims: [...g.dims()] as [number, number, number],
        bbox: p.bbox,
        tris: p.tris,
        patches: p.patches,
        open_edges: p.open_edges,
        dist,
        patch,
      });
      transfers.push(dist.buffer as ArrayBuffer);
      if (patch) transfers.push(patch.buffer as ArrayBuffer);
    }
    post({ type: 'importDone', result, grids }, transfers);
  } catch (e) {
    post({ type: 'importError', message: errText(e) });
  }
}

async function doHydrate(entries: HydrateEntry[]) {
  // Hydration feeds wasm-held grids, so it too can be the first message on a fresh worker.
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  for (const e of entries) {
    hydrateGrid(
      {
        key: e.key,
        origin: e.origin,
        spacing: e.spacing,
        dims: e.dims,
        bbox: e.bbox,
        tris: e.tris,
        patches: e.patches,
        open_edges: e.open_edges,
      },
      e.dist,
      e.patch ?? undefined,
    );
  }
}

async function run(doc: Document) {
  // Setup validation before wasm: an incomplete study is not a failure, and the badge
  // renders these as a neutral hint rather than the red 'analysis failed'. Temperature,
  // convection or heat-flux statements run without loads: the thermal cases drive them.
  if (
    !doc.supports.length ||
    (!doc.loads.length && !doc.temperatures.length && !doc.convections.length && !doc.heat_fluxes.length)
  ) {
    post({
      type: 'error',
      kind: 'setup',
      message: !doc.supports.length
        ? 'add a support: the part needs something to hold it'
        : 'add a load or a fixed temperature, convection or heat flux',
    });
    return;
  }
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  const unh = unhydratedHash(doc);
  if (unh) {
    post({
      type: 'error',
      kind: 'setup',
      message: `mesh ${unh} is not loaded here — open its .fsmz sidecar or re-import the mesh, then run again`,
    });
    return;
  }
  const fill = doc.fill !== null;
  const size = doc.solve?.size ?? null;

  /** One runLevel: meshes, solves, packs and posts the level; its packed result (or null on
   * failure, with the error already posted). */
  const runLevel = (opts: { h?: number; dofs?: number }, label: string, keep = false): LevelResult | null => {
    post({ type: 'level', label });
    // One host measurement per level: the whole leg (wasm mesh + solve + the copies to
    // the main thread), the same contract as the CLI's rung wall.
    const t0 = performance.now();
    try {
      const out = fill ? analyseGraded(doc, opts) : analyse(doc, opts);
      const level = pack(label, out, keep && fill, t0);
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
  const unh = unhydratedHash(doc);
  if (unh) {
    post({
      type: 'exportError',
      message: `mesh ${unh} is not loaded here — open its .fsmz sidecar or re-import the mesh, then export again`,
    });
    return;
  }
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

/** The parameter sweep (GOAL v0.4 section D): every value runs the coarse/refined ladder
 * on a copy of the document, one `sweepPoint` post per value, then `sweepDone`. The
 * mirror of the native `Sweep` arm (crates/fieldsmith-api/src/lib.rs) — same check order,
 * same error texts, same record semantics as `fieldsmith-core/src/rung.rs`. Unlike the
 * native question, which discards partial work on a point failure, this worker streams
 * the points before the failing one so they stand in the table. */
async function doSweep(doc: Document, addr: string, values: number[], rungs: number[]) {
  if (!ready) ready = loadWasm({ pool: navigator.hardwareConcurrency });
  await ready;
  try {
    if (doc.fill !== null) {
      post({ type: 'sweepError', message: 'the sweep runs the solid path — remove the fill statement' });
      return;
    }
    const a = parseAddr(addr); // throws param.ts's mirror of param::parse's text
    resolveAddr(doc, a); // throws param.ts's mirror of param::resolve's text
    if (values.length === 0) {
      post({ type: 'sweepError', message: 'sweep: values is empty — name at least one value of the parameter' });
      return;
    }
    if (!values.every(Number.isFinite)) {
      post({ type: 'sweepError', message: 'sweep: values must all be finite numbers' });
      return;
    }
    if (rungs.length < 2) {
      post({ type: 'sweepError', message: 'rungs: a ladder needs at least two rungs — the rung-to-rung change estimate needs a baseline' });
      return;
    }
    for (let k = 1; k < rungs.length; k++) {
      if (!(rungs[k - 1] < rungs[k])) {
        post({ type: 'sweepError', message: 'rungs: rung DOF targets must increase (the change estimate reads refinement, not coarsening)' });
        return;
      }
    }
    // The sweep reuses this worker's wasm registries: an unhydrated mesh is the same
    // silently-vacuum hazard run() refuses, not something a value change fixes.
    const unh = unhydratedHash(doc);
    if (unh) {
      post({
        type: 'sweepError',
        message: `mesh ${unh} is not loaded here — open its .fsmz sidecar or re-import the mesh, then run again`,
      });
      return;
    }
    for (let i = 0; i < values.length; i++) {
      const value = values[i];
      const d = structuredClone(doc);
      try {
        applyAddr(d, a, value);
      } catch (e) {
        // The honest path: native's apply cannot fire after resolve (same comment in
        // lib.rs), but the mirror names the point if it ever does.
        post({ type: 'sweepError', message: `sweep point ${i + 1} (param ${addr} = ${rustFloat(value)}): ${errText(e)}` });
        return;
      }
      const records: SweepRecord[] = [];
      let prev: { d: number; v: number; c: number } | null = null;
      for (let r = 0; r < rungs.length; r++) {
        const t0 = performance.now();
        try {
          const out = analyse(d, { dofs: rungs[r] });
          const wall = performance.now() - t0;
          const meta = JSON.parse(out.metaJson()) as AnalysisMeta;
          // TrackedValues::of_cases: the max over cases, floored at 1e-9 so a
          // vacuum-flat case still produces a finite change estimate next rung.
          const defl = Math.max(1e-9, ...meta.cases.map((c) => c.max_displacement));
          const vm = Math.max(1e-9, ...meta.cases.map((c) => c.max_vm_elem));
          const compl = Math.max(1e-9, ...meta.cases.map((c) => c.compliance));
          records.push({
            rung: r,
            target_dofs: rungs[r],
            dofs: meta.provenance.dofs,
            elements: meta.provenance.elements,
            wall_ms: wall,
            change: prev
              ? {
                  deflection_rel: (defl - prev.d) / prev.d,
                  vm_rel: (vm - prev.v) / prev.v,
                  compliance_rel: (compl - prev.c) / prev.c,
                }
              : null,
            tracked: { deflection_mm: defl, vm_mpa: vm, compliance_nmm: compl },
          });
          prev = { d: defl, v: vm, c: compl };
          out.free();
        } catch (e) {
          post({ type: 'sweepError', message: `sweep point ${i + 1} of ${values.length} (param ${addr} = ${rustFloat(value)}): ${errText(e)}` });
          return;
        }
      }
      post({ type: 'sweepPoint', value, records });
    }
    post({ type: 'sweepDone', count: values.length });
  } catch (e) {
    // Anything outside the per-point paths (structuredClone, the load itself) reports
    // bare — the mirror's validation-phase throws already carry their native texts.
    post({ type: 'sweepError', message: errText(e) });
  }
}

(self as unknown as { onmessage: ((e: MessageEvent<WorkerIn>) => void) | null }).onmessage = (e) => {
  const m = e.data;
  if (m.type === 'run') void run(m.doc);
  else if (m.type === 'export') void doExport(m.doc, m.h, m.solid);
  else if (m.type === 'slice') doSlice(m.z);
  else if (m.type === 'bytes') doBytes(m.what, m.name);
  else if (m.type === 'import') void doImport(m.bytes, m.kind, m.src, m.opts);
  else if (m.type === 'hydrate') void doHydrate(m.entries);
  else if (m.type === 'sweep') void doSweep(m.doc, m.addr, m.values, m.rungs);
};