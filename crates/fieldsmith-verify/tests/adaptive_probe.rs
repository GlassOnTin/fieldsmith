//! Budget probe for the adaptive dual-contouring export of the graded bracket (GOAL.md line
//! "Export 3MF of graded bracket at 0.1 mm feature resolution <= 15 s"). The uniform mesher
//! measured 411 s at h = 0.1 natively (docs/verification/V8.md); this probe times the
//! adaptive surface, the defect scan (kept out of the budget number: it is verification, not
//! export) and the 3MF write, at the Make-lens spacing 0.25 and the budget resolution 0.1.
//! Ignored: heavy (release). Run with
//!   cargo test -p fieldsmith-verify --release --test adaptive_probe -- --ignored --nocapture

use fieldsmith_core::Resolution;
use fieldsmith_core::fill::graded;
use fieldsmith_field::{expr, volume, V3};
use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
use fieldsmith_verify::example;
use std::time::Instant;

/// The Make lens's default export spacing (mm) — control resolution.
const H_CTRL: f64 = 0.25;
/// GOAL.md budget resolution (mm).
const H_BUDGET: f64 = 0.1;
/// GOAL.md: export <= 15 s.
const BUDGET_S: f64 = 15.0;
/// GOAL.md: mesh volume within 1 % of the field reference.
const VOL_TOL: f64 = 0.01;
/// The field reference spacing measured in v8_probe.rs (fine ladder, Richardson error).
const FIELD_H: f64 = 0.094;

/// Same seconds clock the V8 test hands to `graded`.
fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

#[test]
#[ignore]
fn adaptive_bracket_export_budget() {
    let t0 = Instant::now();
    let doc = example("bracket-filled.fsm");
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).unwrap();
    let build_s = t0.elapsed().as_secs_f64();
    let filled = expr::Expr::Filled(Box::new(g.fill));
    let bounds = expr::bounds(&filled);
    println!("  graded build {:.1} s", build_s);

    let t = Instant::now();
    let v_field = volume::integrate(&filled, FIELD_H, None);
    let i_s = t.elapsed().as_secs_f64();
    println!("  field reference {:.1} mm³ at h {FIELD_H} ({i_s:.1} s)", v_field);

    let field = |p: V3| -> Sample {
        let d = expr::eval(&filled, p);
        Sample { v: d.v, g: d.g }
    };

    for &(h, tag) in &[(H_CTRL, "control"), (H_BUDGET, "budget")] {
        let lo = [bounds.lo[0] - h, bounds.lo[1] - h, bounds.lo[2] - h];
        let hi = [bounds.hi[0] + h, bounds.hi[1] + h, bounds.hi[2] + h];
        let t = Instant::now();
        let mesh = surface_adaptive(field, lo, hi, h, &AdaptiveOpts::default());
        let mesh_s = t.elapsed().as_secs_f64();
        let v_mesh = mesh.volume();
        let t = Instant::now();
        let d = mesh.defects();
        let scan_s = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let bytes = mesh.write_3mf(&doc.name, &doc.material);
        let write_s = t.elapsed().as_secs_f64();
        let dir = std::env::temp_dir().join("fieldsmith-probe");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("bracket-adaptive-{}.3mf", h));
        let _ = std::fs::write(&path, &bytes);
        let pct = 100.0 * (v_mesh - v_field) / v_field;
        println!(
            "  [{}] h {:.2}: {} verts {} tris | mesh {:.1} s, defect scan {:.1} s, 3MF {:.1} s ({:.1} MiB -> {})",
            tag,
            h,
            mesh.verts.len(),
            mesh.tris.len(),
            mesh_s,
            scan_s,
            write_s,
            bytes.len() as f64 / (1024.0 * 1024.0),
            path.display()
        );
        println!("  [{}] volume {:.1} mm³ ({pct:+.3} % vs field), defects {:?}", tag, v_mesh, d);
        if h == H_BUDGET {
            let total = mesh_s + write_s;
            println!(
                "  [{}] budget: mesh + 3MF = {:.1} s vs <= {BUDGET_S} s -> {}",
                tag,
                total,
                if total <= BUDGET_S { "PASS" } else { "FAIL" }
            );
            assert!(total <= BUDGET_S, "0.1 mm budget: mesh+3MF {:.1} s > {BUDGET_S} s", total);
        }
        assert!(
            pct.abs() <= 100.0 * VOL_TOL,
            "[{}] volume off the field reference by {pct:.3} %",
            tag
        );
    }
}