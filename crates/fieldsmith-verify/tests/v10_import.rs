//! M6 step 10 — the import chain as the user's pipeline sees it: GLB / STL / OBJ bytes go
//! through `import_mesh` (read → weld → segment → voxelize → hydrate), then the document
//! node compiles and evaluates like every other shape, patch selectors and the geometric
//! face rule partition the analysis surface, and a generated large model voxelises within
//! budget.
//!
//! The readers' own fidelity cases live in `fieldsmith-mesh/tests/readers.rs`, the
//! voxeliser's in `fieldsmith-core/tests/voxel.rs`, the sidecar round trip in
//! `v10_fsmz.rs`; this file bolts them together and names the numbers.

use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::{region_facets, Facet};
use fieldsmith_doc::{Document, Face, NodeKind, Region, Selector};
use fieldsmith_field::expr;
use fieldsmith_field::volume::volume_auto;
use fieldsmith_mesh::export::{surface, TriMesh};
use fieldsmith_mesh::{stuff, V3};
use fieldsmith_solve::p2::face_nodes;
use fieldsmith_solve::P2Mesh;

fn opts<'a>(
    units: Option<&'a str>,
    up: Option<&'a str>,
    voxel: Option<f64>,
    src: &'a str,
) -> ImportOpts<'a> {
    ImportOpts {
        units,
        up,
        voxel,
        src,
        taken: &[],
        step_chord: None,
    }
}

/// A document holding just the imported nodes plus the given regions.
fn part_doc(nodes: &[fieldsmith_doc::Node], regions: &[Region], name: &str) -> Document {
    let mut doc = Document {
        name: name.into(),
        ..Default::default()
    };
    doc.root.extend(nodes.iter().cloned());
    doc.regions.extend(regions.iter().cloned());
    doc
}

/// Volume, error and grid spacing through the `fieldsmith volume` verb's estimator.
fn doc_volume_all(doc: &Document) -> (f64, f64, f64) {
    let v = volume_auto(&expr::compile_body(doc));
    (v.volume, v.error, v.spacing)
}

/// Signed distance of the compiled document.
fn sample(doc: &Document, p: V3) -> f64 {
    expr::eval(&expr::compile_body(doc), p).v
}

/// The tet surface of the compiled document as P2 boundary facets (the `fieldsmith-core`
/// solve-setup construction).
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

// ---------------------------------------------------------------------------------------------
// GLB fixture: an axis-aligned cube as one Y-up mesh in metres, 8 verts / 12 triangles.
// The JSON is a literal, so the fixture needs no JSON dependency in this crate.

/// A chunk = 8-byte header ([len][type]) + payload padded to a 4-byte boundary (JSON pads
/// with spaces, BIN with zeros).
fn chunk(ty: &[u8; 4], data: &[u8], pad: u8) -> Vec<u8> {
    let n = data.len().next_multiple_of(4);
    let mut out = Vec::with_capacity(8 + n);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    out.resize(8 + n, pad);
    out
}

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

#[rustfmt::skip]
const CUBE_JSON: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"mesh":0}],
"meshes":[{"name":"cube","primitives":[{"attributes":{"POSITION":0},"indices":1}]}],
"accessors":[
{"componentType":5126,"count":8,"type":"VEC3","bufferView":0},
{"componentType":5123,"count":36,"type":"SCALAR","bufferView":1}],
"bufferViews":[{"buffer":0,"byteLength":96},{"buffer":0,"byteOffset":96,"byteLength":72}],
"buffers":[{"byteLength":168}]}"#;

/// The cube in metres (Y-up per the GLB convention): the six outward quads.
fn cube_glb() -> Vec<u8> {
    let s = 0.01;
    #[rustfmt::skip]
    let pos = f32s(&[
        0.0, 0.0, 0.0, s, 0.0, 0.0, s, s, 0.0, 0.0, s, 0.0,
        0.0, 0.0, s,   s, 0.0, s,   s, s, s,    0.0, s, s,
    ]);
    #[rustfmt::skip]
    let idx = u16s(&[
        0, 2, 1, 0, 3, 2, // z− (outward −z in GLB space)
        4, 5, 6, 4, 6, 7, // z+
        0, 4, 7, 0, 7, 3, // x−
        1, 6, 5, 1, 2, 6, // x+
        0, 1, 5, 0, 5, 4, // y−
        3, 6, 2, 3, 7, 6, // y+
    ]);
    build_glb(CUBE_JSON, &[pos, idx].concat())
}

// ---------------------------------------------------------------------------------------------
// STL fixtures

/// Flip any triangle whose normal disagrees with (centroid − centre) — sound for the two
/// fixtures here: a sphere is star-shaped, and the block-with-hole's every boundary
/// triangle faces away from its centre.
fn orient_outward(m: &mut TriMesh, centre: V3) {
    for t in &mut m.tris {
        let (a, b, c) = (
            m.verts[t[0] as usize],
            m.verts[t[1] as usize],
            m.verts[t[2] as usize],
        );
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let mid = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let d = [mid[0] - centre[0], mid[1] - centre[1], mid[2] - centre[2]];
        if n[0] * d[0] + n[1] * d[1] + n[2] * d[2] < 0.0 {
            t.reverse();
        }
    }
}

/// UV sphere: `stacks` meridian rings × `slices` around (same construction the voxeliser's
/// own sphere case uses).
fn uv_sphere(r: f64, c: V3, stacks: usize, slices: usize) -> TriMesh {
    let mut m = TriMesh {
        verts: vec![[c[0], c[1], c[2] + r]],
        tris: Vec::new(),
    };
    for j in 1..stacks {
        let phi = std::f64::consts::PI * j as f64 / stacks as f64;
        for k in 0..slices {
            let th = 2.0 * std::f64::consts::PI * k as f64 / slices as f64;
            m.verts.push([
                c[0] + r * phi.sin() * th.cos(),
                c[1] + r * phi.sin() * th.sin(),
                c[2] + r * phi.cos(),
            ]);
        }
    }
    let south = m.verts.len();
    m.verts.push([c[0], c[1], c[2] - r]);
    let rv = |j: usize, k: usize| (1 + (j - 1) * slices + (k % slices)) as u32; // ring j vertex
    for k in 0..slices {
        m.tris.push([0, rv(1, k), rv(1, k + 1)]);
    }
    for j in 1..stacks - 1 {
        for k in 0..slices {
            let (a, b, cc, d) = (rv(j, k), rv(j, k + 1), rv(j + 1, k + 1), rv(j + 1, k));
            m.tris.push([a, b, cc]);
            m.tris.push([a, cc, d]);
        }
    }
    for k in 0..slices {
        m.tris
            .push([south as u32, rv(stacks - 1, k + 1), rv(stacks - 1, k)]);
    }
    orient_outward(&mut m, c);
    m
}

/// Binary STL of a generated mesh (each shared corner casts identically per index, so the
/// reader's bit-exact weld restores every vertex once).
fn binary_stl(m: &TriMesh) -> Vec<u8> {
    let mut b = vec![0u8; 80]; // header
    b.extend((m.tris.len() as u32).to_le_bytes());
    for t in &m.tris {
        for _ in 0..3 {
            b.extend(0.0f32.to_le_bytes()); // no attribute normal — the winding is the truth
        }
        for &i in t {
            for c in m.verts[i as usize] {
                b.extend((c as f32).to_le_bytes());
            }
        }
        b.extend(0u16.to_le_bytes()); // attribute byte count
    }
    b
}

// ---------------------------------------------------------------------------------------------
// OBJ fixture

/// OBJ text of an 8×8×4 block with a 2×2 vertical hole through x 3..5, y 3..5, hand-wound
/// outward (single-point orientation is unsound for through-hole geometry: no centre puts
/// every hole wall on its outward side, so the 32 triangles are wound explicitly — bottom
/// normal −z, top +z, walls radially outward, hole walls pointing into the hole).
#[rustfmt::skip]
fn hole_obj() -> Vec<u8> {
    const O: [[f64; 2]; 4] = [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
    const I: [[f64; 2]; 4] = [[3.0, 3.0], [5.0, 3.0], [5.0, 5.0], [3.0, 5.0]];
    let mut m = TriMesh { verts: Vec::new(), tris: Vec::new() };
    // Inner and outer rings are both CCW in xy; the windings below follow from that.
    for &z in &[0.0f64, 4.0] {
        for o in &O { m.verts.push([o[0], o[1], z]); }
        for i in &I { m.verts.push([i[0], i[1], z]); }
    }
    // ids: bottom outer 0..3, bottom inner 4..7, top outer 8..11, top inner 12..15.
    for k in 0..4 {
        let k1 = (k + 1) % 4;
        m.tris.push([4 + k1, k1, k]);           // bottom slab, −z
        m.tris.push([4 + k, 4 + k1, k]);
        m.tris.push([8 + k, 8 + k1, 12 + k1]);  // top slab, +z
        m.tris.push([8 + k, 12 + k1, 12 + k]);
        m.tris.push([k, k1, 8 + k1]);           // outer wall, radially out
        m.tris.push([k, 8 + k1, 8 + k]);
        m.tris.push([4 + k1, 4 + k, 12 + k1]);  // hole wall, into the hole (+ring sense)
        m.tris.push([12 + k, 12 + k1, 4 + k]);
    }
    let mut s = String::from("o block\n");
    for p in &m.verts {
        s.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    for t in &m.tris {
        s.push_str(&format!("f {} {} {}\n", t[0] + 1, t[1] + 1, t[2] + 1));
    }
    s.into_bytes()
}

// ---------------------------------------------------------------------------------------------
// The tests

#[test]
fn glb_cube_imports_and_volume_comes_through_the_document() {
    let out = import_mesh("glb", &cube_glb(), &opts(None, None, None, "cube.glb")).unwrap();
    assert_eq!(out.nodes.len(), 1);
    assert_eq!(out.nodes[0].id, "cube", "a named GLB mesh keeps its name");
    let p = &out.parts[0];
    assert_eq!(p.tris, 12);
    assert_eq!(p.patches, 6, "one flat patch per cube face");
    assert_eq!(p.open_edges, 0);
    // GLB defaults bake metres and Y-up into the millimetre, Z-up grid. The 0.01 m
    // coordinates survive as f32 (9.9999998 for a 10 mm edge), so compare loosely.
    for (got, want) in p.bbox.iter().zip([0.0, -10.0, 0.0, 10.0, 0.0, 10.0]) {
        assert!(
            (got - want).abs() < 1e-3,
            "m → mm and Y-up baked at import: bbox {p:?}",
            p = p.bbox
        );
    }

    let doc = part_doc(&out.nodes, &[], "GLB cube");
    let (vol, err, spacing) = doc_volume_all(&doc);
    let want = 10.0 * 10.0 * 10.0;
    assert!(
        (vol - want).abs() / want < 0.01,
        "volume {vol} ± {err} at h {spacing} vs {want}"
    );
    assert!(sample(&doc, [5.0, -5.0, 5.0]) < 0.0, "interior is solid");
    assert!(
        sample(&doc, [5.0, -5.0, 10.5]) > 0.0,
        "just outside the +z face is air"
    );
}

#[test]
fn sphere_through_the_stl_chain_matches_the_closed_form() {
    let bytes = binary_stl(&uv_sphere(5.0, [5.0, 5.0, 5.0], 64, 32));
    let out = import_mesh("stl", &bytes, &opts(None, None, None, "sphere.stl")).unwrap();
    assert_eq!(
        out.nodes[0].id, "mesh1",
        "an anonymous STL part numbers mesh1"
    );
    let p = &out.parts[0];
    assert_eq!(p.open_edges, 0);
    // A smooth closed surface is one 30°-smooth patch — measured 1 here, the same
    // measurement the real Meshy STL gives (one patch, no plane cut in it works).
    assert_eq!(
        p.patches, 1,
        "a 64×32 sphere at 30° dihedral is one smooth patch"
    );

    let doc = part_doc(&out.nodes, &[], "STL sphere");
    let (vol, err, spacing) = doc_volume_all(&doc);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    assert!(
        (vol - exact).abs() / exact < 0.01,
        "volume {vol} ± {err} at h {spacing} vs {exact:.3}"
    );
    // The centre is 5 mm above the surface — beyond the 3-cell band (0.75 at the report's
    // 0.25 spacing) — so the conservative clamp floors the interior distance at −0.75. It
    // reads solid, never the exact −5; the band-exactness sample above covers exactness.
    let centre = sample(&doc, [5.0, 5.0, 5.0]);
    assert!(
        centre < 0.0 && centre > -0.76,
        "the centre reads solid, clamped to the −0.75 band floor: {centre}"
    );
    let pole = sample(&doc, [5.0, 5.0, 9.5]); // 0.5 inside from the pole apex
    assert!(
        (pole + 0.5).abs() < 0.05,
        "inside the 3-cell band the distance is exact, not clamped: {pole}"
    );
}

#[test]
fn concave_obj_with_a_hole_measures_the_closed_form() {
    let out = import_mesh("obj", &hole_obj(), &opts(None, None, None, "hole.obj")).unwrap();
    let p = &out.parts[0];
    assert_eq!(p.tris, 32);
    assert_eq!(
        p.patches, 10,
        "two slabs, four outer walls, four hole walls"
    );
    assert_eq!(p.open_edges, 0);

    let doc = part_doc(&out.nodes, &[], "OBJ hole");
    let (vol, err, spacing) = doc_volume_all(&doc);
    let want = 8.0 * 8.0 * 4.0 - 2.0 * 2.0 * 4.0;
    assert!(
        (vol - want).abs() / want < 0.01,
        "volume {vol} ± {err} at h {spacing} vs {want}"
    );
    assert!(sample(&doc, [4.0, 4.0, 2.0]) > 0.0, "the hole reads open");
    assert!(
        sample(&doc, [2.0, 4.0, 2.0]) < 0.0,
        "solid between the hole and the outer wall"
    );
}

/// One region per segmentation patch (ids start at 1) plus the geometric face rule.
///
/// Measured on this fixture at h = 0.5 (20-cell faces → 1600 boundary facets per face,
/// 9600 total): the six patch regions partition the boundary disjointly, but patch
/// attribution is edge-fringe noisy — facets whose centroid lands exactly on a face
/// junction diagonal sometimes get the neighbouring patch (worst measured purity 1600/1680
/// = 95.2%), and two patches lose 40 of their own facets to neighbours. The geometric face
/// rule is exact on the same fixture: all 1600 bottom-face facets, every one −z-facing.
#[test]
fn patch_selectors_and_the_face_rule_claim_one_cube_face_each() {
    let out = import_mesh(
        "stl",
        &cube_stl_bytes(),
        &opts(None, None, None, "cube.stl"),
    )
    .unwrap();
    let node_id = out.nodes[0].id.clone();
    let patches = match &out.nodes[0].kind {
        NodeKind::Mesh { patches, .. } => *patches as usize,
        _ => panic!("the import produced a mesh node"),
    };

    let mut regions = vec![Region {
        id: "bottom".into(),
        selectors: vec![Selector {
            shape: node_id.clone(),
            face: Face::Bottom,
            patch: None,
        }],
    }];
    for pid in 1..=patches as u32 {
        regions.push(Region {
            id: format!("p{pid}"),
            selectors: vec![Selector {
                shape: node_id.clone(),
                face: Face::All,
                patch: Some(pid),
            }],
        });
    }
    let doc = part_doc(&out.nodes, &regions, "cube patches");
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());

    let (mesh, facets) = surface_facets(&doc, 0.5);

    let mut sets: Vec<Vec<usize>> = Vec::new();
    for r in &regions {
        let fs = region_facets(&doc, &r.id, &mesh, &facets, 0.5).expect("region resolves");
        assert!(!fs.is_empty(), "region '{}' claims nothing", r.id);
        sets.push(fs);
    }

    // The six patch regions partition the whole boundary: pairwise disjoint, no facet left.
    let mut seen = vec![false; facets.len()];
    for s in &sets[1..] {
        for &fi in s {
            assert!(!seen[fi], "facet {fi} claimed by two patch regions");
            seen[fi] = true;
        }
    }
    assert_eq!(
        seen.iter().filter(|&&s| s).count(),
        facets.len(),
        "boundary facets left unclaimed by any patch selector"
    );

    // Each patch region belongs to one flat face: at least 95% of its facets share a
    // single axis normal (the rest are the edge-fringe slivers the doc comment measures).
    for (k, s) in sets.iter().enumerate().skip(1) {
        let mut hist: Vec<([i64; 3], usize)> = Vec::new();
        for &fi in s {
            let key = facets[fi]
                .normal(&mesh)
                .map(|c| (c * 1000.0).round() as i64);
            match hist.iter_mut().find(|(k2, _)| *k2 == key) {
                Some((_, c)) => *c += 1,
                None => hist.push((key, 1)),
            }
        }
        let top = hist.iter().map(|(_, c)| *c).max().unwrap_or(0);
        assert!(
            top as f64 / s.len() as f64 >= 0.95,
            "patch '{}' is not one face: dominant normal covers {top}/{} of its facets",
            regions[k].id,
            s.len()
        );
    }

    // The geometric face rule is exact: the whole bottom face, every facet −z-facing.
    let bottom = &sets[0];
    let per_face = ((10.0 / 0.5) as usize).pow(2) * 4;
    assert_eq!(
        bottom.len(),
        per_face,
        "the face rule misses part of the bottom face"
    );
    for fi in bottom {
        let n = facets[*fi].normal(&mesh);
        assert!(
            (n[0]).abs() < 1e-9 && (n[1]).abs() < 1e-9 && (n[2] + 1.0).abs() < 1e-9,
            "the bottom region carries facet with normal {n:?}"
        );
    }
}

/// The GOAL.md budget: a generated model near 200k triangles voxelises within 5 s and the
/// report carries an honest cell count.
#[test]
fn two_hundred_k_tri_sphere_voxelises_within_budget() {
    // 400 meridian rings × 260 around: 207,560 triangles, ~104k welded vertices.
    let bytes = binary_stl(&uv_sphere(50.0, [50.0, 50.0, 50.0], 400, 260));
    let out = import_mesh("stl", &bytes, &opts(None, None, None, "big.stl")).unwrap();
    let p = &out.parts[0];
    assert!(p.tris >= 200_000, "generated {} triangles", p.tris);

    let key = fieldsmith_field::meshfield::key_from_hex(&p.hash);
    let f = fieldsmith_field::meshfield::lookup(key).expect("hydrated");
    assert_eq!(
        (f.dist.dims[0] * f.dist.dims[1] * f.dist.dims[2]),
        p.cells,
        "the cells report is the grid's node count"
    );
    assert!(
        p.cells <= fieldsmith_core::voxel::MAX_CELLS,
        "cells {} within the budget",
        p.cells
    );
    let budget_ms = if std::env::var_os("FIELDSMITH_SMALL_RAM").is_some() {
        15_000
    } else {
        5_000
    };
    assert!(
        out.ms <= budget_ms,
        "import (read + weld + segment + voxelize) took {} ms; budget {} ms",
        out.ms,
        budget_ms
    );
}

// ---------------------------------------------------------------------------------------------
// Extra fixture helpers shared by two tests

/// The 12-triangle ASCII box (same outward quads the other suites use), for the region test.
fn cube_stl_bytes() -> Vec<u8> {
    let v = |i: usize, j: usize, k: usize| {
        format!(
            "vertex {} {} {}",
            10.0 * i as f64,
            10.0 * j as f64,
            10.0 * k as f64
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
// ---------------------------------------------------------------------------------------------
// 3MF round trip (goal M7, step 6): export a mesh to 3MF with our own writer, import it
// back through the real chain, and check the volume survives.

#[test]
fn threemf_exports_and_reimports_with_the_volume_intact() {
    // Mesh a 20 mm cube, export it as 3MF, re-import the bytes through the real chain.
    let cube = surface(
        |p: V3| {
            let t = [p[0].abs() - 10.0, p[1].abs() - 10.0, p[2].abs() - 10.0];
            let o = [t[0].max(0.0), t[1].max(0.0), t[2].max(0.0)];
            (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt() + t[0].max(t[1]).max(t[2]).min(0.0)
        },
        [-10.0; 3],
        [10.0; 3],
        0.5,
    );
    let v_export = cube.volume();
    let bytes = cube.write_3mf("Round trip", "petg");
    println!(
        "export: {} verts, {} tris, volume {:.1} mm³, {} bytes 3MF",
        cube.verts.len(),
        cube.tris.len(),
        v_export,
        bytes.len()
    );

    let out = import_mesh("3mf", &bytes, &opts(None, None, None, "roundtrip.3mf")).unwrap();
    assert_eq!(out.nodes.len(), 1, "one object in our writer's file");
    // The object's name attribute names the part, sanitized to a valid node id
    // (spaces become underscores).
    assert_eq!(
        out.nodes[0].id, "Round_trip",
        "the object's name attribute names the part"
    );
    let p = &out.parts[0];
    println!(
        "reimport: {} tris, {} welded verts, {} patches, {} open edges, bbox {} mm, voxel {} mm",
        p.tris,
        p.verts,
        p.patches,
        p.open_edges,
        p.bbox.map(|v| v.to_string()).join(" "),
        p.voxel
    );
    assert_eq!(p.open_edges, 0, "the round-tripped mesh is watertight");
    // Our writer emits 6-decimal coordinates, so the re-imported vertices match the
    // export's to the printed precision; the volume is intact to well under 1 %.
    assert_eq!(p.tris as usize, cube.tris.len(), "triangle count survives");

    let doc = part_doc(&out.nodes, &[], "3MF round trip");
    let (vol, err, spacing) = doc_volume_all(&doc);
    println!("reimported volume {vol:.1} ± {err:.1} at h {spacing} vs export {v_export:.1}");
    assert!(
        (vol - v_export).abs() / v_export < 0.01,
        "volume {vol} ± {err} vs export {v_export}"
    );
    // The re-imported part is a field leaf: solid inside, vacuum outside. The writer
    // shifts vertices into the positive octant, so the cube spans [0, 20]³.
    assert!(sample(&doc, [10.0, 10.0, 10.0]) < 0.0, "interior is solid");
    assert!(
        sample(&doc, [10.0, 10.0, 21.0]) > 0.0,
        "just outside the +z face is air"
    );
}
