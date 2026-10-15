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

/// M20 regression: a mesh-leaf body (1:10 scaled CTC-01) with positive-skin graded gyroid
/// fill (`skin = 1.2 mm`) exports through adaptive dual contouring with zero open edges,
/// zero degenerate triangles, zero orientation defects, bounded non-manifold spokes, and
/// mesh volume within 1 % of the field integral (locking out both the f32 corner-crossing
/// degeneracy class and the per-component cavity-inversion volume jump).
#[test]
fn graded_mesh_leaf_with_positive_skin_exports_closed_and_within_volume_bound() {
    use fieldsmith_core::fill::graded;
    use fieldsmith_core::Resolution;
    use fieldsmith_doc::{fsm, Document};
    use fieldsmith_field::{expr, volume};
    use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
    use std::time::Instant;

    fn clock() -> f64 {
        thread_local!(static T0: Instant = Instant::now());
        T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
    }

    let step = tessellate(CTC01, None).unwrap();
    let mut scaled = step.parts[0].mesh.clone();
    for v in &mut scaled.verts {
        *v = [v[0] * 0.1, v[1] * 0.1, v[2] * 0.1];
    }
    let stl = scaled.write_stl();
    let out = import_mesh("stl", &stl, &opts("c01-scaled.stl")).unwrap();
    let base = Document {
        name: "c01-scaled".into(),
        material: "petg".into(),
        root: out.nodes,
        ..Default::default()
    };
    let text = format!(
        "{}\nregion mount = mesh1.nz\nregion work = mesh1.py\nsupport fixed mount\nload press work force=(103.53, 0, 386.37) case=\"Press 15 deg\"\nfill gyroid sheet cell=6 skin=1.2 rho=(0.2, 0.8) curve=1 wall=0.8 margin=2\n",
        fsm::print(&base)
    );
    let doc = fsm::parse(&text).expect("graded mesh-leaf document parses");
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).expect("graded solve");
    let filled = expr::Expr::Filled(Box::new(g.fill));
    let b = expr::bounds(&filled);

    for h in [0.30, 0.25] {
        let lo = [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h];
        let hi = [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h];
        let m = surface_adaptive(
            |p| {
                let d = expr::eval(&filled, p);
                Sample { v: d.v, g: d.g }
            },
            lo,
            hi,
            h,
            &AdaptiveOpts::default(),
        );
        let vf = volume::volume(&filled, h).volume;
        let vm = m.volume();
        let rel_vol = (vm - vf).abs() / vf;
        let d = m.defects();
        println!(
            "c01-scaled skin=1.2 h={h}: {} verts, {} tris, field {vf:.1} mm³, mesh {vm:.1} mm³ (Δ {:+.2} %), defects {d:?}",
            m.verts.len(),
            m.tris.len(),
            (vm - vf) / vf * 100.0
        );
        assert_eq!(d.open_edges, 0, "h={h}: open edges must be zero: {d:?}");
        assert_eq!(
            d.orientation_defects, 0,
            "h={h}: orientation defects must be zero: {d:?}"
        );
        assert_eq!(
            d.degenerate_triangles, 0,
            "h={h}: degenerate triangles must be zero: {d:?}"
        );
        assert!(
            d.non_manifold_edges <= 15,
            "h={h}: non-manifold spokes {d:?} exceeded bound"
        );
        assert!(
            rel_vol < 0.01,
            "h={h}: mesh volume {vm:.1} vs field {vf:.1} ({:.2} % > 1 %)",
            rel_vol * 100.0
        );
    }
}

