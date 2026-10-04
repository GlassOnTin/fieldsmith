//! Export geometry: the closed triangle surface of an implicit solid, its health report, and
//! the STL / 3MF writers.
//!
//! The surface is the boundary triangles of an isosurface-stuffed tetrahedral mesh
//! ([`crate::stuff`]), so watertightness and consistent orientation come from the meshing
//! rather than a repair pass; [`TriMesh::defects`] measures them anyway.

use crate::{stuff, V3};
use miniz_oxide::deflate::core::{
    compress as deflate_compress, create_comp_flags_from_zip_params, CompressorOxide, TDEFLFlush,
    TDEFLStatus,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt::Write as _;

/// A triangle surface with shared vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriMesh {
    pub verts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
}

/// What is wrong with a surface, per diagnostic class.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Defects {
    /// Undirected edges used by only one triangle (the surface is not closed).
    pub open_edges: usize,
    /// Undirected edges used by more than two triangles.
    pub non_manifold_edges: usize,
    /// Edges whose two adjacent triangles wind the same way (usage 2, not one traversal each way).
    pub orientation_defects: usize,
    /// Zero-area triangles (excluded from the edge and intersection scans).
    pub degenerate_triangles: usize,
    /// Pairs of triangles that cross without sharing a vertex.
    pub self_intersections: usize,
}

impl Defects {
    pub fn total(&self) -> usize {
        self.open_edges
            + self.non_manifold_edges
            + self.orientation_defects
            + self.degenerate_triangles
            + self.self_intersections
    }
    pub fn none(&self) -> bool {
        *self == Defects::default()
    }
}

/// The closed surface of a solid field: uniform isosurface stuffing, then the boundary faces.
/// `h` is the lattice spacing (mm); axis-aligned planar faces land on lattice planes, so a
/// box aligned like this comes out dimensionally exact.
pub fn surface(f: impl Fn(V3) -> f64, lo: V3, hi: V3, h: f64) -> TriMesh {
    let (mut m, _) = stuff(f, lo, hi, h);
    m.compact();
    let tris = m.boundary().into_iter().map(|(t, _)| t).collect();
    TriMesh { verts: m.verts, tris }
}

impl TriMesh {
    /// Signed volume: outward-facing triangles sum to the solid volume (mm³).
    pub fn volume(&self) -> f64 {
        let mut v = 0.0;
        for t in &self.tris {
            let a = self.verts[t[0] as usize];
            let b = self.verts[t[1] as usize];
            let c = self.verts[t[2] as usize];
            v += (a[0] * (b[1] * c[2] - c[1] * b[2]) - a[1] * (b[0] * c[2] - c[0] * b[2])
                + a[2] * (b[0] * c[1] - c[0] * b[1]))
                / 6.0;
        }
        v
    }

    /// Outward unit normal of one triangle; zeros when degenerate.
    pub fn normal(&self, t: [u32; 3]) -> V3 {
        let (a, b, c) = (
            self.verts[t[0] as usize],
            self.verts[t[1] as usize],
            self.verts[t[2] as usize],
        );
        unit(cross(sub(b, a), sub(c, a)))
    }

    /// Sections of the plane `z` through the mesh: segment endpoints packed
    /// `[x0, y0, x1, y1, …]` (mm). Contours of the exact exported mesh, not a re-slice of
    /// the field. A triangle lying in the plane contributes nothing; a vertex exactly on
    /// the plane counts as below it, so a triangle touching the plane along a vertex yields
    /// a zero-length segment, which is dropped.
    pub fn slice_at(&self, z: f64) -> Vec<f64> {
        let mut out = Vec::new();
        for t in &self.tris {
            let vs = [
                self.verts[t[0] as usize],
                self.verts[t[1] as usize],
                self.verts[t[2] as usize],
            ];
            let above = vs.map(|v| v[2] > z);
            if above[0] == above[1] && above[1] == above[2] {
                continue; // strictly one side, or lying in the plane
            }
            let mut section = [(0.0f64, 0.0f64); 2];
            let mut n = 0usize;
            for i in 0..3 {
                let (p, q) = (vs[i], vs[(i + 1) % 3]);
                if above[i] == above[(i + 1) % 3] {
                    continue; // both ends on one side
                }
                let f = (z - p[2]) / (q[2] - p[2]);
                section[n] = (p[0] + f * (q[0] - p[0]), p[1] + f * (q[1] - p[1]));
                n += 1;
            }
            debug_assert_eq!(n, 2);
            if n == 2 {
                let (p, q) = (section[0], section[1]);
                if (q.0 - p.0).hypot(q.1 - p.1) > 1e-9 {
                    out.extend_from_slice(&[p.0, p.1, q.0, q.1]);
                }
            }
        }
        out
    }

    /// Count every diagnostic class. Degenerate triangles are excluded from the edge and
    /// intersection scans (their edges are meaningless) and reported on their own.
    ///
    /// The edge census sorts packed undirected edge keys, split by traversal direction,
    /// and counts runs: 8 B per half-edge (a hash map of directed edges plus a seen-set
    /// held ~4 GB at the 0.1 mm bracket export).
    pub fn defects(&self) -> Defects {
        let mut d = Defects::default();
        let degen: Vec<bool> = self.tris.par_iter().map(|&t| self.area(t) <= 1e-12).collect();
        d.degenerate_triangles = degen.iter().filter(|&&g| g).count();
        // Undirected key (min << 32 | max), in `fwd` when traversed min -> max, else `bwd`.
        let mut fwd: Vec<u64> = Vec::new();
        let mut bwd: Vec<u64> = Vec::new();
        for (t, &g) in self.tris.iter().zip(&degen) {
            if g {
                continue;
            }
            for i in 0..3 {
                let (u, v) = (t[i], t[(i + 1) % 3]);
                if u == v {
                    continue; // self-edge: not an edge of the surface
                }
                let key = ((u.min(v) as u64) << 32) | u.max(v) as u64;
                if u < v { fwd.push(key) } else { bwd.push(key) }
            }
        }
        drop(degen);
        fwd.par_sort_unstable();
        bwd.par_sort_unstable();
        let (mut i, mut j) = (0usize, 0usize);
        while i < fwd.len() || j < bwd.len() {
            let k = match (fwd.get(i), bwd.get(j)) {
                (Some(&a), Some(&b)) => a.min(b),
                (Some(&a), None) => a,
                (None, Some(&b)) => b,
                (None, None) => unreachable!(),
            };
            let (mut f, mut b) = (0usize, 0usize);
            while i < fwd.len() && fwd[i] == k {
                f += 1;
                i += 1;
            }
            while j < bwd.len() && bwd[j] == k {
                b += 1;
                j += 1;
            }
            let usage = f + b;
            if usage == 1 {
                d.open_edges += 1;
            } else if usage >= 3 {
                d.non_manifold_edges += 1;
            } else if (f, b) != (1, 1) {
                d.orientation_defects += 1;
            }
        }
        drop((fwd, bwd));
        d.self_intersections = self.self_intersections();
        d
    }

    /// Triangle area (mm²).
    fn area(&self, t: [u32; 3]) -> f64 {
        let (a, b, c) = (
            self.verts[t[0] as usize],
            self.verts[t[1] as usize],
            self.verts[t[2] as usize],
        );
        0.5 * len(cross(sub(b, a), sub(c, a)))
    }

    /// Pairs of triangles that cross without sharing a vertex, found through a uniform grid
    /// over the triangle bounding boxes. Each intersecting pair is counted once.
    /// Under-reporting (named gap): coplanar-touching pairs that overlap without crossing a
    /// transverse segment, and edges grazing the target plane nearly parallel, are missed.
    ///
    /// Candidates are the pairs whose grid-cell boxes (`floor(lo/cell) ..= ceil(hi/cell)`)
    /// share a cell. Each such pair is tested in exactly one cell, the reference cell at
    /// the componentwise max of the two boxes' low corners (it lies in both boxes), so no
    /// per-triangle candidate set is needed. The grid is swept in x-slabs: a slab's
    /// (cell, triangle) entries are sorted, cells are scanned in parallel, and the entries
    /// are dropped before the next slab — memory is bounded by the slab, not the mesh.
    pub fn self_intersections(&self) -> usize {
        let n = self.tris.len();
        if n == 0 {
            return 0;
        }
        let cell = self.hash_cell();
        let cbox = |i: usize| -> [[i64; 3]; 2] {
            let a = self.tri_aabb(&self.tris[i]);
            [
                [0, 1, 2].map(|k| (a[0][k] / cell).floor() as i64),
                [0, 1, 2].map(|k| (a[1][k] / cell).ceil() as i64),
            ]
        };
        // Triangles in order of their box's low x cell (counting sort when the range is
        // sane, comparison sort otherwise).
        let (xmin, xmax) = (0..n)
            .into_par_iter()
            .map(|i| {
                let b = cbox(i);
                (b[0][0], b[1][0])
            })
            .reduce(|| (i64::MAX, i64::MIN), |a, b| (a.0.min(b.0), a.1.max(b.1)));
        let range = (xmax - xmin + 1) as u64;
        let order: Vec<u32> = if range <= 4 * n as u64 + 1024 {
            let mut off = vec![0u32; range as usize + 1];
            for i in 0..n {
                off[(cbox(i)[0][0] - xmin) as usize + 1] += 1;
            }
            for k in 0..range as usize {
                off[k + 1] += off[k];
            }
            let mut order = vec![0u32; n];
            for i in 0..n {
                let k = (cbox(i)[0][0] - xmin) as usize;
                order[off[k] as usize] = i as u32;
                off[k] += 1;
            }
            order
        } else {
            let mut order: Vec<u32> = (0..n as u32).collect();
            order.par_sort_by_key(|&i| (cbox(i as usize)[0][0], i));
            order
        };

        // ~8-27 cells per triangle box at 32 B per entry: ~0.1 GB of entries per slab.
        const SLAB_TRIS: usize = 1 << 18;
        let mut hits = 0usize;
        let mut active: Vec<u32> = Vec::new();
        let mut next = 0usize;
        let mut x0 = xmin;
        while next < n || !active.is_empty() {
            // Slab [x0, x1]: the slabs partition the x cells. It takes ~SLAB_TRIS more
            // triangles in whole low-x columns (they all start inside it: earlier columns
            // went to earlier slabs); once every triangle has started, it runs to the end.
            let x1 = if next < n {
                let mut end = (next + SLAB_TRIS).min(n);
                while end < n && cbox(order[end] as usize)[0][0] == cbox(order[end - 1] as usize)[0][0] {
                    end += 1;
                }
                let x1 = cbox(order[end - 1] as usize)[0][0];
                active.extend_from_slice(&order[next..end]);
                next = end;
                x1
            } else {
                xmax
            };
            debug_assert!(x1 >= x0);
            // Entries of the slab: every (cell, triangle) with the cell's x in [x0, x1].
            let mut ent: Vec<([i64; 3], u32)> = Vec::new();
            for &i in &active {
                let b = cbox(i as usize);
                for x in b[0][0].max(x0)..=b[1][0].min(x1) {
                    for y in b[0][1]..=b[1][1] {
                        for z in b[0][2]..=b[1][2] {
                            ent.push(([x, y, z], i));
                        }
                    }
                }
            }
            ent.par_sort_unstable();
            let mut starts: Vec<usize> = Vec::new();
            for k in 0..ent.len() {
                if k == 0 || ent[k].0 != ent[k - 1].0 {
                    starts.push(k);
                }
            }
            starts.push(ent.len());
            hits += starts
                .par_windows(2)
                .map(|w| {
                    let g = &ent[w[0]..w[1]];
                    let c = g[0].0;
                    let mut h = 0usize;
                    for a in 0..g.len() {
                        let i = g[a].1 as usize;
                        let bi = cbox(i);
                        let t = &self.tris[i];
                        for e in &g[a + 1..] {
                            let j = e.1 as usize; // j > i: entries sort by triangle within a cell
                            let bj = cbox(j);
                            if [0, 1, 2].map(|k| bi[0][k].max(bj[0][k])) != c {
                                continue; // this pair's reference cell is elsewhere
                            }
                            let t2 = self.tris[j];
                            if t.iter().any(|&v| t2.contains(&v)) {
                                continue; // touching triangles share a vertex by construction
                            }
                            if !overlap(&self.tri_aabb(t), &self.tri_aabb(&t2)) {
                                continue;
                            }
                            if seg_vs_tri(self, t, &t2) || seg_vs_tri(self, &t2, t) {
                                h += 1;
                            }
                        }
                    }
                    h
                })
                .sum::<usize>();
            drop(ent);
            // Triangles reaching past this slab stay active for the next one.
            active.retain(|&i| cbox(i as usize)[1][0] > x1);
            x0 = x1 + 1;
        }
        hits
    }

    fn hash_cell(&self) -> f64 {
        if self.tris.is_empty() {
            return 1.0;
        }
        let mut ds: Vec<f64> = self
            .tris
            .iter()
            .take(2048)
            .map(|t| {
                let a = self.tri_aabb(t);
                (a[1][0] - a[0][0]).max(a[1][1] - a[0][1]).max(a[1][2] - a[0][2])
            })
            .collect();
        ds.sort_unstable_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        (ds[ds.len() / 2] * 2.0).max(1e-6)
    }

    fn tri_aabb(&self, t: &[u32; 3]) -> [[f64; 3]; 2] {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for &i in t {
            let p = self.verts[i as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        [lo, hi]
    }

    /// Binary STL (millimetre units): 80-byte header, little-endian, triangle normals and
    /// positions as f32. Shared vertices are written once per triangle, and equal f32 values
    /// always convert identically from the same f64, so watertightness survives quantisation.
    pub fn write_stl(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(84 + self.tris.len() * 50);
        let mut header = [0u8; 80];
        header[..17].copy_from_slice(b"fieldsmith binary");
        out.extend_from_slice(&header);
        out.extend_from_slice(&(self.tris.len() as u32).to_le_bytes());
        for t in &self.tris {
            let n = self.normal(*t);
            let vs = [
                self.verts[t[0] as usize],
                self.verts[t[1] as usize],
                self.verts[t[2] as usize],
            ];
            for v in [n, vs[0], vs[1], vs[2]] {
                for c in v {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]); // attribute byte count
        }
        out
    }

    /// 3MF package (millimetre units) with the part name and the material recorded as a base
    /// material. Vertices are translated into the positive octant (3MF requires it), so the
    /// model's own origin is the part's bounding-box minimum.
    ///
    /// At the export-budget spacings the model text would be gigabytes of XML (the bracket at
    /// 0.1 mm meshes to ~9 M vertices and ~18 M triangles), and a wasm32 page has ~4 GiB of
    /// memory, so the text is never held: chunks of it are formatted and DEFLATE-compressed
    /// into independent segments in a bounded rayon window, and appended to the archive in
    /// order. Chunk contents, segment boundaries and append order are fixed by element index,
    /// so the package is identical whatever the thread count.
    pub fn write_3mf(&self, name: &str, material: &str) -> Vec<u8> {
        let mut min = [f64::MAX; 3];
        for v in &self.verts {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
            }
        }

        let mat = if material.is_empty() {
            String::new()
        } else {
            format!(
                "<basematerials id=\"1\"><base name=\"{}\"/></basematerials>",
                esc(material)
            )
        };
        let pid = if material.is_empty() { "" } else { " pid=\"1\" pindex=\"0\"" };
        let name_attr = if name.is_empty() {
            String::new()
        } else {
            format!(" name=\"{}\"", esc(name))
        };
        let types = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"rels\" \
             ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"model\" \
             ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>",
            "</Types>"
        );
        let rels = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" \
             Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>",
            "</Relationships>"
        );
        // The model text starts at the xml declaration and the object header; the streamed
        // vertex and triangle chunks fill the middle.
        let head = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <model unit=\"millimeter\" xml:lang=\"en-US\" \
             xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
             <resources>{mat}<object id=\"2\" type=\"model\"{name_attr}{pid}><mesh>"
        );
        let tail = "</mesh></object></resources>\n<build><item objectid=\"2\"/></build>\n</model>\n";

        // One window of chunk texts (and their segments) in flight at a time; 16 chunks
        // (~55 MB of text) is the largest window the memory argument needs, whatever the
        // host reports.
        let window = rayon::current_num_threads().clamp(1, 16);
        let vfmt = |ci: usize| -> String {
            let a = ci * STREAM_CHUNK;
            let b = (a + STREAM_CHUNK).min(self.verts.len());
            let mut s = String::with_capacity((b - a) * 44);
            // `write!` into the pre-sized chunk: no allocation per element (a per-element
            // `format!` serialises the rayon window on the wasm allocator's global lock).
            for v in &self.verts[a..b] {
                let _ = write!(
                    s,
                    "<vertex x=\"{:.6}\" y=\"{:.6}\" z=\"{:.6}\"/>",
                    v[0] - min[0],
                    v[1] - min[1],
                    v[2] - min[2]
                );
            }
            s
        };
        let tfmt = |ci: usize| -> String {
            let a = ci * STREAM_CHUNK;
            let b = (a + STREAM_CHUNK).min(self.tris.len());
            let mut s = String::with_capacity((b - a) * 52);
            for t in &self.tris[a..b] {
                let _ = write!(s, "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
            }
            s
        };

        let mut out = Vec::new();
        let mut records = Vec::new();
        stream_entry(&mut out, "[Content_Types].xml", types.as_bytes(), &mut records);
        stream_entry(&mut out, "_rels/.rels", rels.as_bytes(), &mut records);
        let mut z = ZipStream::start(&mut out, "3D/3dmodel.model");
        z.push(head.as_bytes());
        z.push(b"<vertices>");
        feed_chunked(&mut z, self.verts.len(), window, &vfmt);
        z.push(b"</vertices><triangles>");
        feed_chunked(&mut z, self.tris.len(), window, &tfmt);
        z.push(b"</triangles>");
        z.push(tail.as_bytes());
        records.push(z.finish());
        write_central(&mut out, &records);
        out
    }
}

/// XML-escape the five built-in entities.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(u: V3, v: V3) -> V3 {
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}
fn len(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn unit(v: V3) -> V3 {
    let l = len(v);
    if l < 1e-24 {
        [0.0; 3]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}
fn dot(u: V3, v: V3) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

#[cfg(test)]
fn aabb_cells(aabb: [[f64; 3]; 2], cell: f64) -> impl Iterator<Item = [i64; 3]> {
    let mut out = Vec::with_capacity(8);
    for x in (aabb[0][0] / cell).floor() as i64..=(aabb[1][0] / cell).ceil() as i64 {
        for y in (aabb[0][1] / cell).floor() as i64..=(aabb[1][1] / cell).ceil() as i64 {
            for z in (aabb[0][2] / cell).floor() as i64..=(aabb[1][2] / cell).ceil() as i64 {
                out.push([x, y, z]);
            }
        }
    }
    out.into_iter()
}

fn overlap(x: &[[f64; 3]; 2], y: &[[f64; 3]; 2]) -> bool {
    for k in 0..3 {
        if x[1][k] < y[0][k] || y[1][k] < x[0][k] {
            return false;
        }
    }
    true
}

/// Does any edge of `ta` cross the interior of `tb`? Shared-vertex pairs are excluded by the
/// caller, so a crossing edge cannot hit a target vertex.
fn seg_vs_tri(m: &TriMesh, ta: &[u32; 3], tb: &[u32; 3]) -> bool {
    let b = tb.map(|i| m.verts[i as usize]);
    let n = unit(cross(sub(b[1], b[0]), sub(b[2], b[0])));
    if n == [0.0; 3] {
        return false; // degenerate target: counted separately in Defects
    }
    let d0 = dot(n, b[0]);
    for i in 0..3 {
        let p0 = m.verts[ta[i] as usize];
        let p1 = m.verts[ta[(i + 1) % 3] as usize];
        let e = sub(p1, p0);
        let den = dot(n, e);
        if den.abs() < 1e-15 {
            continue; // edge grazing (or parallel to) the target plane
        }
        let t = (d0 - dot(n, p0)) / den;
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let q = [p0[0] + t * e[0], p0[1] + t * e[1], p0[2] + t * e[2]];
        if inside_trip(b, q, 1e-12) {
            return true;
        }
    }
    false
}

/// Inside test on the target triangle, inclusive epsilon (a point landing exactly on a target
/// edge still counts as a crossing). The per-edge `dot(cross(b - a, q - a), cross(b - a, c - a))`
/// measures the signed distance of `q` from edge `ab` in units of the opposite altitude, so
/// `eps` is relative and scale-free: the same geometry a hundred times smaller must reach the
/// same verdict. Both cross products are plane normals, so the ratio is ±(distance/altitude).
fn inside_trip(t: [V3; 3], q: V3, eps: f64) -> bool {
    for i in 0..3 {
        let a = t[i];
        let (b, c) = (t[(i + 1) % 3], t[(i + 2) % 3]);
        let e = cross(sub(b, a), sub(q, a));
        let f = cross(sub(b, a), sub(c, a));
        let g = dot(e, f);
        if g >= 0.0 {
            continue; // inside this edge's half-plane, or exactly on it
        }
        let scale = len(e) * len(f);
        if scale > 0.0 && -g / scale > eps {
            return false; // measurably outside edge `ab`
        }
        // |g| within eps of the edge: numerically on the boundary, count as crossing.
    }
    true
}

// Streaming zip writer for the 3MF package. Each entry's payload is DEFLATE-compressed in
// independent segments (pigz-style): every segment is a fresh level-1 compressor ending on a
// sync flush, so it is a byte-aligned run of non-final blocks, and segments concatenate into
// one valid raw stream that a final empty block closes. Segments of the model text are
// formatted and compressed in parallel and appended in index order, so no payload is ever
// held whole for [`TriMesh::write_3mf`] and the bytes do not depend on the thread count.

/// Compressed-space scratch for one `deflate` pass; any single emitted block fits well below.
const DEFLATE_BUF: usize = 512 * 1024;
/// Elements per streamed text chunk (one DEFLATE segment each). One window of chunks is
/// formatted and compressed at a time. Tests run small chunks so packages carry many segments.
const STREAM_CHUNK: usize = if cfg!(test) { 1024 } else { 65536 };

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Central-directory record of one entry, built when it finishes.
struct CentralEntry {
    name: String,
    offset: u32,
    crc: u32,
    comp: u32,
    uncomp: u32,
}

/// One independently compressed piece of an entry's payload: raw DEFLATE blocks ending on a
/// sync flush (byte-aligned, none final), plus the crc and length of the bytes it encodes.
struct Segment {
    z: Vec<u8>,
    crc: crc32fast::Hasher,
    len: usize,
}

fn level1() -> CompressorOxide {
    CompressorOxide::new(create_comp_flags_from_zip_params(1, 0, 0))
}

/// Compress `data` as one [`Segment`] with a fresh level-1 compressor.
fn deflate_segment(data: &[u8]) -> Segment {
    let mut crc = crc32fast::Hasher::default();
    crc.update(data);
    let mut d = level1();
    let mut buf = vec![0; DEFLATE_BUF];
    let mut z = Vec::with_capacity(data.len() / 4 + 64);
    let mut input = data;
    loop {
        let (status, used, made) = deflate_compress(&mut d, input, &mut buf, TDEFLFlush::Sync);
        assert_eq!(status, TDEFLStatus::Okay, "deflate segment failed: {status:?}");
        z.extend_from_slice(&buf[..made]);
        input = &input[used..];
        // Done once the input is consumed and the flush fitted the scratch buffer.
        if input.is_empty() && made < buf.len() {
            break;
        }
        if used == 0 && made == 0 {
            panic!("deflate made no progress on {} bytes", input.len());
        }
    }
    Segment { z, crc, len: data.len() }
}

/// One streamed zip entry. Segments append in order; [`ZipStream::finish`] closes the stream
/// with a final empty block and patches the local header's crc and size fields, so the sizes
/// never have to be known before the payload is written.
struct ZipStream<'a> {
    out: &'a mut Vec<u8>,
    hdr_at: usize,
    name: String,
    crc: crc32fast::Hasher,
    uncompressed: usize,
    compressed: usize,
}

impl<'a> ZipStream<'a> {
    fn start(out: &'a mut Vec<u8>, name: &str) -> Self {
        let hdr_at = out.len();
        push_u32(out, 0x04034b50);
        push_u16(out, 20); // version needed
        push_u16(out, 0); // flags
        push_u16(out, 8); // method: deflate
        push_u16(out, 0); // mod time
        push_u16(out, 0); // mod date (1980-01-01, the epoch zip pickers accept)
        push_u32(out, 0); // crc, patched in finish
        push_u32(out, 0); // compressed size, patched in finish
        push_u32(out, 0); // uncompressed size, patched in finish
        push_u16(out, name.len() as u16);
        push_u16(out, 0); // extra len
        out.extend_from_slice(name.as_bytes());
        Self {
            hdr_at,
            name: name.into(),
            crc: crc32fast::Hasher::default(),
            uncompressed: 0,
            compressed: 0,
            out,
        }
    }

    /// Append an already compressed segment.
    fn push_segment(&mut self, seg: Segment) {
        self.out.extend_from_slice(&seg.z);
        self.compressed += seg.z.len();
        self.crc.combine(&seg.crc);
        self.uncompressed += seg.len;
    }

    /// Compress and append a small payload piece as its own segment.
    fn push(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.push_segment(deflate_segment(data));
        }
    }

    /// End the payload: the final (empty) block, then patch the local header's crc and
    /// sizes, and return the central-directory record.
    fn finish(self) -> CentralEntry {
        let mut d = level1();
        let mut buf = vec![0; 64];
        let (status, _, made) = deflate_compress(&mut d, &[], &mut buf, TDEFLFlush::Finish);
        assert_eq!(status, TDEFLStatus::Done, "deflate stream did not end: {status:?}");
        self.out.extend_from_slice(&buf[..made]);
        let compressed = self.compressed + made;
        let crc = self.crc.finalize();
        let hdr = &mut self.out[self.hdr_at..];
        hdr[14..18].copy_from_slice(&crc.to_le_bytes());
        hdr[18..22].copy_from_slice(&(compressed as u32).to_le_bytes());
        hdr[22..26].copy_from_slice(&(self.uncompressed as u32).to_le_bytes());
        CentralEntry {
            name: self.name,
            offset: self.hdr_at as u32,
            crc,
            comp: compressed as u32,
            uncomp: self.uncompressed as u32,
        }
    }
}

/// One in-memory payload through the streamed path (the two small package entries).
fn stream_entry(out: &mut Vec<u8>, name: &str, data: &[u8], records: &mut Vec<CentralEntry>) {
    let mut z = ZipStream::start(out, name);
    z.push(data);
    records.push(z.finish());
}

/// Feed `n` elements to `z`, formatted in [`STREAM_CHUNK`] chunks, `window` chunks at a time
/// on the rayon pool — each chunk formatted and compressed into its own segment — and
/// appended in index order. At most one window's text and segments exist at once, which is
/// what bounds the writer's memory for the gigabyte model entries.
fn feed_chunked(
    z: &mut ZipStream<'_>,
    n: usize,
    window: usize,
    fmt: &(impl Fn(usize) -> String + Sync),
) {
    let chunks = n.div_ceil(STREAM_CHUNK);
    let mut at = 0;
    while at < chunks {
        let end = (at + window).min(chunks);
        let segs: Vec<Segment> =
            (at..end).into_par_iter().map(|ci| deflate_segment(fmt(ci).as_bytes())).collect();
        for seg in segs {
            z.push_segment(seg);
        }
        at = end;
    }
}

/// Central directory for the finished entries, then the package's end record; fields as the
/// one-shot writer before it.
fn write_central(out: &mut Vec<u8>, records: &[CentralEntry]) {
    let cd_offset = out.len() as u32;
    for e in records {
        push_u32(out, 0x02014b50);
        push_u16(out, 20); // version made by
        push_u16(out, 20); // version needed
        push_u16(out, 0); // flags
        push_u16(out, 8); // method
        push_u16(out, 0); // time
        push_u16(out, 0); // date
        push_u32(out, e.crc);
        push_u32(out, e.comp);
        push_u32(out, e.uncomp);
        push_u16(out, e.name.len() as u16);
        push_u16(out, 0); // extra
        push_u16(out, 0); // comment
        push_u16(out, 0); // disk start
        push_u16(out, 0); // internal attrs
        push_u32(out, 0); // external attrs
        push_u32(out, e.offset);
        out.extend_from_slice(e.name.as_bytes());
    }
    let cd_size = out.len() as u32 - cd_offset;
    push_u32(out, 0x06054b50);
    push_u16(out, 0);
    push_u16(out, 0);
    push_u16(out, records.len() as u16);
    push_u16(out, records.len() as u16);
    push_u32(out, cd_size);
    push_u32(out, cd_offset);
    push_u16(out, 0);
}

/// Parse a strict binary STL produced by [`TriMesh::write_stl`] or an equivalent writer that
/// pads the record to exactly 50 bytes. Welds positions that are equal as f32 back into
/// shared vertices. Returns `None` for a malformed file (wrong size, truncated record).
pub fn read_stl(bytes: &[u8]) -> Option<TriMesh> {
    if bytes.len() < 84 {
        return None;
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    if bytes.len() != 84 + n * 50 {
        return None;
    }
    let mut verts: Vec<V3> = Vec::new();
    let mut tris = Vec::with_capacity(n);
    let mut index: HashMap<[u32; 3], u32> = HashMap::new();
    let mut rec = [0u8; 50];
    for i in 0..n {
        rec.copy_from_slice(&bytes[84 + i * 50..84 + (i + 1) * 50]);
        // layout: 12 bytes normal, then three xyx corner positions, then u16 attribute
        let f = |o: usize| f32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]]);
        let mut tri = [0u32; 3];
        for (k, off) in [12usize, 24, 36].into_iter().enumerate() {
            // Canonicalise the payload to bits (+0.0 == −0.0 as a vertex) so array keys hash.
            let p: [f32; 3] = [f(off), f(off + 4), f(off + 8)];
            let p = p.map(|c| if c == 0.0 { 0.0 } else { c });
            let next = verts.len() as u32;
            tri[k] = *index
                .entry([p[0].to_bits(), p[1].to_bits(), p[2].to_bits()])
                .or_insert_with(|| {
                    verts.push([p[0] as f64, p[1] as f64, p[2] as f64]);
                    next
                });
        }
        tris.push(tri);
    }
    Some(TriMesh { verts, tris })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// The previous defect scan (hash maps of directed edges and of grid cells), kept as
    /// the reference the bounded-memory scan must reproduce count for count.
    fn defects_reference(m: &TriMesh) -> Defects {
        let mut d = Defects::default();
        let mut edges: HashMap<(u32, u32), i32> = HashMap::with_capacity(m.tris.len() * 3);
        for &t in &m.tris {
            if m.area(t) <= 1e-12 {
                d.degenerate_triangles += 1;
                continue;
            }
            for i in 0..3 {
                *edges.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
            }
        }
        let mut seen: HashSet<(u32, u32)> = HashSet::new();
        for k in edges.keys().copied().collect::<Vec<_>>() {
            let (a, b) = (k.0.min(k.1), k.0.max(k.1));
            if a == b || !seen.insert((a, b)) {
                continue;
            }
            let f = edges.get(&(a, b)).copied().unwrap_or(0);
            let b_ = edges.get(&(b, a)).copied().unwrap_or(0);
            let usage = f + b_;
            if usage == 1 {
                d.open_edges += 1;
            } else if usage >= 3 {
                d.non_manifold_edges += 1;
            } else if (f, b_) != (1, 1) {
                d.orientation_defects += 1;
            }
        }
        let cell = m.hash_cell();
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, t) in m.tris.iter().enumerate() {
            for k in aabb_cells(m.tri_aabb(t), cell) {
                grid.entry(k).or_default().push(i);
            }
        }
        for (i, t) in m.tris.iter().enumerate() {
            let aabb = m.tri_aabb(t);
            let mut candidates: Vec<usize> = Vec::new();
            for k in aabb_cells(aabb, cell) {
                candidates.extend(grid.get(&k).into_iter().flatten().copied());
            }
            candidates.sort_unstable();
            candidates.dedup();
            for j in candidates {
                if j <= i {
                    continue;
                }
                let t2 = m.tris[j];
                if t.iter().any(|&v| t2.contains(&v)) {
                    continue;
                }
                if !overlap(&aabb, &m.tri_aabb(&t2)) {
                    continue;
                }
                if seg_vs_tri(m, t, &t2) || seg_vs_tri(m, &t2, t) {
                    d.self_intersections += 1;
                }
            }
        }
        d
    }

    #[test]
    fn defects_match_reference_scan() {
        // Gyroid sheet through the adaptive mesher: open, non-manifold, degenerate and
        // self-intersecting classes all present, thousands of each kind of cell bucket.
        let gyroid = |p: V3| -> crate::adaptive::Sample {
            let k = 2.0 * std::f64::consts::PI / 6.0;
            let (x, y, z) = (k * p[0], k * p[1], k * p[2]);
            let g = x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
            let dg = [
                x.cos() * y.cos() - z.sin() * x.sin(),
                -x.sin() * y.sin() + y.cos() * z.cos(),
                -y.sin() * z.sin() + z.cos() * x.cos(),
            ];
            let s = g.signum();
            let sheet = (g.abs() - 0.35) / k;
            let b = p.iter().map(|c| c.abs() - 9.0).fold(f64::MIN, f64::max);
            if sheet >= b {
                crate::adaptive::Sample { v: sheet, g: [s * dg[0], s * dg[1], s * dg[2]] }
            } else {
                let mut g = [0.0; 3];
                let a = (0..3).max_by(|&i, &j| p[i].abs().total_cmp(&p[j].abs())).unwrap();
                g[a] = p[a].signum();
                crate::adaptive::Sample { v: b, g }
            }
        };
        let adaptive = crate::adaptive::surface_adaptive(
            gyroid,
            [-10.0; 3],
            [10.0; 3],
            0.25,
            &crate::adaptive::AdaptiveOpts::default(),
        );
        let mut scrambled = surface(sphere, [-10.0; 3], [10.0; 3], 1.0);
        for (i, t) in scrambled.tris.iter_mut().enumerate() {
            if i % 7 == 0 {
                t.swap(1, 2); // orientation defects
            }
        }
        scrambled.tris.truncate(scrambled.tris.len() - 5); // open edges
        let crossing = TriMesh {
            verts: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [1.0, 1.0, -1.0],
                [3.0, 1.0, 1.0],
                [1.0, 3.0, 1.0],
            ],
            tris: vec![[0, 1, 2], [3, 4, 5], [0, 0, 1]],
        };
        let nm = TriMesh {
            verts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
            tris: vec![[0, 1, 2], [0, 1, 3], [0, 1, 4], [1, 0, 2]],
        };
        for (name, m) in [
            ("adaptive gyroid", &adaptive),
            ("scrambled sphere", &scrambled),
            ("crossing", &crossing),
            ("non-manifold fan", &nm),
        ] {
            let want = defects_reference(m);
            let got = m.defects();
            println!("{name}: {} tris {got:?}", m.tris.len());
            assert_eq!(got, want, "{name}: bounded scan diverged from the reference");
        }
        let d = adaptive.defects();
        assert!(d.open_edges > 0 && d.self_intersections > 0, "test mesh must exercise the classes: {d:?}");
    }

    /// Walk the local file headers of a zip written by [`zip`] and inflate each entry,
    /// re-checking the CRCs on the way.
    fn zip_entries(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
        let u32at = |i: usize| u32::from_le_bytes(archive[i..i + 4].try_into().unwrap());
        let u16at = |i: usize| u16::from_le_bytes(archive[i..i + 2].try_into().unwrap());
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + 30 <= archive.len() && u32at(i) == 0x04034b50 {
            let method = u16at(i + 8);
            let crc = u32at(i + 14);
            let csize = u32at(i + 18) as usize;
            let usize_ = u32at(i + 22) as usize;
            let nlen = u16at(i + 26) as usize;
            let name = String::from_utf8(archive[i + 30..i + 30 + nlen].to_vec()).unwrap();
            let raw = &archive[i + 30 + nlen..i + 30 + nlen + csize];
            let data = if method == 8 {
                miniz_oxide::inflate::decompress_to_vec(raw).unwrap()
            } else {
                raw.to_vec()
            };
            assert_eq!(data.len(), usize_, "entry {name} size mismatch");
            assert_eq!(crc32fast::hash(&data), crc, "entry {name} crc mismatch");
            out.push((name, data));
            i += 30 + nlen + csize;
        }
        out
    }

    fn sphere(p: V3) -> f64 {
        (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 10.0
    }

    fn cube(p: V3) -> f64 {
        let t = [p[0].abs() - 10.0, p[1].abs() - 10.0, p[2].abs() - 10.0];
        let o = [t[0].max(0.0), t[1].max(0.0), t[2].max(0.0)];
        (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt() + t[0].max(t[1]).max(t[2]).min(0.0)
    }

    #[test]
    fn sphere_surface_is_clean() {
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 1.0);
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        assert!((m.volume() - exact).abs() / exact < 0.01, "volume {}", m.volume());
        let d = m.defects();
        assert!(d.none(), "defects {d:?}");
    }

    #[test]
    fn cube_surface_is_exact() {
        let m = surface(cube, [-10.0; 3], [10.0; 3], 1.0);
        assert!((m.volume() - 8000.0).abs() < 1e-6, "volume {}", m.volume());
        assert!(m.defects().none(), "defects {:?}", m.defects());
    }

    #[test]
    fn defect_classes() {
        // One triangle: its three edges are each used once.
        let open = TriMesh {
            verts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            tris: vec![[0, 1, 2]],
        };
        let d = open.defects();
        assert_eq!(d.open_edges, 3);
        assert_eq!(d.orientation_defects, 0);
        assert_eq!(d.non_manifold_edges, 0);

        // Two triangles sharing edge (0,1) and winding the same way: that edge is flipped.
        let flip = TriMesh {
            verts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 2.0, 0.0]],
            tris: vec![[0, 1, 2], [0, 1, 3]],
        };
        let d = flip.defects();
        assert_eq!(d.open_edges, 4, "outer edges of both triangles");
        assert_eq!(d.orientation_defects, 1, "the shared edge");

        // Three triangles on edge (0,1): non-manifold there, still one orientation class.
        let nm = TriMesh {
            verts: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            tris: vec![[0, 1, 2], [0, 1, 3], [0, 1, 4]],
        };
        let d = nm.defects();
        assert_eq!(d.non_manifold_edges, 1);
        assert_eq!(d.open_edges, 6);
        assert_eq!(d.orientation_defects, 0);
    }

    #[test]
    fn crossing_pair_detected() {
        let m = TriMesh {
            verts: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0], // triangle in the z = 0 plane
                [1.0, 1.0, -1.0],
                [3.0, 1.0, 1.0],
                [1.0, 3.0, 1.0], // triangle cutting through it from z− to z+
            ],
            tris: vec![[0, 1, 2], [3, 4, 5]],
        };
        // Segment (2,1,0)–(1,2,0) of the crossing lies inside the flat triangle.
        assert_eq!(m.self_intersections(), 1);
        assert_eq!(m.defects().self_intersections, 1);
        assert_eq!(m.defects().open_edges, 6, "two isolated triangles");
    }

    #[test]
    fn stl_round_trip_stays_watertight() {
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 1.0);
        let bytes = m.write_stl();
        let back = read_stl(&bytes).expect("our own STL must parse");
        let d = back.defects();
        assert!(d.none(), "defects after f32 round trip: {d:?}");
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        let v = back.volume();
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
    }

    #[test]
    fn slice_cut_traces_the_profile() {
        let m = surface(cube, [-10.0; 3], [10.0; 3], 1.0);
        // Mid-plane of the 20 mm cube. The stuffed boundary splits each wall into lattice
        // quads, so the cut arrives as many unit-length segments; what must hold is that
        // together they trace the full 80 mm square outline, inside the face bounds.
        let segs = m.slice_at(0.0);
        assert!(segs.len() >= 16, "at least four segments, got {}", segs.len() / 4);
        let mut total = 0.0;
        let mut i = 0;
        while i + 3 < segs.len() {
            total += (segs[i + 2] - segs[i]).hypot(segs[i + 3] - segs[i + 1]);
            i += 4;
        }
        assert!((total - 80.0).abs() < 1e-6, "outline length {total}, want 80");
        assert!(m.slice_at(10.5).is_empty(), "nothing above the solid");
        assert!(m.slice_at(-10.5).is_empty(), "nothing below the solid");
        assert!(m.slice_at(0.0).iter().all(|c| (-10.0..=10.0).contains(c)));
    }

    #[test]
    fn slice_plane_vertex_touches_stay_clean() {
        // A tetrahedron with one apex exactly on the cutting plane: the apex counts as below
        // it, so its two incident edges straddle and their crossing points sit on the apex.
        // No zero-length segment may survive the cut.
        let m = TriMesh {
            verts: vec![
                [0.0, 0.0, -1.0],
                [2.0, 0.0, -1.0],
                [0.0, 2.0, -1.0],
                [1.0, 1.0, 0.0], // apex exactly on the plane z = 0
                [0.0, 0.0, 3.0],
            ],
            tris: vec![[0, 1, 3], [0, 3, 2], [1, 2, 3], [0, 2, 4], [0, 1, 4], [1, 3, 4], [2, 3, 4]],
        };
        let segs = m.slice_at(0.0);
        let mut any = false;
        let mut i = 0;
        while i + 3 < segs.len() {
            let l = (segs[i + 2] - segs[i]).hypot(segs[i + 3] - segs[i + 1]);
            assert!(l > 1e-9, "zero-length segment at {i}: {l}");
            any = true;
            i += 4;
        }
        assert!(any, "the plane z = 0 does cut this solid");
    }

    #[test]
    fn three_mf_package_structure() {
        let m = surface(cube, [-10.0; 3], [10.0; 3], 1.0);
        let bytes = m.write_3mf("test \"bracket\"", "petg");
        let entries = zip_entries(&bytes);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]);
        let model = String::from_utf8(entries[2].1.clone()).unwrap();
        assert!(model.contains("unit=\"millimeter\""), "{model}");
        assert!(model.contains("name=\"test &quot;bracket&quot;\""), "{model}");
        assert!(model.contains("base name=\"petg\""), "{model}");
        assert_eq!(model.matches("<vertex ").count(), m.verts.len());
        assert_eq!(model.matches("<triangle ").count(), m.tris.len());
        assert!(!model.contains("x=\"-"), "coordinates must be in the positive octant");
        assert!(model.contains("<build><item objectid=\"2\"/></build>"), "{model}");
        let types = String::from_utf8(entries[0].1.clone()).unwrap();
        assert!(types.contains("3dmanufacturing-3dmodel+xml"), "{types}");
    }

    #[test]
    fn three_mf_package_is_identical_across_thread_counts() {
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 1.0);
        let p1 = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        let p4 = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let one = p1.install(|| m.write_3mf("stable", "petg"));
        let four = p4.install(|| m.write_3mf("stable", "petg"));
        assert_eq!(one, four, "package bytes must not depend on the thread count");
    }

    /// Golden digest of a whole package (compressed layout included). Re-pinned when the
    /// model entry moved from one sequential DEFLATE stream to independent parallel
    /// segments; the text itself is pinned unchanged by `three_mf_model_text_is_pinned`.
    #[test]
    fn three_mf_package_bytes_are_pinned() {
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 0.5);
        let bytes = m.write_3mf("pinned", "petg");
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &bytes {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        assert_eq!((bytes.len(), format!("{h:016x}")), (578625, "98494099064e7b7c".to_string()));
    }

    /// Every entry of a package: (name, compressed stream, header crc, header length).
    fn zip_raw_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>, u32, usize)> {
        let u32at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let u16at = |i: usize| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
        let mut i = 0usize;
        let mut out = Vec::new();
        while bytes.len() >= i + 30 && u32at(i) == 0x04034b50 {
            let crc = u32at(i + 14);
            let csize = u32at(i + 18) as usize;
            let usize_ = u32at(i + 22) as usize;
            let nlen = u16at(i + 26) as usize;
            let name = String::from_utf8(bytes[i + 30..i + 30 + nlen].to_vec()).unwrap();
            out.push((name, bytes[i + 30 + nlen..i + 30 + nlen + csize].to_vec(), crc, usize_));
            i += 30 + nlen + csize;
        }
        out
    }

    fn fnv(bytes: &[u8]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:016x}")
    }

    /// Golden digest of the model text (inflated), captured from the per-element `format!`
    /// writer with one sequential DEFLATE stream: whatever the compression layout, the text
    /// the slicer reads is pinned.
    #[test]
    fn three_mf_model_text_is_pinned() {
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 0.5);
        let bytes = m.write_3mf("pinned", "petg");
        let e = zip_raw_entries(&bytes);
        let text = miniz_oxide::inflate::decompress_to_vec(&e[2].1).unwrap();
        assert_eq!((text.len(), fnv(&text)), (4516761, "1304e423a349b9bc".to_string()));
    }

    #[test]
    fn segmented_entries_inflate_to_their_payloads() {
        // Each entry is concatenated sync-flushed segments closed by one final block: it must
        // be a single valid raw DEFLATE stream that ends exactly at the entry's compressed
        // size and inflates to the header's length and crc. The sphere at 0.5 with the test
        // chunk of 1024 elements carries several segments per section.
        let m = surface(sphere, [-10.0; 3], [10.0; 3], 0.5);
        assert!(m.tris.len() > 4 * STREAM_CHUNK, "test mesh must span several segments");
        let bytes = m.write_3mf("x", "petg");
        let entries = zip_raw_entries(&bytes);
        assert_eq!(entries.len(), 3, "walked every streamed entry");
        for (name, raw, crc, len) in &entries {
            let mut st =
                miniz_oxide::inflate::stream::InflateState::new(miniz_oxide::DataFormat::Raw);
            let mut text = vec![0u8; len + 1];
            let r = miniz_oxide::inflate::stream::inflate(
                &mut st,
                raw,
                &mut text,
                miniz_oxide::MZFlush::Finish,
            );
            assert_eq!(r.status, Ok(miniz_oxide::MZStatus::StreamEnd), "{name}: stream must end");
            assert_eq!(r.bytes_consumed, raw.len(), "{name}: final block ends the entry");
            assert_eq!(r.bytes_written, *len, "{name}: inflated length");
            let mut h = crc32fast::Hasher::new();
            h.update(&text[..*len]);
            assert_eq!(h.finalize(), *crc, "{name}: crc");
        }
    }
}