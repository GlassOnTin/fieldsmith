//! Quality tetrahedral meshing of an implicit solid by isosurface stuffing.
//!
//! Follows Labelle & Shewchuk, "Isosurface Stuffing: Fast Tetrahedral Meshes with Good Dihedral
//! Angles" (SIGGRAPH 2007), uniform variant:
//!
//! 1. Sample the signed distance field on a body-centred cubic (BCC) lattice.
//! 2. Where a lattice edge crosses the surface close to one of its ends, *warp* that lattice
//!    vertex onto the crossing (its value becomes exactly zero).
//! 3. Cut the remaining crossing edges at the zero level and fill each lattice tetrahedron's
//!    inside part with one of a few stencils. Quadrilateral faces are split along the diagonal
//!    that touches the vertex with the smallest global index, so neighbouring tetrahedra agree.
//!
//! The lattice is aligned to the solid's bounding box with per-axis spacing chosen so that the box
//! is a whole number of cells. Planar faces that coincide with lattice planes (the outer faces of
//! most mechanical parts, and inner faces at grid multiples) are then reproduced exactly.

pub mod adaptive;
pub mod export;
pub mod quality;
pub use quality::{quality, Quality};

use std::collections::HashMap;

pub type V3 = [f64; 3];

/// A linear tetrahedral mesh. Tetrahedra are positively oriented.
#[derive(Clone, Debug, Default)]
pub struct TetMesh {
    pub verts: Vec<V3>,
    pub tets: Vec<[u32; 4]>,
}

/// Lattice parameters actually used (per-axis spacing).
#[derive(Clone, Copy, Debug)]
pub struct Lattice {
    pub origin: V3,
    pub h: V3,
    pub n: [usize; 3],
}

// Warp thresholds from the paper (fractions of the edge length).
const ALPHA_LONG: f64 = 0.24999;
const ALPHA_SHORT: f64 = 0.41189;

impl Lattice {
    /// A lattice covering `[lo, hi]` with spacing close to `h`, padded by one cell, aligned so
    /// that `lo` and `hi` fall on lattice planes.
    pub fn fit(lo: V3, hi: V3, h: f64) -> Lattice {
        let mut origin = [0.0; 3];
        let mut hh = [0.0; 3];
        let mut n = [0usize; 3];
        for a in 0..3 {
            let ext = (hi[a] - lo[a]).max(1e-9);
            let cells = (ext / h).round().max(1.0);
            hh[a] = ext / cells;
            origin[a] = lo[a] - hh[a];
            n[a] = cells as usize + 2;
        }
        Lattice { origin, h: hh, n }
    }
    #[inline]
    fn corner(&self, i: usize, j: usize, k: usize) -> usize {
        i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)
    }
    #[inline]
    fn ncorners(&self) -> usize {
        (self.n[0] + 1) * (self.n[1] + 1) * (self.n[2] + 1)
    }
    #[inline]
    fn centre(&self, i: usize, j: usize, k: usize) -> usize {
        self.ncorners() + i + self.n[0] * (j + self.n[1] * k)
    }
    pub fn nverts(&self) -> usize {
        self.ncorners() + self.n[0] * self.n[1] * self.n[2]
    }
    fn position(&self, id: usize) -> V3 {
        let nc = self.ncorners();
        if id < nc {
            let i = id % (self.n[0] + 1);
            let j = (id / (self.n[0] + 1)) % (self.n[1] + 1);
            let k = id / ((self.n[0] + 1) * (self.n[1] + 1));
            [
                self.origin[0] + i as f64 * self.h[0],
                self.origin[1] + j as f64 * self.h[1],
                self.origin[2] + k as f64 * self.h[2],
            ]
        } else {
            let c = id - nc;
            let i = c % self.n[0];
            let j = (c / self.n[0]) % self.n[1];
            let k = c / (self.n[0] * self.n[1]);
            [
                self.origin[0] + (i as f64 + 0.5) * self.h[0],
                self.origin[1] + (j as f64 + 0.5) * self.h[1],
                self.origin[2] + (k as f64 + 0.5) * self.h[2],
            ]
        }
    }

    /// Every lattice edge once, flagged long (axis-aligned) or short (centre to corner).
    fn edges(&self, mut f: impl FnMut(usize, usize, bool)) {
        let [nx, ny, nz] = self.n;
        // Long corner-corner edges.
        for k in 0..=nz {
            for j in 0..=ny {
                for i in 0..=nx {
                    let a = self.corner(i, j, k);
                    if i < nx {
                        f(a, self.corner(i + 1, j, k), true);
                    }
                    if j < ny {
                        f(a, self.corner(i, j + 1, k), true);
                    }
                    if k < nz {
                        f(a, self.corner(i, j, k + 1), true);
                    }
                }
            }
        }
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = self.centre(i, j, k);
                    // Long centre-centre edges.
                    if i + 1 < nx {
                        f(c, self.centre(i + 1, j, k), true);
                    }
                    if j + 1 < ny {
                        f(c, self.centre(i, j + 1, k), true);
                    }
                    if k + 1 < nz {
                        f(c, self.centre(i, j, k + 1), true);
                    }
                    // Short centre-corner edges.
                    for dk in 0..2 {
                        for dj in 0..2 {
                            for di in 0..2 {
                                f(c, self.corner(i + di, j + dj, k + dk), false);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Every BCC tetrahedron: two adjacent centres and an edge of their shared square face.
    fn tets(&self, mut f: impl FnMut([usize; 4])) {
        let [nx, ny, nz] = self.n;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = self.centre(i, j, k);
                    // Neighbour along +x: shared face at corner-plane i+1.
                    if i + 1 < nx {
                        let d = self.centre(i + 1, j, k);
                        let q = [
                            self.corner(i + 1, j, k),
                            self.corner(i + 1, j + 1, k),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i + 1, j, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                    if j + 1 < ny {
                        let d = self.centre(i, j + 1, k);
                        let q = [
                            self.corner(i, j + 1, k),
                            self.corner(i + 1, j + 1, k),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i, j + 1, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                    if k + 1 < nz {
                        let d = self.centre(i, j, k + 1);
                        let q = [
                            self.corner(i, j, k + 1),
                            self.corner(i + 1, j, k + 1),
                            self.corner(i + 1, j + 1, k + 1),
                            self.corner(i, j + 1, k + 1),
                        ];
                        for e in 0..4 {
                            f([c, d, q[e], q[(e + 1) % 4]]);
                        }
                    }
                }
            }
        }
    }
}

#[inline]
fn lerp(a: V3, b: V3, t: f64) -> V3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Zero crossing of `f` on the segment `a`–`b` (with `fa < 0 < fb` or the reverse), as a
/// parameter in (0, 1). Illinois-modified regula falsi; converges to ~1e-9 of the edge.
fn crossing(f: &impl Fn(V3) -> f64, a: V3, b: V3, fa: f64, fb: f64) -> f64 {
    let (mut t0, mut t1, mut f0, mut f1) = (0.0f64, 1.0f64, fa, fb);
    let mut side = 0;
    let mut t = f0 / (f0 - f1);
    for _ in 0..40 {
        t = (t0 * f1 - t1 * f0) / (f1 - f0);
        if !(t > t0 && t < t1) {
            t = 0.5 * (t0 + t1);
        }
        let ft = f(lerp(a, b, t));
        if ft == 0.0 || (t1 - t0) < 1e-10 {
            break;
        }
        if (ft < 0.0) == (f0 < 0.0) {
            t0 = t;
            f0 = ft;
            if side == -1 {
                f1 *= 0.5;
            }
            side = -1;
        } else {
            t1 = t;
            f1 = ft;
            if side == 1 {
                f0 *= 0.5;
            }
            side = 1;
        }
    }
    t
}

/// Mesh the solid `{p : f(p) < 0}` inside the box `[lo, hi]` with target spacing `h`.
///
/// Lattice positions are computed on demand ([`Lattice::position`] is O(1) arithmetic): at
/// h = 0.25 mm across a 150 mm bracket the full-position table alone would be 1.6 GiB, which
/// does not fit a browser's wasm linear memory alongside `val` and the remap table.
pub fn stuff(f: impl Fn(V3) -> f64, lo: V3, hi: V3, h: f64) -> (TetMesh, Lattice) {
    let lat = Lattice::fit(lo, hi, h);
    let nv = lat.nverts();
    let pos = |i: usize| -> V3 { lat.position(i) };
    let mut val: Vec<f64> = (0..nv).map(|i| f(pos(i))).collect();
    // Points on lattice planes that coincide with faces evaluate to ±rounding noise; snap them.
    let tiny = 1e-9 * lat.h[0].max(lat.h[1]).max(lat.h[2]);
    for v in val.iter_mut() {
        if v.abs() < tiny {
            *v = 0.0;
        }
    }

    // ---- warp vertices that lie too close to a crossing --------------------------------------
    // For each violated vertex keep the nearest violating cut point (as an edge fraction); the
    // cut point becomes the vertex's emitted position (a handful of vertices, so a map, not a
    // whole-lattice table).
    let mut warp: HashMap<usize, (f64, V3)> = HashMap::new();
    lat.edges(|a, b, long| {
        let (fa, fb) = (val[a], val[b]);
        if !((fa < 0.0 && fb > 0.0) || (fa > 0.0 && fb < 0.0)) {
            return;
        }
        let t = crossing(&f, pos(a), pos(b), fa, fb);
        let alpha = if long { ALPHA_LONG } else { ALPHA_SHORT };
        let c = lerp(pos(a), pos(b), t);
        if t < alpha {
            let e = warp.entry(a).or_insert((f64::INFINITY, c));
            if t < e.0 {
                *e = (t, c);
            }
        } else if 1.0 - t < alpha {
            let e = warp.entry(b).or_insert((f64::INFINITY, c));
            if 1.0 - t < e.0 {
                *e = (1.0 - t, c);
            }
        }
    });
    for &v in warp.keys() {
        val[v] = 0.0;
    }

    // ---- stencils ----------------------------------------------------------------------------
    // Emitted position of a lattice vertex: its warped cut point when it was warped, else the
    // lattice position.
    let warp_pos = |v: usize| -> V3 {
        match warp.get(&v) {
            Some(&(_, c)) => c,
            None => pos(v),
        }
    };
    let mut out = TetMesh::default();
    let mut remap: Vec<u32> = vec![u32::MAX; nv];
    let mut cuts: HashMap<(usize, usize), u32> = HashMap::new();
    // Global order used for diagonal choice: lattice vertices by lattice id, cut points after
    // all lattice vertices in creation order (stable for a given lattice and field).
    let mut gid: Vec<usize> = Vec::new();
    let mut lattice_vertex = |v: usize, out: &mut TetMesh, gid: &mut Vec<usize>| -> u32 {
        if remap[v] == u32::MAX {
            remap[v] = out.verts.len() as u32;
            out.verts.push(warp_pos(v));
            gid.push(v);
        }
        remap[v]
    };

    lat.tets(|t| {
        let s = t.map(|v| {
            if val[v] < 0.0 {
                -1i8
            } else if val[v] > 0.0 {
                1
            } else {
                0
            }
        });
        let npos = s.iter().filter(|&&x| x > 0).count();
        let nneg = s.iter().filter(|&&x| x < 0).count();
        if nneg == 0 {
            if npos == 0 {
                // All four vertices on the surface: keep if the tet lies inside.
                let c = [0, 1, 2]
                    .map(|a| (warp_pos(t[0])[a] + warp_pos(t[1])[a] + warp_pos(t[2])[a] + warp_pos(t[3])[a]) * 0.25);
                if f(c) < 0.0 {
                    let v = t.map(|x| lattice_vertex(x, &mut out, &mut gid));
                    emit(&mut out, v);
                }
            }
            return;
        }
        let vids: [u32; 4] = t.map(|x| {
            if val[x] <= 0.0 {
                lattice_vertex(x, &mut out, &mut gid)
            } else {
                u32::MAX
            }
        });
        let mut cut = |a: usize, b: usize, out: &mut TetMesh, gid: &mut Vec<usize>| -> u32 {
            let key = if t[a] < t[b] { (t[a], t[b]) } else { (t[b], t[a]) };
            *cuts.entry(key).or_insert_with(|| {
                let (p, q) = (warp_pos(key.0), warp_pos(key.1));
                let u = crossing(&f, p, q, val[key.0], val[key.1]);
                out.verts.push(lerp(p, q, u));
                gid.push(nv + out.verts.len());
                (out.verts.len() - 1) as u32
            })
        };
        if npos == 0 {
            emit(&mut out, vids);
            return;
        }
        let negs: Vec<usize> = (0..4).filter(|&i| s[i] < 0).collect();
        let zeros: Vec<usize> = (0..4).filter(|&i| s[i] == 0).collect();
        let poss: Vec<usize> = (0..4).filter(|&i| s[i] > 0).collect();
        match (nneg, zeros.len(), npos) {
            (1, _, _) => {
                // One inside vertex: a single tet, outside vertices replaced by cut points.
                let a = negs[0];
                let mut v = vids;
                for &p in &poss {
                    v[p] = cut(a, p, &mut out, &mut gid);
                }
                emit(&mut out, v);
            }
            (2, 0, 2) => {
                // Prism: (a, ac, ad) – (b, bc, bd).
                let (a, b, c, d) = (negs[0], negs[1], poss[0], poss[1]);
                let p = [vids[a], cut(a, c, &mut out, &mut gid), cut(a, d, &mut out, &mut gid)];
                let q = [vids[b], cut(b, c, &mut out, &mut gid), cut(b, d, &mut out, &mut gid)];
                prism(&mut out, &gid, p, q);
            }
            (2, 1, 1) => {
                // Pyramid with apex z over quad (a, b, bd, ad).
                let (a, b, z, d) = (negs[0], negs[1], zeros[0], poss[0]);
                let ad = cut(a, d, &mut out, &mut gid);
                let bd = cut(b, d, &mut out, &mut gid);
                pyramid(&mut out, &gid, [vids[a], vids[b], bd, ad], vids[z]);
            }
            (3, 0, 1) => {
                // Prism: (a, b, c) – (ad, bd, cd).
                let (a, b, c, d) = (negs[0], negs[1], negs[2], poss[0]);
                let p = [vids[a], vids[b], vids[c]];
                let q = [
                    cut(a, d, &mut out, &mut gid),
                    cut(b, d, &mut out, &mut gid),
                    cut(c, d, &mut out, &mut gid),
                ];
                prism(&mut out, &gid, p, q);
            }
            _ => unreachable!("sign pattern {s:?}"),
        }
    });
    (out, lat)
}

fn signed_volume(m: &TetMesh, t: [u32; 4]) -> f64 {
    let [a, b, c, d] = t.map(|i| m.verts[i as usize]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
    (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])) / 6.0
}

/// Append a tetrahedron, oriented positively. Degenerate (zero-volume) pieces are dropped: they
/// arise only where a cut point coincides with a vertex and carry no volume.
fn emit(m: &mut TetMesh, mut t: [u32; 4]) {
    let v = signed_volume(m, t);
    if v.abs() < 1e-14 {
        return;
    }
    if v < 0.0 {
        t.swap(2, 3);
    }
    m.tets.push(t);
}

/// Split a pyramid (quad base `q` in cyclic order, apex `z`) along the base diagonal touching
/// the base vertex with the smallest global index.
fn pyramid(m: &mut TetMesh, gid: &[usize], q: [u32; 4], z: u32) {
    let g = |v: u32| gid[v as usize];
    let k = (0..4).min_by_key(|&i| g(q[i])).unwrap();
    let (a, b, c, d) = (q[k], q[(k + 1) % 4], q[(k + 2) % 4], q[(k + 3) % 4]);
    emit(m, [a, b, c, z]);
    emit(m, [a, c, d, z]);
}

/// Split a triangular prism (bottom `p`, top `q`, `p[i]`–`q[i]` vertical edges) into three
/// tetrahedra whose quad-face diagonals all touch each face's smallest-index vertex
/// (Dompierre et al. 1999), so that neighbouring cells agree.
fn prism(m: &mut TetMesh, gid: &[usize], p: [u32; 3], q: [u32; 3]) {
    let g = |v: u32| gid[v as usize];
    let all = [p[0], p[1], p[2], q[0], q[1], q[2]];
    let k = (0..6).min_by_key(|&i| g(all[i])).unwrap();
    // Rotate/flip so that the minimum is at bottom vertex 0.
    let (r, flip) = (k % 3, k >= 3);
    let (bot, top) = if flip { (q, p) } else { (p, q) };
    let v0 = bot[r];
    let v1 = bot[(r + 1) % 3];
    let v2 = bot[(r + 2) % 3];
    let v3 = top[r];
    let v4 = top[(r + 1) % 3];
    let v5 = top[(r + 2) % 3];
    // Quad opposite v0: (v1, v2, v5, v4). Its diagonal is v1–v5 or v2–v4.
    if g(v1).min(g(v5)) < g(v2).min(g(v4)) {
        emit(m, [v0, v1, v2, v5]);
        emit(m, [v0, v1, v5, v4]);
        emit(m, [v0, v4, v5, v3]);
    } else {
        emit(m, [v0, v1, v2, v4]);
        emit(m, [v0, v4, v2, v5]);
        emit(m, [v0, v4, v5, v3]);
    }
}

impl TetMesh {
    pub fn volume(&self) -> f64 {
        self.tets.iter().map(|&t| signed_volume(self, t)).sum()
    }

    /// Boundary triangles (outward oriented) with the index of the owning tetrahedron.
    ///
    /// The face-usage table is split from the boundary-faces table deliberately: ~2·T distinct
    /// faces across T tets means a combined `HashMap<[u32; 3], (count, face, owner)>` entry
    /// costs 32 B, and at T ≈ 16 M that needs 2^26 buckets ≈ 2.15 GB — past the 32-bit linear
    /// address space (wasm32 calls this "hash table capacity overflow", 64-bit platforms
    /// never see it). A bare count per face fits in ~14 B per bucket, and the boundary faces
    /// (count 1, ≤ T entries) go into their own small map on a second pass.
    pub fn boundary(&self) -> Vec<([u32; 3], u32)> {
        let mut uses: HashMap<[u32; 3], u16> = HashMap::new();
        for t in &self.tets {
            // Outward faces for a positively oriented tet (a, b, c, d).
            let fs = [[t[1], t[2], t[3]], [t[0], t[3], t[2]], [t[0], t[1], t[3]], [t[0], t[2], t[1]]];
            for f in fs {
                let mut key = f;
                key.sort_unstable();
                uses.entry(key).and_modify(|c| *c = c.saturating_add(1)).or_insert(1);
            }
        }
        // Boundary faces: collect their outward orientation (the single occurrence's winding)
        // and owning tet.
        let mut faces: HashMap<[u32; 3], ([u32; 3], u32)> = HashMap::with_capacity(uses.len() / 16 + 1024);
        for (ti, t) in self.tets.iter().enumerate() {
            let fs = [[t[1], t[2], t[3]], [t[0], t[3], t[2]], [t[0], t[1], t[3]], [t[0], t[2], t[1]]];
            for f in fs {
                let mut key = f;
                key.sort_unstable();
                if uses.get(&key).is_some_and(|&c| c == 1) {
                    faces.insert(key, (f, ti as u32));
                }
            }
        }
        let mut out: Vec<([u32; 3], u32)> = faces.into_values().collect();
        out.sort_unstable_by_key(|e| e.1);
        out
    }

    /// Remove vertices not referenced by any tetrahedron.
    pub fn compact(&mut self) {
        let mut used = vec![u32::MAX; self.verts.len()];
        let mut verts = Vec::new();
        for t in self.tets.iter_mut() {
            for v in t.iter_mut() {
                if used[*v as usize] == u32::MAX {
                    used[*v as usize] = verts.len() as u32;
                    verts.push(self.verts[*v as usize]);
                }
                *v = used[*v as usize];
            }
        }
        self.verts = verts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere(p: V3) -> f64 {
        (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 10.0
    }

    #[test]
    fn sphere_volume_and_quality() {
        let (mut m, _) = stuff(sphere, [-10.0; 3], [10.0; 3], 1.0);
        m.compact();
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        let v = m.volume();
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
        let q = quality(&m);
        assert!(q.min_dihedral_deg > 8.0, "min dihedral {}", q.min_dihedral_deg);
        assert!(q.max_dihedral_deg < 168.0, "max dihedral {}", q.max_dihedral_deg);
    }

    #[test]
    fn box_is_exact_and_watertight() {
        let f = |p: V3| {
            let q = [p[0].abs() - 50.0, (p[1] - 5.0).abs() - 5.0, (p[2] - 5.0).abs() - 5.0];
            let o = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
            (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt() + q[0].max(q[1]).max(q[2]).min(0.0)
        };
        let (mut m, _) = stuff(f, [-50.0, 0.0, 0.0], [50.0, 10.0, 10.0], 2.5);
        m.compact();
        assert!((m.volume() - 10000.0).abs() < 1e-6, "volume {}", m.volume());
        // Every boundary edge is shared by exactly two boundary triangles.
        let b = m.boundary();
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for (f, _) in &b {
            for i in 0..3 {
                let (a, c) = (f[i], f[(i + 1) % 3]);
                *edges.entry((a, c)).or_default() += 1;
                *edges.entry((c, a)).or_default() -= 1;
            }
        }
        assert!(edges.values().all(|&c| c == 0), "boundary is not closed and consistently oriented");
    }
}
