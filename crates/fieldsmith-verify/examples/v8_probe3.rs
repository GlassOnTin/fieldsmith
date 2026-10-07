//! V8 probe 3 — the solid bracket's export mesh: are its defects zero?
//!
//! Probe 1 measured the FILLED bracket (2277/2197/547 non-manifold edges by h). To know
//! whether the defect source is the TPMS fill or the mesher itself, this exports the SOLID
//! body (no fill) and reports the same numbers over the same h sweep.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v8_probe3`

use fieldsmith_doc::fsm;
use fieldsmith_field::{expr, V3};
use fieldsmith_mesh::export::surface;

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

fn main() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bracket.fsm"
    ))
    .unwrap();
    let doc = fsm::parse(&text).unwrap();
    let body = expr::compile_body(&doc);
    let b = expr::bounds(&body);
    println!(
        "field volume (auto): {:.1} mm³",
        fieldsmith_field::volume::volume_auto(&body).volume
    );

    for h in [0.25_f64, 0.15, 0.1] {
        print!("mesh h={h}: ");
        let t = now();
        let mesh = surface(
            |p: V3| expr::eval(&body, p).v,
            [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
            [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
            h,
        );
        println!(
            "{} verts {} tris in {:.1} s",
            mesh.verts.len(),
            mesh.tris.len(),
            (now() - t) / 1000.0
        );
        let d = mesh.defects();
        println!("  defects: {d:?}");
        println!("  volume {:.1} mm³", mesh.volume());
    }
}
