//! Stress recovery.

use crate::assemble::element_map;
use crate::P2Mesh;
use fenics_assembly::elasticity::ElasticMaterial;
use fenics_element::{FiniteElement, P2Tetrahedron};

/// Symmetric stress tensor (σxx, σyy, σzz, σxy, σyz, σzx), MPa.
pub type Stress = [f64; 6];

pub fn von_mises(s: &Stress) -> f64 {
    let [xx, yy, zz, xy, yz, zx] = *s;
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2))
        + 3.0 * (xy * xy + yz * yz + zx * zx))
        .sqrt()
}

/// The principal stresses of a symmetric 3×3, sorted ascending. Closed form: the shifted
/// tensor's invariants through the trigonometric eigen solution (the cubic's angles — the
/// standard stable evaluation of a symmetric 3×3's spectrum).
pub fn principal_stresses(s: &Stress) -> [f64; 3] {
    let [xx, yy, zz, xy, yz, zx] = *s;
    let trace = xx + yy + zz;
    let b = [xx - trace / 3.0, yy - trace / 3.0, zz - trace / 3.0];
    // tr(B·B) and det(B) of the shifted symmetric matrix entries.
    let t2 = b[0] * b[0] + b[1] * b[1] + b[2] * b[2] + 2.0 * (xy * xy + yz * yz + zx * zx);
    // det of the symmetric [[b0, xy, zx], [xy, b1, yz], [zx, yz, b2]].
    let det =
        b[0] * b[1] * b[2] + 2.0 * xy * yz * zx - b[0] * yz * yz - b[1] * zx * zx - b[2] * xy * xy;
    if t2 == 0.0 {
        return [trace / 3.0; 3];
    }
    let p = t2 / 6.0;
    let q = det / 2.0;
    let inner = q / (p * p.sqrt());
    let ang = inner.clamp(-1.0, 1.0).acos() / 3.0;
    let amp = 2.0 * p.sqrt();
    let mut out = [
        trace / 3.0 + amp * (ang + 4.0 * std::f64::consts::PI / 3.0).cos(),
        trace / 3.0 + amp * (ang).cos(),
        trace / 3.0 + amp * (ang + 2.0 * std::f64::consts::PI / 3.0).cos(),
    ];
    out.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The most compressive principal stress (the smallest eigenvalue; 0 when none is
/// compressive) — the load measure a buckling indicator reads.
pub fn min_compressive(s: &Stress) -> f64 {
    principal_stresses(s)[0].min(0.0)
}

/// Stress in element `e` at reference point `xi`.
pub fn element_stress(
    mesh: &P2Mesh,
    u: &[f64],
    e: usize,
    young: f64,
    poisson: f64,
    xi: [f64; 3],
) -> Stress {
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
    assert_eq!(young.len(), mesh.tets.len(), "one modulus per element");
    for (e, y) in young.iter().enumerate() {
        let vol = mesh.tet_volume(e).abs();
        for (a, xi) in pts.iter().enumerate() {
            let s = element_stress(mesh, u, e, *y, poisson, *xi);
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

// -- thermal variants (M14) ------------------------------------------------

/// Stress in element `e` at reference point `xi` with the thermal term of the one-way
/// coupled solve: σ = C:B(u) − (3λ+2μ)·α·ΔT(xi)·I. `dt` is ΔT = T − t_ref at `xi`
/// (the caller evaluates it — at a P2 node it is that node's ΔT exactly, by the basis's
/// Kronecker property). Shear components are untouched: the thermal strain is isotropic.
#[allow(clippy::too_many_arguments)]
pub fn element_stress_thermal(
    mesh: &P2Mesh,
    u: &[f64],
    e: usize,
    young: f64,
    poisson: f64,
    alpha: f64,
    dt: f64,
    xi: [f64; 3],
) -> Stress {
    let m = ElasticMaterial::new(young, poisson);
    let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha;
    let mut s = element_stress(mesh, u, e, young, poisson, xi);
    let th = beta * dt;
    s[0] -= th;
    s[1] -= th;
    s[2] -= th;
    s
}

/// [`nodal_stress`] with the thermal term per [`element_stress_thermal`], nodal ΔT in hand
/// (at a node the interpolated field reproduces the nodal ΔT exactly).
pub fn nodal_stress_thermal(
    mesh: &P2Mesh,
    u: &[f64],
    young: &[f64],
    poisson: f64,
    alpha: f64,
    dt: &[f64],
) -> Vec<Stress> {
    let pts = P2Tetrahedron.nodal_coordinates();
    let mut acc = vec![[0.0f64; 6]; mesh.nodes.len()];
    let mut w = vec![0.0f64; mesh.nodes.len()];
    assert_eq!(young.len(), mesh.tets.len(), "one modulus per element");
    assert_eq!(dt.len(), mesh.nodes.len(), "one ΔT per node");
    for (e, y) in young.iter().enumerate() {
        let vol = mesh.tet_volume(e).abs();
        let t = &mesh.tets[e];
        for (a, xi) in pts.iter().enumerate() {
            let s = element_stress_thermal(mesh, u, e, *y, poisson, alpha, dt[t[a] as usize], *xi);
            let n = t[a] as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Newton's identities pin the multiset: sum = trace, sum of squares = tr(A²),
    /// product = det. Together they fix a 3×3's spectrum.
    fn assert_spectrum_ok(s: &Stress, want: [f64; 3], tol: f64) {
        let got = principal_stresses(s);
        let [xx, yy, zz, xy, yz, zx] = *s;
        let trace = xx + yy + zz;
        let tr2 = xx * xx + yy * yy + zz * zz + 2.0 * (xy * xy + yz * yz + zx * zx);
        let det = xx * yy * zz + 2.0 * xy * yz * zx - zz * xy * xy - yy * zx * zx - xx * yz * yz;
        let sum2: f64 = got.iter().map(|x| x * x).sum();
        let product = got[0] * got[1] * got[2];
        assert!(
            (got[0] - want[0]).abs() < tol
                && (got[1] - want[1]).abs() < tol
                && (got[2] - want[2]).abs() < tol,
            "spectrum {got:?} vs want {want:?}"
        );
        assert!(
            (got.iter().sum::<f64>() - trace).abs() < tol,
            "trace identity {got:?}"
        );
        assert!((sum2 - tr2).abs() < tol, "sum-of-squares identity {got:?}");
        assert!((product - det).abs() < tol, "determinant identity {got:?}");
    }

    #[test]
    fn principal_reads_the_known_spectra() {
        // Uniaxial.
        assert_spectrum_ok(&[-100.0, 0.0, 0.0, 0.0, 0.0, 0.0], [-100.0, 0.0, 0.0], 1e-9);
        assert_spectrum_ok(&[50.0, 20.0, 10.0, 0.0, 0.0, 0.0], [10.0, 20.0, 50.0], 1e-9);
        // Pure shear σxy = 10.
        assert_spectrum_ok(&[0.0, 0.0, 0.0, 10.0, 0.0, 0.0], [-10.0, 0.0, 10.0], 1e-9);
        // Hydrostatic: the degenerate branch.
        assert_spectrum_ok(&[7.0, 7.0, 7.0, 0.0, 0.0, 0.0], [7.0, 7.0, 7.0], 1e-9);
        // Hydrostatic + shear.
        assert_spectrum_ok(
            &[7.0, 7.0, 7.0, 4.0, 0.0, 0.0],
            [7.0 - 4.0, 7.0, 7.0 + 4.0],
            1e-9,
        );
        // A full mixed matrix: the three identities FIX the multiset without a
        // reference eigensolve, which is the independent check itself.
        assert_spectrum_ok(
            &[100.0, 60.0, 20.0, 30.0, -5.0, 2.0],
            principal_stresses(&[100.0, 60.0, 20.0, 30.0, -5.0, 2.0]),
            1e-9,
        );
    }

    #[test]
    fn min_compressive_clamps_tension() {
        assert_eq!(min_compressive(&[100.0, 60.0, 20.0, 0.0, 0.0, 0.0]), 0.0);
        assert_eq!(min_compressive(&[-40.0, 10.0, 5.0, 0.0, 0.0, 0.0]), -40.0);
    }
}
