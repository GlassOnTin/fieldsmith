//! Voxel distance fields baked from imported meshes, and the session registry that holds
//! them (goal M6, step 1).
//!
//! An imported mesh is voxelised into a [`Grid3`] of signed distances plus an optional
//! per-cell patch grid. Documents carry no bulk data: a mesh node compiles to
//! [`crate::Expr::Mesh`] holding only the registry key, the baked bounding box and the grid
//! placement, and the grids themselves live in a process-wide registry keyed by content
//! hash. The key never evaluates to geometry on its own — an entry that has not been
//! hydrated (say, a document freshly parsed from `.fsm` on a page reload) behaves as vacuum
//! (`D::EMPTY`), so partial documents stay neutral and nothing hangs.
//!
//! Two invariants make the field safe for raymarching and meshing:
//!
//! * conservative clamping — samples further than [`BAND_CELLS`]·spacing from the surface
//!   return the (signed) band radius, so distances are never overestimated;
//! * the patch grid indexes with the same layout as the distance grid (`patch_index`).

use crate::dual::*;
use crate::expr::{Aabb, Expr};
use crate::lattice::Grid3;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock, RwLock};

/// Distance clamp around the surface, in cells: samples within `BAND_CELLS * spacing` of the
/// surface carry a true distance; beyond it, field values clamp to the band radius.
pub const BAND_CELLS: f64 = 3.0;

/// Patch-grid cell id meaning "no patch claimed here". Segmentation ids start at 1.
pub const NO_PATCH: u16 = u16::MAX;

/// First face code used by mesh patches in the `(leaf, code)` identification stream;
/// primitive geometric faces are codes 0..=9, mesh patch `p` reports `10 + p`.
pub const PATCH_CODE0: u16 = 10;

/// Grid placement of a baked field — stored inside the `Expr` so evaluation and shader
/// generation need no registry access.
#[derive(Clone, Debug, PartialEq)]
pub struct GridSpec {
    pub origin: V3,
    pub spacing: f64,
    pub dims: [usize; 3],
    /// A parallel per-cell patch grid accompanies the distance grid.
    pub patch: bool,
}

/// Import summary mirrored into the document's mesh node (honest reporting that survives
/// reload: the same numbers are re-shown even when the grids are not hydrated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshMeta {
    /// Baked world-space bounds (mm).
    pub bbox: Aabb,
    pub tris: u32,
    pub patches: u32,
    pub open_edges: u32,
}

/// A hydrated mesh field: distance grid, optional patch grid, import metadata.
#[derive(Clone, Debug)]
pub struct MeshField {
    pub dist: Arc<Grid3>,
    /// Per-cell patch id (surface segmentation), `NO_PATCH` where nothing is claimed.
    pub patch: Option<Arc<Vec<u16>>>,
    pub meta: MeshMeta,
}

impl MeshField {
    /// The `GridSpec` this field compiles into an `Expr::Mesh`.
    pub fn spec(&self) -> GridSpec {
        GridSpec {
            origin: self.dist.origin,
            spacing: self.dist.spacing,
            dims: self.dist.dims,
            patch: self.patch.is_some(),
        }
    }

    /// Distance of the surface band: trilinear sample with analytic gradient.
    /// GLSL twin: the `fs_mesh{N}` emitters in [`crate::glsl`].
    #[inline]
    pub fn sample(&self, p: V3) -> D {
        self.dist.sample_grad(p)
    }

    /// Patch id covering the cell nearest `p`. `None` when there is no patch grid, or the
    /// cell is unclaimed. Cell choice matches the GLSL patch sampler: node-centred grid,
    /// `floor` of the clamped coordinate.
    pub fn patch_at(&self, p: V3) -> Option<u16> {
        let idx = self.cell_of(p)?;
        let id = self.patch.as_ref()?[patch_index(self.dist.dims, idx)];
        if id == NO_PATCH {
            None
        } else {
            Some(id)
        }
    }

    /// Grid cell containing `p` (clamped to the grid), `None` only for degenerate 1-cell axes.
    pub fn cell_of(&self, p: V3) -> Option<[usize; 3]> {
        let g = &*self.dist;
        let mut cell = [0usize; 3];
        for a in 0..3 {
            if g.dims[a] == 1 {
                // Node-centred grids never ship 1-cell axes from the voxeliser; treat them
                // as the single node for robustness (matches `Grid3::sample`).
                cell[a] = 0;
                continue;
            }
            let u = ((p[a] - g.origin[a]) / g.spacing).clamp(0.0, (g.dims[a] - 1) as f64);
            cell[a] = (u.floor() as usize).min(g.dims[a] - 1);
        }
        Some(cell)
    }
}

/// Flattened index of `[i, j, k]` in a patch grid with `dims` (matches `Grid3::at` layout).
pub fn patch_index(dims: [usize; 3], idx: [usize; 3]) -> usize {
    (idx[2] * dims[1] + idx[1]) * dims[0] + idx[0]
}

// ---------------------------------------------------------------------------------------------
// Deterministic slot assignment

/// Distinct mesh keys reachable from `e`, collected for GLSL slot assignment.
pub fn mesh_keys(e: &Expr, out: &mut BTreeSet<u64>) {
    use crate::Expr as X;
    match e {
        X::Mesh { key, .. } => {
            out.insert(*key);
        }
        X::Xform { e, .. }
        | X::Shell { e, .. }
        | X::Offset { e, .. }
        | X::Mirror { e, .. }
        | X::Linear { e, .. }
        | X::Circular { e, .. } => mesh_keys(e, out),
        X::Combine { a, b, .. } => {
            mesh_keys(a, out);
            mesh_keys(b, out);
        }
        X::Filled(f) => {
            mesh_keys(&f.body, out);
            mesh_keys(&f.keep, out);
        }
        X::Empty | X::Prim(_) => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Content hashing

/// fnv1a-64 continuation over `bytes`, threading through `h`.
pub fn fnv1a64_update(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = h;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Hash the raw source bytes, then the baked placement (`salt` covers units / up-axis /
/// spacing so a re-import with different bake parameters gets a distinct key).
pub fn hash_mesh_input(bytes: &[u8], salt: &[u8]) -> u64 {
    let h = fnv1a64_update(0xcbf2_9ce4_8422_2325, bytes);
    fnv1a64_update(h, salt)
}

/// Parse a mesh node's hex hash into a registry key. Tolerates a `0x` prefix and surrounding
/// whitespace; malformed text yields 0 (no registry entry ever uses 0, so the `Expr`
/// degrades to vacuum).
pub fn key_from_hex(s: &str) -> u64 {
    u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Region-driven extension (goal M6, step 8)

/// Cell budget for one derived extend grid, mirroring voxel.rs's import budget. The est-
/// imate a document's `validate` makes from the parent node's bounds is an upper bound on
/// the real swept grid, so this only trips for unvalidated paths; both names the remedy.
pub const MAX_EXTEND_CELLS: usize = 6_000_000;

/// Input hash of a derived extend field: the parent mesh's hex hash with the swept patch ids
/// (sorted, deduplicated), the sweep direction, the length and the parent grid's baked
/// spacing. Same inputs → same key, so re-derivations hydrate in place instead of leaking.
pub fn extend_key(
    parent_hash: &str,
    patch_ids: &[u16],
    dir: [f64; 3],
    len: f64,
    spacing: f64,
) -> u64 {
    let mut ids = patch_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut salt = Vec::new();
    salt.extend_from_slice(b"extend");
    salt.extend_from_slice(parent_hash.as_bytes());
    for p in &ids {
        salt.extend_from_slice(&p.to_le_bytes());
    }
    extend_salt_tail(salt, dir, len, spacing)
}

/// Input hash of a derived extend field built from a face-direction selector (no patch
/// involved). Distinct salt prefix from [`extend_key`], so a patch sweep and a face sweep
/// over the same parent can never share a registry entry.
pub fn extend_key_face(
    parent_hash: &str,
    axis: [f64; 3],
    dir: [f64; 3],
    len: f64,
    spacing: f64,
) -> u64 {
    let mut salt = Vec::new();
    salt.extend_from_slice(b"extend-face");
    salt.extend_from_slice(parent_hash.as_bytes());
    for v in axis {
        salt.extend_from_slice(&v.to_le_bytes());
    }
    extend_salt_tail(salt, dir, len, spacing)
}

// The callers' `head` already carries the salt prefix, parent hash and selector payload;
// this closes the hash with dir, len and spacing.
fn extend_salt_tail(mut salt: Vec<u8>, dir: [f64; 3], len: f64, spacing: f64) -> u64 {
    for v in dir {
        salt.extend_from_slice(&v.to_le_bytes());
    }
    salt.extend_from_slice(&len.to_le_bytes());
    salt.extend_from_slice(&spacing.to_le_bytes());
    fnv1a64_update(0xcbf2_9ce4_8422_2325, salt.as_slice())
}

/// Face-direction cone threshold, shared with `region_facets`' normal-parallel test: a
/// surface whose outward normal lies within 45° (cos ≥ 0.7) of the face direction belongs
/// to that face. Same constant on both consumers so a labelled support and a swept skirt
/// see the same face.
pub const FACE_CONE_COS: f64 = 0.7;

/// Shell-side ordinals for [`shell_field`], in `fieldsmith_doc::ShellSide`'s variant
/// order. The raw byte rides [`shell_key`]; expression compile maps the enum.
pub const SHELL_INSIDE: u8 = 0;
pub const SHELL_OUTSIDE: u8 = 1;
pub const SHELL_CENTRE: u8 = 2;

/// Coarsest spacing a derived offset/shell bake may coarsen its expanded grid to before it
/// refuses: the printable ceiling, matching the voxeliser's own clamp. A grid that still
/// exceeds the cell budget at this spacing gets no bake.
pub const MAX_DERIVED_SPACING: f64 = 8.0;

/// Input hash of a mesh-operand offset bake: salt `b"offset-mesh"` over the parent hash and
/// the distance, FNV-folded like the extend recipes. Distinct salt prefix, so an offset of
/// the same parent cannot share a registry entry with any import, extend or shell.
pub fn offset_key(parent_hash: &str, d: f64) -> u64 {
    let mut salt = Vec::new();
    salt.extend_from_slice(b"offset-mesh");
    salt.extend_from_slice(parent_hash.as_bytes());
    salt.extend_from_slice(&d.to_le_bytes());
    fnv1a64_update(0xcbf2_9ce4_8422_2325, salt.as_slice())
}

/// Input hash of a mesh-operand shell bake: `side` is one of the [`SHELL_INSIDE`] /
/// [`SHELL_OUTSIDE`] / [`SHELL_CENTRE`] ordinals.
pub fn shell_key(parent_hash: &str, side: u8, t: f64) -> u64 {
    let mut salt = Vec::new();
    salt.extend_from_slice(b"shell-mesh");
    salt.extend_from_slice(parent_hash.as_bytes());
    salt.push(side);
    salt.extend_from_slice(&t.to_le_bytes());
    fnv1a64_update(0xcbf2_9ce4_8422_2325, salt.as_slice())
}

/// Build the swept solid of a face: every near-surface cell of the parent whose SDF
/// gradient lies within the 45° face cone (normals pointing along the face direction) is
/// swept along the normalised `dir` for `len`, identically to [`swept_patch_field`]. No
/// patch grid needed — this is the geometric face rule for meshes whose segmentation never
/// split the surface (a smooth AI-exported blob is one patch end to end).
///
/// The axis is in the parent grid's own frame; the compile side maps document-frame face
/// directions through the node transform before calling.
pub fn swept_face_field(
    parent: &MeshField,
    dir: [f64; 3],
    len: f64,
    axis: [f64; 3],
) -> Result<MeshField, String> {
    if dir.iter().any(|v| !v.is_finite()) || !len.is_finite() || len <= 0.0 {
        return Err("direction or length is not usable".to_string());
    }
    if axis.iter().any(|v| !v.is_finite()) {
        return Err("face direction is not usable".to_string());
    }
    let empty = "no near-surface cell's normal faces that direction within the 45° cone — the face holds no swept material".to_string();
    swept_field(parent, dir, len, SweepSrc::Face(axis), empty)
}

enum SweepSrc {
    /// Patch-grid cells claiming these ids within one spacing of the surface.
    Patches(BTreeSet<u16>),
    /// Near-surface cells whose SDF gradient lies within the face cone of this axis.
    Face([f64; 3]),
}

fn swept_field(
    parent: &MeshField,
    dir: [f64; 3],
    len: f64,
    src: SweepSrc,
    empty_err: String,
) -> Result<MeshField, String> {
    let n: f64 = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if !n.is_finite() || n <= 1e-9 {
        return Err("sweep direction is zero".to_string());
    }
    let dn = [dir[0] / n, dir[1] / n, dir[2] / n];

    let g = &*parent.dist;
    let [nx, ny, nz] = g.dims;
    let mut sources: Vec<[i64; 3]> = Vec::new();
    match src {
        SweepSrc::Patches(shell) => {
            // The thin shell: swept source cells = claimed ∧ |parent distance| ≤ 1 spacing
            // (the 3-s patch band laterally flares the skirt otherwise).
            let Some(patch) = &parent.patch else {
                return Err("the parent mesh has no patch grid".to_string());
            };
            for (idx, id) in patch.iter().enumerate() {
                if *id == NO_PATCH || !shell.contains(id) {
                    continue;
                }
                let d = parent.dist.data[idx];
                if (d as f64).abs() <= g.spacing {
                    sources.push(decompose_index(idx, [nx, ny, nz]));
                }
            }
        }
        SweepSrc::Face(axis) => {
            // Central differences need a neighbour on both sides of every axis.
            if [nx, ny, nz].iter().any(|&d| d < 3) {
                return Err(
                    "the parent mesh's distance grid is too thin for a face sweep".to_string(),
                );
            }
            let s = 2.0 * g.spacing;
            for k in 1..nz - 1 {
                for j in 1..ny - 1 {
                    for i in 1..nx - 1 {
                        let idx = (k * ny + j) * nx + i;
                        if (g.data[idx] as f64).abs() > g.spacing {
                            continue;
                        }
                        let gx = (g.data[idx + 1] as f64 - g.data[idx - 1] as f64) / s;
                        let gy = (g.data[idx + nx] as f64 - g.data[idx - nx] as f64) / s;
                        let gz = (g.data[idx + nx * ny] as f64 - g.data[idx - nx * ny] as f64) / s;
                        let gl = (gx * gx + gy * gy + gz * gz).sqrt();
                        if gl < 1e-9 {
                            continue;
                        }
                        if (gx * axis[0] + gy * axis[1] + gz * axis[2]) / gl >= FACE_CONE_COS {
                            sources.push([i as i64, j as i64, k as i64]);
                        }
                    }
                }
            }
        }
    }
    if sources.is_empty() {
        return Err(empty_err);
    }
    let spacing = g.spacing;
    let node_pos = |s: [i64; 3]| {
        [
            g.origin[0] + spacing * s[0] as f64,
            g.origin[1] + spacing * s[1] as f64,
            g.origin[2] + spacing * s[2] as f64,
        ]
    };

    // Swept hull: the source hull union its copy displaced by len·n̂.
    let mut hull_lo = node_pos(sources[0]);
    let mut hull_hi = hull_lo;
    for s in &sources {
        let w = node_pos(*s);
        for a in 0..3 {
            let far = w[a] + dn[a] * len;
            hull_lo[a] = hull_lo[a].min(w[a]).min(far);
            hull_hi[a] = hull_hi[a].max(w[a]).max(far);
        }
    }

    // Extend grid: the swept hull plus a band skirt, node-aligned to the parent (the hull
    // corner is a parent node, so shared nodes coincide with the parent's).
    let origin = [
        hull_lo[0] - BAND_CELLS * spacing,
        hull_lo[1] - BAND_CELLS * spacing,
        hull_lo[2] - BAND_CELLS * spacing,
    ];
    let mut dims = [1usize; 3];
    for a in 0..3 {
        let span = (hull_hi[a] + BAND_CELLS * spacing) - (hull_lo[a] - BAND_CELLS * spacing);
        dims[a] = (span / spacing).ceil() as usize + 1;
    }
    let cells = dims[0] as u64 * dims[1] as u64 * dims[2] as u64;
    if cells > MAX_EXTEND_CELLS as u64 {
        return Err(format!(
            "the extend grid would hold {cells} cells (budget {MAX_EXTEND_CELLS}); raise the mesh's voxel spacing or shorten the extension"
        ));
    }

    // Swept mask over the extend grid.
    let mut mask = vec![false; cells as usize];
    // Mask indices use the EXTEND grid's own row stride; parent dims only feed the source
    // decomposition above.
    let (nx, ny) = (dims[0] as i64, dims[1] as i64);
    let at = |i: i64, j: i64, k: i64| (k * ny + j) * nx + i;
    let step = spacing * 0.5;
    let kmax = (len / step).ceil().max(1.0) as i64;
    for s in &sources {
        let base = node_pos(*s);
        for k in 0..=kmax {
            let t = (k as f64 * step).min(len);
            if t < -0.0 {
                continue;
            }
            let p = [
                base[0] + dn[0] * t,
                base[1] + dn[1] * t,
                base[2] + dn[2] * t,
            ];
            let mut c = [0i64; 3];
            for a in 0..3 {
                let u = ((p[a] - origin[a]) / spacing)
                    .round()
                    .clamp(0.0, dims[a] as f64 - 1.0);
                c[a] = u as i64;
            }
            mask[at(c[0], c[1], c[2]) as usize] = true;
        }
    }

    Ok(bake_from_mask(
        &mask, dims, origin, spacing, hull_lo, hull_hi,
    ))
}

/// Build the swept solid of a patch: every patch-grid cell claiming one of `patch_ids` and
/// sitting within one spacing of the parent's surface (the thin shell that hugs it — the 3-s
/// patch band laterally flares the skirt otherwise) is swept along the normalised `dir` for
/// `len`, in quarter-spacing steps; the swept cell mask then becomes a signed distance grid
/// by the same deterministic two-pass chamfer EDT the coupon uses, clamped to the 3-cell
/// band. The result unions with its parent mesh through the document's compile.
///
/// The extend grid spans only the swept hull plus a band skirt, not the whole parent, so a
/// 10 mm skirt on the perforated grid is ~16k cells, not the parent's 6M.
///
/// Error strings name the patch ids — the caller (compile-side) already knows which region
/// and node they belong to.
pub fn swept_patch_field(
    parent: &MeshField,
    dir: [f64; 3],
    len: f64,
    patch_ids: &[u16],
) -> Result<MeshField, String> {
    let mut ids = patch_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Err("no patch ids named".to_string());
    }
    if dir.iter().any(|v| !v.is_finite()) || !len.is_finite() || len <= 0.0 {
        return Err("direction or length is not usable".to_string());
    }
    if parent.patch.is_none() {
        return Err("the parent mesh has no patch grid".to_string());
    }
    let empty = format!(
        "no patch-grid cell claims patches {ids:?} within the baked band — the patch segmented but holds no swept material"
    );
    swept_field(
        parent,
        dir,
        len,
        SweepSrc::Patches(ids.into_iter().collect()),
        empty,
    )
}

/// Grid cell `[i, j, k]` of a flattened index (x fastest, the `Grid3` layout).
fn decompose_index(idx: usize, dims: [usize; 3]) -> [i64; 3] {
    let [nx, ny, _] = dims;
    let i = idx % nx;
    let rem = idx / nx;
    let j = rem % ny;
    let k = rem / ny;
    [i as i64, j as i64, k as i64]
}

/// Unsigned chamfer distance (in cells) from every cell to the nearest cell of the mask.
/// Two passes over 26 neighbours, like the coupon's EDT: ascending with earlier offsets,
/// descending with later ones. `reverse` seeds the empties instead of the mask (the interior
/// half of the signed transform). Layout is the `Grid3` x-fastest order — coupon's own
/// indexing differs; do not copy it here.
fn chamfer_distance_cells(mask: &[bool], dims: [usize; 3], reverse: bool) -> Vec<f32> {
    let big = 1e30f32;
    let [nx, ny, nz] = dims;
    let mut d = vec![big; nx * ny * nz];
    for (idx, m) in mask.iter().enumerate() {
        if *m != reverse {
            d[idx] = 0.0;
        }
    }
    let mut earlier = Vec::new();
    let mut later = Vec::new();
    for dk in -1i32..=1 {
        for dj in -1..=1 {
            for di in -1..=1 {
                if (di, dj, dk) == (0, 0, 0) {
                    continue;
                }
                let w = ((di * di + dj * dj + dk * dk) as f32).sqrt();
                // Pass 0 walks k, then j, then i ascending: an already-computed neighbour
                // sorts earlier in that (dk, dj, di) tuple order.
                let earlier_n = dk < 0 || (dk == 0 && (dj < 0 || (dj == 0 && di < 0)));
                if earlier_n {
                    earlier.push((di, dj, dk, w));
                } else {
                    later.push((di, dj, dk, w));
                }
            }
        }
    }
    let relax = |d: &mut [f32], offs: &[(i32, i32, i32, f32)], rev: bool| {
        let ks: Box<dyn Iterator<Item = i64>> = if rev {
            Box::new((0..nz as i64).rev())
        } else {
            Box::new(0..nz as i64)
        };
        for k in ks {
            let js: Box<dyn Iterator<Item = i64>> = if rev {
                Box::new((0..ny as i64).rev())
            } else {
                Box::new(0..ny as i64)
            };
            for j in js {
                let ii: Box<dyn Iterator<Item = i64>> = if rev {
                    Box::new((0..nx as i64).rev())
                } else {
                    Box::new(0..nx as i64)
                };
                for i in ii {
                    let idx = ((k * ny as i64 + j) * nx as i64 + i) as usize;
                    if d[idx] == 0.0 {
                        continue;
                    }
                    for (di, dj, dk, w) in offs {
                        let (ci, cj, ck) = (i + *di as i64, j + *dj as i64, k + *dk as i64);
                        if ci < 0
                            || cj < 0
                            || ck < 0
                            || ci >= nx as i64
                            || cj >= ny as i64
                            || ck >= nz as i64
                        {
                            continue;
                        }
                        let nidx = ((ck * ny as i64 + cj) * nx as i64 + ci) as usize;
                        let cand = d[nidx] + w;
                        if cand < d[idx] {
                            d[idx] = cand;
                        }
                    }
                }
            }
        }
    };
    relax(&mut d, &earlier, false);
    relax(&mut d, &later, true);
    d
}

/// Signed-distance bake from a solid-cell mask over the given grid geometry, grown half a
/// spacing past `hull_lo`/`hull_hi` like every sweep. Shared tail of the extend sweeps and
/// of the derived offset/shell bakes (goal M7, step 2).
///
/// Signed EDM: distance to the nearest occupied cell outside the mask, to the nearest
/// empty cell inside it; sign per mask; clamp to the band. Cell units in, mm out.
fn bake_from_mask(
    mask: &[bool],
    dims: [usize; 3],
    origin: [f64; 3],
    spacing: f64,
    hull_lo: [f64; 3],
    hull_hi: [f64; 3],
) -> MeshField {
    let d_out = chamfer_distance_cells(mask, dims, false);
    let d_in = chamfer_distance_cells(mask, dims, true);
    let mut data = vec![0f32; dims[0] * dims[1] * dims[2]];
    let clamp = (BAND_CELLS * spacing) as f32;
    for (i, v) in data.iter_mut().enumerate() {
        let x = if mask[i] { -d_in[i] } else { d_out[i] } * spacing as f32;
        *v = x.clamp(-clamp, clamp);
    }

    let meta = MeshMeta {
        bbox: Aabb {
            lo: [
                hull_lo[0] - 0.5 * spacing,
                hull_lo[1] - 0.5 * spacing,
                hull_lo[2] - 0.5 * spacing,
            ],
            hi: [
                hull_hi[0] + 0.5 * spacing,
                hull_hi[1] + 0.5 * spacing,
                hull_hi[2] + 0.5 * spacing,
            ],
        },
        tris: 0,
        patches: 0,
        open_edges: 0,
    };
    MeshField {
        dist: Arc::new(Grid3 {
            origin,
            spacing,
            dims,
            data,
        }),
        patch: None,
        meta,
    }
}

/// Aabb of a mask's solid cells as cell centres, grown half a spacing — `None` when the
/// mask is empty (an emptied solid; callers fall back to the parent's bounds).
fn mask_hull(mask: &[bool], dims: [usize; 3], origin: [f64; 3], spacing: f64) -> Option<(V3, V3)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (idx, s) in mask.iter().enumerate() {
        if !*s {
            continue;
        }
        let c = decompose_index(idx, dims);
        for a in 0..3 {
            let p = origin[a] + spacing * c[a] as f64;
            lo[a] = lo[a].min(p);
            hi[a] = hi[a].max(p);
        }
    }
    if !lo[0].is_finite() {
        return None;
    }
    Some((lo, hi))
}

/// A derived field sharing the parent's grid geometry (origin, spacing, dims) with a
/// pointwise-transformed distance array — the fast route of [`offset_field`]/[`shell_field`].
/// The stored distances are exact inside the band, so the derived zero set is exact; bounds
/// re-scan from the transformed solid and grow by `pad` mm past the half-spacing the hull
/// convention adds — the transformed zero set can sit up to `pad` past the last solid cell
/// (half a spacing is the parent's own interp distance, the rest is the transform's shift)
/// and derived leaves report their bbox with no margin, so the shift must be inside it.
/// Bounds fall back to the parent's for an emptied solid.
fn pointwise_field(parent: &MeshField, pad: f64, f: impl Fn(f32) -> f32) -> MeshField {
    let g = &*parent.dist;
    let data: Vec<f32> = g.data.iter().map(|v| f(*v)).collect();
    let solid: Vec<bool> = data.iter().map(|v| *v <= 0.0).collect();
    let bbox = match mask_hull(&solid, g.dims, g.origin, g.spacing) {
        Some((lo, hi)) => Aabb {
            lo: lo.map(|v| v - 0.5 * g.spacing - pad),
            hi: hi.map(|v| v + 0.5 * g.spacing + pad),
        },
        None => parent.meta.bbox,
    };
    MeshField {
        dist: Arc::new(Grid3 {
            origin: g.origin,
            spacing: g.spacing,
            dims: g.dims,
            data,
        }),
        patch: None,
        meta: MeshMeta {
            bbox,
            tris: 0,
            patches: 0,
            open_edges: 0,
        },
    }
}

/// The parent's solid sign field (`stored ≤ 0`) resampled onto an expanded grid by nearest
/// cell, clamped at the parent's border. Cells beyond the parent span read its border,
/// which holds positive padding (the parent's solid always lies a band inside its grid),
/// so nothing leaks outward.
fn sampled_mask(parent: &MeshField, dims: [usize; 3], origin: [f64; 3], s: f64) -> Vec<bool> {
    let g = &*parent.dist;
    let [nx, ny, nz] = g.dims;
    let edge = [nx - 1, ny - 1, nz - 1];
    let mut mask = vec![false; dims[0] * dims[1] * dims[2]];
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                let idx = (k * dims[1] + j) * dims[0] + i;
                let p = [
                    origin[0] + s * i as f64,
                    origin[1] + s * j as f64,
                    origin[2] + s * k as f64,
                ];
                let mut c = [0usize; 3];
                for a in 0..3 {
                    c[a] = ((p[a] - g.origin[a]) / g.spacing)
                        .round()
                        .clamp(0.0, edge[a] as f64) as usize;
                }
                mask[idx] = g.data[(c[2] * ny + c[1]) * nx + c[0]] <= 0.0;
            }
        }
    }
    mask
}

/// Geometry of an expanded mask-bake grid: the parent's span grown by `extent` and a fresh
/// band on every side, spacing doubled from the parent's until the cell budget fits — never
/// past [`MAX_DERIVED_SPACING`], where refusal names the node as the extend budget does.
///
/// The margin is snapped up to a whole-cell multiple of `s`, so the expanded samples share
/// the parent's own sample planes whatever the extent's remainder: the nearest-resampled
/// sign field is then the parent's mask unchanged, and the bake's landing stays phase-free.
/// Without the snap a fractional extent interleaves the fresh planes between the parent's,
/// the sign boundary rounds to a neighbour sample, and an isosurface can land past or short
/// of the requested offset by up to half a spacing in either direction.
fn expanded_grid(
    parent: &MeshField,
    extent: f64,
    node: &str,
    what: &str,
) -> Result<([usize; 3], [f64; 3], f64), String> {
    let g = &*parent.dist;
    let span = [
        (g.dims[0] - 1) as f64 * g.spacing,
        (g.dims[1] - 1) as f64 * g.spacing,
        (g.dims[2] - 1) as f64 * g.spacing,
    ];
    let mut s = g.spacing;
    loop {
        let margin = (extent / s).ceil() * s + BAND_CELLS * s;
        let mut dims = [1usize; 3];
        for a in 0..3 {
            dims[a] = ((span[a] + 2.0 * margin) / s).ceil() as usize + 1;
        }
        let cells = dims[0] as u64 * dims[1] as u64 * dims[2] as u64;
        if cells <= MAX_EXTEND_CELLS as u64 {
            let origin = [
                g.origin[0] - margin,
                g.origin[1] - margin,
                g.origin[2] - margin,
            ];
            return Ok((dims, origin, s));
        }
        if s >= MAX_DERIVED_SPACING {
            return Err(format!(
                "the {what} grid of {node} would hold {cells} cells (budget {MAX_EXTEND_CELLS}); raise the mesh's voxel spacing or lower the thickness"
            ));
        }
        s = (s * 2.0).min(MAX_DERIVED_SPACING);
    }
}

/// Offset a hydrated mesh field by `d` mm (positive grows, negative shrinks) into a derived
/// grid — the mesh-operand branch of `Shell` / `Offset` modifiers compiled from documents
/// (goal M7, step 2).
///
/// Within one band (`|d| < [`BAND_CELLS`]·spacing) the bake transforms the parent's stored
/// distances pointwise: the stored values are exact there, so the derived surface is exact
/// and nothing is re-budgeted. From one band out the stored values saturate, so the solid
/// mask (the sign field alone, still right) rebuilds on an expanded grid and redistances
/// through the chamfer pair. The mask bake's isosurface sits half a spacing past the
/// outermost kept cell centre (the convention every extend sweep shares): keeping the ring
/// round half-up lands the surface at least `d` out of the parent's sign boundary and at
/// most one spacing past it, phase-free — the expanded margin is snapped to whole cells,
/// so the landing does not depend on whether `d` divides the parent's spacing.
pub fn offset_field(parent: &MeshField, d: f64, node: &str) -> Result<MeshField, String> {
    if !d.is_finite() || d == 0.0 {
        return Err(format!(
            "the offset distance on {node} is not usable (zero or not finite)"
        ));
    }
    let s = parent.dist.spacing;
    if d.abs() < BAND_CELLS * s {
        let band = (BAND_CELLS * s) as f32;
        let o = d as f32;
        return Ok(pointwise_field(parent, d.abs(), |v| {
            (v - o).clamp(-band, band)
        }));
    }
    let (dims, origin, s_new) = expanded_grid(parent, d.abs(), node, "offset")?;
    let mask = sampled_mask(parent, dims, origin, s_new);
    let d_out = chamfer_distance_cells(&mask, dims, false);
    let d_in = chamfer_distance_cells(&mask, dims, true);
    // The +0.5 rounds the kept ring half-up (outward): the bake surface then sits at
    // outermost-kept + 0.5·s_new — never short of the param, at most one spacing past it.
    // Integer-ratio offsets land half a cell out; non-integer ones round outward first.
    let th = d.abs() / s_new + 0.5;
    let kept: Vec<bool> = if d > 0.0 {
        mask.iter()
            .zip(&d_out)
            .map(|(m, dv)| *m || *dv <= th as f32)
            .collect()
    } else {
        mask.iter()
            .zip(&d_in)
            .map(|(m, dv)| *m && *dv >= th as f32)
            .collect()
    };
    let (hull_lo, hull_hi) = match mask_hull(&kept, dims, origin, s_new) {
        Some(h) => h,
        // An emptied solid is a legitimate result (a shrink that removed everything): the
        // field evaluates as vacuum inside the parent's former bounds.
        None => (parent.meta.bbox.lo, parent.meta.bbox.hi),
    };
    Ok(bake_from_mask(&kept, dims, origin, s_new, hull_lo, hull_hi))
}

/// Shell a hydrated mesh field: a wall of thickness `t` mm hugging the surface
/// — [`SHELL_INSIDE`] eats into the parent, [`SHELL_OUTSIDE`] grows off it,
/// [`SHELL_CENTRE`] straddles it — into a derived grid, matching the eval-time
/// `Expr::Shell` algebra (goal M7, step 2).
///
/// Same two routes as [`offset_field`]: pointwise (exact, `t` within one band) or a mask
/// bake beyond it, whose wall thresholds quantise to whole grid cells the same way.
pub fn shell_field(parent: &MeshField, side: u8, t: f64, node: &str) -> Result<MeshField, String> {
    if !t.is_finite() || t <= 0.0 {
        return Err(format!(
            "the shell thickness on {node} is not usable (zero, negative or not finite)"
        ));
    }
    let s = parent.dist.spacing;
    if t < BAND_CELLS * s {
        let band = (BAND_CELLS * s) as f32;
        let tv = t as f32;
        let derived = match side {
            SHELL_INSIDE => {
                pointwise_field(parent, t, move |v: f32| v.max(-(v + tv)).clamp(-band, band))
            }
            SHELL_OUTSIDE => {
                pointwise_field(parent, t, move |v: f32| (v - tv).max(-v).clamp(-band, band))
            }
            SHELL_CENTRE => pointwise_field(parent, t, move |v: f32| {
                (v.abs() - 0.5 * tv).clamp(-band, band)
            }),
            _ => {
                return Err(format!(
                    "the shell on {node} names an unknown side ({side})"
                ))
            }
        };
        return Ok(derived);
    }
    let (dims, origin, s_new) = expanded_grid(parent, t, node, "shell")?;
    let mask = sampled_mask(parent, dims, origin, s_new);
    let d_out = chamfer_distance_cells(&mask, dims, false);
    let d_in = chamfer_distance_cells(&mask, dims, true);
    // Wall thresholds, depth in whole cells from the surface (zero stored values count as
    // surface cells of the inside parent mask). The +0.5 rounds the kept ring half-up, the
    // same convention as offsets: the measured thickness is the floor of `t/s + 0.5` whole
    // cells, within half a spacing of `t`, and phase-free (the expanded margin is snapped,
    // so a remainder thickness cannot shift either face).
    let th = t / s_new + 0.5;
    let wall: Vec<bool> = match side {
        // The rim of solid within reach of an empty cell — the outer face sits on the
        // parent's surface, so the wall's outer surface stays put.
        SHELL_INSIDE => mask
            .iter()
            .zip(&d_in)
            .map(|(m, dv)| *m && *dv <= th as f32)
            .collect(),
        SHELL_OUTSIDE => mask
            .iter()
            .zip(&d_out)
            .map(|(m, dv)| !*m && *dv <= th as f32)
            .collect(),
        SHELL_CENTRE => mask
            .iter()
            .zip(&d_in)
            .zip(&d_out)
            .map(|((m, dv), ov)| (*m && *dv <= 0.5 * th as f32) || (!*m && *ov <= 0.5 * th as f32))
            .collect(),
        _ => {
            return Err(format!(
                "the shell on {node} names an unknown side ({side})"
            ))
        }
    };
    let (hull_lo, hull_hi) = match mask_hull(&wall, dims, origin, s_new) {
        Some(h) => h,
        None => (parent.meta.bbox.lo, parent.meta.bbox.hi),
    };
    Ok(bake_from_mask(&wall, dims, origin, s_new, hull_lo, hull_hi))
}

static REGISTRY: OnceLock<RwLock<BTreeMap<u64, Arc<MeshField>>>> = OnceLock::new();

fn registry() -> &'static RwLock<BTreeMap<u64, Arc<MeshField>>> {
    REGISTRY.get_or_init(|| RwLock::new(BTreeMap::new()))
}

thread_local! {
    /// One-entry lookup memo. Evals run over millions of points, each passing through
    /// [`lookup`]; documents reference few meshes, so this turns an acquire-on-RwLock plus
    /// two refcounts into a pointer compare. Correct because keys are content addresses —
    /// a re-hydrate of the same key carries identical data.
    static MEMO: RefCell<(u64, Option<Arc<MeshField>>)> = const { RefCell::new((u64::MAX, None)) };
}

fn memo_reset() {
    MEMO.with(|m| *m.borrow_mut() = (u64::MAX, None));
}

/// Bake a field into the registry under `key` (replacing any previous entry) and return the
/// shared handle. Also refreshes this thread's memo so later evals see the new data.
pub fn hydrate(key: u64, field: MeshField) -> Arc<MeshField> {
    let arc = Arc::new(field);
    registry()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, arc.clone());
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.0 == key {
            *m = (key, Some(arc.clone()));
        }
    });
    arc
}

/// Look up a hydrated field. `None` = not in this session: the `Expr` evaluates as vacuum.
pub fn lookup(key: u64) -> Option<Arc<MeshField>> {
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.0 == key {
            return m.1.clone();
        }
        let f = registry()
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        *m = (key, f.clone());
        f
    })
}

/// Remove one field (its document no longer references it). Memo reset is session-wide on
/// this thread; other threads re-memo on their next lookup.
pub fn forget(key: u64) {
    registry()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key);
    memo_reset();
}

/// Remove every field (full session teardown).
pub fn clear_all() {
    registry()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    memo_reset();
}

/// Keys currently hydrated, ascending.
pub fn live_keys() -> Vec<u64> {
    registry()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::Grid3;

    fn tiny_grid(v: f32) -> Grid3 {
        Grid3 {
            origin: [0.0; 3],
            spacing: 1.0,
            dims: [1, 1, 1],
            data: vec![v],
        }
    }

    #[test]
    fn registry_round_trip_and_memo() {
        let k = hash_mesh_input(b"m", b"");
        hydrate(
            k,
            MeshField {
                dist: Arc::new(tiny_grid(-1.0)),
                patch: None,
                meta: MeshMeta {
                    bbox: Aabb {
                        lo: [0.0; 3],
                        hi: [0.0; 3],
                    },
                    tris: 2,
                    patches: 1,
                    open_edges: 0,
                },
            },
        );
        assert_eq!(lookup(k).unwrap().meta.tris, 2);
        forget(k);
        assert!(lookup(k).is_none());
        assert!(!live_keys().contains(&k));
    }

    #[test]
    fn key_from_hex_tolerates_prefix_and_rejects_junk() {
        assert_eq!(key_from_hex(" 0x0123456789abcdef"), 0x0123_4567_89ab_cdef);
        assert_eq!(key_from_hex("fedcba9876543210"), 0xfedc_ba98_7654_3210);
        assert_eq!(key_from_hex("notahash"), 0);
        // Keys of 0 are reserved for malformed hashes.
        assert!(lookup(0).is_none());
    }
}
