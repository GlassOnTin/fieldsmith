//! M6 step 7 — the `.fsmz` sidecar round trip.
//!
//! Save → load → save is byte-identical and the load re-hydrates the registry with the
//! exact saved grids (f32 / u16 bit for bit). The named-failure paths: a corrupted payload
//! names its entry (`<hash>.dist`), a manifest entry with no payload names its hash, a
//! document/manifest disagreement names both, and an unreferenced entry is refused.
//! A two-arm extend (face rule + patch) round-trips with one manifest record per arm, its
//! face axis stored in the parent's local grid frame.

use fieldsmith_core::fsmz;
use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_doc::{Document, Face, Node, NodeKind, Op, Region, Selector, Transform};
use fieldsmith_field::meshfield::{key_from_hex, lookup};
use fieldsmith_mesh::zip::{read_zip, ZipWriter};

/// ASCII STL of an axis-aligned box with outward windings (12 triangles, 6 flat patches).
fn box_stl(lo: [f64; 3], hi: [f64; 3]) -> Vec<u8> {
    let v = |i: usize, j: usize, k: usize| {
        format!(
            "vertex {} {} {}",
            if i == 0 { lo[0] } else { hi[0] },
            if j == 0 { lo[1] } else { hi[1] },
            if k == 0 { lo[2] } else { hi[2] }
        )
    };
    type Quad = ((usize, usize, usize), (usize, usize, usize), (usize, usize, usize), (usize, usize, usize));
    let quads: [Quad; 6] = [
        ((0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)), // x−
        ((1, 1, 0), (1, 1, 1), (1, 0, 1), (1, 0, 0)), // x+
        ((1, 0, 0), (1, 0, 1), (0, 0, 1), (0, 0, 0)), // y−
        ((0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)), // y+
        ((0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)), // z−
        ((0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)), // z+
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

/// A one-mesh document plus its part's 16-hex hash; `src` salts the key so test cases
/// share the registry without colliding.
fn mesh_doc(src: &str) -> (Document, String) {
    let bytes = box_stl([0.0; 3], [10.0, 8.0, 6.0]);
    let mut doc = Document {
        name: "Sidecar".into(),
        ..Default::default()
    };
    let out = import_mesh(
        "stl",
        &bytes,
        &ImportOpts {
            units: None,
            up: None,
            voxel: Some(1.5),
            src,
            taken: &[],
            step_chord: None,
        },
    )
    .unwrap();
    let hash = out.parts[0].hash.clone();
    doc.root.extend(out.nodes);
    (doc, hash)
}

/// Byte span of a named entry's payload, walking local file headers the way `read_zip` does.
fn entry_span(bytes: &[u8], name: &str) -> Option<std::ops::Range<usize>> {
    let u32at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let u16at = |i: usize| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let mut i = 0usize;
    while i + 30 <= bytes.len() && u32at(i) == 0x04034b50 {
        let csize = u32at(i + 18) as usize;
        let nlen = u16at(i + 26) as usize;
        let s = i + 30;
        if bytes[s..s + nlen] == *name.as_bytes() {
            return Some(s + nlen..s + nlen + csize);
        }
        i = s + nlen + csize;
    }
    None
}

/// Re-pack the walked entries (an edited manifest, a dropped entry, an extra one …).
fn repack<'a, I: IntoIterator<Item = &'a fieldsmith_mesh::zip::ZipEntry>>(entries: I) -> Vec<u8> {
    let mut z = ZipWriter::new();
    for e in entries {
        z.entry(&e.name, &e.data);
    }
    z.finish()
}

#[test]
fn round_trip_is_bit_exact_and_rehydrates() {
    let (doc, hash) = mesh_doc("v10-fsmz-roundtrip");
    let key = key_from_hex(&hash);
    let before = lookup(key).expect("import hydrated");

    let bytes1 = fsmz::save(&doc).expect("save");
    let out = fsmz::load(&bytes1).expect("load");
    let after = lookup(key).expect("load re-hydrates");

    // The grids themselves are bit for bit.
    assert_eq!(
        before.dist.data, after.dist.data,
        "distance samples survive"
    );
    assert_eq!(
        before.patch.as_deref(),
        after.patch.as_deref(),
        "patch ids survive"
    );
    assert_eq!(before.meta, after.meta, "import summary survives");
    assert_eq!(before.dist.spacing, after.dist.spacing);

    // The sidecar manifest reads back as written.
    assert_eq!(out.manifest.meshes.len(), 1);
    let m = &out.manifest.meshes[0];
    assert_eq!(m.hash, hash);
    assert_eq!(m.units, "mm");
    assert_eq!(m.up, "z");
    assert_eq!(m.tris, 12);
    assert_eq!(m.patches, 6);
    assert!(m.patch);

    // And saving the reloaded document is byte-identical (fresh parse/serialization of
    // everything, grids included).
    let bytes2 = fsmz::save(&out.doc).expect("save again");
    assert_eq!(bytes1, bytes2);
}

#[test]
fn corrupt_payload_names_its_entry() {
    let (doc, hash) = mesh_doc("v10-fsmz-corrupt");
    let bytes = fsmz::save(&doc).expect("save");

    // One flipped byte anywhere in `<hash>.dist`'s compressed payload — here the last one.
    let dname = format!("{hash}.dist");
    let span = entry_span(&bytes, &dname).expect("dist entry present");
    let mut bytes = bytes;
    let last = span.end - 1;
    bytes[last] ^= 0x01;

    let err = fsmz::load(&bytes).unwrap_err();
    assert!(
        err.contains(&dname),
        "the corruption error names its entry, got: {err}"
    );
}

#[test]
fn missing_grid_entry_names_the_hash() {
    let (doc, hash) = mesh_doc("v10-fsmz-missing");
    let bytes = fsmz::save(&doc).expect("save");
    let entries = read_zip(&bytes).expect("reads");

    // Re-pack everything except the dist payload.
    let kept: Vec<_> = entries
        .iter()
        .filter(|e| e.name != format!("{hash}.dist"))
        .collect();
    let err = fsmz::load(&repack(kept)).unwrap_err();
    assert!(
        err.contains(&format!("mesh '{hash}'")),
        "the missing-entry error names the hash, got: {err}"
    );
}

/// A mesh node the manifest does not record (its record and payload entries both dropped)
/// is refused with the node's name and hash — previously it loaded with its grid silently
/// unhydrated. A hand-written empty manifest keeps the sidecar otherwise well-formed.
#[test]
fn mesh_node_without_manifest_record_is_refused() {
    let (doc, _hash) = mesh_doc("v10-fsmz-unrecorded");
    let bytes = fsmz::save(&doc).expect("save");
    let entries = read_zip(&bytes).expect("reads");
    let kept: Vec<_> = entries
        .iter()
        .filter(|e| e.name != "manifest.json" && !e.name.ends_with(".dist") && !e.name.ends_with(".patch"))
        .collect();
    let mut z = ZipWriter::new();
    for e in kept {
        z.entry(&e.name, &e.data);
    }
    z.entry("manifest.json", b"{\"v\":1,\"meshes\":[],\"extends\":[]}");
    let err = fsmz::load(&z.finish()).unwrap_err();
    assert!(
        err.contains("mesh1") && err.contains("no record"),
        "the unrecorded mesh is refused naming its node and hash, got: {err}"
    );
}

#[test]
fn manifest_document_disagreement_names_both() {
    let (doc, hash) = mesh_doc("v10-fsmz-mismatch");
    let bytes = fsmz::save(&doc).expect("save");
    let mut entries = read_zip(&bytes).expect("reads");
    for e in &mut entries {
        if e.name == "manifest.json" {
            let text = std::str::from_utf8(&e.data).unwrap();
            e.data = text.replace("\"tris\": 12", "\"tris\": 13").into_bytes();
        }
    }
    let err = fsmz::load(&repack(&entries)).unwrap_err();
    assert!(
        err.contains("tris") && err.contains(&hash),
        "the disagreement error names the field and the mesh, got: {err}"
    );
}

#[test]
fn unreferenced_entry_is_refused() {
    let (doc, _hash) = mesh_doc("v10-fsmz-extra");
    let bytes = fsmz::save(&doc).expect("save");
    let mut entries: Vec<_> = read_zip(&bytes).expect("reads").into_iter().collect();
    // A dist payload no manifest record claims — left behind by a botched edit.
    let bogus = "ff0f00ff0f00ff00.dist".to_string();
    entries.push(fieldsmith_mesh::zip::ZipEntry {
        name: bogus.clone(),
        data: vec![1, 2, 3, 4],
    });
    let err = fsmz::load(&repack(&entries)).unwrap_err();
    assert!(
        err.contains(&bogus),
        "the leftover error names the unreferenced entry, got: {err}"
    );
}

/// An extend node reading a region with a face-rule selector plus a patch selector, so the
/// save derives one swept grid per arm (a face-record with `face_axis`, a patch record
/// with `patch_ids`) and the load must pair each arm with its record.
#[test]
fn face_and_patch_arm_extends_round_trip() {
    let (mut doc, hash) = mesh_doc("v10-fsmz-facearm");
    // The box's six flat faces are patches 1..6 (first-triangle order); 1 is the x− face.
    doc.regions.push(Region {
        id: "bottomish".into(),
        selectors: vec![
            Selector {
                shape: "mesh1".into(),
                face: Face::Bottom,
                patch: None,
            },
            Selector {
                shape: "mesh1".into(),
                face: Face::All,
                patch: Some(1),
            },
        ],
    });
    doc.root.push(Node {
        id: "ext1".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        material: None,
        kind: NodeKind::Extend {
            region: "bottomish".into(),
            dir: [0.0, 0.0, -1.0],
            len: 4.0,
        },
    });

    let bytes1 = fsmz::save(&doc).expect("save with face + patch arms");
    let out = fsmz::load(&bytes1).expect("load pairs both arms with their records");
    assert_eq!(
        out.manifest.extends.len(),
        2,
        "one manifest record per region arm"
    );
    let record = |x: &&fsmz::ManifestExtend| x.id == "ext1";
    let face = out
        .manifest
        .extends
        .iter()
        .filter(record)
        .find(|x| x.face_axis.is_some())
        .expect("the face arm has its record");
    let patch = out
        .manifest
        .extends
        .iter()
        .filter(record)
        .find(|x| !x.patch_ids.is_empty())
        .expect("the patch arm has its record");
    assert_eq!(
        face.face_axis,
        Some([0.0, 0.0, -1.0]),
        "an identity transform maps the doc-frame bottom axis verbatim"
    );
    assert!(face.patch_ids.is_empty(), "the face arm records an axis, not patch ids");
    assert_eq!(face.region, "bottomish");
    assert_eq!(face.parent_hash, patch.parent_hash, "both arms name the same parent");
    assert_eq!(face.parent_hash, hash);
    assert_eq!(patch.patch_ids, vec![1], "the x− face is patch 1 of the six");
    assert!(patch.face_axis.is_none());

    let bytes2 = fsmz::save(&out.doc).expect("re-save");
    assert_eq!(bytes1, bytes2, "multi-arm extends round-trip byte-identical");
}

/// With the mesh node rotated, the sweep key must use the doc-frame dir mapped through the
/// parent's transform (the frame the derivation hashed) — on save and on load alike, or
/// the re-save would re-derive a different grid and the byte comparison would catch it.
#[test]
fn rotated_mesh_face_extend_round_trips() {
    let (mut doc, _hash) = mesh_doc("v10-fsmz-facearm-rot");
    let mesh = doc.root.first_mut().expect("one mesh node");
    mesh.transform = Transform {
        translate: [2.0, 0.0, 0.0],
        rotate: [90.0, 0.0, 0.0], // the mesh's local +y now points to doc +z
    };
    doc.regions.push(Region {
        id: "down".into(),
        selectors: vec![Selector {
            shape: "mesh1".into(),
            face: Face::Bottom,
            patch: None,
        }],
    });
    doc.root.push(Node {
        id: "ext1".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        material: None,
        kind: NodeKind::Extend {
            region: "down".into(),
            dir: [0.0, 0.0, -1.0],
            len: 4.0,
        },
    });

    let bytes1 = fsmz::save(&doc).expect("save");
    let out = fsmz::load(&bytes1).expect("load");
    let face = out.manifest.extends.iter().find(|x| x.id == "ext1").expect("record");
    let a = face.face_axis.expect("the face arm records its axis");
    for (got, want) in a.iter().zip([0.0, -1.0, 0.0]) {
        assert!(
            (got - want).abs() < 1e-9,
            "rot_x(90°) maps doc (0,0,-1) to local (0,-1,0), got {a:?}"
        );
    }

    let bytes2 = fsmz::save(&out.doc).expect("re-save");
    assert_eq!(bytes1, bytes2, "the frame-mapped extend round-trips byte-identical");
}
