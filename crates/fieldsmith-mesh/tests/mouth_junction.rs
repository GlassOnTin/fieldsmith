//! The mouth junction: a plate face cut by a cylindrical wall.
//!
//! This is the V3 quarter-model defect (docs/verification/V3.md, "Lattice class sensitivity"):
//! the symmetry plane x=0 is a max-face of the composite field — the field evaluates to exactly
//! 0 across the whole plane wherever the hole's own field is negative — so lattice edges lying
//! in the plane never cross the surface strictly. Their material-side end value is exactly 0,
//! the strict-sign scan skips them, no junction node lands on the rim (0,1,z), and the wedge
//! between the arc and the plane keeps its first free node's displacement gradient unbalanced:
//! V3 measures ~45.5 MPa at graze spacings where the half model reads ~31 MPa.
//!
//! The spacings below bracket the windows of the edge the rim crosses:
//!
//! - `claim`: the rim crossing sits at s = 0.129 of the in-plane edge — inside the long-edge
//!   warp window ALPHA_LONG = 0.24999. The straddling lattice corner on the material side must
//!   warp onto the rim.
//! - `cut`: the crossing sits at s = 0.676 — mid-edge. Warping would fold the face mesh, so the
//!   mesher must emit a cut node at the rim instead.
//! - `seal`: at h = 0.07 the crossing sits at s = 0.2857, just past the warp window, and a
//!   body-centre straddle crossing lands at s = 0.223 inside it — the tangent-layer claim that
//!   keeps the plane's wedge facets, which is what the V3 quarter-model h = 0.07 row measures.
//!
//! Before the fix the tests fail: the strict-sign scan never fires on the in-plane edges and
//! the nearest vertex to the rim is ~0.02 (the shaved wedge's own nodes) away.

use fieldsmith_mesh::{stuff, Lattice, TetMesh, V3};

/// A square plate `[0,L]² × [0,T]` with a full-height cylindrical hole of radius 1 centred on
/// the corner (0,0): `f = max(box SDF, 1 − √(x²+y²))`. The box term is the exact box distance,
/// so on the faces x=0 and y=0 it is exactly 0.0 and the composite reduces to max(0, hole) —
/// exactly 0 across the face where r > 1, positive inside the hole. That is the max-face class
/// of V3's quarter symmetry planes.
fn plate_corner_hole(p: V3, l: f64, t: f64) -> f64 {
    let q = [
        (p[0] - l / 2.0).abs() - l / 2.0,
        (p[1] - l / 2.0).abs() - l / 2.0,
        (p[2] - t / 2.0).abs() - t / 2.0,
    ];
    let box_sdf = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2) + q[2].max(0.0).powi(2)).sqrt()
        + q[0].max(q[1]).max(q[2]).min(0.0);
    box_sdf.max(1.0 - (p[0] * p[0] + p[1] * p[1]).sqrt())
}

/// Every z lattice level inside [0, t].
fn z_levels(lat: &Lattice, t: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut j = 0usize;
    loop {
        let z = lat.origin[2] + j as f64 * lat.h[2];
        if z > t + 1e-9 {
            break;
        }
        if z >= -1e-9 {
            out.push(z);
        }
        j += 1;
    }
    out
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

/// The rim (0,1,z) and (1,0,z) must carry a mesh vertex at every z lattice level: the wall
/// meets the face there, and without the node the wedge between arc and plane is unmeshed
/// material against a face whose own nodes cannot carry the displacement.
fn assert_mouth_apexes(mesh: &TetMesh, lat: &Lattice, t: f64) {
    for z in z_levels(lat, t) {
        for apex in [[0.0, 1.0, z], [1.0, 0.0, z]] {
            let d = nearest(mesh, apex);
            assert!(
                d < 5e-3,
                "no vertex at the mouth apex ({:.4},{:.4},{:.4}): nearest vertex is {:.4} away",
                apex[0],
                apex[1],
                apex[2],
                d
            );
        }
    }
}

/// Every vertex that sits on the plane x = 0 must belong to at least one boundary face whose
/// three nodes are all on the plane. The plane is a free surface of the solid, so its mesh must
/// close there; a strip vertex held only by faces that leave the plane hangs free in one field
/// direction — the mechanism behind V3's quarter-model peak at h = 0.07 (46.31 MPa measured,
/// 31.15 expected).
fn assert_plane_sealed(mesh: &TetMesh, axis: usize) {
    let on_plane = |v: [f64; 3]| v[axis].abs() < 1e-6;
    let mut sealed = vec![false; mesh.verts.len()];
    for (face, _) in mesh.boundary() {
        if face.iter().all(|&n| on_plane(mesh.verts[n as usize])) {
            for &n in &face {
                sealed[n as usize] = true;
            }
        }
    }
    for (i, v) in mesh.verts.iter().enumerate() {
        assert!(
            !on_plane(*v) || sealed[i],
            "vertex {i} at ({:.4},{:.4},{:.4}) sits on the plane x={} but belongs to no \
             boundary face with all three nodes on that plane",
            v[0],
            v[1],
            v[2],
            "0"
        );
    }
}

#[test]
fn plane_vertices_are_sealed_at_the_graze_tangent_when_a_body_centre_straddles() {
    // L = 2.03, t = 0.14, h = 0.07 → 29 cells of exactly 0.07 on x/y (the plane x = 0 is a
    // lattice row, as in V3's quarter model) and 2 of 0.07 in z. The rim crossing on the
    // max-face corner edge sits at s = (1 − 0.98)/0.07 = 0.2857, past ALPHA_LONG — the plane
    // gets its cut node. The centre-to-centre straddle crossing sits at
    // 1 − t = (1.015 − √(1 − 0.035²))/0.07 = 0.223, inside ALPHA_LONG = 0.24999 — the
    // body-centre vertex on the material side of the strip gets claimed, which is what drops
    // the mouth wedges' plane facets (V3 h = 0.07: 46.31 MPa, +52.7 %).
    let (l, t, h) = (2.03, 0.14, 0.07);
    let (mesh, lat) = stuff(
        |p| plate_corner_hole(p, l, t),
        [0.0, 0.0, 0.0],
        [l, l, t],
        h,
    );
    let v = (l * l - std::f64::consts::FRAC_PI_4) * t;
    assert!(
        (mesh.volume() - v).abs() < 5e-3 * v,
        "volume {} vs analytic {}",
        mesh.volume(),
        v
    );
    assert_mouth_apexes(&mesh, &lat, t);
    assert_plane_sealed(&mesh, 0);
}

#[test]
fn mouth_corner_is_claimed_when_the_rim_crossing_sits_in_the_claim_window() {
    // L = 2.7385, h = 0.11 → 25 cells of 0.10954; each face's straddling row pair is
    // 0.98586 / 1.09540, so the rim crosses at s = (1 − 0.98586)/0.10954 = 0.129 < 0.24999.
    let (l, t, h) = (2.7385, 0.44, 0.11);
    let (mesh, lat) = stuff(
        |p| plate_corner_hole(p, l, t),
        [0.0, 0.0, 0.0],
        [l, l, t],
        h,
    );
    let v = (l * l - std::f64::consts::FRAC_PI_4) * t;
    assert!(
        (mesh.volume() - v).abs() < 5e-3 * v,
        "volume {} vs analytic {}",
        mesh.volume(),
        v
    );
    assert_mouth_apexes(&mesh, &lat, t);
}

#[test]
fn mouth_rim_gets_a_cut_node_when_the_crossing_sits_mid_edge() {
    // L = 1.85, h = 0.063 → 29 cells of 0.0637931; rows 0.95690 / 1.02069, crossing at
    // s = (1 − 0.95690)/0.0637931 = 0.676 — mid-edge, outside both warp windows, so the
    // junction node must come from a cut, not a warp.
    let (l, t, h) = (1.85, 0.44, 0.063);
    let (mesh, lat) = stuff(
        |p| plate_corner_hole(p, l, t),
        [0.0, 0.0, 0.0],
        [l, l, t],
        h,
    );
    let v = (l * l - std::f64::consts::FRAC_PI_4) * t;
    assert!(
        (mesh.volume() - v).abs() < 5e-3 * v,
        "volume {} vs analytic {}",
        mesh.volume(),
        v
    );
    assert_mouth_apexes(&mesh, &lat, t);
}
