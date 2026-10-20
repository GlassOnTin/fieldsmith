//! V42 — the routed cubic law reads the measured direction dependence.
//!
//! V36 measured how direction-dependent the homogenised lattice law is (E*(45°)/E*(0°)
//! up to 2.07× density-normalized) and fitted a cubic tensor per family; the engine
//! routes that tensor per element (`ortho.rs`'s kernel, `assemble`'s per-element
//! routing, `stress.rs`'s routed recovery). This gate closes the chain end to end:
//!
//! 1. **The fit reproduces V36's measurement.** Each family's `law_cubic_at` triple,
//!    inverted to compliance in closed form, must predict the [110]-over-[100] modulus
//!    ratio V36 measured (the density-normalized table). The inversion is the cubic
//!    [110] direction formula 1/E[110] = S11 − (S11 − S12 − S44/2)/2 — the same
//!    relation V36's fit uses (its printed recipe was wrong; see the report's note).
//! 2. **The routed solve reads the tensor.** A homogeneous cubic bar under an imposed
//!    affine strain (patch test: u = ε·x on every boundary node, ε = S : σ from the
//!    closed form) must recover σ = Σ·n⊗n in every element and the closed-form
//!    directional modulus — for n = [100] and n = [110] through the real
//!    `assemble` → `solve` → `element_stress_mat` route.
//! 3. **The isotropic route is untouched.** `cubic: Some(all-None)` solves
//!    bit-identically to `cubic: None`, and the primitive-network family (no coupon
//!    fit) still reports `None` — the isotropic law stays the default everywhere a
//!    fit does not exist.
//!
//! Named gaps are listed in the report's Coverage and limits section.

use fieldsmith_core::fill::law_cubic_at;
use fieldsmith_doc::{Family, FillMode};
use fieldsmith_solve::{
    assemble, element_stress_mat, solve, Material, P2Mesh, Problem, SolverOptions, V3,
};
use fieldsmith_verify::{Check, Report};
use std::collections::HashMap;

/// The five V36-measured families and their pinned density-normalized
/// E*(45°)/E*(0°) ratios (the same table V36's gate holds).
const MEASURED: [(&str, Family, FillMode, f64); 5] = [
    ("gyroid sheet", Family::Gyroid, FillMode::Sheet, 1.135842),
    ("primitive sheet", Family::Primitive, FillMode::Sheet, 2.069422),
    ("diamond sheet", Family::Diamond, FillMode::Sheet, 0.8127922),
    ("gyroid network", Family::Gyroid, FillMode::Network, 1.567271),
    ("diamond network", Family::Diamond, FillMode::Network, 2.217194),
];

/// The cubic compliance (S11, S12, S44) of a cubic stiffness triple, inverted in
/// closed form: D = (C11 − C12)(C11 + 2·C12), S11 = (C11 + C12)/D, S12 = −C12/D,
/// S44 = 1/C44.
fn compliance(c11: f64, c12: f64, c44: f64) -> (f64, f64, f64) {
    let d = (c11 - c12) * (c11 + 2.0 * c12);
    ((c11 + c12) / d, -c12 / d, 1.0 / c44)
}

/// 1/E(n) for a cubic compliance along the unit direction n (the standard cubic
/// direction formula; n = [100] gives S11, n = [110] gives S11 − (S11 − S12 − S44/2)/2).
fn inv_modulus(s11: f64, s12: f64, s44: f64, n: V3) -> f64 {
    let aniso = s11 - s12 - s44 / 2.0;
    let nn = [n[0] * n[0], n[1] * n[1], n[2] * n[2]];
    s11 - 2.0 * aniso * (nn[0] * nn[1] + nn[1] * nn[2] + nn[2] * nn[0])
}

/// The strain of the uniaxial stress σ = Σ·n⊗n under a cubic compliance: ε = S : σ.
fn strain_for_stress(s11: f64, s12: f64, s44: f64, sigma: f64, n: V3) -> [[f64; 3]; 3] {
    // σ in tensor form: Σ·n⊗n. Voigt (xx, yy, zz, xy, yz, zx).
    let sv = [
        sigma * n[0] * n[0],
        sigma * n[1] * n[1],
        sigma * n[2] * n[2],
        sigma * n[0] * n[1],
        sigma * n[1] * n[2],
        sigma * n[2] * n[0],
    ];
    // ε from the compliance: normals ε_ii = S12·tr σ + (S11 − S12)·σ_ii, shears
    // γ = S44·τ, tensor ε = γ/2.
    let e = |i: usize| s12 * (sv[0] + sv[1] + sv[2]) + (s11 - s12) * sv[i];
    let g = |i: usize| s44 * sv[3 + i];
    let mut out = [[0.0f64; 3]; 3];
    out[0][0] = e(0);
    out[1][1] = e(1);
    out[2][2] = e(2);
    out[0][1] = g(0) / 2.0;
    out[1][0] = out[0][1];
    out[1][2] = g(1) / 2.0;
    out[2][1] = out[1][2];
    out[2][0] = g(2) / 2.0;
    out[0][2] = out[2][0];
    out
}

/// A stair-tessellated box (the V34 helper): `cubes` hex cells of side `edge`, six
/// tets each, orientation normalised, volume asserted.
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
    let det3 = |t: &[u32; 4]| -> f64 {
        let p = t.map(|v| map[&v]);
        let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let c = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    };
    for t in &mut tets {
        if det3(t) < 0.0 {
            t.swap(2, 3);
        }
    }
    assert!(tets.iter().all(|t| det3(t) > 0.0), "orientations");
    let volume = edge * edge * edge * (nx * ny * nz) as f64;
    let sum: f64 = tets.iter().map(|t| det3(t) / 6.0).sum();
    assert!(
        (sum - volume).abs() < 1e-9 * volume,
        "volume identity: {sum} vs {volume}"
    );
    P2Mesh::from_linear(&verts, &tets)
}

/// The patch test: impose the affine field u = ε·x on every boundary node (the exact
/// solution of zero-body-force elasticity with that boundary data — affine fields are
/// in the P2 span), solve through the routed cubic route, and return the worst
/// relative error of the recovered element stress against σ = Σ·n⊗n, plus the
/// apparent modulus Σ/ε(n).
fn patch_test(mesh: &P2Mesh, c: &fieldsmith_solve::ortho::CubicMaterial, sigma: f64, n: V3) -> (f64, f64) {
    let (s11, s12, s44) = compliance(c.c11, c.c12, c.c44);
    let eps = strain_for_stress(s11, s12, s44, sigma, n);
    let mat = Material {
        young: vec![1000.0; mesh.tets.len()],
        poisson: 0.3,
        cubic: Some(vec![Some(*c); mesh.tets.len()]),
    };
    // The affine boundary data g(x) = ε·x (rigid motion zero: g(0) = 0).
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
    // The solver's Dirichlet handling fixes the boundary at ZERO (`apply_dirichlet`
    // zeroes the fixed rows and the solve starts from 0), so the patch solves for
    // v = u − g with f = −K·g on the free dofs, and the affine field is added back
    // before recovery: u = v + g.
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
        fixed: fixed.clone(),
        force,
    };
    let (v, rep) = solve(&problem, SolverOptions::default()).expect("patch solve");
    assert!(rep.converged, "patch solve did not converge");
    let u: Vec<f64> = v.iter().zip(&g).map(|(a, b)| a + b).collect();
    // Recovered stress at each element centroid vs σ = Σ·n⊗n.
    let want: [f64; 6] = [
        sigma * n[0] * n[0],
        sigma * n[1] * n[1],
        sigma * n[2] * n[2],
        sigma * n[0] * n[1],
        sigma * n[1] * n[2],
        sigma * n[2] * n[0],
    ];
    let mut worst = 0.0f64;
    for e in 0..mesh.tets.len() {
        let s = element_stress_mat(mesh, &u, e, &mat, [0.25, 0.25, 0.25]);
        for i in 0..6 {
            let err = (s[i] - want[i]).abs() / sigma;
            worst = worst.max(err);
        }
    }
    // The apparent modulus along n: Σ over the axial strain ε(n) = n·ε·n.
    let eps_n = (0..3).map(|i| (0..3).map(|j| n[i] * eps[i][j] * n[j]).sum::<f64>()).sum::<f64>();
    (worst, sigma / eps_n)
}

#[test]
fn v42_anisotropic_route() {
    let mut r = Report::new(
        "V42",
        "the routed cubic law: the fit reproduces V36 and the solve reads the tensor",
    );

    // 1. The fit reproduces V36's measured direction dependence (closed form).
    for &(label, fam, mode, measured) in &MEASURED {
        let c = law_cubic_at(fam, mode, 1000.0, 0.3, 0.22).expect("fit exists");
        let (s11, s12, s44) = compliance(c.c11, c.c12, c.c44);
        let e0 = 1.0 / s11;
        let e45 = 1.0 / inv_modulus(s11, s12, s44, [1.0 / 2.0f64.sqrt(), 1.0 / 2.0f64.sqrt(), 0.0]);
        r.check(Check::relative(
            &format!("{label}: fitted tensor predicts E[110]/E[100] (V36 measured)"),
            measured,
            e45 / e0,
            "ratio",
            2e-4,
        ));
    }

    // 2. The routed solve reads the tensor: patch tests on the gyroid-sheet fit along
    //    [100] and [110]. The stress must come back as Σ·n⊗n everywhere, and the
    //    apparent modulus ratio must equal the closed-form (and V36-measured) one.
    let mesh = box_mesh([6, 2, 2], 5.0);
    let c = law_cubic_at(Family::Gyroid, FillMode::Sheet, 1000.0, 0.3, 0.22).expect("fit");
    let sigma = 10.0;
    let n0: V3 = [1.0, 0.0, 0.0];
    let n45: V3 = [1.0 / 2.0f64.sqrt(), 1.0 / 2.0f64.sqrt(), 0.0];
    let (err0, e0) = patch_test(&mesh, &c, sigma, n0);
    let (err45, e45) = patch_test(&mesh, &c, sigma, n45);
    r.check(Check::absolute(
        "patch [100]: recovered stress vs Σ·n⊗n (worst element, relative)",
        0.0,
        err0,
        "rel",
        1e-6,
    ));
    r.check(Check::absolute(
        "patch [110]: recovered stress vs Σ·n⊗n (worst element, relative)",
        0.0,
        err45,
        "rel",
        1e-6,
    ));
    let (s11, s12, s44) = compliance(c.c11, c.c12, c.c44);
    let closed = inv_modulus(s11, s12, s44, n0) / inv_modulus(s11, s12, s44, n45);
    r.check(Check::relative(
        "patch: solved E[110]/E[100] vs the closed form",
        closed,
        e45 / e0,
        "ratio",
        1e-4,
    ));
    r.check(Check::relative(
        "patch: solved E[110]/E[100] vs V36 measured (gyroid sheet)",
        1.135842,
        e45 / e0,
        "ratio",
        1e-3,
    ));

    // 3. The isotropic route is untouched: all-None cubic solves bit-identically to
    //    cubic: None, and the primitive network (no fit) still reports None.
    let iso_mat = Material {
        young: vec![1000.0; mesh.tets.len()],
        poisson: 0.3,
        cubic: None,
    };
    let none_mat = Material {
        young: vec![1000.0; mesh.tets.len()],
        poisson: 0.3,
        cubic: Some(vec![None; mesh.tets.len()]),
    };
    let mut fixed = vec![0u8; mesh.nodes.len()];
    let mut force = vec![0.0; mesh.nodes.len() * 3];
    for (i, p) in mesh.nodes.iter().enumerate() {
        if (p[0] - 30.0).abs() < 1e-9 {
            force[3 * i] += 1.0 / mesh.nodes.iter().filter(|q| (q[0] - 30.0).abs() < 1e-9).count() as f64;
        }
        if p[0].abs() < 1e-9 {
            fixed[i] = 0b111;
        }
    }
    let solve_with = |mat: &Material| {
        let problem = Problem {
            mesh: mesh.clone(),
            material: mat.clone(),
            fixed: fixed.clone(),
            force: force.clone(),
        };
        let (u, _) = solve(&problem, SolverOptions::default()).expect("iso solve");
        let s: Vec<[f64; 6]> = (0..mesh.tets.len())
            .map(|e| element_stress_mat(&mesh, &u, e, mat, [0.25, 0.25, 0.25]))
            .collect();
        (u, s)
    };
    let (u_a, s_a) = solve_with(&iso_mat);
    let (u_b, s_b) = solve_with(&none_mat);
    let bit_identical = u_a == u_b
        && s_a.len() == s_b.len()
        && s_a
            .iter()
            .zip(&s_b)
            .all(|(x, y)| x.iter().zip(y).all(|(p, q)| p == q));
    r.check(Check {
        quantity: "cubic: Some(all-None) solves bit-identically to cubic: None".into(),
        reference: 1.0,
        measured: if bit_identical { 1.0 } else { 0.0 },
        unit: "bool".into(),
        tolerance: 0.0,
        absolute: true,
    });
    let prim_none = law_cubic_at(Family::Primitive, FillMode::Network, 1000.0, 0.3, 0.22).is_none();
    r.check(Check {
        quantity: "the primitive network (no coupon fit) routes isotropic".into(),
        reference: 1.0,
        measured: if prim_none { 1.0 } else { 0.0 },
        unit: "bool".into(),
        tolerance: 0.0,
        absolute: true,
    });

    r.section(
        "Protocol",
        "The fit check inverts each family's `law_cubic_at` triple to compliance in closed \
         form and applies the cubic direction formula \
         1/E(n) = S11 − 2(S11 − S12 − S44/2)(n₁²n₂² + n₂²n₃² + n₃²n₁²). The patch test \
         imposes the affine field u = ε·x (ε = S : σ, σ = Σ·n⊗n, Σ = 10 MPa) on every \
         boundary node of a 30×10×10 box (6×2×2 hex cells, six tets each) and solves \
         through the routed route — `assemble` (per-element cubic kernel), PCG, \
         `element_stress_mat` — then compares the recovered stress against Σ·n⊗n at every \
         element centroid. Affine fields are in the P2 span, so the only error is the \
         solver's.",
    )
    .section(
        "The fit recipe's [110] inversion",
        "V36 printed its S44 recipe as S44 = 2(1/E45 − S12), which is not the cubic [110] \
         inversion: the [110] modulus is 1/E[110] = S11 − (S11 − S12 − S44/2)/2, i.e. \
         S44 = 4/E45 − 2(S11 + S12). The two agree only when E45 = E0. With the corrected \
         inversion the fitted tensors reproduce V36's measured ratios exactly (the check \
         tolerances are the fit constants' 5-digit rounding); the C11 and C12/C11 side of \
         the fit is unaffected — it comes from E[100] and ν, not from E[110].",
    )
    .section(
        "Coverage and limits",
        "- The patch test proves the routed kernel+assembly+recovery implement the tensor \
         the fit carries; it does not re-measure the lattice (that is V36's coupon run). \
         The chain V36 measurement → fit → law → solve is closed by checks 1 and 2 \
         together, not by either alone.\n\
         - One density (0.22, the fit point) and two directions ([100], [110]) are tested \
         through the solve; the density bridge and the other families' patch rows are \
         covered by the fit check's closed form, not by a solve.\n\
         - The equilibrated upper end still refuses the cubic route (fail closed in \
         `run_case`); the dual stress projection's cubic extension stays tracked.\n\
         - The document grammar has no lattice-rotation option: in the product the lattice \
         axes are the mesh axes, and direction dependence enters through the part's \
         orientation relative to its loads — the patch test's rotated loading is the \
         equivalent statement.",
    );
    r.finish();
}
