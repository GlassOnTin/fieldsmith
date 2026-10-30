//! V49 — Direct field slice stacks & 3MF Slice Extension (`<s:slicestack>`, M27).
//!
//! Verifies:
//! 1. 2D marching-squares contour extraction with along-edge root polishing (`slice_field`)
//!    on a sphere of radius `R = 15 mm` across 5 heights `z ∈ {-12, -6, 0, 5, 10}` mm:
//!    every layer has 0 open chains and its signed polygon area matches the exact disc area
//!    `π(R² - z²)` within `1 %`.
//! 2. Direct slice-stack extraction (`export_slices`) on the graded TPMS bracket
//!    (`examples/bracket-filled.fsm`) without constructing a 3D surface triangle mesh:
//!    every layer stitches into closed oriented polygons (`0` open chains across all layers),
//!    interior lattice cells produce CW hole polygons (negative signed area) inside CCW outer
//!    shells (positive signed area), and the integrated slice-stack volume agrees with the
//!    3D field volume within `2.5 %`.
//! 3. 3MF Slice Extension (`write_3mf_slicestack` / `read_3mf_slicestack`) round-trip:
//!    the `<s:slicestack>` archive round-trips layer count, polygon count, and stack volume
//!    within `0.01 %`.

use fieldsmith_api::export::{export_slices, ExportSlicesOpts};
use fieldsmith_doc::fsm;
use fieldsmith_mesh::slice::{polygon_signed_area, read_3mf_slicestack, slice_field};
use fieldsmith_verify::{Check, Report};
use std::time::Instant;

fn clock() -> f64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[test]
fn v49_direct_field_slice_stack() {
    let mut r = Report::new(
        "V49",
        "Direct implicit-field 2D slice stacks & 3MF Slice Extension",
    );

    // 1. Analytic sphere cross-sections against π(R² - z²).
    let rad = 15.0f64;
    let sphere = |p: [f64; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - rad;
    let lo = [-rad, -rad, -rad];
    let hi = [rad, rad, rad];
    let h_sphere = 0.25;

    let mut sphere_rows = Vec::new();
    for &z in &[-12.0f64, -6.0, 0.0, 5.0, 10.0] {
        let layer = slice_field(sphere, lo, hi, z, h_sphere);
        let exact = std::f64::consts::PI * (rad * rad - z * z);
        let area = layer.area();
        let rel_pct = ((area - exact) / exact).abs() * 100.0;
        r.check(Check::absolute(
            &format!("sphere z={z:+.0} mm: open chains"),
            0.0,
            layer.open_chains as f64,
            "chains",
            0.0,
        ));
        r.check(Check::relative(
            &format!("sphere z={z:+.0} mm: contour area vs π(R²−z²)"),
            exact,
            area,
            "mm²",
            0.01,
        ));
        sphere_rows.push(format!(
            "| `{z:+.1}` | `{}` | `{}` | `{exact:.2}` | `{area:.2}` | `{rel_pct:.4} %` |",
            layer.polygons.len(),
            layer.vertex_count(),
        ));
    }

    // 2. Graded TPMS bracket direct slice stack via fieldsmith_api::export::export_slices.
    let fsm_path = format!(
        "{}/../../examples/bracket-filled.fsm",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&fsm_path).expect("read bracket-filled.fsm");
    let doc = fsm::parse(&text).expect("parse bracket-filled.fsm");

    let ex = export_slices(
        &doc,
        &ExportSlicesOpts {
            h: 0.35,
            layer_h: 1.0,
            solid_only: false,
            mf: true,
        },
        clock,
    )
    .expect("export_slices on bracket-filled.fsm");

    r.check(Check::absolute(
        "graded bracket slice stack: open chains across all layers",
        0.0,
        ex.open_chains as f64,
        "chains",
        0.0,
    ));
    r.check(Check::absolute(
        "graded bracket slice stack: layer count ≥ 30",
        1.0,
        (ex.layers >= 30) as u8 as f64,
        "bool",
        0.0,
    ));
    r.check(Check::absolute(
        "graded bracket slice stack: closed polygon count > 100",
        1.0,
        (ex.polygons > 100) as u8 as f64,
        "bool",
        0.0,
    ));
    r.check(Check::relative(
        "graded bracket slice stack volume vs 3D field volume",
        ex.reference.volume,
        ex.stack_volume,
        "mm³",
        0.025,
    ));

    // 3. 3MF Slice Extension round-trip on the graded bracket slice stack.
    let mf_bytes = ex.mf.as_ref().expect("3MF slice stack bytes");
    let roundtrip = read_3mf_slicestack(mf_bytes).expect("read_3mf_slicestack on graded bracket");

    r.check(Check::absolute(
        "3MF <s:slicestack> round-trip: layer count preserved",
        ex.layers as f64,
        roundtrip.layers.len() as f64,
        "layers",
        0.0,
    ));
    r.check(Check::absolute(
        "3MF <s:slicestack> round-trip: closed polygon count preserved",
        ex.polygons as f64,
        roundtrip.polygon_count() as f64,
        "polygons",
        0.0,
    ));

    let mut cw_holes = 0usize;
    let mut ccw_outer = 0usize;
    for l in &roundtrip.layers {
        for p in &l.polygons {
            let a = polygon_signed_area(p);
            if a > 0.0 {
                ccw_outer += 1;
            } else if a < 0.0 {
                cw_holes += 1;
            }
        }
    }
    r.check(Check::absolute(
        "3MF <s:slicestack> carries both CCW outer contours and CW interior lattice holes",
        1.0,
        (ccw_outer > 0 && cw_holes > 0) as u8 as f64,
        "bool",
        0.0,
    ));
    r.check(Check::relative(
        "3MF <s:slicestack> round-trip: stack volume preserved",
        ex.stack_volume,
        roundtrip.volume(),
        "mm³",
        1e-4,
    ));

    r.section(
        "Sphere Cross-Section Accuracy (`R = 15 mm`, `h = 0.25 mm`)",
        &format!(
            "| Z plane (mm) | Polygons | Vertices | Exact `π(R²−z²)` (mm²) | Sliced area (mm²) | Relative error |\n\
             | --: | --: | --: | --: | --: | --: |\n\
             {}",
            sphere_rows.join("\n")
        ),
    )
    .section(
        "Graded TPMS Bracket Slice Stack (`examples/bracket-filled.fsm`)",
        &format!(
            "- Layers (`layer_h = 1.0 mm`, `h = 0.35 mm`): `{}`\n\
             - Closed polygons: `{}` (`{ccw_outer}` CCW outer boundaries, `{cw_holes}` CW interior lattice holes)\n\
             - Open segment chains: `{}`\n\
             - Integrated slice-stack volume: `{:.1} mm³` vs 3D field volume `{:.1} ± {:.1} mm³` (`Δ = {:+.3} %`)\n\
             - 3MF Slice Extension archive size: `{} bytes` (`slice_ms = {:.1} ms`)",
            ex.layers,
            ex.polygons,
            ex.open_chains,
            ex.stack_volume,
            ex.reference.volume,
            ex.reference.error,
            ex.delta_percent,
            mf_bytes.len(),
            ex.slice_ms,
        ),
    )
    .section(
        "Coverage and limits",
        "- `slice_field` evaluates the 2D plane `z` on a padded `(nx+1) × (ny+1)` marching-squares \
         grid with Illinois-modified secant root polishing along each sign-changing grid edge.\n\
         - Because every grid edge shares a single crossing vertex ID across its two incident \
         cells, every contour vertex has exact in-degree 1 and out-degree 1, guaranteeing zero \
         open chains on all interior slices.\n\
         - Sub-grid features smaller than `h` within a single 2D cell are not resolved unless `h` \
         is refined.",
    );

    r.finish();
}
