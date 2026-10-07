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
