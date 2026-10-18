//! Thermal solver tests: element quantities against hand integration, the lifted
//! Dirichlet conduction solve against an exactly representable field, and the thermal
//! stress recovery against the mechanism it must cancel (free uniform expansion).

use fieldsmith_solve::p2::P2Mesh;
use fieldsmith_solve::pcg::SolverOptions;
use fieldsmith_solve::{
    element_flux, element_laplace, element_robin, element_thermal_integrals, nodal_stress_thermal,
    solve_conduction, thermal_load, HeatFluxBc, RobinBc, ThermalRefData,
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

/// The unit cube as the six Freudenthal tets off the main diagonal A→H, all with
/// positive orientation.
fn cube_mesh() -> P2Mesh {
    let v = [
        [0.0, 0.0, 0.0], // 0 A
        [1.0, 0.0, 0.0], // 1 B
        [0.0, 1.0, 0.0], // 2 C
        [0.0, 0.0, 1.0], // 3 D
        [1.0, 1.0, 0.0], // 4 E
        [1.0, 0.0, 1.0], // 5 F
        [0.0, 1.0, 1.0], // 6 G
        [1.0, 1.0, 1.0], // 7 H
    ];
    let tets: [[u32; 4]; 6] = [
        [0, 1, 4, 7],
        [0, 5, 1, 7],
        [0, 4, 2, 7],
        [0, 2, 6, 7],
        [0, 3, 5, 7],
        [0, 6, 3, 7],
    ];
    let m = P2Mesh::from_linear(&v, &tets);
    for t in &m.tets {
        let p = |i: usize| m.nodes[t[i] as usize];
        let (u, vw) = (
            [p(1)[0] - p(0)[0], p(1)[1] - p(0)[1], p(1)[2] - p(0)[2]],
            (
                [p(2)[0] - p(0)[0], p(2)[1] - p(0)[1], p(2)[2] - p(0)[2]],
                [p(3)[0] - p(0)[0], p(3)[1] - p(0)[1], p(3)[2] - p(0)[2]],
            ),
        );
        let (w, z) = vw;
        let vol6 = u[0] * (w[1] * z[2] - w[2] * z[1]) - u[1] * (w[0] * z[2] - w[2] * z[0])
            + u[2] * (w[0] * z[1] - w[1] * z[0]);
        assert!(vol6 > 0.0, "negative orientation at {t:?}: {vol6}");
    }
    m
}

/// The mesh node id of the midside of the edge joining vertices `p` and `q`.
fn midside(mesh: &P2Mesh, p: u32, q: u32) -> u32 {
    let key = if p < q { [p, q] } else { [q, p] };
    let k = mesh
        .mid_edge
        .iter()
        .position(|&e| e == key)
        .expect("midside edge in table");
    mesh.nverts as u32 + k as u32
}

/// The hull triangles as the solver's facet node lists `[c0, c1, c2, m01, m12, m20]`:
/// corner triples met once by the tets. Facet winding is irrelevant to the Robin term
/// (it integrates the surface metric, not the normal).
fn hull_facets(mesh: &P2Mesh) -> Vec<[u32; 6]> {
    let faces = |t: &[u32; 10]| {
        [
            [t[0], t[1], t[2]],
            [t[0], t[1], t[3]],
            [t[0], t[2], t[3]],
            [t[1], t[2], t[3]],
        ]
    };
    let mut count: std::collections::HashMap<[u32; 3], usize> = Default::default();
    for t in &mesh.tets {
        for f in faces(t) {
            let mut k = f;
            k.sort_unstable();
            *count.entry(k).or_insert(0) += 1;
        }
    }
    let mut out = Vec::new();
    for t in &mesh.tets {
        for f in faces(t) {
            let mut k = f;
            k.sort_unstable();
            if count[&k] != 1 {
                continue;
            }
            out.push([
                f[0],
                f[1],
                f[2],
                midside(mesh, f[0], f[1]),
                midside(mesh, f[1], f[2]),
                midside(mesh, f[2], f[0]),
            ]);
        }
    }
    out
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
    let (t, report) = solve_conduction(&mesh, &d, &[], &[], SolverOptions::default()).unwrap();
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
    let e = solve_conduction(&mesh, &d, &[], &[], SolverOptions::default()).unwrap_err();
    assert!(e.contains("at least one temperature region"), "{e}");
    assert!(e.contains("every face would be insulated"), "{e}");
}

// ---------------------------------------------------------------------------
// Convection (Robin): the surface-mass integrals, then the solve against an
// analytically known profile.
// ---------------------------------------------------------------------------

#[test]
fn robin_element_integrals_are_the_partition_of_unity() {
    // The unit tet's face spanned by (0,0,0), (1,0,0), (0,1,0): area 0.5. In the P2
    // nodal order its nodes are the corners 0,1,2 and the midsides 01,12,20 = 4,5,6
    // (edges (0,1),(1,2),(0,2) are the first three in P2Mesh's edge order).
    let mesh = unit_tet();
    let (m, l) = element_robin(&mesh, &[0, 1, 2, 4, 5, 6]);
    let sum_l: f64 = l.iter().sum();
    let sum_m: f64 = m.iter().flat_map(|r| r.iter()).sum();
    assert!((sum_l - 0.5).abs() < 1e-12, "Σl = {sum_l}");
    assert!((sum_m - 0.5).abs() < 1e-12, "ΣΣm = {sum_m}");
    for a in 0..6 {
        let row_sum: f64 = m[a].iter().sum();
        assert!(
            (row_sum - l[a]).abs() < 1e-12,
            "row {a}: {row_sum} vs {}",
            l[a]
        );
        assert!(m[a][a] > 0.0);
        for (b, &mab) in m[a].iter().enumerate() {
            assert!((mab - m[b][a]).abs() < 1e-14, "asymmetric {a},{b}");
        }
    }
}

#[test]
fn convection_bar_matches_the_analytic_profile() {
    // The 1D wall: T(0) = 100 fixed, air at 20 with h·k = 1 across x = 1. The flux is
    // q = (100 − 20)/(L/k + 1/h) = 80/2 = 40, the field T = 100 − 40·x, so the surface
    // sits at 60 = (20 + 100)/2. The linear profile lies in the P2 space and satisfies
    // every equation pointwise, so the discrete solve must reproduce it to solver
    // accuracy.
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    assert_eq!(wall.len(), 2, "the x = 1 face is two triangles");
    let opts = SolverOptions {
        rtol: 1e-12,
        ..SolverOptions::default()
    };
    let (t, report) = solve_conduction(
        &mesh,
        &d,
        &[RobinBc {
            h: 1.0,
            t_inf: 20.0,
            facets: wall,
        }],
        &[],
        opts,
    )
    .unwrap();
    assert!(report.converged, "{report:?}");
    assert!(report.iterations > 0, "expected real iterations");
    for (i, ti) in t.iter().enumerate() {
        let want = 100.0 - 40.0 * mesh.nodes[i][0];
        assert!(
            (ti - want).abs() < 1e-9,
            "node {i} at {:?}: {ti} vs {want}",
            mesh.nodes[i]
        );
    }
}

#[test]
fn convection_only_body_settles_to_t_inf() {
    // No Dirichlet: the h > 0 band on the whole boundary lifts the Laplacian's constant
    // mode, and T ≡ t_inf satisfies every equation exactly.
    let mesh = star_mesh();
    let facets = hull_facets(&mesh);
    assert_eq!(facets.len(), 4, "the unit tet's four faces");
    let opts = SolverOptions {
        rtol: 1e-12,
        ..SolverOptions::default()
    };
    let (t, report) = solve_conduction(
        &mesh,
        &vec![None; mesh.nodes.len()],
        &[RobinBc {
            h: 1.0,
            t_inf: 42.0,
            facets,
        }],
        &[],
        opts,
    )
    .unwrap();
    assert!(report.converged, "{report:?}");
    for (i, ti) in t.iter().enumerate() {
        assert!((ti - 42.0).abs() < 1e-9, "node {i}: {ti} vs 42",);
    }
}

#[test]
fn zero_convection_band_is_a_no_op() {
    // h = 0 contributes h·MΓ = 0 and h·t_inf·l = 0: the result and the iteration count
    // must match the empty band bit for bit.
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    let band = RobinBc {
        h: 0.0,
        t_inf: 20.0,
        facets: wall,
    };
    let (t0, r0) = solve_conduction(&mesh, &d, &[band], &[], SolverOptions::default()).unwrap();
    let (t1, r1) = solve_conduction(&mesh, &d, &[], &[], SolverOptions::default()).unwrap();
    assert_eq!(t0, t1);
    assert_eq!(r0.iterations, r1.iterations);
}

#[test]
fn convection_band_validates() {
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    for h in [-1.0, f64::NAN] {
        let e = solve_conduction(
            &mesh,
            &d,
            &[RobinBc {
                h,
                t_inf: 20.0,
                facets: wall.clone(),
            }],
            &[],
            SolverOptions::default(),
        )
        .unwrap_err();
        assert!(e.contains("h must be finite"), "h = {h}: {e}");
    }
    let e = solve_conduction(
        &mesh,
        &d,
        &[RobinBc {
            h: 1.0,
            t_inf: f64::NAN,
            facets: wall,
        }],
        &[],
        SolverOptions::default(),
    )
    .unwrap_err();
    assert!(e.contains("t_inf must be finite"), "{e}");
}

// ---------------------------------------------------------------------------
// Heat flux (Neumann): the lump integral, the solve against an analytically
// known profile, and the bands' effect on fixed rows and each other.
// ---------------------------------------------------------------------------

#[test]
fn flux_lump_integrals_partition_the_face() {
    // element_flux is element_robin's lump row without the matrix. On the P2 face trace
    // each corner function is its own λ(2λ−1) — mean zero over the face — and each
    // edge-midside function is 4λiλj — mean A/3 — so a uniform q lands entirely on the
    // three midsides and conserves q·A.
    let mesh = unit_tet();
    let l = element_flux(&mesh, &[0, 1, 2, 4, 5, 6]);
    let sum_l: f64 = l.iter().sum();
    assert!((sum_l - 0.5).abs() < 1e-12, "Σl = {sum_l}");
    for x in l[0..3].iter() {
        assert!(x.abs() < 1e-14, "corner lumps: {l:?}");
    }
    for x in l[3..6].iter() {
        assert!((x - 1.0 / 6.0).abs() < 1e-12, "midside lumps: {l:?}");
    }
}

#[test]
fn heatflux_bar_matches_the_analytic_profile() {
    // The 1D wall with a prescribed inflow instead of a convection: T(0) = 100 fixed,
    // q = +2 W/mm² across x = 1 heats the solid, so ∂T/∂n = q/k = 2 gives T = 100 + 2x
    // with the far face at 102. The profile lies in the P2 space and satisfies every
    // equation pointwise, so the discrete solve must reproduce it to solver accuracy.
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    assert_eq!(wall.len(), 2, "the x = 1 face is two triangles");
    let opts = SolverOptions {
        rtol: 1e-12,
        ..SolverOptions::default()
    };
    let (t, report) = solve_conduction(
        &mesh,
        &d,
        &[],
        &[HeatFluxBc { q: 2.0, facets: wall }],
        opts,
    )
    .unwrap();
    assert!(report.converged, "{report:?}");
    assert!(report.iterations > 0, "expected real iterations");
    for (i, ti) in t.iter().enumerate() {
        let want = 100.0 + 2.0 * mesh.nodes[i][0];
        assert!(
            (ti - want).abs() < 1e-9,
            "node {i} at {:?}: {ti} vs {want}",
            mesh.nodes[i]
        );
    }
}

#[test]
fn heatflux_on_fixed_face_is_silent() {
    // A flux band on the fixed face contributes q·l only to lifted rows, and the lift
    // zeroes the fixed rows' rhs: the field and the iteration count must match the
    // flux-free solve bit for bit.
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall0 = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 0.0))
        .collect::<Vec<_>>();
    assert!(!wall0.is_empty(), "the x = 0 face has facets");
    let (t0, r0) = solve_conduction(
        &mesh,
        &d,
        &[],
        &[HeatFluxBc {
            q: 10.0,
            facets: wall0,
        }],
        SolverOptions::default(),
    )
    .unwrap();
    let (t1, r1) = solve_conduction(&mesh, &d, &[], &[], SolverOptions::default()).unwrap();
    assert_eq!(t0, t1);
    assert_eq!(r0.iterations, r1.iterations);
}

#[test]
fn heatflux_band_validates() {
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = vec![Some(100.0); mesh.nodes.len()];
    let wall = hull_facets(&mesh);
    let e = solve_conduction(
        &mesh,
        &d,
        &[],
        &[HeatFluxBc {
            q: f64::NAN,
            facets: wall,
        }],
        SolverOptions::default(),
    )
    .unwrap_err();
    assert!(e.contains("q must be finite"), "{e}");
}

#[test]
fn flux_only_body_names_its_gap() {
    // No fixed node, no convection, only a heat flux: still unsolvable, and the error
    // must say why a flux does not rescue it (the two-variant gate message).
    let mesh = star_mesh();
    let facets = hull_facets(&mesh);
    let d: Vec<Option<f64>> = vec![None; mesh.nodes.len()];
    let e = solve_conduction(
        &mesh,
        &d,
        &[],
        &[HeatFluxBc {
            q: 1.0,
            facets,
        }],
        SolverOptions::default(),
    )
    .unwrap_err();
    assert!(e.contains("a heat flux q alone does not fix the field's level"), "{e}");
}

#[test]
fn flux_bands_superpose() {
    // Two bands stacked on the same face add their q to the load vector; one band with
    // q = 0.5 + 1.5 must land on the same field to roundoff (the load sums are not
    // bit-identical, so exact equality is not the bar).
    let mesh = cube_mesh();
    let d: Vec<Option<f64>> = mesh
        .nodes
        .iter()
        .map(|p| if p[0] == 0.0 { Some(100.0) } else { None })
        .collect();
    let wall = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    let (t0, _) = solve_conduction(
        &mesh,
        &d,
        &[],
        &[
            HeatFluxBc {
                q: 0.5,
                facets: wall.clone(),
            },
            HeatFluxBc {
                q: 1.5,
                facets: wall,
            },
        ],
        SolverOptions::default(),
    )
    .unwrap();
    let wall2 = hull_facets(&mesh)
        .into_iter()
        .filter(|f| f[..3].iter().all(|&c| mesh.nodes[c as usize][0] == 1.0))
        .collect::<Vec<_>>();
    let (t1, _) = solve_conduction(
        &mesh,
        &d,
        &[],
        &[HeatFluxBc {
            q: 2.0,
            facets: wall2,
        }],
        SolverOptions::default(),
    )
    .unwrap();
    for (a, b) in t0.iter().zip(t1.iter()) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }
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
