//! Region-driven mesh extension (goal M6, step 8).
//!
//! `swept_patch_field` turns the thin shell of patch-claimed cells into a swept solid:
//! sources claimed by the named patches with |distance| ≤ 1 spacing, stamped along `dir`
//! for `len`, then a two-pass 26-neighbour chamfer signed EDT. The result is an ordinary
//! `MeshField`: it samples, integrates and unions with its parent like any mesh leaf, is
//! deterministic, and the compile path (`Expr::Extend`) degrades to vacuum over the swept
//! bounds when the parent is not hydrated.

use fieldsmith_doc::{Document, Face, Node, NodeKind, Op, Region, Selector, Transform};
use fieldsmith_field::dual::D;
use fieldsmith_field::expr::Expr;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    extend_key, extend_key_face, hash_mesh_input, hydrate, key_from_hex, lookup, patch_index,
    swept_face_field, swept_patch_field, MeshField, MeshMeta, MAX_EXTEND_CELLS, NO_PATCH,
};
use fieldsmith_field::volume::integrate_with;
use fieldsmith_field::{bounds, Aabb};
use fieldsmith_field::{compile_body, eval};
use std::collections::HashSet;
use std::sync::Arc;

/// Solid volume of a baked field at the fixed 0.25 spacing integration step.
fn volume_of(f: &MeshField) -> f64 {
    integrate_with(&|p| f.sample(p), f.meta.bbox, 0.25, None)
}

/// Bake an exact box signed-distance field (mirrors the mesh_expr.rs fixture) with patch
/// id 1 on the node row that lies in the bottom face plane. `hole_half_width` excludes the
/// strip |x| ≤ hole across the face, standing in for a perforated patch: the real
/// segmentation would leave the cells over a hole unclaimed and the sweep then leaves the
/// hole open. `None` claims the whole face.
fn bake_box(half: [f64; 3], spacing: f64, hole_half_width: Option<f64>) -> MeshField {
    let band = 3.0 * spacing;
    let dims = [
        ((2.0 * half[0] + 2.0 * band) / spacing).ceil() as usize + 2,
        ((2.0 * half[1] + 2.0 * band) / spacing).ceil() as usize + 2,
        ((2.0 * half[2] + 2.0 * band) / spacing).ceil() as usize + 2,
    ];
    let origin = [-(half[0] + band), -(half[1] + band), -(half[2] + band)];
    let mut data = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                let p = [
                    origin[0] + spacing * i as f64,
                    origin[1] + spacing * j as f64,
                    origin[2] + spacing * k as f64,
                ];
                let q = [
                    p[0].abs() - half[0],
                    p[1].abs() - half[1],
                    p[2].abs() - half[2],
                ];
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
                lo: [-half[0], -half[1], -half[2]],
                hi: [half[0], half[1], half[2]],
            },
            tris: 12,
            patches: 0,
            open_edges: 0,
        },
    };
    // The face plane is the node row 3·spacing above the grid origin for any spacing
    // (origin sits half+3s below zero), so the footprint check below sees the face nodes
    // exactly, including the boundary nodes at |x| = half.
    let z_face = -half[2];
    let mut patch = vec![NO_PATCH; dims[0] * dims[1] * dims[2]];
    let mut claimed = 0usize;
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                let p = [
                    origin[0] + spacing * i as f64,
                    origin[1] + spacing * j as f64,
                    origin[2] + spacing * k as f64,
                ];
                let on_row = (p[2] - z_face).abs() <= 0.5 * spacing;
                let in_footprint =
                    p[0].abs() <= half[0] + 0.5 * spacing && p[1].abs() <= half[1] + 0.5 * spacing;
                let over_hole = hole_half_width.is_some_and(|h| p[0].abs() <= h);
                if on_row && in_footprint && !over_hole {
                    patch[patch_index(dims, [i, j, k])] = 1;
                    claimed += 1;
                }
            }
        }
    }
    field.patch = Some(Arc::new(patch));
    field.meta.patches = 1;
    assert!(claimed > 0, "fixture claimed no patch cells");
    field
}

/// The same box with no patch grid at all (an unlabelled import).
fn bake_unpatched(half: [f64; 3], spacing: f64) -> MeshField {
    let mut f = bake_box(half, spacing, None);
    f.patch = None;
    f.meta.patches = 0;
    f
}

/// A document with a mesh node and one extend node fed by `region`.
fn extend_doc(hash: &str, bbox: [f64; 6], region: &str, dir: [f64; 3], len: f64) -> Document {
    Document {
        name: "extend test".into(),
        root: vec![
            Node {
                id: "mesh1".into(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform::default(),
                kind: NodeKind::Mesh {
                    src: "test.stl".into(),
                    hash: hash.to_string(),
                    units: "mm".into(),
                    up: "z".into(),
                    voxel: 1.0,
                    bbox,
                    tris: 12,
                    patches: 1,
                    open_edges: 0,
                },
            },
            Node {
                id: "ext1".into(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform::default(),
                kind: NodeKind::Extend {
                    region: region.into(),
                    dir,
                    len,
                },
            },
        ],
        regions: vec![Region {
            id: region.into(),
            selectors: vec![Selector {
                shape: "mesh1".into(),
                face: Face::All,
                patch: Some(1),
            }],
        }],
        ..Default::default()
    }
}

/// The same document, but the region selects a geometric face of the mesh (the face-axis
/// rule) instead of a patch id: `region r = mesh1.bottom` on an unpatched import.
fn extend_doc_face(hash: &str, bbox: [f64; 6], region: &str, dir: [f64; 3], len: f64) -> Document {
    Document {
        name: "extend face test".into(),
        root: vec![
            Node {
                id: "mesh1".into(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform::default(),
                kind: NodeKind::Mesh {
                    src: "test.stl".into(),
                    hash: hash.to_string(),
                    units: "mm".into(),
                    up: "z".into(),
                    voxel: 1.0,
                    bbox,
                    tris: 12,
                    patches: 0,
                    open_edges: 0,
                },
            },
            Node {
                id: "ext1".into(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform::default(),
                kind: NodeKind::Extend {
                    region: region.into(),
                    dir,
                    len,
                },
            },
        ],
        regions: vec![Region {
            id: region.into(),
            selectors: vec![Selector {
                shape: "mesh1".into(),
                face: Face::Bottom,
                patch: None,
            }],
        }],
        ..Default::default()
    }
}

/// Leaf compiled from a derived field: hydrate under its recipe key first.
fn derived_leaf(key: u64, f: &MeshField) -> Expr {
    hydrate(key, f.clone());
    Expr::Mesh {
        key,
        bbox: f.meta.bbox,
        spec: Some(f.spec()),
    }
}

/// Per-test mesh input key. `hydrate` replaces the registry entry under a key, so parallel
/// test threads must not hydrate differently-authored grids under one shared hash — these
/// tests once shared a fixed `0123456789abcdef` hash and the weaker grid (patched vs
/// patch-stripped, different sweep lengths) won nondeterministically, depending on which
/// test's bake the compiler saw. Each test derives its own key from a tag; the doc node
/// carries `format!("{key:016x}")` and the compiler parses it back with `key_from_hex`,
/// which round-trips exactly.
fn test_key(tag: &str) -> u64 {
    hash_mesh_input(format!("extend_field:{tag}").as_bytes(), b"")
}

#[test]
fn swept_cube_patch_volume_and_shape() {
    let half = [5.0; 3];
    let parent = bake_box(half, 1.0, None);
    let hash = format!("{:016x}", test_key("swept-cube"));
    let a = swept_patch_field(&parent, [0.0, 0.0, -1.0], 10.0, &[1]).expect("sweep succeeds");
    // Determinism: a second derivation is bit-identical (the EDT is a pure function of the
    // mask; no hashing, threading or float-order variation is allowed through).
    let b = swept_patch_field(&parent, [0.0, 0.0, -1.0], 10.0, &[1]).expect("sweep succeeds");
    assert_eq!(a.dist.dims, b.dist.dims);
    assert_eq!(a.dist.origin, b.dist.origin);
    assert_eq!(a.dist.data, b.dist.data);

    // Node-centred EDT convention: the swept node hull is the 11×11 face-node block swept
    // 10 mm, and the solid boundary sits half a spacing beyond the outer nodes on each
    // axis (10+1)³ = 1331 mm³ nominally. The trilinear zero contour pulls the outermost
    // node ring slightly inside that box — measured 1317.5, 1.0 % under — so the window is
    // 2 %.
    let v = volume_of(&a);
    assert!(
        (v - 1331.0).abs() < 1331.0 * 0.02,
        "swept prism volume {v} vs EDT-implied 11³ = 1331 (nominal face·len = 1000)"
    );

    // Sign checks against the node hull: material under the swept face, vacuum outside.
    let e = derived_leaf(extend_key(&hash, &[1], [0.0, 0.0, -1.0], 10.0, 1.0), &a);
    assert!(eval(&e, [0.0, 0.0, -6.0]).v < 0.0, "mid-skirt solid");
    assert!(eval(&e, [0.0, 0.0, -15.0]).v < 0.0, "skirt tip solid");
    assert!(
        eval(&e, [0.0, 0.0, -16.0]).v > 0.0,
        "just past the tip: vacuum"
    );
    assert!(
        eval(&e, [6.5, 0.0, -6.0]).v > 0.0,
        "beside the skirt: vacuum"
    );
    assert!(
        eval(&e, [0.0, 0.0, -16.0 - 2.0]).v > 0.0,
        "far below: vacuum"
    );

    // The extend leaf reports the parent's bbox displaced by the sweep, padded one spacing.
    let eb = bounds(&e);
    for (ax, expected) in [(0, -5.5), (1, -5.5), (2, -15.5)] {
        assert_eq!(eb.lo[ax], expected, "lo axis {ax}");
    }
    for (ax, expected) in [(0, 5.5), (1, 5.5), (2, -4.5)] {
        assert_eq!(eb.hi[ax], expected, "hi axis {ax}");
    }
    // The derivation's meta bbox agrees with what a hydrated parent compiles to.
    let f2 = lookup(extend_key(&hash, &[1], [0.0, 0.0, -1.0], 10.0, 1.0))
        .expect("derived leaf hydrated");
    for ax in 0..3 {
        assert_eq!(eb.lo[ax], f2.meta.bbox.lo[ax]);
        assert_eq!(eb.hi[ax], f2.meta.bbox.hi[ax]);
    }
}

#[test]
fn swept_volume_halves_its_voxel_margin_with_spacing() {
    // Same cube at spacing 0.5: the +half-cell-per-axis EDT margin shrinks with it, so the
    // measured prism converges toward the nominal 10×10×10 (10.5³ = 1157.625).
    let half = [5.0; 3];
    let parent = bake_box(half, 0.5, None);
    let a = swept_patch_field(&parent, [0.0, 0.0, -1.0], 10.0, &[1]).expect("sweep succeeds");
    let v = volume_of(&a);
    assert!(
        (v - 1157.625).abs() < 1157.625 * 0.01,
        "fine-spacing prism volume {v} vs EDT-implied 10.5³ = 1157.625"
    );
}

#[test]
fn perforated_patch_leaves_holes_open() {
    // A face patch interrupted by a 3 mm strip (nodes |x| ≤ 1 unclaimed): the sweep keeps
    // the strip open — material only under the claimed columns.
    let half = [5.0; 3];
    let parent = bake_box(half, 1.0, Some(1.5));
    let hash = format!("{:016x}", test_key("perforated"));
    let a = swept_patch_field(&parent, [0.0, 0.0, -1.0], 5.0, &[1]).expect("sweep succeeds");
    // Two strips: x from the hull edge crossing (−5.5) to the hole crossing (−1.5), so each
    // is 4 wide × (11 + 1) deep × (5 + 1) tall → 2·4·11·6 = 528 mm³ nominally. The zero
    // contour pulls the boundary ring in ~1–3 % (same effect as the solid-face sweep, which
    // measured 1.0 % under); measured 511.2, so the window is 5 %.
    let v = volume_of(&a);
    assert!(
        (v - 528.0).abs() < 528.0 * 0.05,
        "perforated sweep volume {v} vs strips 2·(4·11·6) = 528"
    );
    let e = derived_leaf(extend_key(&hash, &[1], [0.0, 0.0, -1.0], 5.0, 1.0), &a);
    assert!(eval(&e, [0.0, 0.0, -6.0]).v > 0.0, "under the hole: vacuum");
    assert!(eval(&e, [0.0, 0.0, -4.0]).v > 0.0, "above the hole: vacuum");
    assert!(eval(&e, [-3.5, 0.0, -6.0]).v < 0.0, "strip interior solid");
    assert!(
        eval(&e, [3.5, 0.0, -6.0]).v < 0.0,
        "other strip interior solid"
    );
}

#[test]
fn union_with_parent_leaves_body_untouched_above_the_patch() {
    let half = [5.0; 3];
    let parent = bake_box(half, 1.0, None);
    let key = test_key("union-parent");
    let hash = format!("{key:016x}");
    hydrate(key, parent.clone());
    let doc = extend_doc(
        &hash,
        [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
        "bottom",
        [0.0, 0.0, -1.0],
        10.0,
    );
    let errs = doc.validate();
    assert!(errs.is_empty(), "doc validates clean: {errs:?}");
    let body = compile_body(&doc);

    // The compiled body unions the parent mesh with the swept extension, unblended.
    let Expr::Combine {
        op: Op::Add,
        blend: 0.0,
        a,
        b,
    } = &body
    else {
        panic!("expected an unblended two-leaf Add body, got {body:?}");
    };
    assert!(
        matches!(**a, Expr::Mesh { spec: Some(_), .. })
            && matches!(**b, Expr::Mesh { spec: Some(_), .. }),
        "both leaves are mesh nodes carrying grid slots"
    );

    // Above the patch the parent's samples win: interior points above the face are exactly
    // the parent's values, the extension contributes nothing.
    let parent_leaf = &**a;
    for p in [[0.0, 0.0, 0.0], [0.0, 0.0, -4.0], [4.0, 3.0, 2.0]] {
        let pv = eval(parent_leaf, p).v;
        let bv = eval(&body, p).v;
        assert!(pv < 0.0, "parent solid at {p:?}");
        assert_eq!(bv, pv, "body untouched by the sweep at {p:?}");
    }
    // Below the face the skirt is solid where the parent was air.
    let below = eval(&body, [0.0, 0.0, -6.0]);
    assert!(below.v < 0.0, "skirt solid at (0, 0, -6): {}", below.v);
    assert!(
        eval(&body, [0.0, 0.0, -16.0]).v > 0.0,
        "past the skirt: vacuum"
    );

    // Bounds are the union of the parent box and the swept hull's padded bbox.
    let want = Aabb {
        lo: [-5.5, -5.5, -15.5],
        hi: [5.5, 5.5, 5.0],
    };
    assert_eq!(bounds(&body), want);

    // Volume bookkeeping: parent + extension minus the 0.5-cm³ wedge where both are solid
    // (the extension's top zero crossing sits a half spacing above the box's bottom face
    // across the box footprint). Integration is conservative, so allow 2%.
    let v_body = integrate_with(&|p| eval(&body, p), bounds(&body), 0.25, None);
    let v_parent = integrate_with(&|p| eval(parent_leaf, p), bounds(parent_leaf), 0.25, None);
    let derived = swept_patch_field(&parent, [0.0, 0.0, -1.0], 10.0, &[1]).expect("sweep succeeds");
    let v_ext = volume_of(&derived);
    let want = v_parent + v_ext - 50.0;
    assert!(
        (v_body - want).abs() < want * 0.02,
        "union volume {v_body} vs parent {v_parent} + extension {v_ext} − wedge 50 = {want}"
    );
}

#[test]
fn extend_follows_the_parent_mesh_transform() {
    // The swept field is derived from the hydrated grid, which lives in the mesh node's
    // local frame — the compiled arm must carry the node's placement, or the skirt stays at
    // the origin while the body moves.
    let half = [5.0; 3];
    let parent = bake_box(half, 1.0, None);
    let key = test_key("transform-parent");
    let hash = format!("{key:016x}");
    hydrate(key, parent.clone());

    // Translated parent: the skirt hangs below the shifted body, not below the origin.
    let mut moved = extend_doc(
        &hash,
        [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
        "bottom",
        [0.0, 0.0, -1.0],
        10.0,
    );
    moved.root[0].transform = Transform {
        translate: [10.0, 0.0, 0.0],
        rotate: [0.0, 0.0, 0.0],
    };
    let body = compile_body(&moved);
    assert!(
        eval(&body, [10.0, 0.0, 0.0]).v < 0.0,
        "shifted parent solid"
    );
    assert!(
        eval(&body, [10.0, 0.0, -6.0]).v < 0.0,
        "skirt follows the parent"
    );
    assert!(
        eval(&body, [0.0, 0.0, -6.0]).v > 0.0,
        "where the unshifted skirt would sit: vacuum"
    );
    assert!(
        eval(&body, [10.0, 0.0, -16.0]).v > 0.0,
        "past the skirt: vacuum"
    );
    let b = bounds(&body);
    assert_eq!(b.lo, [4.5, -5.5, -15.5], "shifted union lo");
    assert_eq!(b.hi, [15.5, 5.5, 5.0], "shifted union hi");

    // Rotated parent (+90° about y maps local (x, z) → world (z, −x)); the extend's
    // direction is read in the DOCUMENT frame and mapped through the node placement into
    // the grid sweep: dir (−1, 0, 0) document = leaf (0, 0, −1) = the patch's own outward
    // normal, so the skirt trails into world −x. A z-rotation would not discriminate — the
    // square skirt footprint maps onto itself — so the rotation must turn the leaf sweep
    // into a horizontal world axis.
    let mut spun = extend_doc(
        &hash,
        [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
        "bottom",
        [-1.0, 0.0, 0.0],
        10.0,
    );
    spun.root[0].transform = Transform {
        translate: [0.0, 0.0, 0.0],
        rotate: [0.0, 90.0, 0.0],
    };
    let body = compile_body(&spun);
    assert!(eval(&body, [0.0, 0.0, 0.0]).v < 0.0, "rotated parent solid");
    assert!(
        eval(&body, [-10.0, 0.0, 0.0]).v < 0.0,
        "skirt follows the rotation"
    );
    assert!(
        eval(&body, [0.0, 0.0, -6.0]).v > 0.0,
        "where the unrotated skirt would sit: vacuum"
    );
    assert!(
        eval(&body, [16.0, 0.0, 0.0]).v > 0.0,
        "past the rotated skirt: vacuum"
    );
    let b = bounds(&body);
    // Rotation re-derives the corners through the matrix, so compare with a tolerance
    // (translation-only bounds above stay exact).
    let eps = 1e-9;
    assert!(
        (b.lo[0] - (-15.5)).abs() < eps,
        "rotated union lo x: {:?}",
        b.lo
    );
    assert!(
        (b.lo[1] - (-5.5)).abs() < eps,
        "rotated union lo y: {:?}",
        b.lo
    );
    assert!(
        (b.lo[2] - (-5.5)).abs() < eps,
        "rotated union lo z: {:?}",
        b.lo
    );
    // The skirt lands at world x ∈ [−15.5, −4.5]; the un-swept half of the parent box
    // (x up to +5) still sets the union's hi x.
    assert!(
        (b.hi[0] - 5.0).abs() < eps,
        "rotated union hi x: {:?}",
        b.hi
    );
    assert!(
        (b.hi[1] - 5.5).abs() < eps,
        "rotated union hi y: {:?}",
        b.hi
    );
    assert!(
        (b.hi[2] - 5.5).abs() < eps,
        "rotated union hi z: {:?}",
        b.hi
    );
}

#[test]
fn unhydrated_parent_compiles_to_vacuum_over_swept_bounds() {
    let hash = "ffffffffffffffff";
    assert!(lookup(key_from_hex(hash)).is_none());
    let bbox = [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0];
    let dir = [0.0, 0.0, -1.0];
    let doc = extend_doc(hash, bbox, "bottom", dir, 10.0);
    let body = compile_body(&doc);
    // Both leaves carry no slots: the unhydrated mesh leaf itself, and the vacuum
    // placeholder the extend compiles to over the swept bounds of the node's bbox.
    let Expr::Combine {
        op: Op::Add,
        blend: 0.0,
        a,
        b,
    } = &body
    else {
        panic!("expected a two-leaf Add body, got {body:?}");
    };
    let parent_hash_key = key_from_hex(hash);
    match (&**a, &**b) {
        (
            Expr::Mesh {
                key,
                bbox: mb,
                spec,
            },
            Expr::Mesh {
                key: 0,
                bbox: _eb,
                spec: espec,
            },
        ) => {
            assert_eq!(*key, parent_hash_key, "the parent leaf keeps its own key");
            assert!(spec.is_none(), "unhydrated leaf has no slots");
            assert_eq!(
                *mb,
                Aabb {
                    lo: [bbox[0], bbox[1], bbox[2]],
                    hi: [bbox[3], bbox[4], bbox[5]]
                }
            );
            assert!(espec.is_none(), "no grid slots for the placeholder");
        }
        other => panic!("expected two vacuum mesh leaves, got {other:?}"),
    }
    // The placeholder covers the parent box union its copy displaced by len·dir.
    let want = Aabb {
        lo: [
            bbox[0].min(bbox[0] + dir[0] * 10.0),
            bbox[1].min(bbox[1] + dir[1] * 10.0),
            bbox[2].min(bbox[2] + dir[2] * 10.0),
        ],
        hi: [
            bbox[3].max(bbox[3] + dir[0] * 10.0),
            bbox[4].max(bbox[4] + dir[1] * 10.0),
            bbox[5].max(bbox[5] + dir[2] * 10.0),
        ],
    };
    // The extend leaf's bbox — the key-0 leaf, arm b above — is exactly that placeholder.
    let Expr::Combine { b: extend_leaf, .. } = &body else {
        unreachable!()
    };
    let Expr::Mesh { bbox: got, .. } = &**extend_leaf else {
        unreachable!()
    };
    assert_eq!(*got, want);
    assert_eq!(bounds(&body), want);
    assert_eq!(
        eval(&body, [0.0, 0.0, -6.0]),
        D::EMPTY,
        "vacuum placeholder"
    );
}

#[test]
fn extend_keys_distinguish_recipes_and_fold_id_order() {
    const HASH: &str = "0123456789abcdef";
    let base = extend_key(HASH, &[1, 2], [0.0, 0.0, -1.0], 10.0, 1.0);
    // Ids are sorted and deduplicated inside the salt: order and duplicates do not move it.
    assert_eq!(base, extend_key(HASH, &[2, 1], [0.0, 0.0, -1.0], 10.0, 1.0));
    assert_eq!(
        base,
        extend_key(HASH, &[1, 2, 1], [0.0, 0.0, -1.0], 10.0, 1.0)
    );
    // Every recipe ingredient is load-bearing.
    let mut different = HashSet::new();
    different.insert(base);
    for k in [
        extend_key(HASH, &[1], [0.0, 0.0, -1.0], 10.0, 1.0),
        extend_key(HASH, &[2], [0.0, 0.0, -1.0], 10.0, 1.0),
        extend_key(HASH, &[1, 2], [0.0, 0.0, -1.0], 10.0, 0.5),
        extend_key(HASH, &[1, 2], [0.0, 0.0, -1.0], 12.0, 1.0),
        extend_key(HASH, &[1, 2], [1.0, 0.0, -1.0], 10.0, 1.0),
        extend_key(HASH, &[1, 2], [1.0, 0.0, 0.0], 10.0, 0.5),
        extend_key("fedcba9876543210", &[1, 2], [0.0, 0.0, -1.0], 10.0, 1.0),
    ] {
        different.insert(k);
    }
    assert_eq!(different.len(), 8, "distinct recipes hash apart");

    // Never equal to the parent mesh key derived from the same hash text.
    let parent_key = key_from_hex(HASH);
    assert_ne!(base, parent_key);
    assert_ne!(key_from_hex("ffffffffffffffff"), 0, "hash-0 stays reserved");
    // The budget constant stays the documented shared cap.
    assert_eq!(MAX_EXTEND_CELLS, 6_000_000);
}

#[test]
fn swept_patch_field_rejects_unusable_inputs() {
    let patched = bake_box([5.0; 3], 1.0, None);
    let unpatched = bake_unpatched([5.0; 3], 1.0);
    // A patch grid carrying a single claimed node on the bottom face plane.
    let mut one_cell = bake_unpatched([5.0; 3], 1.0);
    let mut patch =
        vec![NO_PATCH; one_cell.dist.dims[0] * one_cell.dist.dims[1] * one_cell.dist.dims[2]];
    let cell = one_cell.cell_of([2.0, 2.0, -5.0]).expect("cell exists");
    patch[patch_index(one_cell.dist.dims, cell)] = 1;
    one_cell.patch = Some(Arc::new(patch));

    assert!(swept_patch_field(&patched, [0.0, 0.0, -1.0], 10.0, &[]).is_err());
    assert!(swept_patch_field(&patched, [0.0, 0.0, 0.0], 10.0, &[1])
        .expect_err("zero direction is refused")
        .contains("zero"));
    assert!(swept_patch_field(&patched, [0.0, 0.0, -1.0], 0.0, &[1]).is_err());
    assert!(swept_patch_field(&patched, [0.0, 0.0, -1.0], -5.0, &[1]).is_err());
    assert!(swept_patch_field(&patched, [0.0, 0.0, -1.0], f64::NAN, &[1]).is_err());
    let e = swept_patch_field(&unpatched, [0.0, 0.0, -1.0], 10.0, &[1])
        .expect_err("parent without a patch grid is refused");
    assert!(e.contains("no patch grid"), "got: {e}");
    let e = swept_patch_field(&patched, [0.0, 0.0, -1.0], 10.0, &[7])
        .expect_err("unclaimed patch id is refused");
    assert!(
        e.contains("no patch-grid cell claims patches [7]"),
        "got: {e}"
    );
    // The single claimed node sweeps a one-node column: the node hull is 1×1 nodes over the
    // swept length (11 nodes tall for len 10, spacing 1) and a single claimed node among
    // unclaimed neighbours has its trilinear zero set only over ~0.55 of its cell, so the
    // integrated column is ~6 mm³ rather than the 3×3×11 = 99 a mask-hull prism would give.
    // Measured 5.998.
    let thin = swept_patch_field(&one_cell, [0.0, 0.0, -1.0], 10.0, &[1]).expect("sweep succeeds");
    let v = volume_of(&thin);
    assert!(
        (v - 6.0).abs() < 6.0 * 0.03,
        "single-node source sweeps a ~6 mm³ column, got {v}"
    );
}

#[test]
fn swept_face_rule_sweeps_only_the_cone_facing_cells() {
    // The face-axis rule: sources are every near-surface cell (|dist| ≤ 1 spacing) whose
    // SDF gradient falls in the 45° cone of the face axis — the bottom band of an
    // unpatched box, no patch grid consulted. At a box edge the bottom cells of the side
    // walls lean diagonally outward at 26.6° off the axis (cos 0.894 ≥ 0.7), so the cone
    // admits them too and the swept footprint is the full 11×11 node square of the band
    // minus the four corner columns, whose gradient straddles two faces and drops below
    // the threshold (the corner cell at (±5, ±5, −5) tilts 45° the other way, cos
    // −0.707 < 0.7), 13 rows tall for the 3-row band swept 10 mm.
    let parent = bake_unpatched([5.0; 3], 1.0);
    let hash = format!("{:016x}", test_key("face-cone"));
    let a = swept_face_field(&parent, [0.0, 0.0, -1.0], 10.0, [0.0, 0.0, -1.0])
        .expect("sweep succeeds");
    // Determinism: a second derivation is bit-identical.
    let b = swept_face_field(&parent, [0.0, 0.0, -1.0], 10.0, [0.0, 0.0, -1.0])
        .expect("sweep succeeds");
    assert_eq!(a.dist.dims, b.dist.dims);
    assert_eq!(a.dist.origin, b.dist.origin);
    assert_eq!(a.dist.data, b.dist.data);

    // The 3-row source band (z −6..−4) swept 10 mm down: hull 11×11×13 nodes → grid
    // 17×17×19 at origin (hull lo − 3 cells)/z. A top-face or side-wall sweep would blow
    // the z dims — the exact dims pin the source band to the bottom face only.
    assert_eq!(a.dist.dims, [17, 17, 19]);
    assert_eq!(a.dist.origin, [-8.0, -8.0, -19.0]);
    let eb = a.meta.bbox;
    assert_eq!(eb.lo, [-5.5, -5.5, -16.5]);
    assert_eq!(eb.hi, [5.5, 5.5, -3.5]);

    // Node-centred EDT convention as in the patch test: the hull prism is
    // 11 × 11 × 13 = 1573 mm³. The cone-cut corners round the prism's four vertical
    // edges — measured 1511.0, 3.9 % under — so the window is 5 %.
    let v = volume_of(&a);
    assert!(
        (v - 1573.0).abs() < 1573.0 * 0.05,
        "swept face volume {v} vs EDT-implied 11·11·13 = 1573 (nominal face·len = 1000)"
    );

    // Sign checks: solid from the (invisible, parent-overlapped) top of the skirt to the
    // tip, vacuum past the tip and beyond the footprint's half-spacing margin.
    let e = derived_leaf(
        extend_key_face(&hash, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0], 10.0, 1.0),
        &a,
    );
    assert!(eval(&e, [0.0, 0.0, -6.0]).v < 0.0, "mid-skirt solid");
    assert!(eval(&e, [0.0, 0.0, -15.9]).v < 0.0, "skirt tip solid");
    assert!(
        eval(&e, [0.0, 0.0, -17.0]).v > 0.0,
        "just past the tip: vacuum"
    );
    assert!(eval(&e, [0.0, 0.0, -18.0]).v > 0.0, "far below: vacuum");
    assert!(
        eval(&e, [6.0, 0.0, -10.0]).v > 0.0,
        "beside the skirt: vacuum"
    );
}

#[test]
fn face_sweep_margin_shrinks_with_spacing() {
    // Same face sweep at spacing 0.5: the source band doubles to six node rows (|dist| ≤
    // 0.5 spans ±1 node above/below the face row) and the footprint fills in to 21×21
    // nodes; the 23-row swept hull spans 11.5 mm deep, 10.5 wide. The EDT margin shrinks
    // to a quarter spacing, so the prism converges toward the nominal face·len = 1000.
    let parent = bake_unpatched([5.0; 3], 0.5);
    let a = swept_face_field(&parent, [0.0, 0.0, -1.0], 10.0, [0.0, 0.0, -1.0])
        .expect("sweep succeeds");
    assert_eq!(a.dist.dims, [27, 27, 29]);
    // 10.5 × 10.5 × 11.5 = 1267.9 mm³; measured 1253.5, 1.1 % under (corner rounding) —
    // window 2 %, matching the coarse-spacing patch test.
    let v = volume_of(&a);
    assert!(
        (v - 1267.875).abs() < 1267.875 * 0.02,
        "fine-spacing face volume {v} vs EDT-implied 10.5·10.5·11.5 = 1267.9"
    );
}

#[test]
fn face_and_patch_recipes_key_apart() {
    const HASH: &str = "0123456789abcdef";
    let base = extend_key_face(HASH, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0], 10.0, 1.0);
    // Every ingredient of the face recipe is load-bearing: the axis, the sweep
    // direction, the length, the spacing and the parent hash. The patch recipe over the
    // same face shares none of it (different salt prefix), so a patch skirt and a face
    // skirt of the same parent never collide in the registry.
    let mut different = HashSet::new();
    different.insert(base);
    for k in [
        extend_key_face(HASH, [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], 10.0, 1.0),
        extend_key_face(HASH, [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], 10.0, 1.0),
        extend_key_face(HASH, [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], 10.0, 1.0),
        extend_key_face(HASH, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0], 12.0, 1.0),
        extend_key_face(HASH, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0], 10.0, 0.5),
        extend_key_face(
            "fedcba9876543210",
            [0.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
            10.0,
            1.0,
        ),
        extend_key(HASH, &[1], [0.0, 0.0, -1.0], 10.0, 1.0),
    ] {
        different.insert(k);
    }
    assert_eq!(different.len(), 8, "distinct recipes hash apart");
    // And neither recipe collides with the parent mesh key it derives from.
    assert_ne!(base, key_from_hex(HASH));
}

#[test]
fn swept_face_field_rejects_unusable_inputs() {
    let parent = bake_unpatched([5.0; 3], 1.0);
    let axis = [0.0, 0.0, -1.0];
    assert!(swept_face_field(&parent, [0.0, 0.0, 0.0], 10.0, axis)
        .expect_err("zero direction is refused")
        .contains("zero"));
    assert!(swept_face_field(&parent, [0.0, 0.0, -1.0], 0.0, axis).is_err());
    assert!(swept_face_field(&parent, [0.0, 0.0, -1.0], -5.0, axis).is_err());
    assert!(swept_face_field(&parent, [0.0, 0.0, -1.0], f64::NAN, axis).is_err());
    for bad_axis in [[f64::NAN, 0.0, 0.0], [1.0, 0.0, f64::INFINITY]] {
        let e = swept_face_field(&parent, [0.0, 0.0, -1.0], 10.0, bad_axis)
            .expect_err("non-finite face axis is refused");
        assert!(e.contains("face direction is not usable"), "got: {e}");
    }
    // A grid too thin for the ±1-cell gradient stencil is refused before it is read.
    let mut thin = bake_unpatched([5.0; 3], 1.0);
    Arc::make_mut(&mut thin.dist).dims = [3, 3, 2];
    let e =
        swept_face_field(&thin, [0.0, 0.0, -1.0], 10.0, axis).expect_err("thin grid is refused");
    assert!(e.contains("too thin"), "got: {e}");
    // A field with no near-surface cells (all air at +2) selects nothing.
    let mut far = bake_unpatched([5.0; 3], 1.0);
    Arc::make_mut(&mut far.dist).data.fill(2.0);
    let e = swept_face_field(&far, [0.0, 0.0, -1.0], 10.0, axis)
        .expect_err("a face with no near-surface cells is refused");
    assert!(e.contains("no near-surface cell"), "got: {e}");
}

#[test]
fn face_region_compiles_and_unions_with_the_parent() {
    let parent = bake_unpatched([5.0; 3], 1.0);
    let key = test_key("face-region");
    let hash = format!("{key:016x}");
    hydrate(key, parent.clone());
    let doc = extend_doc_face(
        &hash,
        [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
        "bottom",
        [0.0, 0.0, -1.0],
        10.0,
    );
    // The extend check accepts a mesh face selector with no patch grid: this is the
    // check_extends parents rule this milestone added.
    let errs = doc.validate();
    assert!(errs.is_empty(), "doc validates clean: {errs:?}");
    let body = compile_body(&doc);

    let Expr::Combine {
        op: Op::Add,
        blend: 0.0,
        a,
        b,
    } = &body
    else {
        panic!("expected an unblended two-leaf Add body, got {body:?}");
    };
    assert!(
        matches!(**a, Expr::Mesh { spec: Some(_), .. })
            && matches!(**b, Expr::Mesh { spec: Some(_), .. }),
        "both leaves are mesh nodes carrying grid slots"
    );
    // The derived field hydrated under the face recipe key (axis and dir both in the
    // identity leaf frame here).
    assert!(
        lookup(extend_key_face(
            &hash,
            [0.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
            10.0,
            1.0
        ))
        .is_some(),
        "face-derived leaf hydrated"
    );

    // Above the swept band the parent's samples win untouched.
    let parent_leaf = &**a;
    for p in [[0.0, 0.0, 0.0], [0.0, 0.0, -3.0], [4.0, 3.0, 2.0]] {
        let pv = eval(parent_leaf, p).v;
        let bv = eval(&body, p).v;
        assert!(pv < 0.0, "parent solid at {p:?}");
        assert_eq!(bv, pv, "body untouched by the sweep at {p:?}");
    }
    // Below the face the skirt is solid where the parent was air.
    assert!(eval(&body, [0.0, 0.0, -6.0]).v < 0.0, "skirt solid");
    assert!(eval(&body, [0.0, 0.0, -16.0]).v < 0.0, "skirt tip solid");
    assert!(
        eval(&body, [0.0, 0.0, -18.0]).v > 0.0,
        "past the skirt: vacuum"
    );

    // Bounds: the parent box union the swept hull's padded bbox (the skirt reaches wider
    // and deeper than the parent, the box's top stays the parent's).
    let want = Aabb {
        lo: [-5.5, -5.5, -16.5],
        hi: [5.5, 5.5, 5.0],
    };
    assert_eq!(bounds(&body), want);

    // Volume bookkeeping: the wedge where the extension overlaps the parent is exactly the
    // both-solid region inside the overlap box, integrated directly rather than
    // hard-coded.
    let extend_leaf = &**b;
    let wedge_box = Aabb {
        lo: [-5.5, -5.5, -5.0],
        hi: [5.5, 5.5, -3.5],
    };
    let wedge = integrate_with(
        &|p| D::new(eval(parent_leaf, p).v.max(eval(extend_leaf, p).v), [0.0; 3]),
        wedge_box,
        0.25,
        None,
    );
    let v_body = integrate_with(&|p| eval(&body, p), bounds(&body), 0.25, None);
    let v_parent = integrate_with(&|p| eval(parent_leaf, p), bounds(parent_leaf), 0.25, None);
    let derived = swept_face_field(&parent, [0.0, 0.0, -1.0], 10.0, [0.0, 0.0, -1.0])
        .expect("sweep succeeds");
    let v_ext = volume_of(&derived);
    let want_v = v_parent + v_ext - wedge;
    assert!(
        (v_body - want_v).abs() < want_v * 0.02,
        "union volume {v_body} vs parent {v_parent} + extension {v_ext} − wedge {wedge} = {want_v}"
    );
}

#[test]
fn face_region_follows_the_parent_transform() {
    // Rotated parent (+90° about y maps local (x, z) → world (z, −x)). The face axis and
    // the sweep direction are both read in the DOCUMENT frame and mapped through the node
    // placement into the leaf grid: axis (0, 0, −1) → leaf (1, 0, 0) selects the leaf +x
    // wall's band, dir (0, 0, −1) → leaf (1, 0, 0) sweeps along that wall's own outward
    // normal, so the skirt hangs straight down (world −z) from the wall that faces down
    // after rotation. A leaf-frame reader would sweep leaf −z and land the skirt at world
    // −x instead.
    let parent = bake_unpatched([5.0; 3], 1.0);
    let key = test_key("face-transform");
    let hash = format!("{key:016x}");
    hydrate(key, parent.clone());
    let mut spun = extend_doc_face(
        &hash,
        [-5.0, -5.0, -5.0, 5.0, 5.0, 5.0],
        "bottom",
        [0.0, 0.0, -1.0],
        10.0,
    );
    spun.root[0].transform = Transform {
        translate: [0.0, 0.0, 0.0],
        rotate: [0.0, 90.0, 0.0],
    };
    let errs = spun.validate();
    assert!(errs.is_empty(), "doc validates clean: {errs:?}");
    let body = compile_body(&spun);

    assert!(eval(&body, [0.0, 0.0, 0.0]).v < 0.0, "rotated parent solid");
    assert!(
        eval(&body, [0.0, 0.0, -10.0]).v < 0.0,
        "skirt hangs below the world-down-facing wall"
    );
    assert!(eval(&body, [0.0, 0.0, -16.0]).v < 0.0, "skirt tip solid");
    assert!(
        eval(&body, [0.0, 0.0, -18.0]).v > 0.0,
        "past the skirt: vacuum"
    );
    assert!(
        eval(&body, [-11.0, 0.0, -5.0]).v > 0.0,
        "where a leaf-frame dir sweep would land: vacuum"
    );
    // The derived leaf hydrated with BOTH mappings applied: the key's axis and dir are the
    // leaf-frame images of the document (0, 0, −1), computed through the same transform
    // helper the compiler uses (the 90° rotation leaves ~6e-17 of dust on the x components,
    // so the recipe key is derived here rather than hard-coded).
    let rig = fieldsmith_field::expr::Rigid::from_doc(&spun.root[0].transform);
    let leaf = rig.to_local_dir([0.0, 0.0, -1.0]);
    assert!(
        (leaf[0] - 1.0).abs() < 1e-12 && leaf[1].abs() < 1e-12,
        "the document-down axis maps to the leaf +x face: {leaf:?}"
    );
    assert!(
        lookup(extend_key_face(&hash, leaf, leaf, 10.0, 1.0)).is_some(),
        "rotated face-derived leaf hydrated under its leaf-frame key"
    );

    let b = bounds(&body);
    let eps = 1e-9;
    assert!(
        (b.lo[0] - (-5.5)).abs() < eps,
        "rotated union lo x: {:?}",
        b.lo
    );
    assert!(
        (b.lo[1] - (-5.5)).abs() < eps,
        "rotated union lo y: {:?}",
        b.lo
    );
    assert!(
        (b.lo[2] - (-16.5)).abs() < eps,
        "rotated union lo z: {:?}",
        b.lo
    );
    assert!(
        (b.hi[0] - 5.5).abs() < eps,
        "rotated union hi x: {:?}",
        b.hi
    );
    assert!(
        (b.hi[1] - 5.5).abs() < eps,
        "rotated union hi y: {:?}",
        b.hi
    );
    assert!(
        (b.hi[2] - 5.0).abs() < eps,
        "rotated union hi z: {:?}",
        b.hi
    );
}
