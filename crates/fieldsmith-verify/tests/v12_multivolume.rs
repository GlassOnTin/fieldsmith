//! Goal M9 / V12 — the multi-volume round trip against a foreign file. The 3MF
//! Consortium's `multiple_cylinders.3mf` (vendored under `tests/fixtures/`, BSD-2-Clause,
//! © 2018 3MF Consortium, `https://github.com/3MFConsortium/3mf-samples`) is one mesh
//! object instanced six times with build-item transforms and a basematerials label on the
//! object: the reader expands the items, the import makes one field leaf per placed item
//! without adopting the file's ids, the composed part carries labels with a mixed-material
//! warning, and the labelled per-volume export reads back with its materials.
//!
//! Legs that are not in this file: the headless compose/solve/re-export CLI run and the
//! PrusaSlicer measurements are recorded in `docs/verification/V12.md`; the reader's own
//! acceptance and rejection cases are in `fieldsmith-mesh/tests/readers.rs`.

use fieldsmith_core::import::{import_mesh, ImportOpts, ImportOut};
use fieldsmith_core::{region_facets, Facet};
use fieldsmith_doc::{Document, Face, NodeKind, Op, Region, Selector, Transform};
use fieldsmith_field::expr;
use fieldsmith_mesh::export::{surface, write_3mf_volumes, TriMesh, Volume3mf};
use fieldsmith_mesh::threemf::read_3mf;
use fieldsmith_solve::P2Mesh;
use fieldsmith_solve::p2::face_nodes;

const SAMPLE: &[u8] = include_bytes!("fixtures/multiple_cylinders.3mf");
const SAMPLE_SRC: &str = "multiple_cylinders.3mf";

fn import_sample() -> ImportOut {
    let opts = ImportOpts {
        units: None,
        up: None,
        voxel: None,
        src: SAMPLE_SRC,
        taken: &[],
        step_chord: None,
    };
    import_mesh("3mf", SAMPLE, &opts).expect("the Consortium sample imports")
}

#[test]
fn the_foreign_sample_imports_as_six_placed_leaves() {
    let out = import_sample();
    assert_eq!(out.nodes.len(), 6, "one leaf per build item");
    for (i, n) in out.nodes.iter().enumerate() {
        // The sample names its object "Cylinder"; the id chooser numbers item 2+ as
        // Cylinder1, Cylinder2, … (case preserved).
        let want = if i == 0 { "Cylinder".to_string() } else { format!("Cylinder{i}") };
        assert_eq!(n.id, want, "the id chooser numbers the instances");
        assert_eq!(n.material, None, "import does not adopt the file's ids");
    }
    for p in &out.parts {
        assert_eq!(p.open_edges, 0, "each instance is a closed shell");
        assert_eq!(p.tris, 88, "the sample's tessellation count");
    }
    // Distinct placements: the items are a 3×2 array — three x columns at 0, 21 and 42 mm
    // (the sample's item translations), two items per column.
    let mut xs: Vec<f64> = out.parts.iter().map(|p| p.bbox[0]).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for (i, x) in xs.into_iter().enumerate() {
        assert!(
            (x - (i / 2) as f64 * 21.0).abs() < 1e-6,
            "instance {i} of the sorted column list sits at x {x}, the sample's item translation"
        );
    }
}

#[test]
fn the_composed_part_labels_volumes_and_warns_mixing() {
    let out = import_sample();
    let mut doc = Document {
        name: "Sample plate".into(),
        material: "petg".into(),
        ..Default::default()
    };
    doc.root = out.nodes.clone();
    // A connecting plate through all six cylinders, carrying the other label.
    doc.root.push(fieldsmith_doc::Node {
        id: "plate".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform {
            translate: [31.0, 9.9, 10.0],
            rotate: [0.0; 3],
        },
        material: Some("abs".into()),
        color: None,
        kind: NodeKind::Box {
            size: [80.0, 21.0, 4.0],
            round: 0.5,
        },
    });
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    let w = doc.material_warnings();
    assert_eq!(w.len(), 1, "one warning for the mixed pair: {w:?}");
    assert!(w[0].contains("abs"), "{:?}", w[0]);
    assert!(w[0].contains("petg"), "{:?}", w[0]);
    assert!(w[0].contains("plate"), "{:?}", w[0]);
    // With the cylinder labelled the document material and the plate's label removed,
    // nothing differs from the solve's material: no warning.
    doc.root[0].material = Some("petg".into());
    doc.root[6].material = None;
    assert!(
        doc.material_warnings().is_empty(),
        "same-material labels warn about nothing"
    );
}

#[test]
fn an_imported_leaves_patch_labels_face_regions() {
    // The composed part from the previous case, with a region on cylinder one's patch 1
    // (an imported mesh's labelled face) and the plate's +z face: both resolve onto the
    // analysis surface, disjointly.
    let out = import_sample();
    let mut doc = Document {
        name: "Sample plate".into(),
        material: "petg".into(),
        ..Default::default()
    };
    doc.root = out.nodes.clone();
    doc.root.push(fieldsmith_doc::Node {
        id: "plate".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform {
            translate: [31.0, 9.9, 10.0],
            rotate: [0.0; 3],
        },
        material: None,
        color: None,
        kind: NodeKind::Box {
            size: [80.0, 21.0, 4.0],
            round: 0.5,
        },
    });
    doc.regions.push(Region {
        id: "hold".into(),
        selectors: vec![Selector {
            shape: "Cylinder1".into(),
            face: Face::All,
            patch: Some(1),
        }],
    });
    doc.regions.push(Region {
        id: "press".into(),
        selectors: vec![Selector {
            shape: "plate".into(),
            face: Face::Py,
            patch: None,
        }],
    });
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());

    let body = expr::compile_body(&doc);
    let b = expr::bounds(&body);
    let (mut lin, _) = fieldsmith_mesh::stuff(
        |p| expr::eval(&body, p).v,
        b.lo,
        b.hi,
        1.0,
    );
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
    let hold = region_facets(&doc, "hold", &mesh, &facets, 1.0).expect("patch region resolves");
    let press = region_facets(&doc, "press", &mesh, &facets, 1.0).expect("face region resolves");
    assert!(!hold.is_empty(), "the imported leaf's patch 1 claims facets");
    assert!(!press.is_empty(), "the plate's +z face claims facets");
    for &a in &hold {
        assert!(
            !press.contains(&a),
            "the two regions claim facet {a} twice"
        );
    }
}

#[test]
fn the_composed_part_exports_per_volume_and_reads_back_labelled() {
    let out = import_sample();
    let mut doc = Document {
        name: "Sample plate".into(),
        material: "petg".into(),
        ..Default::default()
    };
    doc.root = out.nodes.clone();
    doc.root.push(fieldsmith_doc::Node {
        id: "plate".into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform {
            translate: [31.0, 9.9, 10.0],
            rotate: [0.0; 3],
        },
        material: Some("abs".into()),
        color: None,
        kind: NodeKind::Box {
            size: [80.0, 21.0, 4.0],
            round: 0.5,
        },
    });

    // The export path (mirrors the CLI's per-volume policy for this add-only document):
    // mesh each node alone at the report spacing, label node material or the document
    // material, and ship the volumes.
    const H: f64 = 0.5;
    let mut meshes: Vec<(String, String, TriMesh)> = Vec::new();
    for n in &doc.root {
        let fe = expr::compile_node(&doc, n);
        let vol = fieldsmith_field::volume::volume(&fe, H);
        let nb = expr::bounds(&fe);
        let tri = surface(
            |p| expr::eval(&fe, p).v,
            [
                nb.lo[0] - H,
                nb.lo[1] - H,
                nb.lo[2] - H,
            ],
            [
                nb.hi[0] + H,
                nb.hi[1] + H,
                nb.hi[2] + H,
            ],
            H,
        );
        // The mesher's own volume against its field reference, per volume.
        let mv = tri.volume();
        assert!(
            (mv - vol.volume).abs() / vol.volume < 0.01,
            "volume '{}' mesh {:.1} vs field {:.1}",
            n.id,
            mv,
            vol.volume
        );
        let mat = n.material.clone().unwrap_or_else(|| doc.material.clone());
        meshes.push((n.id.clone(), mat, tri));
    }
    let vols: Vec<Volume3mf> = meshes
        .iter()
        .map(|(id, mat, m)| Volume3mf {
            name: id,
            material: mat,
            color: "",
            mesh: m,
        })
        .collect();
    let bytes = write_3mf_volumes(&vols);
    let back = read_3mf(&bytes).expect("our own package reads back");
    assert_eq!(back.len(), 7, "one object per volume");
    let mats: Vec<&str> = back.iter().map(|p| p.material.as_deref().unwrap_or("")).collect();
    assert_eq!(mats[..6], ["petg"; 6], "unlabelled cylinders carry the document material");
    assert_eq!(mats[6], "abs", "the plate carries its own label");
    for (p, (id, _, m)) in back.iter().zip(&meshes) {
        assert_eq!(p.name, *id);
        assert_eq!(p.mesh.verts.len(), m.verts.len());
        assert_eq!(p.mesh.tris, m.tris, "volume '{id}' survives the round trip");
    }
}