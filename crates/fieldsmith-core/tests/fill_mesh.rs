//! Graded fill on an imported-mesh document, measured (goal M7, step 4).
//!
//! V10 declared `Filled` (graded lattice) on mesh-node docs untested: "bounds/eval paths are
//! shape-agnostic; likely composes, not measured". This test closes that gap: a 40×10×10 mm
//! box imported through the real STL chain (read → weld → segment → voxelize → hydrate →
//! document node) is clamped on its −x face and pulled on its +x face, filled with a graded
//! gyroid-sheet lattice, and the full graded pipeline runs — solid solve, grading pass,
//! graded re-solve, mass integration.
//!
//! What is measured (and asserted):
//! - the graded re-solve converges and deflects more than the solid reference (the lattice
//!   is softer);
//! - all three element classes appear (skin/solid, lattice, void pockets);
//! - per-element moduli stay in (0, E_solid] and follow the φ mapping;
//! - masses: the graded part is lighter than the solid, both positive, and the solid mass
//!   lands within a few percent of the closed-form 40·10·10 mm³ × density;
//! - the achieved design density stays inside the law's measured band [0.18, 0.40];
//! - the mesh's patch labels still claim their faces after the fill (the fill wraps the
//!   body but does not disturb region/facet matching on the solid surface).
//!
//! Every measured number is printed so the report is honest about what was run.

use fieldsmith_core::fill::{graded, CLASS_LATTICE, CLASS_SOLID};
use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::regions::region_facets;
use fieldsmith_core::{material_of, Resolution};
use fieldsmith_doc::{Document, Face, Region, Selector};
use fieldsmith_field::expr;
use fieldsmith_mesh::stuff;
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::P2Mesh;

/// ASCII STL of an axis-aligned box, outward-wound (same fixture shape as the import
/// tests use).
fn box_stl(lo: [f64; 3], hi: [f64; 3]) -> Vec<u8> {
    let v = |i: usize, j: usize, k: usize| {
        format!(
            "vertex {} {} {}",
            if i == 0 { lo[0] } else { hi[0] },
            if j == 0 { lo[1] } else { hi[1] },
            if k == 0 { lo[2] } else { hi[2] }
        )
    };
    #[rustfmt::skip]
    #[allow(clippy::type_complexity)]
    let quads: [((usize, usize, usize), (usize, usize, usize), (usize, usize, usize), (usize, usize, usize)); 6] = [
        ((0,0,0), (0,0,1), (0,1,1), (0,1,0)), // x−
        ((1,1,0), (1,1,1), (1,0,1), (1,0,0)), // x+
        ((1,0,0), (1,0,1), (0,0,1), (0,0,0)), // y−
        ((0,1,0), (0,1,1), (1,1,1), (1,1,0)), // y+
        ((0,0,0), (0,1,0), (1,1,0), (1,0,0)), // z−
        ((0,0,1), (1,0,1), (1,1,1), (0,1,1)), // z+
    ];
    let mut s = String::from("solid box\n");
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
    s.push_str("endsolid box\n");
    s.into_bytes()
}

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Import the 40×10×10 bar through the STL chain and build the document: the imported
/// mesh node, cap/tip regions on its −x/+x faces (the geometric face rule), a clamp on
/// the cap, a 1200 N pull on the tip, and a graded gyroid-sheet fill.
fn mesh_bar_doc() -> Document {
    let bytes = box_stl([0.0, 0.0, 0.0], [40.0, 10.0, 10.0]);
    let out = import_mesh(
        "stl",
        &bytes,
        &ImportOpts {
            units: None,
            up: None,
            voxel: Some(1.0),
            src: "bar.stl",
            taken: &[],
            step_chord: None,
        },
    )
    .expect("STL import succeeds");
    assert_eq!(out.nodes.len(), 1, "one part in the STL");
    let node = out.nodes.into_iter().next().unwrap();
    let id = node.id.clone();

    let mut doc = Document {
        name: "Mesh fill bar".into(),
        ..Default::default()
    };
    doc.root.push(node);
    doc.regions.push(Region {
        id: "cap".into(),
        selectors: vec![Selector {
            shape: id.clone(),
            face: Face::Nx,
            patch: None,
        }],
    });
    doc.regions.push(Region {
        id: "tip".into(),
        selectors: vec![Selector {
            shape: id.clone(),
            face: Face::Px,
            patch: None,
        }],
    });
    doc.supports.push(fieldsmith_doc::Support {
        id: "clamp".into(),
        region: "cap".into(),
        fixed: [true; 3],
    });
    doc.loads.push(fieldsmith_doc::Load {
        id: "pull".into(),
        region: "tip".into(),
        case: "Load 1".into(),
        kind: fieldsmith_doc::LoadKind::Force {
            vector: [1200.0, 0.0, 0.0],
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
    doc.fill = Some(fieldsmith_doc::Fill {
        family: fieldsmith_doc::Family::Gyroid,
        mode: fieldsmith_doc::FillMode::Sheet,
        cell: 5.0,
        skin: 1.0,
        rho_min: 0.2,
        rho_max: 0.4,
        curve: 1.0,
        min_wall: 0.3,
        protect: vec![],
        margin: 0.0,
        case: None,
    });
    doc
}

#[test]
fn graded_fill_on_a_mesh_document_is_measured() {
    let d = mesh_bar_doc();
    assert!(d.validate().is_empty(), "validate: {:?}", d.validate());

    let (base, g) = graded(&d, Resolution::Size(1.5), now).expect("graded analysis on a mesh doc");
    println!("=== graded fill on an imported mesh ===");
    println!(
        "solid: {} elements, {} DOF, h {:.2} mm",
        base.provenance.elements, base.provenance.dofs, base.provenance.element_size
    );
    let solid = &base.cases[0];
    let graded_case = &g.cases[0];
    println!(
        "solid: peak vm {:.2} MPa, max disp {:.4} mm",
        solid.max_vm_elem, solid.max_displacement
    );
    println!(
        "graded: peak vm {:.2} MPa, max disp {:.4} mm (converged {})",
        graded_case.max_vm_elem, graded_case.max_displacement, graded_case.solve.converged
    );
    println!(
        "mass: graded {:.2} g vs solid {:.2} g ({:.1}% lighter)",
        g.mass_g,
        g.solid_mass_g,
        100.0 * (1.0 - g.mass_g / g.solid_mass_g)
    );
    println!(
        "design density {:.3}..{:.3}, sigma_ref {:.2} MPa",
        g.rho_design[0], g.rho_design[1], g.sigma_ref
    );

    // The graded re-solve converges.
    assert!(
        graded_case.solve.converged,
        "graded re-solve did not converge: {:?}",
        graded_case.solve
    );
    assert_eq!(
        g.cases.len(),
        base.cases.len(),
        "one graded case per solid case"
    );

    // The lattice is softer than the solid bar, so it deflects more.
    assert!(
        graded_case.max_displacement > solid.max_displacement,
        "graded deflection {:.6} vs solid {:.6}",
        graded_case.max_displacement,
        solid.max_displacement
    );

    // All three element classes appear: skin/solid, lattice, void pockets.
    let (mut n_solid, mut n_lat, mut n_void) = (0usize, 0usize, 0usize);
    for &k in &g.class {
        match k {
            CLASS_SOLID => n_solid += 1,
            CLASS_LATTICE => n_lat += 1,
            _ => n_void += 1,
        }
    }
    println!("classes: solid {n_solid}, lattice {n_lat}, void {n_void}");
    assert!(
        n_solid > 0 && n_lat > 0 && n_void > 0,
        "solid={n_solid} lattice={n_lat} void={n_void}"
    );

    // Per-element moduli: strictly positive, at most the solid modulus, and exactly the
    // φ mapping (self-consistency of element_stiffness). At h = 1.5 and cell = 5
    // (h/cell = 0.3 < 1) the engine's blend weight clamps to 0, so the mapping is pure
    // Voigt: young = E·φ.
    let mat = material_of(&d).unwrap();
    for i in 0..g.young.len() {
        let y = g.young[i];
        let phi = g.rho_elem[i];
        assert!((0.0..=1.0).contains(&phi), "rho_elem[{i}] = {phi}");
        assert!(y > 0.0 && y <= mat.e * (1.0 + 1e-9), "young[{i}] = {y}");
        let voigt = mat.e * phi.max(1e-6);
        assert!(
            (y - voigt).abs() < 1e-6 * mat.e,
            "young[{i}] = {y} does not match Voigt E·φ ({voigt}) at φ = {phi}"
        );
    }

    // The achieved design density stays inside the law's measured band [0.18, 0.40].
    // (cell 5, wall 0.3: sheet walls at ρmin are t = ρ·cell/3.09 ≈ 0.32 mm, above the
    // authored minimum, so no min-wall clamp binds; ρmax = 0.4 is the band top.)
    assert!(
        g.rho_design[0] >= 0.17 && g.rho_design[1] <= 0.42,
        "achieved design density {:.4}..{:.4} strays outside the law's measured band [0.18, 0.40]",
        g.rho_design[0],
        g.rho_design[1]
    );

    // Masses: the graded part is lighter than the solid, both positive, and the solid mass
    // lands within a few percent of the closed-form 40·10·10 mm³ × 7.85 g/cm³ ≈ 31.4 g.
    assert!(g.solid_mass_g > 0.0, "solid mass {:.6}", g.solid_mass_g);
    assert!(
        g.mass_g > 0.0 && g.mass_g < g.solid_mass_g,
        "masses {:.6} / {:.6}",
        g.mass_g,
        g.solid_mass_g
    );
    assert!(
        (g.solid_mass_g - 31.4).abs() < 1.5,
        "solid mass {:.3} g vs closed-form 31.4 g",
        g.solid_mass_g
    );
    assert!(g.sigma_ref > 0.0, "sigma_ref = {}", g.sigma_ref);

    // The mesh's patch labels still claim their faces after the fill: the fill wraps the
    // body field but does not disturb region/facet matching on the solid surface. Build the
    // P2 surface and check the cap/tip regions claim facets.
    let body = expr::compile_body(&d);
    let b = expr::bounds(&body);
    let (mut lin, _) = stuff(|p| expr::eval(&body, p).v, b.lo, b.hi, 1.5);
    lin.compact();
    let mesh = P2Mesh::from_linear(&lin.verts, &lin.tets);
    let facets: Vec<fieldsmith_core::Facet> = lin
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
            fieldsmith_core::Facet {
                nodes: face_nodes(tet, local),
            }
        })
        .collect();
    let cap = region_facets(&d, "cap", &mesh, &facets, 1.5).expect("cap region resolves");
    let tip = region_facets(&d, "tip", &mesh, &facets, 1.5).expect("tip region resolves");
    println!("cap facets: {}, tip facets: {}", cap.len(), tip.len());
    assert!(
        !cap.is_empty(),
        "cap region claims facets on the mesh document"
    );
    assert!(
        !tip.is_empty(),
        "tip region claims facets on the mesh document"
    );
    // The cap is the whole −x face: 10×10 mm at h = 1.5 → ~44 facets (P2, 2 per element face).
    // The tip is the whole +x face likewise. Both non-empty is the claim that matters.
}
