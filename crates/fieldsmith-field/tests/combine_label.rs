//! Label claims through composites (goal M7, step 1).
//!
//! `eval_face` and `eval_mesh_patch` must attribute a composite's surface to the operand
//! whose own surface is nearest — argmin |d|, ties to the earlier operand, blend fillet
//! bands to neither — uniformly for booleans, group selectors and extend nodes. The GLSL
//! twin (`face_fn`'s Combine arm) is checked structurally here; numeric GPU agreement
//! stays with the Playwright e2e, as for every other ident path (cargo cannot run WebGL).

use fieldsmith_doc::{Document, Face, Node, NodeKind, Op, Selector, Transform};
use fieldsmith_field::expr::{eval_face, eval_mesh_patch, placed_shape, Expr};
use fieldsmith_field::glsl;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    hash_mesh_input, hydrate, lookup, patch_index, MeshField, MeshMeta, BAND_CELLS, NO_PATCH,
};
use fieldsmith_field::{compile_body, eval, Aabb};
use std::sync::Arc;

/// Bake an exact box signed-distance grid (band-clamped, like every voxel field), with
/// patch ids 1 (bottom, −z) and 2 (top, +z) authored on the surface cells above the
/// (+x, +y) quadrant of the ±z centres.
fn baked_cube(half: f64, spacing: f64) -> MeshField {
    let band = BAND_CELLS * spacing;
    let dims = [
        ((2.0 * half + 2.0 * band) / spacing).ceil() as usize + 2,
        ((2.0 * half + 2.0 * band) / spacing).ceil() as usize + 2,
        ((2.0 * half + 2.0 * band) / spacing).ceil() as usize + 2,
    ];
    let origin = [-(half + band); 3];
    let mut data = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                let p = [
                    origin[0] + spacing * i as f64,
                    origin[1] + spacing * j as f64,
                    origin[2] + spacing * k as f64,
                ];
                let q = [p[0].abs() - half, p[1].abs() - half, p[2].abs() - half];
                let m = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
                let outside = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                let inside = q[0].max(q[1]).max(q[2]).min(0.0);
                data.push((outside + inside).clamp(-band, band) as f32);
            }
        }
    }
    let dist = Grid3 {
        origin,
        spacing,
        dims,
        data,
    };
    let mut field = MeshField {
        dist: Arc::new(dist),
        patch: None,
        meta: MeshMeta {
            bbox: Aabb {
                lo: [-half; 3],
                hi: [half; 3],
            },
            tris: 12,
            patches: 2,
            open_edges: 0,
        },
    };
    let mut patch = vec![NO_PATCH; dims[0] * dims[1] * dims[2]];
    for (z, id) in [(-half, 1u16), (half, 2u16)] {
        let cell = field.cell_of([0.0, 0.0, z]).expect("surface cell exists");
        patch[patch_index(dims, cell)] = id;
    }
    field.patch = Some(Arc::new(patch));
    field
}

fn cube_node(id: &str, key: u64) -> Node {
    Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        material: None,
        color: None,
        kind: NodeKind::Mesh {
            src: "test.stl".into(),
            hash: format!("{key:016x}"),
            units: "mm".into(),
            up: "z".into(),
            voxel: 1.0,
            bbox: [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
            tris: 12,
            patches: 2,
            open_edges: 0,
        },
    }
}

fn box_node(id: &str, size: [f64; 3], translate: [f64; 3], op: Op) -> Node {
    Node {
        id: id.into(),
        op,
        blend: 0.0,
        suppressed: false,
        transform: Transform {
            translate,
            rotate: [0.0; 3],
        },
        material: None,
        color: None,
        kind: NodeKind::Box { size, round: 0.0 },
    }
}

fn hydrate_cube(tag: &[u8]) -> u64 {
    let key = hash_mesh_input(tag, b"");
    if lookup(key).is_none() {
        hydrate(key, baked_cube(5.0, 1.0));
    }
    key
}

fn group(children: Vec<Node>) -> Node {
    Node {
        id: "grp".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        material: None,
        color: None,
        kind: NodeKind::Group {
            children,
            modifiers: vec![],
        },
    }
}

#[test]
fn union_claims_follow_the_nearest_operand() {
    let key = hydrate_cube(b"combine:union");
    let mut doc = Document::default();
    doc.root.push(cube_node("mesh1", key));
    doc.root
        .push(box_node("b", [2.0; 3], [20.0, 0.0, 0.0], Op::Add));
    let e = compile_body(&doc);
    // On the box's own face, far from the mesh surface: face Px, no patch claim.
    let (d, f) = eval_face(&e, [21.0, 0.0, 0.0]);
    assert!(d.abs() < 1e-9, "box surface distance {d}");
    assert_eq!(f, Face::Px);
    assert_eq!(eval_mesh_patch(&e, [21.0, 0.0, 0.0]), None);
    // On the mesh's top patch cell, far from the box: face All, patch 2 — the composite
    // surface here is the mesh's.
    let (d, f) = eval_face(&e, [0.0, 0.0, 5.0]);
    assert!(d.abs() < 1e-9, "mesh surface distance {d}");
    assert_eq!(f, Face::All);
    assert_eq!(eval_mesh_patch(&e, [0.0, 0.0, 5.0]), Some(2));
}

#[test]
fn tie_prefers_the_earlier_operand() {
    // Two abutting slabs share the plane x = 5: the first operand's Px wins, not the
    // second's Nx (the codes differ, so the tie-break is observable).
    let mut doc = Document::default();
    doc.root
        .push(box_node("a", [10.0, 2.0, 2.0], [0.0; 3], Op::Add));
    doc.root
        .push(box_node("b", [10.0, 2.0, 2.0], [10.0, 0.0, 0.0], Op::Add));
    let e = compile_body(&doc);
    let (d, f) = eval_face(&e, [5.0, 0.0, 0.0]);
    assert!(d.abs() < 1e-9, "{d}");
    assert_eq!(f, Face::Px, "tie at the shared plane belongs to `a`");
    // Patched meshes stacked exactly: the same tie rule picks the lower cube's top patch,
    // never the upper cube's bottom patch (ids 2 vs 1).
    let ka = hydrate_cube(b"combine:tie-a");
    let kb = hydrate_cube(b"combine:tie-b");
    let mut doc = Document::default();
    doc.root.push(cube_node("m1", ka));
    let mut up = cube_node("m2", kb);
    up.transform.translate = [0.0, 0.0, 10.0];
    doc.root.push(up);
    let e = compile_body(&doc);
    assert_eq!(eval_mesh_patch(&e, [0.0, 0.0, 5.0]), Some(2), "tie → m1");
}

#[test]
fn subtract_splits_host_and_wall() {
    let key = hydrate_cube(b"combine:sub");
    let mut doc = Document::default();
    doc.root.push(cube_node("mesh1", key));
    // A cutter through z, offset in y so the authored top-patch cell (x, y ∈ [0, 1))
    // survives; the hole opens a wall on the cutter's own faces.
    doc.root.push(box_node(
        "cut",
        [4.0, 4.0, 12.0],
        [-4.0, 3.0, 0.0],
        Op::Subtract,
    ));
    let e = compile_body(&doc);
    // Hole wall on the cutter's local +x plane (world x = −2), inside the mesh: the
    // cutter owns it — its face (named in its own frame), no patch.
    let (d, f) = eval_face(&e, [-2.0, 3.0, 0.0]);
    assert!(d.abs() < 1e-9, "wall distance {d}");
    assert_eq!(f, Face::Px);
    assert_eq!(eval_mesh_patch(&e, [-2.0, 3.0, 0.0]), None);
    // The cutter's Ny wall too (y = 1).
    let (d, f) = eval_face(&e, [-4.0, 1.0, 0.0]);
    assert!(d.abs() < 1e-9, "wall distance {d}");
    assert_eq!(f, Face::Ny);
    // Surviving top surface in an authored patch cell, away from the hole: the mesh keeps
    // its patch.
    let (d, f) = eval_face(&e, [0.2, 0.3, 5.0]);
    assert!(d.abs() < 1e-9, "top distance {d}");
    assert_eq!(f, Face::All);
    assert_eq!(eval_mesh_patch(&e, [0.2, 0.3, 5.0]), Some(2));
    // Inside the cutter the material is gone, and the label functions answer without
    // pre-testing the composite surface — `region_facets` gates claims on d ≈ 0, which
    // is what makes the removed region unclaimable.
    assert!(eval(&e, [-4.0, 3.0, 0.0]).v > 0.0, "cut volume is outside");
}

#[test]
fn intersect_claims_the_governing_operand() {
    let key = hydrate_cube(b"combine:isect");
    let mut doc = Document::default();
    doc.root.push(cube_node("mesh1", key));
    doc.root.push(box_node(
        "limiter",
        [4.0, 4.0, 12.0],
        [2.0, 2.0, 0.0],
        Op::Intersect,
    ));
    let e = compile_body(&doc);
    // On the limiter's +x plane (x = 4), inside the mesh: the limiter governs.
    let (d, f) = eval_face(&e, [4.0, 2.0, 0.0]);
    assert!(d.abs() < 1e-9, "{d}");
    assert_eq!(f, Face::Px);
    assert_eq!(eval_mesh_patch(&e, [4.0, 2.0, 0.0]), None);
    // The mesh's top inside the limiter footprint survives and keeps its patch.
    let (d, f) = eval_face(&e, [0.2, 0.3, 5.0]);
    assert!(d.abs() < 1e-9, "{d}");
    assert_eq!(f, Face::All);
    assert_eq!(eval_mesh_patch(&e, [0.2, 0.3, 5.0]), Some(2));
}

#[test]
fn blend_band_labels_neither_operand() {
    let key = hydrate_cube(b"combine:blend");
    let mut doc = Document::default();
    doc.root.push(cube_node("mesh1", key));
    // Cube over a plate, 1 mm apart: with a blend radius the junction is a fillet band
    // belonging to neither operand.
    doc.root.push(box_node(
        "plate",
        [20.0, 20.0, 4.0],
        [6.0, 0.0, -8.0],
        Op::Add,
    ));
    doc.root[1].blend = 2.0; // blends apply to the follower node in compile_list
    let e = compile_body(&doc);
    // On the plate's top face near the seam: da ≈ 1.12 (mesh), db = 0 — both inside the
    // 2 mm band → fillet, code All, no patch.
    let (_, f) = eval_face(&e, [5.5, 0.0, -6.0]);
    assert_eq!(f, Face::All, "fillet band belongs to neither operand");
    assert_eq!(eval_mesh_patch(&e, [5.5, 0.0, -6.0]), None);
    // On the plate's far +x face the blend is inert (the operands are 11 mm apart) and
    // the plate keeps its face; the composite distance is the plain smin's, ≈ 0 there.
    let (d, f) = eval_face(&e, [16.0, 0.0, -8.0]);
    assert!(d.abs() < 1e-6, "plate face distance {d}");
    assert_eq!(f, Face::Px);
    // The mesh's top, away from the band, keeps its patch.
    assert_eq!(eval_mesh_patch(&e, [0.0, 0.0, 5.0]), Some(2));
}

#[test]
fn group_selector_composes_through_eval() {
    let key = hydrate_cube(b"combine:group");
    let mut doc = Document::default();
    doc.root.push(group(vec![
        cube_node("mesh1", key),
        box_node("b", [2.0; 3], [20.0, 0.0, 0.0], Op::Add),
    ]));
    // The group's placed shape is a Combine (compile_list); claims run the same rule.
    let g = placed_shape(&doc, "grp").expect("group exists");
    assert!(
        matches!(g, Expr::Combine { .. }),
        "group compiles to a Combine"
    );
    let (_, f) = eval_face(&g, [21.0, 0.0, 0.0]);
    assert_eq!(f, Face::Px);
    assert_eq!(eval_mesh_patch(&g, [21.0, 0.0, 0.0]), None);
    assert_eq!(eval_mesh_patch(&g, [0.0, 0.0, 5.0]), Some(2));
    // A leaf name still resolves to its own expression (what `fs_ident` scans per leaf);
    // the group path is what group selectors and extend nodes reach.
    let leaf = placed_shape(&doc, "mesh1").expect("leaf exists");
    assert!(
        matches!(leaf, Expr::Mesh { .. }),
        "leaf selection stays leaf-shaped"
    );
    assert_eq!(eval_mesh_patch(&leaf, [0.0, 0.0, 5.0]), Some(2));
}

#[test]
fn blend_band_claims_no_patch_near_fillets() {
    // Two stacked cubes with a blend: on shared coplanar surfaces both operands sit at 0,
    // inside the band → no patch; on m1's top away from m2 the patch survives. Note the
    // rule is evaluated per point without a composite-surface pre-test here (the facet
    // path adds that gate); the assertion documents the label rule itself.
    let ka = hydrate_cube(b"combine:blend-mesh-a");
    let kb = hydrate_cube(b"combine:blend-mesh-b");
    let mut doc = Document::default();
    doc.root.push(cube_node("m1", ka));
    let mut b = cube_node("m2", kb);
    b.blend = 3.0;
    b.transform.translate = [9.0, 0.0, 0.0];
    doc.root.push(b);
    let e = compile_body(&doc);
    // Seam neighbourhood of m1's top (m2 spans x ∈ [4, 14]): both distances ≤ 3.
    assert_eq!(
        eval_mesh_patch(&e, [4.5, 0.3, 5.0]),
        None,
        "fillet band point"
    );
    // m1's untouched top patch (4 mm from m2's edge) keeps claiming.
    assert_eq!(eval_mesh_patch(&e, [0.0, 0.5, 5.0]), Some(2));
    // m2's own top claims its patch (argmin |d| → m2; same patch-id layout as m1's).
    assert_eq!(eval_mesh_patch(&e, [9.2, 0.3, 5.0]), Some(2));
}

/// Structural GLSL check: a group selector's overlay face function carries the argmin
/// winner rule; mesh operands stay sampler-free vacuum in overlays; the blend band
/// returns the composed distance with code 0.
#[test]
fn glsl_face_fn_mirrors_the_combine_rule() {
    let key = hydrate_cube(b"combine:glsl");
    let mut doc = Document::default();
    let mut b = box_node("b", [2.0; 3], [20.0, 0.0, 0.0], Op::Add);
    b.blend = 1.5;
    doc.root.push(group(vec![cube_node("mesh1", key), b]));
    let ov = glsl::overlay(
        &doc,
        &[glsl::OverlayItem {
            selectors: vec![Selector {
                shape: "grp".into(),
                face: Face::All,
                patch: None,
            }],
            color: [1.0; 3],
        }],
    );
    assert!(
        ov.contains("abs(db.x) < abs(da.x) ? db : da"),
        "argmin winner rule: {ov}"
    );
    assert!(
        ov.contains("if (abs(da.x) < 1.5 && abs(db.x) < 1.5)"),
        "blend band guard: {ov}"
    );
    assert!(!ov.contains("fs_mesh"), "overlay stays sampler-free: {ov}");
    assert!(ov.contains("1e9"), "mesh operand vacuums in overlays: {ov}");
}
