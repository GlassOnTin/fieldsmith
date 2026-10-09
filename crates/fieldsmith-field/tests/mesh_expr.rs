//! Mesh `Expr` evaluation, registry hydration and shader emission (goal M6, step 1).
//!
//! The CPU `Expr::Mesh` path must behave like an ordinary leaf: hydrated fields sample with
//! sane signs, gradients and volumes; unhydrated keys evaluate as vacuum so a document is
//! neutral until its grids arrive; transforms, raymarching, patch lookup and GLSL emission
//! compose.

use fieldsmith_doc::{fsm, Document, Node, NodeKind, Op, Transform};
use fieldsmith_field::dual::D;
use fieldsmith_field::expr::{eval_mesh_patch, placed_shape, raymarch, Expr};
use fieldsmith_field::glsl;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    hash_mesh_input, hydrate, lookup, mesh_keys, patch_index, MeshField, MeshMeta, BAND_CELLS,
    NO_PATCH,
};
use fieldsmith_field::prim::Prim;
use fieldsmith_field::volume::integrate;
use fieldsmith_field::{bounds, compile_body, eval, Aabb};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Bake an exact box signed-distance field: nodes carry the true distance inside the band of
/// [`BAND_CELLS`]·spacing around the surface, clamped to ±band beyond (never overestimates).
fn bake_box(half: [f64; 3], spacing: f64, with_patches: bool) -> MeshField {
    let band = BAND_CELLS * spacing;
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
                // Interior depth is the largest *un-clamped* axis gap (its sign is negative).
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
    if with_patches {
        // Author patch ids on the cells that contain the ±z surface centres; this exercises
        // the lookup pipeline (cell_of → patch_index → grid id).
        let mut patch = vec![NO_PATCH; dims[0] * dims[1] * dims[2]];
        for (z, id) in [(-half[2], 1u16), (half[2], 2u16)] {
            let cell = field.cell_of([0.0, 0.0, z]).expect("probed cell exists");
            patch[patch_index(dims, cell)] = id;
        }
        field.patch = Some(Arc::new(patch));
        field.meta.patches = 2;
    }
    field
}

/// A document whose only node is a placed mesh leaf.
fn mesh_doc(key: u64, bbox: [f64; 6], transform: Transform) -> Document {
    Document {
        name: "mesh test".into(),
        root: vec![mesh_node("mesh1", format!("{key:016x}"), bbox, transform)],
        ..Default::default()
    }
}

fn mesh_node(id: &str, hash: String, bbox: [f64; 6], transform: Transform) -> Node {
    Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform,
        kind: NodeKind::Mesh {
            src: "test.stl".into(),
            hash,
            units: "mm".into(),
            up: "z".into(),
            voxel: 1.0,
            bbox,
            tris: 12,
            patches: 0,
            open_edges: 0,
        },
    }
}

/// Build the `Expr` leaf bound to a hydrated key (bbox and spec as compiled from the meta).
fn mesh_leaf(key: u64) -> Expr {
    let f = lookup(key).expect("field hydrated before building the leaf");
    Expr::Mesh {
        key,
        bbox: f.meta.bbox,
        spec: Some(f.spec()),
        face_hint: None,
    }
}

fn bbox6(half: [f64; 3]) -> [f64; 6] {
    [-half[0], -half[1], -half[2], half[0], half[1], half[2]]
}

fn placed_mesh(doc: &Document) -> Expr {
    placed_shape(doc, "mesh1").expect("mesh node exists")
}

#[test]
fn hydrated_box_signs_gradient_and_volume() {
    let half = [5.0; 3];
    let key = hash_mesh_input(b"mesh_expr:box", b"");
    hydrate(key, bake_box(half, 1.0, false));
    let e = mesh_leaf(key);
    // Centre: clamped interior depth = band.
    let c = eval(&e, [0.0, 0.0, 0.0]);
    assert_eq!(c.v, -BAND_CELLS * 1.0);
    // Just inside the +x face: exact (negative) distance, outward gradient.
    let near = eval(&e, [half[0] - 0.1, 0.0, 0.0]);
    assert!((near.v - (-0.1)).abs() < 1e-6, "v = {}", near.v);
    for (i, want) in [(0usize, 1.0), (1, 0.0), (2, 0.0)] {
        assert!((near.g[i] - want).abs() < 1e-6, "g[{i}] = {}", near.g[i]);
    }
    // Just outside (still within the band): exact.
    let out = eval(&e, [half[0] + 0.2, 0.0, 0.0]);
    assert!((out.v - 0.2).abs() < 1e-6, "v = {}", out.v);
    // Bounds come from the mesh, not the padded grid.
    let b = bounds(&e);
    assert_eq!(b.lo, [-5.0; 3]);
    assert_eq!(b.hi, [5.0; 3]);
    // Volume within 1% of the closed form.
    let v = integrate(&e, 0.25, None);
    assert!((v - 1000.0).abs() / 1000.0 < 0.01, "volume {v}");
}

#[test]
fn unhydrated_key_is_vacuum_and_neutral_in_a_document() {
    // A key no test has hydrated: the leaf itself and its document are vacuum.
    let key = hash_mesh_input(b"mesh_expr:never-hydrated", b"");
    assert!(lookup(key).is_none());
    let leaf = Expr::Mesh {
        key,
        bbox: Aabb {
            lo: [-3.0; 3],
            hi: [3.0; 3],
        },
        spec: None,
        face_hint: None,
    };
    assert_eq!(eval(&leaf, [0.0, 0.0, 0.0]), D::EMPTY);
    let mut doc = mesh_doc(key, bbox6([8.0; 3]), Transform::default());
    doc.root.insert(
        0,
        Node {
            id: "plate".into(),
            op: Op::Add,
            blend: 0.0,
            suppressed: false,
            transform: Transform::default(),
            kind: NodeKind::Box {
                size: [16.0; 3],
                round: 0.0,
            },
        },
    );
    let with_mesh = compile_body(&doc);
    let alone = Expr::Prim(Prim::Box {
        half: [8.0; 3],
        round: 0.0,
    });
    for p in [[0.0, 0.0, 0.0], [6.0, 3.0, -5.0], [9.0, 0.0, 0.0]] {
        assert_eq!(eval(&with_mesh, p).v, eval(&alone, p).v, "at {p:?}");
    }
    // The unhydrated node does not inflate bounds either.
    assert_eq!(bounds(&with_mesh), bounds(&alone));
}

#[test]
fn transform_wraps_mesh_sampling_world_space() {
    // A slab along x, rotated 90° about z: solid along y in world space.
    let half = [5.0, 1.0, 1.0];
    let key = hash_mesh_input(b"mesh_expr:slab", b"");
    hydrate(key, bake_box(half, 0.5, false));
    let doc = mesh_doc(key, bbox6(half), Transform::default());
    let placed = placed_mesh(&doc);
    assert!(
        eval(&placed, [0.0, 1.5, 0.0]).v > 0.0,
        "untransformed: outside the slab in y"
    );
    // Rotate 90° about z: the slab runs along y.
    let mut doc = doc.clone();
    doc.root[0].transform = Transform {
        translate: [0.0; 3],
        rotate: [0.0, 0.0, 90.0],
    };
    let e = placed_mesh(&doc);
    assert!(matches!(e, Expr::Xform { .. }), "transform wraps the leaf");
    // World (0, 4.5, 0) is local (4.5, 0, 0) → inside; the unrotated axis is outside.
    let inside = eval(&e, [0.0, 4.5, 0.0]);
    assert!(
        inside.v < 0.0,
        "rotated slab solid at (0, 4.5, 0): {}",
        inside.v
    );
    assert!(
        (inside.g[1] - 1.0).abs() < 1e-6,
        "gradient along the slab's short axis: {:?}",
        inside.g
    );
    assert!(
        eval(&e, [4.5, 0.0, 0.0]).v > 0.0,
        "empty beside the rotated slab"
    );
    // Rotation composed with a translation along x.
    doc.root[0].transform = Transform {
        translate: [10.0, 0.0, 0.0],
        rotate: [0.0, 0.0, 90.0],
    };
    let moved = placed_mesh(&doc);
    assert!(eval(&moved, [10.0, 4.5, 0.0]).v < 0.0);
    assert!(eval(&moved, [4.5, 0.0, 0.0]).v > 0.0);
}

#[test]
fn raymarch_hits_a_hydrated_mesh_surface() {
    let half = [3.0; 3];
    let key = hash_mesh_input(b"mesh_expr:raymarch", b"");
    hydrate(key, bake_box(half, 0.75, false));
    let e = mesh_leaf(key);
    let t = raymarch(&e, [0.0, 0.0, 10.0], [0.0, 0.0, -1.0], 100.0)
        .expect("ray toward the box terminates");
    assert!((t - (10.0 - half[2])).abs() < 0.05, "hit at t = {t}");
    // A ray running parallel to the surface misses rather than hanging.
    assert!(raymarch(&e, [5.0, 0.0, half[2] + 0.5], [1.0, 0.0, 0.0], 100.0).is_none());
}

#[test]
fn patch_at_returns_authored_ids() {
    let field = bake_box([5.0; 3], 1.0, true);
    assert_eq!(field.meta.patches, 2);
    assert_eq!(field.patch_at([0.0, 0.0, 5.0]), Some(2), "top");
    assert_eq!(field.patch_at([0.0, 0.0, -5.0]), Some(1), "bottom");
    assert_eq!(field.patch_at([0.0, 0.0, 0.0]), None, "interior");
    assert_eq!(field.patch_at([5.0, 0.0, 0.0]), None, "side face");
    let plain = bake_box([5.0; 3], 1.0, false);
    assert_eq!(plain.patch_at([0.0, 0.0, 5.0]), None, "no patch grid");
}

#[test]
fn eval_mesh_patch_follows_transform_and_prims_are_none() {
    let half = [5.0; 3];
    let key = hash_mesh_input(b"mesh_expr:patch_eval", b"");
    hydrate(key, bake_box(half, 1.0, true));
    let doc = mesh_doc(key, bbox6(half), Transform::default());
    let placed = placed_mesh(&doc);
    assert_eq!(eval_mesh_patch(&placed, [0.0, 0.0, 5.0]), Some(2), "top");
    assert_eq!(
        eval_mesh_patch(&placed, [0.0, 0.0, -5.0]),
        Some(1),
        "bottom"
    );
    assert_eq!(eval_mesh_patch(&placed, [0.0, 0.0, 0.0]), None, "interior");
    assert_eq!(eval_mesh_patch(&placed, [5.0, 0.0, 0.0]), None, "side face");

    // Rotated 90° about x, the box's local bottom patch faces world +y and the patch query
    // follows the Xform's local mapping.
    let mut doc = doc.clone();
    doc.root[0].transform = Transform {
        translate: [0.0; 3],
        rotate: [90.0, 0.0, 0.0],
    };
    let placed = placed_mesh(&doc);
    assert_eq!(
        eval_mesh_patch(&placed, [0.0, 5.0, 0.0]),
        Some(1),
        "local bottom → world +y"
    );
    assert_eq!(
        eval_mesh_patch(&placed, [0.0, -5.0, 0.0]),
        Some(2),
        "local top → world −y"
    );

    // A primitive leaf never claims a patch, even on its own surface.
    let mut doc = doc.clone();
    doc.root[0].kind = NodeKind::Box {
        size: [10.0; 3],
        round: 0.0,
    };
    let prim = placed_mesh(&doc);
    assert_eq!(eval_mesh_patch(&prim, [0.0, 0.0, 5.0]), None);
}

#[test]
fn registry_replace_reflects_the_new_field() {
    let key = hash_mesh_input(b"mesh_expr:replace", b"");
    hydrate(key, bake_box([2.0; 3], 1.0, false));
    let bbox = lookup(key).unwrap().meta.bbox;
    let e = Expr::Mesh {
        key,
        bbox,
        spec: None,
        face_hint: None,
    };
    let p = [1.5, 0.0, 0.0];
    assert!(eval(&e, p).v < 0.0, "first bake is solid at (1.5, 0, 0)");
    // Re-hydrate the same key with a smaller box: the same Expr must read the new data
    // (hydrate refreshes this thread's memo).
    hydrate(key, bake_box([0.5; 3], 1.0, false));
    let after = eval(&e, p);
    assert!(
        after.v > 0.5,
        "re-hydrated key samples the new data: {}",
        after.v
    );
    // A cold thread's memo reads the registry fresh.
    let other = std::thread::spawn(move || eval(&e, p).v);
    assert!(other.join().unwrap() > 0.5, "cold thread sees the replace");
}

/// Every `fs_meshN` name appearing in a GLSL text (definitions and references share the
/// shape `fs_mesh<digits>`; `u_meshNp` sampler names are a different prefix and ignored).
fn mesh_fn_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find("fs_mesh") {
        rest = &rest[i + "fs_mesh".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            out.insert(format!("fs_mesh{digits}"));
        }
    }
    out
}

#[test]
fn glsl_generate_emits_mesh_sampler_and_spec() {
    let key = hash_mesh_input(b"mesh_expr:glsl", b"");
    let spec = {
        let f = bake_box([4.0; 3], 0.5, true);
        let s = f.spec();
        hydrate(key, f);
        s
    };
    let mut doc = mesh_doc(key, bbox6([4.0; 3]), Transform::default());
    doc.root.insert(
        0,
        Node {
            id: "pad".into(),
            op: Op::Subtract,
            blend: 0.0,
            suppressed: false,
            transform: Transform::default(),
            kind: NodeKind::Box {
                size: [2.0; 3],
                round: 0.0,
            },
        },
    );
    let e = compile_body(&doc);
    let mut keys = BTreeSet::new();
    mesh_keys(&e, &mut keys);
    assert_eq!(keys, BTreeSet::from([key]), "mesh_keys walks the tree");
    let g = glsl::generate(&e);
    for needle in [
        "fs_mesh0(p)",
        "uniform highp sampler3D u_mesh0;",
        "uniform highp usampler3D u_mesh0p;",
        "texelFetch",
    ] {
        assert!(g.source.contains(needle), "missing {needle}");
    }
    assert_eq!(g.meshes.len(), 1, "one mesh texture declared");
    let m = &g.meshes[0];
    assert_eq!(m.slot, 0);
    assert_eq!(m.key, key);
    assert_eq!(m.dims, spec.dims);
    assert_eq!(m.spacing, spec.spacing);
    assert!(m.patch, "patch grid declared");
    // The ident pass is glued after the body in one fragment program: the mesh leaf
    // references the body's `fs_mesh0` and the body-declared `u_mesh0p` fetch, with the
    // cell math of `MeshField::cell_of` and the patch code as `PATCH_CODE0 + id` (the
    // literal form of 10 under `lit` is "1.0e1"). Numeric GPU↔CPU agreement is measured
    // in step 9's Playwright e2e — cargo cannot execute WebGL.
    let (src, ids) = glsl::ident(&doc);
    assert_eq!(ids.len(), 2, "mesh leaf participates in identification");
    assert!(src.contains("fs_ident(vec3 p)"), "{src}");
    assert!(
        src.contains("fs_mesh0(p).x"),
        "ident reads the body field: {src}"
    );
    assert!(
        src.contains("u_mesh0p, ivec3(floor(u)), 0).r"),
        "patch fetch: {src}"
    );
    assert!(
        src.contains("1.0e1 + float(texelFetch(u_mesh0p"),
        "patch code: {src}"
    );
    // `clamp((p - origin) / spacing, 0, n - 1)` mirrors `cell_of` exactly.
    assert!(
        src.contains("clamp((p - vec3("),
        "ident cell coordinates clamp to the last node: {src}"
    );
    assert!(
        !src.contains("uniform"),
        "ident declares nothing of its own: {src}"
    );
    // The overlay stays a standalone program: mesh leaves vacuum out there (no sampler
    // reference, no unbound sampler).
    let ov = glsl::overlay(
        &doc,
        &[glsl::OverlayItem {
            selectors: vec![fieldsmith_doc::Selector {
                shape: "mesh1".into(),
                face: fieldsmith_doc::Face::All,
                patch: None,
            }],
            color: [1.0, 0.4, 0.2],
        }],
    );
    assert!(
        ov.contains("return vec2(1e9, 0.0);") || ov.contains("vec2(1e9, 0.0)"),
        "{ov}"
    );
    assert!(!ov.contains("fs_mesh0"), "overlay must not bind samplers");
}

/// An unpatched mesh declares no `usampler3D`; its ident reports face code 0 (Face::All),
/// matching CPU `eval_face`.
#[test]
fn glsl_unpatched_mesh_ident_reports_face_all() {
    let key = hash_mesh_input(b"mesh_expr:glsl-plain", b"");
    hydrate(key, bake_box([3.0; 3], 0.5, false));
    let doc = mesh_doc(key, bbox6([3.0; 3]), Transform::default());
    let e = compile_body(&doc);
    let g = glsl::generate(&e);
    assert!(
        !g.source.contains("usampler3D"),
        "no patch sampler: {}",
        g.source
    );
    assert!(
        g.meshes.iter().all(|m| !m.patch),
        "spec carries patch=false"
    );
    let (src, _) = glsl::ident(&doc);
    assert!(src.contains("return vec2(fs_mesh0(p).x, 0.0);"), "{src}");
    assert!(
        !src.contains("usampler3D"),
        "no patch fetch anywhere: {src}"
    );
}

/// Structural probe standing in for numeric GPU/CPU parity (cargo cannot execute WebGL;
/// the numeric check is step 9's Playwright e2e): in the glued program — body source plus
/// ident source, exactly what fragSource concatenates — every `fs_meshN` the ident
/// references must have been defined by the body pass, and each sampler uniform must be
/// declared exactly once.
#[test]
fn glsl_ident_references_are_covered_by_body_definitions() {
    let ka = hash_mesh_input(b"mesh_expr:probe-a", b"");
    let kb = hash_mesh_input(b"mesh_expr:probe-b", b"");
    // Slots are the BTreeSet ranks, so slot 0 is the smaller key: make it patched; slot 1
    // stays unpatched so both ident branches are exercised in one program.
    let (k0, k1) = (ka.min(kb), ka.max(kb));
    hydrate(k0, bake_box([2.0; 3], 0.5, true));
    let spec1 = bake_box([5.0; 3], 1.0, false).spec();
    assert!(!spec1.patch);
    hydrate(k1, bake_box([5.0; 3], 1.0, false));
    let mut doc = mesh_doc(k0, bbox6([2.0; 3]), Transform::default());
    doc.root.push(mesh_node(
        "mesh2",
        format!("{k1:016x}"),
        bbox6([5.0; 3]),
        Transform::default(),
    ));
    let e = compile_body(&doc);
    let g = glsl::generate(&e);
    let (isrc, ids) = glsl::ident(&doc);
    assert_eq!(ids.len(), 2, "both mesh leaves identify");

    let defs = mesh_fn_names(&g.source);
    let refs = mesh_fn_names(&format!("{}\n{}", g.source, isrc));
    assert_eq!(
        defs,
        BTreeSet::from(["fs_mesh0".into(), "fs_mesh1".into()]),
        "{defs:?}"
    );
    assert_eq!(refs, defs, "every ident reference has a body definition");

    assert_eq!(g.meshes.len(), 2, "both slots declared");
    assert!(g
        .meshes
        .iter()
        .any(|m| m.slot == 0 && m.key == k0 && m.patch));
    assert!(g
        .meshes
        .iter()
        .any(|m| m.slot == 1 && m.key == k1 && !m.patch));
    assert_eq!(
        g.meshes.iter().find(|m| m.slot == 1).unwrap().dims,
        spec1.dims,
        "slot 1 carries the unpatched mesh's spec"
    );
    assert!(
        isrc.contains("fs_mesh1(p).x, 0.0"),
        "unpatched leaf reports code 0: {isrc}"
    );
    // Exactly one declaration per sampler in the glued program.
    let glued = format!("{}\n{}", g.source, isrc);
    assert_eq!(glued.matches("uniform highp sampler3D u_mesh0;").count(), 1);
    assert_eq!(
        glued.matches("uniform highp usampler3D u_mesh0p;").count(),
        1
    );
    assert_eq!(glued.matches("uniform highp sampler3D u_mesh1;").count(), 1);
}

/// An unhydrated mesh key bakes `spec: None`; neither the field nor the ident pass may
/// reference a sampler for it (the vacuum mirrors CPU `D::EMPTY`).
#[test]
fn glsl_unhydrated_mesh_stays_vacuum_in_field_and_ident() {
    let key = hash_mesh_input(b"mesh_expr:glsl-never", b"");
    assert!(lookup(key).is_none());
    let doc = mesh_doc(key, bbox6([8.0; 3]), Transform::default());
    let e = compile_body(&doc);
    let g = glsl::generate(&e);
    assert!(g.meshes.is_empty(), "no texture for an unhydrated mesh");
    assert!(!g.source.contains("fs_mesh"), "{}", g.source);
    assert!(!g.source.contains("u_mesh"), "{}", g.source);
    let (src, ids) = glsl::ident(&doc);
    assert_eq!(ids.len(), 1, "the unhydrated leaf still participates");
    assert!(!src.contains("fs_mesh"), "{src}");
    assert!(src.contains("1e9"), "vacuum distance: {src}");
}

#[test]
fn selector_patch_prints_and_validates() {
    let mut doc = fsm::parse("fieldsmith 1\nbox b size=(10, 10, 10)\nregion r = b.top\n").unwrap();
    let text = fsm::print(&doc);
    assert!(text.contains("region r = b.top"), "{text}");
    doc.regions[0].selectors[0].patch = Some(3);
    let text = fsm::print(&doc);
    assert!(text.contains("region r = b.p3"), "{text}");
    // Selecting a patch of a non-mesh shape is invalid.
    let errs = doc.validate();
    assert!(
        errs.iter().any(|e| e.contains("not a mesh node")),
        "{errs:?}"
    );
    // Pointing at a real mesh node validates clean when the patch id is inside the
    // import's segmentation; id 0 and ids beyond the segmented count are refused.
    let bbox = bbox6([4.0; 3]);
    let mut doc = mesh_doc(
        hash_mesh_input(b"mesh_expr:sel", b""),
        bbox,
        Transform::default(),
    );
    if let NodeKind::Mesh { patches, .. } = &mut doc.root[0].kind {
        *patches = 6;
    }
    doc.regions.push(fieldsmith_doc::Region {
        id: "r".into(),
        selectors: vec![fieldsmith_doc::Selector {
            shape: "mesh1".into(),
            face: fieldsmith_doc::Face::All,
            patch: Some(3),
        }],
    });
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    doc.regions[0].selectors[0].patch = Some(7);
    assert!(
        doc.validate()
            .iter()
            .any(|e| e.contains("segmented 6 patches")),
        "patch 7 exceeds the mesh's 6 patches: {:?}",
        doc.validate()
    );
    doc.regions[0].selectors[0].patch = Some(0);
    assert!(
        doc.validate().iter().any(|e| e.contains("start at 1")),
        "patch 0 is not a valid id"
    );
}

#[test]
fn mesh_node_field_validation() {
    let mut doc = Document::default();

    doc.root.push(mesh_node(
        "mesh1",
        "zzzz".into(), // not hex
        bbox6([4.0; 3]),
        Transform::default(),
    ));
    let errs = doc.validate();
    assert!(errs.iter().any(|e| e.contains("hex")), "{errs:?}");

    doc.root[0] = mesh_node(
        "mesh1",
        "0123456789abcdef".into(),
        bbox6([4.0; 3]),
        Transform::default(),
    );
    if let NodeKind::Mesh { units, .. } = &mut doc.root[0].kind {
        *units = "parsecs".into();
    }
    assert!(doc.validate().iter().any(|e| e.contains("mm, cm, m, in")));

    if let NodeKind::Mesh { up, .. } = &mut doc.root[0].kind {
        *up = "x".into();
    }
    assert!(doc.validate().iter().any(|e| e.contains("y or z")));

    if let NodeKind::Mesh { voxel, .. } = &mut doc.root[0].kind {
        *voxel = -1.0;
    }
    assert!(doc.validate().iter().any(|e| e.contains("voxel")));

    if let NodeKind::Mesh { bbox, .. } = &mut doc.root[0].kind {
        bbox[0] = 100.0; // xmin > xmax
    }
    assert!(doc.validate().iter().any(|e| e.contains("inverted")));

    // Five mesh nodes exceed the four-texture budget.
    let mut doc = Document::default();
    for i in 0..5 {
        doc.root.push(mesh_node(
            &format!("mesh{i}"),
            format!("{:016x}", i as u64 + 1),
            bbox6([4.0; 3]),
            Transform::default(),
        ));
    }
    let errs = doc.validate();
    assert!(errs.iter().any(|e| e.contains("at most 4")), "{errs:?}");
    // Four are fine.
    doc.root.pop();
    assert!(!doc.validate().iter().any(|e| e.contains("at most 4")));
}
