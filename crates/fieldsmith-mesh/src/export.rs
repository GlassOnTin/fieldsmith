//! Export geometry: the closed triangle surface of an implicit solid, its health report, and
//! the STL / 3MF writers.
//!
//! The surface is the boundary triangles of an isosurface-stuffed tetrahedral mesh
//! ([`crate::stuff`]), so watertightness and consistent orientation come from the meshing
//! rather than a repair pass; [`TriMesh::defects`] measures them anyway.

use crate::{stuff, V3};
use std::collections::{HashMap, HashSet};

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
    pub fn defects(&self) -> Defects {
        let mut d = Defects::default();
        let mut edges: HashMap<(u32, u32), i32> = HashMap::with_capacity(self.tris.len() * 3);
        for &t in &self.tris {
            if self.area(t) <= 1e-12 {
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
                continue; // each undirected edge once
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

    /// Pairs of triangles that cross without sharing a vertex, found through a spatial hash
    /// of the triangle bounding boxes. Each intersecting pair is counted once.
    /// Under-reporting (named gap): coplanar-touching pairs that overlap without crossing a
    /// transverse segment, and edges grazing the target plane nearly parallel, are missed.
    pub fn self_intersections(&self) -> usize {
        let cell = self.hash_cell();
        let mut grid: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
        for (i, t) in self.tris.iter().enumerate() {
            let aabb = self.tri_aabb(t);
            for k in aabb_cells(aabb, cell) {
                grid.entry(k).or_default().push(i);
            }
        }
        let mut hits = 0usize;
        for (i, t) in self.tris.iter().enumerate() {
            let aabb = self.tri_aabb(t);
            let mut candidates: Vec<usize> = Vec::new();
            for k in aabb_cells(aabb, cell) {
                candidates.extend(grid.get(&k).into_iter().flatten().copied());
            }
            candidates.sort_unstable();
            candidates.dedup(); // one candidate per pair, whatever the cell overlap
            for j in candidates {
                if j <= i {
                    continue; // visit each pair once
                }
                let t2 = self.tris[j];
                if t.iter().any(|&v| t2.contains(&v)) {
                    continue; // touching triangles share a vertex by construction
                }
                if !overlap(&aabb, &self.tri_aabb(&t2)) {
                    continue;
                }
                if seg_vs_tri(self, t, &t2) || seg_vs_tri(self, &t2, t) {
                    hits += 1;
                }
            }
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
    pub fn write_3mf(&self, name: &str, material: &str) -> Vec<u8> {
        let mut min = [f64::MAX; 3];
        for v in &self.verts {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
            }
        }
        let mut verts = String::with_capacity(self.verts.len() * 40);
        verts.push_str("<vertices>");
        for v in &self.verts {
            verts.push_str(&format!(
                "<vertex x=\"{:.6}\" y=\"{:.6}\" z=\"{:.6}\"/>",
                v[0] - min[0],
                v[1] - min[1],
                v[2] - min[2]
            ));
        }
        verts.push_str("</vertices>");
        let mut tris = String::with_capacity(self.tris.len() * 32);
        tris.push_str("<triangles>");
        for t in &self.tris {
            tris.push_str(&format!("<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]));
        }
        tris.push_str("</triangles>");

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
        let model = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <model unit=\"millimeter\" xml:lang=\"en-US\" \
             xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n\
             <resources>{mat}<object id=\"2\" type=\"model\"{name_attr}{pid}>\
             <mesh>{verts}{tris}</mesh></object></resources>\n\
             <build><item objectid=\"2\"/></build>\n</model>\n"
        );
        let content_types = concat!(
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
        zip(&[
            ("[Content_Types].xml".into(), content_types.as_bytes().to_vec()),
            ("_rels/.rels".into(), rels.as_bytes().to_vec()),
            ("3D/3dmodel.model".into(), model.into_bytes()),
        ])
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

/// Inside test on the target triangle, inclusive epsilon (an edge landing exactly on a target
/// edge still counts as a crossing).
fn inside_trip(t: [V3; 3], q: V3, eps: f64) -> bool {
    let mut worst = 0.0;
    for i in 0..3 {
        let a = t[i];
        let (b, c) = (t[(i + 1) % 3], t[(i + 2) % 3]);
        let e = cross(sub(b, a), sub(q, a));
        let f = cross(sub(b, a), sub(c, a));
        let g = dot(e, f);
        if g < worst {
            worst = g;
        }
    }
    worst >= -eps
}

/// Minimal zip archive: DEFLATE-compressed entries, no zip64 (entries < 4 GiB), no encryption,
/// no extra fields. Sufficient for a conformant 3MF package.
fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let crc = crc32fast::hash(data);
        let comp = miniz_oxide::deflate::compress_to_vec(data, 6);
        let offset = out.len() as u32;
        push_u32(&mut out, 0x04034b50);
        push_u16(&mut out, 20); // version needed
        push_u16(&mut out, 0); // flags
        push_u16(&mut out, 8); // method: deflate
        push_u16(&mut out, 0); // mod time
        push_u16(&mut out, 0); // mod date (1980-01-01, the epoch zip pickers accept)
        push_u32(&mut out, crc);
        push_u32(&mut out, comp.len() as u32);
        push_u32(&mut out, data.len() as u32);
        push_u16(&mut out, name.len() as u16);
        push_u16(&mut out, 0); // extra len
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&comp);

        push_u32(&mut central, 0x02014b50);
        push_u16(&mut central, 20); // version made by
        push_u16(&mut central, 20); // version needed
        push_u16(&mut central, 0); // flags
        push_u16(&mut central, 8); // method
        push_u16(&mut central, 0); // time
        push_u16(&mut central, 0); // date
        push_u32(&mut central, crc);
        push_u32(&mut central, comp.len() as u32);
        push_u32(&mut central, data.len() as u32);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 0); // extra
        push_u16(&mut central, 0); // comment
        push_u16(&mut central, 0); // disk start
        push_u16(&mut central, 0); // internal attrs
        push_u32(&mut central, 0); // external attrs
        push_u32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    let cd_size = (out.len() as u32) - cd_offset;
    push_u32(&mut out, 0x06054b50);
    push_u16(&mut out, 0);
    push_u16(&mut out, 0);
    push_u16(&mut out, entries.len() as u16);
    push_u16(&mut out, entries.len() as u16);
    push_u32(&mut out, cd_size);
    push_u32(&mut out, cd_offset);
    push_u16(&mut out, 0);
    out
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
}