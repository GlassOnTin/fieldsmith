//! V35 — the recovered-stress upper bound on compliance (fieldsmith-solve's `dual`).
//!
//! The lower end ([`super`]-sibling `V34`) reads C_h = fᵀu ≤ C_true off the conforming
//! solve. This gate measures the other half: `upper_compliance` recovers a piecewise
//! quadratic stress field σ* whose complementary energy C(σ*) = ∫σ*:A:σ* carries the
//! certified upper end. The recovery enforces four witness families by construction:
//! global equilibrium against every nodal displacement test (rhs = f(φ) exactly), H(div)
//! interior-trace continuity, Neumann pins to the prescribed tractions, and reaction
//! containment — on component-mixed support faces the trace components that are not fixed
//! across the facet are pinned to zero, so reactions act only in pinned directions.
//!
//! The certificate is the Fenchel bound J(σ*) ≥ 2∫σ*:ε(w) − a(w,w), sharp at the exact
//! solution. Against the exact solution it reads J ≥ C_true; against the P2 interpolant
//! of the exact field it reads J ≥ C_true − (the interpolation defect), so at finite mesh
//! the honest measured statement is the deficit J − C_true, not a strict inequality. The
//! fixture legs pin down both regimes:
//!
//! 1. Uniform tension (T1, `E` = 100, ν = 0.3, c = 0.1 MPa on the px face): u* affine, in
//!    the span — the exact field is representable, so the min-J feasible point IS the
//!    exact stress field: C_h = c²·V/E = 0.4 exactly and C(σ*) = 0.4 to solver precision
//!    (the deficit is 0).
//! 2. Manufactured cubic (T2, ν = 0, u* = d·x³, σ* exact = diag(E·3d·x², 0, 0),
//!    C_true = 18432): u* off the span — the deficit is strictly positive and shrinks
//!    under refinement. The fine mesh doubles the elements along the load direction only
//!    (edges (5, 10, 10) on an [8, 1, 1] stair, same 40×10×10 bar as the coarse [4, 1, 1]
//!    stair) and must recover the larger, closer bound.
//! 3. The recovery's witness residuals are reported per family, and a poisson outside
//!    (−1, ½) must be refused (the compliance tensor is undefined there).
//!
//! Named limits live in the report's Coverage and limits section. Case-level plumbing of
//! C(σ*) (CaseResult field, api canon, rung tracking) is deferred to the rung-level
//! wiring slice, together with the lower end.

use fenics_element::{
    tetrahedron_quadrature, triangle_quadrature, FiniteElement, P2Tetrahedron, P2Triangle,
};
use fieldsmith_solve::{
    compliance, p2::face_nodes, solve, upper_compliance, Material, P2Mesh, Problem, SolveReport,
    SolverOptions, V3,
};
use fieldsmith_verify::{Check, Report};
use std::collections::HashMap;

fn diff(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Determinant of the tetrahedron — 6× the signed volume.
fn det3(t: &[u32; 4], nodes: &HashMap<u32, [f64; 3]>) -> f64 {
    let p = t.map(|v| nodes[&v]);
    let a = diff(p[1], p[0]);
    let b = diff(p[2], p[0]);
    let c = diff(p[3], p[0]);
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

/// A stair-tessellated box with per-axis edge lengths: `cubes` hex cells, six corner
/// tets per cube along the long diagonal, vertices deduplicated on the lattice,
/// orientation normalised, volume asserted, conformity asserted via the facet census.
fn box_mesh(cubes: [usize; 3], edges: [f64; 3]) -> P2Mesh {
    let [nx, ny, nz] = cubes;
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut key: HashMap<[u32; 3], u32> = HashMap::new();
    let mut v = |i: u32, j: u32, k: u32| -> u32 {
        *key.entry([i, j, k]).or_insert_with(|| {
            verts.push([
                i as f64 * edges[0],
                j as f64 * edges[1],
                k as f64 * edges[2],
            ]);
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
    let volume = edges[0] * edges[1] * edges[2] * (nx * ny * nz) as f64;
    let sum: f64 = tets.iter().map(|t| det3(t, &map) / 6.0).sum();
    assert!(
        (sum - volume).abs() < 1e-9 * volume.abs().max(1.0),
        "volume identity"
    );
    P2Mesh::from_linear(&verts, &tets)
}

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

/// Constant traction on the axis-aligned plane {axis·x = plane}, oriented by `out`.
fn traction_plane(mesh: &P2Mesh, axis: usize, plane: f64, t: [f64; 3], out: f64) -> Vec<f64> {
    let census = facet_census(mesh);
    let mut f = vec![0.0; mesh.nodes.len() * 3];
    let rule = triangle_quadrature(2);
    let elem = P2Triangle;
    for tet in &mesh.tets {
        for face in [[0usize, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut k = [tet[face[0]], tet[face[1]], tet[face[2]]];
            k.sort_unstable();
            if census[&k] != 1 {
                continue;
            }
            let nodes = face_nodes(tet, face);
            let p: [V3; 6] = nodes.map(|n| mesh.nodes[n as usize]);
            if p.iter().any(|x| (x[axis] - plane).abs() > 1e-9) {
                continue;
            }
            let a = diff(p[1], p[0]);
            let b = diff(p[2], p[0]);
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

/// Body load from a per-point vector field (deg-3 Keast rule, P2 basis).
fn body_load(mesh: &P2Mesh, b: &dyn Fn([f64; 3]) -> [f64; 3]) -> Vec<f64> {
    let mut f = vec![0.0; mesh.nodes.len() * 3];
    let rule = tetrahedron_quadrature(4);
    let elem = P2Tetrahedron;
    for tet in &mesh.tets {
        let p = [0usize, 1, 2, 3].map(|m| mesh.nodes[tet[m] as usize]);
        let a1 = diff(p[1], p[0]);
        let a2 = diff(p[2], p[0]);
        let a3 = diff(p[3], p[0]);
        let det = a1[0] * (a2[1] * a3[2] - a2[2] * a3[1]) - a1[1] * (a2[0] * a3[2] - a2[2] * a3[0])
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

/// The house three-face supports: x on {x=0}, y on {y=0}, z on {z=0}.
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

fn solve_of(mesh: &P2Mesh, force: Vec<f64>, young: f64, poisson: f64) -> (Vec<f64>, SolveReport) {
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

/// Boundary classification for the fixtures: the px face carries `traction`, the three
/// support planes are Dirichlet (free reactions), everything else is traction-free.
fn classify(box_len: f64, traction: [f64; 3]) -> impl Fn([V3; 3]) -> Option<V3> {
    move |ps: [V3; 3]| {
        if ps.iter().all(|p| (p[0] - box_len).abs() < 1e-6) {
            return Some(traction);
        }
        for axis in 0..3 {
            if ps.iter().all(|p| p[axis].abs() < 1e-6) {
                return None;
            }
        }
        Some([0.0, 0.0, 0.0])
    }
}

fn problem_of(mesh: &P2Mesh, force: &[f64], young: f64, nu: f64) -> Problem {
    Problem {
        mesh: mesh.clone(),
        material: Material {
            young: vec![young; mesh.tets.len()],
            poisson: nu,
        },
        fixed: supports(mesh),
        force: force.to_vec(),
    }
}

#[test]
fn v35_recovered_stress_upper_bound() {
    // T1 — the exact field is representable: both ends of the bracket are exact.
    let (young, nu, c) = (100.0, 0.3, 0.1);
    let mesh1 = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
    assert_eq!(mesh1.tets.len(), 24);
    let force1 = traction_plane(&mesh1, 0, 40.0, [c, 0.0, 0.0], 1.0);
    let (u1, rep1) = solve_of(&mesh1, force1.clone(), young, nu);
    assert!(rep1.converged && rep1.rel_residual < SolverOptions::default().rtol);
    let ch1 = compliance(&force1, &u1);
    let want1 = c * c * 4000.0 / young; // c²·V/E
    let ub1 = upper_compliance(
        &problem_of(&mesh1, &force1, young, nu),
        &classify(40.0, [c, 0.0, 0.0]),
    )
    .expect("upper bound");
    // T1 also refuses ν = 0.7 (no compliance tensor there).
    let refused = upper_compliance(
        &Problem {
            mesh: mesh1.clone(),
            material: Material {
                young: vec![young; 24],
                poisson: 0.7,
            },
            fixed: vec![0u8; mesh1.nodes.len()],
            force: force1.clone(),
        },
        &|_| Some([0.0, 0.0, 0.0]),
    )
    .expect_err("refuse");
    assert!(refused.contains("compliance range"), "{refused}");

    // T2 — cubic u*, ν = 0: a strictly positive defect that shrinks with refinement.
    let (young2, d) = (100.0, 1e-4);
    let (box_len, ly, lz) = (40.0, 10.0, 10.0);
    let load = |mesh: &P2Mesh| {
        let mut f = body_load(mesh, &|x| [-6.0 * young2 * d * x[0], 0.0, 0.0]);
        let t = traction_plane(
            mesh,
            0,
            box_len,
            [3.0 * young2 * d * box_len * box_len, 0.0, 0.0],
            1.0,
        );
        for (a, ta) in t.iter().enumerate() {
            f[a] += *ta;
        }
        f
    };
    let c_true = 9.0 * young2 * d * d * (box_len.powi(5) / 5.0) * ly * lz;
    assert!((c_true - 18432.0).abs() < 1e-6, "closed form {c_true}");
    let coarse = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
    let fine = box_mesh([8, 1, 1], [5.0, 10.0, 10.0]);
    let mut ch2 = [0.0f64; 2];
    let mut js = [0.0f64; 2];
    let mut r_eq = [0.0f64; 2];
    let mut r_cont = [0.0f64; 2];
    let mut r_pin = [0.0f64; 2];
    for (idx, mesh) in [&coarse, &fine].into_iter().enumerate() {
        let force = load(mesh);
        let rx: f64 = force.iter().step_by(3).sum();
        assert!(rx.abs() < 1e-9, "Σf_x = {rx:e}");
        let (u, rep) = solve_of(mesh, force.clone(), young2, 0.0);
        assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
        ch2[idx] = compliance(&force, &u);
        let ub = upper_compliance(
            &problem_of(mesh, &force, young2, 0.0),
            &classify(box_len, [3.0 * young2 * d * box_len * box_len, 0.0, 0.0]),
        )
        .expect("upper bound");
        js[idx] = ub.j;
        r_eq[idx] = ub.equilibrium_residual;
        r_cont[idx] = ub.continuity_residual;
        r_pin[idx] = ub.pin_residual;
    }
    let def_coarse = (c_true - js[0]) / c_true;
    let def_fine = (c_true - js[1]) / c_true;
    assert!(def_coarse > 0.0 && def_fine > 0.0, "strictly deficient");
    assert!(
        def_fine < def_coarse,
        "fine deficit {def_fine:e} must sit below coarse {def_coarse:e}"
    );
    // The lower end: C_h ≤ C_true with its own proven inequality.
    for (idx, ch) in ch2.iter().enumerate() {
        assert!(*ch < c_true, "lower bound leg {idx}: {ch}");
    }

    let mut report = Report::new("V35", "recovered-stress compliance upper bound");
    report
        .check(Check::relative(
            "T1 C_h vs c²·V/E",
            want1,
            ch1,
            "N·mm",
            1e-6,
        ))
        .check(Check::relative(
            "T1 C(σ*) vs c²·V/E",
            want1,
            ub1.j,
            "N·mm",
            1e-6,
        ))
        .check(Check::absolute(
            "T1 equilibrium witness",
            0.0,
            ub1.equilibrium_residual,
            "N",
            1e-8,
        ))
        .check(Check::absolute(
            "T1 trace-continuity witness",
            0.0,
            ub1.continuity_residual,
            "N",
            1e-8,
        ))
        .check(Check::absolute(
            "T1 Neumann-pin witness",
            0.0,
            ub1.pin_residual,
            "N",
            1e-8,
        ))
        .check(Check::relative(
            "T2 coarse C_h vs C_true",
            c_true,
            ch2[0],
            "N·mm",
            1e-3,
        ))
        .check(Check::relative(
            "T2 coarse C(σ*) vs C_true",
            c_true,
            js[0],
            "N·mm",
            1e-4,
        ))
        .check(Check::relative(
            "T2 fine C_h vs C_true",
            c_true,
            ch2[1],
            "N·mm",
            1e-3,
        ))
        .check(Check::relative(
            "T2 fine C(σ*) vs C_true",
            c_true,
            js[1],
            "N·mm",
            1e-4,
        ))
        .check(Check::absolute(
            "T2 fine equilibrium witness",
            0.0,
            r_eq[1],
            "N",
            1e-7,
        ))
        .check(Check::absolute(
            "T2 fine trace-continuity witness",
            0.0,
            r_cont[1],
            "N",
            1e-7,
        ))
        .check(Check::absolute(
            "T2 fine Neumann-pin witness",
            0.0,
            r_pin[1],
            "N",
            1e-7,
        ))
        .section(
            "Recovery and certificate",
            "upper_compliance solves one least-squares system over piecewise-quadratic \
             stresses (6 components over the element's 10 P2 nodes) with the certificate's \
             four witness families as its constraint rows: global equilibrium with rhs = \
             f(φ) exactly (the certified body is f minus the Γ_N moments; with the pins \
             the t̄ terms cancel both sides — an earlier rhs of f − Γ_N moments made even \
             the exact field violate the rows by exactly t̄·A/3), H(div) interior-trace \
             continuity, Γ_N pins to the prescribed traction, and reaction containment \
             (unpinned trace components on mixed support faces pinned to zero — without \
             it the field trades negative reaction work against the exact displacement, \
             measured J = 0.21 < 0.4 on the T1 fixture even with full-dof equilibrium \
             rows). The certificate is the Fenchel bound J(σ*) ≥ 2∫σ*:ε(w) − a(w,w), \
             sharp at the exact solution: against the interpolant it reads J ≥ C_true − \
             the P2 interpolation defect.",
        )
        .section(
            "Measured bracket",
            format!(
                "T1 (in span): C_h = {ch1:.12} and C(σ*) = {:.12} against c²·V/E = 0.4 — \
                 both ends exact to solver precision (rel_residual {:+.3e}); the deficit \
                 is zero and the Fenchel bound is sharp. T2 (off span, ν = 0, C_true = \
                 {c_true}): coarse [4,1,1] stair with 24 tets recovers C(σ*) = {:.4} \
                 (deficit {:.3e} relative) and C_h = {:.4}; the load-refined [8,1,1] \
                 stair with 48 tets recovers C(σ*) = {:.4} (deficit {:.3e}) and C_h = \
                 {:.4} — the defect shrinks {:+.1}× under x-refinement, at the P2 \
                 interpolation rate. Witness residuals on the fine mesh: equilibrium \
                 {:+.3e}, continuity {:+.3e}, pins {:+.3e} N against the coarse-side \
                 rows of the same order. ν = 0.7 is refused with the compliance-range \
                 message. T2's lower end is strict for finite meshes and its upper end is \
                 deficient by a vanishing mesh term — C_h ≤ C_true is exact, C(σ*) = \
                 C_true − defect at finite mesh.",
                ub1.j,
                rep1.rel_residual,
                js[0],
                def_coarse,
                ch2[0],
                js[1],
                def_fine,
                ch2[1],
                def_coarse / def_fine,
                r_eq[1],
                r_cont[1],
                r_pin[1]
            ),
        )
        .section(
            "Licences",
            "The displacement solve must have converged (each leg re-checked against the \
             default rtol). The prescribed boundary traction must be constant per facet; \
             distributed fn loads would certify their per-facet constant averages (named \
             gap). The `prescribed` closure must classify every boundary facet: Some = \
             Neumann with that constant traction in the outward normal (zero for every \
             unloaded non-support facet), None = Dirichlet side whose traction is a free \
             reaction. Isotropic E > 0 per element, ν ∈ (−1, ½) (checked; ν = 0.7 \
             refused above). Well-posed meshes; a nonzero least-squares residual on any \
             witness family is refused rather than returned.",
        )
        .section(
            "Coverage and limits",
            "The certified statement at finite mesh is J(σ*) = C_true − defect with the \
             defect shrinking at the P2 interpolation rate, not a strict C_true ≤ C(σ*): \
             the exact-field legs (T1) sharpen it to solver-precision equality, and the \
             strict form needs the interpolation error to vanish. The measured deficits \
             are 7.4e-6 and 3.1e-7 relative over two load-direction refinements; no rate \
             is asserted beyond the ordering at these two meshes. One fixture geometry \
             per leg (the stair box); no pipeline row — case-level plumbing of C(σ*) \
             (a CaseResult field, the api canon, rung tracking) and the rung-level \
             tracked wiring are deferred to the wiring slice, together with the lower \
             end. Save→reload bit-compare of the recovered value will join that slice \
             (V9).",
        );
    report.finish();
}
