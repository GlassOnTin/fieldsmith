//! Face-local frames and distributed loads (M11).
//!
//! What is measured, each on the real analysis path:
//! - jacobi3 eigen-residuals on symmetric 3×3 (the plane fit's engine);
//! - the frame on a flat region: outward normal, the deterministic x rule, tiny RMS,
//!   and a readout of the in-plane extent;
//! - a curved region (a full cylinder's side) refused by name at the stated bound
//!   (10 % of extent) — the load errors instead of silently flattening;
//! - a constant density reproduces the `force=` load exactly (same facets, same
//!   traction, same resultant and moment — the equivalence the P2 scheme needs);
//! - `total=` rescales to the stated resultant;
//! - a near-zero signed resultant (cancellation) is refused for rescale, naming the
//!   numbers; a divergent density (`1/0`) is caught at a facet, naming its coordinates.

use fieldsmith_core::case_force_report;
use fieldsmith_core::region_facets;
use fieldsmith_core::regions::{face_frame, jacobi3};
use fieldsmith_core::Facet;
use fieldsmith_doc::{
    Document, Face, Load, LoadDir, LoadKind, Node, NodeKind, Op, Region, Selector,
};
use fieldsmith_field::expr;
use fieldsmith_mesh::stuff;
use fieldsmith_solve::{p2::face_nodes, P2Mesh};

/// The P2 surface of the compiled document as boundary facets (the construction
/// `analyse` uses internally).
fn surface_facets(doc: &Document, h: f64) -> (P2Mesh, Vec<Facet>) {
    let body = expr::compile_body(doc);
    let b = expr::bounds(&body);
    let (mut lin, _) = stuff(|p| expr::eval(&body, p).v, b.lo, b.hi, h);
    lin.compact();
    let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
    let facets: Vec<Facet> = lin
        .boundary()
        .iter()
        .map(|(f, t)| {
            let tet = &mesh.tets[*t as usize];
            let local = f.map(|v| {
                tet[..4]
                    .iter()
                    .position(|&x| x == v)
                    .expect("face vertex in tet")
            });
            Facet {
                nodes: face_nodes(tet, local),
            }
        })
        .collect();
    (mesh, facets)
}

fn push(doc: &mut Document, id: &str, kind: NodeKind) {
    doc.root.push(Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Default::default(),
        material: None,
        color: None,
        kind,
    });
}

fn region(doc: &mut Document, id: &str, shape: &str, face: Face) {
    doc.regions.push(Region {
        id: id.into(),
        selectors: vec![Selector {
            shape: shape.into(),
            face,
            patch: None,
        }],
    });
}

fn load(doc: &mut Document, id: &str, region: &str, kind: LoadKind) {
    doc.loads.push(Load {
        id: id.into(),
        region: region.into(),
        case: "Load 1".into(),
        kind,
    });
}

/// Box 40×10×10 mm centred on the origin: top face z = +5, x spans ±20.
fn box_doc() -> Document {
    let mut doc = Document {
        name: "Dist test box".into(),
        ..Document::default()
    };
    push(
        &mut doc,
        "b",
        NodeKind::Box {
            size: [40.0, 10.0, 10.0],
            round: 0.0,
        },
    );
    region(&mut doc, "top", "b", Face::Pz);
    doc.material = "petg".into();
    doc
}

/// Cylinder r = 12.5, h = 25 along z, centred: its `side` region is a full curved band.
fn cyl_doc() -> Document {
    let mut doc = Document {
        name: "Dist test cyl".into(),
        ..Document::default()
    };
    push(
        &mut doc,
        "c",
        NodeKind::Cylinder {
            radius: 12.5,
            height: 25.0,
            round: 0.0,
        },
    );
    region(&mut doc, "side", "c", Face::Side);
    doc.material = "petg".into();
    doc
}

fn load_kind(src: &str, dir: LoadDir, total: Option<f64>) -> LoadKind {
    LoadKind::Distributed {
        expr: src.into(),
        dir,
        total,
    }
}

#[test]
fn jacobi3_returns_sorted_eigenpairs_with_small_residuals() {
    let a = [[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 5.0]];
    let (eig, vecs) = jacobi3(&a);
    assert!(
        (eig[0] - 5.0).abs() < 1e-9 && (eig[1] - 3.0).abs() < 1e-9 && (eig[2] - 1.0).abs() < 1e-9,
        "eigenvalues {eig:?}"
    );
    for idx in 0..3 {
        let lam = eig[idx];
        let v = vecs[idx];
        let res: f64 = (0..3)
            .map(|r| a[r][0] * v[0] + a[r][1] * v[1] + a[r][2] * v[2] - lam * v[r])
            .map(f64::abs)
            .sum();
        assert!(res < 1e-9, "residual {res} for eigenvalue {lam}");
    }
    assert!(eig[0] >= eig[1] && eig[1] >= eig[2], "descending");

    // A non-trivially-scaled symmetric matrix: residuals must again be tiny.
    let b = [[4.0, 1.0, 0.3], [1.0, -2.0, 3.0], [0.3, 3.0, 1.0]];
    let (eig, vecs) = jacobi3(&b);
    assert!(eig[0] >= eig[1] && eig[1] >= eig[2], "{eig:?}");
    for idx in 0..3 {
        let lam = eig[idx];
        let v = vecs[idx];
        let res: f64 = (0..3)
            .map(|r| b[r][0] * v[0] + b[r][1] * v[1] + b[r][2] * v[2] - lam * v[r])
            .map(f64::abs)
            .sum();
        assert!(res < 1e-9, "residual {res} for eigenvalue {lam}");
    }
}

#[test]
fn face_frame_on_a_flat_top_face() {
    let d = box_doc();
    let h = 1.0;
    let (mesh, facets) = surface_facets(&d, h);
    let fs = region_facets(&d, "top", &mesh, &facets, h).expect("top region resolves");
    assert!(!fs.is_empty(), "the top face has facets");
    let frame = face_frame(&mesh, &facets, &fs);

    // Outward normal up; x = world x (the tie between x and y breaks to x); y = n × x.
    for (k, want) in [0.0, 0.0, 1.0].iter().enumerate() {
        assert!((frame.n[k] - want).abs() < 1e-6, "n[{k}] = {}", frame.n[k]);
    }
    for (k, want) in [1.0, 0.0, 0.0].iter().enumerate() {
        assert!((frame.x[k] - want).abs() < 1e-6, "x[{k}] = {}", frame.x[k]);
    }
    for (k, want) in [0.0, 1.0, 0.0].iter().enumerate() {
        assert!((frame.y[k] - want).abs() < 1e-6, "y[{k}] = {}", frame.y[k]);
    }
    // The centroids sit on z = +5 to mesh precision: the fit residual is tiny.
    assert!(frame.rms < 1e-6, "rms = {}", frame.rms);
    // The extent readout tracks the face's long side (40 mm) within mesh rounding.
    assert!(
        (frame.extent - 40.0).abs() < 40.0 * 0.2,
        "extent = {} mm on a 40 mm face",
        frame.extent
    );
    // The frame passes its own stated bound.
    assert!(
        frame.rms <= 0.1 * frame.extent,
        "rms {} exceeds {:#} of extent",
        frame.rms,
        0.1 * frame.extent
    );
}

#[test]
fn curved_side_is_refused_by_name() {
    let mut d = cyl_doc();
    let h = 1.0;
    let (mesh, facets) = surface_facets(&d, h);
    let fs = region_facets(&d, "side", &mesh, &facets, h).expect("side region resolves");
    assert!(!fs.is_empty(), "the cylinder's side has facets");
    let frame = face_frame(&mesh, &facets, &fs);
    // The residual of a full lateral band exceeds the stated bound (r = 12.5 mm of
    // sag over a ~30 mm spread): refused before any traction is built.
    assert!(
        frame.rms > 0.1 * frame.extent,
        "expected refusal: rms {} vs bound {}",
        frame.rms,
        0.1 * frame.extent
    );
    load(
        &mut d,
        "w",
        "side",
        load_kind("1", LoadDir::Global { v: [0.0, 0.0, 1.0] }, None),
    );
    let err =
        case_force_report(&d, &mesh, &facets, "Load 1", h).expect_err("the curved side is refused");
    assert!(
        err.0.contains("curves away from its fitted plane"),
        "{}",
        err.0
    );
    assert!(err.0.contains("'side'"), "{}", err.0);
}

#[test]
fn constant_density_equals_the_force_load() {
    // 0.4 N/mm² downward over the 40×10 top face ⇔ force = (0, 0, −160) N uniform.
    let mut d_f = box_doc();
    load(
        &mut d_f,
        "push",
        "top",
        LoadKind::Force {
            vector: [0.0, 0.0, -160.0],
        },
    );
    let mut d_q = box_doc();
    load(
        &mut d_q,
        "push",
        "top",
        load_kind(
            "0.4",
            LoadDir::Global {
                v: [0.0, 0.0, -1.0],
            },
            None,
        ),
    );
    let (m_f, fc_f) = surface_facets(&d_f, 1.0);
    let (m_q, fc_q) = surface_facets(&d_q, 1.0);
    assert_eq!(m_f.nodes.len(), m_q.nodes.len(), "same mesh both docs");
    let (f_f, r_f, _) = case_force_report(&d_f, &m_f, &fc_f, "Load 1", 1.0).unwrap();
    let (f_q, r_q, _) = case_force_report(&d_q, &m_q, &fc_q, "Load 1", 1.0).unwrap();
    let scale = f_f.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-300);
    let dmax = f_f
        .iter()
        .zip(&f_q)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(dmax < 1e-9 * scale, "nodal force diff {dmax} vs {scale}");
    // The reports agree too: resultant and moment.
    let (rf, rq) = (&r_f[0], &r_q[0]);
    for k in 0..3 {
        assert!(
            (rf.resultant[k] - rq.resultant[k]).abs() < 1e-6 * 160.0,
            "resultant[{}] {} vs {}",
            k,
            rf.resultant[k],
            rq.resultant[k]
        );
        assert!(
            (rf.moment[k] - rq.moment[k]).abs() < 1e-6 * 160.0 * 10.0,
            "moment[{}] {} vs {}",
            k,
            rf.moment[k],
            rq.moment[k]
        );
    }
    assert!(
        (rf.resultant[2] + 160.0).abs() < 1e-6 * 160.0,
        "the force load's resultant returns its vector ({})",
        rf.resultant[2]
    );
}

#[test]
fn total_rescales_to_the_stated_resultant() {
    let mut d = box_doc();
    // One-sided band: the right half of the top face at 0.5 N/mm² (a 20×10 patch,
    // 100 N unscaled), rescaled to 30 N resultant.
    load(
        &mut d,
        "half",
        "top",
        load_kind(
            "0 < x < 20 -> 0.5",
            LoadDir::Global {
                v: [0.0, 0.0, -1.0],
            },
            Some(30.0),
        ),
    );
    let (mesh, facets) = surface_facets(&d, 1.0);
    let (_, r, _) = case_force_report(&d, &mesh, &facets, "Load 1", 1.0).unwrap();
    let rr = &r[0];
    assert!(
        (rr.resultant[2] + 30.0).abs() < 30.0 * 1e-9,
        "resultant[2] = {}",
        rr.resultant[2]
    );
    assert_ne!(rr.scale, 1.0, "the rescale applied");
    assert!(
        (rr.unscaled - 100.0).abs() < 100.0 * 0.2,
        "unscaled = {} N (a 20×10 patch at 0.5 N/mm²)",
        rr.unscaled
    );
    assert!(rr.fit_rms.is_some() && rr.fit_extent.is_some());
}

#[test]
fn zero_resultant_rescale_is_refused() {
    // fn = x over a symmetric face: the signed resultant cancels, so a total= rescale
    // would need an unbounded factor — refused, with the numbers named.
    let mut d = box_doc();
    load(
        &mut d,
        "cancel",
        "top",
        load_kind(
            "x",
            LoadDir::Global {
                v: [0.0, 0.0, -1.0],
            },
            Some(10.0),
        ),
    );
    let (mesh, facets) = surface_facets(&d, 1.0);
    let err = case_force_report(&d, &mesh, &facets, "Load 1", 1.0)
        .expect_err("a cancelling density refuses rescale");
    assert!(err.0.contains("cannot rescale"), "{}", err.0);
    assert!(err.0.contains("almost cancels"), "{}", err.0);
}

#[test]
fn divergent_density_is_caught_at_a_facet() {
    // 1/0 survives compile (static checks live in validate) — the per-facet finite
    // guard is the last line, and it names the facet.
    let mut d = box_doc();
    load(
        &mut d,
        "bad",
        "top",
        load_kind(
            "1 / 0",
            LoadDir::Global {
                v: [0.0, 0.0, -1.0],
            },
            None,
        ),
    );
    let (mesh, facets) = surface_facets(&d, 1.0);
    let err = case_force_report(&d, &mesh, &facets, "Load 1", 1.0)
        .expect_err("a divergent density is caught");
    assert!(err.0.contains("not finite at facet"), "{}", err.0);
    assert!(err.0.contains("diverges"), "{}", err.0);
}
