// Main-thread mesh-grid cache (goal M6 step 9).
//
// The analysis worker is killed on every document edit, so its in-process registry is
// volatile. This main-thread cache owns the hydrated grids — filled by the worker's
// `importDone` and by `.fsmz` loads (whose wasm side effect hydrates the main registry) —
// and re-feeds a fresh worker (`{type:'hydrate'}`) before each run or export. Extend nodes
// never ride in the feed: their grids are derived in-process by compilation wherever they
// are needed.

import type { Document, Vec3 } from './types';
import { walk } from './types';
import { hydrateGrid, meshGrid, meshHydrated } from './wasm';
import type { HydrateEntry, SidecarManifest } from './wasm';

const cache = new Map<string, HydrateEntry>();

/** Store a grid in the cache and register it in this thread's registry when missing. The
 * entry's typed arrays stay owned by the cache (callers pass copies). */
export function put(entry: HydrateEntry): void {
  cache.set(entry.key, entry);
  if (!meshHydrated(entry.key)) {
    hydrateGrid(
      {
        key: entry.key,
        origin: entry.origin,
        spacing: entry.spacing,
        dims: entry.dims,
        bbox: entry.bbox,
        tris: entry.tris,
        patches: entry.patches,
        open_edges: entry.open_edges,
      },
      entry.dist,
      entry.patch ?? undefined,
    );
  }
}

export function has(keyHex: string): boolean {
  return cache.has(keyHex);
}

/** Hashes of every mesh node in the document (the extend keys derive from these). */
export function docMeshHashes(doc: Document): string[] {
  const out: string[] = [];
  walk(doc.root, (n) => {
    if (n.kind.type === 'mesh') out.push(n.kind.hash);
  });
  return out;
}

/** Cached entries for the document's mesh nodes, duplicates dropped. Missing hashes are
 * skipped — the worker's run guard names them when they really matter. */
export function entriesFor(doc: Document): HydrateEntry[] {
  const out: HydrateEntry[] = [];
  const seen = new Set<string>();
  for (const hash of docMeshHashes(doc)) {
    if (seen.has(hash)) continue;
    seen.add(hash);
    const e = cache.get(hash);
    if (e) out.push(e);
  }
  return out;
}

/** Copy a loaded sidecar's grids into the cache. `loadFsmz` hydrates the main registry as a
 * side effect, so the arrays come back from it; the document supplies the per-node bbox of
 * each mesh (the manifest record repeats units/up but not bounds). Returns the keys that
 * were not hydrated — an empty list is the expected outcome. */
export function cacheSidecar(doc: Document, manifest: SidecarManifest): string[] {
  const missing: string[] = [];
  const nodeBbox = new Map<string, [number, number, number, number, number, number]>();
  walk(doc.root, (n) => {
    if (n.kind.type === 'mesh' && !nodeBbox.has(n.kind.hash)) nodeBbox.set(n.kind.hash, n.kind.bbox);
  });
  for (const rec of manifest.meshes) {
    const bbox = nodeBbox.get(rec.hash);
    if (!meshHydrated(rec.hash) || !bbox) {
      missing.push(rec.hash);
      continue;
    }
    const g = meshGrid(rec.hash);
    put({
      key: rec.hash,
      origin: [g.origin()[0], g.origin()[1], g.origin()[2]],
      spacing: g.spacing(),
      dims: [g.dims()[0], g.dims()[1], g.dims()[2]],
      bbox,
      tris: rec.tris,
      patches: rec.patches,
      open_edges: rec.open_edges,
      dist: g.dist(),
      patch: g.patchData() ?? null,
    });
  }
  for (const rec of manifest.extends ?? []) {
    if (!meshHydrated(rec.hash)) {
      missing.push(rec.hash);
      continue;
    }
    const g = meshGrid(rec.hash);
    put({
      key: rec.hash,
      origin: [g.origin()[0], g.origin()[1], g.origin()[2]],
      spacing: g.spacing(),
      dims: [g.dims()[0], g.dims()[1], g.dims()[2]],
      bbox: rec.bbox,
      tris: 0,
      patches: 0,
      open_edges: 0,
      dist: g.dist(),
      patch: null,
    });
  }
  return missing;
}

/** Grid arrays for one shader slot, cache-first. Extend keys not in the cache read straight
 * from the registry (compilation hydrated them; their meta never leaves with a transfer).
 * `null` = the slot's grid is in neither — the viewport must not render the leaf. */
export function slotGrid(keyHex: string): {
  dist: Float32Array;
  patch: Uint16Array | null;
  dims: Vec3;
  origin: Vec3;
  spacing: number;
} | null {
  if (!meshHydrated(keyHex)) {
    const e = cache.get(keyHex);
    if (!e) return null;
    put(e);
    if (!meshHydrated(keyHex)) return null;
  }
  const g = meshGrid(keyHex);
  return {
    dist: g.dist(),
    patch: g.patchData() ?? null,
    dims: [g.dims()[0], g.dims()[1], g.dims()[2]],
    origin: [g.origin()[0], g.origin()[1], g.origin()[2]],
    spacing: g.spacing(),
  };
}

/** One import point for the app. */
export const meshcache = { put, has, docMeshHashes, entriesFor, cacheSidecar, slotGrid };