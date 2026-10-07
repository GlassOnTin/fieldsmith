//! Vertex welding and patch segmentation (goal M6, step 2).
//!
//! STL writes every triangle's corners, so imports arrive as disconnected triangles; welding
//! merges vertices that sit within a tolerance into shared ones. Segmentation then groups
//! triangles that meet along a welded edge at a shallow dihedral angle — one id per smooth
//! face of a meshy export, used later as the patch grid's group id. Both steps are
//! deterministic: representatives by scan order, BFS by triangle index, no hash-order
//! dependence.

use crate::export::TriMesh;
use crate::V3;
use std::collections::{HashMap, VecDeque};

/// Merge vertices closer than `tol` (quantised-cell hashing over a 27-cell neighbourhood,
/// union-find with smallest-root representatives). Triangle indices are remapped; no
/// triangles are dropped, so a weld that collapses corners leaves degenerate triangles in
/// place (callers count them in the defect report).
pub fn weld_verts(m: &TriMesh, tol: f64) -> TriMesh {
    assert!(tol > 0.0, "weld tolerance must be positive");
    let n = m.verts.len();
    let mut parent: Vec<u32> = (0..n as u32).collect();
    fn find(parent: &mut [u32], a: u32) -> u32 {
        let mut a = a;
        while parent[a as usize] != a {
            parent[a as usize] = parent[parent[a as usize] as usize];
            a = parent[a as usize];
        }
        a
    }
    let mut cells: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let cell_of =
        |p: V3| -> [i64; 3] { [p[0] / tol, p[1] / tol, p[2] / tol].map(|c| c.floor() as i64) };
    for (i, &p) in m.verts.iter().enumerate() {
        cells.entry(cell_of(p)).or_default().push(i as u32);
    }
    for (i, &p) in m.verts.iter().enumerate() {
        let c = cell_of(p);
        for dx in [-1i64, 0, 1] {
            for dy in [-1i64, 0, 1] {
                for dz in [-1i64, 0, 1] {
                    let Some(list) = cells.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else {
                        continue;
                    };
                    for &j in list {
                        if j as usize == i {
                            continue;
                        }
                        let q = m.verts[j as usize];
                        let d =
                            (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
                        if d <= tol * tol {
                            let (a, b) = (find(&mut parent, i as u32), find(&mut parent, j));
                            if a != b {
                                // smallest root wins: representative choice is order-independent
                                if a < b {
                                    parent[b as usize] = a;
                                } else {
                                    parent[a as usize] = b;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // First vertex in scan order per component provides the position and the new index.
    let mut slot: Vec<Option<u32>> = vec![None; n];
    let mut verts: Vec<V3> = Vec::new();
    for (i, &p) in m.verts.iter().enumerate() {
        let r = find(&mut parent, i as u32) as usize;
        if slot[r].is_none() {
            slot[r] = Some(verts.len() as u32);
            verts.push(p);
        }
    }
    let mut remap =
        |i: usize| slot[find(&mut parent, i as u32) as usize].expect("root assigned above");
    TriMesh {
        verts,
        tris: m
            .tris
            .iter()
            .map(|t| t.map(|v| remap(v as usize)))
            .collect(),
    }
}

/// For each triangle slot, the triangle sharing that directed edge (`u32::MAX` when the edge
/// is open). An edge used more than twice is wired only between its first two triangles —
/// non-manifold usage is the defect report's job, not adjacency's.
#[derive(Clone, Debug)]
pub struct Adjacency {
    pub nbr: Vec<[u32; 3]>,
}

pub fn edge_adjacency(m: &TriMesh) -> Adjacency {
    let mut nbr = vec![[u32::MAX; 3]; m.tris.len()];
    // (unordered edge) -> (triangle, slot) of the first user; u32::MAX marks exhausted
    let mut seen: HashMap<(u32, u32), (u32, u8)> = HashMap::with_capacity(m.tris.len() * 3);
    for (ti, t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let key = (a.min(b), a.max(b));
            if a == b {
                continue; // degenerate slot has no edge
            }
            match seen.entry(key) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert((ti as u32, k as u8));
                }
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    let (tj, kj) = *e.get();
                    if tj != u32::MAX {
                        nbr[ti][k] = tj;
                        nbr[tj as usize][kj as usize] = ti as u32;
                        e.insert((u32::MAX, 0));
                    }
                }
            }
        }
    }
    Adjacency { nbr }
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Group triangles into patches: a BFS from the lowest-indexed unvisited triangle, crossing
/// an edge only when both sides have non-zero area and the dihedral between their (absolute)
/// normals is under `max_deg` — so winding direction does not matter. Returns the group id
/// of each triangle, ids starting at 1 and assigned in first-encounter order.
pub fn segment(m: &TriMesh, adj: &Adjacency, max_deg: f64) -> Vec<u32> {
    let mut id = vec![0u32; m.tris.len()];
    let normal = |t: [u32; 3]| -> V3 {
        let (a, b, c) = (
            m.verts[t[0] as usize],
            m.verts[t[1] as usize],
            m.verts[t[2] as usize],
        );
        cross(
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        )
    };
    let flat: Vec<V3> = m.tris.iter().map(|&t| normal(t)).collect();
    let mut next = 0u32;
    for seed in 0..m.tris.len() {
        if id[seed] != 0 {
            continue;
        }
        next += 1;
        id[seed] = next;
        let mut q = VecDeque::new();
        q.push_back(seed);
        while let Some(t) = q.pop_front() {
            for k in 0..3 {
                let tj = adj.nbr[t][k] as usize;
                if tj == u32::MAX as usize || id[tj] != 0 {
                    continue;
                }
                let (na, nb) = (flat[t], flat[tj]);
                let la2 = na[0] * na[0] + na[1] * na[1] + na[2] * na[2];
                let lb2 = nb[0] * nb[0] + nb[1] * nb[1] + nb[2] * nb[2];
                if la2 == 0.0 || lb2 == 0.0 {
                    continue;
                }
                let dt =
                    (na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2]) / (la2.sqrt() * lb2.sqrt());
                let ang = dt.clamp(-1.0, 1.0).abs().acos();
                if ang < max_deg {
                    id[tj] = next;
                    q.push_back(tj);
                }
            }
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tetrahedron: every edge is used exactly twice, so nothing is open.
    fn tet() -> TriMesh {
        let verts: Vec<V3> = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let tris = vec![[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];
        TriMesh { verts, tris }
    }

    /// A cube as two triangles per face, sharing welded corners.
    fn cube() -> TriMesh {
        let verts: Vec<V3> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let tris = vec![
            [0, 2, 1],
            [0, 3, 2], // z = 0
            [4, 5, 6],
            [4, 6, 7], // z = 1
            [0, 1, 5],
            [0, 5, 4], // y = 0
            [2, 3, 7],
            [2, 7, 6], // y = 1
            [1, 2, 6],
            [1, 6, 5], // x = 1
            [3, 0, 4],
            [3, 4, 7], // x = 0
        ];
        TriMesh { verts, tris }
    }

    #[test]
    fn tet_adjacency_has_no_open_edges() {
        let m = tet();
        let adj = edge_adjacency(&m);
        assert!(adj.nbr.iter().all(|t| t.iter().all(|&e| e != u32::MAX)));
        // Each pair of adjacent triangles appears on both sides.
        for (ti, t) in adj.nbr.iter().enumerate() {
            for &tj in t {
                assert!(adj.nbr[tj as usize].contains(&(ti as u32)), "{ti} ↔ {tj}");
            }
        }
    }

    #[test]
    fn cube_segments_into_six_faces() {
        let m = cube();
        let adj = edge_adjacency(&m);
        let id = segment(&m, &adj, 30.0_f64.to_radians());
        let mut ids = id.to_vec();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6], "{id:?}");
        // Deterministic: a second run is identical.
        assert_eq!(segment(&m, &adj, 30.0_f64.to_radians()), id);
    }

    #[test]
    fn weld_merges_nearby_verts_and_keeps_triangles() {
        let m = TriMesh {
            verts: vec![[0.0; 3], [1e-4, 0.0, 0.0], [1.0, 0.0, 0.0]],
            tris: vec![[0, 1, 2]],
        };
        let w = weld_verts(&m, 1e-3);
        assert_eq!(w.verts.len(), 2);
        assert_eq!(w.tris.len(), 1, "no dropping");
        assert_eq!(w.tris[0], [0, 0, 1], "collapsed corners stay as-is");
        // Below the tolerance nothing merges.
        let w = weld_verts(&m, 1e-5);
        assert_eq!(w.verts.len(), 3);
    }
}
