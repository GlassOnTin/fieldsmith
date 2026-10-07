//! V8 probe 5 — where the export's memory actually goes, phase by phase.
//!
//! Probe 4's VmHWM is cumulative across the three exports and does not move when the big
//! position table is removed, so this probe prints the LIVE RSS after every phase of a
//! single h = 0.25 export (lattice size, mesh, volume, defects, STL bytes), plus the
//! lattice node count, so the dominant resident set is measured rather than guessed.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v8_probe5`

use fieldsmith_doc::fsm;
use fieldsmith_field::{expr, V3};
use fieldsmith_mesh::export::surface;
use fieldsmith_mesh::Lattice;

fn rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("VmRSS:") {
            return v.split_whitespace().next().unwrap().parse::<f64>().unwrap() / 1024.0;
        }
    }
    0.0
}

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
    let h = 0.25_f64;
    let b = expr::bounds(&body);
    let lo = [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h];
    let hi = [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h];
    let lat = Lattice::fit(lo, hi, h);
    println!(
        "lattice n={:?}, nodes={} (pos table would be {:.0} MiB at 24 B/node)",
        lat.n,
        lat.nverts(),
        lat.nverts() as f64 * 24.0 / 1048576.0
    );
    println!("before mesh: {:.0} MiB", rss_mib());

    let t = now();
    let mesh = surface(|p: V3| expr::eval(&body, p).v, lo, hi, h);
    println!(
        "mesh {} verts {} tris in {:.1} s, after mesh: {:.0} MiB",
        mesh.verts.len(),
        mesh.tris.len(),
        (now() - t) / 1000.0,
        rss_mib()
    );

    let _v = mesh.volume();
    println!("after volume: {:.0} MiB", rss_mib());

    let _d = mesh.defects();
    println!("after defects: {:.0} MiB", rss_mib());

    let bytes = mesh.write_3mf(&doc.name, &doc.material);
    println!(
        "after 3MF ({:.0} MiB): {:.0} MiB",
        bytes.len() as f64 / 1048576.0,
        rss_mib()
    );
    let stl = mesh.write_stl();
    println!(
        "after STL ({:.1} MiB): {:.0} MiB",
        stl.len() as f64 / 1048576.0,
        rss_mib()
    );
}
