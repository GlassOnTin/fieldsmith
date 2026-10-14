//! M10 — V14: mainstream-CAD STEP files through the user's chain: B-rep tessellation into
//! the same mesh path as GLB/OBJ/STL/3MF, open-edge count, the analytic flux volume against
//! the tessellated mesh's divergence volume (the field estimator's auto-grid bias on thin
//! walls is a separate honesty — see the `fieldsmith volume` numbers in
//! docs/verification/V14.md), cone-face surface coverage, and the 3MF re-export round trip.
//!
//! Fixture ladder (goal): the NIST MBE PMI Validation and Conformance Testing Project's
//! CTC parts (AP203 e2; vendored under tests/fixtures/). Three are fully
//! analytic — planes, cylinders, cones — so the tessellator refuses nothing:
//!    CTC-01 rd — millimetre file, 139 b-rep faces, 2 cones (faces 73 and 75 in file order);
//!    CTC-03 rc — inch-unit file (the reader scales to mm), 139 faces, 0 cones — its shape
//!                is a thin ~3.2 mm sheet, so the `fieldsmith volume` auto grid cannot bind
//!                the 1 % row on it; the tessellated-mesh divergence row below can;
//!    CTC-04 rd — millimetre file, 517 faces, 116 half-window cones, and the rib-root plane
//!                whose earcut wedges run chords spanning three rings' edges on one
//!                parameter line (the tess_structured/earcut collinearity case).
//! CTC-02 rc and CTC-05 rd carry B-spline faces (outside the analytic row) and are also
//! vendored. CTC-02 is a millimetre file (raw coordinates ~1E2); CTC-05 declares INCH in
//! its single GLOBAL_UNIT_ASSIGNED_CONTEXT — the SI millimetre units in the file are only
//! the conversion-factor bases (25.4 mm) — and its raw coordinates are inch-scale, so the
//! reader's INCH resolution is the correct one: after ITS scaling the part matches
//! CTC-02's millimetre-scaled geometry (same CTC part, same dimensions).

use fieldsmith_core::import::{import_mesh, ImportOpts};
use fieldsmith_core::step::tessellate;

const CTC01: &[u8] = include_bytes!("fixtures/nist_ctc_01_asme1_rd.stp");
const CTC02: &[u8] = include_bytes!("fixtures/nist_ctc_02_asme1_rc.stp");
const CTC03: &[u8] = include_bytes!("fixtures/nist_ctc_03_asme1_rc.stp");
const CTC04: &[u8] = include_bytes!("fixtures/nist_ctc_04_asme1_rd.stp");
const CTC05: &[u8] = include_bytes!("fixtures/nist_ctc_05_asme1_rd.stp");

/// The goal's provisional STEP budget: parse + tessellate a ≤ 50 MB file in ≤ 30 s.
/// Measured here on the 0.75 MB CTC-04 (largest vendored fixture); the 50 MB ceiling
/// itself has no fixture — recorded as such in docs/verification/V14.md.
const STEP_BUDGET_MS: u64 = 30_000;

fn opts<'a>(src: &'a str) -> ImportOpts<'a> {
    ImportOpts {
        units: None,
        up: None,
        voxel: None,
        src,
        taken: &[],
        step_chord: None,
    }
}

/// The chain as the user's pipeline sees it: import → weld → voxelize, patch labelling
/// present, no open edges, units and provenance from the file, tessellated mesh volume vs
/// the B-rep's analytic flux volume within 1 %, and the parse/tessellate budget.
#[test]
fn analytic_nist_fixtures_import_voxelize_and_report_within_the_goal_bounds() {
    for (src, bytes, unit, analytic) in [
        ("nist_ctc_01_asme1_rd.stp", CTC01, "MILLIMETRE", true),
        ("nist_ctc_02_asme1_rc.stp", CTC02, "MILLIMETRE", false),
        ("nist_ctc_03_asme1_rc.stp", CTC03, "INCH", true),
        ("nist_ctc_04_asme1_rd.stp", CTC04, "MILLIMETRE", true),
        ("nist_ctc_05_asme1_rd.stp", CTC05, "INCH", false),
    ] {
        let out = import_mesh("step", bytes, &opts(src)).unwrap();
        let p = &out.parts[0];
        assert_eq!(p.open_edges, 0, "{src}: every triangle side pairs");
        assert!(p.patches > 0, "{src}: the weld+segment chain labels faces");
        let report = out.step.as_ref().expect("STEP provenance");
        assert_eq!(report.solids, 1, "{src}: one solid");
        assert_eq!(
            report.length_unit, unit,
            "{src}: units read from the file's global unit context"
        );
        assert!(
            report.refusals.is_empty(),
            "{src}: fully analytic, nothing refused: {:?}",
            report.refusals
        );
        let flux = p.step.as_ref().unwrap().volume;
        // The whole-chain tessellation and the direct entry point agree.
        let direct = tessellate(bytes, None).unwrap();
        let dp = &direct.parts[0];
        assert_eq!(
            dp.tris, p.tris,
            "{src}: import_mesh tessellates identically"
        );
        // The imported geometry's volume = signed divergence over the tessellated mesh;
        // the reference = the B-rep's analytic flux volume over the sampled boundary.
        // A B-spline face contributes no flux potential, so on the two fixtures carrying
        // them the flux reference is absent outright and the 1 % row does not bind: the
        // divergence stands alone in the report (docs/verification/V14.md).
        let mv = dp.mesh.volume();
        println!(
            "{src}: {} tris, {} welded verts, {} patches, unit {}, chord {} mm, parse {} ms + tessellate {} ms",
            p.tris, p.verts, p.patches, report.length_unit, report.chord,
            report.parse_ms, report.tess_ms
        );
        if analytic {
            let flux = flux.expect("analytic faces carry the flux potential");
            println!(
                "{src}: analytic flux volume {flux:.3} mm³, tessellated mesh divergence {mv:.3} mm³ (Δ {:.3} %)",
                (mv - flux).abs() / flux * 100.0
            );
            assert!(
                (mv - flux).abs() / flux < 0.01,
                "{src}: mesh divergence {mv:.3} vs analytic flux {flux:.3}, bound 1 %"
            );
        } else {
            println!(
                "{src}: B-spline faces carry no flux potential; tessellated mesh divergence {mv:.3} mm³ (flux row not binding)"
            );
        }
        // Whole-chain sanity: voxelize + hydrate included (the read+voxelize budget for
        // mesh imports is 5 s; CTC-04 at 542k grid cells lands well under).
        assert!(out.ms <= 5_000, "{src}: whole import took {} ms", out.ms);
        assert!(
            report.parse_ms + report.tess_ms <= STEP_BUDGET_MS,
            "{src}: parse {} ms + tessellate {} ms within the provisional {} ms budget",
            report.parse_ms,
            report.tess_ms,
            STEP_BUDGET_MS
        );
    }
}

/// Cone-face coverage. The report binds three named faces: CTC-01's b-rep faces 73 and 75
/// (its two cones; the file's labels are empty, so file order is the identifier), plus
/// CTC-04's first cone. Every cone on both fixtures must hold the same 85 % bound.
#[test]
fn cone_faces_carry_at_least_85_percent_surface_coverage() {
    for (src, bytes, named_faces) in [
        ("nist_ctc_01_asme1_rd.stp", CTC01, [73usize, 75].as_slice()),
        ("nist_ctc_02_asme1_rc.stp", CTC02, [].as_slice()),
        ("nist_ctc_04_asme1_rd.stp", CTC04, [].as_slice()),
        ("nist_ctc_05_asme1_rd.stp", CTC05, [].as_slice()),
    ] {
        let doc = tessellate(bytes, None).unwrap();
        let cones: Vec<(usize, &fieldsmith_core::step::FaceStat)> = doc.parts[0]
            .face_stats
            .iter()
            .enumerate()
            .filter(|(_, fs)| fs.surf.to_uppercase().contains("CONE"))
            .collect();
        for &(i, fs) in &cones {
            let cov = fs.mesh_area / fs.quad_area;
            assert!(
                cov >= 0.85,
                "{src} b-rep face {i} ({}): mesh area {:.3} vs quad area {:.3} = {cov:.4}",
                fs.surf,
                fs.mesh_area,
                fs.quad_area
            );
            if named_faces.contains(&i) {
                println!(
                    "{src}: named cone face {i}: tris {}, quad area {:.3}, mesh area {:.3}, coverage {cov:.4}",
                    fs.tris, fs.quad_area, fs.mesh_area
                );
            }
        }
        if let Some(&(i, fs)) = cones.first() {
            println!(
                "{src}: first cone (the report's third named face): b-rep face {i}, coverage {:.4}",
                fs.mesh_area / fs.quad_area
            );
        }
        let min = cones
            .iter()
            .map(|(_, fs)| fs.mesh_area / fs.quad_area)
            .fold(f64::INFINITY, f64::min);
        println!("{src}: {} cone faces, min coverage {min:.4}", cones.len());
        if src.contains("01") {
            assert_eq!(
                cones.iter().map(|&(i, _)| i).collect::<Vec<_>>(),
                vec![73, 75],
                "{src}: the report's two named cone faces"
            );
        } else {
            assert!(!cones.is_empty(), "{src}: analytic fixture carries cones");
        }
    }
    // CTC-03 has no cones (the cone row does not apply); its cylinder coverages go out for
    // the report — the quad-area metric over-reports coverage (cov > 1) there, noted in
    // docs/verification/V14.md as a metric artifact, not a surface defect.
    let doc = tessellate(CTC03, None).unwrap();
    let covs: Vec<f64> = doc.parts[0]
        .face_stats
        .iter()
        .filter(|fs| fs.surf.to_uppercase().contains("CYLINDER"))
        .map(|fs| fs.mesh_area / fs.quad_area)
        .collect();
    println!(
        "nist_ctc_03_asme1_rc.stp: {} cylinder faces, coverage {:.4}..{:.4} (metric artifact: > 1)",
        covs.len(),
        covs.iter().cloned().fold(f64::INFINITY, f64::min),
        covs.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    );
}

/// The tessellated-mesh divergence across the chord ladder: the default chord (scene
/// diagonal × 3e-4) plus finer rungs. This is the report's convergence evidence, not a
/// CI case — the finest rungs mesh hundreds of thousands of triangles. Run with:
///   cargo test -p fieldsmith-verify --test v14_step chord_ladder -- --ignored --nocapture
/// Refusals are part of the record: a rung the tessellator refuses (CTC-03's
/// bound-containment at 0.03; CTC-04's torus triangle cap at 0.01) is evidence about the
/// chord tolerance's working range on these files, named in docs/verification/V14.md.
#[test]
#[ignore]
fn chord_ladder() {
    for (src, bytes) in [
        ("nist_ctc_01_asme1_rd.stp", CTC01),
        ("nist_ctc_03_asme1_rc.stp", CTC03),
        ("nist_ctc_04_asme1_rd.stp", CTC04),
    ] {
        let base = tessellate(bytes, None).unwrap();
        let p0 = &base.parts[0];
        let analytic = p0.volume.unwrap();
        println!("{src}: flux {analytic:.3}");
        for chord in [None, Some(0.1), Some(0.03), Some(0.01)] {
            match tessellate(bytes, chord) {
                Ok(doc) => {
                    let p = &doc.parts[0];
                    let mv = p.mesh.volume();
                    println!(
                        "{src}: chord {chord:?} tris {} mesh {mv:.3} Δ {:.3} %",
                        p.tris,
                        (mv - analytic).abs() / analytic * 100.0
                    );
                }
                Err(e) => println!("{src}: chord {chord:?} REFUSED: {e}"),
            }
        }
    }
}

/// Re-export: the tessellated part writes through our 3MF writer and re-imports through the
/// real chain watertight, triangle count and bounds intact. (V12 covers the multi-volume
/// 3MF build-item path; V10 covers the .fsmz sidecar mechanism shared with mesh imports.)
#[test]
fn tessellated_parts_reexport_to_3mf_and_roundtrip_watertight() {
    for (src, bytes) in [
        ("nist_ctc_01_asme1_rd", CTC01),
        ("nist_ctc_04_asme1_rd", CTC04),
    ] {
        let doc = tessellate(bytes, None).unwrap();
        let p = &doc.parts[0];
        let reexport = p.mesh.write_3mf(src, "petg");
        let out = import_mesh("3mf", &reexport, &opts("reexport.3mf")).unwrap();
        let p2 = &out.parts[0];
        assert_eq!(
            p2.open_edges, 0,
            "{src}: the 3MF re-export re-imports watertight"
        );
        assert_eq!(
            p2.tris as usize,
            p.mesh.tris.len(),
            "{src}: triangle count survives the round trip"
        );
        // The writer emits 6-decimal coordinates: bounds survive to that precision.
        // Absolute bounds do not: the writer translates each volume into the positive
        // octant (3MF requires it), per volume — so the round trip is compared on
        // extents (max − min), matching the V8/V12 convention.
        let mut got_min = [f64::INFINITY; 3];
        let mut got_max = [f64::NEG_INFINITY; 3];
        for v in &p.mesh.verts {
            for d in 0..3 {
                got_min[d] = got_min[d].min(v[d]);
                got_max[d] = got_max[d].max(v[d]);
            }
        }
        for d in 0..3 {
            let want = got_max[d] - got_min[d];
            let got = p2.bbox[3 + d] - p2.bbox[d];
            assert!(
                (got - want).abs() < 1e-3 * want.abs().max(1.0),
                "{src}: axis {d} extent {got} vs {want} after the round trip"
            );
        }
        println!(
            "{src}: 3MF re-export {} bytes, re-import {} tris, {} welded verts, {} open edges",
            reexport.len(),
            p2.tris,
            p2.verts,
            p2.open_edges
        );
    }
}
