//! Does the adaptive dual-contouring mesher stay defect-free on the plain solid bracket —
//! the model the uniform path reports zero defects of every class on (v8_probe3)? The
//! measured defect classes of the graded fill (multi-sheet cells at wall-crossing lines,
//! corner-snapped degeneracies, cross-level slits) live in the fill's wall-on-lattice
//! geometry; the plain solid has none of that, and if the adaptive mesh is to be the
//! export default it should be shown clean there too. Ignored: heavy (release). Run with
//!   cargo test -p fieldsmith-verify --release --test adaptive_plain_probe -- --ignored --nocapture

use fieldsmith_field::expr;
use fieldsmith_verify::example;
use std::time::Instant;

#[test]
#[ignore]
fn adaptive_plain_bracket_defects() {
    let t0 = Instant::now();
    let doc = example("bracket.fsm");
    let e = expr::compile_body(&doc);
    let h = 0.25;
    let bounds = expr::bounds(&e);
    let lo = [bounds.lo[0] - h, bounds.lo[1] - h, bounds.lo[2] - h];
    let hi = [bounds.hi[0] + h, bounds.hi[1] + h, bounds.hi[2] + h];
    let m = fieldsmith_mesh::adaptive::surface_adaptive(
        |p| {
            let d = expr::eval(&e, p);
            fieldsmith_mesh::adaptive::Sample { v: d.v, g: d.g }
        },
        lo,
        hi,
        h,
        &fieldsmith_mesh::adaptive::AdaptiveOpts::default(),
    );
    println!(
        "  built {} verts {} tris in {:.1} s",
        m.verts.len(),
        m.tris.len(),
        t0.elapsed().as_secs_f64()
    );
    let d = m.defects();
    println!("  defects {d:?}");
    let v = m.volume();
    println!("  mesh volume {v:.1} mm³");
}
