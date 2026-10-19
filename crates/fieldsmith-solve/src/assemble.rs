//! P2 elasticity stiffness assembly.

use crate::bsr::Bsr;
use crate::{Material, P2Mesh};
use fenics_assembly::elasticity::ElasticMaterial;
use fenics_element::{tetrahedron_quadrature, AffineSimplexMap, FiniteElement, P2Tetrahedron};

/// Reference P2 gradients at the degree-2 quadrature points, with weights.
pub struct RefData {
    pub weights: Vec<f64>,
    /// `grads[q][a]` = ∇ξ φ_a at point q.
    pub grads: Vec<Vec<[f64; 3]>>,
}

impl RefData {
    pub fn new() -> RefData {
        let rule = tetrahedron_quadrature(2);
        let el = P2Tetrahedron;
        RefData {
            weights: rule.points.iter().map(|q| q.weight).collect(),
            grads: rule
                .points
                .iter()
                .map(|q| el.evaluate_gradients(&q.point))
                .collect(),
        }
    }
}

impl Default for RefData {
    fn default() -> Self {
        Self::new()
    }
}

/// Physical gradients of the 10 basis functions at reference point gradients `g`, plus |det J|.
pub fn element_map(mesh: &P2Mesh, e: usize) -> AffineSimplexMap<3> {
    let t = &mesh.tets[e];
    let v = [0, 1, 2, 3].map(|i| mesh.nodes[t[i] as usize]);
    AffineSimplexMap::from_tetrahedron_vertices(&v)
        .expect("degenerate tetrahedron in analysis mesh")
}

/// Element stiffness as 10×10 blocks of 3×3.
pub fn element_stiffness(
    mesh: &P2Mesh,
    e: usize,
    young: f64,
    poisson: f64,
    rd: &RefData,
) -> Box<[[f64; 9]; 100]> {
    let m = ElasticMaterial::new(young, poisson);
    let (lam, mu) = (m.lame_lambda(), m.lame_mu());
    let map = element_map(mesh, e);
    let det = map.det_jacobian.abs();
    let mut k = Box::new([[0.0f64; 9]; 100]);
    let mut g = [[0.0f64; 3]; 10];
    for (q, w) in rd.weights.iter().enumerate() {
        for (ga, grad) in g.iter_mut().zip(rd.grads[q].iter()) {
            *ga = map.transform_gradient(grad);
        }
        let s = w * det;
        for a in 0..10 {
            for b in 0..10 {
                let ga = g[a];
                let gb = g[b];
                let dot = ga[0] * gb[0] + ga[1] * gb[1] + ga[2] * gb[2];
                let blk = &mut k[a * 10 + b];
                for i in 0..3 {
                    for j in 0..3 {
                        let mut v = lam * ga[i] * gb[j] + mu * ga[j] * gb[i];
                        if i == j {
                            v += mu * dot;
                        }
                        blk[i * 3 + j] += s * v;
                    }
                }
            }
        }
    }
    k
}

/// Global stiffness matrix (no boundary conditions applied).
pub fn assemble(mesh: &P2Mesh, mat: &Material) -> Bsr {
    let rd = RefData::new();
    let mut a = Bsr::from_elements(mesh.nodes.len(), mesh.tets.iter().copied());
    for e in 0..mesh.tets.len() {
        // The cubic law's element stiffness when the material carries a triple for it
        // (the anisotropic lattice route); the isotropic kernel otherwise, unchanged.
        let ke = match mat.cubic.as_ref().and_then(|cs| cs[e].as_ref()) {
            Some(c) => crate::ortho::element_stiffness_cubic(mesh, e, c, &rd),
            None => element_stiffness(mesh, e, mat.young[e], mat.poisson, &rd),
        };
        let t = &mesh.tets[e];
        for i in 0..10 {
            let row = t[i] as usize;
            for j in 0..10 {
                a.add(row, t[j], &ke[i * 10 + j]);
            }
        }
    }
    a
}
