//! Face selectors on derived solids, end to end (goal M7, step 5).
//!
//! V10's stated gap: "The swept solid exposes no face selector of its own — a Face on an
//! Extend node falls through to the primitive branch, so the skirt's side walls cannot be
//! labelled today." This test closes it: an imported box is extended downward 10 mm
//! through a face-rule extend, a region selects `ext1.nz` (the skirt's swept face), and
//! `region_facets` claims the skirt's tip facets on the P2 surface — the same path the
//! app's setup lens and the solver's `fixed_mask`/`case_force` use. A support on that
//! region clamps the skirt wall.
//!
//! What is measured:
//! - the `ext1.nz` region claims facets on the skirt's tip (the −z face of the swept
//!   solid), and none on the parent's top or the skirt's side walls;
//! - the claimed area matches the skirt's footprint (the swept hull's cross-section);
//! - a support on `ext1.nz` clamps the skirt tip: `fixed_mask` marks nodes there;
//! - `ext1.all` and `ext1.side` validate as errors (a derived solid exposes its swept
//!   face, not `all`/`side`).

use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::{analyse, fixed_mask, region_facets, Facet, Resolution};
use fieldsmith_doc::{Document, Face, Load, LoadKind, NodeKind, Region, Selector, Support};
use fieldsmith_field::expr;
use fieldsmith_mesh::stuff;
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::P2Mesh;

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// ASCII STL of a 10×10×10 mm cube at the origin, outward-wound.
fn cube_stl() -> Vec<u8> {
    let lo = [0.0, 0.0, 0.0];
    let hi = [10.0, 10.0, 10.0];
    let v = |i: usize, j: usize, k: usize| {
        format!(
            "vertex {} {} {}",
            if i == 0 { lo[0] } else { hi[0] },
            if j == 0 { lo[1] } else { hi[1] },
            if k == 0 { lo[2] } else { hi[2] }
        )
    };
    type Quad = (
        (usize, usize, usize),
        (usize, usize, usize),
        (usize, usize, usize),
        (usize, usize, usize),
    );
    let quads: [Quad; 6] = [
        ((0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)), // x−
        ((1, 1, 0), (1, 1, 1), (1, 0, 1), (1, 0, 0)), // x+
        ((1, 0, 0), (1, 0, 1), (0, 0, 1), (0, 0, 0)), // y−
        ((0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)), // y+
        ((0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)), // z−
        ((0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)), // z+
    ];
    let mut s = String::from("solid cube\n");
    for quad in quads {
        for tri in [[quad.0, quad.1, quad.2], [quad.0, quad.2, quad.3]] {
            s.push_str("facet normal 0 0 0\nouter loop\n");
            for c in tri {
                s.push_str(&v(c.0, c.1, c.2));
                s.push('\n');
            }
            s.push_str("endloop\nendfacet\n");
        }
    }
    s.push_str("endsolid cube\n");
    s.into_bytes()
}

/// The P2 surface of the compiled document as boundary facets (the same construction
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

/// Import the cube, extend its bottom face downward 10 mm, and build the document:
/// the imported mesh, a `bottom` region on the mesh's −z face feeding the extend, and
/// a `skirt` region on the extend's −z face (the swept tip). A support clamps the
/// skirt tip; a load pulls the cube's +z face up.
fn skirt_doc() -> Document {
    let out = import_mesh(
        "stl",
        &cube_stl(),
        &ImportOpts {
            units: None,
            up: None,
            voxel: Some(1.0),
            src: "cube.stl",
            taken: &[],
        },
    )
    .expect("STL import succeeds");
    let node = out.nodes.into_iter().next().unwrap();
    let id = node.id.clone();
    let NodeKind::Mesh { bbox, .. } = &node.kind else {
        panic!("expected a mesh node");
    };
    let bbox = *bbox;

    let mut doc = Document {
        name: "Skirt clamp".into(),
        ..Default::default()
    };
    doc.root.push(node);
    doc.root.push(fieldsmith_doc::Node {
        id: "skirt".into(),
        op: fieldsmith_doc::Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Default::default(),
        kind: NodeKind::Extend {
            region: "bottom".into(),
            dir: [0.0, 0.0, -1.0],
            len: 10.0,
        },
    });
    doc.regions.push(Region {
        id: "bottom".into(),
        selectors: vec![Selector {
            shape: id.clone(),
            face: Face::Nz,
            patch: None,
        }],
    });
    doc.regions.push(Region {
        id: "tip".into(),
        selectors: vec![Selector {
            shape: "skirt".into(),
            face: Face::Nz,
            patch: None,
        }],
    });
    doc.regions.push(Region {
        id: "top".into(),
        selectors: vec![Selector {
            shape: id.clone(),
            face: Face::Pz,
            patch: None,
        }],
    });
    doc.supports.push(Support {
        id: "clamp".into(),
        region: "tip".into(),
        fixed: [true; 3],
    });
    doc.loads.push(Load {
        id: "pull".into(),
        region: "top".into(),
        case: "Load 1".into(),
        kind: LoadKind::Force {
            vector: [0.0, 0.0, 100.0],
        },
    });
    doc.material = "steel200".into();
    doc.custom_materials.push(fieldsmith_doc::Material {
        id: "steel200".into(),
        name: "Steel (E 200 GPa)".into(),
        e: 200_000.0,
        nu: 0.3,
        yield_strength: 250.0,
        density: 7.85,
        min_wall: 0.8,
        source: "test".into(),
    });
    let _ = bbox;
    doc
}

#[test]
fn extend_face_selector_claims_the_skirt_tip() {
    let d = skirt_doc();
    assert!(d.validate().is_empty(), "validate: {:?}", d.validate());

    let h = 1.0;
    let (mesh, facets) = surface_facets(&d, h);
    println!("=== extend face selector on the skirt ===");
    println!("surface facets: {}", facets.len());

    // The `tip` region (skirt.nz) claims facets on the skirt's swept tip.
    let tip = region_facets(&d, "tip", &mesh, &facets, h).expect("tip region resolves");
    let tip_area: f64 = tip.iter().map(|&i| facets[i].area(&mesh)).sum();
    println!("tip (skirt.nz) facets: {} ({:.1} mm²)", tip.len(), tip_area);
    assert!(!tip.is_empty(), "the skirt.nz region claims facets");
    // The skirt's tip is the swept hull's footprint: the cube's 10×10 face plus the
    // half-spacing margin the sweep's hull adds, with the chamfer's corner rounding
    // trimming the corners. Measured 105.0 mm² — between the cube's face (100) and
    // the full margin footprint (11×11 = 121).
    assert!(
        tip_area > 100.0 && tip_area < 121.0 * 1.05,
        "tip area {tip_area:.1} mm² outside (100, 127) mm²"
    );
    // Every claimed facet's normal lies within the 45° face cone of −z (cos ≥ 0.7) —
    // the same cone `region_facets` applies. The corner-rounded edge facets tilt
    // between −z and the side, so the bound is the cone, not a pure −z normal.
    for &fi in &tip {
        let n = facets[fi].normal(&mesh);
        assert!(
            n[2] <= -0.7,
            "tip facet normal {n:?} outside the −z face cone"
        );
    }

    // The `top` region (mesh1.pz) claims the cube's top face — not the skirt.
    let top = region_facets(&d, "top", &mesh, &facets, h).expect("top region resolves");
    let top_area: f64 = top.iter().map(|&i| facets[i].area(&mesh)).sum();
    println!("top (mesh1.pz) facets: {} ({:.1} mm²)", top.len(), top_area);
    assert!(!top.is_empty(), "the mesh1.pz region claims facets");
    // The top face's claimed area is the cube's 10×10 face plus the half-spacing
    // margin the voxelised field's trilinear interpolation rounds outward at the
    // edges — measured 107.6 mm². Allow 10 %.
    assert!(
        (top_area - 100.0).abs() < 100.0 * 0.10,
        "top area {top_area:.1} mm² vs the cube's 100 mm² face"
    );
    for &fi in &top {
        let n = facets[fi].normal(&mesh);
        assert!(
            n[2] >= 0.7,
            "top facet normal {n:?} outside the +z face cone"
        );
    }

    // The tip and top regions are disjoint (different surfaces).
    let tip_set: std::collections::HashSet<usize> = tip.iter().copied().collect();
    for &fi in &top {
        assert!(
            !tip_set.contains(&fi),
            "facet {fi} claimed by both tip and top"
        );
    }

    // A support on the tip region clamps the skirt: fixed_mask marks nodes there, and
    // the analysis runs to completion (the skirt wall is clampable — V10's gap closed).
    let fixed = fixed_mask(&d, &mesh, &facets, h).expect("support resolves");
    let n_fixed = fixed.iter().filter(|&&f| f != 0).count();
    println!("fixed nodes on the skirt tip: {n_fixed}");
    assert!(n_fixed > 0, "the support clamps nodes on the skirt tip");

    // The full analysis runs with the skirt clamped and the cube pulled.
    let a = analyse(&d, Resolution::Size(h), now).expect("analysis with a clamped skirt");
    let c = &a.cases[0];
    println!(
        "solve: {} elements, {} DOF; peak vm {:.2} MPa, max disp {:.4} mm (converged {})",
        a.provenance.elements,
        a.provenance.dofs,
        c.max_vm_elem,
        c.max_displacement,
        c.solve.converged
    );
    assert!(c.solve.converged, "the skirt-clamped solve converges");
    assert!(c.max_displacement > 0.0, "the pull displaces the cube");
}

#[test]
fn extend_all_and_side_are_refused() {
    // `all` and `side` on an extend are validate errors (a derived solid exposes its
    // swept face, not `all`/`side`).
    let d = skirt_doc();
    let mut d_all = d.clone();
    d_all.regions.push(Region {
        id: "everything".into(),
        selectors: vec![Selector {
            shape: "skirt".into(),
            face: Face::All,
            patch: None,
        }],
    });
    let errs = d_all.validate();
    assert!(
        errs.iter().any(|e| e.contains("exposes its swept face")),
        "{errs:?}"
    );

    let mut d_side = d.clone();
    d_side.regions.push(Region {
        id: "wall".into(),
        selectors: vec![Selector {
            shape: "skirt".into(),
            face: Face::Side,
            patch: None,
        }],
    });
    let errs = d_side.validate();
    assert!(
        errs.iter().any(|e| e.contains("exposes its swept face")),
        "{errs:?}"
    );
}
