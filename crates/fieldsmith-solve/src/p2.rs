//! P2 tetrahedral mesh: vertices plus edge midpoints.

use crate::V3;
use std::collections::HashMap;

/// Local edges of a P2 tetrahedron, matching `fenics_element::P2Tetrahedron` nodes 4..9.
pub const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (0, 2), (0, 3), (1, 3), (2, 3)];

#[derive(Clone, Debug, Default)]
pub struct P2Mesh {
    /// Node positions: `nverts` vertices first, then one node per edge.
    pub nodes: Vec<V3>,
    pub nverts: usize,
    pub tets: Vec<[u32; 10]>,
    /// For each midside node (index − nverts), its edge's vertex pair.
    pub mid_edge: Vec<[u32; 2]>,
}

impl P2Mesh {
    pub fn from_linear(verts: &[V3], tets: &[[u32; 4]]) -> P2Mesh {
        let nverts = verts.len();
        let mut nodes = verts.to_vec();
        let mut mid_edge = Vec::new();
        let mut edge_id: HashMap<(u32, u32), u32> = HashMap::with_capacity(tets.len() * 2);
        let mut out = Vec::with_capacity(tets.len());
        for t in tets {
            let mut n = [0u32; 10];
            n[..4].copy_from_slice(t);
            for (k, &(a, b)) in EDGES.iter().enumerate() {
                let (va, vb) = (t[a], t[b]);
                let key = if va < vb { (va, vb) } else { (vb, va) };
                let id = *edge_id.entry(key).or_insert_with(|| {
                    let pa = verts[key.0 as usize];
                    let pb = verts[key.1 as usize];
                    nodes.push([(pa[0] + pb[0]) * 0.5, (pa[1] + pb[1]) * 0.5, (pa[2] + pb[2]) * 0.5]);
                    mid_edge.push([key.0, key.1]);
                    (nodes.len() - 1) as u32
                });
                n[4 + k] = id;
            }
            out.push(n);
        }
        P2Mesh { nodes, nverts, tets: out, mid_edge }
    }

    pub fn ndofs(&self) -> usize {
        self.nodes.len() * 3
    }

    /// Element volume (affine).
    pub fn tet_volume(&self, e: usize) -> f64 {
        let t = &self.tets[e];
        let p = |i: usize| self.nodes[t[i] as usize];
        let (a, b, c, d) = (p(0), p(1), p(2), p(3));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        (u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0]) + u[2] * (v[0] * w[1] - v[1] * w[0])) / 6.0
    }
}

/// Local corner and midside node positions of a P2 triangle face of a tetrahedron, given the
/// face's three local vertex indices: returns the 6 global node ids (3 corners, then the
/// midsides of edges 0–1, 1–2, 2–0).
pub fn face_nodes(t: &[u32; 10], f: [usize; 3]) -> [u32; 6] {
    let mid = |a: usize, b: usize| -> u32 {
        for (k, &(x, y)) in EDGES.iter().enumerate() {
            if (x == a && y == b) || (x == b && y == a) {
                return t[4 + k];
            }
        }
        unreachable!()
    };
    [t[f[0]], t[f[1]], t[f[2]], mid(f[0], f[1]), mid(f[1], f[2]), mid(f[2], f[0])]
}
