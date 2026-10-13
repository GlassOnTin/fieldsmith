//! Thermal solver tests: element quantities against hand integration, the lifted
//! Dirichlet conduction solve against an exactly representable field, and the thermal
//! stress recovery against the mechanism it must cancel (free uniform expansion).

use fieldsmith_solve::p2::P2Mesh;
use fieldsmith_solve::pcg::SolverOptions;
use fieldsmith_solve::{
    element_laplace, element_thermal_integrals, nodal_stress_thermal, solve_conduction,
    thermal_load, ThermalRefData,
};

const V: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

fn unit_tet() -> P2Mesh {
    P2Mesh::from_linear(&V, &[[0, 1, 2, 3]])
}

/// One interior node: four corner tets around E inside the unit tet ABCD. The boundary is
/// the big tet's faces; E and the corner-to-E midsides are interior.
fn star_mesh() -> P2Mesh {
    let verts = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.1, 0.1, 0.1], // E, interior (0.1 + 0.1 + 0.1 < 1)
    ];
    let tets: [[u32; 4]; 4] = [[0, 1, 2, 4], [0, 2, 3, 4], [0, 3, 1, 4], [1, 3, 2, 4]];
    let m = P2Mesh::from_linear(&verts, &tets);
    for t in &m.tets {
        let p = |i: usize| m.nodes[t[i] as usize];
        let (u, v, w) = (
            [p(1)[0] - p(0)[0], p(1)[1] - p(0)[1], p(1)[2] - p(0)[2]],
            [p(2)[0] - p(0)[0], p(2)[1] - p(0)[1], p(2)[2] - p(0)[2]],
            [p(3)[0] - p(0)[0], p(3)[1] - p(0)[1], p(3)[2] - p(0)[2]],
        );
        let vol6 = u[0] * (v[1] * w[2] - v[2] * w[1]) - u[1] * (v[0] * w[2] - v[2] * w[0])
            + u[2] * (v[0] * w[1] - v[1] * w[0]);
        assert!(vol6 > 0.0, "negative orientation at {t:?}: {vol6}");
    }
    m
}

#[test]
fn element_laplace_rows_sum_to_zero() {
    // φ ≡ const is in the kernel of ∇²·∇: every row of the element Laplacian sums to
    // zero, the diagonal is strictly positive, and the matrix is symmetric.
    let mesh = unit_tet();
    let rd = fieldsmith_solve::assemble::RefData::new();
    let l = element_laplace(&mesh, 0, &rd);
    for (a, row) in l.iter().enumerate() {
        let sum: f64 = row.iter().sum();
        assert!(sum.abs() < 1e-13, "row {a} sums to {sum}");
        assert!(l[a][a] > 0.0, "diagonal {a} is {}", l[a][a]);
        for (b, lb) in l.iter().enumerate() {
            assert!((row[b] - lb[a]).abs() < 1e-15, "asymmetric {a},{b}");
        }
    }
}

#[test]
fn thermal_integrals_match_hand_values() {
    let mesh = unit_tet();
    let rd = ThermalRefData::new();
    let vol = mesh.tet_volume(0).abs();

    // ΔT = x: ∂φa/∂x_i for the vertex basis φa = λa(2λa−1) is (4λa−1)∂λa/∂x_i; each
    // integrand is a monomial in x, y, z integrated by moments
    // ∫x^a y^b z^c dV = a!b!c!·3!/(3+a+b+c)!·V. Hand values (V = 1/6, ∫x = 1/24,
    // ∫x² = 1/60, ∫xy = 1/120, ∫xλ0 = 1/120):
    //   a0: each comp ∫x(1−4λ0) = 1/24 − 4/120 = 1/120 (isotropic ∇λ0)
    //   a1: comp x ∫x(4x−1) = 1/40, comp y/z ∂φ1/∂x_i = 0.
    //   a2: comp y ∫x(4y−1) = −1/120; a3 mirrored in z.
    let dt_x: Vec<f64> = mesh.nodes.iter().map(|p| p[0]).collect();
    let g = element_thermal_integrals(&mesh, 0, &dt_x, &rd);
    let want = [
        [1.0 / 120.0, 1.0 / 120.0, 1.0 / 120.0],
        [1.0 / 40.0, 0.0, 0.0],
        [0.0, -1.0 / 120.0, 0.0],
        [0.0, 0.0, -1.0 / 120.0],
    ];
    for (a, row) in want.iter().enumerate() {
        for (i, e) in row.iter().enumerate() {
            assert!(
                (g[a][i] - e).abs() < 1e-14,
                "vertex {a} comp {i}: {} vs {e}",
                g[a][i]
            );
        }
    }
    // Partition of unity: Σ_a ∂φa/∂x_i = 0 pointwise, so every component's row sum is 0
    // exactly under any quadrature — a consistency check on assembling/indexing.
    for i in 0..3 {
        let sum: f64 = g.iter().map(|row| row[i]).sum();
        assert!(sum.abs() < 1e-14, "component {i} sums to {sum}");
    }

    // ΔT = 1: ∫ ∂φa/∂x_i dV. Vertex rows vanish (∫(4λa−1)dV = 0). Midside rows come
    // from ∫4λb dV = V over the four sub-simplexes (∫λb dV = V/4) — e.g. a5 = (4λ2,
    // 4λ1, 0) integrates to (V, V, 0). Hand table:
    let g1 = element_thermal_integrals(&mesh, 0, &[1.0; 10], &rd);
    let want1 = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
        [0.0, -vol, -vol], // 4(λ0−λ1), −4λ1, −4λ1
        [vol, vol, 0.0],   // 4λ2, 4λ1, 0
        [-vol, 0.0, -vol], // −4λ2, 4(λ0−λ2), −4λ2
        [-vol, -vol, 0.0], // −4λ3, −4λ3, 4(λ0−λ3)
        [vol, 0.0, vol],   // 4λ3, 0, 4λ1
        [0.0, vol, vol],   // 0, 4λ3, 4λ2
    ];
    for (a, row) in want1.iter().enumerate() {
        for (i, e) in row.iter().enumerate() {
            assert!(
                (g1[a][i] - e).abs() < 1e-14,
                "const {a},{i}: {} vs {e}",
                g1[a][i]
            );
        }
    }
}

#[test]
fn thermal_load_scales_the_integrals_and_conserves() {
    let young = 70_000.0;
    let poisson = 0.33;
    let alpha = 2.3e-5;
    let m = fenics_assembly::elasticity::ElasticMaterial::new(young, poisson);
    let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha;

    // On the unit tet with ΔT = x: f[3a+i] = beta·∫ x·∂φa/∂x_i dV — the vertex rows
    // hand-checked in thermal_integrals_match_hand_values, carried through the β scale.
    let mesh = unit_tet();
    let dt_x: Vec<f64> = mesh.nodes.iter().map(|p| p[0]).collect();
    let f = thermal_load(
        &mesh,
        &dt_x,
        &[young],
        poisson,
        alpha,
        &ThermalRefData::new(),
    );
    let want = [
        beta / 120.0,
        beta / 120.0,
        beta / 120.0,
        beta / 40.0,
        0.0,
        0.0,
        0.0,
        -beta / 120.0,
        0.0,
        0.0,
        0.0,
        -beta / 120.0,
    ];
    for (k, e) in want.iter().enumerate() {
        assert!((f[k] - e).abs() < 1e-10 * beta, "f[{k}] = {} vs {e}", f[k]);
    }
    // The whole load sums to zero per component (partition of unity per element).
    for i in 0..3 {
        let sum: f64 = f.iter().skip(i).step_by(3).sum();
        assert!(sum.abs() < 1e-9 * beta, "component {i} sums to {sum}");
    }

    // Uniform ΔT = 1 on the star mesh: a free body under uniform expansion carries no
    // net force — the load must sum to zero per component.
    let star = star_mesh();
    let f1 = thermal_load(
        &star,
        &vec![1.0; star.nodes.len()],
        &vec![young; star.tets.len()],
        poisson,
        2.3e-5,
        &ThermalRefData::new(),
    );
    for i in 0..3 {
        let sum: f64 = f1.iter().skip(i).step_by(3).sum();
        assert!(sum.abs() < 1e-9 * beta, "component {i} sums to {sum}");
    }
}

#[test]
fn conduction_reproduces_linear_field_exactly() {
    // x is harmonic and lies in the P2 space; with Dirichlet T = x on the whole
    // boundary, every interior node's row residual vanishes by Green's identity, so the
    // discrete solution must reproduce T = x exactly. Exercises assemble, the Dirichlet
    // lift, and the scalar two-level PCG end to end.
    let mesh = star_mesh();
    let mut d: Vec<Option<f64>> = mesh.nodes.iter().map(|p| Some(p[0])).collect();
    // Free set: the interior vertex E (node 4) and the midsides of the edges touching it.
    d[4] = None;
    for (k, &[a, b]) in mesh.mid_edge.iter().enumerate() {
        if a == 4 || b == 4 {
            d[mesh.nverts + k] = None;
        }
    }
    let (t, report) = solve_conduction(&mesh, &d, SolverOptions::default()).unwrap();
    assert!(report.converged, "{report:?}");
    assert!(report.iterations > 0, "expected real iterations");
    for (i, ti) in t.iter().enumerate() {
        let want = d[i].unwrap_or(mesh.nodes[i][0]);
        assert!(
            (ti - want).abs() < 1e-9,
            "node {i} at {:?}: {ti} vs {want}",
            mesh.nodes[i]
        );
    }
}

#[test]
fn conduction_names_the_unsolvable_case() {
    let mesh = star_mesh();
    let d: Vec<Option<f64>> = vec![None; mesh.nodes.len()];
    let e = solve_conduction(&mesh, &d, SolverOptions::default()).unwrap_err();
    assert!(e.contains("at least one temperature region"), "{e}");
}

#[test]
fn thermal_stress_cancels_free_expansion() {
    // The element-scale mechanism V19(a) verifies on the full pipeline: displacement
    // u = αΔT·(x, y, z) with uniform ΔT carries elastic stress exactly (3λ+2μ)αΔT·I,
    // which the thermal term must subtract to roundoff.
    let mesh = unit_tet();
    let (dt0, alpha, young) = (100.0, 2.3e-5, 70_000.0);
    let poisson = 0.33;
    let mut u = vec![0.0; mesh.ndofs()];
    for (n, p) in mesh.nodes.iter().enumerate() {
        for i in 0..3 {
            u[3 * n + i] = alpha * dt0 * p[i];
        }
    }
    let dt = vec![dt0; mesh.nodes.len()];
    // Single element: the nodal average reduces to the element stress at the node.
    let s = nodal_stress_thermal(&mesh, &u, &[young], poisson, alpha, &dt);
    let scale = young * alpha * dt0;
    for (n, sn) in s.iter().enumerate() {
        for (k, c) in sn.iter().enumerate() {
            assert!(c.abs() < 1e-9 * scale, "node {n} comp {k}: {c}");
        }
    }
    // The un-subtracted elastic stress carries the full (3λ+2μ)αΔT — the cancellation
    // above is not passing through a zero test.
    let s_el = fieldsmith_solve::nodal_stress(&mesh, &u, &[young], poisson);
    let m = fenics_assembly::elasticity::ElasticMaterial::new(young, poisson);
    let beta = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha * dt0;
    for (k, c) in s_el[0].iter().take(3).enumerate() {
        assert!((c - beta).abs() < 1e-9 * beta, "elastic diag {k}");
    }
}

#[test]
fn thermal_stress_subtraction_only_touches_the_diagonal() {
    // A pure shear displacement state u = s·(y, x, 0) must survive the thermal term
    // untouched: εxy = s is the only strain, the thermal strain is isotropic.
    let mesh = unit_tet();
    let (dt0, alpha, young) = (50.0, 2.3e-5, 70_000.0);
    let poisson = 0.33;
    let s = 1e-3;
    let mut u = vec![0.0; mesh.ndofs()];
    for (n, p) in mesh.nodes.iter().enumerate() {
        u[3 * n] = s * p[1];
        u[3 * n + 1] = s * p[0];
    }
    let dt = vec![dt0; mesh.nodes.len()];
    let s_th = nodal_stress_thermal(&mesh, &u, &[young], poisson, alpha, &dt);
    let s_el = fieldsmith_solve::nodal_stress(&mesh, &u, &[young], poisson);
    let m = fenics_assembly::elasticity::ElasticMaterial::new(young, poisson);
    let th = (3.0 * m.lame_lambda() + 2.0 * m.lame_mu()) * alpha * dt0;
    for k in 0..6 {
        if k < 3 {
            assert!(
                ((s_el[0][k] - th) - s_th[0][k]).abs() < 1e-9 * th.abs().max(1.0),
                "diag {k}"
            );
        } else {
            // same arithmetic path as the un-subtracted stress: must agree to the bit
            assert!((s_el[0][k] - s_th[0][k]).abs() == 0.0, "shear {k} moved");
        }
    }
}
