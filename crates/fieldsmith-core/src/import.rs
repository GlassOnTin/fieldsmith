//! Mesh import (goal M6, step 6): read a GLB / STL / OBJ, bake units and the up-axis in,
//! weld and segment patches, voxelize, hydrate the session registry and emit document
//! nodes — one [`Node`] per named part, named from the file's scene structure.
//!
//! Everything heavy lives here in plain Rust so wasm bindings, the CLI `import` verb and
//! tests drive the identical path. The content hash covers the source bytes plus the whole
//! bake recipe (kind, source path, part index, part name, units, up-axis, requested voxel
//! size), so a re-import with different parameters keys a fresh registry entry instead of
//! silently reusing stale grids.

use crate::now_ms;
use fieldsmith_doc::{Node, NodeKind, Op, Transform};
use fieldsmith_field::meshfield::{hash_mesh_input, hydrate};
use fieldsmith_mesh::export::{read_stl_any, TriMesh};
use fieldsmith_mesh::{gltf, obj, threemf, weld, NamedPart};

/// Dihedral angle (radians, matching `weld::segment`'s inputs and its tests) below which
/// neighbouring triangles join one patch. Doubling when a mesh splits past the patch-grid
/// cap (see the import loop).
const SEGMENT_DEG: f64 = 30.0_f64.to_radians();

/// Units the source coordinates are in, converted to millimetres at import.
fn unit_scale(units: &str) -> Result<f64, String> {
    Ok(match units {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" => 25.4,
        other => return Err(format!("units '{other}' is not one of mm, cm, m, in")),
    })
}

/// Rotate +90° about X (mapping the source +Y into world +Z) when the up-axis is `y`.
fn up_correction(up: &str) -> Result<bool, String> {
    match up {
        "y" => Ok(true),
        "z" => Ok(false),
        other => Err(format!("up-axis '{other}' is not one of y, z")),
    }
}

/// Scale and rotate the source coordinates into millimetre, Z-up world space. A uniform
/// positive scale and a proper rotation keep triangle windings outward.
fn bake(m: &TriMesh, scale: f64, up_y: bool) -> TriMesh {
    let verts = m
        .verts
        .iter()
        .map(|p| {
            let p = if up_y { [p[0], -p[2], p[1]] } else { *p };
            [p[0] * scale, p[1] * scale, p[2] * scale]
        })
        .collect();
    TriMesh {
        verts,
        tris: m.tris.clone(),
    }
}

/// Weld tolerance relative to the mesh extent: binary STL rewrites each corner in f32 so
/// shared corners agree exactly, ASCII writers round to ~6 digits; 1e-5 of the diagonal
/// covers that while keeping distinct detail vertices apart.
fn weld_tol(m: &TriMesh) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [-f64::INFINITY; 3];
    for p in &m.verts {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    let diag = (hi[0] - lo[0]).hypot(hi[1] - lo[1]).hypot(hi[2] - lo[2]);
    (diag * 1e-5).clamp(1e-6, 0.01)
}

/// An imported part name as a legal node id (`[A-Za-z_][A-Za-z0-9_]*`): every other
/// character collapses to `_`, a digit-leading name gains an `m` prefix, an empty one
/// falls back to "mesh".
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        return "mesh".into();
    }
    if s.chars().next().unwrap().is_ascii_digit() {
        format!("m{s}")
    } else {
        s
    }
}

/// The first free id from `base` — the bare name for real part names; the anonymous STL /
/// OBJ placeholder "mesh" is numbered from 1 (mesh1, mesh2, …) so a lone import reads
/// `mesh1` like a fresh primitve would read `box1`.
fn pick_id(base: &str, taken: &[String]) -> String {
    let taken_id = |c: &String| taken.iter().any(|t| t == c);
    if base != "mesh" && !taken_id(&base.to_string()) {
        return base.into();
    }
    (1..)
        .map(|i| format!("{base}{i}"))
        .find(|c| !taken_id(c))
        .unwrap()
}

/// One part's honest import report (mirrored into the node; re-shown after reload).
#[derive(Clone, Debug, serde::Serialize)]
pub struct ImportReport {
    /// The part's raw name (GLB node / OBJ object; the file stem slot for STL).
    pub name: String,
    /// The file's own material label when the format records one (3MF basematerials);
    /// reported but not adopted as a solve or document material — another tool's labels
    /// are not this document's material ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The generated node id.
    pub id: String,
    pub tris: u32,
    /// Welded vertex count — what the voxeliser actually consumed.
    pub verts: usize,
    pub patches: u32,
    pub open_edges: u32,
    /// Baked world-space bounds (mm): `[xmin, ymin, zmin, xmax, ymax, zmax]`.
    pub bbox: [f64; 6],
    /// 16-hex-digit registry key (`fsm`'s `hash=` field).
    pub hash: String,
    /// Requested voxel spacing (mm) — possibly widened in the bake; the baked spacing is
    /// `spacing`.
    pub voxel: f64,
    /// Baked spacing of the distance grid (mm).
    pub spacing: f64,
    /// Grid node count (the bake's largest allocation).
    pub cells: usize,
    /// This part's weld + segment + voxelize time (ms).
    pub ms: u64,
}

/// The import result: nodes to splice into `doc.root` (in `parts` order) and the reports.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ImportOut {
    pub nodes: Vec<Node>,
    pub parts: Vec<ImportReport>,
    /// Whole-import wall-clock time (ms), reads included.
    pub ms: u64,
}

/// Import parameters. `units` / `up` default per kind (GLB: metres, Y-up; STL/OBJ:
/// millimetres, Z-up). `taken` lists ids already in the document so generated node ids
/// stay unique; `src` is provenance only.
pub struct ImportOpts<'a> {
    pub units: Option<&'a str>,
    pub up: Option<&'a str>,
    /// Requested voxel spacing (mm); `None` uses the default derivation
    /// (`voxel::default_spacing`, widened to fit the cell budget).
    pub voxel: Option<f64>,
    pub src: &'a str,
    pub taken: &'a [String],
}

/// Read `bytes` as the mesh kind (`"glb"`, `"stl"` or `"obj"`), returning one named part
/// per scene element (STL is a single anonymous part).
fn read_any(kind: &str, bytes: &[u8]) -> Result<Vec<NamedPart>, String> {
    match kind {
        "glb" => gltf::read_glb(bytes),
        "stl" => {
            let mesh = read_stl_any(bytes)
                .ok_or_else(|| "not recognised as a binary or ASCII STL file".to_string())?;
            Ok(vec![NamedPart {
                name: "mesh".into(),
                mesh,
                material: None,
            }])
        }
        "obj" => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| "OBJ source is not valid UTF-8 text".to_string())?;
            obj::read_obj(text)
        }
        "3mf" => threemf::read_3mf(bytes),
        _ => Err(format!("kind '{kind}' is not one of glb, stl, obj, 3mf")),
    }
}

/// Import `bytes` as the mesh kind (`"glb"`, `"stl"`, `"obj"` or `"3mf"`) and hydrate the
/// registry. A 3MF file's units come from its `<model unit="…">` attribute (the reader
/// scales to millimetres); the `units` override still applies on top.
pub fn import_mesh(kind: &str, bytes: &[u8], opts: &ImportOpts) -> Result<ImportOut, String> {
    let t0 = now_ms();
    if !matches!(kind, "glb" | "stl" | "obj" | "3mf") {
        return Err(format!("kind '{kind}' is not one of glb, stl, obj, 3mf"));
    }
    let parts = read_any(kind, bytes).map_err(|e| format!("{kind} import failed: {e}"))?;
    if parts.is_empty() {
        return Err("the file contains no geometry".into());
    }

    // A 3MF file's units come from its model attribute and are already scaled to
    // millimetres by the reader; the `units` override applies on top of the file's own
    // scale only when the caller names one (the default is mm, i.e. no extra scale).
    let (def_units, def_up) = match kind {
        "glb" => ("m", "y"),
        _ => ("mm", "z"),
    };
    let units = opts.units.unwrap_or(def_units);
    let up = opts.up.unwrap_or(def_up);
    let scale = if kind == "3mf" {
        // The reader already applied the file's unit scale; the override, when given,
        // treats the file's coordinates as being in that unit (an explicit re-interpret).
        match opts.units {
            Some(_) => unit_scale(units)?,
            None => 1.0,
        }
    } else {
        unit_scale(units)?
    };
    let up_y = up_correction(up)?;

    let mut taken = opts.taken.to_vec();
    let mut nodes = Vec::with_capacity(parts.len());
    let mut reports = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let baked = bake(&part.mesh, scale, up_y);
        let m = weld::weld_verts(&baked, weld_tol(&baked));
        // Segment patches; a finely-detailed surface can split past the patch-grid cap
        // (the moon-globe lithophane emits 228k patches at 30°), so coarsen the dihedral
        // angle and re-segment until the ids fit. Every surface is still segmented —
        // just at a coarser angle, with the report showing the resulting patch count.
        let adj = weld::edge_adjacency(&m);
        let mut deg = SEGMENT_DEG;
        let mut segs = weld::segment(&m, &adj, deg);
        while segs
            .iter()
            .max()
            .is_some_and(|&mx| mx > crate::voxel::MAX_PATCH_ID)
            && deg < std::f64::consts::PI
        {
            deg = (deg * 2.0).min(std::f64::consts::PI);
            segs = weld::segment(&m, &adj, deg);
        }
        let v = crate::voxel::voxelize(&m, Some(&segs), opts.voxel)
            .map_err(|e| format!("import of '{}': {e}", part.name))?;
        let meta = &v.field.meta;
        let bbox = meta.bbox;

        // Requested spacing for the node: an explicit value, else the same default the
        // voxeliser derived (`fit_spacing(None)` starts from `default_spacing`).
        let req_voxel = opts
            .voxel
            .unwrap_or_else(|| crate::voxel::default_spacing(&bbox));
        let salt = format!(
            "{kind}|{}|{i}|{}|{units}|{up}|{req_voxel:.6}",
            opts.src, part.name
        );
        let key = hash_mesh_input(bytes, salt.as_bytes());
        let hash = format!("{key:016x}");
        hydrate(key, v.field.clone());

        let id = pick_id(&sanitize(&part.name), &taken);
        taken.push(id.clone());
        nodes.push(Node {
            id: id.clone(),
            op: Op::Add,
            blend: 0.0,
            suppressed: false,
            transform: Transform::default(),
            material: None,
            kind: NodeKind::Mesh {
                src: opts.src.into(),
                hash: hash.clone(),
                units: units.into(),
                up: up.into(),
                voxel: req_voxel,
                bbox: [
                    bbox.lo[0], bbox.lo[1], bbox.lo[2], bbox.hi[0], bbox.hi[1], bbox.hi[2],
                ],
                tris: meta.tris,
                patches: meta.patches,
                open_edges: meta.open_edges,
            },
        });
        reports.push(ImportReport {
            name: part.name.clone(),
            material: part.material.clone(),
            id,
            tris: meta.tris,
            verts: m.verts.len(),
            patches: meta.patches,
            open_edges: meta.open_edges,
            bbox: [
                bbox.lo[0], bbox.lo[1], bbox.lo[2], bbox.hi[0], bbox.hi[1], bbox.hi[2],
            ],
            hash,
            voxel: req_voxel,
            spacing: v.field.dist.spacing,
            cells: v.cells,
            ms: v.ms,
        });
    }
    Ok(ImportOut {
        nodes,
        parts: reports,
        ms: (now_ms() - t0) as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_field::meshfield::{key_from_hex, lookup};

    /// ASCII STL of an axis-aligned box with outward windings (each face checked by hand:
    /// the per-face normal from the vertex cross product points away from the box centre).
    fn box_stl(lo: [f64; 3], hi: [f64; 3]) -> Vec<u8> {
        // Corner (i, j, k): x/y/z picks lo or hi per component.
        let v = |i: usize, j: usize, k: usize| {
            format!(
                "vertex {} {} {}",
                if i == 0 { lo[0] } else { hi[0] },
                if j == 0 { lo[1] } else { hi[1] },
                if k == 0 { lo[2] } else { hi[2] }
            )
        };
        // Quads in outward-CCW order (i, j, k components):
        // x−: yz seen from −x; x+: mirrored; y−/y+/z−/z+ likewise.
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

    fn opts<'a>(
        units: Option<&'a str>,
        up: Option<&'a str>,
        voxel: Option<f64>,
        src: &'a str,
        taken: &'a [String],
    ) -> ImportOpts<'a> {
        ImportOpts {
            units,
            up,
            voxel,
            src,
            taken,
        }
    }

    #[test]
    fn stl_box_imports_one_mesh1_node_with_six_patches() {
        let bytes = box_stl([0.0; 3], [10.0, 8.0, 6.0]);
        let out = import_mesh("stl", &bytes, &opts(None, None, None, "", &[])).unwrap();
        assert_eq!(out.nodes.len(), 1);
        let n = &out.nodes[0];
        assert_eq!(n.id, "mesh1");
        assert_eq!(n.op, Op::Add);
        let NodeKind::Mesh {
            src,
            units,
            up,
            voxel,
            bbox,
            tris,
            patches,
            open_edges,
            hash,
        } = n.kind.clone()
        else {
            panic!("node is not a mesh");
        };
        assert_eq!(src, "");
        assert_eq!(units, "mm");
        assert_eq!(up, "z");
        assert!(voxel > 0.0);
        assert_eq!(tris, 12);
        assert_eq!(patches, 6, "six flat faces, one patch each");
        assert_eq!(open_edges, 0);
        assert_eq!(
            bbox,
            [0.0, 0.0, 0.0, 10.0, 8.0, 6.0],
            "baked bounds are the source bounds for mm / Z-up"
        );
        // The node's hash opens a hydrated registry entry with the same meta.
        let key = key_from_hex(&hash);
        assert_ne!(key, 0);
        let f = lookup(key).expect("import hydrates its key");
        assert_eq!(f.meta.bbox.lo, [bbox[0], bbox[1], bbox[2]]);
        assert_eq!(f.meta.bbox.hi, [bbox[3], bbox[4], bbox[5]]);
        assert_eq!(f.meta.patches, 6);
        assert_eq!(f.dist.spacing, out.parts[0].spacing);
        // Volumetric check via the hydrated grid: a 10×8×6 box holds 480 mm³.
        let g = &f.dist;
        let mut vol = 0.0f64;
        for k in 0..g.dims[2] - 1 {
            for j in 0..g.dims[1] - 1 {
                for i in 0..g.dims[0] - 1 {
                    let p = [
                        g.origin[0] + (i as f64 + 0.5) * g.spacing,
                        g.origin[1] + (j as f64 + 0.5) * g.spacing,
                        g.origin[2] + (k as f64 + 0.5) * g.spacing,
                    ];
                    if g.sample_grad(p).v < 0.0 {
                        vol += g.spacing.powi(3);
                    }
                }
            }
        }
        assert!((vol - 480.0).abs() / 480.0 < 0.01, "volume {vol} vs 480");
    }

    #[test]
    fn units_and_up_bake_into_the_grid() {
        // A 0..2 cm box (mm numbers unchanged) imported as centimetres covers 0..20 mm.
        let bytes = box_stl([0.0; 3], [2.0, 1.0, 1.0]);
        let out = import_mesh("stl", &bytes, &opts(Some("cm"), None, None, "", &[])).unwrap();
        assert_eq!(out.parts[0].bbox, [0.0, 0.0, 0.0, 20.0, 10.0, 10.0]);

        // The same box imported as Y-up rotates (x,y,z) → (x,−z,y), so its thin axis (1 mm
        // in y) becomes z and the z extent mirrors negative.
        let bytes = box_stl([0.0, 0.0, 0.0], [10.0, 1.0, 8.0]);
        let out = import_mesh("stl", &bytes, &opts(None, Some("y"), None, "", &[])).unwrap();
        assert_eq!(out.parts[0].bbox, [0.0, -8.0, 0.0, 10.0, 0.0, 1.0]);
        assert_eq!(out.parts[0].tris, 12);

        // Bad option values are refused with the allowed sets named.
        assert_eq!(
            import_mesh("stl", &bytes, &opts(Some("ft"), None, None, "", &[])).unwrap_err(),
            "units 'ft' is not one of mm, cm, m, in"
        );
        assert_eq!(
            import_mesh("stl", &bytes, &opts(None, Some("x"), None, "", &[])).unwrap_err(),
            "up-axis 'x' is not one of y, z"
        );
    }

    #[test]
    fn reimport_recipe_changes_the_key() {
        let bytes = box_stl([0.0; 3], [5.0; 3]);
        let a = import_mesh("stl", &bytes, &opts(None, None, None, "f.stl", &[])).unwrap();
        let b = import_mesh("stl", &bytes, &opts(None, None, None, "f.stl", &[])).unwrap();
        assert_eq!(
            a.parts[0].hash, b.parts[0].hash,
            "same file + recipe → same key"
        );
        let c = import_mesh("stl", &bytes, &opts(None, None, Some(1.0), "f.stl", &[])).unwrap();
        assert_ne!(
            a.parts[0].hash, c.parts[0].hash,
            "changed voxel → fresh key"
        );
        let d = import_mesh("stl", &bytes, &opts(None, None, None, "g.stl", &[])).unwrap();
        assert_ne!(
            a.parts[0].hash, d.parts[0].hash,
            "changed src provenance → fresh key"
        );
    }

    #[test]
    fn ids_keep_clear_of_taken_and_each_other() {
        let bytes = box_stl([0.0; 3], [5.0; 3]);
        let out = import_mesh(
            "stl",
            &bytes,
            &opts(None, None, None, "", &["mesh".into(), "mesh1".into()]),
        )
        .unwrap();
        assert_eq!(
            out.nodes[0].id, "mesh2",
            "taken mesh1 pushes the anonymous node to mesh2"
        );
    }

    #[test]
    fn obj_import_names_parts_from_objects() {
        let obj = b"\
v 0 0 0
v 2 0 0
v 2 2 0
v 0 2 0
v 0 0 2
v 2 0 2
v 2 2 2
v 0 2 2
o base
f 1 4 3
f 1 3 2
f 5 6 7
f 5 7 8
f 1 2 6
f 1 6 5
f 2 3 7
f 2 7 6
f 3 4 8
f 3 8 7
f 4 1 5
f 4 5 8
";
        let out = import_mesh("obj", obj, &opts(None, None, None, "", &[])).unwrap();
        assert_eq!(out.nodes[0].id, "base");
        assert_eq!(out.parts[0].name, "base");
        assert_eq!(out.parts[0].patches, 6);
        assert_eq!(out.parts[0].open_edges, 0);

        let bad = import_mesh("obj", b"", &opts(None, None, None, "", &[]));
        assert!(bad.is_err(), "empty file refuses");
        assert_eq!(
            import_mesh("step", b"", &opts(None, None, None, "", &[])).unwrap_err(),
            "kind 'step' is not one of glb, stl, obj, 3mf"
        );
    }

    /// Zip a model-text-only 3MF in the mesh crate writer's layout.
    fn zip_model(model: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut records = Vec::new();
        fieldsmith_mesh::zip::stream_entry(
            &mut out,
            "[Content_Types].xml",
            b"<Types/>",
            &mut records,
        );
        fieldsmith_mesh::zip::stream_entry(
            &mut out,
            "_rels/.rels",
            b"<Relationships/>",
            &mut records,
        );
        fieldsmith_mesh::zip::stream_entry(
            &mut out,
            "3D/3dmodel.model",
            model.as_bytes(),
            &mut records,
        );
        fieldsmith_mesh::zip::write_central(&mut out, &records);
        out
    }

    #[test]
    fn threemf_import_makes_a_leaf_per_object_with_its_material() {
        // Two tetrahedron objects (watertight, legs 10 and 5 mm), each carrying a
        // basematerials label: the import yields one named node per object and reports
        // each one's file material without adopting it as a document material.
        let tet = |id: u32, name: &str, pindex: u32, s: f64| {
            format!(
                "<object id=\"{id}\" type=\"model\" name=\"{name}\" pid=\"1\" pindex=\"{pindex}\"><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"{s}\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"{s}\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"{s}\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
<triangle v1=\"0\" v2=\"3\" v3=\"2\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\"/></triangles>\
</mesh></object>"
            )
        };
        let model = format!(
            "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>\
<basematerials id=\"1\"><base name=\"PETG\"/><base name=\"TPU95A\"/></basematerials>{}{}</resources>\
<build><item objectid=\"2\"/><item objectid=\"3\"/></build></model>",
            tet(2, "body", 0, 10.0),
            tet(3, "clip", 1, 5.0),
        );
        let out = import_mesh(
            "3mf",
            &zip_model(&model),
            &opts(None, None, None, "t.3mf", &[]),
        )
        .unwrap();
        assert_eq!(out.nodes.len(), 2, "one mesh node per object");
        assert_eq!(out.nodes[0].id, "body");
        assert_eq!(out.nodes[1].id, "clip");
        assert_eq!(out.parts[0].material.as_deref(), Some("PETG"));
        assert_eq!(out.parts[1].material.as_deref(), Some("TPU95A"));
        assert_eq!(out.parts[0].open_edges, 0, "each tet is a closed shell");
        assert_eq!(out.parts[0].tris, 4);
        // The generated nodes carry no document material: the file's labels are
        // provenance, and material assignment stays a document decision.
        assert_eq!(
            out.nodes[0].material, None,
            "import does not adopt file labels"
        );
    }

    #[test]
    fn threemf_import_expands_instances_into_leaves() {
        // One object built twice with item transforms: two nodes, in build order, named
        // apart by the id chooser, each in its item's place (the transform is baked, so
        // the two bboxes sit apart by the translation).
        let tet = "<object id=\"2\" type=\"model\" name=\"peg\"><mesh><vertices>\
<vertex x=\"0\" y=\"0\" z=\"0\"/><vertex x=\"4\" y=\"0\" z=\"0\"/>\
<vertex x=\"0\" y=\"4\" z=\"0\"/><vertex x=\"0\" y=\"0\" z=\"4\"/></vertices>\
<triangles><triangle v1=\"0\" v2=\"2\" v3=\"1\"/><triangle v1=\"0\" v2=\"1\" v3=\"3\"/>\
<triangle v1=\"0\" v2=\"3\" v3=\"2\"/><triangle v1=\"1\" v2=\"2\" v3=\"3\"/></triangles>\
</mesh></object>";
        let model = format!(
            "<?xml version=\"1.0\"?><model unit=\"millimeter\"><resources>{tet}</resources>\
<build><item objectid=\"2\"/><item objectid=\"2\" \
transform=\"1 0 0 0 1 0 0 0 1 40 0 0\"/></build></model>"
        );
        let out = import_mesh(
            "3mf",
            &zip_model(&model),
            &opts(None, None, None, "p.3mf", &[]),
        )
        .unwrap();
        assert_eq!(out.nodes.len(), 2, "one node per build item");
        assert_eq!(out.nodes[0].id, "peg");
        assert_eq!(out.nodes[1].id, "peg1", "the id chooser numbers the duplicate");
        let size = |p: &ImportReport| [p.bbox[3] - p.bbox[0], p.bbox[4] - p.bbox[1], p.bbox[5] - p.bbox[2]];
        assert_eq!(size(&out.parts[0]), size(&out.parts[1]), "shape is identical");
        assert_eq!(out.parts[0].bbox[0], 0.0);
        assert_eq!(out.parts[1].bbox[0], 40.0, "the item translation is baked");
    }
}
