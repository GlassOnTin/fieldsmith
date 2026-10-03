//! Stress recovery.

use crate::assemble::element_map;
use crate::P2Mesh;
use fenics_assembly::elasticity::ElasticMaterial;
use fenics_element::{FiniteElement, P2Tetrahedron};

/// Symmetric stress tensor (σxx, σyy, σzz, σxy, σyz, σzx), MPa.
pub type Stress = [f64; 6];

pub fn von_mises(s: &Stress) -> f64 {
    let [xx, yy, zz, xy, yz, zx] = *s;
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2)) + 3.0 * (xy * xy + yz * yz + zx * zx)).sqrt()
}

/// Stress in element `e` at reference point `xi`.
pub fn element_stress(mesh: &P2Mesh, u: &[f64], e: usize, young: f64, poisson: f64, xi: [f64; 3]) -> Stress {
    let m = ElasticMaterial::new(young, poisson);
    let (lam, mu) = (m.lame_lambda(), m.lame_mu());
    let map = element_map(mesh, e);
    let g = P2Tetrahedron.evaluate_gradients(&xi);
    let t = &mesh.tets[e];
    // Displacement gradient H[i][j] = ∂u_i/∂x_j.
    let mut h = [[0.0f64; 3]; 3];
    for a in 0..10 {
        let gp = map.transform_gradient(&g[a]);
        let n = t[a] as usize;
        for i in 0..3 {
            for j in 0..3 {
                h[i][j] += u[3 * n + i] * gp[j];
            }
        }
    }
    let tr = h[0][0] + h[1][1] + h[2][2];
    let eps = |i: usize, j: usize| 0.5 * (h[i][j] + h[j][i]);
    [
        lam * tr + 2.0 * mu * eps(0, 0),
        lam * tr + 2.0 * mu * eps(1, 1),
        lam * tr + 2.0 * mu * eps(2, 2),
        2.0 * mu * eps(0, 1),
        2.0 * mu * eps(1, 2),
        2.0 * mu * eps(2, 0),
    ]
}

/// Nodal stresses for every P2 node: element stresses evaluated at the node, averaged over the
/// elements sharing it with volume weights.
pub fn nodal_stress(mesh: &P2Mesh, u: &[f64], young: &[f64], poisson: f64) -> Vec<Stress> {
    let pts = P2Tetrahedron.nodal_coordinates();
    let mut acc = vec![[0.0f64; 6]; mesh.nodes.len()];
    let mut w = vec![0.0f64; mesh.nodes.len()];
    for e in 0..mesh.tets.len() {
        let vol = mesh.tet_volume(e).abs();
        for (a, xi) in pts.iter().enumerate() {
            let s = element_stress(mesh, u, e, young[e], poisson, *xi);
            let n = mesh.tets[e][a] as usize;
            for k in 0..6 {
                acc[n][k] += vol * s[k];
            }
            w[n] += vol;
        }
    }
    for (s, wn) in acc.iter_mut().zip(&w) {
        if *wn > 0.0 {
            for v in s.iter_mut() {
                *v /= wn;
            }
        }
    }
    acc
}
