//! Tetrahedral mesh quality measures.

use crate::{TetMesh, V3};

/// Summary of element quality.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Quality {
    pub tets: usize,
    pub min_dihedral_deg: f64,
    pub max_dihedral_deg: f64,
    /// Smallest volume relative to the cube of the mean edge length.
    pub min_volume_ratio: f64,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The six dihedral angles (degrees) of a tetrahedron.
pub fn dihedrals(p: [V3; 4]) -> [f64; 6] {
    // Face normals (outward up to sign) opposite each vertex.
    let n = |a: usize, b: usize, c: usize| cross(sub(p[b], p[a]), sub(p[c], p[a]));
    let faces = [n(1, 2, 3), n(0, 3, 2), n(0, 1, 3), n(0, 2, 1)];
    let mut out = [0.0; 6];
    let mut k = 0;
    for i in 0..4 {
        for j in (i + 1)..4 {
            // The edge opposite faces i and j; dihedral = π − angle between outward normals.
            let c = dot(faces[i], faces[j]) / (len(faces[i]) * len(faces[j])).max(1e-300);
            out[k] = 180.0 - c.clamp(-1.0, 1.0).acos().to_degrees();
            k += 1;
        }
    }
    out
}

pub fn quality(m: &TetMesh) -> Quality {
    let mut q = Quality {
        tets: m.tets.len(),
        min_dihedral_deg: 180.0,
        max_dihedral_deg: 0.0,
        min_volume_ratio: f64::INFINITY,
    };
    for t in &m.tets {
        let p = t.map(|i| m.verts[i as usize]);
        for d in dihedrals(p) {
            q.min_dihedral_deg = q.min_dihedral_deg.min(d);
            q.max_dihedral_deg = q.max_dihedral_deg.max(d);
        }
        let e = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        let mean = e.iter().map(|&(a, b)| len(sub(p[a], p[b]))).sum::<f64>() / 6.0;
        let v = dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6.0;
        q.min_volume_ratio = q.min_volume_ratio.min(v / mean.powi(3));
    }
    if m.tets.is_empty() {
        q.min_dihedral_deg = 0.0;
        q.min_volume_ratio = 0.0;
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_tet_dihedral() {
        let s = 1.0 / 2f64.sqrt();
        let p = [
            [1.0, 0.0, -s],
            [-1.0, 0.0, -s],
            [0.0, 1.0, s],
            [0.0, -1.0, s],
        ];
        for d in dihedrals(p) {
            assert!((d - 70.528779).abs() < 1e-5, "{d}");
        }
    }
}
