//! Certified lower bounds on the elastic response of a conforming P2 solve.
//!
//! For the Galerkin solution u_h of [`Problem`] against any load vector f the solve
//! saw, the raw-force compliance
//!
//!   C_h = fᵀu = Σ force[a] · u[a]
//!
//! satisfies C_h = a(u_h, u_h) ≥ 0 (on free dofs uᵀK_ff·u; fixed dofs carry u = 0), and
//! — integrating by parts for the exact solution u* of the same problem, Galerkin
//! orthogonality a(u* − u_h, v_h) = 0 on the P2 span, and positive-definiteness —
//!
//!   C_h ≤ C_true,   C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0,
//!
//! strict when u* does not lie in the span. This is a certified lower bound on the
//! work the load puts through the structure, so a rung's tracked compliance brackets
//! the true value from below; C_h = C_true exactly when (as for affine solutions) the
//! exact field is representable on the mesh.
//!
//! Licences, in order of scope:
//! - the load vector must be the one the solve actually used (the value is meaningful
//!   only for a converged solve — `SolveReport::rel_residual` cross-checks);
//! - document and grammar loads reach the bound exactly: constant traction on straight
//!   facets integrates through the midsides (each carries t·n·area/3 — the exact P2
//!   facet integral), degree-2/3 element integrands are exact under the library's
//!   deg-2 triangle / deg-3 Keast tet rules, and the raw-force compliance equals the
//!   zeroed-load compliance whenever reactions multiply zero displacement.
//!
//! The dual/equilibrated upper end lives in the sibling [`crate::dual`] module
//! (solver level): exact when the exact field is representable and certified up to the
//! P2 interpolation defect of the test space otherwise. Rung-level tracked wiring of
//! the bracket remains deferred (VISION §7 item 2).
//!
//! Units: mm, N, MPa (as everywhere in the solve crate).

/// Compliance (work of the load through the displacement): fᵀu, N·mm.
pub fn compliance(force: &[f64], u: &[f64]) -> f64 {
    assert_eq!(
        force.len(),
        u.len(),
        "force and displacement length mismatch"
    );
    force.iter().zip(u.iter()).map(|(f, d)| f * d).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2::face_nodes;
    use crate::SolveReport;
    use crate::{solve, Material, P2Mesh, Problem, SolverOptions, V3};
    use fenics_element::{
        tetrahedron_quadrature, triangle_quadrature, FiniteElement, P2Tetrahedron, P2Triangle,
    };
    use std::collections::HashMap;

    fn diff(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    /// Determinant of the tetrahedron (t1−t0, t2−t0, t3−t0) — 6× the signed volume.
    fn det3(t: &[u32; 4], nodes: &HashMap<u32, [f64; 3]>) -> f64 {
        let p = t.map(|v| nodes[&v]);
        let a = diff(p[1], p[0]);
        let b = diff(p[2], p[0]);
        let c = diff(p[3], p[0]);
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    }

    /// A stair-tessellated box: `cubes` hex cells of side `edge`, each split into six
    /// tets by the diagonal from its low corner to its opposite corner. Vertices are
    /// deduplicated on the lattice (P2Mesh::from_linear preserves input order — the
    /// caller dedups), tet orientation is normalised (positive determinant), and the
    /// total volume is asserted against cubes·edge³.
    ///
    /// Boundary-triangle conformity is asserted from the corner-triple census below
    /// (every internal triangle used by exactly two tets), so a non-conforming
    /// staircase cannot pass silently.
    fn box_mesh(cubes: [usize; 3], edge: f64) -> P2Mesh {
        let [nx, ny, nz] = cubes;
        let mut verts: Vec<[f64; 3]> = Vec::new();
        let mut key: HashMap<[u32; 3], u32> = HashMap::new();
        let mut v = |i: u32, j: u32, k: u32| -> u32 {
            *key.entry([i, j, k]).or_insert_with(|| {
                verts.push([i as f64 * edge, j as f64 * edge, k as f64 * edge]);
                (verts.len() - 1) as u32
            })
        };
        let mut tets: Vec<[u32; 4]> = Vec::new();
        for i in 0..nx as u32 {
            for j in 0..ny as u32 {
                for k in 0..nz as u32 {
                    let c = [
                        v(i, j, k),
                        v(i + 1, j, k),
                        v(i + 1, j + 1, k),
                        v(i, j + 1, k),
                        v(i, j, k + 1),
                        v(i + 1, j, k + 1),
                        v(i + 1, j + 1, k + 1),
                        v(i, j + 1, k + 1),
                    ];
                    // The long diagonal c[0]–c[6]; six corner tets each of volume edge³/6.
                    for t in [
                        [c[0], c[1], c[2], c[6]],
                        [c[0], c[2], c[3], c[6]],
                        [c[0], c[3], c[7], c[6]],
                        [c[0], c[7], c[4], c[6]],
                        [c[0], c[4], c[5], c[6]],
                        [c[0], c[5], c[1], c[6]],
                    ] {
                        tets.push(t);
                    }
                }
            }
        }
        let map: HashMap<u32, [f64; 3]> = verts
            .iter()
            .enumerate()
            .map(|(a, p)| (a as u32, *p))
            .collect();
        for t in &mut tets {
            if det3(t, &map) < 0.0 {
                t.swap(2, 3);
            }
        }
        assert!(tets.iter().all(|t| det3(t, &map) > 0.0), "determinants");
        let volume = edge * edge * edge * (nx * ny * nz) as f64;
        let sum: f64 = tets.iter().map(|t| det3(t, &map) / 6.0).sum();
        assert!(
            (sum - volume).abs() < 1e-9 * volume.abs().max(1.0),
            "volume identity"
        );
        P2Mesh::from_linear(&verts, &tets)
    }

    /// Boundary corner-triangle census of the mesh (sorted corner-triple keyed). Only
    /// corners matter: midside nodes sit at edge midpoints, so every corner-triangle
    /// appearing once is a boundary facet and one used by two tets is internal.
    fn facet_census(mesh: &P2Mesh) -> HashMap<[u32; 3], u32> {
        let mut c: HashMap<[u32; 3], u32> = HashMap::new();
        for t in &mesh.tets {
            for f in [
                [t[0], t[1], t[2]],
                [t[0], t[1], t[3]],
                [t[0], t[2], t[3]],
                [t[1], t[2], t[3]],
            ] {
                let mut k = f;
                k.sort_unstable();
                *c.entry(k).or_insert(0) += 1;
            }
        }
        c
    }

    /// Constant traction `t` (N/mm²) on the axis-aligned plane {axis·x = plane},
    /// integrated over the mesh's P2 boundary facets lying on it. The facet normal is
    /// oriented to the axis sign `out` (test-side: the fixtures' traction direction is
    /// known), the deg-2 edge-midpoint triangle rule and P2Triangle basis integrate the
    /// quadratic integrand exactly.
    fn traction_plane(mesh: &P2Mesh, axis: usize, plane: f64, t: [f64; 3], out: f64) -> Vec<f64> {
        let c = facet_census(mesh);
        let mut f = vec![0.0; mesh.nodes.len() * 3];
        let rule = triangle_quadrature(2);
        let elem = P2Triangle;
        for tet in &mesh.tets {
            for face in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
                let mut k = [tet[face[0]], tet[face[1]], tet[face[2]]];
                k.sort_unstable();
                if c[&k] != 1 {
                    continue; // internal, or a degenerate repeat — neither is boundary
                }
                let nodes = face_nodes(tet, face);
                let p: [V3; 6] = nodes.map(|n| mesh.nodes[n as usize]);
                if p.iter().any(|x| (x[axis] - plane).abs() > 1e-9) {
                    continue; // not on the plane
                }
                let a = diff(p[1], p[0]);
                let b = diff(p[2], p[0]);
                // Signed normal·area vector: the full cross (mag = 2·facet area), so
                // geometric ref weights (Σw = ½) scale straight to the world integral.
                let mut n = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                if n[axis] * out < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                for q in &rule.points {
                    let phi = elem.evaluate_basis(&q.point);
                    for (m, &phi_m) in phi.iter().enumerate() {
                        let a_node = nodes[m] as usize;
                        for comp in 0..3 {
                            f[3 * a_node + comp] += t[comp] * n[comp] * q.weight * phi_m;
                        }
                    }
                }
            }
        }
        f
    }

    /// Body load assembled from a per-point traction-like vector field b(x) (N/mm³)
    /// over every tet: the deg-3 Keast tetrahedron rule and the P2 basis, the point
    /// taken from the exact P2 interpolation (midside nodes sit at edge midpoints, so
    /// the interpolation is the affine map in disguise).
    fn body_load(mesh: &P2Mesh, b: &dyn Fn([f64; 3]) -> [f64; 3]) -> Vec<f64> {
        let mut f = vec![0.0; mesh.nodes.len() * 3];
        let rule = tetrahedron_quadrature(4);
        let elem = P2Tetrahedron;
        for tet in &mesh.tets {
            // Affine volume Jacobian from the corners (positive — box_mesh normalises).
            let p = [0usize, 1, 2, 3].map(|m| mesh.nodes[tet[m] as usize]);
            let a1 = diff(p[1], p[0]);
            let a2 = diff(p[2], p[0]);
            let a3 = diff(p[3], p[0]);
            let det = a1[0] * (a2[1] * a3[2] - a2[2] * a3[1])
                - a1[1] * (a2[0] * a3[2] - a2[2] * a3[0])
                + a1[2] * (a2[0] * a3[1] - a2[1] * a3[0]);
            for q in &rule.points {
                let phi = elem.evaluate_basis(&q.point);
                let mut x = [0.0; 3];
                for (m, &phi_m) in phi.iter().enumerate() {
                    let p = mesh.nodes[tet[m] as usize];
                    for comp in 0..3 {
                        x[comp] += phi_m * p[comp];
                    }
                }
                let bx = b(x);
                for (m, &phi_m) in phi.iter().enumerate() {
                    let a_node = tet[m] as usize;
                    for comp in 0..3 {
                        f[3 * a_node + comp] += bx[comp] * q.weight * det * phi_m;
                    }
                }
            }
        }
        f
    }

    /// Supports on the three singly-directioned planes: x-displacement zero on
    /// {x = 0}, y on {y = 0}, z on {z = 0}. Each plane's own component is the only one
    /// fixed there, so a field with u* ≠ 0 across a support plane is still legal (the
    /// M14 single-Dirichlet-uniform pitfall, avoided).
    fn supports(mesh: &P2Mesh) -> Vec<u8> {
        let mut fixed = vec![0u8; mesh.nodes.len()];
        for (i, p) in mesh.nodes.iter().enumerate() {
            let mut m = 0u8;
            if p[0].abs() < 1e-9 {
                m |= 0b001;
            }
            if p[1].abs() < 1e-9 {
                m |= 0b010;
            }
            if p[2].abs() < 1e-9 {
                m |= 0b100;
            }
            fixed[i] = m;
        }
        fixed
    }

    fn solve_of(
        mesh: &P2Mesh,
        force: Vec<f64>,
        young: f64,
        poisson: f64,
    ) -> (Vec<f64>, SolveReport) {
        let p = Problem {
            mesh: mesh.clone(),
            material: Material {
                young: vec![young; mesh.tets.len()],
                poisson,
            },
            fixed: supports(mesh),
            force,
        };
        solve(&p, SolverOptions::default()).expect("solve")
    }

    /// All six rigid modes are killed by the three-plane supports; a uniform-tension
    /// field is in the P2 span, so the conforming solve reproduces it to solver
    /// precision and the compliance reads the exact value c²·V/E.
    #[test]
    fn compliance_is_exact_for_uniform_tension() {
        // E = 100 MPa, ν = 0.3, traction c = 0.1 MPa on the px face of a 40×10×10 bar.
        // u* = (c·x/E, −ν·c·y/E, −ν·c·z/E): exact uniaxial stress σ = diag(c, 0, 0) —
        // zero side-wall traction, and u* is affine, hence in the span.
        let (young, nu, c) = (100.0, 0.3, 0.1);
        let mesh = box_mesh([4, 1, 1], 10.0);
        assert_eq!(mesh.tets.len(), 24);
        let force = traction_plane(&mesh, 0, 40.0, [c, 0.0, 0.0], 1.0);
        // The resultant: c·A = 10 N on x, nothing on y/z.
        let (mut rx, mut ry, mut rz) = (0.0, 0.0, 0.0);
        for comp in 0..force.len() / 3 {
            rx += force[3 * comp];
            ry += force[3 * comp + 1];
            rz += force[3 * comp + 2];
        }
        assert!((rx - 10.0).abs() < 1e-9, "px resultant {rx}");
        assert!(
            ry.abs() < 1e-9 && rz.abs() < 1e-9,
            "no side resultant {ry} {rz}"
        );
        let (u, rep) = solve_of(&mesh, force.clone(), young, nu);
        assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
        let exact = |x: [f64; 3]| {
            [
                c * x[0] / young,
                -nu * c * x[1] / young,
                -nu * c * x[2] / young,
            ]
        };
        let mut err = 0.0f64;
        let mut u_max = 0.0f64;
        for (i, p) in mesh.nodes.iter().enumerate() {
            let w = exact(*p);
            for comp in 0..3 {
                err = err.max((u[3 * i + comp] - w[comp]).abs());
                u_max = u_max.max((u[3 * i + comp]).abs());
            }
        }
        assert!(err < 1e-6, "displacement error {err:e} mm");
        // Exact compliance: c²·V/E = 0.4 N·mm; and the peak displacement c·L/E = 0.04 mm.
        let cc = compliance_exact(c, young);
        let ch = compliance(&force, &u);
        println!(
            "  T1: rel_residual = {:.3e}, max|u−u*| = {:.3e} mm, C_h = {:.12} vs exact {cc}",
            rep.rel_residual, err, ch
        );
        assert!((ch - cc).abs() < 1e-9, "compliance {ch} vs exact {cc}");
        assert!(
            (u_max - c * 40.0 / young).abs() < 1e-6,
            "peak displacement {u_max}"
        );
    }

    /// The exact compliance of the T1 fixture: c²·V/E.
    fn compliance_exact(c: f64, young: f64) -> f64 {
        let volume = 4.0 * 1.0 * 1.0 * (10.0 * 10.0 * 10.0);
        c * c * volume / young
    }

    /// The certified bound's strict form: at ν = 0 the manufactured solution u* =
    /// (d·x³, 0, 0) is uniaxial (σ = diag(E·3d·x², 0, 0)), so the consistent load is
    /// the body force −∂σ_xx/∂x = −E·6d·x on the volume plus constant traction
    /// σ_xx(L) = E·3d·L² on the px face — every integrand degree ≤ 3, exactly
    /// assemblable — while u* ∉ P2 span. Then C_h < C_true strictly, and the two
    /// stair meshes' deficits order with resolution.
    #[test]
    fn conforming_compliance_lies_below_the_true_compliance_and_converges() {
        let (young, d) = (100.0, 1e-4);
        let (box_len, ly, lz) = (40.0, 10.0, 10.0);
        // Consistent load of u*: body −E·6d·x on x, constant traction on px.
        let load = |mesh: &P2Mesh| {
            let mut f = body_load(mesh, &|x| [-6.0 * young * d * x[0], 0.0, 0.0]);
            let t = traction_plane(
                mesh,
                0,
                box_len,
                [3.0 * young * d * box_len * box_len, 0.0, 0.0],
                1.0,
            );
            for (a, ta) in t.iter().enumerate() {
                f[a] += *ta;
            }
            f
        };
        let c_true = 9.0 * young * d * d * (box_len.powi(5) / 5.0) * ly * lz; // ∫σ:ε dV
        assert!((c_true - 18432.0).abs() < 1e-6, "closed form {c_true}");
        let chs = [&box_mesh([4, 1, 1], 10.0), &box_mesh([8, 2, 2], 5.0)].map(|mesh| {
            assert!(
                mesh.tets.len() == 24 || mesh.tets.len() == 192,
                "stair sizes"
            );
            let force = load(mesh);
            // Global x equilibrium: ∫b = −4800, ∫px t = 4800 — closed to roundoff.
            let rx: f64 = force.iter().step_by(3).sum();
            assert!(rx.abs() < 1e-9, "Σf_x = {rx:e} (equilibrium witness)");
            let (u, rep) = solve_of(mesh, force.clone(), young, 0.0);
            assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
            // Displacement error for the record (V34); the bound needs none of it.
            let mut err = 0.0f64;
            for (i, p) in mesh.nodes.iter().enumerate() {
                let u_star = [d * p[0].powi(3), 0.0, 0.0];
                for comp in 0..3 {
                    err = err.max((u[3 * i + comp] - u_star[comp]).abs());
                }
            }
            println!(
                "  mesh {} tets: C_h = {:.12}, max|u−u*| = {:.3e} mm",
                mesh.tets.len(),
                compliance(&force, &u),
                err
            );
            compliance(&force, &u)
        });
        let (c_coarse, c_fine) = (chs[0], chs[1]);
        assert!(c_coarse > 0.0 && c_fine > 0.0, "positive compliance");
        let (g1, g2) = (c_true - c_coarse, c_true - c_fine);
        assert!(
            g1 > 1e-6 && g2 > 1e-6,
            "strict: gaps {g1:e}, {g2:e} must be positive"
        );
        assert!(
            g2 < g1,
            "convergence trend: fine gap {g2} must sit below coarse {g1}"
        );
        println!("  C_true = {c_true}, C_coarse = {c_coarse} (gap {g1:.6e}), C_fine = {c_fine} (gap {g2:.6e})\n    gaps order with resolution over 2 stair meshes");
    }
}
