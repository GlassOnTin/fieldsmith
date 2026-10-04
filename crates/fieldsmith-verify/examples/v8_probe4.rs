//! V8 probe 4 — peak native memory of one export, the wasm32 ceiling check.
//!
//! The browser build traps (`unreachable`) during `exportSolid` at h ≤ 0.3 while the same
//! call succeeds natively. This probe measures the native peak RSS of exactly the export
//! path (`fieldsmith-wasm::exportSolid`'s sequence: compile body → surface → volume →
//! defects → STL bytes), so the wasm trap can be pinned on memory rather than codegen:
//! if the native peak exceeds the 3.9 GiB linear-memory ceiling, that is the whole story.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v8_probe4`

use fieldsmith_doc::fsm;
use fieldsmith_field::{expr, V3};
use fieldsmith_mesh::export::surface;

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Peak RSS in MiB since process start, from VmHWM.
fn peak_rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("VmHWM:") {
            return v.split_whitespace().next().unwrap().parse::<f64>().unwrap() / 1024.0;
        }
    }
    0.0
}

struct Rss;
impl Drop for Rss {
    fn drop(&mut self) {
        println!("  peak RSS {:.0} MiB", peak_rss_mib());
    }
}

fn main() {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/bracket.fsm")).unwrap();
    let doc = fsm::parse(&text).unwrap();
    let body = expr::compile_body(&doc);

    for h in [0.4_f64, 0.3, 0.25] {
        println!("solid export h={h}:");
        let _g = Rss;
        let b = expr::bounds(&body);
        let _lo = [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h];
        let _hi = [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h];
        let t = now();
        let mesh = surface(
            |p: V3| expr::eval(&body, p).v,
            _lo,
            _hi,
            h,
        );
        println!("  mesh {} verts {} tris in {:.1} s", mesh.verts.len(), mesh.tris.len(), (now() - t) / 1000.0);
        let _v = mesh.volume();
        let d = mesh.defects();
        let _stl = mesh.write_stl();
        println!("  stl {:.1} MiB, defects {:?}", _stl.len() >> 20, d);
    }
}