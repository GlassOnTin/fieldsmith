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
}

export type WorkerIn = { type: 'run'; doc: Document };

export type WorkerOut =
  | { type: 'level'; label: string }
  | { type: 'levelDone'; result: LevelResult }
  | { type: 'done'; skippedFine: string | null }
  /** kind 'setup': the study is not analysable yet (no support or no load) — not a failure. */
  | { type: 'error'; message: string; kind?: 'setup' };