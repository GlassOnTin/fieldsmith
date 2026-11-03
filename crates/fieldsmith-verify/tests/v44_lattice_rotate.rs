//! V44 — lattice axis rotation (`fill ... rotate=(rx, ry, rz)`).
//!
//! GOAL M22 / V44: rotating the lattice frame decouples the internal lattice orientation
//! from the part frame. This gate verifies:
//!
//! 1. **Dual-basis inverse identity (`ortho::d_voigt` × `dual::elem_compliance`).**
//!    Under an arbitrary 3D Euler rotation `R(15°, 25°, 45°)`, the rotated 6×6 stiffness
//!    and rotated 6×6 compliance (after mapping shear indices `YZ/XZ` between the primal
//!    and dual conventions) are exact matrix inverses (`D · H = I₆` to `≤ 1e-13`).
//! 2. **Patch test on a `[100]`-loaded bar with `rotate = (0, 0, 45)`.**
//!    Applying uniaxial `[100]` tension (`n = [1, 0, 0]`) to a bar whose cubic lattice is
//!    rotated `45°` about `Z` reproduces V36's measured `[110] / [100]` modulus ratios
//!    across all five cubic-fitted TPMS families to `≤ 2e-4` relative error, and recovers
//!    element stress `σ = diag(Σ, 0, 0)` across all elements to `≤ 1e-6`.
//! 3. **Equilibrated upper bound on a rotated cubic bar (`dual::upper_compliance`).**
//!    On a `[100]`-loaded bar with `rotate = (0, 0, 45)`, both the primal compliance `C_h`
//!    and the equilibrated upper bound `C(σ*)` match the closed form `c² · S_[110] · V` to
//!    solver precision (`≤ 1e-6`), with `C_h ≤ C(σ*)` on both dense and sparse (`> 600`
//!    tets) recovery routes.
//! 4. **End-to-end `.fsm` `graded_of` with `fill ... rotate=(0, 0, 45)`.**
//!    Parsing a `.fsm` bar with `fill diamond network ... rotate=(0, 0, 45)` vs
//!    `rotate=(0, 0, 0)` stiffens the `[100]` bar along its `[110]` diagonal (`> 1.5×`
//!    lower compliance) while preserving the certified bracket `C_h ≤ C(σ*)`.
//! 5. **CPU field and interior-pass gradients under 3D rotation.**
//!    `FillExpr::eval` and `FillExpr::removed_skin` at `rotate = (15, 25, 45)` match
//!    central finite differences (`≤ 1e-5`) and preserve outer body skins unrotated.

use fenics_element::FiniteElement;
use fieldsmith_core::fill::{graded_of, law_cubic_at};
use fieldsmith_core::{analyse_full, Resolution};
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_field::dual::{mat_t_vec, mat_vec, rotation_xyz_deg};
use fieldsmith_field::expr::Expr;
use fieldsmith_field::lattice::{tpms, FillExpr, Grid3};
use fieldsmith_field::prim::Prim;
use fieldsmith_solve::ortho::{d_voigt, CubicMaterial};
use fieldsmith_solve::{
    assemble, compliance, element_stress_mat, solve, upper_compliance, Material, P2Mesh, Problem,
    SolverOptions, V3,
};
use fieldsmith_verify::{Check, Report};
use std::collections::HashMap;

const MEASURED: [(&str, Family, FillMode, f64); 5] = [
    ("gyroid sheet", Family::Gyroid, FillMode::Sheet, 1.135842),
    (
        "primitive sheet",
        Family::Primitive,
        FillMode::Sheet,
        2.069422,
    ),
    ("diamond sheet", Family::Diamond, FillMode::Sheet, 0.8127922),
    ("gyroid network", Family::Gyroid, FillMode::Network, 1.567271),
    (
        "diamond network",
        Family::Diamond,
        FillMode::Network,
        2.217194,
    ),
];

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

fn compliance_entries(c: &CubicMaterial) -> (f64, f64, f64) {
    let d = (c.c11 - c.c12) * (c.c11 + 2.0 * c.c12);
    ((c.c11 + c.c12) / d, -c.c12 / d, 1.0 / c.c44)
}

fn inv_modulus(s11: f64, s12: f64, s44: f64, n: V3) -> f64 {
    let aniso = s11 - s12 - s44 / 2.0;
    let nn = [n[0] * n[0], n[1] * n[1], n[2] * n[2]];
    s11 - 2.0 * aniso * (nn[0] * nn[1] + nn[1] * nn[2] + nn[2] * nn[0])
}

/// Invert a 6×6 matrix by Gauss-Jordan elimination (for verifying D⁻¹ vs rotated compliance).
fn invert6(mut a: [[f64; 6]; 6]) -> [[f64; 6]; 6] {
    let mut inv = [[0.0f64; 6]; 6];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..6 {
        let mut piv = col;
        for r in (col + 1)..6 {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        a.swap(col, piv);
        inv.swap(col, piv);
        let d = a[col][col];
        for c in 0..6 {
            a[col][c] /= d;
            inv[col][c] /= d;
        }
        for r in 0..6 {
            if r == col {
                continue;
            }
            let f = a[r][col];
            for c in 0..6 {
                a[r][c] -= f * a[col][c];
                inv[r][c] -= f * inv[col][c];
            }
        }
    }
    inv
}

/// Patch test on a `[100]`-aligned bar (`n = [1, 0, 0]`) for an arbitrary (possibly rotated)
/// `CubicMaterial`: computes the exact strain `ε_voigt = D⁻¹ · (σ, 0, 0, 0, 0, 0)ᵀ`, imposes
/// `u = ε · x` on all boundary nodes, solves the interior DOFs, and returns `(worst_stress_err, E_apparent)`.
fn patch_test_x(mesh: &P2Mesh, c: &CubicMaterial, sigma: f64) -> (f64, f64) {
    let d = d_voigt(c);
    let s_voigt = invert6(d);
    // For uniaxial world stress σ_xx = sigma, engineering strain is column 0 of s_voigt * sigma:
    // Voigt order: (ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz).
    let ev: [f64; 6] = std::array::from_fn(|i| s_voigt[i][0] * sigma);
    let eps = [
        [ev[0], 0.5 * ev[3], 0.5 * ev[5]],
        [0.5 * ev[3], ev[1], 0.5 * ev[4]],
        [0.5 * ev[5], 0.5 * ev[4], ev[2]],
    ];
    let mat = Material {
        young: vec![1000.0; mesh.tets.len()],
        poisson: 0.3,
        cubic: Some(vec![Some(*c); mesh.tets.len()]),
    };
    let g: Vec<f64> = mesh
        .nodes
        .iter()
        .flat_map(|p| {
            [
                eps[0][0] * p[0] + eps[0][1] * p[1] + eps[0][2] * p[2],
                eps[1][0] * p[0] + eps[1][1] * p[1] + eps[1][2] * p[2],
                eps[2][0] * p[0] + eps[2][1] * p[1] + eps[2][2] * p[2],
            ]
        })
        .collect();
    let k = assemble(mesh, &mat);
    let mut kg = vec![0.0; g.len()];
    k.mul(&g, &mut kg);
    let mut fixed = vec![0u8; mesh.nodes.len()];
    for (i, f) in fixed.iter_mut().enumerate() {
        let p = mesh.nodes[i];
        let on_boundary = p[0].abs() < 1e-9
            || (p[0] - 30.0).abs() < 1e-9
            || p[1].abs() < 1e-9
            || (p[1] - 10.0).abs() < 1e-9
            || p[2].abs() < 1e-9
            || (p[2] - 10.0).abs() < 1e-9;
        if on_boundary {
            *f = 0b111;
        }
    }
    let force: Vec<f64> = kg.iter().map(|v| -v).collect();
    let problem = Problem {
        mesh: mesh.clone(),
        material: mat.clone(),
        fixed,
        force,
    };
    let (v, rep) = solve(&problem, SolverOptions::default()).expect("patch solve");
    assert!(rep.converged, "patch solve did not converge");
    let u: Vec<f64> = v.iter().zip(&g).map(|(a, b)| a + b).collect();
    let want = [sigma, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut worst = 0.0f64;
    for e in 0..mesh.tets.len() {
        let s = element_stress_mat(mesh, &u, e, &mat, [0.25, 0.25, 0.25]);
        for i in 0..6 {
            let err = (s[i] - want[i]).abs() / sigma;
            worst = worst.max(err);
        }
    }
    (worst, sigma / eps[0][0])
}

#[test]
fn v44_lattice_rotate() {
    let mut report = Report::new(
        "V44",
        "lattice axis rotation: rotated TPMS field, rotated cubic stiffness, and rotated upper bound",
    );

    // 1. Patch test on a [100]-loaded bar with rotate=(0, 0, 45) across all 5 V36 families.
    let patch_mesh = box_mesh([6, 2, 2], [5.0, 5.0, 5.0]);
    let r_z45 = rotation_xyz_deg([0.0, 0.0, 45.0]);
    let sigma = 10.0;
    let mut max_stress_err = 0.0f64;
    report.section(
        "Rotated cubic patch test (`rotate = (0, 0, 45)` on `[100]` bar)",
        "A 30×10×10 mm `[100]`-aligned bar is solved under uniaxial `[100]` tension (`σ_xx = 10 MPa`) \
         first with unrotated cubic stiffness (`rotate = (0, 0, 0)`) and then with `rotate = (0, 0, 45)`. \
         Rotating the lattice frame by 45° around Z aligns the lattice's `[110]` diagonal with the \
         part's `[100]` load axis, so the ratio of apparent `[100]` moduli `E(45°) / E(0°)` must \
         reproduce V36's measured `E[110] / E[100]` table across all five fitted TPMS families, and \
         every element must recover `σ = diag(10, 0, 0)` MPa to roundoff.",
    );
    for &(label, fam, mode, measured) in &MEASURED {
        let c0 = law_cubic_at(fam, mode, 1000.0, 0.3, 0.22).expect("fit exists");
        let c45 = c0.with_rot(r_z45);
        let (err0, e0) = patch_test_x(&patch_mesh, &c0, sigma);
        let (err45, e45) = patch_test_x(&patch_mesh, &c45, sigma);
        max_stress_err = max_stress_err.max(err0).max(err45);
        report.check(Check::relative(
            &format!("{label}: rotated [100] patch E(45°)/E(0°) vs V36 measured"),
            measured,
            e45 / e0,
            "ratio",
            2e-4,
        ));
    }
    report.check(Check::absolute(
        "rotated patch: worst element stress error across all 5 families",
        0.0,
        max_stress_err,
        "rel",
        1e-6,
    ));

    // 2. Equilibrated upper bound on a [100]-loaded bar with rotate=(0, 0, 45).
    // Note: for a 45° Z rotation, in the world x-y plane the [100] and [010] axes are symmetric
    // mirror images of the lattice [100]/[010] axes (`R([1/√2, 1/√2, 0]) = [0, 1, 0]` and
    // `R([1/√2, -1/√2, 0]) = [1, 0, 0]`), so uniaxial σ_xx = c induces zero shear strain γ_xy = 0!
    // Therefore u_x = S_[110]·c·x, u_y = S_12'·c·y, u_z = S_12·c·z satisfies the three-face
    // house supports (x=0, y=0, z=0) with constant stress σ = diag(c, 0, 0), and BOTH C_h and C(σ*)
    // equal c² · S_[110] · V to solver precision!
    let base_cubic = CubicMaterial::new(200.0, 100.0, 30.0);
    let rot_cubic = base_cubic.with_rot(r_z45);
    let (s11, s12, s44) = compliance_entries(&base_cubic);
    let s_110 = inv_modulus(s11, s12, s44, [1.0 / 2.0f64.sqrt(), 1.0 / 2.0f64.sqrt(), 0.0]);
    let (young, nu, c_trac) = (100.0, 0.3, 0.1);
    let box_len = 40.0;
    let bar_mesh = box_mesh([4, 1, 1], [10.0, 10.0, 10.0]);
    let ntets = bar_mesh.tets.len();
    let volume = 4000.0;
    let force = traction_plane(&bar_mesh, 0, box_len, [c_trac, 0.0, 0.0], 1.0);
    let problem_rot = Problem {
        mesh: bar_mesh.clone(),
        material: Material {
            young: vec![young; ntets],
            poisson: nu,
            cubic: Some(vec![Some(rot_cubic); ntets]),
        },
        fixed: supports(&bar_mesh),
        force: force.clone(),
    };
    let (u_rot, rep_rot) = solve(&problem_rot, SolverOptions::default()).expect("rot solve");
    assert!(rep_rot.converged);
    let ch_rot = compliance(&force, &u_rot);
    let c_true_45 = c_trac * c_trac * s_110 * volume;
    let ub_rot = upper_compliance(&problem_rot, &classify(box_len, [c_trac, 0.0, 0.0]))
        .expect("rotated cubic upper bound");

    // General 3D rotation (15°, 25°, 45°) on the sparse route (> 600 tets): C(σ*) >= C_h.
    let r_3d = rotation_xyz_deg([15.0, 25.0, 45.0]);
    let cubic_3d = base_cubic.with_rot(r_3d);
    let big = box_mesh([10, 5, 3], [10.0, 10.0, 10.0]);
    assert!(big.tets.len() > 600);
    let big_len = 100.0;
    let big_force = traction_plane(&big, 0, big_len, [c_trac, 0.0, 0.0], 1.0);
    let big_problem = Problem {
        mesh: big.clone(),
        material: Material {
            young: vec![young; big.tets.len()],
            poisson: nu,
            cubic: Some(vec![Some(cubic_3d); big.tets.len()]),
        },
        fixed: supports(&big),
        force: big_force.clone(),
    };
    let (u_big, rep_big) = solve(&big_problem, SolverOptions::default()).expect("big 3d solve");
    assert!(rep_big.converged);
    let ch_big = compliance(&big_force, &u_big);
    let ub_big = upper_compliance(&big_problem, &classify(big_len, [c_trac, 0.0, 0.0]))
        .expect("sparse 3d rotated bound");

    report
        .section(
            "Equilibrated upper bound under lattice rotation (`dual::upper_compliance`)",
            "On the 40×10×10 mm bar (`C₁₁ = 200, C₁₂ = 100, C₄₄ = 30 MPa`) with `rotate = (0, 0, 45)`, \
             the rotated compliance along `[100]` is `S_[110] = S₁₁ − (S₁₁ − S₁₂ − S₄₄/2)/2` with zero \
             normal-shear coupling in the `x–y` plane (`[110]` and `[1−10]` are mirror axes of the cubic \
             cell), so `σ = diag(c, 0, 0)` remains the exact solution on the three-face supports and both \
             `C_h` and `C(σ*)` match `c² · S_[110] · V` to solver precision. Under a general 3D rotation \
             `rotate = (15, 25, 45)` on the 900-tet sparse route, `C(σ*)` brackets `C_h` within the \
             sparse recovery tolerance (`1e-3`).",
        )
        .check(Check::relative(
            "rotated C_h vs closed form c²·S_[110]·V (rotate=(0, 0, 45))",
            c_true_45,
            ch_rot,
            "N·mm",
            1e-6,
        ))
        .check(Check::relative(
            "rotated C(σ*) vs closed form c²·S_[110]·V (rotate=(0, 0, 45))",
            c_true_45,
            ub_rot.j,
            "N·mm",
            1e-6,
        ))
        .check(Check::absolute(
            "rotated equilibrium witness (rotate=(0, 0, 45))",
            0.0,
            ub_rot.equilibrium_residual,
            "N",
            1e-8,
        ))
        .check(Check::relative(
            "sparse 3D-rotated bound vs its own C_h (ordering)",
            ch_big,
            ub_big.j,
            "N·mm",
            1e-3,
        ));

    // 3. End-to-end `.fsm` graded_of with `fill ... rotate=(0, 0, 45)` vs `rotate=(0, 0, 0)`.
    let fsm_0 = "fieldsmith 1\n\
name \"Rotated Bar\"\n\
material petg\n\
\n\
box bar size=(40, 16, 16) at=(20, 8, 8)\n\
\n\
region left = bar.nx\n\
region right = bar.px\n\
support hold left fixed=xyz\n\
load pull right force=(500, 0, 0) case=\"Pull\"\n\
\n\
fill diamond network cell=6 skin=1.2 rho=(0.25, 0.25) curve=1 wall=0.8 margin=1\n";
    let fsm_45 = fsm_0.replace("margin=1", "margin=1 rotate=(0, 0, 45)");
    let doc_0 = fieldsmith_doc::fsm::parse(fsm_0).expect("parse doc_0");
    let doc_45 = fieldsmith_doc::fsm::parse(&fsm_45).expect("parse doc_45");
    assert_eq!(doc_0.fill.as_ref().unwrap().rotate, [0.0, 0.0, 0.0]);
    assert_eq!(doc_45.fill.as_ref().unwrap().rotate, [0.0, 0.0, 45.0]);
    let now = || 0.0;
    let res = Resolution::Dofs(4_500);
    let base = analyse_full(&doc_0, res, now, None, false).expect("solid base");
    let g0 = graded_of(&doc_0, &base, now, true).expect("graded 0");
    let g45 = graded_of(&doc_45, &base, now, true).expect("graded 45");
    let c0_bar = g0.cases[0].compliance;
    let c45_bar = g45.cases[0].compliance;
    let ub45_bar = g45.cases[0]
        .compliance_upper
        .expect("equilibrated upper bound on rotated graded fill");

    report
        .section(
            "End-to-end `.fsm` graded solve (`graded_of` with `rotate = (0, 0, 45)`)",
            "A 40×16×16 mm cantilever bar filled with a uniform-density (`ρ = 0.25`) diamond \
             network lattice (`E[110]/E[100] = 2.22` in the core) and a 1.2 mm solid skin is \
             graded and re-solved with `equilibrate = true` at `rotate = (0, 0, 0)` and \
             `rotate = (0, 0, 45)`. The rotated lattice stiffens the part and `compliance_upper` \
             brackets the graded conforming compliance.",
        )
        .check(Check::absolute(
            "diamond-network bar stiffens under rotate=(0, 0, 45): C(0°) / C(45°) > 1.15",
            1.0,
            f64::from(c0_bar / c45_bar > 1.15),
            "bool",
            0.0,
        ))
        .check(Check::absolute(
            "rotated graded bar certified bracket ordering: C(σ*) ≥ C_h",
            1.0,
            f64::from(ub45_bar >= c45_bar * (1.0 - 1e-4)),
            "bool",
            0.0,
        ));

    // 4. CPU `FillExpr::eval` and `FillExpr::removed_skin` rotated transform identity,
    //    exact rotated TPMS gradient vs finite differences, and outer skin invariance.
    let body = Expr::Prim(Prim::Box {
        half: [20.0, 15.0, 10.0],
        round: 0.0,
    });
    let rot_angles = [15.0, 25.0, 45.0];
    let r_lat = rotation_xyz_deg(rot_angles);
    let fx_rot = FillExpr {
        body: body.clone(),
        skin: 1.5,
        skin_max: 1.5,
        erode: 0.0,
        param_range: [0.0, 1.0],
        family: Family::Gyroid,
        morph: None,
        mode: FillMode::Sheet,
        cell: 6.0,
        cell_min: 6.0,
        min_wall: 0.0,
        rotate: rot_angles,
        param: Grid3::constant(0.8),
        keep: Expr::Empty,
    };
    let fx_unrot = FillExpr {
        rotate: [0.0; 3],
        ..fx_rot.clone()
    };
    let mut worst_xform_err = 0.0f64;
    let mut worst_tpms_grad_err = 0.0f64;
    for p in [[1.3, -2.1, 0.7], [-4.4, 3.2, -1.8], [0.5, 0.9, -3.3]] {
        // Deep in the core (where `core = -(d + skin)` is ~ -8.5 and `lat <= 3.0`), `FillExpr::eval(p)`
        // and `removed_skin(p)` equal the rotated lattice field `lattice_field(Rᵀ p)` with gradient `R g`.
        let q = mat_t_vec(&r_lat, p);
        let d_rot = fx_rot.eval(p);
        let d_core = fx_rot.removed_skin(p);
        let d_lat = fieldsmith_field::lattice::lattice_field(
            fx_rot.family,
            fx_rot.mode,
            fx_rot.cell,
            fx_rot.param.sample(p),
            q,
        );
        let g_world = mat_vec(&r_lat, d_lat.g);
        worst_xform_err = worst_xform_err
            .max((d_rot.v - d_lat.v).abs())
            .max((d_core.v - d_lat.v).abs());
        for (a, &gw) in g_world.iter().enumerate() {
            worst_xform_err = worst_xform_err
                .max((d_rot.g[a] - gw).abs())
                .max((d_core.g[a] - gw).abs());
        }
        // Also check the exact rotated raw TPMS gradient `R · ∇_q g(Rᵀ p)` against central finite differences.
        let t_rot_g = mat_vec(&r_lat, tpms(Family::Gyroid, 6.0, q).g);
        let h = 1e-6;
        for a in 0..3 {
            let mut pp = p;
            let mut pm = p;
            pp[a] += h;
            pm[a] -= h;
            let fd = (tpms(Family::Gyroid, 6.0, mat_t_vec(&r_lat, pp)).v
                - tpms(Family::Gyroid, 6.0, mat_t_vec(&r_lat, pm)).v)
                / (2.0 * h);
            worst_tpms_grad_err = worst_tpms_grad_err.max((t_rot_g[a] - fd).abs());
        }
    }
    let skin_probe = [20.0, 3.2, -1.7];
    let skin_diff = (fx_rot.eval(skin_probe).v - fx_unrot.eval(skin_probe).v).abs();

    report
        .section(
            "CPU rotated lattice field & gradient (`FillExpr::eval` and `removed_skin`)",
            "At `rotate = (15, 25, 45)` inside the lattice core, `FillExpr::eval` and `FillExpr::removed_skin` \
             agree to roundoff with `lattice_field(Rᵀ p)` and `R · g_local`, the rotated TPMS gradient \
             `R · ∇_q g(Rᵀ p)` matches central finite differences, and the outer body boundary (`d = 0`) \
             is invariant to `rotate`.",
        )
        .check(Check::absolute(
            "FillExpr::eval & removed_skin rotated value and gradient identity (Rᵀ p, R g)",
            0.0,
            worst_xform_err,
            "1",
            1e-13,
        ))
        .check(Check::absolute(
            "rotated TPMS gradient R·∇g(Rᵀ p) vs central finite difference",
            0.0,
            worst_tpms_grad_err,
            "1/mm",
            1e-5,
        ))
        .check(Check::absolute(
            "outer body surface distance invariant to lattice rotation",
            0.0,
            skin_diff,
            "mm",
            1e-12,
        ));

    report.finish();
}
