// Main-thread WebAssembly access. All documents cross the boundary as JSON.
import init, * as w from '../pkg/fieldsmith_wasm.js';
import wasmUrl from '../pkg/fieldsmith_wasm_bg.wasm?url';
import type { Document, Face, Material, Vec3 } from './types';

let ready: Promise<void> | null = null;

export function loadWasm(): Promise<void> {
  if (!ready) ready = init({ module_or_path: wasmUrl }).then(() => undefined);
  return ready;
}

export { wasmUrl };

export type ParseResult = { ok: Document } | { error: { line: number; message: string } };

export function parseFsm(text: string): ParseResult {
  return JSON.parse(w.parse_fsm(text));
}

export function printFsm(doc: Document): string {
  return w.print_fsm(JSON.stringify(doc));
}

export function validate(doc: Document): string[] {
  return JSON.parse(w.validate(JSON.stringify(doc)));
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
}

/** Face encoding used by generated GLSL (matches `Face::ALL` in Rust). */
export const FACE_CODES: Face[] = ['all', 'px', 'nx', 'py', 'ny', 'pz', 'nz', 'side', 'top', 'bottom'];

export function bodyShader(doc: Document): ShaderInfo {
  return JSON.parse(w.body_shader(JSON.stringify(doc)));
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

export interface PickHit {
  t: number;
  point: Vec3;
  normal: Vec3;
  shape: string | null;
  face: Face | null;
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
}

/** The wasm `AnalysisOut` handle. Typed arrays are views into wasm memory — copy before
 * transferring (`arr.slice()`). */
export interface AnalysisHandle {
  metaJson(): string;
  nodes(): Float64Array;
  surfaceFacets(): Uint32Array;
  surfaceOwner(): Uint32Array;
  surfaceRegion(): Uint32Array;
  /** case index must be < `meta.cases.length` */
  u(ci: number): Float64Array;
  vmNodal(ci: number): Float64Array;
  /** Case index must be < `meta.cases.length`. */
  vmElem(ci: number): Float64Array;
  vmEnvelope(): Float64Array;
  vmElemEnvelope(): Float64Array;
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
  /** Nodal displacements per case, 3 per node (mm). */
  u: Float64Array[];
  /** Element-centroid von Mises per case (MPa). */
  vmElem: Float64Array[];
  /** Max over cases of element-centroid von Mises (MPa). */
  vmElemEnvelope: Float64Array;
  /** Fill levels only: per-element class of the same mesh (0 solid / 1 lattice / 2 void). */
  class: Uint8Array | null;
}

export type WorkerIn =
  | { type: 'run'; doc: Document }
  | { type: 'export'; doc: Document; h: number; solid: boolean }
  /** Scrub request for the layer preview of the last export (no-op while none exists). */
  | { type: 'slice'; z: number }
  /** Fetch the last export's file bytes as a copy; name echoes back for the filename. */
  | { type: 'bytes'; what: 'stl' | '3mf'; name: string };

export type ExportDone = { meta: ExportMeta };

export type WorkerOut =
  | { type: 'level'; label: string }
  | { type: 'levelDone'; result: LevelResult }
  | { type: 'done'; skippedFine: string | null }
  /** kind 'setup': the study is not analysable yet (no support or no load) — not a failure. */
  | { type: 'error'; message: string; kind?: 'setup' }
  | { type: 'exportStart'; h: number; solid: boolean }
  /** The mesh stays in the worker; `meta` is its health report, `slice`/`bytes` query it. */
  | { type: 'exportDone'; mesh: ExportDone }
  | { type: 'exportError'; message: string }
  /** Latest-wins: `z` is the slice this answer belongs to (ignore stale ones), `segs` is
   * packed `[x0, y0, x1, y1, …]` (mm) or null when there is no export mesh. */
  | { type: 'sliceDone'; z: number; segs: Float64Array | null }
  | { type: 'bytesDone'; what: 'stl' | '3mf'; name: string; bytes: ArrayBuffer };