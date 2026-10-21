//! V43 — the equilibrated upper end routes the cubic anisotropic law.
//!
//! The certified bracket's lower leg (the conforming solve) already read the cubic
//! per-element law; the upper leg refused it — the dual recovery's complementary-energy
//! Gram was written for the isotropic compliance tensor, and the core refused the
//! combination rather than certify through the wrong A. This gate puts the cubic
//! compliance tensor into the recovery and holds the bracket against the closed form:
//!
//! - a bar in uniaxial [100] tension inside a cubic material has the exact field
//!   σ = diag(c, 0, 0), ε_xx = S₁₁·c, ε_yy = ε_zz = S₁₂·c, which satisfies the house
//!   three-face supports exactly — so C_true = c²·S₁₁·V and both bracket ends are exact
//!   to solver precision (the same in-span fixture V35's T1 uses, anisotropic);
//! - the isotropic limit (C₄₄ = (C₁₁ − C₁₂)/2) reproduces the isotropic route's
//!   recovered bound to roundoff — the no-constants-default path stays unchanged;
//! - an unstable cubic triple (a negative shear constant) is refused by name;
//! - a mixed element set (one cubic element among isotropic ones) still brackets;
//! - the sparse recovery route leaves no debug artefact behind (the direct sparse
//!   solve shipped with a dump of the whole S matrix to /tmp on every call).

use fieldsmith_solve::ortho::CubicMaterial;
use fieldsmith_solve::{
    compliance, solve, upper_compliance, Material, P2Mesh, Problem, SolverOptions,
};
use fieldsmith_verify::{Check, Report};
use fenics_element::FiniteElement;
use std::collections::HashMap;

fn diff(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn det3(t: &[u32; 4], nodes: &HashMap<u32, [f64; 3]>) -> f64 {
    let p = t.map(|v| nodes[&v]);
    let a = diff(p[1], p[0]);
    let b = diff(p[2], p[0]);
    let c = diff(p[3], p[0]);
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

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

fn traction_plane(mesh: &P2Mesh, axis: usize, plane: f64, t: [f64; 3], out: f64) -> Vec<f64> {
    let census = facet_census(mesh);
    let mut f = vec![0.0; mesh.nodes.len() * 3];
    let rule = fenics_element::triangle_quadrature(2);
    let elem = fenics_element::P2Triangle;
    for tet in &mesh.tets {
        for face in [[0usize, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut k = [tet[face[0]], tet[face[1]], tet[face[2]]];
            k.sort_unstable();
            if census[&k] != 1 {
                continue;
            }
            let nodes = fieldsmith_solve::p2::face_nodes(tet, face);
            let p: [[f64; 3]; 6] = nodes.map(|n| mesh.nodes[n as usize]);
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
                for (mmm, &phi_m) in phi.iter().enumerate() {
                    let a_node = nodes[mmm] as usize;
                    for comp in 0..3 {
                        f[3 * a_node + comp] += t[comp] * n[comp] * q.weight * phi_m;
                    }
                }
            }
        }
    }
    f
}

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

fn classify(box_len: f64, traction: [f64; 3]) -> impl Fn([[f64; 3]; 3]) -> Option<[f64; 3]> {
    move |ps: [[f64; 3]; 3]| {
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

/// Closed-form cubic compliance entries from the stiffness constants.
fn compliance_entries(c: &CubicMaterial) -> (f64, f64, f64) {
    let d = (c.c11 - c.c12) * (c.c11 + 2.0 * c.c12);
    ((c.c11 + c.c12) / d, -c.c12 / d, 1.0 / c.c44)
}

#[test]
fn v43_cubic_upper_bound() {
    let dump = "/tmp/smat.txt";
    let _ = std::fs::remove_file(dump);

    // A genuinely anisotropic cubic triple (Zener 0.6): the [100] response is what the
    // bar's uniaxial tension reads, and it differs from every isotropic reading.
    let cubic = CubicMaterial {
        c11: 200.0,
        c12: 100.0,
        c44: 30.0,
        rot: None,
    };
    assert!(cubic.is_stable());
    let (s11, _s12, _s44) = compliance_entries(&cubic);
    let (young, nu, c) = (100.0, 0.3, 0.1);
    let box_len = 40.0;
    let mesh = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
    let ntets = mesh.tets.len();
    let volume = 4000.0;
    let force = traction_plane(&mesh, 0, box_len, [c, 0.0, 0.0], 1.0);

    // The routed solve: every element carries the cubic triple.
    let problem = Problem {
        mesh: mesh.clone(),
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: Some(vec![Some(cubic); ntets]),
        },
        fixed: supports(&mesh),
        force: force.clone(),
    };
    let (u, rep) = solve(&problem, SolverOptions::default()).expect("cubic solve");
    assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
    let ch = compliance(&force, &u);
    let c_true = c * c * s11 * volume;

    // The upper end on the same problem.
    let ub = upper_compliance(&problem, &classify(box_len, [c, 0.0, 0.0]))
        .expect("cubic upper bound");

    // The isotropic route on the same fixture, for the limit's roundoff agreement.
    let iso_problem = Problem {
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: None,
        },
        ..problem.clone()
    };
    let ub_iso = upper_compliance(&iso_problem, &classify(box_len, [c, 0.0, 0.0]))
        .expect("isotropic upper bound");
    // The isotropic limit of the cubic triple: C₄₄ = (C₁₁ − C₁₂)/2 reproduces E, ν.
    let lim = CubicMaterial::isotropic(young, nu);
    let lim_problem = Problem {
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: Some(vec![Some(lim); ntets]),
        },
        ..problem.clone()
    };
    let ub_lim = upper_compliance(&lim_problem, &classify(box_len, [c, 0.0, 0.0]))
        .expect("isotropic-limit upper bound");

    // An unstable triple (negative shear constant) must be refused by name.
    let bad = CubicMaterial {
        c11: 200.0,
        c12: 100.0,
        c44: -30.0,
        rot: None,
    };
    let bad_problem = Problem {
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: Some(vec![Some(bad); ntets]),
        },
        ..problem.clone()
    };
    let refused =
        upper_compliance(&bad_problem, &classify(box_len, [c, 0.0, 0.0])).expect_err("refuse");

    // Mixed element set: one cubic element among isotropic ones still brackets.
    let mut mixed = vec![None; ntets];
    mixed[0] = Some(cubic);
    let mixed_problem = Problem {
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: Some(mixed),
        },
        ..problem.clone()
    };
    let (u_mixed, rep_m) = solve(&mixed_problem, SolverOptions::default()).expect("mixed solve");
    assert!(rep_m.converged);
    let ch_mixed = compliance(&force, &u_mixed);
    let ub_mixed = upper_compliance(&mixed_problem, &classify(box_len, [c, 0.0, 0.0]))
        .expect("mixed upper bound");

    // The sparse route (a mesh past the 600-tet dense threshold) must leave no debug
    // artefact and still bracket its own conforming compliance.
    let big = box_mesh([10, 5, 3], [10.0, 10.0, 10.0]);
    assert!(big.tets.len() > 600, "sparse route needs > 600 tets");
    let big_len = 100.0;
    let big_force = traction_plane(&big, 0, big_len, [c, 0.0, 0.0], 1.0);
    let big_problem = Problem {
        mesh: big.clone(),
        material: Material {
            young: vec![young; big.tets.len()],
            poisson: nu,
            cubic: Some(vec![Some(cubic); big.tets.len()]),
        },
        fixed: supports(&big),
        force: big_force.clone(),
    };
    let (u_big, rep_b) = solve(&big_problem, SolverOptions::default()).expect("big solve");
    assert!(rep_b.converged);
    let ch_big = compliance(&big_force, &u_big);
    let ub_big =
        upper_compliance(&big_problem, &classify(big_len, [c, 0.0, 0.0])).expect("sparse bound");
    let dump_left = std::path::Path::new(dump).exists();

    let mut report = Report::new("V43", "the equilibrated upper end routes the cubic law");
    report
        .section(
            "Protocol",
            "A 40×10×10 mm bar of a cubic material (C₁₁ = 200, C₁₂ = 100, C₄₄ = 30 MPa, \
             Zener 0.6) in uniaxial [100] tension, traction c = 0.1 MPa on the far face, \
             the house three-face supports. The exact field σ = diag(c, 0, 0) satisfies \
             equilibrium, the continuity, and the pins, so both bracket ends read the \
             closed form C_true = c²·S₁₁·V to solver precision.",
        )
        .check(Check::relative(
            "C_h vs closed form c²·S₁₁·V (cubic [100] bar)",
            c_true,
            ch,
            "N·mm",
            1e-6,
        ))
        .check(Check::relative(
            "C(σ*) vs closed form c²·S₁₁·V (cubic [100] bar)",
            c_true,
            ub.j,
            "N·mm",
            1e-6,
        ))
        .check(Check::absolute(
            "bracket ordering: C_h − C(σ*) ≤ roundoff (both ends exact here)",
            0.0,
            (ch - ub.j).max(0.0),
            "N·mm",
            1e-9,
        ))
        .check(Check::absolute(
            "equilibrium witness",
            0.0,
            ub.equilibrium_residual,
            "N",
            1e-8,
        ))
        .check(Check::absolute(
            "trace-continuity witness",
            0.0,
            ub.continuity_residual,
            "N",
            1e-8,
        ))
        .check(Check::absolute(
            "Neumann-pin witness",
            0.0,
            ub.pin_residual,
            "N",
            1e-8,
        ))
        .section(
            "The isotropic limit and the mixed set",
            "At C₄₄ = (C₁₁ − C₁₂)/2 the cubic compliance equals the isotropic one, so the \
             recovered bound must agree with the isotropic route's to roundoff (different \
             arithmetic paths, not bit-identical). One cubic element among isotropic ones \
             must still bracket.",
        )
        .check(Check::relative(
            "isotropic-limit C(σ*) matches the isotropic route to roundoff",
            ub_iso.j,
            ub_lim.j,
            "N·mm",
            1e-9,
        ))
        .check(Check::absolute(
            "mixed set brackets: C(σ*) ≥ C_h",
            1.0,
            f64::from(ub_mixed.j >= ch_mixed),
            "",
            0.0,
        ))
        .section(
            "Refusals and the sparse route",
            "A negative shear constant is Born-unstable — the recovery must refuse it by \
             name rather than certify through a non-positive-definite A. The sparse route \
             (900 tets, past the dense threshold) must leave no /tmp/smat.txt debug dump \
             behind and still bracket its own conforming compliance (the sparse witnesses \
             carry a 2e-4 relative slack, so the ordering check gets 1e-3).",
        )
        .check(Check::absolute(
            "unstable cubic triple refused by name",
            1.0,
            f64::from(refused.contains("unstable")),
            "",
            0.0,
        ))
        .check(Check::absolute(
            "sparse route leaves no /tmp/smat.txt debug dump",
            0.0,
            f64::from(dump_left),
            "",
            0.0,
        ))
        .check(Check::relative(
            "sparse-route cubic bound vs its own C_h (ordering)",
            ch_big,
            ub_big.j,
            "N·mm",
            1e-3,
        ));
    report.finish();
}
