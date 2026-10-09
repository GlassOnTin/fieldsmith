//! Boolean volumes on mesh operands against closed form (goal M7, step 3).
//!
//! M7's first checklist item: booleans on imported meshes are verified compositions, not
//! presumed. A hydrated baked-cube mesh (exact box SDF, band-clamped like every voxel
//! field, patch ids 1/2 authored on the bottom/top surface cells) unions and cuts against
//! primitive boxes and against a second mesh, and every composed volume is integrated
//! through `volume::volume` and compared with the closed form. Patch labels surviving
//! each composition are checked through `eval_mesh_patch` on the composite surface — the
//! step-1 rule (argmin |d|, ties to the earlier operand) decides which operand owns a
//! point, so a surviving mesh face keeps its patch and a cutter's wall claims none.
//!
//! Error budget: the baked fixture's trilinear field rounds the cube's edges and corners
//! inward (the field value near an edge is less negative than the exact SDF, so the
//! occupancy estimate undercounts there). The mesh alone integrates to 995.92 mm³ at
//! h = 0.2 (0.41 % low) and 998.59 at h = 0.1 (0.14 % low) — first-order in h, inherent
//! to the trilinear field representation, not a composition defect. The composite tests
//! use h = 0.2 with a 2 % tolerance, which covers the trilinear error with margin for
//! the composite's extra surface. Every measured row is printed by `measured` so
//! retuning sees the numbers, not assumptions.

use fieldsmith_doc::{Document, Face, Node, NodeKind, Op, Transform};
use fieldsmith_field::expr::{eval, eval_mesh_patch, Expr};
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    hash_mesh_input, hydrate, lookup, patch_index, MeshField, MeshMeta, BAND_CELLS, NO_PATCH,
};
use fieldsmith_field::{compile_body, volume, Aabb};
use std::sync::Arc;

/// Bake an exact box signed-distance grid (band-clamped, like every voxel field), with
/// patch ids 1 (bottom, −z) and 2 (top, +z) authored on the surface cells above the
/// (+x, +y) quadrant of the ±z centres. Same fixture as `combine_label.rs`.
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

fn mesh_node(id: &str, key: u64, half: f64) -> Node {
    Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        kind: NodeKind::Mesh {
            src: "test.stl".into(),
            hash: format!("{key:016x}"),
            units: "mm".into(),
            up: "z".into(),
            voxel: 1.0,
            bbox: [-half, -half, -half, half, half, half],
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
        kind: NodeKind::Box { size, round: 0.0 },
    }
}

/// Hydrate a half-`half` baked cube under a tag-derived key.
fn hyd_cube(tag: &str, half: f64) -> u64 {
    let key = hash_mesh_input(format!("boolvol:{tag}").as_bytes(), b"");
    if lookup(key).is_none() {
        hydrate(key, baked_cube(half, 1.0));
    }
    key
}

/// Volume of the compiled document body, printing the measured/closed-form pair so
/// tolerance constants can be set from runs.
fn measured(e: &Expr, h: f64, name: &str, want: f64) -> f64 {
    let v = volume::volume(e, h).volume;
    println!(
        "{name}: measured {v:.2} mm³, closed form {want:.2} mm³, error {:.3}%",
        100.0 * (v - want).abs() / want
    );
    v
}

fn expect_close(v: f64, want: f64, tol_pct: f64, name: &str) {
    assert!(
        100.0 * (v - want).abs() / want <= tol_pct,
        "{name}: {v} vs closed form {want} beyond {tol_pct}%"
    );
}

#[test]
fn mesh_union_box_volume_matches_closed_form() {
    // Mesh cube half 5 (1000 mm³) union a 4×4×12 box centred at x = 7: the box spans
    // x ∈ [5, 9] (half-extent 2 about 7), y/z ∈ [−2, 2], z ∈ [−6, 6]. The box's −x face
    // sits exactly on the cube's +x face (x = 5) — they touch, overlap volume 0.
    // Union = 1000 + 192 = 1192 mm³.
    //
    // The baked-cube fixture's trilinear field rounds the edges/corners inward (measured:
    // the mesh alone integrates to 995.92 at h = 0.2, 0.41 % low; 998.59 at h = 0.1,
    // 0.14 % low — the error is the trilinear interpolation's, first-order in h). The
    // 2 % tolerance at h = 0.2 covers that with margin for the composite's extra surface.
    let key = hyd_cube("union-box", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("mesh1", key, 5.0));
    doc.root
        .push(box_node("b", [4.0, 4.0, 12.0], [7.0, 0.0, 0.0], Op::Add));
    let e = compile_body(&doc);
    assert!(matches!(e, Expr::Combine { .. }), "union compiles: {e:?}");
    let want = 1000.0 + 192.0;
    let v = measured(&e, 0.2, "mesh ∪ box", want);
    expect_close(v, want, 2.0, "mesh ∪ box");
    // The two solids touch at x = 5: the mesh's +x face and the box's −x face coincide.
    // A point just inside the seam on either side is solid.
    assert!(
        eval(&e, [4.9, 0.0, 0.0]).v < 0.0,
        "mesh side of the seam solid"
    );
    assert!(
        eval(&e, [5.1, 0.0, 0.0]).v < 0.0,
        "box side of the seam solid"
    );
}

#[test]
fn mesh_subtract_box_volume_matches_closed_form() {
    // A 4×4×12 cutter through the cube, centred at (0, 0, 0): the cutter spans
    // x ∈ [−6, 6], y/z ∈ [−2, 2]. Inside the cube (x ∈ [−5, 5]) the removed span is
    // x ∈ [−5, 5] (10 mm), y/z ∈ [−2, 2] (4 each) → removed 10·4·4 = 160 mm³.
    // Remaining = 1000 − 160 = 840 mm³.
    let key = hyd_cube("sub-box", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("mesh1", key, 5.0));
    doc.root.push(box_node(
        "cut",
        [4.0, 4.0, 12.0],
        [0.0, 0.0, 0.0],
        Op::Subtract,
    ));
    let e = compile_body(&doc);
    let want = 1000.0 - 160.0;
    let v = measured(&e, 0.2, "mesh − box", want);
    expect_close(v, want, 2.0, "mesh − box");
    // The hole is open: the cube's centre (0, 0, 0) lies inside the cutter and reads vacuum.
    assert!(eval(&e, [0.0, 0.0, 0.0]).v > 0.0, "cut volume is outside");
    // And a point outside the cutter's y/z extent (y = 3) stays solid.
    assert!(
        eval(&e, [0.0, 3.0, 0.0]).v < 0.0,
        "beside the cutter is solid"
    );
}

#[test]
fn mesh_union_mesh_volume_matches_closed_form() {
    // Two half-5 cubes, the second shifted +8 in x: overlap 2·10·10 = 200 mm³.
    // Union = 2·1000 − 200 = 1800 mm³.
    let ka = hyd_cube("union-mesh-a", 5.0);
    let kb = hyd_cube("union-mesh-b", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("m1", ka, 5.0));
    let mut up = mesh_node("m2", kb, 5.0);
    up.transform.translate = [8.0, 0.0, 0.0];
    doc.root.push(up);
    let e = compile_body(&doc);
    let want = 1800.0;
    let v = measured(&e, 0.2, "mesh ∪ mesh", want);
    expect_close(v, want, 2.0, "mesh ∪ mesh");
}

#[test]
fn mesh_subtract_mesh_volume_matches_closed_form() {
    // Second cube shifted +8 in x cuts a 2·10·10 = 200 mm³ slab off the first's +x face.
    let ka = hyd_cube("sub-mesh-a", 5.0);
    let kb = hyd_cube("sub-mesh-b", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("m1", ka, 5.0));
    let mut up = mesh_node("m2", kb, 5.0);
    up.transform.translate = [8.0, 0.0, 0.0];
    up.op = Op::Subtract;
    doc.root.push(up);
    let e = compile_body(&doc);
    let want = 1000.0 - 200.0;
    let v = measured(&e, 0.2, "mesh − mesh", want);
    expect_close(v, want, 2.0, "mesh − mesh");
    // The cut face at x = 3 is solid on the −x side, vacuum on the +x side.
    assert!(eval(&e, [2.9, 0.0, 0.0]).v < 0.0, "kept side solid");
    assert!(eval(&e, [3.1, 0.0, 0.0]).v > 0.0, "cut side vacuum");
}

#[test]
fn mesh_intersect_box_volume_matches_closed_form() {
    // A 6×6×6 limiter centred at (3, 0, 0) intersects the cube: the overlap is
    // x ∈ [0, 5] (5 long), y/z ∈ [−3, 3] (6 each) → 5·6·6 = 180 mm³.
    let key = hyd_cube("isect-box", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("mesh1", key, 5.0));
    doc.root.push(box_node(
        "lim",
        [6.0, 6.0, 6.0],
        [3.0, 0.0, 0.0],
        Op::Intersect,
    ));
    let e = compile_body(&doc);
    let want = 180.0;
    let v = measured(&e, 0.2, "mesh ∩ box", want);
    expect_close(v, want, 2.0, "mesh ∩ box");
}

#[test]
fn patch_labels_survive_union_and_cut() {
    // Union: the mesh's surviving top patch cell (0.2, 0.3, 5) keeps patch 2; the box's
    // own +x face at x = 10 claims no patch (primitive faces carry none).
    let key = hyd_cube("labels-union", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("mesh1", key, 5.0));
    doc.root
        .push(box_node("b", [4.0, 4.0, 12.0], [8.0, 0.0, 0.0], Op::Add));
    let e = compile_body(&doc);
    assert_eq!(
        eval_mesh_patch(&e, [0.2, 0.3, 5.0]),
        Some(2),
        "surviving mesh top keeps its patch through the union"
    );
    assert_eq!(
        eval_mesh_patch(&e, [10.0, 0.0, 0.0]),
        None,
        "the box's own face claims no patch"
    );

    // Cut: the cutter's wall at x = 2 (its local −x plane, world x = 2) belongs to the
    // cutter — no patch; the mesh's surviving top away from the hole keeps patch 2.
    let key = hyd_cube("labels-cut", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("mesh1", key, 5.0));
    doc.root.push(box_node(
        "cut",
        [4.0, 4.0, 12.0],
        [-4.0, 3.0, 0.0],
        Op::Subtract,
    ));
    let e = compile_body(&doc);
    assert_eq!(
        eval_mesh_patch(&e, [0.2, 0.3, 5.0]),
        Some(2),
        "surviving mesh top keeps its patch through the cut"
    );
    assert_eq!(
        eval_mesh_patch(&e, [-2.0, 3.0, 0.0]),
        None,
        "the cutter's hole wall claims no patch"
    );
    // The bottom patch survives on the untouched −z face.
    assert_eq!(
        eval_mesh_patch(&e, [0.2, 0.3, -5.0]),
        Some(1),
        "untouched bottom keeps patch 1"
    );
}

#[test]
fn patch_labels_survive_mesh_mesh_composition() {
    // Mesh − mesh: the cut face at x = 3 belongs to the cutter (no patch); the first
    // cube's untouched top keeps patch 2.
    let ka = hyd_cube("labels-mm-a", 5.0);
    let kb = hyd_cube("labels-mm-b", 5.0);
    let mut doc = Document::default();
    doc.root.push(mesh_node("m1", ka, 5.0));
    let mut up = mesh_node("m2", kb, 5.0);
    up.transform.translate = [8.0, 0.0, 0.0];
    up.op = Op::Subtract;
    doc.root.push(up);
    let e = compile_body(&doc);
    assert_eq!(
        eval_mesh_patch(&e, [0.2, 0.3, 5.0]),
        Some(2),
        "m1's top keeps its patch through mesh − mesh"
    );
    assert_eq!(
        eval_mesh_patch(&e, [3.0, 0.0, 0.0]),
        None,
        "the cut face belongs to the cutter, no patch"
    );
    // Face claims agree: the composite face at the surviving top is All (mesh surface).
    // At the cut face the composite is max(d_m1, −d_m2): both operands meet at zero, and
    // the step-1 rule (argmin |d|, ties to the earlier operand `a` = m1) picks m1's face
    // — which is All (mesh surfaces carry no geometric face). The cutter's patch is None
    // either way, so the patch claim is stable; only the face code differs from the box
    // cutter's case (where the box's exact Nx face wins the tie).
    let (d, f) = fieldsmith_field::expr::eval_face(&e, [0.2, 0.3, 5.0]);
    assert!(d.abs() < 1e-9, "top surface distance {d}");
    assert_eq!(f, Face::All, "mesh surface face is All");
    let (d, f) = fieldsmith_field::expr::eval_face(&e, [3.0, 0.0, 0.0]);
    assert!(d.abs() < 1e-9, "cut face distance {d}");
    assert_eq!(
        f,
        Face::All,
        "cut face is a mesh surface — face All, no patch"
    );
}
