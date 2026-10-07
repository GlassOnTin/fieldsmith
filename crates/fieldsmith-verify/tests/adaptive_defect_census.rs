//! Where do the adaptive dual-contouring surface's residual defect classes come from on the
//! graded fill? With the per-sheet vertex separation and head-of-weld chord registry, the
//! adaptive surface measures 0 open edges, 10 non-manifold edges, 0 degenerate triangles and
//! 4959 self-intersecting pairs at h = 0.25 (the uniform mesher on the same field reports
//! 2277 non-manifold, none of the others). This census dumps samples of each residual class
//! with coordinates and cross-correlates them (open endpoints vs non-manifold endpoints vs
//! crossing points), and compares the adaptive and uniform meshes over one defect region with
//! the field value at every corner. Ignored: heavy (release). Run with
//!   cargo test -p fieldsmith-verify --release --test adaptive_defect_census -- --ignored --nocapture

use fieldsmith_core::fill::graded;
use fieldsmith_core::Resolution;
use fieldsmith_field::{expr, V3};
use fieldsmith_mesh::adaptive::{surface_adaptive, AdaptiveOpts, Sample};
use fieldsmith_mesh::export::TriMesh;
use fieldsmith_verify::example;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Same seconds clock the V8 test hands to `graded`.
fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(u: V3, v: V3) -> V3 {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}
fn len(v: V3) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn dot(u: V3, v: V3) -> f64 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}
fn unit(v: V3) -> V3 {
    let l = len(v);
    if l < 1e-24 {
        [0.0; 3]
    } else {
        [v[0] / l, v[1] / l, v[2] / l]
    }
}
fn area_of(m: &TriMesh, t: [u32; 3]) -> f64 {
    let (a, b, c) = (
        m.verts[t[0] as usize],
        m.verts[t[1] as usize],
        m.verts[t[2] as usize],
    );
    0.5 * len(cross(sub(b, a), sub(c, a)))
}

fn inside_trip(t: [V3; 3], q: V3, eps: f64) -> bool {
    for i in 0..3 {
        let a = t[i];
        let (b, c) = (t[(i + 1) % 3], t[(i + 2) % 3]);
        let e = cross(sub(b, a), sub(q, a));
        let f = cross(sub(b, a), sub(c, a));
        let g = dot(e, f);
        if g >= 0.0 {
            continue;
        }
        let scale = len(e) * len(f);
        if scale > 0.0 && -g / scale > eps {
            return false;
        }
    }
    true
}

/// Segment against target triangle; returns the crossing point when it crosses. Boundary-exact
/// contacts (q on the target's edge chain) do not count: matches TriMesh::self_intersections.
fn seg_tri(m: &TriMesh, ta: &[u32; 3], tb: &[u32; 3], i: usize) -> Option<V3> {
    let b = tb.map(|v| m.verts[v as usize]);
    let n = unit(cross(sub(b[1], b[0]), sub(b[2], b[0])));
    if n == [0.0; 3] {
        return None;
    }
    let d0 = dot(n, b[0]);
    let p0 = m.verts[ta[i] as usize];
    let p1 = m.verts[ta[(i + 1) % 3] as usize];
    let e = sub(p1, p0);
    let den = dot(n, e);
    if den.abs() < 1e-15 {
        return None;
    }
    let t = (d0 - dot(n, p0)) / den;
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let q = [p0[0] + t * e[0], p0[1] + t * e[1], p0[2] + t * e[2]];
    if inside_trip(b, q, 1e-12) && !on_boundary_chain(&b, q) {
        Some(q)
    } else {
        None
    }
}

/// Mirror of export.rs `on_boundary_chain`: exact contact (within 1e-9, L-inf, mm) with `t`'s
/// edge chain.
fn on_boundary_chain(t: &[V3; 3], q: V3) -> bool {
    let near = |p: V3| {
        (p[0] - q[0])
            .abs()
            .max((p[1] - q[1]).abs().max((p[2] - q[2]).abs()))
            <= 1e-9
    };
    (0..3).any(|i| {
        let (pa, pb) = (t[i], t[(i + 1) % 3]);
        if near(pa) {
            return true;
        }
        let d = sub(pb, pa);
        let w = sub(q, pa);
        let den = dot(d, d);
        if den == 0.0 {
            return near(pa);
        }
        let u = dot(w, d) / den;
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let r = sub(w, [u * d[0], u * d[1], u * d[2]]);
        r[0].abs().max(r[1].abs().max(r[2].abs())) <= 1e-9
    })
}

#[test]
#[ignore]
fn adaptive_defect_census() {
    let t0 = Instant::now();
    let doc = example("bracket-filled.fsm");
    let (_, g) = graded(&doc, Resolution::Dofs(150_000), clock).unwrap();
    let filled = expr::Expr::Filled(Box::new(g.fill));
    let h = 0.25;
    let bounds = expr::bounds(&filled);
    let lo = [bounds.lo[0] - h, bounds.lo[1] - h, bounds.lo[2] - h];
    let hi = [bounds.hi[0] + h, bounds.hi[1] + h, bounds.hi[2] + h];
    let m = surface_adaptive(
        |p: V3| -> Sample {
            let d = expr::eval(&filled, p);
            Sample { v: d.v, g: d.g }
        },
        lo,
        hi,
        h,
        &AdaptiveOpts::default(),
    );
    println!(
        "  built {} verts {} tris in {:.1} s",
        m.verts.len(),
        m.tris.len(),
        t0.elapsed().as_secs_f64()
    );

    // Edge census, minus the self-intersection scan (mirrors TriMesh::defects).
    let mut edges: HashMap<(u32, u32), i32> = HashMap::with_capacity(m.tris.len() * 3);
    let mut degen: Vec<usize> = Vec::new();
    for (i, &t) in m.tris.iter().enumerate() {
        if area_of(&m, t) <= 1e-12 {
            degen.push(i);
            continue;
        }
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    let mut und: Vec<(u32, u32)> = edges
        .keys()
        .map(|&(a, b)| (a.min(b), a.max(b)))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    und.sort_unstable();
    let mut open: Vec<(u32, u32)> = Vec::new();
    let mut nonman: Vec<(u32, u32)> = Vec::new();
    for &(a, b) in &und {
        let f = edges.get(&(a, b)).copied().unwrap_or(0);
        let bk = edges.get(&(b, a)).copied().unwrap_or(0);
        let usage = f + bk;
        if usage == 1 {
            open.push((a, b));
        } else if usage >= 3 {
            nonman.push((a, b));
        }
    }
    println!(
        "  open {} non-manifold {} degenerate {}",
        open.len(),
        nonman.len(),
        degen.len()
    );

    // First tri using each undirected edge (any one is enough for printing).
    let mut owner: HashMap<(u32, u32), usize> = HashMap::new();
    for (i, &t) in m.tris.iter().enumerate() {
        for k in 0..3 {
            let e = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
            owner.entry(e).or_insert(i);
        }
    }
    let print_edge = |tag: &str, e: (u32, u32)| {
        let i = owner[&e];
        let t = m.tris[i];
        let third = t
            .iter()
            .find(|&&c| c != e.0 && c != e.1)
            .copied()
            .unwrap_or(0);
        println!(
            "    {tag} ({},{}) p1 {:?} p2 {:?} tri {i} {:?} third ({third}) {:?}",
            e.0, e.1, m.verts[e.0 as usize], m.verts[e.1 as usize], t, m.verts[third as usize]
        );
    };
    println!("  -- first open edges --");
    for &e in open.iter().take(8) {
        print_edge("open", e);
    }
    println!("  -- first non-manifold edges --");
    for &e in nonman.iter().take(8) {
        print_edge("nm", e);
    }
    println!("  -- first degenerate triangles --");
    for &i in degen.iter().take(8) {
        let t = m.tris[i];
        println!(
            "    degen {i} {:?} p {:?} {:?} {:?}",
            t, m.verts[t[0] as usize], m.verts[t[1] as usize], m.verts[t[2] as usize]
        );
    }
    // Cross-correlate: are open endpoints and non-manifold endpoints the same vertices?
    let nm_ends: HashSet<u32> = nonman.iter().flat_map(|&(a, b)| [a, b]).collect();
    let open_ends: HashSet<u32> = open.iter().flat_map(|&(a, b)| [a, b]).collect();
    let both = nm_ends.intersection(&open_ends).count();
    println!(
        "  endpoint overlap: {} of {} open ends are also non-manifold ends ({} nm ends)",
        both,
        open_ends.len(),
        nm_ends.len()
    );

    // Self intersections through a spatial hash (as TriMesh::self_intersections does).
    let cell = 0.5;
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let cell_of = |p: V3| p.map(|c| (c / cell).floor() as i64);
    let mut aabb_lo: Vec<[i64; 3]> = Vec::with_capacity(m.tris.len());
    let mut aabb_hi: Vec<[i64; 3]> = Vec::with_capacity(m.tris.len());
    for t in &m.tris {
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for &i in t {
            let p = m.verts[i as usize];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        aabb_lo.push(cell_of(lo));
        aabb_hi.push(cell_of(hi));
    }
    for (i, (lo, hi)) in aabb_lo.iter().zip(&aabb_hi).enumerate() {
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    grid.entry([x, y, z]).or_default().push(i as u32);
                }
            }
        }
    }
    let mut pairs: Vec<(usize, usize, V3)> = Vec::new();
    let mut bins = [0usize; 3]; // |n1·n2|: <0.1 (perpendicular sheets), 0.1..0.9, >=0.9 (coplanar)
    for (i, &t) in m.tris.iter().enumerate() {
        let (il, ih) = (aabb_lo[i], aabb_hi[i]);
        let mut cand: Vec<u32> = Vec::new();
        for x in il[0]..=ih[0] {
            for y in il[1]..=ih[1] {
                for z in il[2]..=ih[2] {
                    if let Some(v) = grid.get(&[x, y, z]) {
                        cand.extend_from_slice(v);
                    }
                }
            }
        }
        cand.sort_unstable();
        cand.dedup();
        for &j in &cand {
            let j = j as usize;
            if j <= i {
                continue;
            }
            let t2 = m.tris[j];
            if t.iter().any(|&v| t2.contains(&v)) {
                continue;
            }
            let mut hit = None;
            for (ta_, tb_) in [([t[0], t[1], t[2]], t2), (t2, t)] {
                for e in 0..3 {
                    if let Some(q) = seg_tri(&m, &ta_, &tb_, e) {
                        hit = Some(q);
                        break;
                    }
                }
                if hit.is_some() {
                    break;
                }
            }
            if let Some(q) = hit {
                let n1 = unit(cross(
                    sub(m.verts[t[1] as usize], m.verts[t[0] as usize]),
                    sub(m.verts[t[2] as usize], m.verts[t[0] as usize]),
                ));
                let n2 = unit(cross(
                    sub(m.verts[t2[1] as usize], m.verts[t2[0] as usize]),
                    sub(m.verts[t2[2] as usize], m.verts[t2[0] as usize]),
                ));
                let dd = (n1[0] * n2[0] + n1[1] * n2[1] + n1[2] * n2[2]).abs();
                bins[if dd < 0.1 {
                    0
                } else if dd < 0.9 {
                    1
                } else {
                    2
                }] += 1;
                pairs.push((i, j, q));
            }
        }
    }
    println!(
        "  self-intersecting pairs total {} | normal-angle bins (perp/mid/coplanar) {:?}",
        pairs.len(),
        bins
    );
    println!("  -- first self-intersecting pairs (tri indices, corners, crossing point) --");
    for &(i, j, q) in pairs.iter().take(12) {
        let t = m.tris[i];
        let t2 = m.tris[j];
        let n1 = unit(cross(
            sub(m.verts[t[1] as usize], m.verts[t[0] as usize]),
            sub(m.verts[t[2] as usize], m.verts[t[0] as usize]),
        ));
        let n2 = unit(cross(
            sub(m.verts[t2[1] as usize], m.verts[t2[0] as usize]),
            sub(m.verts[t2[2] as usize], m.verts[t2[0] as usize]),
        ));
        let a1 = area_of(&m, t);
        let a2 = area_of(&m, t2);
        // nearest lattice-plane multiple per axis for the crossing point
        let near_plane = q.map(|c: f64| {
            let r = (c / 0.25).round() * 0.25;
            (c - r).abs()
        });
        println!(
            "    x ({i},{j}) q {:?} |n1·n2| {:.3} areas {:.2e} {:.2e} plane-miss {:?}",
            q,
            (n1[0] * n2[0] + n1[1] * n2[1] + n1[2] * n2[2]).abs(),
            a1,
            a2,
            near_plane
        );
    }

    // Are the crossing points near the open / non-manifold endpoints?
    let landmarks: Vec<V3> = open
        .iter()
        .chain(nonman.iter())
        .flat_map(|&(a, b)| [m.verts[a as usize], m.verts[b as usize]])
        .collect();
    for &(i, j, q) in pairs.iter().take(12) {
        let mut best = f64::MAX;
        for &p in &landmarks {
            let dlen = len(sub(p, q));
            if dlen < best {
                best = dlen;
            }
        }
        println!("    q of ({i},{j}): nearest open/nm endpoint at {best:.3e} mm");
    }

    // Focused comparison: the adaptive surface and the uniform surface over the same small
    // region, with the field value at every dumped corner. This is the empirical answer to
    // what topology each mesher builds in a defect region.
    let region_c: V3 = [-0.05, 8.4, 13.3];
    let r: f64 = 0.6;
    let fld = |p: V3| -> f64 { expr::eval(&filled, p).v };
    // Precise field probe around the sample intersection point [−0.04, 8.25, 13.5]: rows in
    // y, columns in x, at fixed z = 13.5; are the values exact zeros, sign-oscillating eps,
    // or genuinely signed?
    let pc: V3 = [0.0, 8.25, 13.5];
    println!("  -- field values, z = 13.5, rows y (0.125), cols x (0.0625) --");
    for iy in 0..5 {
        let y = pc[1] - 0.25 + iy as f64 * 0.125;
        let mut row = String::new();
        for ix in 0..9 {
            let x = pc[0] - 0.375 + ix as f64 * 0.0625;
            row.push_str(&format!("{:>10.3e} ", fld([x, y, pc[2]])));
        }
        println!("    y {y:.4}: {row}");
    }
    println!("  -- field values along x at y = 8.4375, z = 13.0 --");
    let mut row = String::new();
    for ix in 0..9 {
        let x = pc[0] - 0.375 + ix as f64 * 0.0625;
        row.push_str(&format!("{:>10.3e} ", fld([x, 8.4375, 13.0])));
    }
    println!("    {row}");
    println!("  -- adaptive tris with a corner in the region --");
    dump_region(&m, region_c, r, &fld);
    let tu = fieldsmith_mesh::export::surface(fld, lo, hi, h);
    println!("  -- uniform tris with a corner in the region --");
    dump_region(&tu, region_c, r, &fld);
    let du = tu.defects();
    println!("  uniform defects {:?}", du);
}

/// Print every triangle of `m` with a corner inside the box around `c`, its corner ids,
/// positions and field values.
fn dump_region(m: &TriMesh, c: V3, r: f64, fld: &dyn Fn(V3) -> f64) {
    let near = |p: V3| {
        for k in 0..3 {
            if (p[k] - c[k]).abs() > r {
                return false;
            }
        }
        true
    };
    let mut n = 0usize;
    for (i, &t) in m.tris.iter().enumerate() {
        let (p0, p1, p2) = (
            m.verts[t[0] as usize],
            m.verts[t[1] as usize],
            m.verts[t[2] as usize],
        );
        if !(near(p0) || near(p1) || near(p2)) {
            continue;
        }
        n += 1;
        if n > 40 {
            continue; // print at most 40, still count the rest
        }
        println!(
            "    tri {i} {:?}\n      p {:?} f {:.6}\n        {:?} f {:.6}\n        {:?} f {:.6}",
            t,
            p0,
            fld(p0),
            p1,
            fld(p1),
            p2,
            fld(p2)
        );
    }
    println!("    total {} tris near the region", n);
}
