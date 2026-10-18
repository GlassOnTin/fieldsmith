// WebAssembly access (main thread and the analysis worker). All documents cross the
// boundary as JSON.
//
// Two builds of the same crate: `pkg-mt` (shared memory, a rayon pool on Web Workers) and
// `pkg` (single thread). Shared memory needs a cross-origin isolated page (COOP/COEP
// headers), so the threaded build loads only when `crossOriginIsolated` holds; only the
// chosen variant is fetched.
import type * as W from '../pkg/fieldsmith_wasm.js';
import type { Document, Face, Material, Vec3 } from './types';
import { withLetTables } from './types';

/** The bound API (the two builds differ only in their init entry points). */
let w: Omit<typeof W, 'default' | 'initSync'>;
let ready: Promise<void> | null = null;

/** Whether this context can run the threaded build. */
export const threaded: boolean = typeof crossOriginIsolated !== 'undefined' && crossOriginIsolated;

/** Load the wasm module. `pool` (worker only): start the rayon pool on that many threads
 * — the threaded build runs rayon work on the calling thread until it does. */
export function loadWasm(opts: { pool?: number } = {}): Promise<void> {
  if (!ready) {
    ready = (async () => {
      if (threaded) {
        const [m, u] = await Promise.all([
          import('../pkg-mt/fieldsmith_wasm.js'),
          import('../pkg-mt/fieldsmith_wasm_bg.wasm?url'),
        ]);
        await m.default({ module_or_path: u.default });
        if (opts.pool && opts.pool > 1) {
          await m.initThreadPool(opts.pool);
        }
        w = m;
      } else {
        const [m, u] = await Promise.all([
          import('../pkg/fieldsmith_wasm.js'),
          import('../pkg/fieldsmith_wasm_bg.wasm?url'),
        ]);
        await m.default({ module_or_path: u.default });
        w = m;
      }
    })();
  }
  return ready;
}

export type ParseResult = { ok: Document } | { error: { line: number; message: string } };

export function parseFsm(text: string): ParseResult {
  const r = JSON.parse(w.parse_fsm(text));
  if ('ok' in r) withLetTables(r.ok);
  return r;
}

export function printFsm(doc: Document): string {
  return w.print_fsm(JSON.stringify(doc));
}

export function validate(doc: Document): string[] {
  return JSON.parse(w.validate(JSON.stringify(doc)));
}

/** Validation plus the material-labelling warnings (the solve caveat: analysis stays
 * single-material). */
export function validateWithWarnings(doc: Document): { errors: string[]; warnings: string[] } {
  return JSON.parse(w.validateWithWarnings(JSON.stringify(doc)));
}

/** A compiled load-density fn for the setup editor (M11): the band domain, 121 samples of
 * q(x) at y = 0, the per-1-mm integral over those samples, and the validate() strings. */
export interface LoadFnProbe {
  ok: boolean;
  error?: string;
  domain?: { lo: number; hi: number; from: 'bands' | 'requested' };
  bands?: { lo: number; hi: number }[];
  xs?: number[];
  q?: number[];
  integral?: number;
  checks?: string[];
}

export function loadFnProbe(src: string, lo: number, hi: number): LoadFnProbe {
  return JSON.parse(w.loadFnProbe(src, lo, hi));
}

export function materialLibrary(): Material[] {
  return JSON.parse(w.material_library());
}

export interface ShaderInfo {
  source: string;
  bounds: [Vec3, Vec3] | null;
  /** `vec2 fs_ident(vec3 p)` → (leaf index, face code). */
  ident: string;
  /** Leaf ids in `fs_ident` index order. */
  leaves: string[];
  /** Mesh texture specs to bind as `u_mesh{slot}` / `u_mesh{slot}p`. */
  meshes: MeshTexInfo[];
  /** Filled documents: the program carries `vec4 fs_core(vec3 p)`, the interior pass. */
  core: boolean;
  /** The core pass's `u_param` grid spec — bind its data (via `previewFillParamData`) as
   * the texture; `null` means a single-node grid whose literal the source already embeds.
   * `core` with `grid: null` is fine; both `false` together for body-only programs. */
  grid: { dims: [number, number, number]; origin: Vec3; spacing: number } | null;
}

/** One emitted mesh texture slot (matches `MeshTexOut` in Rust). */
export interface MeshTexInfo {
  slot: number;
  /** Registry key as 16 hex digits (u64 values exceed JS number precision); the serde
   * name `key_hex` crosses verbatim, as with `ImportReport.open_edges`. */
  key_hex: string;
  dims: [number, number, number];
  origin: Vec3;
  spacing: number;
  patch: boolean;
}

/** Face encoding used by generated GLSL (matches `Face::ALL` in Rust). */
export const FACE_CODES: Face[] = ['all', 'px', 'nx', 'py', 'ny', 'pz', 'nz', 'side', 'top', 'bottom'];

export function bodyShader(doc: Document): ShaderInfo {
  return JSON.parse(w.body_shader(JSON.stringify(doc)));
}

/** Data for the `u_param` texture named by `bodyShader`'s `grid` (f32, row-major with x
 * fastest). Re-derived per call; always in sync with the grid spec the same document's
 * `bodyShader` reported. */
export function previewFillParamData(doc: Document): Float32Array {
  return w.previewFillParamData(JSON.stringify(doc));
}

/** One shape as placed in the model, for drawing a ghost. */
export function shapeShader(doc: Document, id: string): ShaderInfo {
  return JSON.parse(w.shape_shader(JSON.stringify(doc), id));
}

export interface OverlayItem {
  selectors: { shape: string; face: Face }[];
  color: [number, number, number];
}

export function overlayShader(doc: Document, items: OverlayItem[]): string {
  return w.overlay_shader(JSON.stringify(doc), JSON.stringify(items));
}

export function evalBody(doc: Document, points: Float64Array): Float64Array {
  return w.eval_body(JSON.stringify(doc), points);
}

/** The interior (core) field of the preview fill, at the same points — the Rust twin of the
 * shader's `fs_core`. */
export function evalCore(doc: Document, points: Float64Array): Float64Array {
  return w.eval_core(JSON.stringify(doc), points);
}

/** [`evalCore`] with the `u_param` grid data supplied explicitly, so both sides of the V16
 * comparison consume the exact texel values the GPU texture holds (some stacks quantize
 * float-texture uploads to half precision). */
export function evalCoreWithParam(doc: Document, param: Float32Array, points: Float64Array): Float64Array {
  return w.eval_core_with_param(JSON.stringify(doc), param, points);
}

export interface PickHit {
  t: number;
  point: Vec3;
  normal: Vec3;
  shape: string | null;
  face: Face | null;
  /** Surface patch of the hit on a mesh leaf (`mesh1.p3` selectors); null for prim faces. */
  patch: number | null;
}

export function pick(doc: Document, o: Vec3, d: Vec3): PickHit | null {
  return JSON.parse(w.pick(JSON.stringify(doc), o[0], o[1], o[2], d[0], d[1], d[2]));
}

export function nodeFaces(doc: Document, id: string): Face[] {
  return JSON.parse(w.node_faces(JSON.stringify(doc), id));
}

export function bodyBounds(doc: Document): [Vec3, Vec3] | null {
  return JSON.parse(w.body_bounds(JSON.stringify(doc)));
}

// ---- mesh import ---------------------------------------------------------------------

/** One imported part's report (mirrored into its node; shown again after reload). */
export interface ImportReport {
  name: string;
  /** Generated node id (unique against the document's ids). */
  id: string;
  tris: number;
  /** Welded vertex count — what the voxeliser consumed. */
  verts: number;
  patches: number;
  open_edges: number;
  /** Baked world-space bounds (mm): [xmin, ymin, zmin, xmax, ymax, zmax]. */
  bbox: [number, number, number, number, number, number];
  /** 16 hex digits — the mesh node's `hash` field. */
  hash: string;
  /** Requested voxel spacing (mm); the baked grid spacing is `spacing`. */
  voxel: number;
  spacing: number;
  cells: number;
  ms: number;
}

/** Import result: nodes to splice into `doc.root` plus per-part reports. */
export interface ImportOut {
  nodes: Document['root'];
  parts: ImportReport[];
  ms: number;
}

/** Import GLB / STL / OBJ / 3MF bytes (`kind`: 'glb' | 'stl' | 'obj' | '3mf') into mesh
 * nodes. Units and up-axis default per kind (GLB: m + Y-up; STL/OBJ: mm + Z-up; 3MF:
 * the file's own `<model unit>` already scaled to mm) and are baked into the grid. */
export function importMesh(
  bytes: Uint8Array,
  kind: 'glb' | 'stl' | 'obj' | '3mf',
  opts: { units?: string; up?: string; voxel?: number; src?: string; ids?: string[] } = {},
): ImportOut {
  return JSON.parse(w.importMesh(bytes, kind, JSON.stringify({
    units: opts.units ?? null,
    up: opts.up ?? null,
    voxel: opts.voxel ?? null,
    src: opts.src ?? '',
    ids: opts.ids ?? [],
  })));
}

/** One hydrated mesh grid, read back by key for Data3DTexture upload (wasm-bindgen handle:
 * getters are methods; typed arrays are copies, not wasm-memory views). */
export interface GridInfo {
  keyHex(): string;
  origin(): Float64Array;
  spacing(): number;
  dims(): Uint32Array;
  patch(): boolean;
  /** Signed distances (f32, row-major x fastest). */
  dist(): Float32Array;
  /** Patch ids (u16, same layout; 65535 = unclaimed), undefined when unpatched. */
  patchData(): Uint16Array | undefined;
}

/** Read a hydrated mesh grid by its 16-hex key. Throws when unhydrated (open the .fsmz or
 * re-import first). */
export function meshGrid(keyHex: string): GridInfo {
  return w.meshGrid(keyHex);
}

/** Grid spec for `hydrateGrid` (matches `HydrateMesh` in Rust). */
export interface HydrateSpec {
  /** 16 hex digits. */
  key: string;
  origin: Vec3;
  spacing: number;
  dims: [number, number, number];
  bbox: [number, number, number, number, number, number];
  tris: number;
  patches: number;
  open_edges: number;
}

/** Register a mesh grid in this context's registry (the worker and the main thread each
 * keep their own). Returns the 16-hex key. Throws on a malformed key, a length/f64-count
 * mismatch against `dims` or a non-finite distance. */
export function hydrateGrid(spec: HydrateSpec, dist: Float32Array, patch?: Uint16Array): string {
  return w.hydrateMesh(JSON.stringify(spec), dist, patch);
}

/** Whether the grid behind `keyHex` is in this context's registry. */
export function meshHydrated(keyHex: string): boolean {
  return w.meshHydrated(keyHex);
}

// ---- .fsmz sidecar --------------------------------------------------------------------

/** One mesh-grid record in a sidecar manifest (matches `ManifestMesh` in Rust). */
export interface ManifestMesh {
  hash: string;
  units: string;
  up: string;
  voxel: number;
  origin: Vec3;
  spacing: number;
  dims: [number, number, number];
  tris: number;
  patches: number;
  open_edges: number;
  dist_crc32: number;
  patch: boolean;
  patch_crc32?: number;
}

/** One region-driven extend record (matches `ManifestExtend` in Rust): the derived sweep
 * grid keyed by its recipe (parent mesh, patch ids, direction, length). */
export interface ManifestExtend {
  /** The extend node's id. */
  id: string;
  /** 16 hex digits — the derived grid's key; the data entry is `ext-<hash>.dist`. */
  hash: string;
  region: string;
  /** Hash of the parent mesh node the sweep is derived from. */
  parent_hash: string;
  /** The region's patch ids naming the parent, sorted and deduped. */
  patch_ids: number[];
  dir: Vec3;
  len: number;
  origin: Vec3;
  spacing: number;
  dims: [number, number, number];
  bbox: [number, number, number, number, number, number];
  dist_crc32: number;
}

export interface SidecarManifest {
  v: number;
  meshes: ManifestMesh[];
  /** Extends records: absent from writers predating them. */
  extends?: ManifestExtend[];
}

/** Load a `.fsmz` sidecar: `{doc, manifest}`; the mesh-grid registry is hydrated as a
 * side effect. Throws naming the reason — an unreadable archive or a doc/manifest
 * disagreement — with the failing entry or hash named. */
export function loadFsmz(bytes: Uint8Array): { doc: Document; manifest: SidecarManifest } {
  const r = JSON.parse(w.loadFsmz(bytes));
  withLetTables(r.doc);
  return r;
}

/** Write the `.fsmz` sidecar for the document. Throws naming the hash when a mesh grid is
 * not hydrated (re-import the source or load the sidecar first). */
export function saveFsmz(doc: Document): Uint8Array {
  return w.saveFsmz(JSON.stringify(doc));
}

// ---- analysis ----------------------------------------------------------------------

export interface Quality {
  tets: number;
  min_dihedral_deg: number;
  max_dihedral_deg: number;
  min_volume_ratio: number;
}

export interface SolveReport {
  dofs: number;
  free_dofs: number;
  coarse_dofs: number;
  iterations: number;
  rel_residual: number;
  converged: boolean;
}

export interface Provenance {
  elements: number;
  order: number;
  nodes: number;
  dofs: number;
  element_size: number;
  mesh_ms: number;
  solve_ms: number;
  /** Thermal cases' conduction + expansion solve time; 0 for documents without
   * temperature statements. Included in total_ms. */
  thermal_ms?: number;
  total_ms: number;
  quality: Quality;
  iterations: number;
}

export interface CaseSummary {
  name: string;
  max_von_mises: number;
  max_vm_elem: number;
  max_displacement: number;
  solve: SolveReport;
  /** Applied-load provenance when the case has resolved loads (serde field of
   * fieldsmith_core::CaseResult; absent on older bundles). */
  load_reports?: LoadReport[];
  /** Thermal case only (M14): the nodal °C field, and the conduction solve's own
   * report (`solve` is the expansion solve's report). */
  temperature?: number[];
  conduction?: SolveReport;
}

/** What one labelled load applied at the solve (M11 provenance): resultant and moment
 * about the document origin, and for distributed (`fn=`) loads the unscaled density
 * resultant, the factor `total=` applied and the face-frame fit numbers. */
export interface LoadReport {
  id: string;
  region: string;
  facets: number;
  resultant: Vec3;
  moment: Vec3;
  unscaled: number;
  scale: number;
  fit_rms: number | null;
  fit_extent: number | null;
}

/** The natural frequencies `solve modes=<n>` adds to an analysis (M8): Hz, ascending. The
 * eigenvalues are ω² (rad/s)² in the same order. */
export interface ModalInfo {
  frequencies: number[];
  eigenvalues: number[];
  solve_ms: number;
}

export interface AnalysisMeta {
  provenance: Provenance;
  cases: CaseSummary[];
  yield_strength: number;
  poisson: number;
  /** Fill levels only: solid-reference per-case maxima in `cases` order (serde name). */
  solid_cases?: CaseMax[];
  /** Fill levels only: the fill spec, masses and the achieved density window. */
  fill?: FillInfo;
  /** When the document's `solve modes` asks for it: the first modes of the static mesh. */
  modes?: ModalInfo;
}

/** The wasm `AnalysisOut` handle. Typed arrays are views into wasm memory — copy before
 * transferring (`arr.slice()`). */
export interface AnalysisHandle {
  metaJson(): string;
  nodes(): Float64Array;
  surfaceFacets(): Uint32Array;
  surfaceOwner(): Uint32Array;
  surfaceRegion(): Uint32Array;
  /** Concatenated per-region facet sets, blocks in doc.regions order; block `ri` spans
   * `regionFacetOffsets[ri] .. [ri + 1]`. Unlike `surfaceRegion` a facet claimed by several
   * regions appears in every claiming region's block. */
  regionFacetSets(): Uint32Array;
  /** Cumulative sizes of the `regionFacetSets` blocks, length = number of regions + 1. */
  regionFacetOffsets(): Uint32Array;
  /** case index must be < `meta.cases.length` */
  u(ci: number): Float64Array;
  vmNodal(ci: number): Float64Array;
  /** Case index must be < `meta.cases.length`. */
  vmElem(ci: number): Float64Array;
  vmEnvelope(): Float64Array;
  vmElemEnvelope(): Float64Array;
  /** Nodal temperature of one case (°C); empty when the case is mechanical. */
  temperature(ci: number): Float64Array;
  /** Mass-normalised mode shapes the run solved (solid handle only): how many. 0 without
   * a `solve modes=` on the document; the graded handle carries none. */
  modeShapeCount?(): number;
  /** One mode shape's nodal displacements, 3 per node — empty when absent or the index is
   * out of range. */
  modeShape?(index: number): Float64Array;
  free(): void;
}

export function analyse(doc: Document, opts: { h?: number; dofs?: number } = {}): AnalysisHandle {
  return w.analyse(JSON.stringify(doc), opts.h, opts.dofs);
}

// ---- graded fill analysis -------------------------------------------------------------

export interface CaseMax {
  name: string;
  max_vm_elem: number;
  max_displacement: number;
}

export interface FillInfo {
  family: string;
  mode: string;
  cell: number;
  skin: number;
  rho_min: number;
  rho_max: number;
  curve: number;
  min_wall: number;
  /** Achieved design density after wall clamps (min, max) — what the lattice actually is. */
  rho_design: [number, number];
  sigma_ref: number;
  mass_g: number;
  solid_mass_g: number;
  fill_ms: number;
  solve_ms: number;
}

/** The wasm `GradedOut` handle: solid mesh plus the graded re-solve. The solid reference
 * maxima are in `metaJson().solidCases`; all displacement/stress getters are the graded
 * solve. Typed arrays are views into wasm memory — copy before transferring (`arr.slice()`). */
export interface GradedHandle extends AnalysisHandle {
  /** Per-element class: 0 solid (skin, protected), 1 lattice, 2 void. */
  class(): Uint8Array;
  /** Per-element material fraction of the fill (1 solid, 0 void; the modulus input). */
  rhoElem(): Float64Array;
  /** Graded density parameter grid `[ox, oy, oz, spacing]` (mm). */
  fillParamSpec(): Float64Array;
  /** Graded density parameter grid dims `[nx, ny, nz]`. */
  fillParamDims(): Uint32Array;
  /** Graded density parameter grid values, row-major with x fastest. */
  fillParamData(): Float32Array;
}

export function analyseGraded(doc: Document, opts: { h?: number; dofs?: number } = {}): GradedHandle {
  return w.analyseGraded(JSON.stringify(doc), opts.h, opts.dofs);
}

// ---- export (make lens) -------------------------------------------------------------

/** What the export mesh is, measured against the field it came from. */
export interface ExportMeta {
  /** "solid" or "graded fill". */
  what: string;
  /** Mesh spacing (mm). */
  h: number;
  verts: number;
  tris: number;
  field_volume: number;
  field_err: number;
  mesh_volume: number;
  delta_pct: number;
  open_edges: number;
  non_manifold_edges: number;
  orientation_defects: number;
  degenerate_triangles: number;
  self_intersections: number;
  build_ms: number;
  mesh_ms: number;
  ref_ms: number;
  scan_ms: number;
}

/** The wasm `MeshOut` handle: the exported triangles plus queries over them. Typed arrays
 * are views into wasm memory — copy before transferring (`arr.slice()`). */
export interface MeshHandle {
  metaJson(): string;
  verts(): Float64Array;
  tris(): Uint32Array;
  stlBytes(): Uint8Array;
  threeMfBytes(): Uint8Array;
  sliceAt(z: number): Float64Array;
  free(): void;
}

/** A graded handle also exports its stored fill without re-solving. */
export type GradedHandleWithExport = GradedHandle & { export(h: number): MeshHandle };

export function exportSolid(doc: Document, h: number): MeshHandle {
  return w.exportSolid(JSON.stringify(doc), h);
}

/** Export the graded fill, building it (stress solve included) when no handle is at hand. */
export function exportGraded(doc: Document, h: number): MeshHandle {
  return w.exportGraded(JSON.stringify(doc), h);
}

/** The graded parameter grid riding a fill level: spec `[ox, oy, oz, spacing]`, dims
 * `[nx, ny, nz]`, values row-major with x fastest (mm). */
export interface FillParamLevel {
  spec: Float64Array;
  dims: Uint32Array;
  data: Float32Array;
}

/** One ladder level streamed back from the analysis worker. Typed arrays are standalone
 * copies (transferred buffers). */
export interface LevelResult {
  label: string;
  meta: AnalysisMeta;
  nodes: Float64Array;
  surfaceFacets: Uint32Array;
  surfaceOwner: Uint32Array;
  /** Region index per surface facet (u32::MAX = none); indexes doc.regions. */
  surfaceRegion: Uint32Array;
  /** Concatenated per-region facet sets (blocks in doc.regions order) plus their cumulative
   * offsets, length regions+1. Overlapping regions each keep the facets they claim. */
  regionFacetSets: Uint32Array;
  regionFacetOffsets: Uint32Array;
  /** Nodal displacements per case, 3 per node (mm). */
  u: Float64Array[];
  /** Element-centroid von Mises per case (MPa). */
  vmElem: Float64Array[];
  /** Nodal °C per case; null for mechanical cases (M14). */
  temperature: (Float64Array | null)[];
  /** Max over cases of element-centroid von Mises (MPa). */
  vmElemEnvelope: Float64Array;
  /** Modal runs only: the mass-normalised mode shapes concatenated, one 3-per-node block
   * per mode (block k spans k·3·nodes .. (k+1)·3·nodes, mesh-node order); null otherwise. */
  modeShapes: Float64Array | null;
  /** Fill levels only: per-element class of the same mesh (0 solid / 1 lattice / 2 void). */
  class: Uint8Array | null;
  /** Fill levels only: the graded parameter grid for the bound texture. */
  fillParam: FillParamLevel | null;
  /** Host-measured wall time of this level in the worker (mesh + solve + the copies to
   * the main thread), ms — the browser-side counterpart of the CLI's one Instant per rung. */
  runMs: number;
}

/** One mesh grid handed to (or from) a worker: spec plus payloads. The typed arrays are
 * plain standalone copies; transfer their buffers for the trip and cache both sides. */
export interface HydrateEntry {
  /** 16 hex digits. */
  key: string;
  origin: Vec3;
  spacing: number;
  dims: [number, number, number];
  bbox: [number, number, number, number, number, number];
  tris: number;
  patches: number;
  open_edges: number;
  dist: Float32Array;
  patch: Uint16Array | null;
}

export type WorkerIn =
  | { type: 'run'; doc: Document }
  | { type: 'export'; doc: Document; h: number; solid: boolean }
  /** Scrub request for the layer preview of the last export (no-op while none exists). */
  | { type: 'slice'; z: number }
  /** Fetch the last export's file bytes as a copy; name echoes back for the filename. */
  | { type: 'bytes'; what: 'stl' | '3mf'; name: string }
  /** Voxelise an imported model in the worker (rayon threads); the grids come back in
   * `importDone` for the main-thread cache. */
  | { type: 'import'; bytes: ArrayBuffer; kind: 'glb' | 'stl' | 'obj' | '3mf'; src: string; opts: { units?: string; up?: string; voxel?: number; ids?: string[] } }
  /** Register mesh grids in the worker's registry (fresh copies — the main-thread cache
   * keeps ownership of its own). Posted before `run`/`export` when the doc carries meshes. */
  | { type: 'hydrate'; entries: HydrateEntry[] }
  /** The parameter sweep (GOAL v0.4 section D): every value runs the coarse/refined
   * ladder on a copy of the document, records stream back per point. */
  | { type: 'sweep'; doc: Document; addr: string; values: number[]; rungs: number[] };

export type ExportDone = { meta: ExportMeta };

/** One sweep rung's measured record — the fields the ladder table and the CSV read
 * from the native `RungRecord`. `element_size_mm` rides the same native record but is
 * neither a CSV column nor a UI cell, so the mirror omits it. */
export interface SweepTracked {
  deflection_mm: number;
  vm_mpa: number;
}
export interface SweepChange {
  deflection_rel: number;
  vm_rel: number;
}
export interface SweepRecord {
  rung: number;
  target_dofs: number;
  dofs: number;
  elements: number;
  wall_ms: number;
  /** The rung-over-rung change estimate; null on rung 0 (no baseline). */
  change: SweepChange | null;
  tracked: SweepTracked;
}

/** The web sweep's rung schedule — the ladder's own coarse/refined constants, shared
 * with the analysis worker. */
export const COARSE_DOFS = 30_000;
export const FINE_DOFS = 150_000;

export type WorkerOut =
  | { type: 'level'; label: string }
  | { type: 'levelDone'; result: LevelResult }
  | { type: 'done'; skippedFine: string | null }
  /** Import finished: nodes/reports to splice into the document, grids to put in the
   * main-thread cache (their buffers arrive transferred). The worker keeps its own copies
   * hydrated for the next run. */
  | { type: 'importDone'; result: ImportOut; grids: HydrateEntry[] }
  | { type: 'importError'; message: string }
  /** kind 'setup': the study is not analysable yet (no support or no load) — not a failure. */
  | { type: 'error'; message: string; kind?: 'setup' }
  | { type: 'exportStart'; h: number; solid: boolean }
  /** The mesh stays in the worker; `meta` is its health report, `slice`/`bytes` query it. */
  | { type: 'exportDone'; mesh: ExportDone }
  /** `trapped`: the wasm instance hit a trap (e.g. out of memory) and is unusable. */
  | { type: 'exportError'; message: string; trapped?: boolean }
  /** Latest-wins: `z` is the slice this answer belongs to (ignore stale ones), `segs` is
   * packed `[x0, y0, x1, y1, …]` (mm) or null when there is no export mesh. */
  | { type: 'sliceDone'; z: number; segs: Float64Array | null }
  | { type: 'bytesDone'; what: 'stl' | '3mf'; name: string; bytes: ArrayBuffer }
  /** The sweep, streaming: one post per point with that point's full record list
   * (`value` echoes the request order), then a `sweepDone` tally. */
  | { type: 'sweepPoint'; value: number; records: SweepRecord[] }
  | { type: 'sweepDone'; count: number }
  | { type: 'sweepError'; message: string };