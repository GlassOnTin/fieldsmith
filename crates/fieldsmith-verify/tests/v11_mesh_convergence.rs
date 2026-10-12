//! V11 — mesh convergence: an imported body under face-labelled boundary loads.
//!
//! GOAL section H, item 2 (the mesh-convergence case deferred in G): boundary loads on
//! an imported body applied through labelled regions, convergence against a resolved
//! reference.
//!
//! The case: a 60×10×10 mm cantilever bar imported through the real STL chain (read →
//! weld → segment → voxelize → hydrate → document node), clamped on its −x face and
//! carrying 100 N downward on its +x face. The clamp and the load are applied through
//! the geometric face rule on the imported mesh's regions (the same `region_facets`
//! path the app uses) — the bar's STL segments into 6 flat patches, and the face rule
//! claims the whole −x/+x faces. Patch-labelled boundary loads are covered by the same
//! path: a `.pN` selector names one patch, and the region claims exactly that patch's
//! facets (measured in V10); this case exercises the face rule on an imported body end
//! to end.
//!
//! A bending case (not tension) because the answer then depends on the mesh resolving
//! the stress gradient — a tension case's deflection and mid-span stress are exact at
//! every mesh size (the load is spread over the region by `case_force`), so a tension
//! ladder cannot distinguish a converged mesh from a wrong one. The bending case's
//! tip deflection and mid-span stress converge as h refines.
//!
//! The document goes through `fieldsmith_core::analyse` (field → isosurface-stuffed
//! tets → P2 solve → nodal stress recovery) at a mesh-size ladder h = 2.0, 1.0, 0.5 mm.
//! The reference is Timoshenko (Euler–Bernoulli + shear): δ = F·L³/(3·E·I) + F·L/(G·A·k)
//! and σ = M·c/I with M = F·(L−x). The bar is 60 mm long (L/h = 6), so shear adds
//! 2.1 % — small enough for a 5 % tolerance at the finest mesh, large enough that a
//! pure Euler–Bernoulli reference would fail (the first draft at L = 20 mm, L/h = 2,
//! measured 17 % high on deflection and 72 % high on root stress; the shear term explains
//! the deflection, and the root stress probe sat inside the clamp's Poisson-restraint
//! concentration — both fixed by the longer bar and the mid-span probe). The checks:
//!
//! - the tip deflection converges toward the closed form as h refines (the finest mesh
//!   within 5 % of theory, the coarsest within 15 %);
//! - the ladder narrows monotonically toward theory (h = 1.0 within 10 % of h = 0.5);
//! - the mid-span bending stress at the top fibre matches M·c/I (within 5 % at h = 0.5);
//! - the labelled regions claim the full −x/+x faces (100 mm² each) at every mesh size
//!   (the face rule works on the imported mesh, not just on primitives).
//!
//! What this case does NOT measure: the mesh's own field error. The imported bar's
//! voxelised SDF is exact within the band (a box), but its trilinear interpolation
//! rounds the edges/corners inward by O(voxel) — the same error `boolean_volume.rs`
//! pins at 0.41 % on a half-5 cube at h = 0.2. Here the bar is 60×10×10 mm at voxel
//! 1.0 mm, so the field's volume error is larger; the deflection and stress checks
//! absorb it because they probe the field's interior (where the band is exact), not
//! the surface. A case that measures the surface error directly is the V8 mesh row.
//!
//! Tolerances are on the reported value, as the user would see it.

use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::probe::{probe, Locator};
use fieldsmith_core::{analyse, region_facets, Facet, Resolution};
use fieldsmith_doc::{Document, Face, Load, LoadKind, Region, Selector, Support};
use fieldsmith_field::expr;
use fieldsmith_mesh::stuff;
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::P2Mesh;
use fieldsmith_verify::{Check, Report};

fn clock() -> f64 {
    thread_local!(static T0: std::time::Instant = std::time::Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// ASCII STL of an axis-aligned box, outward-wound (20×10×10 mm bar at the origin).
fn bar_stl() -> Vec<u8> {
    let lo = [0.0, 0.0, 0.0];
    let hi = [60.0, 10.0, 10.0];
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
    let mut s = String::from("solid bar\n");
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
    s.push_str("endsolid bar\n");
    s.into_bytes()
}

/// Import the bar and build the document: cap/tip regions on the −x/+x faces (the
/// geometric face rule on the imported mesh), a clamp on the cap, a 100 N downward
/// load on the tip (a cantilever, so the answer depends on the mesh resolving the
/// bending stress gradient), steel (E = 200 GPa).
fn mesh_bar_doc() -> Document {
    let out = import_mesh(
        "stl",
        &bar_stl(),
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
        name: "Mesh convergence bar".into(),
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
    doc.supports.push(Support {
        id: "clamp".into(),
        region: "cap".into(),
        fixed: [true; 3],
    });
    doc.loads.push(Load {
        id: "bend".into(),
        region: "tip".into(),
        case: "Load 1".into(),
        kind: LoadKind::Force {
            vector: [0.0, 0.0, -100.0],
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
    doc
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

#[test]
fn v11_mesh_convergence() {
    let d = mesh_bar_doc();
    assert!(d.validate().is_empty(), "validate: {:?}", d.validate());

    // A bending case, where the answer depends on the mesh resolving the stress gradient:
    // the bar is clamped on its −x face and carries 100 N downward on its +x face.
    // Timoshenko reference (Euler–Bernoulli + shear): δ = F·L³/(3·E·I) + F·L/(G·A·k),
    // σ = M·c/I with M = F·(L−x) at the probe point. The bar is 60×10×10 mm (L/h = 6),
    // long enough that shear adds 2.1 % — small enough that the mesh ladder still
    // resolves the gradient. The stress probe sits at mid-span, away from the clamp's
    // Poisson-restraint concentration at the root.
    let (f, l, e, b, hgt) = (100.0, 60.0, 200_000.0, 10.0, 10.0);
    let nu = 0.3;
    let g = e / (2.0 * (1.0 + nu)); // shear modulus
    let area = b * hgt;
    let i = b * hgt * hgt * hgt / 12.0;
    let k_shear = 5.0 / 6.0; // rectangular cross-section shear correction
    let defl_ref = f * l * l * l / (3.0 * e * i) + f * l / (g * area * k_shear);
    let x_mid = l / 2.0;
    let sigma_ref = f * (l - x_mid) * (hgt / 2.0) / i;

    let mut rows = String::from(
        "| Element size | Tets | DOFs | Iterations | Mesh | Solve | Tip deflection | \
         σxx at mid-span top | Cap facets | Tip facets |\n\
         | --: | --: | --: | --: | --: | --: | --: | --: | --: | --: |\n",
    );
    let mut deflections = Vec::new();
    let mut sigmas = Vec::new();
    let mut facet_counts = Vec::new();
    for h in [2.0, 1.0, 0.5] {
        let a = analyse(&d, Resolution::Size(h), clock).expect("analysis");
        let loc = Locator::new(&a);
        // Tip deflection: the z-displacement at the centre of the loaded face.
        let tip =
            probe(&a, &loc, Some(0), [60.0 - 1e-6, 5.0, 10.0 - 1e-6]).expect("tip inside mesh");
        // Mid-span bending stress: σxx at the top fibre (x = L/2, z = 10), away from
        // the clamp's Poisson-restraint concentration at the root.
        let root = probe(&a, &loc, Some(0), [30.0, 5.0, 10.0 - 1e-6])
            .expect("mid-span top fibre inside mesh");
        let p = &a.provenance;
        // The cap/tip regions claim the full −x/+x faces at this mesh size.
        let (mesh, facets) = surface_facets(&d, h);
        let cap = region_facets(&d, "cap", &mesh, &facets, h).expect("cap region resolves");
        let tipf = region_facets(&d, "tip", &mesh, &facets, h).expect("tip region resolves");
        let cap_area: f64 = cap.iter().map(|&i| facets[i].area(&mesh)).sum();
        let tip_area: f64 = tipf.iter().map(|&i| facets[i].area(&mesh)).sum();
        rows += &format!(
            "| {h} mm | {} | {} | {} | {:.0} ms | {:.0} ms | {:.5} mm | {:.3} MPa | \
             {} ({:.1} mm²) | {} ({:.1} mm²) |\n",
            p.elements,
            p.dofs,
            p.iterations,
            p.mesh_ms,
            p.solve_ms,
            -tip.displacement[2],
            root.stress[0],
            cap.len(),
            cap_area,
            tipf.len(),
            tip_area
        );
        deflections.push(-tip.displacement[2]);
        sigmas.push(root.stress[0]);
        facet_counts.push((cap.len(), tipf.len()));
        // The regions claim the full 10×10 mm faces (100 mm² each) at every mesh size.
        assert!(
            (cap_area - 100.0).abs() < 1.0,
            "cap region area {cap_area:.2} mm² at h = {h} vs the face's 100 mm²"
        );
        assert!(
            (tip_area - 100.0).abs() < 1.0,
            "tip region area {tip_area:.2} mm² at h = {h} vs the face's 100 mm²"
        );
    }

    let fine = *deflections.last().unwrap();
    let coarse = *deflections.first().unwrap();
    let fine_sigma = *sigmas.last().unwrap();

    let mut r = Report::new(
        "V11",
        "Mesh convergence: imported cantilever under face-labelled boundary loads",
    );
    // Finest mesh vs theory: δ = F·L³/(3·E·I) = 0.0267 mm.
    r.check(Check::relative(
        "Tip deflection at h = 0.5 mm (finest mesh)",
        defl_ref,
        fine,
        "mm",
        0.05,
    ));
    // Coarsest mesh vs theory: the ladder's first row is further off.
    r.check(Check::relative(
        "Tip deflection at h = 2.0 mm (coarsest mesh)",
        defl_ref,
        coarse,
        "mm",
        0.15,
    ));
    // Mid-span bending stress vs M·c/I with M = F·(L/2) = 18 MPa (tension on the top
    // fibre for a downward tip load). The probe reads the top fibre at x = L/2, away
    // from the clamp's Poisson-restraint concentration at the root.
    r.check(Check::relative(
        "σxx at mid-span top fibre, h = 0.5 mm",
        sigma_ref,
        fine_sigma,
        "MPa",
        0.05,
    ));
    // Convergence: the ladder narrows toward theory (h = 1.0 within 10 % of h = 0.5).
    r.check(Check::relative(
        "convergence: h = 1.0 deflection against the h = 0.5 reference",
        fine,
        deflections[1],
        "mm",
        0.10,
    ));
    r.section(
        "Reference",
        format!(
            "Timoshenko cantilever: δ = F·L³/(3·E·I) + F·L/(G·A·k) = {defl_ref:.5} mm with \
         I = b·h³/12 = {i:.3} mm⁴, G = E/(2(1+ν)) = {g:.0} MPa, k = 5/6, and \
         σ = M·c/I = {sigma_ref:.3} MPa with M = F·(L/2) at mid-span, c = {hgt}/2. \
         Bar 60 × 10 × 10 mm, E = 200 GPa, ν = 0.3, clamped on x = 0, 100 N down on \
         x = 60. The document is an imported STL mesh node; the clamp and load regions \
         use the geometric face rule (Face::Nx / Face::Px) on the imported mesh — the \
         face rule claims the whole −x/+x faces. The Timoshenko reference includes shear \
         (2.1 % here at L/h = 6); the stress probe sits at mid-span, away from the clamp's \
         Poisson-restraint concentration at the root."
        ),
    );
    r.section("Mesh study", rows);
    r.section(
        "Method",
        "The STL bar goes through the real import chain (read → weld → segment → voxelize → \
         hydrate → document node), then `fieldsmith_core::analyse` at h = 2.0, 1.0, 0.5 mm \
         (field → isosurface-stuffed tets → P2 solve → nodal stress recovery). The tip value \
         is the probed z-displacement at the centre of the loaded face; the stress value is \
         the probed σxx at the top fibre at mid-span. The cap/tip regions are matched through \
         `region_facets` (the same path the app uses), and their claimed area is asserted \
         against the face's 100 mm² at every mesh size.",
    );
    r.section(
        "Convergence",
        format!(
            "Tip deflections: h = 2.0 → {:.5} mm, h = 1.0 → {:.5} mm, h = 0.5 → {:.5} mm \
             (Timoshenko {:.5} mm). Mid-span σxx: {:.3} / {:.3} / {:.3} MPa \
             (theory {sigma_ref:.3} MPa). Facet counts per region: {:?}.",
            deflections[0],
            deflections[1],
            fine,
            defl_ref,
            sigmas[0],
            sigmas[1],
            fine_sigma,
            facet_counts
        ),
    );
    r.finish();
}
