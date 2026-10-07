//! V8 probe — the graded bracket as an export mesh.
//!
//! Answers, with measurements:
//! 1. how the field-integrator volume converges on the filled field (the V8 reference),
//! 2. how the export mesh's volume and defect counts evolve with the mesh spacing `h`,
//! 3. where the non-manifold edges sit (skin zone or lattice core, and their midpoints).

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_doc::fsm;
use fieldsmith_field::{expr, volume, V3};
use fieldsmith_mesh::export::surface;
use std::collections::HashMap;

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// The undirected edges used by three or more boundary triangles, as midpoints (mm).
fn non_manifold_midpoints(m: &fieldsmith_mesh::export::TriMesh) -> Vec<V3> {
    let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &m.tris {
        for i in 0..3 {
            *edges.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for &(a, b) in edges.keys() {
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi || !seen.insert((lo, hi)) {
            continue;
        }
        let f = edges.get(&(lo, hi)).copied().unwrap_or(0);
        let b_ = edges.get(&(hi, lo)).copied().unwrap_or(0);
        if f + b_ >= 3 {
            let p = m.verts[lo as usize];
            let q = m.verts[hi as usize];
            out.push([
                0.5 * (p[0] + q[0]),
                0.5 * (p[1] + q[1]),
                0.5 * (p[2] + q[2]),
            ]);
        }
    }
    out
}

fn main() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bracket-filled.fsm"
    ))
    .unwrap();
    let doc = fsm::parse(&text).unwrap();
    println!("graded…");
    let t0 = now();
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), now).unwrap();
    println!(
        "  mass {:.2} g of {:.2} g solid, {:.1} s",
        g.mass_g,
        g.solid_mass_g,
        (now() - t0) / 1000.0
    );

    let filled = expr::Expr::Filled(Box::new(g.fill));
    let body = expr::compile_body(&doc);
    let b = expr::bounds(&filled);
    println!("body bounds {b:?}");

    // ---- 1) field-integrator convergence ----------------------------------------------------
    println!("\nfield volume of the filled field (fine, coarse, richardson err/3):");
    for h in [1.5, 0.75, 0.375, 0.19, 0.094] {
        let t = now();
        let fine = volume::integrate(&filled, h, None);
        let coarse = volume::integrate(&filled, 2.0 * h, None);
        let err = (fine - coarse).abs() / 3.0;
        println!(
            "  h {h:.4}: {fine:.1} mm³  (coarse {coarse:.1}, err±{err:.1})  {:.1} s",
            (now() - t) / 1000.0
        );
    }

    // ---- 2) export mesh vs field, with h ----------------------------------------------------
    let h_ref = 0.094;
    let t = now();
    let v_ref_converged = volume::integrate(&filled, h_ref, None);
    println!(
        "\nreference at h={h_ref}: {v_ref_converged:.1} mm³ ({:.1} s)",
        (now() - t) / 1000.0
    );
    for h in [0.25, 0.15, 0.1] {
        let t = now();
        let tri = surface(
            |p: V3| expr::eval(&filled, p).v,
            [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
            [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
            h,
        );
        let mesh_s = (now() - t) / 1000.0;
        let v = tri.volume();
        let t = now();
        let d = tri.defects();
        let scan_s = (now() - t) / 1000.0;
        println!(
            "\nmesh h={h:.3}: {} verts {} tris, mesh {mesh_s:.1} s, scan {scan_s:.1} s",
            tri.verts.len(),
            tri.tris.len()
        );
        println!(
            "  mesh volume {v:.1} mm³ vs field@0.094 {v_ref_converged:.1} mm³  Δ {:+.3} %",
            (v - v_ref_converged) / v_ref_converged * 100.0
        );
        println!("  defects: {d:?}");

        // ---- 3) where do the non-manifold edges sit? ---------------------------------------
        let mids = non_manifold_midpoints(&tri);
        let (mut skin_n, mut core_n, mut outside_n) = (0usize, 0usize, 0usize);
        for &p in mids.iter().take(50_000) {
            let d_body = expr::eval(&body, p);
            if d_body.v > 0.0 {
                outside_n += 1;
            } else if d_body.v > -0.7 - 2.0 * h {
                // within the outer skin shell (skin 1.2 mm from either side)
                skin_n += 1;
            } else {
                core_n += 1;
            }
        }
        println!(
            "  non-manifold edges: {} ({skin_n} in the skin shell, {core_n} in the lattice core, {outside_n} outside the body)",
            mids.len()
        );
        for &p in mids.iter().take(12) {
            println!("      ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]);
        }
    }
}
