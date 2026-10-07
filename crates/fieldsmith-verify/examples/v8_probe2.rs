//! V8 probe 2 — are the graded fill's non-manifold edges real geometry or mesh artifacts?
//!
//! At h = 0.25 mm the export mesh of the filled bracket carries ~2277 non-manifold edges that
//! do not vanish on refinement (2197 at 0.15, 547 at 0.1). This probe classifies each
//! non-manifold edge endpoint by the field it came from, marching rays outwards from the
//! vertex (the vertex is on the zero set):
//!
//! * **touch** — no direction starts in material (d < 0 at a hair outside v): the material
//!   vanishes to a cusp at v — the field's own zero set touches itself there. A property of
//!   the field, not of the mesh.
//! * **squeezed** — every interior direction exits within ≤ 0.06 mm: fused walls with no
//!   printable interior left; treated the same as a touch for the print result.
//! * **zeroed node** — |d(v)| ≤ 1e-9 AND no interior: the isosurface passes exactly through
//!   a stuffed-lattice node (degenerate warp), a mesher-robustness case.
//! * **fused wall** — interior spans are healthy (≥ 0.06 mm in the thinnest interior
//!   direction): two walls merged into thicker material; the shared edge then comes from
//!   lattice alignment, not from the field.
//!
//! Plus chain structure: union-find over the edges shows whether pinches form continuous
//! lines or are scattered.
//!
//! Run: `cargo run -p fieldsmith-verify --release --example v8_probe2`

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_doc::fsm;
use fieldsmith_field::{expr, V3};
use fieldsmith_mesh::export::{surface, TriMesh};
use std::collections::HashMap;

fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// The undirected edges used by three or more triangles, as (a, b) vertex indices.
fn non_manifold_edges(m: &TriMesh) -> Vec<(u32, u32)> {
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
            out.push((lo, hi));
        }
    }
    out
}

fn dirs26() -> Vec<V3> {
    let mut out = Vec::new();
    for x in [-1.0_f64, 0.0, 1.0] {
        for y in [-1.0_f64, 0.0, 1.0] {
            for z in [-1.0_f64, 0.0, 1.0] {
                if x == 0.0 && y == 0.0 && z == 0.0 {
                    continue;
                }
                let l = (x * x + y * y + z * z).sqrt();
                out.push([x / l, y / l, z / l]);
            }
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
    println!("  built in {:.1} s", (now() - t0) / 1000.0);

    let filled = expr::Expr::Filled(Box::new(g.fill));
    let b = expr::bounds(&filled);
    let h = 0.25_f64;
    println!("meshing at h={h}…");
    let t = now();
    let mesh = surface(
        |p: V3| expr::eval(&filled, p).v,
        [b.lo[0] - h, b.lo[1] - h, b.lo[2] - h],
        [b.hi[0] + h, b.hi[1] + h, b.hi[2] + h],
        h,
    );
    println!(
        "  {} verts {} tris in {:.1} s",
        mesh.verts.len(),
        mesh.tris.len(),
        (now() - t) / 1000.0
    );

    let edges = non_manifold_edges(&mesh);
    println!("\n{} non-manifold edges", edges.len());

    // ---- chain structure: union-find over edges sharing a vertex --------------------------
    let mut parent: HashMap<u32, u32> = HashMap::new();
    fn find(p: &mut HashMap<u32, u32>, x: u32) -> u32 {
        let r = match p.get(&x) {
            Some(&y) if y != x => find(p, y),
            _ => x,
        };
        p.insert(x, r);
        r
    }
    for &(a, b) in &edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent.insert(ra, rb);
    }
    let mut size: HashMap<u32, usize> = HashMap::new();
    for &(a, _) in &edges {
        *size.entry(find(&mut parent, a)).or_default() += 1;
    }
    let mut sizes: Vec<usize> = size.values().copied().collect();
    sizes.sort_unstable();
    println!(
        "chains: {} components over {} edges, biggest {} (median {})",
        size.len(),
        edges.len(),
        sizes.last().copied().unwrap_or(0),
        sizes[sizes.len() / 2]
    );

    // ---- field classification of edge endpoints ------------------------------------------
    let dirs = dirs26();
    let (mut n_touch, mut n_zero, mut n_fused) = (0usize, 0usize, 0usize);
    let mut spans: Vec<f64> = Vec::new();
    let mut touched_edges = 0usize;
    let t_probe = now();
    const DELTA: f64 = 1e-3;
    for &(a, b) in edges.iter().take(4000) {
        let mut edge_touched = false;
        for v in [a, b] {
            let idx = v as usize;
            if idx >= mesh.verts.len() {
                continue;
            }
            let p = mesh.verts[idx];
            let dv = expr::eval(&filled, p).v;
            if dv > 0.0 {
                continue; // a vertex outside the material: not a pinch candidate
            }
            let mut min_span = f64::INFINITY;
            let mut interior_dirs = 0usize;
            for dir in &dirs {
                let q = [
                    p[0] + DELTA * dir[0],
                    p[1] + DELTA * dir[1],
                    p[2] + DELTA * dir[2],
                ];
                if expr::eval(&filled, q).v >= 0.0 {
                    continue; // pointing out of the material
                }
                interior_dirs += 1;
                // exit distance: march until d ≥ 0, up to 1.2 mm
                let mut t = 0.01_f64;
                let mut span = 1.2;
                while t <= 1.2 {
                    let w = expr::eval(
                        &filled,
                        [p[0] + t * dir[0], p[1] + t * dir[1], p[2] + t * dir[2]],
                    )
                    .v;
                    if w >= 0.0 {
                        span = t;
                        break;
                    }
                    t += 0.01;
                }
                if span < min_span {
                    min_span = span;
                }
            }
            if interior_dirs == 0 {
                if dv.abs() <= 1e-9 {
                    n_zero += 1;
                } else {
                    n_touch += 1;
                }
                edge_touched = true;
            } else if min_span <= 0.06 {
                n_touch += 1;
                edge_touched = true;
                spans.push(min_span);
            } else {
                n_fused += 1;
                spans.push(min_span);
            }
        }
        if edge_touched {
            touched_edges += 1;
        }
    }
    spans.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f64| {
        if spans.is_empty() {
            f64::NAN
        } else {
            spans[((q * (spans.len() - 1) as f64) as usize).min(spans.len() - 1)]
        }
    };
    println!(
        "endpoints probed in {:.1} s: {} touches, {} zeroed nodes, {} fused walls of healthy width\n  edge vertices with vanishing material: {} of {}\n  edges with a vanishing endpoint: {} of {}\n  interior spans at fused vertices: median {:.3} mm, p10 {:.3}",
        (now() - t_probe) / 1000.0,
        n_touch,
        n_zero,
        n_fused,
        n_touch + n_zero,
        n_touch + n_fused + n_zero,
        touched_edges,
        edges.len(),
        at(0.5),
        at(0.1)
    );
}
