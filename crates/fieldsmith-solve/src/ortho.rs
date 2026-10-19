//! Cubic anisotropic elasticity in a fixed material frame (VISION §7 item 2, the
//! anisotropic homogenisation rung). The TPMS's own axes give the frame: the fill's
//! lattice argument is the world position, so the lattice's principal directions are the
//! mesh axes for every element — the [100]/[010]/[001] directions, with the [110]-45°
//! response V36 measured. A cubic material carries three constants, C11, C12, C44
//! (MPa), which degenerate to the isotropic pair exactly when C44 = (C11 − C12)/2.
//!
//! The parity with the isotropic kernel is the slice's gate: at that degeneracy the new
//! contraction reproduces the assembled `element_stiffness` and the stress recovery to
//! roundoff, so the isotropic law stays the no-constants-default unchanged.

use fenics_assembly::elasticity::ElasticMaterial;

/// Cubic stiffness constants (MPa) in the lattice's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicMaterial {
    pub c11: f64,
    pub c12: f64,
    pub c44: f64,
}

impl CubicMaterial {
    /// The isotropic limit's constants: C11 = λ + 2μ, C12 = λ, C44 = μ.
    pub fn isotropic(youngs_modulus: f64, poissons_ratio: f64) -> CubicMaterial {
        let m = ElasticMaterial::new(youngs_modulus, poissons_ratio);
        let (lam, mu) = (m.lame_lambda(), m.lame_mu());
        CubicMaterial {
            c11: lam + 2.0 * mu,
            c12: lam,
            c44: mu,
        }
    }

    /// Physical stability of a cubic material: C11 − C12 > 0, and all three constants
    /// positive (the Born criteria for the cubic class).
    pub fn is_stable(&self) -> bool {
        self.c11 - self.c12 > 0.0 && self.c11 > 0.0 && self.c12 > 0.0 && self.c44 > 0.0
    }
}

/// Voigt 6×6 stiffness of the cubic class in the lattice frame (row-major). Voigt
/// ordering (ε11, ε22, ε33, 2ε12, 2ε23, 2ε13) — the engineering shear strain convention,
/// matching `stress::Stress`'s (σxx, σyy, σzz, σxy, σyz, σzx) stress ordering: the
/// contraction D·(ε11, ε22, ε33, γ12, γ23, γ13) recovers the six stress components.
pub fn d_voigt(m: &CubicMaterial) -> [[f64; 6]; 6] {
    let mut d = [[0.0f64; 6]; 6];
    // The 3×3 normal block: C11 on the diagonal, C12 off it.
    for (i, row) in d.iter_mut().enumerate().take(3) {
        for (j, v) in row.iter_mut().enumerate().take(3) {
            *v = if i == j { m.c11 } else { m.c12 };
        }
    }
    // Engineering shear strain (γ12 = 2ε12): σ12 = C44·γ12 = 2·C44·ε12, so the shear
    // diagonal carries C44 itself (not 2·C44 — the doubling is in the strain vector).
    d[3][3] = m.c44;
    d[4][4] = m.c44;
    d[5][5] = m.c44;
    d
}

/// Element stiffness as 10×10 blocks of 3×3 for a CUBIC material in the mesh's own
/// frame — same shape, same quadrature and sign conventions as
/// `crate::assemble::element_stiffness`; the contraction is the Voigt form:
/// K_{aα,bβ} += w·|J| · e6(a, α)ᵀ · D · e6(b, β), where e6(x, α) is the symmetric
/// strain 6-vector of basis-function x's α-component (ε11, ε33..., γ12 = 2ε12).
pub fn element_stiffness_cubic(
    mesh: &crate::P2Mesh,
    e: usize,
    m: &CubicMaterial,
    rd: &crate::assemble::RefData,
) -> Box<[[f64; 9]; 100]> {
    let map = crate::assemble::element_map(mesh, e);
    let det = map.det_jacobian.abs();
    let d = d_voigt(m);
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
                let blk = &mut k[a * 10 + b];
                // The symmetric strains of this basis-function pair per component pair.
                // Strain 6-vectors for each basis's three components, precomputed once.
                let ea = strain6(ga);
                let eb = strain6(gb);
                for alpha in 0..3 {
                    let ca = ea[alpha];
                    for beta in 0..3 {
                        let cb = eb[beta];
                        let mut v = 0.0;
                        for i in 0..6 {
                            for j in 0..6 {
                                v += ca[i] * d[i][j] * cb[j];
                            }
                        }
                        blk[alpha * 3 + beta] += s * v;
                    }
                }
            }
        }
    }
    k
}

/// The symmetric strain 6-vector (ε11, ε22, ε33, γ12, γ23, γ13) of each displacement
/// component of a basis function with physical gradient `g` — row α is the strain
/// induced by u_α = φ_a.
fn strain6(g: [f64; 3]) -> [[f64; 6]; 3] {
    [
        [g[0], 0.0, 0.0, g[1], 0.0, g[2]],
        [0.0, g[1], 0.0, g[0], g[2], 0.0],
        [0.0, 0.0, g[2], 0.0, g[1], g[0]],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d_voigt_degenerates_to_the_isotropic_stiffness() {
        // C44 = (C11 − C12)/2 ⇒ the cubic D equals the isotropic (λ, μ) D exactly.
        for &(e, nu) in &[(200000.0f64, 0.3), (1500.0, 0.38), (70000.0, 0.33)] {
            let m = ElasticMaterial::new(e, nu);
            let (lam, mu) = (m.lame_lambda(), m.lame_mu());
            let c = CubicMaterial {
                c11: lam + 2.0 * mu,
                c12: lam,
                c44: (lam + 2.0 * mu - lam) / 2.0,
            };
            assert!(
                c.is_stable(),
                "the isotropic limit is a stable cubic ({e}, {nu})"
            );
            let d = d_voigt(&c);
            // The isotropic Voigt D: diagonal (λ+2μ, λ, λ, μ, μ, μ), λ elsewhere in the
            // 3×3 normal block. Equalities are to roundoff: (λ+2μ) − λ ≠ 2μ at the last
            // bit through the subtraction path.
            let eq = |a: f64, b: f64| assert!((a - b).abs() <= 1e-12 * (b.abs() + 1.0));
            eq(d[0][0], lam + 2.0 * mu);
            eq(d[0][1], lam);
            eq(d[0][2], lam);
            eq(d[1][0], lam);
            eq(d[1][1], lam + 2.0 * mu);
            eq(d[1][2], lam);
            eq(d[2][0], lam);
            eq(d[2][1], lam);
            eq(d[2][2], lam + 2.0 * mu);
            eq(d[3][3], mu);
            eq(d[4][4], mu);
            eq(d[5][5], mu);
            for (i, j) in [
                (0usize, 3usize),
                (0, 4),
                (0, 5),
                (1, 3),
                (1, 4),
                (1, 5),
                (2, 3),
                (2, 4),
                (2, 5),
                (3, 4),
                (3, 5),
                (4, 5),
            ] {
                eq(d[i][j], 0.0);
            }
        }
    }

    fn unit_tet() -> crate::P2Mesh {
        let v = [
            [0.0, 0.0, 0.0],
            [2.0, 0.1, 0.0],
            [0.0, 1.7, 0.2],
            [0.1, 0.0, 2.3],
        ];
        crate::P2Mesh::from_linear(&v, &[[0, 1, 2, 3]])
    }

    fn cube_mesh() -> crate::P2Mesh {
        let v = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let tets: [[u32; 4]; 6] = [
            [0, 1, 4, 7],
            [0, 5, 1, 7],
            [0, 4, 2, 7],
            [0, 2, 6, 7],
            [0, 3, 5, 7],
            [0, 6, 3, 7],
        ];
        crate::P2Mesh::from_linear(&v, &tets)
    }

    /// Relative difference of the two elements' stiffnesses, normalized to the overall
    /// matrix scale (per-entry normalization explodes on near-zero blocks; the parity
    /// claim is about the operator, so the scale is the right denominator).
    fn rel_gap(a: &[[f64; 9]; 100], b: &[[f64; 9]; 100]) -> f64 {
        let scale = a
            .iter()
            .flatten()
            .map(|v| v.abs().max(1e-300))
            .fold(0.0f64, f64::max);
        let gap = a
            .iter()
            .flatten()
            .zip(b.iter().flatten())
            .map(|(&va, &vb)| (va - vb).abs())
            .fold(0.0f64, f64::max);
        gap / scale
    }

    #[test]
    fn the_cubic_element_stiffness_parity_matches_the_isotropic_kernel() {
        // The same (λ, μ) pair through both contractions: the element matrices agree to
        // roundoff (different summation order, not a different operator).
        let rd = crate::assemble::RefData::new();
        for mesh in [unit_tet(), cube_mesh()] {
            for &(e_m, nu) in &[(200000.0f64, 0.3), (1500.0, 0.38)] {
                let c = CubicMaterial::isotropic(e_m, nu);
                let a_cubic = element_stiffness_cubic(&mesh, 0, &c, &rd);
                let a_iso = crate::assemble::element_stiffness(&mesh, 0, e_m, nu, &rd);
                let gap = rel_gap(&a_cubic, &a_iso);
                assert!(gap < 1e-12, "parity gap {gap:.3e} at ({e_m}, {nu})");
            }
        }
    }
}
