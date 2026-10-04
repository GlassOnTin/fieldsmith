//! Adaptive dual-contouring surface of an implicit solid (the export path).
//!
//! The uniform mesher ([`crate::stuff`]) samples every lattice vertex in the box: at 0.1 mm
//! over a 100 mm part that is ~2.4e8 field evaluations for one boundary shell, which is why
//! the GOAL export budget (3MF <= 15 s at 0.1 mm) measured 411 s. This mesher refines an
//! octree only where the surface can be. A candidate cell becomes a leaf as soon as its
//! corners certify that no isosurface passes through it: either the corners straddle (then a
//! crossing must be resolved and the cell refines), or, sharing one sign, every corner value
//! stands farther from zero than the local gradient bound lets the surface come within the
//! cell (`min|f| > lip * max|g| * half-diagonal`). Interior and far-field cells stop one or
//! two refinements down; a band around the isosurface refines to `h`, where the quad
//! assembly is marching-cubes class. At the finest candidate level every cell is a leaf.
//!
//! Surface assembly is dual. Every leaf gets one vertex: a mass-point-regularised QEF over
//! the crossings of its own sign-changing edges (normal of each plane from the nearer
//! endpoint's gradient, which keeps planar faces planar), or the cell centre when no edge is
//! crossed. Every face of a leaf whose corner signs straddle emits one quad spanning the
//! face's two crossed perimeter edges, owned by the finer leaf of the pair (lower key at
//! equal size, itself when the occupant across is coarser). Quad corners on perimeter edges
//! are crossings of the fine-grid edge, deduplicated by edge key: the two faces sharing an
//! edge both contain that edge and so both straddle, and each face pair is emitted exactly
//! once, so each crossing is used by exactly one quad on either side of the edge and the
//! mesh closes across arbitrary level jumps without a balance pass. A face whose four
//! perimeter crossings alternate (the marching-squares saddle) emits two quads paired in
//! perimeter order, the one configuration the corner signs do not settle.
//!
//! Zero-valued corner samples fold into the inside (v <= 0 is solid), as in the uniform
//! mesher: a surface lying on a lattice plane resolves to in-plane quads on the outer side
//! of the plane, whose inner-side neighbour face then does not straddle, so exactly one
//! surface is built.

use crate::{V3};
use crate::export::TriMesh;
use rayon::prelude::*;
use std::collections::HashMap;

/// One field sample: value (mm, negative inside) and spatial gradient (from
/// `expr::eval`'s dual numbers, which carry it for free).
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub v: f64,
    pub g: V3,
}

/// Refinement knobs. `lip` scales the gradient bound in the no-surface certificate: 1.0 is
/// the tight bound; larger refines a wider band (safer when the field's gradient grows
/// between corners, at linear cost in band thickness). Default 2.0.
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveOpts {
    pub lip: f64,
}

impl Default for AdaptiveOpts {
    fn default() -> Self {
        AdaptiveOpts { lip: 2.0 }
    }
}

/// Fine-grid coordinates: 21 bits per axis, so the domain side is at most 2^21 fine units
/// (209 m at 0.1 mm). Cell and corner keys pack `(x, y, z)` the same way, x highest.
const MAX_UNITS: u32 = 1 << 21;

#[inline]
fn fine_key(x: u64, y: u64, z: u64) -> u64 {
    (x << 42) | (y << 21) | z
}
#[inline]
fn key_x(k: u64) -> u64 {
    k >> 42
}
#[inline]
fn key_y(k: u64) -> u64 {
    (k >> 21) & 0x1f_ffff
}
#[inline]
fn key_z(k: u64) -> u64 {
    k & 0x1f_ffff
}
#[inline]
fn gmag(s: Sample) -> f64 {
    (s.g[0] * s.g[0] + s.g[1] * s.g[1] + s.g[2] * s.g[2]).sqrt()
}
#[inline]
fn gdot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Corner bit = x + 2y + 4z. Face k (0:-x 1:+x 2:-y 3:+y 4:-z 5:+z) perimeter corners in
/// cyclic order; consecutive pairs are the face's perimeter edges.
const FACE_CORNERS: [[u8; 4]; 6] = [
    [0, 4, 6, 2],
    [1, 5, 7, 3],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 2, 3, 1],
    [4, 5, 7, 6],
];
/// The cell's corner in direction `bit` (fine-corner key at the cell's own level).
#[inline]
fn corner_key(cell: u64, step: u64, bit: u64) -> u64 {
    let (bx, by, bz) = (bit & 1, (bit >> 1) & 1, (bit >> 2) & 1);
    fine_key(key_x(cell) + bx * step, key_y(cell) + by * step, key_z(cell) + bz * step)
}
/// Fine-unit offset of face k's neighbouring cell.
#[inline]
fn face_neighbour(cell: u64, step: u64, f: usize) -> (i64, i64, i64) {
    const OFF: [[i64; 3]; 6] = [
        [-1, 0, 0],
        [1, 0, 0],
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
    ];
    let o = OFF[f];
    (
        key_x(cell) as i64 + o[0] * step as i64,
        key_y(cell) as i64 + o[1] * step as i64,
        key_z(cell) as i64 + o[2] * step as i64,
    )
}
/// The edge key is the sorted pair of its endpoint corner keys.
#[inline]
fn edge_key(a: u64, b: u64) -> (u64, u64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// One octree level: tested cells (sorted by cell key) and their corner samples (sorted by
/// corner key). Kept until stitching, which reads neighbours' corners.
struct Level {
    /// 2^d cells per side.
    units: u64,
    cell_keys: Vec<u64>,
    refined: Vec<bool>,
    /// Flat leaf id per cell (u32::MAX when refined).
    leaf_ix: Vec<u32>,
    corner_keys: Vec<u64>,
    corner_vals: Vec<Sample>,
}

/// Per-leaf record for the stitch passes.
struct LeafRec {
    /// The leaf's dual vertex (QEF, or the cell centre when nothing is crossed).
    qpos: V3,
    /// Bit k set = face k's corner signs straddle.
    mixed: u8,
    /// The 8 corner values, corner-bit order.
    vals: [f64; 8],
    /// Crossings of this leaf's sign-changing edges (empty for unmixed leaves).
    xings: Vec<Xing>,
}

/// Crossing of the isosurface through a fine-grid edge: sorted endpoint keys, the
/// linear-interpolation point, and the unit normal at the crossing (nearer endpoint's
/// gradient) for the QEF planes and quad orientation.
#[derive(Clone, Copy)]
struct Xing {
    ekey: (u64, u64),
    p: V3,
    n: V3,
}

/// Who occupies the region of a queried cell: the leaf that covers it, refined cells (only
/// possible for the query cell itself), or nothing (kept as a defensive case; the root is
/// always present, so this only means "outside the domain").
enum Occupant {
    None,
    Refined,
    Leaf { level: u32, flat: u32 },
}

/// Walk up from `from_level` to the leaf covering the queried cell, given as the neighbouring
/// cell's min-corner key at `from_level`. Ancestors strictly below a present cell are absent
/// from the tested set by construction, so the first present ancestor is the covering leaf.
fn occupant(levels: &[Level], depth: u32, from_level: u32, nk: u64) -> Occupant {
    for dd in (0..=from_level as u64).rev() {
        let step = 1u64 << (depth as u64 - dd);
        // Floor each axis to this level's grid: the cell containing the queried region.
        let ax = (key_x(nk) / step) * step;
        let ay = (key_y(nk) / step) * step;
        let az = (key_z(nk) / step) * step;
        let ak = fine_key(ax, ay, az);
        let lvl = &levels[dd as usize];
        if let Ok(ci) = lvl.cell_keys.binary_search(&ak) {
            if lvl.refined[ci] {
                debug_assert_eq!(dd as u32, from_level, "only the queried cell itself can be refined");
                return Occupant::Refined;
            }
            return Occupant::Leaf { level: dd as u32, flat: lvl.leaf_ix[ci] };
        }
    }
    Occupant::None
}

/// Mesh the surface of the solid `{p : field(p).v <= 0}` in a cube centred on `[lo, hi]`
/// whose side is a power-of-two number of leaves of size `h` and covers the box with at
/// least 2.5 leaves to spare.
pub fn surface_adaptive<F: Fn(V3) -> Sample + Sync>(
    field: F,
    lo: V3,
    hi: V3,
    h: f64,
    opts: &AdaptiveOpts,
) -> TriMesh {
    assert!(h > 0.0, "leaf size must be positive");
    let mut need = 2.5f64;
    for a in 0..3 {
        need = need.max((hi[a] - lo[a]).max(0.0) / h + 2.5);
    }
    let w = (need.ceil() as u64).next_power_of_two();
    assert!(
        w <= MAX_UNITS as u64,
        "domain side {} fine units exceeds the 2^21 key space (h = {} mm)",
        w,
        h
    );
    let depth = w.trailing_zeros();
    let side = w as f64 * h;
    let origin: V3 = std::array::from_fn(|a| (lo[a] + hi[a]) * 0.5 - side * 0.5);

    // ---- refine level by level; only refined cells' children are candidates ----------------
    let mut todo: Vec<u64> = vec![0];
    let mut levels: Vec<Level> = Vec::with_capacity(depth as usize + 1);
    for d in 0..=depth {
        let step = 1u64 << (depth - d);
        let units = 1u64 << d;
        todo.sort_unstable();
        todo.dedup();

        // All candidates' corners, deduplicated (adjacent cells share corners), evaluated in
        // parallel. Fixed chunks of an indexed parallel map preserve order, so the sample
        // values — and everything built from them — are run-to-run identical.
        let mut ckeys: Vec<u64> = Vec::with_capacity(todo.len() * 8);
        for &ck in &todo {
            for bit in 0..8u64 {
                ckeys.push(corner_key(ck, step, bit));
            }
        }
        ckeys.par_sort_unstable();
        ckeys.dedup();
        let chunk = 1usize << 13;
        let corner_vals: Vec<Sample> = ckeys
            .par_chunks(chunk)
            .flat_map_iter(|ch| {
                ch.iter()
                    .map(|&k| {
                        let p = [
                            origin[0] + key_x(k) as f64 * h,
                            origin[1] + key_y(k) as f64 * h,
                            origin[2] + key_z(k) as f64 * h,
                        ];
                        let mut s = field(p);
                        if s.v.abs() < 1e-9 * h {
                            s.v = 0.0; // plane-aligned snap, same convention as the uniform mesher
                        }
                        s
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // Certify or refine each candidate (parallel); children assembled sequentially.
        let refined: Vec<bool> = todo
            .par_iter()
            .map(|&ck| {
                let mut inside = false;
                let mut outside = false;
                let mut vmin = f64::INFINITY;
                let mut gmax = 0.0f64;
                for c in 0..8u64 {
                    let s = corner_vals[lookup(&ckeys, corner_key(ck, step, c))];
                    if s.v <= 0.0 {
                        inside = true;
                    } else {
                        outside = true;
                    }
                    vmin = vmin.min(s.v.abs());
                    gmax = gmax.max(gmag(s));
                }
                // Refine when the surface may pass through the cell. At the finest level all
                // candidates are leaves: the boundary then has h resolution.
                d < depth && ((inside && outside) || vmin <= opts.lip * 0.5 * (step as f64 * h) * 3f64.sqrt() * gmax)
            })
            .collect();
        let mut next: Vec<u64> = Vec::new();
        for (ci, &ck) in todo.iter().enumerate() {
            if !refined[ci] {
                continue;
            }
            let sub = step / 2;
            let (ix, iy, iz) = (key_x(ck), key_y(ck), key_z(ck));
            for b in 0..8u64 {
                let (bx, by, bz) = (b & 1, (b >> 1) & 1, (b >> 2) & 1);
                next.push(fine_key(ix + bx * sub, iy + by * sub, iz + bz * sub));
            }
        }
        levels.push(Level {
            units,
            cell_keys: std::mem::take(&mut todo),
            refined,
            leaf_ix: Vec::new(),
            corner_keys: ckeys,
            corner_vals,
        });
        todo = next;
    }

    // ---- flat leaf list (level-major, key-sorted) and per-level cell -> leaf index ---------
    let mut leaves: Vec<(u32, u64)> = Vec::new();
    {
        let mut ix: Vec<Vec<u32>> = levels.iter().map(|l| vec![u32::MAX; l.cell_keys.len()]).collect();
        for (li, lvl) in levels.iter().enumerate() {
            for (ci, &ck) in lvl.cell_keys.iter().enumerate() {
                if !lvl.refined[ci] {
                    ix[li][ci] = leaves.len() as u32;
                    leaves.push((li as u32, ck));
                }
            }
        }
        for (li, lvl) in levels.iter_mut().enumerate() {
            lvl.leaf_ix = std::mem::take(&mut ix[li]);
        }
    }

    // ---- per-leaf records (parallel): vertex, mixed faces, crossings -----------------------
    let chunk = 1usize << 12;
    let per_leaf: Vec<LeafRec> = leaves
        .par_chunks(chunk)
        .flat_map_iter(|blk| {
            blk.iter().map(|&(li, key)| leaf_record(&levels, li as usize, key, depth, &origin, h))
        })
        .collect();

    // ---- stitch: one vertex per leaf, canonical crossings, quads ---------------------------
    // Vertex ids: leaves in flat order own the id slots. Only mixed leaves are referenced
    // (an emitted pair straddles on the shared face, so both of its cells straddle).
    let mut leaf_vert: Vec<u32> = Vec::with_capacity(per_leaf.len());
    let mut verts: Vec<V3> = Vec::new();
    for rec in &per_leaf {
        if rec.mixed != 0 {
            leaf_vert.push(verts.len() as u32);
            verts.push(rec.qpos);
        } else {
            leaf_vert.push(u32::MAX);
        }
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    // Crossing vertices are keyed by fine-grid edge; whichever leaf first emits the edge's
    // crossing creates the vertex, later users reuse it. Points computed from the same
    // corner samples interpolate identically, so this is exact.
    let mut crossing_vert: HashMap<(u64, u64), u32> = HashMap::new();
    for (li, rec) in per_leaf.iter().enumerate() {
        if rec.mixed == 0 {
            continue;
        }
        let (lvl_ix, ckey) = leaves[li];
        let lvl = &levels[lvl_ix as usize];
        let step = 1u64 << (depth - lvl_ix);
        let va = leaf_vert[li];
        for f in 0..6usize {
            if rec.mixed & (1 << f) == 0 {
                continue;
            }
            // Face on the domain boundary: no occupant, skip (the field is positive well
            // before the boundary, so this face carries no surface).
            let (nx, ny, nz) = face_neighbour(ckey, step, f);
            if nx < 0 || ny < 0 || nz < 0 {
                continue;
            }
            let (nx, ny, nz) = (nx as u64, ny as u64, nz as u64);
            if nx >= lvl.units * step || ny >= lvl.units * step || nz >= lvl.units * step {
                continue;
            }
            match occupant(&levels, depth, lvl_ix, fine_key(nx, ny, nz)) {
                Occupant::None | Occupant::Refined => continue,
                Occupant::Leaf { level: od, flat: bf } => {
                    // Canonical owner: the finer leaf of the pair, the lower key at equal
                    // size. A coarser occupant sees me as the fine side and skips; refined
                    // neighbours' children emit their sub-faces.
                    if od == lvl_ix && !(ckey < leaves[bf as usize].1) {
                        continue;
                    }
                    emit_face(
                        rec,
                        va,
                        leaf_vert[bf as usize],
                        f,
                        ckey,
                        step,
                        &mut verts,
                        &mut tris,
                        &mut crossing_vert,
                    );
                }
            }
        }
    }

    // Crossings sitting exactly on a corner (t = 0 or 1: the plane-aligned snap) coincide
    // positionally across different edges; weld them so the surface is edge-closed there.
    weld_exact(&mut verts, &mut tris);
    orient(&mut tris);
    // Per-quad winding is only a first guess: at plane-aligned creases the field gradient
    // ties between planes and emission can wind a band of quads backwards. Consistency is
    // restored by `orient`; with the surface closed its global sign is then uniform, so
    // the windings whose volume comes out negative are flipped to the outward convention.
    if signed_volume(&verts, &tris) < 0.0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    compact(&mut verts, &mut tris);
    TriMesh { verts, tris }
}

/// Emit the quad(s) for a straddling face between leaf A (me, `arec`, vertex `va`) and the
/// leaf `vb` on the other side. Quad corners: A's vertex, B's vertex, and the two crossings
/// of the face's perimeter edges, keyed by the fine-grid edge. Winding is a first guess
/// (against the average crossing normal) and is settled by `orient` plus the volume sign.
#[allow(clippy::too_many_arguments)]
fn emit_face(
    arec: &LeafRec,
    va: u32,
    vb: u32,
    f: usize,
    ckey: u64,
    step: u64,
    verts: &mut Vec<V3>,
    tris: &mut Vec<[u32; 3]>,
    crossing_vert: &mut HashMap<(u64, u64), u32>,
) {
    // A strip between two cells whose fan centres coincide (both collapse onto a plane
    // seam, where every crossing snaps to the same lattice plane) carries no area: emit
    // nothing, and let the weld join the two halves directly.
    {
        let (p, q) = (verts[va as usize], verts[vb as usize]);
        if (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs() < 1e-9 * 0.5 {
            return;
        }
    }
    let bits = FACE_CORNERS[f];
    let mut xp = [0u32; 4]; // crossing vertex ids, perimeter order
    let mut xn = [V3::default(); 4];
    let mut n_x = 0usize;
    for e in 0..4 {
        let (b0, b1) = (bits[e] as u64, bits[(e + 1) % 4] as u64);
        let (a_, b_) = (arec.vals[b0 as usize], arec.vals[b1 as usize]);
        if !((a_ <= 0.0 && b_ > 0.0) || (b_ <= 0.0 && a_ > 0.0)) {
            continue;
        }
        let ek = edge_key(corner_key(ckey, step, b0), corner_key(ckey, step, b1));
        let x = xings_find(arec, ek);
        let id = *crossing_vert.entry(ek).or_insert_with(|| {
            verts.push(x.p);
            (verts.len() - 1) as u32
        });
        xp[n_x] = id;
        xn[n_x] = x.n;
        n_x += 1;
    }
    debug_assert!(n_x == 2 || n_x == 4, "straddling face with {} crossings", n_x);
    if n_x != 2 && n_x != 4 {
        return;
    }
    // A face whose two crossings coincide positionally (both snapped to one lattice corner
    // by the plane-alignment fold) carries no area: emit nothing, its strip has zero width.
    let coincident = |i: usize, j: usize| -> bool {
        let (p, q) = (verts[xp[i] as usize], verts[xp[j] as usize]);
        (p[0] - q[0]).abs() + (p[1] - q[1]).abs() + (p[2] - q[2]).abs() < 1e-9 * 0.5
    };
    if n_x == 2 {
        if coincident(0, 1) {
            return;
        }
        emit_quad(va, vb, xp[0], xp[1], xn[0], xn[1], verts, tris);
    } else {
        // Saddle: the face's perimeter crosses the surface twice. Pair the crossings whose
        // gradient normals agree — the two boundary crossings of one sheet share its normal,
        // a pair straddling two sheets does not. The alternative pairing covers the second
        // topology; ties (zero gradients) keep the perimeter-order pairing.
        let g = [gmag_at(xn[0]), gmag_at(xn[1]), gmag_at(xn[2]), gmag_at(xn[3])];
        let sim = |i: usize, j: usize| -> f64 {
            if g[i] > 1e-30 && g[j] > 1e-30 {
                gdot(xn[i], xn[j]) / (g[i] * g[j])
            } else {
                0.0
            }
        };
        let d01 = sim(0, 1) + sim(2, 3);
        let d12 = sim(1, 2) + sim(3, 0);
        if d01 >= d12 {
            emit_quad(va, vb, xp[0], xp[1], xn[0], xn[1], verts, tris);
            emit_quad(va, vb, xp[2], xp[3], xn[2], xn[3], verts, tris);
        } else {
            emit_quad(va, vb, xp[1], xp[2], xn[1], xn[2], verts, tris);
            emit_quad(va, vb, xp[3], xp[0], xn[3], xn[0], verts, tris);
        }
    }
}

/// Two triangles for one quad of corners (a, x0, b, x1) in cyclic order, flipped so the
/// normal of the first triangle agrees with the crossing normals. The diagonal triangle
/// (a, x0, x1) is degenerate along a straight strip and must not be the reference; testing
/// with the actual first triangle is what this does. This is only a guess: `orient` and the
/// global volume sign settle the final winding.
#[allow(clippy::too_many_arguments)]
fn emit_quad(
    va: u32,
    vb: u32,
    x0: u32,
    x1: u32,
    n0: V3,
    n1: V3,
    verts: &[V3],
    tris: &mut Vec<[u32; 3]>,
) {
    let mut m = [n0[0] + n1[0], n0[1] + n1[1], n0[2] + n1[2]];
    if m[0] * m[0] + m[1] * m[1] + m[2] * m[2] < 1e-30 {
        // Degenerate gradients at both crossings: orient by the vertex-to-vertex direction.
        let (p, q) = (verts[va as usize], verts[vb as usize]);
        m = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    }
    let (p0, q0, b) = (verts[va as usize], verts[x0 as usize], verts[vb as usize]);
    let u = [q0[0] - p0[0], q0[1] - p0[1], q0[2] - p0[2]];
    let v = [b[0] - p0[0], b[1] - p0[1], b[2] - p0[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let aligned = n[0] * m[0] + n[1] * m[1] + n[2] * m[2] >= 0.0;
    let (s, t) = if aligned { (x0, x1) } else { (x1, x0) };
    tris.push([va, s, vb]);
    tris.push([va, vb, t]);
}

/// The leaf's record: corner values and positions from its level's tables, mixed-face mask,
/// QEF vertex, and its sign-change edges' crossings.
fn leaf_record(
    levels: &[Level],
    li: usize,
    key: u64,
    depth: u32,
    origin: &V3,
    h: f64,
) -> LeafRec {
    let lvl = &levels[li];
    let step = 1u64 << (depth - li as u32);
    let (ix, iy, iz) = (key_x(key), key_y(key), key_z(key));
    let mut vals = [0.0f64; 8];
    let mut gs = [[0.0f64; 3]; 8];
    let mut pos = [[0.0f64; 3]; 8];
    for b in 0..8u64 {
        let c = corner_key(key, step, b);
        let s = lvl.corner_vals[lookup(&lvl.corner_keys, c)];
        vals[b as usize] = s.v;
        gs[b as usize] = s.g;
        let (bx, by, bz) = (b & 1, (b >> 1) & 1, (b >> 2) & 1);
        pos[b as usize] = [
            origin[0] + (ix + bx * step) as f64 * h,
            origin[1] + (iy + by * step) as f64 * h,
            origin[2] + (iz + bz * step) as f64 * h,
        ];
    }
    // Mixed-face mask: face k straddles iff its 4 corners are not uniform.
    let mut mixed = 0u8;
    for (k, face) in FACE_CORNERS.iter().enumerate() {
        let mut any_in = false;
        let mut any_out = false;
        for &b in face {
            if vals[b as usize] <= 0.0 {
                any_in = true;
            } else {
                any_out = true;
            }
        }
        if any_in && any_out {
            mixed |= 1 << k;
        }
    }
    // Crossings of the leaf's 12 edges; QEF planes with nearer-endpoint normals.
    const EDGES: [(u8, u8); 12] = [
        (0, 1), (2, 3), (4, 5), (6, 7), // along x
        (0, 2), (1, 3), (4, 6), (5, 7), // along y
        (0, 4), (1, 5), (2, 6), (3, 7), // along z
    ];
    let mut planes: Vec<(V3, V3)> = Vec::new();
    let mut xings: Vec<Xing> = Vec::new();
    for (a, b) in EDGES {
        let (ua, ub) = (vals[a as usize], vals[b as usize]);
        if !((ua <= 0.0 && ub > 0.0) || (ub <= 0.0 && ua > 0.0)) {
            continue;
        }
        let t = (ua / (ua - ub)).clamp(0.0, 1.0);
        let mut p = [0.0f64; 3];
        for i in 0..3 {
            p[i] = pos[a as usize][i] + (pos[b as usize][i] - pos[a as usize][i]) * t;
        }
        // Normal from the nearer endpoint's gradient (exact on plane-aligned faces).
        let (ga, gb) = (gs[a as usize], gs[b as usize]);
        let (na, nb) = (gmag_at(ga), gmag_at(gb));
        let mut n = if ua.abs() <= ub.abs() { ga } else { gb };
        if na < 1e-30 && nb < 1e-30 {
            n = [0.0, 0.0, 0.0]; // no usable gradient: QEF skips, orientation falls back
        }
        planes.push((p, n));
        xings.push(Xing { ekey: edge_key(corner_key(key, step, a as u64), corner_key(key, step, b as u64)), p, n });
    }
    let qpos = qef_vertex(&planes, key, step, origin, h);
    LeafRec { qpos, mixed, vals, xings }
}

/// Mass-point QEF over the leaf's crossing planes: minimise sum (n·(q - x))^2 + mu |q - c|^2,
/// clamped into the cell. mu = 1e-3 of one plane regularises the mass point.
fn qef_vertex(planes: &[(V3, V3)], key: u64, step: u64, origin: &V3, h: f64) -> V3 {
    if planes.is_empty() {
        let (ix, iy, iz) = (key_x(key), key_y(key), key_z(key));
        return [
            origin[0] + (ix + step / 2) as f64 * h,
            origin[1] + (iy + step / 2) as f64 * h,
            origin[2] + (iz + step / 2) as f64 * h,
        ];
    }
    let mu = 1e-3;
    let mut a = [0.0f64; 9];
    let mut b = [0.0f64; 3];
    let mut c = [0.0f64; 3];
    for (x, n) in planes {
        let dot = n[0] * x[0] + n[1] * x[1] + n[2] * x[2];
        for i in 0..3 {
            c[i] += x[i];
            if n[i] == 0.0 {
                continue;
            }
            for j in 0..3 {
                a[i * 3 + j] += n[i] * n[j];
            }
            b[i] += n[i] * dot;
        }
    }
    for i in 0..3 {
        a[i * 4] += mu;
        c[i] /= planes.len() as f64;
        b[i] += mu * c[i]; // the mass point anchors at the crossings' centroid, not the origin
    }
    let solved = match cholesky3(a, b, 1e-14) {
        Some(q) => q,
        None => c,
    };
    let mut q = solved;
    if !q.iter().all(|x| x.is_finite()) {
        q = c;
    }
    // Clamp into the cell.
    let lo = [
        origin[0] + key_x(key) as f64 * h,
        origin[1] + key_y(key) as f64 * h,
        origin[2] + key_z(key) as f64 * h,
    ];
    let hi = [
        origin[0] + (key_x(key) + step) as f64 * h,
        origin[1] + (key_y(key) + step) as f64 * h,
        origin[2] + (key_z(key) + step) as f64 * h,
    ];
    for i in 0..3 {
        q[i] = q[i].clamp(lo[i].min(hi[i]) + 0.0, lo[i].max(hi[i]));
    }
    q
}

/// Cholesky solve of a symmetric row-major 3x3 system. None when not numerically SPD.
fn cholesky3(a: [f64; 9], b: [f64; 3], eps: f64) -> Option<[f64; 3]> {
    let l11 = (a[0] - eps).sqrt();
    if l11 <= 0.0 {
        return None;
    }
    let l21 = a[3] / l11;
    let d22 = a[4] - l21 * l21 - eps;
    if d22 <= 0.0 {
        return None;
    }
    let l22 = d22.sqrt();
    let l31 = a[6] / l11;
    let l32 = (a[7] - l31 * l21) / l22;
    let d33 = a[8] - l31 * l31 - l32 * l32;
    if d33 <= -eps {
        return None;
    }
    let l33 = d33.max(0.0).sqrt();
    if l33 <= 0.0 {
        return None;
    }
    // forward: L y = b
    let y0 = b[0] / l11;
    let y1 = (b[1] - l21 * y0) / l22;
    let y2 = (b[2] - l31 * y0 - l32 * y1) / l33;
    // backward: L^T x = y
    let x2 = y2 / l33;
    let x1 = (y1 - l32 * x2) / l22;
    let x0 = (y0 - l21 * x1 - l31 * x2) / l11;
    Some([x0, x1, x2])
}

#[inline]
fn gmag_at(g: V3) -> f64 {
    (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt()
}

#[inline]
fn lookup(keys: &[u64], k: u64) -> usize {
    keys.binary_search(&k).expect("generated corner key missing from its own table")
}

#[inline]
fn xings_find(rec: &LeafRec, ek: (u64, u64)) -> Xing {
    rec.xings.iter().copied().find(|x| x.ekey == ek).expect("perimeter crossing missing from the leaf's edge records")
}

/// Weld exactly coincident vertices (bit-identical f64 positions). Positions are compared as
/// raw bits; genuinely distinct crossings are a leaf-size apart, so only snap cases match:
/// crossings that land on a shared corner (t = 0 or 1), leaf fan centres whose QEF point
/// lands exactly on a lattice corner, and a fan centre that lands on such a crossing's
/// corner. Keeping a fan centre separate from a coincident crossing does not preserve the
/// cell's quads — they are already zero-length edges and slits — it only leaves the slit
/// open on each side (open edges, phantom self-intersections at the identical point), so
/// everything at one position becomes one vertex and the pinch is what it already is.
///
/// Welding keys are bit triples hashed with a fixed-seed fx-style hasher (no random seed, so
/// run-to-run behaviour is identical) — tens of millions of bit keys at SipHash rates cost
/// seconds the export budget does not have.
fn weld_exact(verts: &mut [V3], tris: &mut Vec<[u32; 3]>) {
    type Key = (u64, u64, u64);
    let mut seen: HashMap<Key, u32, BuildFx> = HashMap::default();
    let new_ids: Vec<u32> = (0..verts.len())
        .map(|i| {
            let v = verts[i];
            let k = (v[0].to_bits(), v[1].to_bits(), v[2].to_bits());
            match seen.get(&k) {
                Some(&head) => head,
                None => {
                    seen.insert(k, i as u32);
                    i as u32
                }
            }
        })
        .collect();
    for t in &mut *tris {
        for c in t.iter_mut() {
            *c = new_ids[*c as usize];
        }
    }
}

/// fx-style hasher (rustc's): word-wise rotate-xor-multiply, no seed, hence deterministic
/// hashing for the weld keys.
#[derive(Default, Clone, Copy)]
struct BuildFx;
impl std::hash::BuildHasher for BuildFx {
    type Hasher = FxHasher;
    fn build_hasher(&self) -> FxHasher {
        FxHasher(0)
    }
}
struct FxHasher(u64);
impl std::hash::Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.0 = (self.0 ^ u64::from_le_bytes(c.try_into().expect("8 bytes")))
                .rotate_left(5)
                .wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        if !chunks.remainder().is_empty() {
            let mut buf = [0u8; 8];
            buf[..chunks.remainder().len()].copy_from_slice(chunks.remainder());
            self.0 = (self.0 ^ u64::from_le_bytes(buf))
                .rotate_left(5)
                .wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0 ^ i).rotate_left(5).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

/// Orient triangles consistently within each edge-connected component, then leave the
/// component's global sign to the caller, which fixes the overall winding from the sign
/// of the enclosed volume (a closed, consistently-oriented surface has one global sign;
/// per-quad references are unreliable exactly where this pass matters, at plane-aligned
/// creases whose field gradient ties between planes).
///
/// Propagation only crosses manifold edges (two incident triangles): a non-manifold seam
/// edge carries more, and no 2-colouring exists there.
fn orient(tris: &mut [ [u32; 3] ]) {
    // Undirected edge -> the two directed half-edges through it, laid out contiguously by
    // sorting (packed edge key, half-edge index). At tens of millions of edges a sorted
    // adjacency array is several times faster than a hash map of per-key vectors.
    let n_t = tris.len();
    let nh = 3 * n_t;
    let mut he: Vec<(u64, u32)> = Vec::with_capacity(nh);
    for (ti, t) in tris.iter().enumerate() {
        let base = 3 * ti as u32;
        for e in 0..3 {
            let (u, v) = (t[e], t[(e + 1) % 3]);
            // Self-edges of degenerate triangles carry no adjacency: keep them in the
            // array (their group is never a propagator) so the indices stay uniform.
            let key = ((u.min(v) as u64) << 32) | u.max(v) as u64;
            he.push((key, base + e as u32));
        }
    }
    he.par_sort_unstable();

    // Group boundaries and each half-edge's group: a new group starts where the key
    // changes. Serial scan of the sorted array, one pass.
    let mut gid: Vec<u32> = vec![0; nh];
    let mut starts: Vec<u32> = Vec::new();
    {
        let mut prev: Option<u64> = None;
        for (j, &(k, idx)) in he.iter().enumerate() {
            if prev != Some(k) {
                starts.push(j as u32);
                prev = Some(k);
            }
            gid[idx as usize] = (starts.len() - 1) as u32;
        }
    }

    let mut seen = vec![false; n_t];
    let mut stack: Vec<u32> = Vec::new();
    for seed in 0..n_t {
        if seen[seed] {
            continue;
        }
        seen[seed] = true;
        stack.push(seed as u32);
        while let Some(ti) = stack.pop() {
            let ti = ti as usize;
            // Walk this triangle's build-time half-edges (gid indexes them). A flip only
            // permutes the corners; the undirected edge set never changes, so these stay
            // the triangle's own edges however the pass has rewound it so far.
            for slot in 0..3 {
                let g = gid[3 * ti + slot] as usize;
                let s = starts[g] as usize;
                let en = if g + 1 < starts.len() { starts[g + 1] as usize } else { nh };
                if en - s != 2 {
                    continue; // self-pair or non-manifold seam edge: no 2-colouring exists there
                }
                let key = he[s].0;
                let (a, b) = ((key >> 32) as u32, key as u32);
                if a == b {
                    continue;
                }
                // The triangle's current traversal of that edge, from its present winding.
                let t = tris[ti];
                let mut dir = None;
                for e in 0..3 {
                    let (p, q) = (t[e], t[(e + 1) % 3]);
                    if p == a && q == b {
                        dir = Some((p, q));
                        break;
                    }
                    if p == b && q == a {
                        dir = Some((p, q));
                        break;
                    }
                }
                let Some((u, v)) = dir else { continue };
                for &(_, mhalf) in &he[s..en] {
                    let nt = (mhalf / 3) as usize;
                    if nt == ti || seen[nt] {
                        continue;
                    }
                    let t2 = tris[nt];
                    // The neighbour must traverse the shared edge opposite to `t`; find
                    // which direction it currently does.
                    let mut same_dir = None;
                    for k in 0..3 {
                        if t2[k] == u && t2[(k + 1) % 3] == v {
                            same_dir = Some(true);
                            break;
                        }
                        if t2[k] == v && t2[(k + 1) % 3] == u {
                            same_dir = Some(false);
                            break;
                        }
                    }
                    let Some(same) = same_dir else { continue };
                    if same {
                        tris[nt] = [t2[0], t2[2], t2[1]];
                    }
                    seen[nt] = true;
                    stack.push(nt as u32);
                }
            }
        }
    }
}

/// Signed tetrahedron-fan volume of a triangle mesh (sixth of the triple-product sum).
fn signed_volume(verts: &[V3], tris: &[[u32; 3]]) -> f64 {
    let mut v = 0.0;
    for t in tris {
        let (p, q, r) = (verts[t[0] as usize], verts[t[1] as usize], verts[t[2] as usize]);
        let ux = q[0] - p[0]; let uy = q[1] - p[1]; let uz = q[2] - p[2];
        let wx = r[0] - p[0]; let wy = r[1] - p[1]; let wz = r[2] - p[2];
        let c0 = uy * wz - uz * wy;
        let c1 = uz * wx - ux * wz;
        let c2 = ux * wy - uy * wx;
        v += (p[0] * c0 + p[1] * c1 + p[2] * c2) / 6.0;
    }
    v
}

/// Drop unreferenced vertices and renumber the triangles against the kept set.
fn compact(verts: &mut Vec<V3>, tris: &mut Vec<[u32; 3]>) {
    let mut used = vec![false; verts.len()];
    for t in &*tris {
        for &c in t {
            used[c as usize] = true;
        }
    }
    let mut new_ids = vec![u32::MAX; verts.len()];
    let mut keep: Vec<V3> = Vec::with_capacity(verts.len());
    for (i, v) in verts.iter().enumerate() {
        if used[i] {
            new_ids[i] = keep.len() as u32;
            keep.push(*v);
        }
    }
    for t in &mut *tris {
        for c in t.iter_mut() {
            *c = new_ids[*c as usize];
        }
    }
    *verts = keep;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orient_matches_hashmap_reference_bitwise() {
        // The CSR pass must reproduce the hash-map reference exactly: a component's
        // 2-colouring is unique and the seeds come in the same index order, so the final
        // triangles have to be bit-identical, on a mesh scrambled by deterministic flips.
        for (name, f, lo, hi, h) in [
            ("aligned box", box_c([0.0; 3], [5.0, 4.0, 3.0]), [-17.0; 3], [17.0; 3], 0.5),
            ("thin slab", box_c([0.0; 3], [15.0, 15.0, 0.15]), [-17.0; 3], [17.0; 3], 0.5),
        ] {
            let m = surface_adaptive(&f, lo, hi, h, &AdaptiveOpts::default());
            let scrambled: Vec<[u32; 3]> = m
                .tris
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let mut x = (i as u64) | 1;
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    if x & 1 == 0 { *t } else { [t[0], t[2], t[1]] }
                })
                .collect();
            let vol_ref = signed_volume(&m.verts, &m.tris).abs();
            let mut a = scrambled.clone();
            orient(&mut a);
            let mut b = scrambled;
            orient_hm(&mut b);
            assert_eq!(a, b, "{name}: CSR orient diverged from the hash-map reference");
            let vol1 = signed_volume(&m.verts, &a).abs();
            let d = TriMesh { verts: m.verts.clone(), tris: a }.defects();
            assert_eq!(d.open_edges, 0, "{name}: {:?}", d);
            assert_eq!(d.orientation_defects, 0, "{name}: {:?}", d);
            assert!(
                (vol1 - vol_ref).abs() <= 1e-9 * vol_ref.max(1.0),
                "{name}: orient did not recover the solid volume: {vol1} vs {vol_ref}"
            );
        }
    }

    /// The previous orient implementation (hash-map adjacency) for comparison.
    fn orient_hm(tris: &mut [ [u32; 3] ]) {
        let mut adj: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for (ti, t) in tris.iter().enumerate() {
            for e in 0..3 {
                let (u, v) = (t[e], t[(e + 1) % 3]);
                if u == v {
                    continue;
                }
                let key = (u.min(v), u.max(v));
                adj.entry(key).or_default().push(ti as u32);
            }
        }
        let mut seen = vec![false; tris.len()];
        let mut stack: Vec<u32> = Vec::new();
        for seed in 0..tris.len() {
            if seen[seed] {
                continue;
            }
            seen[seed] = true;
            stack.push(seed as u32);
            while let Some(ti) = stack.pop() {
                let t = tris[ti as usize];
                for e in 0..3 {
                    let (u, v) = (t[e], t[(e + 1) % 3]);
                    if u == v {
                        continue;
                    }
                    let key = (u.min(v), u.max(v));
                    let Some(mates) = adj.get(&key) else { continue };
                    if mates.len() != 2 {
                        continue;
                    }
                    for &nti in mates {
                        let nt = nti as usize;
                        if nt == ti as usize || seen[nt] {
                            continue;
                        }
                        let t2 = tris[nt];
                        let mut same_dir = None;
                        for k in 0..3 {
                            if t2[k] == u && t2[(k + 1) % 3] == v {
                                same_dir = Some(true);
                                break;
                            }
                            if t2[k] == v && t2[(k + 1) % 3] == u {
                                same_dir = Some(false);
                                break;
                            }
                        }
                        let Some(same) = same_dir else { continue };
                        if same {
                            tris[nt] = [t2[0], t2[2], t2[1]];
                        }
                        seen[nt] = true;
                        stack.push(nti);
                    }
                }
            }
        }
    }

    /// Max-of-abs-planes solid (box/slab) with its subgradient on the active plane.
    fn box_c(centre: V3, half: V3) -> impl Fn(V3) -> Sample {
        move |p: V3| {
            let mut f = -f64::INFINITY;
            let mut g = [0.0f64; 3];
            for i in 0..3 {
                let d = (p[i] - centre[i]).abs() - half[i];
                if d > f {
                    f = d;
                    g = [0.0, 0.0, 0.0];
                    g[i] = (p[i] - centre[i]).signum();
                }
            }
            Sample { v: f, g }
        }
    }

    fn sample_sphere(orig: V3, r: f64) -> impl Fn(V3) -> Sample {
        move |p: V3| {
            let d = [p[0] - orig[0], p[1] - orig[1], p[2] - orig[2]];
            let rho = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if rho > 1e-9 {
                Sample { v: rho - r, g: [d[0] / rho, d[1] / rho, d[2] / rho] }
            } else {
                Sample { v: -r, g: [0.0, 0.0, 1.0] }
            }
        }
    }

    /// Cylindrical tube shell of half-wall `hw` about `rmid`, capped at ±hz.
    fn tube_shell(hw: f64, rmid: f64, hz: f64) -> impl Fn(V3) -> Sample {
        move |p: V3| {
            let rho = (p[0] * p[0] + p[1] * p[1]).sqrt();
            let fr = (rho - rmid).abs() - hw;
            let fz = p[2].abs() - hz;
            if fr >= fz {
                if rho < 1e-9 {
                    // Radial gradient is undefined on the axis; the value is not.
                    return Sample { v: fr, g: [0.0, 0.0, 0.0] };
                }
                let s = (rho - rmid).signum();
                Sample { v: fr, g: [s * p[0] / rho, s * p[1] / rho, 0.0] }
            } else {
                Sample { v: fz, g: [0.0, 0.0, p[2].signum()] }
            }
        }
    }

    fn check(
        name: &str,
        f: impl Fn(V3) -> Sample + Sync,
        lo: V3,
        hi: V3,
        h: f64,
        v_exact: f64,
        tol: f64,
    ) {
        let m = surface_adaptive(f, lo, hi, h, &AdaptiveOpts::default());
        let v = m.volume();
        let d = m.defects();
        println!(
            "{}: verts {} tris {} volume {:.4} (exact {:.4}, {:+.3}%) defects {}",
            name,
            m.verts.len(),
            m.tris.len(),
            v,
            v_exact,
            100.0 * (v - v_exact) / v_exact,
            d.total(),
        );
        assert!(d.none(), "{}: defects {:?}", name, d);
        assert!((v - v_exact).abs() < tol * v_exact, "{}: volume {} vs {}", name, v, v_exact);
    }

    #[test]
    fn lattice_aligned_box() {
        check("box", box_c([0.0; 3], [5.0, 4.0, 3.0]), [-7.0; 3], [7.0; 3], 0.5, 480.0, 0.005);
    }

    #[test]
    fn off_lattice_box() {
        check(
            "offset box",
            box_c([0.0; 3], [5.05, 3.95, 2.95]),
            [-7.0; 3],
            [7.0; 3],
            0.5,
            470.661,
            0.005,
        );
    }

    #[test]
    fn sphere_volume() {
        check("sphere", sample_sphere([0.0; 3], 5.0), [-7.0; 3], [7.0; 3], 0.5, 523.5988, 0.02);
    }

    #[test]
    fn thin_slab_finer_than_leaf() {
        // Sheet thickness 0.3 mm against leaf 0.5 mm: crossings both inside one cell.
        check("thin slab", box_c([0.0; 3], [15.0, 15.0, 0.15]), [-17.0; 3], [17.0; 3], 0.5, 270.0, 0.05);
    }

    #[test]
    fn tube_shell_two_surfaces() {
        check("tube", tube_shell(2.0, 10.0, 6.0), [-14.0; 3], [14.0; 3], 1.0, 3015.93, 0.02);
    }

    #[test]
    fn determinism_bit_identical() {
        let f = sample_sphere([0.0; 3], 5.0);
        let a = surface_adaptive(&f, [-7.0; 3], [7.0; 3], 0.5, &AdaptiveOpts::default());
        let b = surface_adaptive(&f, [-7.0; 3], [7.0; 3], 0.5, &AdaptiveOpts::default());
        assert_eq!(a, b);
    }
}