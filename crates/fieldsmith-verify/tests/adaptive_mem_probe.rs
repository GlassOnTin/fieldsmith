//! Memory probe for the wasm32 ceiling question the browser 0.1 mm export hit (the Make
//! export aborts with a bare `unreachable` trap in Chrome both headless and headed, while
//! 0.25 passes; the native budget probe ran inside a machine with RAM orders of magnitude
//! above Chrome's 4 GB wasm32 linear-memory limit, so it could not see this). Repeats the
//! export path's exact stage sequence natively and reports cumulative peak RSS plus the
//! per-stage delta of that high-water mark, so the stages that push the peak together are
//! visible. Ignored: heavy (release). Run with
//!   cargo test -p fieldsmith-verify --release --test adaptive_mem_probe -- --ignored --nocapture

use fieldsmith_core::Resolution;
use fieldsmith_core::fill::graded;
use fieldsmith_field::{expr, volume, V3};
use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
use fieldsmith_verify::example;
use std::time::Instant;

/// GOAL.md budget resolution (mm); 0.25 is also measured for the passing control.
const H_BUDGET: f64 = 0.1;
const H_CTRL: f64 = 0.25;
/// The field reference spacing measured in v8_probe.rs.
const FIELD_H: f64 = 0.094;

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Current RSS and the cumulative high-water mark (VmHWM), the process peak that matches
/// what a wasm32 page would die on (modulo allocator overhead differences).
fn rss_hwm() -> (f64, f64) {
    let s = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let mut rss = 0.0;
    let mut hwm = 0.0;
    for line in s.lines() {
        if let Some(u) = line.strip_prefix("VmRSS:") {
            rss = u.split_whitespace().next().unwrap().parse().unwrap();
        }
        if let Some(u) = line.strip_prefix("VmHWM:") {
            hwm = u.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    (rss * 1024.0, hwm * 1024.0)
}

/// Peak RSS added across one stage: record VmHWM before the stage, after it, report the
/// delta — with sequential stages, a positive delta is the stage's contribution to the peak.
struct Stage {
    hwm: f64,
    mark: String,
}

fn stage_open(mark: &str) -> Stage {
    let hwm = rss_hwm().1;
    println!("  {:<28} rss now {:.0} MiB", mark, rss_hwm().0 / 1048576.0);
    Stage { hwm, mark: mark.to_string() }
}

impl Stage {
    fn close(self) -> f64 {
        let h = rss_hwm().1;
        let d = (h - self.hwm) / 1048576.0;
        println!(
            "  {:<28} peak +{:0.0} MiB (hwm {:.0} MiB)",
            self.mark, d, h / 1048576.0
        );
        d
    }
}

#[test]
#[ignore]
fn adaptive_bracket_export_memory() {
    let doc = example("bracket-filled.fsm");
    let st = stage_open("graded build");
    let t0 = Instant::now();
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).unwrap();
    let build_s = t0.elapsed().as_secs_f64();
    st.close();
    let filled = expr::Expr::Filled(Box::new(g.fill));
    let bounds = expr::bounds(&filled);

    let st = stage_open("field reference");
    let t0 = Instant::now();
    let v_field = volume::integrate(&filled, FIELD_H, None);
    let ref_s = t0.elapsed().as_secs_f64();
    st.close();

    let field = |p: V3| -> Sample {
        let d = expr::eval(&filled, p);
        Sample { v: d.v, g: d.g }
    };

    for &(h, tag) in &[(H_CTRL, "control"), (H_BUDGET, "budget")] {
        let lo = [bounds.lo[0] - h, bounds.lo[1] - h, bounds.lo[2] - h];
        let hi = [bounds.hi[0] + h, bounds.hi[1] + h, bounds.hi[2] + h];
        let t0 = Instant::now();
        let st = stage_open(&format!("[{}] mesh h {}", tag, h));
        let mesh = surface_adaptive(field, lo, hi, h, &AdaptiveOpts::default());
        let mesh_s = t0.elapsed().as_secs_f64();
        st.close();

        let t0 = Instant::now();
        let st = stage_open(&format!("[{}] defect scan", tag));
        // The scan is 100 s at 0.1 and adds nothing to the peak; when only the mesh stage is
        // under study (FIELDSMITH_PROBE_NO_SCAN set) skip it — the digest below pins the mesh
        // bytes, so identical digests imply identical defect counts.
        let d = if std::env::var_os("FIELDSMITH_PROBE_NO_SCAN").is_some() {
            None
        } else {
            Some(mesh.defects())
        };
        let scan_s = t0.elapsed().as_secs_f64();
        st.close();

        let t0 = Instant::now();
        let st = stage_open(&format!("[{}] 3MF write", tag));
        let bytes = mesh.write_3mf(&doc.name, &doc.material);
        let write_s = t0.elapsed().as_secs_f64();
        st.close();

        let (rss, hwm) = rss_hwm();
        println!(
            "  [{}] h {}: {} verts {} tris; mesh {:.1} s, scan {:.1} s, 3MF {:.1} s; package {:.0} MiB",
            tag,
            h,
            mesh.verts.len(),
            mesh.tris.len(),
            mesh_s,
            scan_s,
            write_s,
            bytes.len() as f64 / 1048576.0
        );
        println!("  [{}] mesh digest {:016x} (fnv-1a over vertex f64 bits, then triangle ids)", tag, digest(&mesh));
        println!(
            "  [{}] end of export path: rss {:.0} MiB (mesh+field resident), hwm {:.0} MiB; field ref {:.1} mm³ ({ref_s:.1} s, build {build_s:.1} s), defects {d:?}",
            tag, rss / 1048576.0, hwm / 1048576.0, v_field
        );
        drop(bytes);
    }
    println!(
        "  stage peak contributions above are VmHWM deltas: native peak that the wasm32 page must hold inside its 4 GB (Chrome) / 2 GB (Firefox) linear-memory limit."
    );
}

/// FNV-1a over every vertex coordinate's f64 bits, then every triangle's ids: equal digests
/// on the same input mean byte-identical meshes (to FNV's collision odds).
fn digest(m: &fieldsmith_mesh::export::TriMesh) -> u64 {
    let mut x: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |w: u64| {
        for b in w.to_le_bytes() {
            x ^= b as u64;
            x = x.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for v in &m.verts {
        for c in v {
            eat(c.to_bits());
        }
    }
    for t in &m.tris {
        for &c in t {
            eat(c as u64);
        }
    }
    x
}