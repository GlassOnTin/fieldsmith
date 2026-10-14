//! Voxelising an imported triangle mesh into a signed-distance grid plus a patch grid
//! (goal M6, step 3).
//!
//! Every distance node within reach of the surface holds the exact distance to the nearest
//! triangle (Ericson's point-triangle algorithm over an AABB bucket grid searched ring by
//! ring, remembering the *triangle id*); nodes beyond the search ceiling are signed by a
//! flood from the grid boundary — reachable through no surface node means outside (+band),
//! an enclosed pocket means inside (−band). Values never overestimate the true distance:
//! in-band nodes carry it exactly, everything else clamps to the band radius, which is what
//! raymarching and meshing need. Triangle windings must point outward; STL, OBJ and GLB
//! exports do.
//!
//! The patch grid stores the segmentation id of the nearest triangle (ids 1..=65524 so a
//! face code `PATCH_CODE0 + id` fits a u16) and is [`NO_PATCH`] beyond the band.

use crate::now_ms;
use fieldsmith_field::expr::Aabb;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{MeshField, MeshMeta, BAND_CELLS, NO_PATCH};
use fieldsmith_field::V3;
use fieldsmith_mesh::export::TriMesh;
use fieldsmith_mesh::weld::edge_adjacency;
use rayon::prelude::*;
use std::collections::VecDeque;
use std::sync::Arc;

/// Node budget, in the spirit of `fill.rs`'s MAX_GRID_NODES: a grid beyond this is refused
/// rather than swapped.
pub const MAX_CELLS: usize = 6_000_000;

/// Default-spacing clamps (mm): the printable floor and the coarse ceiling.
pub const MIN_SPACING: f64 = 0.25;
pub const MAX_SPACING: f64 = 8.0;

/// Largest segmentation id the patch grid can hold: the surface code is
/// `PATCH_CODE0 + id`, which must stay below [`NO_PATCH`] (65534 ≤ 65535).
pub const MAX_PATCH_ID: u32 = NO_PATCH as u32 - 1 - 10;

/// How many bucket rings around a node are searched for exact distances. The band is
/// ≤ 1.5 bucket cells (band = 3·spacing, cell ≥ 2·spacing), so ring 2 suffices; ring 4
/// keeps one ring of slack and stretches the exact zone to 4 cells = 2·band.
const SEARCH_RINGS: i64 = 4;

/// The result of [`voxelize`]: a hydrated [`MeshField`] plus cost counters for the
/// import report.
pub struct Voxeled {
    pub field: MeshField,
    /// Number of grid nodes produced.
    pub cells: usize,
    /// Wall-clock build time (ms).
    pub ms: u64,
}

/// Default spacing: ~180³ nodes over the bbox diagonal, clamped to printable sizes.
pub fn default_spacing(b: &Aabb) -> f64 {
    let d = (b.hi[0] - b.lo[0])
        .hypot(b.hi[1] - b.lo[1])
        .hypot(b.hi[2] - b.lo[2]);
    (d / 180.0).clamp(MIN_SPACING, MAX_SPACING)
}

/// Node grid covering `b` with a band margin per axis: the first/last nodes sit
/// `BAND_CELLS·spacing` outside the bbox, so every boundary node is at least band away
/// from the surface and the flood's +side seeds are provably outside.
pub fn grid_dims(b: &Aabb, spacing: f64) -> ([usize; 3], V3) {
    let band = BAND_CELLS * spacing;
    let origin = [b.lo[0] - band, b.lo[1] - band, b.lo[2] - band];
    let dims = [
        ((b.hi[0] - b.lo[0] + 2.0 * band) / spacing).ceil() as usize + 1,
        ((b.hi[1] - b.lo[1] + 2.0 * band) / spacing).ceil() as usize + 1,
        ((b.hi[2] - b.lo[2] + 2.0 * band) / spacing).ceil() as usize + 1,
    ];
    (dims, origin)
}

/// Spacing derivation: an explicit value is taken as-is and checked against the node
/// budget ("raise the voxel size"); the default starts at [`default_spacing`] and widens
/// just enough to fit the budget.
pub fn fit_spacing(b: &Aabb, given: Option<f64>) -> Result<f64, String> {
    match given {
        Some(s) => {
            if !s.is_finite() || s <= 0.0 {
                return Err(format!(
                    "voxel spacing must be finite and positive, got {s}"
                ));
            }
            let (dims, _) = grid_dims(b, s);
            let cells = dims[0] as f64 * dims[1] as f64 * dims[2] as f64;
            if cells as usize > MAX_CELLS {
                return Err(format!(
                    "voxel spacing {s} mm needs {cells:.0} cells (budget {MAX_CELLS}); raise the voxel size"
                ));
            }
            Ok(s)
        }
        None => {
            let mut s = default_spacing(b);
            loop {
                let (dims, _) = grid_dims(b, s);
                let cells = dims[0] as f64 * dims[1] as f64 * dims[2] as f64;
                if cells as usize <= MAX_CELLS {
                    return Ok(s);
                }
                // cells ∝ spacing⁻³ (plus small band terms): grow until it fits.
                let next = (s * (cells / MAX_CELLS as f64).cbrt() * 1.001).min(MAX_SPACING * 2.0);
                if next <= s {
                    return Err(format!(
                        "the mesh needs more than {MAX_CELLS} voxel cells at any spacing; simplify or split it"
                    ));
                }
                s = next;
            }
        }
    }
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Closest point on triangle abc to p (Ericson, *Real-Time Collision Detection* 5.1.5).
fn closest_on_tri(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [a[0] + v * ab[0], a[1] + v * ab[1], a[2] + v * ab[2]];
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [a[0] + w * ac[0], a[1] + w * ac[1], a[2] + w * ac[2]];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [
            b[0] + w * (c[0] - b[0]),
            b[1] + w * (c[1] - b[1]),
            b[2] + w * (c[2] - b[2]),
        ];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [
        a[0] + v * ab[0] + w * ac[0],
        a[1] + v * ab[1] + w * ac[1],
        a[2] + v * ab[2] + w * ac[2],
    ]
}

/// Record one sign vote into the running strongest-vote pair (`neg` holds magnitudes).
fn take_vote(v: f64, pos: &mut f64, neg: &mut f64) {
    if v >= 0.0 {
        *pos = (*pos).max(v);
    } else {
        *neg = (*neg).max(-v);
    }
}

/// AABB bucket grid over the mesh: every triangle is filed into each bucket cell its bbox
/// covers. The bucket grid spans the node grid (same origin, cell ≥ 2·spacing), so tri
/// indices need no clamping.
struct Buckets {
    origin: V3,
    cell: f64,
    dims: [usize; 3],
    lists: Vec<Vec<u32>>,
}

/// Cap on bucket cells per axis, so bucket memory stays small on stretched parts.
const RING_SPAN: usize = 64;

impl Buckets {
    fn build(mesh: &TriMesh, origin: V3, grid_span: V3, spacing: f64) -> Buckets {
        let cell = (spacing * 2.0)
            .max(grid_span[0].max(grid_span[1]).max(grid_span[2]) / RING_SPAN as f64);
        let dims = grid_span.map(|s| (s / cell).floor() as usize + 1);
        let mut lists: Vec<Vec<u32>> = (0..dims[0] * dims[1] * dims[2])
            .map(|_| Vec::new())
            .collect();
        for (ti, t) in mesh.tris.iter().enumerate() {
            let (a, b, c) = (
                mesh.verts[t[0] as usize],
                mesh.verts[t[1] as usize],
                mesh.verts[t[2] as usize],
            );
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for v in [a, b, c] {
                for s in 0..3 {
                    lo[s] = lo[s].min(v[s]);
                    hi[s] = hi[s].max(v[s]);
                }
            }
            let idx = |axis: usize, v: f64| -> usize {
                (((v - origin[axis]) / cell)
                    .floor()
                    .clamp(0.0, (dims[axis] - 1) as f64)) as usize
            };
            for k in idx(2, lo[2])..=idx(2, hi[2]) {
                for j in idx(1, lo[1])..=idx(1, hi[1]) {
                    for i in idx(0, lo[0])..=idx(0, hi[0]) {
                        lists[(k * dims[1] + j) * dims[0] + i].push(ti as u32);
                    }
                }
            }
        }
        Buckets {
            origin,
            cell,
            dims,
            lists,
        }
    }

    fn vote(mesh: &TriMesh, t: u32, p: V3, cp: V3) -> f64 {
        let t3 = mesh.tris[t as usize];
        let (a, b, c) = (
            mesh.verts[t3[0] as usize],
            mesh.verts[t3[1] as usize],
            mesh.verts[t3[2] as usize],
        );
        let n = [
            (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]),
            (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]),
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]),
        ];
        let n2 = dot(n, n);
        if n2 == 0.0 {
            return 0.0; // degenerate tri: no opinion (counts as outside)
        }
        let nl = n2.sqrt();
        (p[0] - cp[0]) * n[0] / nl + (p[1] - cp[1]) * n[1] / nl + (p[2] - cp[2]) * n[2] / nl
    }

    /// Distance and index of the nearest triangle, plus the strongest sign votes among the
    /// triangles tying at the minimum: `pos` is the largest vote ≥ 0 and `neg` the largest
    /// magnitude among votes < 0. `f64::INFINITY` (no tri) means the node is beyond the
    /// exact search zone and gets its sign from the flood instead. The search stops as soon
    /// as unsearched rings cannot hold a strictly closer triangle (they lie at ≥ (r−1)·cell),
    /// so the reported distance is exact whenever it is reported at all.
    fn nearest(&self, mesh: &TriMesh, p: V3) -> (f64, u32, f64, f64) {
        let mut best = f64::INFINITY;
        let mut best_t = u32::MAX;
        let (mut pos, mut neg) = (0.0f64, 0.0f64);
        let mut bc = [0usize; 3];
        for a in 0..3 {
            bc[a] = (((p[a] - self.origin[a]) / self.cell)
                .floor()
                .clamp(0.0, (self.dims[a] - 1) as f64)) as usize;
        }
        // A vote is recorded for every triangle tying at the current minimum. The strongest
        // vote decides the sign: a point inside the material has p−cp pointing into the
        // material for every nearest face, so all its votes are ≤ 0 (outside: ≥ 0), and the
        // largest-|vote| face is the least tangent to p, hence the least degenerate. This
        // resolves the wall-plane case (a node over the extension of a wall, tangent to it,
        // getting a 0 vote from that wall and a real vote from the wall it actually
        // neighbours); exact ties of like magnitude stay outside, as does a zero vote.
        for ring in 0..=SEARCH_RINGS {
            if ring > 0 && best <= ((ring - 1) as f64 * self.cell).powi(2) {
                break;
            }
            for oz in -ring..=ring {
                for oy in -ring..=ring {
                    for ox in -ring..=ring {
                        if ox.abs().max(oy.abs()).max(oz.abs()) != ring {
                            continue;
                        }
                        let (i, j, k) = (bc[0] as i64 + ox, bc[1] as i64 + oy, bc[2] as i64 + oz);
                        if i < 0
                            || j < 0
                            || k < 0
                            || i >= self.dims[0] as i64
                            || j >= self.dims[1] as i64
                            || k >= self.dims[2] as i64
                        {
                            continue;
                        }
                        let (i, j, k) = (i as usize, j as usize, k as usize);
                        for &t in &self.lists[(k * self.dims[1] + j) * self.dims[0] + i] {
                            let t3 = mesh.tris[t as usize];
                            let cp = closest_on_tri(
                                p,
                                mesh.verts[t3[0] as usize],
                                mesh.verts[t3[1] as usize],
                                mesh.verts[t3[2] as usize],
                            );
                            let d2 = dot(sub(p, cp), sub(p, cp));
                            if d2 < best {
                                best = d2;
                                best_t = t;
                                (pos, neg) = (0.0, 0.0);
                                take_vote(Self::vote(mesh, t, p, cp), &mut pos, &mut neg);
                            } else if d2 <= best + best * 1e-9 {
                                // Geometric tie (a node equidistant to two faces of an
                                // edge): both votes stand; the strongest decides.
                                take_vote(Self::vote(mesh, t, p, cp), &mut pos, &mut neg);
                            }
                        }
                    }
                }
            }
        }
        // Exactness certificate: any tri not visited lies at ≥ SEARCH_RINGS·cell, so best
        // is the true nearest distance only when it is at most that far away.
        if best <= (SEARCH_RINGS as f64 * self.cell).powi(2) {
            (best, best_t, pos, neg)
        } else {
            (f64::INFINITY, u32::MAX, 0.0, 0.0)
        }
    }
}

fn bbox_of(verts: &[V3]) -> Aabb {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for v in verts {
        for a in 0..3 {
            lo[a] = lo[a].min(v[a]);
            hi[a] = hi[a].max(v[a]);
        }
    }
    Aabb { lo, hi }
}

/// Flat-index coordinates decompose as `(k·dims[1] + j)·dims[0] + i`. A node is on the
/// grid boundary on any axis at index 0 or dims−1.
fn on_boundary(idx: usize, dims: &[usize; 3]) -> bool {
    let i = idx % dims[0];
    let j = (idx / dims[0]) % dims[1];
    let k = idx / (dims[0] * dims[1]);
    i == 0 || j == 0 || k == 0 || i + 1 == dims[0] || j + 1 == dims[1] || k + 1 == dims[2]
}

fn visit_flood(vals: &mut [f32], q: &mut VecDeque<usize>, nb: usize, band: f32) {
    if vals[nb].is_infinite() {
        vals[nb] = band;
        q.push_back(nb);
    }
}

/// Voxelise a watertight-or-not triangle mesh (mm, outward winding) into a signed-distance
/// [`MeshField`]; `segs` (from `weld::segment`) turns the patch grid on.
pub fn voxelize(
    mesh: &TriMesh,
    segs: Option<&[u32]>,
    spacing: Option<f64>,
) -> Result<Voxeled, String> {
    let t0 = now_ms();
    if mesh.tris.is_empty() {
        return Err("the mesh has no triangles".into());
    }
    if let Some(segs) = segs {
        if segs.len() != mesh.tris.len() {
            return Err(format!(
                "segmentation has {} ids for {} triangles",
                segs.len(),
                mesh.tris.len()
            ));
        }
        if let Some(&mx) = segs.iter().max() {
            if mx > MAX_PATCH_ID {
                return Err(format!(
                    "segmentation id {mx} exceeds the patch-grid cap of {MAX_PATCH_ID} ids"
                ));
            }
        }
    }
    if mesh.verts.iter().any(|v| v.iter().any(|c| !c.is_finite())) {
        return Err("the mesh has a vertex with a non-finite coordinate".into());
    }
    let bbox = bbox_of(&mesh.verts);
    let ext = [
        bbox.hi[0] - bbox.lo[0],
        bbox.hi[1] - bbox.lo[1],
        bbox.hi[2] - bbox.lo[2],
    ];
    for (a, &e) in ext.iter().enumerate() {
        if e <= 0.0 {
            return Err(format!(
                "the mesh is flat along {} ({e} mm extent); it does not bound a solid",
                ["x", "y", "z"][a]
            ));
        }
    }
    let spacing = fit_spacing(&bbox, spacing)?;
    let (dims, origin) = grid_dims(&bbox, spacing);
    let band = BAND_CELLS * spacing;
    let buckets = Buckets::build(
        mesh,
        origin,
        [
            (dims[0] - 1) as f64 * spacing,
            (dims[1] - 1) as f64 * spacing,
            (dims[2] - 1) as f64 * spacing,
        ],
        spacing,
    );
    let n = dims[0] * dims[1] * dims[2];
    debug_assert!(n <= MAX_CELLS);

    // Distance + patch: exact within the search ceiling, INFINITY elsewhere (flood inputs).
    let (mut vals, patches): (Vec<f32>, Vec<u16>) = (0..n)
        .into_par_iter()
        .map(|idx| {
            let i = idx % dims[0];
            let j = (idx / dims[0]) % dims[1];
            let k = idx / (dims[0] * dims[1]);
            let p = [
                origin[0] + (i as f64) * spacing,
                origin[1] + (j as f64) * spacing,
                origin[2] + (k as f64) * spacing,
            ];
            let (d2, t, pos, neg) = buckets.nearest(mesh, p);
            if d2.is_infinite() {
                return (f32::INFINITY, NO_PATCH);
            }
            let ud = d2.sqrt();
            // Sign from the strongest closest-triangle vote (see `nearest`): inside when
            // the negative vote strictly dominates; ties and zero votes stay outside.
            let sign: f64 = if neg > pos { -1.0 } else { 1.0 };
            if ud <= band {
                let patch = match segs {
                    Some(s) => s[t as usize] as u16,
                    None => NO_PATCH,
                };
                ((sign * ud) as f32, patch)
            } else {
                // Exact but beyond the band: a better-than-clamp estimate, no patch claim.
                ((sign * ud) as f32, NO_PATCH)
            }
        })
        .unzip();

    // Flood the unknown nodes from the grid boundary: reachable ⇒ outside (+band),
    // enclosed pockets ⇒ inside (−band). The in-band shell is an impenetrable barrier.
    let band32 = band as f32;
    let mut q: VecDeque<usize> = VecDeque::new();
    for (idx, v) in vals.iter_mut().enumerate() {
        if *v == f32::INFINITY && on_boundary(idx, &dims) {
            *v = band32;
            q.push_back(idx);
        }
    }
    let d0 = dims[0];
    let dd = dims[0] * dims[1];
    while let Some(cur) = q.pop_front() {
        let i = cur % d0;
        let j = (cur / d0) % dims[1];
        let k = cur / dd;
        if i > 0 {
            visit_flood(&mut vals, &mut q, cur - 1, band32);
        }
        if i + 1 < d0 {
            visit_flood(&mut vals, &mut q, cur + 1, band32);
        }
        if j > 0 {
            visit_flood(&mut vals, &mut q, cur - d0, band32);
        }
        if j + 1 < dims[1] {
            visit_flood(&mut vals, &mut q, cur + d0, band32);
        }
        if k > 0 {
            visit_flood(&mut vals, &mut q, cur - dd, band32);
        }
        if k + 1 < dims[2] {
            visit_flood(&mut vals, &mut q, cur + dd, band32);
        }
    }
    for v in vals.iter_mut() {
        if v.is_infinite() {
            *v = -band32;
        }
    }

    let adj = edge_adjacency(mesh);
    // weld_verts deliberately keeps triangles a collapse turned degenerate
    // (two corners merged); edge_adjacency skips those collapsed slots, but
    // only for tris it walks — here the flat census must too, or every
    // degenerate's phantom (a,a) slot counts as an open edge.
    let open_edges = adj
        .nbr
        .iter()
        .enumerate()
        .flat_map(|(ti, t)| {
            t.iter().enumerate().filter(move |&(k, &e)| {
                e == u32::MAX && mesh.tris[ti][k] != mesh.tris[ti][(k + 1) % 3]
            })
        })
        .count() as u32;
    let patches_count = segs.map_or(0, |s| s.iter().max().copied().unwrap_or(0));
    let meta = MeshMeta {
        bbox,
        tris: mesh.tris.len() as u32,
        patches: patches_count,
        open_edges,
    };
    let field = MeshField {
        dist: Arc::new(Grid3 {
            origin,
            spacing,
            dims,
            data: vals,
        }),
        patch: segs.map(|_| Arc::new(patches)),
        meta,
    };
    Ok(Voxeled {
        field,
        cells: n,
        ms: (now_ms() - t0) as u64,
    })
}
