//! The cap-disc chime: a subtracted cylinder whose cap discs coincide with the parent's caps.
//!
//! On the cap plane the composite reads `max(outer = 0, −bore = 0) = 0` across the whole disc
//! r < ri — the coincident-surface zero patch. The spur pass flips lattice corners on that patch
//! to `val = eps` so no cell keeps them as material. The max-face scan must not then treat a
//! spur corner as a crossing end: its field value is exactly 0, so the claim-window bisection
//! degenerates and would pin the corner at its own inside-the-void position as a kept corner —
//! material tets reaching ~h/3 deep into the bore at every cap (the V20 regression measured
//! 16/20/32 penetrated nodes with 36/28/28 emitting tets at h = 6/4/3).
//!
//! The bound here is the mesher's designed coincidence tolerance, not zero: a kept vertex near
//! a wall stands off by at most one spur-probe diagonal, eps = 0.05·max_h. Vertices deeper than
//! that are a defect.

use fieldsmith_mesh::{stuff, TetMesh, V3};

/// A capped cylinder `r ≤ radius`, `|z| ≤ half_h`: fieldsmith's cylinder primitive with
/// round = 0 (`extrude_combine`: exterior as the hypot of the violated axes, interior as the max).
fn capped_cylinder(p: V3, radius: f64, half_h: f64) -> f64 {
    let rl = (p[0] * p[0] + p[1] * p[1]).sqrt();
    let wx = rl - radius;
    let wy = p[2].abs() - half_h;
    let ol = (wx.max(0.0) * wx.max(0.0) + wy.max(0.0) * wy.max(0.0)).sqrt();
    if ol > 0.0 {
        ol
    } else {
        wx.max(wy)
    }
}

/// The Lame-class annulus: outer cylinder minus the bore, caps coinciding, centred at z = len/2
/// so the cap planes sit at z = 0 and z = len as in V20.
fn annulus(p: V3, ri: f64, ro: f64, len: f64) -> f64 {
    let half = len / 2.0;
    let pc = [p[0], p[1], p[2] - half];
    let mut f = capped_cylinder(pc, ro, half);
    let bore = capped_cylinder(pc, ri, half);
    if -bore > f {
        f = -bore;
    }
    f
}

#[test]
fn no_vertex_stands_deeper_into_the_bore_than_the_coincidence_offset() {
    let (ri, ro, len) = (10.0, 20.0, 120.0);
    for h in [6.0, 3.0] {
        let (mesh, _lat) = stuff(
            |p| annulus(p, ri, ro, len),
            [-ro, -ro, 0.0],
            [ro, ro, len],
            h,
        );
        let eps = 0.05 * h;
        let deep: Vec<[f64; 3]> = mesh
            .verts
            .iter()
            .filter(|p| (p[0] * p[0] + p[1] * p[1]).sqrt() < ri - 1.2 * eps)
            .copied()
            .collect();
        assert!(
            deep.is_empty(),
            "h={h}: {} vertices deeper than {:.3} into the bore of r={ri} (designed standoff is \
             one spur diagonal, eps={eps:.3}); worst r={:.4}; first: {:?}",
            deep.len(),
            1.2 * eps,
            deep.iter()
                .map(|p| (p[0] * p[0] + p[1] * p[1]).sqrt())
                .fold(f64::INFINITY, f64::min),
            deep.first(),
        );

        // The chime rim where each cap meets the bore wall must still carry vertices: without
        // them the cap face mesh runs past the wall into the disc.
        for z in [eps, len - eps] {
            for apex in [[ri, 0.0, z], [0.0, ri, z], [-ri, 0.0, z], [0.0, -ri, z]] {
                let d = nearest(&mesh, apex);
                assert!(
                    d < 0.8 * h,
                    "h={h}: no vertex near the chime rim ({:.2},{:.2},{:.2}): nearest \
                     vertex is {:.4} away",
                    apex[0],
                    apex[1],
                    apex[2],
                    d
                );
            }
        }
    }
}

fn nearest(mesh: &TetMesh, p: V3) -> f64 {
    mesh.verts
        .iter()
        .map(|v| {
            ((v[0] - p[0]) * (v[0] - p[0])
                + (v[1] - p[1]) * (v[1] - p[1])
                + (v[2] - p[2]) * (v[2] - p[2]))
                .sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}
