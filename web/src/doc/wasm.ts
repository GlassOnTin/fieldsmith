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
