//! Reader tests (goal M6, step 2): STL (binary + ASCII), GLB and OBJ, including every
//! named-rejection path. GLB fixtures are assembled in-test with a small chunk builder.

use fieldsmith_mesh::export::{read_ascii_stl, read_stl, read_stl_any, TriMesh};
use fieldsmith_mesh::gltf::read_glb;
use fieldsmith_mesh::obj::read_obj;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------------------------
// GLB fixture builder

/// A chunk = 8-byte header ([len][type]) + payload padded to a 4-byte boundary. JSON pads
/// with spaces, BIN with zeros (the spec's requirement — our reader is agnostic but the
/// fixtures stay true). The declared length includes the padding.
fn chunk(ty: &[u8; 4], data: &[u8], pad: u8) -> Vec<u8> {
    let n = data.len().next_multiple_of(4);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    out.resize(8 + n, pad);
    out
}

/// Assemble a GLB from JSON text and a BIN payload.
fn build_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let jc = chunk(b"JSON", json.as_bytes(), b' ');
    let bc = chunk(b"BIN\0", bin, 0);
    let total = (12 + jc.len() + bc.len()) as u32;
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend(jc);
    out.extend(bc);
    out
}

fn f32s(vals: &[f32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u16s(vals: &[u16]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u32s(vals: &[u32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn i16s(vals: &[i16]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u8s(vals: &[u8]) -> Vec<u8> {
    vals.to_vec()
}

/// Document + binary payload for the standard one-triangle GLB: vertices (0,0,0), (1,0,0),
/// (0,1,0), indices 0..2 as u16.
fn tri_glb() -> (Value, Vec<u8>) {
    let pos = f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let idx = u16s(&[0, 1, 2]);
    let doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"name": "tri", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "accessors": [
            {"componentType": 5126, "count": 3, "type": "VEC3", "bufferView": 0},
            {"componentType": 5123, "count": 3, "type": "SCALAR", "bufferView": 1}
        ],
        "bufferViews": [
            {"buffer": 0, "byteLength": pos.len()},
            {"buffer": 0, "byteOffset": pos.len(), "byteLength": idx.len()}
        ],
        "buffers": [{"byteLength": pos.len() + idx.len()}]
    });
    let mut bin = pos.clone();
    bin.extend(&idx);
    (doc, bin)
}

fn to_glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
    build_glb(&doc.to_string(), bin)
}

/// Positions come back through f32, so rotation-matrix products carry ~1e-16 error:
/// compare component-wise within 1e-9.
fn approx(a: &[[f64; 3]], b: &[[f64; 3]], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}: vertex count");
    for (i, (pa, pb)) in a.iter().zip(b.iter()).enumerate() {
        for c in 0..3 {
            assert!(
                (pa[c] - pb[c]).abs() < 1e-9,
                "{what}: vert {i} component {c}: {pa:?} vs {pb:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// STL

#[test]
fn binary_stl_round_trip() {
    let verts: Vec<[f64; 3]> = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let tris = vec![[0u32, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]];
    let m = TriMesh { verts, tris };
    let bytes = m.write_stl();
    let back = read_stl_any(&bytes).expect("binary STL parses");
    assert_eq!(back.verts, m.verts);
    assert_eq!(back.tris, m.tris);
}

#[test]
fn ascii_stl_parses_and_garbage_fails() {
    // A tetrahedron in ASCII STL; consecutive corner triples commit as triangles and the
    // equal-corner weld gives 4 vertices.
    let text = "\nsolid tet\nfacet normal 0 0 1\nouter loop\n\
                 vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\n\
                 facet normal 0 -1 0\nouter loop\n\
                 vertex 0 0 0\nvertex 0 0 1\nvertex 1 0 0\nendloop\nendfacet\n\
                 facet normal 1 0 0\nouter loop\n\
                 vertex 0 0 0\nvertex 0 1 0\nvertex 0 0 1\nendloop\nendfacet\n\
                 facet normal -1 -1 -1\nouter loop\n\
                 vertex 1 0 0\nvertex 0 1 0\nvertex 0 0 1\nendloop\nendfacet\n\
                 endsolid tet\n";
    let m = read_ascii_stl(text).expect("ascii tet parses");
    assert_eq!(m.verts.len(), 4);
    assert_eq!(m.tris.len(), 4);
    assert!(m.verts.contains(&[1.0, 0.0, 0.0]));
    // Same bytes through read_stl_any.
    let back = read_stl_any(text.as_bytes()).expect("ascii STL through read_stl_any");
    assert_eq!(back.tris, m.tris);
    // And the same mesh through the binary path welds identically (equal f32 corners).
    assert_eq!(read_stl(&m.write_stl()).unwrap().verts, m.verts);

    // Garbage: no triangles recognised.
    assert!(read_ascii_stl("hello world\nnothing here\n").is_none());
    assert!(read_stl_any(b"hello world").is_none());
    // Unparseable coordinates: None rather than a panic.
    assert!(read_ascii_stl("vertex x y z\nvertex 0 0 0\nvertex 1 0 0\n").is_none());
    // The strict binary reader must reject the ASCII text.
    assert!(read_stl(text.as_bytes()).is_none());
}

// ---------------------------------------------------------------------------------------------
// GLB

#[test]
fn glb_minimal_triangle() {
    let (mut doc, bin) = tri_glb();
    // Unknown top-level keys are ignored.
    doc["extras"] = json!({"whatever": [1, 2, 3]});
    doc["images"] = json!([{"uri": "unused.png"}]);
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts.len(), 1);
    let p = &parts[0];
    assert_eq!(p.name, "tri");
    assert_eq!(
        p.mesh.verts,
        vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(p.mesh.tris, vec![[0u32, 1, 2]]);
}

#[test]
fn glb_transform_baking() {
    // (a) scale + translation (TRS)
    let (mut doc, bin) = tri_glb();
    doc["nodes"][0] = json!({"mesh": 0, "translation": [10.0, 0.0, 0.0], "scale": [2.0, 2.0, 2.0]});
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(
        parts[0].mesh.verts,
        vec![[10.0, 0.0, 0.0], [12.0, 0.0, 0.0], [10.0, 2.0, 0.0]]
    );

    // (b) rotation: 90° about z via the unit quaternion [0, 0, √2/2, 1]; (1,0,0) → (0,1,0).
    let (mut doc, bin) = tri_glb();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    doc["nodes"][0] = json!({"mesh": 0, "rotation": [0.0, 0.0, s, s]});
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    approx(
        &parts[0].mesh.verts,
        &[[0.0; 3], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]],
        "rotation",
    );

    // (c) an explicit column-major matrix doing the same rotation plus translation.
    let (mut doc, bin) = tri_glb();
    doc["nodes"][0] = json!({"mesh": 0, "matrix": [0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0,
                                                   0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0]});
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    approx(
        &parts[0].mesh.verts,
        &[[5.0, 6.0, 7.0], [5.0, 7.0, 7.0], [4.0, 6.0, 7.0]],
        "matrix",
    );

    // (d) TRS composed down the tree: the parent rotates 90°, the child translates; the
    // child placement applies first, then the parent's rotation.
    let (mut doc, bin) = tri_glb();
    doc["scenes"][0]["nodes"] = json!([1]);
    doc["nodes"] = json!([
        {"mesh": 0, "translation": [3.0, 0.0, 0.0]},
        {"name": "spin", "rotation": [0.0, 0.0, s, s], "children": [0]}
    ]);
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts[0].name, "tri", "mesh name wins over the node hint");
    approx(
        &parts[0].mesh.verts,
        &[[0.0, 3.0, 0.0], [0.0, 4.0, 0.0], [-1.0, 3.0, 0.0]],
        "parent rotate + child translate",
    );
}

#[test]
fn glb_instancing_and_names() {
    let (mut doc, bin) = tri_glb();
    doc["nodes"] = json!([
        {"name": "left", "mesh": 0},
        {"name": "right", "mesh": 0, "translation": [10.0, 0.0, 0.0]}
    ]);
    doc["scenes"][0]["nodes"] = json!([0, 1]);
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts.len(), 1, "one mesh, two instances inside its part");
    assert_eq!(parts[0].mesh.verts.len(), 6);
    assert_eq!(parts[0].mesh.tris.len(), 2);
    assert_eq!(
        parts[0].mesh.verts[3],
        [10.0, 0.0, 0.0],
        "second instance baked"
    );

    // Name fallbacks: node-name hint when the mesh is unnamed; mesh{index} when neither.
    let (mut doc, bin) = tri_glb();
    doc["meshes"][0].as_object_mut().unwrap().remove("name");
    doc["nodes"][0] = json!({"name": "holder", "mesh": 0});
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts[0].name, "holder");
    let (mut doc, bin) = tri_glb();
    doc["nodes"][0] = json!({"mesh": 0});
    doc["meshes"][0].as_object_mut().unwrap().remove("name");
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts[0].name, "mesh1");

    // Two distinct meshes become two named parts in first-encounter order.
    let (mut doc, bin) = tri_glb();
    let pos2 = f32s(&[5.0, 5.0, 5.0, 6.0, 5.0, 5.0, 5.0, 6.0, 5.0]);
    doc["nodes"] = json!([{"name": "a", "mesh": 1}, {"name": "b", "mesh": 0}]);
    doc["scenes"][0]["nodes"] = json!([0, 1]);
    doc["meshes"] = json!([
        {"name": "second", "primitives": [{"attributes": {"POSITION": 2}, "indices": 1}]},
        {"name": "first", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}
    ]);
    doc["accessors"] = json!([
        {"componentType": 5126, "count": 3, "type": "VEC3", "bufferView": 0},
        {"componentType": 5123, "count": 3, "type": "SCALAR", "bufferView": 1},
        {"componentType": 5126, "count": 3, "type": "VEC3", "bufferView": 2}
    ]);
    doc["bufferViews"] = json!([
        {"buffer": 0, "byteLength": 36},
        {"buffer": 0, "byteOffset": 36, "byteLength": 6},
        {"buffer": 0, "byteOffset": 42, "byteLength": 36}
    ]);
    let mut full_bin = bin.clone();
    full_bin.extend(&pos2);
    doc["buffers"][0] = json!({"byteLength": full_bin.len()});
    let parts = read_glb(&to_glb(&doc, &full_bin)).expect("parses");
    assert_eq!(parts.len(), 2);
    assert_eq!(
        (parts[0].name.as_str(), parts[1].name.as_str()),
        ("first", "second")
    );
    assert_eq!(parts[0].mesh.verts[0], [0.0; 3]);
    assert_eq!(parts[1].mesh.verts[0], [5.0, 5.0, 5.0]);
}

#[test]
fn glb_soup_without_indices() {
    let (mut doc, mut bin) = tri_glb();
    doc["meshes"][0]["primitives"][0]
        .as_object_mut()
        .unwrap()
        .remove("indices");
    // Drop the index accessor + its view; soup vertices must be divisible by 3.
    doc["accessors"] =
        json!([{"componentType": 5126, "count": 3, "type": "VEC3", "bufferView": 0}]);
    doc["bufferViews"] = json!([{"buffer": 0, "byteLength": 36}]);
    doc["buffers"][0] = json!({"byteLength": 36});
    bin.truncate(36);
    let parts = read_glb(&to_glb(&doc, &bin)).expect("parses");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].mesh.tris, vec![[0u32, 1, 2]]);
}

#[test]
fn glb_index_component_types() {
    // All five supported index encodings (u8, i16, u16, u32, f32) read to the same triangle.
    let fixtures: [(u32, Vec<u8>, &str); 5] = [
        (5121, u8s(&[0, 1, 2]), "u8"),
        (5122, i16s(&[0i16, 1, 2]), "i16"),
        (5123, u16s(&[0, 1, 2]), "u16"),
        (5125, u32s(&[0, 1, 2]), "u32"),
        (5126, f32s(&[0.0, 1.0, 2.0]), "f32"),
    ];
    for (ct, raw, desc) in fixtures {
        let (mut doc, mut bin) = tri_glb();
        doc["accessors"][1] =
            json!({"componentType": ct, "count": 3, "type": "SCALAR", "bufferView": 1});
        doc["bufferViews"][1]["byteLength"] = json!(raw.len());
        doc["buffers"][0] = json!({"byteLength": 36 + raw.len()});
        bin.truncate(36);
        bin.extend(&raw);
        let parts = read_glb(&to_glb(&doc, &bin)).unwrap_or_else(|e| panic!("{desc} indices: {e}"));
        assert_eq!(parts[0].mesh.tris, vec![[0u32, 1, 2]], "{desc}");
    }
}

#[test]
fn glb_named_rejections() {
    let check = |bytes: &[u8], needle: &str| {
        let err = read_glb(bytes).expect_err(needle);
        assert!(err.contains(needle), "needle '{needle}' not in '{err}'");
    };

    check(b"magic1234567", "not a GLB");
    check(b"glTF", "truncated (shorter than");

    // Version and total length live in the 12-byte header.
    let (doc, bin) = tri_glb();
    let mut bytes = to_glb(&doc, &bin);
    bytes[4] = 1;
    check(&bytes, "GLB version 1");
    let mut bytes = to_glb(&doc, &bin);
    let declared = (bytes.len() as u32) + 10;
    bytes[8..12].copy_from_slice(&declared.to_le_bytes());
    check(&bytes, "truncated (declared length");

    // Chunk framing: a JSON-less file, doubled BIN, oversized chunk length.
    let mut only_bin = b"glTF".to_vec();
    only_bin.extend(&2u32.to_le_bytes());
    let bc = chunk(b"BIN\0", &bin, 0);
    only_bin.extend(&(12 + bc.len() as u32).to_le_bytes());
    only_bin.extend(bc);
    check(&only_bin, "no JSON chunk");
    let mut doubled = to_glb(&tri_glb().0, &tri_glb().1);
    doubled.extend(chunk(b"BIN\0", &[0; 4], 0));
    let len = doubled.len() as u32;
    doubled[8..12].copy_from_slice(&len.to_le_bytes());
    check(&doubled, "more than one BIN");
    let mut over = to_glb(&tri_glb().0, &tri_glb().1);
    over[12..16].copy_from_slice(&u32::MAX.to_le_bytes()); // JSON chunk length, header intact
    check(&over, "chunk length overruns");

    // Buffer-level rejections.
    let (mut doc, bin) = tri_glb();
    doc["buffers"][0] =
        json!({"byteLength": bin.len(), "uri": "data:application/octet-stream;base64,AAA="});
    check(&to_glb(&doc, &bin), "data URI");
    let (mut doc, bin) = tri_glb();
    doc["buffers"][0] = json!({"byteLength": bin.len(), "uri": "mesh.bin"});
    check(&to_glb(&doc, &bin), "external file");
    let (mut doc, bin) = tri_glb();
    doc["buffers"][0] = json!({"byteLength": bin.len() + 1000});
    check(&to_glb(&doc, &bin), "bytes but the GLB's BIN chunk");

    // Geometry-level rejections.
    let (mut doc, bin) = tri_glb();
    doc["meshes"][0]["primitives"][0]["mode"] = json!(5);
    check(&to_glb(&doc, &bin), "only TRIANGLES (4)");
    let (mut doc, bin) = tri_glb();
    doc["accessors"][0]["sparse"] =
        json!({"count": 1, "indices": {"bufferView": 0, "count": 1}, "values": {"bufferView": 0}});
    check(&to_glb(&doc, &bin), "sparse");
    let (mut doc, bin) = tri_glb();
    doc["accessors"][0]["normalized"] = json!(true);
    check(&to_glb(&doc, &bin), "quantised");
    let (mut doc, bin) = tri_glb();
    doc["nodes"][0]["skin"] = json!(0);
    check(&to_glb(&doc, &bin), "skinned");
    let (mut doc, bin) = tri_glb();
    doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"] = json!(5);
    check(&to_glb(&doc, &bin), "accessor 5 does not exist");
    let (mut doc, bin) = tri_glb();
    doc["meshes"][0]["primitives"][0]["attributes"] = json!({});
    check(&to_glb(&doc, &bin), "no POSITION attribute");

    // Index count not divisible by 3.
    let (mut doc, mut bin) = tri_glb();
    let raw = u16s(&[0, 1, 2, 3]);
    doc["accessors"][1]["count"] = json!(4);
    doc["bufferViews"][1]["byteLength"] = json!(raw.len());
    doc["buffers"][0] = json!({"byteLength": 36 + raw.len()});
    bin.truncate(36);
    bin.extend(&raw);
    check(&to_glb(&doc, &bin), "index count 4 is not divisible by 3");

    // Accessor overrun: fewer bytes in the view than the accessor needs.
    let (mut doc, bin) = tri_glb();
    doc["bufferViews"][1]["byteLength"] = json!(3);
    check(&to_glb(&doc, &bin), "overrun");

    // extensionsRequired and projective matrices.
    let (mut doc, bin) = tri_glb();
    doc["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
    check(&to_glb(&doc, &bin), "KHR_draco_mesh_compression");
    let (mut doc, bin) = tri_glb();
    doc["nodes"][0]["matrix"] =
        json!([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    check(&to_glb(&doc, &bin), "projective");

    // Soup whose vertex count is not divisible by 3.
    let (mut doc, bin) = tri_glb();
    doc["meshes"][0]["primitives"][0]
        .as_object_mut()
        .unwrap()
        .remove("indices");
    doc["accessors"] =
        json!([{"componentType": 5126, "count": 4, "type": "VEC3", "bufferView": 0}]);
    doc["bufferViews"] = json!([{"buffer": 0, "byteLength": 48}]);
    doc["buffers"][0] = json!({"byteLength": 48});
    let mut bin4 = bin;
    bin4.truncate(36);
    bin4.extend(f32s(&[9.0, 9.0, 9.0]));
    check(&to_glb(&doc, &bin4), "vertex count 4 is not divisible by 3");
}

#[test]
fn glb_scene_selection_and_roots() {
    // A scene index that does not exist.
    let (mut doc, bin) = tri_glb();
    doc["scene"] = json!(3);
    let err = read_glb(&to_glb(&doc, &bin)).expect_err("bad scene index");
    assert!(err.contains("scene index 3"), "{err}");
    // A scene with an empty node list falls back to the non-child roots.
    let (mut doc, bin) = tri_glb();
    doc["scenes"][0] = json!({"nodes": []});
    let parts = read_glb(&to_glb(&doc, &bin)).expect("fallback roots");
    assert_eq!(parts.len(), 1);
    // A node-tree cycle is an error, not a hang.
    let (mut doc, bin) = tri_glb();
    doc["nodes"] = json!([{"mesh": 0, "children": [1]}, {"children": [0]}]);
    let err = read_glb(&to_glb(&doc, &bin)).expect_err("cycle");
    assert!(err.contains("cycle"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// OBJ

/// A small OBJ mixing named parts, a quad, negative indices and ignorable lines. Global
/// vertices: 0=(0,0,0), 1=(1,0,0), 2=(0,1,0), 3=(1,1,0).
const FOUR_PART_OBJ: &str = "\
# exported by hand
mtllib none.mtl
v 0 0 0
v 1 0 0
v 0 1 0
v 1 1 0
vt 0 0
vn 0 0 1
f 1 2 3
o A
f 1 2 3
g B
f 1 2 3 4
s 1
o C
f -3 -2 -1
";

#[test]
fn obj_parses_and_splits() {
    let parts = read_obj(FOUR_PART_OBJ).expect("parses");
    assert_eq!(
        parts
            .iter()
            .map(|p| (p.name.as_str(), p.mesh.tris.len()))
            .collect::<Vec<_>>(),
        vec![("", 1), ("A", 1), ("B", 2), ("C", 1)]
    );
    // The pre-`o` face landed in the unnamed part with global indices 1..3.
    assert_eq!(parts[0].name, "");
    assert_eq!(
        parts[0].mesh.verts,
        vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(parts[0].mesh.tris, vec![[0u32, 1, 2]]);
    // B is the quad f 1 2 3 4 fanned into two triangles.
    assert_eq!(parts[2].mesh.tris, vec![[0u32, 1, 2], [0, 2, 3]]);
    assert_eq!(parts[2].mesh.verts[3], [1.0, 1.0, 0.0]);
    // C uses negative (count-from-end) references: -3, -2, -1 over 4 global vertices.
    assert_eq!(
        parts[3].mesh.verts,
        vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]
    );
}

#[test]
fn obj_face_token_formats() {
    // v//vn, v/vt and plain v references all parse; the vertex component drives indices.
    let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nvt 0 0\nf 1//1 2//1 3/1\n";
    let parts = read_obj(text).expect("parses");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].mesh.tris, vec![[0u32, 1, 2]]);
}

#[test]
fn obj_named_rejections() {
    let check = |text: &str, needle: &str| {
        let err = read_obj(text).expect_err(needle);
        assert!(err.contains(needle), "needle '{needle}' not in '{err}'");
    };
    check("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2\n", "3 are needed");
    check("v 0 0 0\nf 0 0 0\n", "start at 1");
    check("v 0 0 0\nf 1 2 99\n", "only 1 exist");
    // A face before any vertices: the reference is out of range.
    check("f 1 2 3\n", "only 0 exist");
    check("v 0 0 0\nf a b c\n", "not a vertex index");
    check("v 0 0 0 2\nf 1 1 1\n", "rational");
    check("v 0 0\nf 1 1 1\n", "needs x y z");
    check("v 0 0 0\nv 1 0 0\n", "no faces");
    check("o empty\n", "no faces");
}

#[test]
fn obj_unnamed_single_part() {
    let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let parts = read_obj(text).expect("parses");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "");
    assert_eq!(parts[0].mesh.tris, vec![[0u32, 1, 2]]);
}

/// Smoke test on the milestone's real meshy export (step 10 re-checks it properly through
/// the CLI). Skips with a note when the file is not on this machine.
///
/// Measured facts asserted here: the reader welds equal corners itself, so an exact-corner
/// meshy STL arrives with shared vertices already; the mesh is watertight (0 open edges);
/// at a 30° segmentation threshold the organic surface is one smooth patch, which is why
/// the milestone's "extend the bottom" flow needs the geometric-plane region, not a patch
/// click. Outputs a patch-count sweep for the region design.
#[test]
fn meshy_stl_smoke() {
    let path = std::path::Path::new(
        "/home/ian/Downloads/Meshy_AI_Perforated Grid with Black Fragment_1791318204_generate.stl",
    );
    if !path.exists() {
        eprintln!("meshy_stl_smoke: fixture not present, skipping");
        return;
    }
    let bytes = std::fs::read(path).expect("read");
    let m = read_stl_any(&bytes).expect("parses");
    assert_eq!(m.tris.len(), 87_286, "measured ground truth");

    let adj = fieldsmith_mesh::weld::edge_adjacency(&m);
    let open = adj
        .nbr
        .iter()
        .flat_map(|t| t.iter())
        .filter(|&&e| e == u32::MAX)
        .count();
    let t0 = std::time::Instant::now();
    let id = fieldsmith_mesh::weld::segment(&m, &adj, 30.0_f64.to_radians());
    let t1 = std::time::Instant::now();
    eprintln!(
        "segment 30° {:?} → {} patches; open edges = {}",
        t1 - t0,
        id.iter().copied().max().unwrap(),
        open
    );
    assert_eq!(open, 0, "watertight per ground truth");
    // Threshold sweep: how the patch count responds to a stricter cut, and the size of the
    // patch holding the lowest-z triangle (the region the milestone wants to extend).
    let zmin = m.verts.iter().fold(f64::INFINITY, |a, p| a.min(p[2]));
    let seed = id
        .iter()
        .enumerate()
        .find(|(t, _)| m.verts[m.tris[*t][0] as usize][2] == zmin)
        .map(|(_, &g)| g)
        .unwrap();
    for deg in [30, 20, 15, 10] {
        let id = fieldsmith_mesh::weld::segment(&m, &adj, (deg as f64).to_radians());
        let bottom = id.iter().filter(|&&g| g == seed).count();
        let total = id.iter().copied().max().unwrap();
        eprintln!("{deg}° → {total} patches, seed patch holds {bottom} tris");
    }
    // Whatever the threshold, the segmentation is total and deterministic.
    let id30 = fieldsmith_mesh::weld::segment(&m, &adj, 30.0_f64.to_radians());
    assert_eq!(id30, id);
}

// ---------------------------------------------------------------------------------------------
// 3MF reader (goal M7, step 6)

use fieldsmith_mesh::export::surface;
use fieldsmith_mesh::threemf::read_3mf;

/// A cube as an SDF (half 10, centred at the origin) for the round-trip fixture.
fn cube_sdf(p: [f64; 3]) -> f64 {
    let t = [p[0].abs() - 10.0, p[1].abs() - 10.0, p[2].abs() - 10.0];
    let o = [t[0].max(0.0), t[1].max(0.0), t[2].max(0.0)];
    (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt() + t[0].max(t[1]).max(t[2]).min(0.0)
}

#[test]
fn threemf_round_trip_through_our_own_writer() {
    // Mesh a cube, write it as 3MF with our own writer, read it back: the mesh must be
    // bit-identical (our writer emits 6-decimal coordinates in the positive octant, so the
    // round trip is exact to the printed precision).
    let m = surface(cube_sdf, [-10.0; 3], [10.0; 3], 1.0);
    let bytes = m.write_3mf("bracket", "petg");
    let parts = read_3mf(&bytes).expect("our own 3MF reads back");
    assert_eq!(parts.len(), 1, "one object in our writer's file");
    let back = &parts[0];
    assert_eq!(back.name, "bracket", "the object's name attribute survives");
    assert_eq!(back.mesh.tris.len(), m.tris.len(), "triangle count");
    assert_eq!(back.mesh.verts.len(), m.verts.len(), "vertex count");
    // Our writer shifts vertices into the positive octant by the mesh's min corner, so
    // the read-back vertices are offset. The SHAPE (differences) is identical.
    let shift = [
        back.mesh.verts[0][0] - m.verts[0][0],
        back.mesh.verts[0][1] - m.verts[0][1],
        back.mesh.verts[0][2] - m.verts[0][2],
    ];
    for (a, b) in back.mesh.verts.iter().zip(&m.verts) {
        for k in 0..3 {
            assert!(
                (a[k] - shift[k] - b[k]).abs() < 1e-5,
                "vertex {a:?} vs {b:?} (shift {shift:?})"
            );
        }
    }
    assert_eq!(back.mesh.tris, m.tris, "triangle indices identical");
    // The volume survives the round trip.
    let v0 = m.volume();
    let v1 = back.mesh.volume();
    assert!(
        (v1 - v0).abs() / v0 < 1e-4,
        "volume {v1} vs {v0} after the round trip"
    );
}

#[test]
fn threemf_units_scale_to_millimetres() {
    // The same cube written in metres must read back 1000× larger.
    let m = surface(cube_sdf, [-10.0; 3], [10.0; 3], 1.0);
    let bytes = m.write_3mf("m", "petg");
    // Rewrite the model entry with unit="meter" by patching the model text inside the
    // zip: simplest is to rebuild the model text from the read-back parts and re-zip with
    // our own writer's layout. Instead, parse the model text, swap the unit, and re-zip.
    let entries = fieldsmith_mesh::zip::read_zip(&bytes).unwrap();
    let model = String::from_utf8(
        entries
            .iter()
            .find(|e| e.name == "3D/3dmodel.model")
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    let model_m = model.replace("unit=\"millimeter\"", "unit=\"meter\"");
    assert_ne!(model, model_m, "the unit attribute is present");
    // Re-zip with the same layout (stored entries are fine for the reader).
    let mut out = Vec::new();
    let mut records = Vec::new();
    for e in &entries {
        let data = if e.name == "3D/3dmodel.model" {
            model_m.as_bytes()
        } else {
            &e.data
        };
        fieldsmith_mesh::zip::stream_entry(&mut out, &e.name, data, &mut records);
    }
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let parts = read_3mf(&out).expect("metre-unit 3MF reads");
    let v = parts[0].mesh.volume();
    let v0 = m.volume();
    // metre → mm is ×1000 per axis, so the volume is ×1000³.
    assert!(
        (v - v0 * 1e9).abs() / (v0 * 1e9) < 1e-4,
        "metre-unit volume {v} vs {v0}×10⁹ = {}",
        v0 * 1e9
    );
}

#[test]
fn threemf_named_rejections() {
    // Not a zip.
    let e = read_3mf(b"not a zip file at all").unwrap_err();
    assert!(e.contains("zip") || e.contains("archive"), "{e}");
    // A zip with no model entry.
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(&mut out, "readme.txt", b"hello", &mut records);
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let e = read_3mf(&out).unwrap_err();
    assert!(e.contains("3dmodel.model"), "{e}");
    // A model with no mesh.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources><object id=\"1\" type=\"model\"></object></resources></model>";
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let e = read_3mf(&out).unwrap_err();
    assert!(e.contains("no object with a mesh"), "{e}");
    // A mesh with a triangle index out of range.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources><object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/></vertices><triangles><triangle v1=\"0\" v2=\"1\" v3=\"5\"/></triangles></mesh></object></resources></model>";
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let e = read_3mf(&out).unwrap_err();
    assert!(e.contains("out of range"), "{e}");
    // An unsupported unit.
    let model = "<?xml version=\"1.0\"?><model unit=\"micron\"><resources><object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices><triangles></triangles></mesh></object></resources></model>";
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let e = read_3mf(&out).unwrap_err();
    assert!(e.contains("unit"), "{e}");
    // A vertex missing its z attribute.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources><object id=\"1\"><mesh><vertices><vertex x=\"0\" y=\"0\"/></vertices><triangles></triangles></mesh></object></resources></model>";
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let e = read_3mf(&out).unwrap_err();
    assert!(e.contains("z"), "{e}");
}

#[test]
fn threemf_reads_a_foreign_single_object_file() {
    // A hand-written minimal 3MF (one tetrahedron, centimetre units, an unnamed object):
    // the reader must scale cm → mm and name the part "mesh".
    let model = "<?xml version=\"1.0\"?>
<model unit=\"centimeter\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">
 <resources>
  <object id=\"1\" type=\"model\">
   <mesh>
    <vertices>
     <vertex x=\"0\" y=\"0\" z=\"0\"/>
     <vertex x=\"1\" y=\"0\" z=\"0\"/>
     <vertex x=\"0\" y=\"1\" z=\"0\"/>
     <vertex x=\"0\" y=\"0\" z=\"1\"/>
    </vertices>
    <triangles>
     <triangle v1=\"0\" v2=\"2\" v3=\"1\"/>
     <triangle v1=\"0\" v2=\"1\" v3=\"3\"/>
     <triangle v1=\"0\" v2=\"3\" v3=\"2\"/>
     <triangle v1=\"1\" v2=\"2\" v3=\"3\"/>
    </triangles>
   </mesh>
  </object>
 </resources>
 <build><item objectid=\"1\"/></build>
</model>
";
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(&mut out, "[Content_Types].xml", b"<Types/>", &mut records);
    fieldsmith_mesh::zip::stream_entry(&mut out, "_rels/.rels", b"<Relationships/>", &mut records);
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    let parts = read_3mf(&out).expect("foreign 3MF reads");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "mesh", "an unnamed object is named 'mesh'");
    let m = &parts[0].mesh;
    assert_eq!(m.verts.len(), 4);
    assert_eq!(m.tris.len(), 4);
    // cm → mm: the (1, 0, 0) vertex reads (10, 0, 0).
    assert_eq!(
        m.verts[1],
        [10.0, 0.0, 0.0],
        "centimetre scaled to millimetres"
    );
    // A unit tetrahedron (edge 10 mm) has volume 10³/6 = 166.67 mm³.
    let v = m.volume().abs();
    assert!(
        (v - 1000.0 / 6.0).abs() / (1000.0 / 6.0) < 1e-9,
        "tetrahedron volume {v}"
    );
}

// ---------------------------------------------------------------------------------------------
// 3MF materials and multi-volume packages (goal M9)

use fieldsmith_mesh::export::{write_3mf_volumes, Volume3mf};

/// Zip a model-text-only 3MF in our writer's layout; the reader only needs the model entry.
fn zip_model(model: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut records = Vec::new();
    fieldsmith_mesh::zip::stream_entry(&mut out, "[Content_Types].xml", b"<Types/>", &mut records);
    fieldsmith_mesh::zip::stream_entry(&mut out, "_rels/.rels", b"<Relationships/>", &mut records);
    fieldsmith_mesh::zip::stream_entry(
        &mut out,
        "3D/3dmodel.model",
        model.as_bytes(),
        &mut records,
    );
    fieldsmith_mesh::zip::write_central(&mut out, &records);
    out
}

/// The model text inside a packaged 3MF.
fn model_text(bytes: &[u8]) -> String {
    let entries = fieldsmith_mesh::zip::read_zip(bytes).unwrap();
    String::from_utf8(
        entries
            .iter()
            .find(|e| e.name == "3D/3dmodel.model")
            .unwrap()
            .data
            .clone(),
    )
    .unwrap()
}

/// A complete object holding a tetrahedron (legs 10 mm, watertight) with the given
/// attributes, plus any `<resources>` content that follows it. Extras after the object is
/// the declaration order other tools are free to use; our own writer declares the group
/// first (covered by the writer tests' round trips).
fn tet_model(object_attrs: &str, extras: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<object id=\"2\"{object_attrs}><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"10\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"10\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"10\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
<triangle v1=\"0\" v2=\"3\" v3=\"2\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\"/></triangles>\
</mesh></object>{extras}</resources><build><item objectid=\"2\"/></build></model>"
    )
}

#[test]
fn threemf_writer_several_volumes_with_materials() {
    // Two volumes, two materials: one <object> each with its name and its group's
    // pid/pindex, one shared <basematerials>, one build item per object. Each volume reads
    // back with its own name, material and shape, translated to its own positive octant.
    let a = surface(cube_sdf, [-10.0; 3], [10.0; 3], 1.0);
    let b = surface(cube_sdf, [-5.0; 3], [5.0; 3], 0.5);
    let bytes = write_3mf_volumes(&[
        Volume3mf {
            name: "body",
            material: "PETG",
            color: "",
            mesh: &a,
        },
        Volume3mf {
            name: "clip",
            material: "TPU95A",
            color: "",
            mesh: &b,
        },
    ]);
    let parts = read_3mf(&bytes).expect("the two-volume 3MF reads back");
    assert_eq!(parts.len(), 2, "one part per volume");
    assert_eq!(parts[0].name, "body");
    assert_eq!(parts[0].material.as_deref(), Some("PETG"));
    assert_eq!(parts[1].name, "clip");
    assert_eq!(parts[1].material.as_deref(), Some("TPU95A"));
    for (part, m0) in parts.iter().zip([&a, &b]) {
        for v in &part.mesh.verts {
            assert!(
                v.iter().all(|c| *c >= -1e-6),
                "vertex {v:?} fell below zero (the per-volume octant shift)"
            );
        }
        assert_eq!(part.mesh.tris.len(), m0.tris.len(), "triangle count");
        assert!(
            (part.mesh.volume() - m0.volume()).abs() / m0.volume() < 1e-4,
            "volume survives the round trip"
        );
    }
    // The package: one shared basematerials group in encounter order, and one object and
    // build item per volume with the group's index as pindex.
    let model = model_text(&bytes);
    assert!(
        model.contains(
            "<basematerials id=\"1\"><base name=\"PETG\" displaycolor=\"#DFDEDA\"/><base name=\"TPU95A\" displaycolor=\"#DFDEDA\"/></basematerials>"
        ),
        "one shared group in encounter order; colourless volumes take the app default: {model}"
    );
    assert!(
        model.contains("<object id=\"2\" type=\"model\" name=\"body\" pid=\"1\" pindex=\"0\">"),
        "{model}"
    );
    assert!(
        model.contains("<object id=\"3\" type=\"model\" name=\"clip\" pid=\"1\" pindex=\"1\">"),
        "{model}"
    );
    assert!(
        model.contains("<item objectid=\"2\"/><item objectid=\"3\"/>"),
        "{model}"
    );

    // The single-volume wrapper writes the same model text the volumes API writes for one
    // volume.
    let wrapper = a.write_3mf("body", "PETG");
    let direct = write_3mf_volumes(&[Volume3mf {
        name: "body",
        material: "PETG",
        color: "",
        mesh: &a,
    }]);
    assert_eq!(
        model_text(&wrapper),
        model_text(&direct),
        "the wrapper and the volumes API agree"
    );
}

#[test]
fn threemf_writer_dedups_materials_and_skips_empty_labels() {
    // Three volumes, two labels: the group holds one base per unique label and the
    // unlabelled volume names no group.
    let a = surface(cube_sdf, [-10.0; 3], [10.0; 3], 1.0);
    let bytes = write_3mf_volumes(&[
        Volume3mf {
            name: "bare",
            material: "",
            color: "",
            mesh: &a,
        },
        Volume3mf {
            name: "left",
            material: "ABS",
            color: "",
            mesh: &a,
        },
        Volume3mf {
            name: "right",
            material: "ABS",
            color: "",
            mesh: &a,
        },
    ]);
    let parts = read_3mf(&bytes).expect("reads back");
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].material, None, "an empty label names no group");
    assert_eq!(parts[1].material.as_deref(), Some("ABS"));
    assert_eq!(parts[2].material.as_deref(), Some("ABS"));
    let model = model_text(&bytes);
    assert!(
        model.contains(
            "<basematerials id=\"1\"><base name=\"ABS\" displaycolor=\"#DFDEDA\"/></basematerials>"
        ),
        "one base per unique label: {model}"
    );
    assert!(
        model.contains("<object id=\"2\" type=\"model\" name=\"bare\"><mesh>"),
        "no pid on the unlabelled volume: {model}"
    );
    assert!(
        model.contains("<object id=\"3\" type=\"model\" name=\"left\" pid=\"1\" pindex=\"0\">"),
        "{model}"
    );
    assert!(
        model.contains("<object id=\"4\" type=\"model\" name=\"right\" pid=\"1\" pindex=\"0\">"),
        "{model}"
    );
}

#[test]
fn threemf_reader_resolves_materials_declared_after_the_object() {
    // The collection pass runs before the object pass, so a basematerials group declared
    // after the object that cites it still resolves.
    let model = tet_model(
        " name=\"tet\" pid=\"3\" pindex=\"1\"",
        "<basematerials id=\"3\"><base name=\"PLA\"/><base name=\"PETG\"/></basematerials>",
    );
    let parts = read_3mf(&zip_model(&model)).expect("reads");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].name, "tet");
    assert_eq!(
        parts[0].material.as_deref(),
        Some("PETG"),
        "pindex 1 of the group"
    );
}

#[test]
fn threemf_named_rejections_materials() {
    // A duplicate basematerials id.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<basematerials id=\"1\"><base name=\"A\"/></basematerials>\
<basematerials id=\"1\"><base name=\"B\"/></basematerials>\
<object id=\"2\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices>\
<triangles></triangles></mesh></object></resources></model>";
    let e = read_3mf(&zip_model(model)).unwrap_err();
    assert!(e.contains("used more than once"), "{e}");

    // An object naming a group the model does not define.
    let e = read_3mf(&zip_model(&tet_model(" pid=\"3\" pindex=\"0\"", ""))).unwrap_err();
    assert!(e.contains("no basematerials group with id 3"), "{e}");

    // A pindex past the group's bases.
    let model = tet_model(
        " pid=\"3\" pindex=\"1\"",
        "<basematerials id=\"3\"><base name=\"A\"/></basematerials>",
    );
    let e = read_3mf(&zip_model(&model)).unwrap_err();
    assert!(e.contains("has 1 base materials"), "{e}");

    // A pindex without a pid.
    let e = read_3mf(&zip_model(&tet_model(" pindex=\"0\"", ""))).unwrap_err();
    assert!(e.contains("but no pid attribute"), "{e}");

    // Non-integer pid and pindex.
    let e = read_3mf(&zip_model(&tet_model(" pid=\"a\" pindex=\"0\"", ""))).unwrap_err();
    assert!(e.contains("pid=\"a\" is not an integer"), "{e}");
    let model = tet_model(" pid=\"3\" pindex=\"x\"", "");
    let e = read_3mf(&zip_model(&model)).unwrap_err();
    assert!(e.contains("pindex is not an integer"), "{e}");

    // Per-triangle material: refused, not silently dropped.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<object id=\"2\"><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"10\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"10\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"10\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
<triangle v1=\"0\" v2=\"3\" v3=\"2\"/><triangle pid=\"5\" v1=\"1\" v2=\"2\" v3=\"3\"/>\
</triangles></mesh></object></resources><build><item objectid=\"2\"/></build></model>";
    let e = read_3mf(&zip_model(model)).unwrap_err();
    assert!(e.contains("per-triangle material"), "{e}");

    // Input ending inside a <basematerials>.
    let model = "<model unit=\"millimeter\"><resources><basematerials id=\"1\"><base name=\"A\"/>";
    let e = read_3mf(&zip_model(model)).unwrap_err();
    assert!(
        e.contains("not closed") || e.contains("unexpected closing"),
        "{e}"
    );

    // A <base> with no name attribute.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<basematerials id=\"1\"><base/></basematerials>\
<object id=\"2\"><mesh><vertices><vertex x=\"0\" y=\"0\" z=\"0\"/></vertices>\
<triangles></triangles></mesh></object></resources></model>";
    let e = read_3mf(&zip_model(model)).unwrap_err();
    assert!(e.contains("is missing its name attribute"), "{e}");
}

/// A model from an arbitrary string (items, component containers, everything the
/// `tet_model` fixture can't express), zipped like a real 3MF.
fn raw_model(model: &str) -> Vec<u8> {
    zip_model(model)
}

#[test]
fn threemf_reader_expands_build_items() {
    // One cylinder object instanced twice with build-item transforms — the 3MF
    // Consortium's multiple_cylinders.3mf pattern. Each item becomes its own part,
    // the transform baked into the vertices (translation scaled by the model unit), and
    // a mirrored item keeps its volume sign, winding reversed.
    let model = "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<object id=\"2\" name=\"cyl\"><mesh><vertices>\
<vertex x=\"1\" y=\"0\" z=\"0\"/><vertex x=\"0\" y=\"1\" z=\"0\"/>\
<vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object>\
</resources>\
<build><item objectid=\"2\"/><item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 4 0 0\"/>\
<item objectid=\"2\" transform=\"0 -1 0 1 0 0 0 0 1 0 0 0\"/>\
<item objectid=\"2\" transform=\"-1 0 0 0 1 0 0 0 1 0 0 0\"/></build></model>";
    let parts = read_3mf(&zip_model(model)).expect("instanced items expand");
    assert_eq!(parts.len(), 4, "one part per build item");
    assert_eq!(parts[0].name, "cyl");
    for p in &parts {
        assert_eq!(
            p.name, "cyl",
            "the part keeps the object's name (import dedupes)"
        );
        assert_eq!(p.mesh.tris.len(), 1);
    }
    assert_eq!(parts[0].mesh.verts[0], [1.0, 0.0, 0.0]);
    assert_eq!(
        parts[1].mesh.verts[0],
        [5.0, 0.0, 0.0],
        "translation baked in"
    );
    assert_eq!(
        parts[2].mesh.verts[0],
        [0.0, 1.0, 0.0],
        "90° rotation about +z baked in"
    );
    assert_eq!(parts[3].mesh.verts[0], [-1.0, 0.0, 0.0], "mirror baked in");
    assert!(
        parts[3].mesh.tris[0] == [0, 2, 1],
        "negative-determinant transform reverses the winding: {:?}",
        parts[3].mesh.tris[0]
    );
    assert!(parts[2].mesh.tris[0] == [0, 1, 2]);
}

#[test]
fn threemf_reader_scales_item_translations_by_the_model_unit() {
    // The item matrix is applied in model units: its translation scales with the unit
    // like everything else, so the part lands in millimetres.
    let model = "<?xml version=\"1.0\"?><model unit=\"centimeter\"><resources>\
<object id=\"2\" name=\"c\"><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"1\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"1\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
<triangle v1=\"0\" v2=\"3\" v3=\"2\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\"/></triangles>\
</mesh></object></resources>\
<build><item objectid=\"2\" transform=\"1 0 0 0 1 0 0 0 1 2 0 0\"/></build></model>";
    let parts = read_3mf(&raw_model(model)).expect("reads");
    assert_eq!(
        parts[0].mesh.verts[0],
        [20.0, 0.0, 0.0],
        "2 cm → 20 mm move"
    );
}

#[test]
fn threemf_reader_refuses_damaged_build_items() {
    // A one-triangle mesh, as a reusable object body.
    let body = "<object id=\"2\"><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"1\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"1\" z=\"0\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"1\" v3=\"2\"/></triangles></mesh></object>";

    // An object id used twice.
    let e = read_3mf(&raw_model(&format!(
        "<model unit=\"millimeter\"><resources>{body}\
<object id=\"2\"/></resources><build><item objectid=\"2\"/></build></model>"
    )))
    .unwrap_err();
    assert!(e.contains("2' is used more than once"), "{e}");

    // An item referencing an object the model never defined.
    let e = read_3mf(&raw_model(&format!(
        "<model unit=\"millimeter\"><resources>{body}\
</resources><build><item objectid=\"7\"/></build></model>"
    )))
    .unwrap_err();
    assert!(e.contains("'7' which the model does not define"), "{e}");

    // An item without an objectid.
    let e = read_3mf(&raw_model(&format!(
        "<model unit=\"millimeter\"><resources>{body}\
</resources><build><item/></build></model>"
    )))
    .unwrap_err();
    assert!(e.contains("has no objectid attribute"), "{e}");

    // An item referencing a component container (an object with no mesh).
    let e = read_3mf(&raw_model(&format!(
        "<model unit=\"millimeter\"><resources>{body}\
<object id=\"3\" name=\"stack\"><components componentpath=\"2\"/></object></resources>\
<build><item objectid=\"3\"/></build></model>"
    )))
    .unwrap_err();
    assert!(
        e.contains("has no mesh (component objects are not supported"),
        "{e}"
    );

    // A transform with the wrong number count and a non-number.
    let with_transform = |t: &str| {
        let m = format!(
            "<model unit=\"millimeter\"><resources>{body}\
</resources><build><item objectid=\"2\"{t}/></build></model>"
        );
        read_3mf(&raw_model(&m)).unwrap_err()
    };
    let e = with_transform(" transform=\"1 2 3 4 5 6 7 8 9 10 11\"");
    assert!(e.contains("transform has 11 numbers, expected 12"), "{e}");
    let e = with_transform(" transform=\"1 2 3 4 5 6 7 8 9 10 11 x\"");
    assert!(e.contains("transform value 'x' is not a number"), "{e}");

    // An unclosed <build>.
    let model = format!(
        "<model unit=\"millimeter\"><resources>{body}\
</resources><build><item objectid=\"2\"/>"
    );
    let e = read_3mf(&raw_model(&model)).unwrap_err();
    assert!(e.contains("<build> is not closed"), "{e}");
}
