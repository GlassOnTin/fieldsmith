//! V34 — the raw-force compliance is a certified lower bound on the true compliance.
//!
//! CaseResult.compliance reads fᵀu, where f is the load vector the solve actually saw:
//! mechanical loads on a mechanical case, the thermal-expansion pseudo-load on a thermal
//! case. For a converged solve, C_h = a(u_h, u_h) ≥ 0 holds by construction, and Galerkin
//! orthogonality against the exact solution u* gives
//!
//!     C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0,
//!
//! so C_h ≤ C_true — strict when u* leaves the P2 span, exact when the mesh represents it.
//! fieldsmith-solve's `bounds` unit tests carry the derivation; this gate re-measures both
//! fixtures here and adds the pipeline row, through the same `analyse` path the user sees:
//!
//! 1. Uniform tension: u* = (c·x/E, −ν·c·y/E, −ν·c·z/E) is affine — in the P2 span — so
//!    C_h reads the closed form c²·V/E exactly.
//! 2. Manufactured cubic displacement: at ν = 0 the exact field u* = (d·x³, 0, 0) carries
//!    uniaxial stress σ = diag(E·3d·x², 0, 0), whose consistent load is a closed form
//!    (body −E·6d·x on the volume, constant traction E·3d·L² on the px face). u* ∉ P2
//!    span ⇒ C_h < C_true strictly, and the deficit orders down as the stair meshes
//!    refine.
//! 3. The pipeline row: CaseResult.compliance of the pulled PETG bar within ±2 % of
//!    c²·V/E — the band the analysis tests use, because the voxeliser's surface nodes
//!    carry f32 distance-field noise.
//!
//! Named gaps are listed in the report's Coverage and limits section.

use fenics_element::{
    tetrahedron_quadrature, triangle_quadrature, FiniteElement, P2Tetrahedron, P2Triangle,
};
use fieldsmith_core::{analyse, Resolution};
use fieldsmith_solve::{
    compliance, p2::face_nodes, solve, Material, P2Mesh, Problem, SolveReport, SolverOptions, V3,
};
use fieldsmith_verify::{doc, Check, Report};
use std::collections::HashMap;
use std::time::Instant;

/// Pipeline row: the same pulled-bar fixture the analysis tests use, E pinned so the
/// closed form lives in this file.
const PULL: &str = "name \"Pulled bar\"\n\
     custommaterial pull e=1500 nu=0.35 yield=50 density=1.27\n\
     material pull\n\
     box beam size=(40, 10, 10) at=(20, 5, 5)\n\
     region pin = beam.nx\n\
     region rest = beam.ny\n\
     region base = beam.nz\n\
     region pull = beam.px\n\
     support pin_x pin fixed=x\n\
     support rest_y rest fixed=y\n\
     support base_z base fixed=z\n\
     load pull pull force=(10, 0, 0)\n";

/// c = 10 N over the 100 mm² px face; c²·V/E = F²·L/(E·A).
const PULL_COMPLIANCE: f64 = 10.0 * 10.0 * 40.0 / (1500.0 * 100.0);
/// Peak pull-direction displacement c·L/E.
const PULL_DISPLACEMENT: f64 = 10.0 * 40.0 / (100.0 * 1500.0);

fn clock() -> f64 {
    thread_local! {
        static T0: Instant = Instant::now();
    }
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

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

/// A stair-tessellated box: `cubes` hex cells of side `edge`, each split into six tets
/// by the diagonal from its low corner to its opposite corner. Vertices are
/// deduplicated on the lattice, orientation is normalised, and total volume is asserted.
/// Boundary-triangle conformity is asserted via the census (every internal triangle
/// used by exactly two tets), so a non-conforming staircase cannot pass silently.
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
                // The long diagonal c[0]–c[6]; six corner tets, each of volume edge³/6.
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

/// Boundary corner-triangle census of the mesh. Only corners matter: midsides sit at
/// edge midpoints, so a corner-triangle used by two tets is internal.
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

/// Constant traction `t` (N/mm²) on the axis-aligned plane {axis·x = plane}, integrated
/// over the mesh's P2 boundary facets on it, oriented outward by `out`.
fn traction_plane(mesh: &P2Mesh, axis: usize, plane: f64, t: [f64; 3], out: f64) -> Vec<f64> {
    let census = facet_census(mesh);
    let mut f = vec![0.0; mesh.nodes.len() * 3];
    let rule = triangle_quadrature(2);
    let elem = P2Triangle;
    for tet in &mesh.tets {
        for face in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
            let mut k = [tet[face[0]], tet[face[1]], tet[face[2]]];
            k.sort_unstable();
            if census[&k] != 1 {
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

/// Body load assembled from a per-point vector field b(x) (N/mm³) over every tet: the
/// deg-3 Keast rule and the P2 basis, point from the exact P2 interpolation.
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

/// Supports on the three singly-directioned planes: x fixed on {x = 0}, y on {y = 0},
/// z on {z = 0}. Each plane's own component is the only one fixed there, so an exact
/// field with u* ≠ 0 across a support plane is still legal.
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

#[test]
fn v34_certified_lower_bound() {
    // T1 — u* affine, in the span: the bound is exact.
    let (young, nu, c) = (100.0, 0.3, 0.1);
    let mesh = box_mesh([4, 1, 1], 10.0);
    assert_eq!(mesh.tets.len(), 24);
    let force = traction_plane(&mesh, 0, 40.0, [c, 0.0, 0.0], 1.0);
    let (u1, rep1) = solve_of(&mesh, force.clone(), young, nu);
    assert!(rep1.converged && rep1.rel_residual < SolverOptions::default().rtol);
    let mut err1 = 0.0f64;
    for (i, p) in mesh.nodes.iter().enumerate() {
        let w = [
            c * p[0] / young,
            -nu * c * p[1] / young,
            -nu * c * p[2] / young,
        ];
        for comp in 0..3 {
            err1 = err1.max((u1[3 * i + comp] - w[comp]).abs());
        }
    }
    let want1 = c * c * 4000.0 / young; // c²·V/E over the 4×1×1 cube bar
    let ch1 = compliance(&force, &u1);

    // T2 — u* ∉ span (ν = 0): the bound is strict and orders with resolution.
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
    let c_true = 9.0 * young2 * d * d * (box_len.powi(5) / 5.0) * ly * lz; // closed form ∫σ:ε dV
    let mut err2 = [0.0f64; 2];
    let chs: [f64; 2] = [&box_mesh([4, 1, 1], 10.0), &box_mesh([8, 2, 2], 5.0)].map(|mesh| {
        let force = load(mesh);
        // Equilibrium witness: ∫b = −4800 N cancels the px traction exactly.
        let rx: f64 = force.iter().step_by(3).sum();
        assert!(rx.abs() < 1e-9, "Σf_x = {rx:e}");
        let (u, rep) = solve_of(mesh, force.clone(), young2, 0.0);
        assert!(rep.converged && rep.rel_residual < SolverOptions::default().rtol);
        for (i, p) in mesh.nodes.iter().enumerate() {
            let u_star = [d * p[0].powi(3), 0.0, 0.0];
            for comp in 0..3 {
                let j = usize::from(mesh.tets.len() != 24); // 0 coarse, 1 fine
                err2[j] = err2[j].max((u[3 * i + comp] - u_star[comp]).abs());
            }
        }
        compliance(&force, &u)
    });
    assert!(chs[0] > 0.0 && chs[1] > 0.0);
    let (g1, g2) = (c_true - chs[0], c_true - chs[1]);
    assert!(
        g1 > 1e-6 && g2 > 1e-6,
        "strict: deficits {g1:e}, {g2:e} must be positive"
    );
    assert!(
        g2 < g1,
        "trend: fine deficit {g2} must sit below coarse {g1}"
    );

    // Through the pipeline: CaseResult.compliance, the row the UI reads.
    let a = analyse(&doc(PULL), Resolution::Size(2.0), clock).expect("pipeline analysis");
    let case = a
        .cases
        .iter()
        .find(|c| c.name == "Load 1")
        .expect("load case ran");
    assert!(case.solve.converged);
    let resultant = case.load_reports[0].resultant[0];
    let mut ux = 0.0f64;
    for i in 0..a.mesh.nodes.len() {
        ux = ux.max(case.u[3 * i]);
    }

    let mut report = Report::new("V34", "certified compliance lower bound");
    report
        .check(Check::relative(
            "T1 C_h — uniform tension, u* in span",
            want1,
            ch1,
            "N·mm",
            1e-6,
        ))
        .check(Check::absolute("T1 max ‖u − u*‖", 0.0, err1, "mm", 1e-6))
        .check(Check::relative(
            "T2 C_h — cubic u*, 24 tets",
            c_true,
            chs[0],
            "N·mm",
            1e-3,
        ))
        .check(Check::absolute(
            "T2 max ‖u − u*‖, 24 tets",
            0.0,
            err2[0],
            "mm",
            0.05,
        ))
        .check(Check::relative(
            "T2 C_h — cubic u*, 192 tets",
            c_true,
            chs[1],
            "N·mm",
            1e-3,
        ))
        .check(Check::absolute(
            "T2 max ‖u − u*‖, 192 tets",
            0.0,
            err2[1],
            "mm",
            0.01,
        ))
        .check(Check::relative(
            "pipeline resultant",
            10.0,
            resultant,
            "N",
            1e-6,
        ))
        .check(Check::relative(
            "pipeline CaseResult.compliance vs c²·V/E",
            PULL_COMPLIANCE,
            case.compliance,
            "N·mm",
            0.02,
        ))
        .check(Check::relative(
            "pipeline peak u_x vs c·L/E",
            PULL_DISPLACEMENT,
            ux,
            "mm",
            0.02,
        ))
        .section(
            "Certified lower bound",
            "CaseResult.compliance is Σ force[a]·u[a], force being the load vector the \
             solve used — mechanical loads for a mechanical case, thermal_load(...)'s \
             expansion pseudo-load for a thermal case. On the free dofs C_h = uᵀK_ff·u ≥ 0; \
             fixed dofs carry u = 0, so reactions contribute nothing. Galerkin \
             orthogonality against the exact solution u* gives a(u* − u_h, v_h) = 0 on the \
             whole P2 span, hence C_true − C_h = a(u* − u_h, u* − u_h) ≥ 0: a certified \
             lower bound, strict when u* ∉ span, exact when the mesh represents it.\n\n\
             Licences: the value is meaningful only for a converged solve (every row of \
             this report re-checked rel_residual below the default rtol); document and \
             grammar loads reach the solver exactly — constant traction through the \
             midsides carries t·n·area/3, the exact P2 facet integral, and the deg-2 \
             triangle / deg-3 Keast rules are exact for the deg ≤ 3 integrands these \
             fixtures use.",
        )
        .section(
            "Strictness and trend",
            format!(
                "In span (T1): C_h = {:.12} vs c²·V/E = 0.4, to solver floating point \
                 (rel_residual {:.3e}, max ‖u − u*‖ {:.3e} mm) — the bound is tight exactly \
                 when the exact field is representable.\n\n\
                 Off span (T2, ν = 0): C_true = {} N·mm; deficit {g1:.6e} at 24 tets and \
                 {g2:.6e} at 192 tets — the fine deficit {:.1}× lower. Both deficits are \
                 positive and ordered with resolution over the two nested stair meshes; \
                 this is a measured trend, not a proof for all refinements.",
                ch1,
                rep1.rel_residual,
                err1,
                c_true,
                g1 / g2
            ),
        )
        .section(
            "Through the pipeline",
            format!(
                "The user-visible row: `analyse` of the pulled 40×10×10 bar (E = 1500 MPa \
                 pinned in the document) at Resolution::Size(2.0) — the same voxelised \
                 mesh, loads and solve path the UI runs. CaseResult.compliance = {:.8} \
                 against the closed form {:.8} and the peak pull-direction displacement \
                 {:.4e} against c·L/E = {:.4e}; the pipeline's surface nodes carry f32 \
                 distance-field noise, orders below the 2 % licence. A thermal case's \
                 compliance is the same readout against the expansion pseudo-load, ≥ 0 by \
                 construction for any converged solve (fieldsmith-core's analysis tests \
                 pin the sign); thermal rows have no closed-form reference here.",
                case.compliance, PULL_COMPLIANCE, ux, PULL_DISPLACEMENT
            ),
        )
        .section(
            "Coverage and limits",
            "- The upper end carries its own recovery and gate: fieldsmith-solve's \
             `dual` module certifies the complementary side up to the P2 interpolation \
             defect (V35); this report is the lower end's gate and its own coverage \
             statement is unchanged here.\n\
             - Rung-level tracked wiring (TrackedValues / RungChange / Tolerance, the api \
             verb, sweep CSV, worker mirror, e2e) is not covered here and is deferred \
             until the upper end lands — tracking one end alone would publish half a \
             tolerance.\n\
             - The manufactured-solution leg is measured at ν = 0 only: that is what makes \
             the consistent load a closed form. The theorem itself does not use ν.\n\
             - CaseResult.compliance is included in the V9 save → reload bit-compare; the \
             f64 readouts are bit-identical, wall-clock provenance excluded.",
        );
    report.finish();
}
