//! Step-3 tests: the voxeliser's closed-form volume accuracy, band exactness,
//! conservative clamping, patch stability, flood signs and error paths.
//!
//! Volumes are measured over the {v < 0} region of the reconstructed grid with per-cell
//! midpoint quadrature on the cell-local trilinear interpolant — the same function
//! `Grid3::sample` reads inside a cell, so the tests probe the field the mesher will
//! actually see, not an artefact of a particular meshing scheme.

use fieldsmith_core::voxel::{
    default_spacing, fit_spacing, grid_dims, voxelize, Voxeled, MAX_CELLS,
};
use fieldsmith_field::expr::Aabb;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::V3;
use fieldsmith_mesh::export::TriMesh;
use fieldsmith_mesh::weld::{edge_adjacency, segment};
use std::collections::HashSet;

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Signed distance of an axis-aligned box (negative inside) — exact, linear on faces.
fn sd_box(p: V3, lo: V3, hi: V3) -> f64 {
    let h = [
        (hi[0] - lo[0]) / 2.0,
        (hi[1] - lo[1]) / 2.0,
        (hi[2] - lo[2]) / 2.0,
    ];
    let c = [
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    ];
    let d = [
        (p[0] - c[0]).abs() - h[0],
        (p[1] - c[1]).abs() - h[1],
        (p[2] - c[2]).abs() - h[2],
    ];
    d[0].max(0.0).hypot(d[1].max(0.0)).hypot(d[2].max(0.0)) + d[0].max(d[1]).max(d[2]).min(0.0)
}

/// Volume of the {v < 0} region of a trilinear grid: per-cell midpoint quadrature over the
/// cell-local trilinear interpolant that [`Grid3::sample`] reads inside the cell.
///
/// A trilinear function is multi-affine — its extremes lie on the cell corners — so when
/// all eight corner samples agree in sign, the cell counts whole or not at all with no
/// sampling. Mixed-sign cells are swept at R³ uniform midpoints, each contributing
/// (s/R)³ when the interpolated value is negative. Midpoint sampling is exact where the
/// surface lies on node planes (a node-aligned 8 mm³ box measures 8.000000); on curved
/// boundaries it converges like a midpoint rule, and the residual against a closed form
/// is the grid's own discretisation error at the convex edges, not an integration artefact.
/// (For contrast: marching the same corners with linear tetrahedra systematically undercuts
/// bilinear corner cells — a node-aligned 8 mm³ box reads 7.66 — and the deficit survives
/// subdivision, which is why quadrature is used here.)
fn field_volume(g: &Grid3) -> f64 {
    const R: usize = 6;
    let s = g.spacing;
    let step = s / R as f64;
    let node = |i: usize, j: usize, k: usize| g.data[(k * g.dims[1] + j) * g.dims[0] + i] as f64;
    let mut vol = 0.0f64;
    for k in 0..g.dims[2].saturating_sub(1) {
        for j in 0..g.dims[1].saturating_sub(1) {
            for i in 0..g.dims[0].saturating_sub(1) {
                // corner b = (b&1, b>>1&1, b>>2&1) → node (i+di, j+dj, k+dk)
                let mut vv = [0.0f64; 8];
                for (b, v) in vv.iter_mut().enumerate() {
                    let (di, dj, dk) = (b & 1, (b >> 1) & 1, (b >> 2) & 1);
                    *v = node(i + di, j + dj, k + dk);
                }
                let neg = vv.iter().any(|&v| v < 0.0);
                let pos = vv.iter().any(|&v| v >= 0.0);
                if !neg {
                    continue;
                }
                if !pos {
                    vol += s * s * s;
                    continue;
                }
                // trilinear corner values (a..h = corners (0,0,0), (1,0,0), …, (1,1,1));
                // the mixed-sign cell's sub-midpoints are cheap, and negative-only cells
                // never reach here
                let (a, b, c, d, e, f, h, g8) =
                    (vv[0], vv[1], vv[2], vv[3], vv[4], vv[5], vv[6], vv[7]);
                let t = |x: f64| 1.0 - x;
                for wk in 0..R {
                    let dw = (wk as f64 + 0.5) / R as f64;
                    for wj in 0..R {
                        let dv = (wj as f64 + 0.5) / R as f64;
                        for wi in 0..R {
                            let du = (wi as f64 + 0.5) / R as f64;
                            let val = t(dw)
                                * (t(dv) * (t(du) * a + du * b) + dv * (t(du) * c + du * d))
                                + dw * (t(dv) * (t(du) * e + du * f) + dv * (t(du) * h + du * g8));
                            if val < 0.0 {
                                vol += step * step * step;
                            }
                        }
                    }
                }
            }
        }
    }
    vol
}

/// A cube with the weld.rs outward winding (shared corners, two tris per face).
fn cube(lo: V3, size: f64) -> TriMesh {
    let (x, y, z) = (lo[0], lo[1], lo[2]);
    let (xx, yy, zz) = (x + size, y + size, z + size);
    let verts: Vec<V3> = vec![
        [x, y, z],
        [xx, y, z],
        [xx, yy, z],
        [x, yy, z],
        [x, y, zz],
        [xx, y, zz],
        [xx, yy, zz],
        [x, yy, zz],
    ];
    let tris = vec![
        [0, 2, 1],
        [0, 3, 2], // z = lo
        [4, 5, 6],
        [4, 6, 7], // z = hi
        [0, 1, 5],
        [0, 5, 4], // y = lo
        [2, 3, 7],
        [2, 7, 6], // y = hi
        [1, 2, 6],
        [1, 6, 5], // x = hi
        [3, 0, 4],
        [3, 4, 7], // x = lo
    ];
    TriMesh { verts, tris }
}

/// Append a planar quad as two triangles wound so the face normal has positive dot with
/// `out` (the outward reference direction).
fn push_quad(m: &mut TriMesh, a: V3, b: V3, c: V3, d: V3, out: V3) {
    let base = m.verts.len() as u32;
    for p in [a, b, c, d] {
        m.verts.push(p);
    }
    let n = cross(sub(b, a), sub(c, a));
    if dot(n, out) < 0.0 {
        m.tris.push([base + 2, base + 1, base]);
        m.tris.push([base + 3, base + 2, base]);
    } else {
        m.tris.push([base, base + 1, base + 2]);
        m.tris.push([base, base + 2, base + 3]);
    }
}

/// A 10×10×2 box with a 4×4 axial hole through z: volume 200 − 32 = 168 mm³, concave from
/// the material's point of view. Outward normals of the hole walls point into the hole.
fn tube() -> TriMesh {
    let mut m = TriMesh {
        verts: Vec::new(),
        tris: Vec::new(),
    };
    // bottom z = 0 / top z = 2, split around the hole
    let zfaces: [(f64, V3); 2] = [(0.0, [0.0, 0.0, -1.0]), (2.0, [0.0, 0.0, 1.0])];
    for (z, out) in zfaces {
        push_quad(
            &mut m,
            [0.0, 0.0, z],
            [10.0, 0.0, z],
            [10.0, 3.0, z],
            [0.0, 3.0, z],
            out,
        );
        push_quad(
            &mut m,
            [0.0, 7.0, z],
            [10.0, 7.0, z],
            [10.0, 10.0, z],
            [0.0, 10.0, z],
            out,
        );
        push_quad(
            &mut m,
            [0.0, 3.0, z],
            [3.0, 3.0, z],
            [3.0, 7.0, z],
            [0.0, 7.0, z],
            out,
        );
        push_quad(
            &mut m,
            [7.0, 3.0, z],
            [10.0, 3.0, z],
            [10.0, 7.0, z],
            [7.0, 7.0, z],
            out,
        );
    }
    // outer walls
    push_quad(
        &mut m,
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 0.0, 2.0],
        [0.0, 0.0, 2.0],
        [0.0, -1.0, 0.0],
    );
    push_quad(
        &mut m,
        [0.0, 10.0, 0.0],
        [10.0, 10.0, 0.0],
        [10.0, 10.0, 2.0],
        [0.0, 10.0, 2.0],
        [0.0, 1.0, 0.0],
    );
    push_quad(
        &mut m,
        [0.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 10.0, 2.0],
        [0.0, 0.0, 2.0],
        [-1.0, 0.0, 0.0],
    );
    push_quad(
        &mut m,
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [10.0, 10.0, 2.0],
        [10.0, 0.0, 2.0],
        [1.0, 0.0, 0.0],
    );
    // inner walls (normals into the hole)
    push_quad(
        &mut m,
        [3.0, 3.0, 0.0],
        [3.0, 7.0, 0.0],
        [3.0, 7.0, 2.0],
        [3.0, 3.0, 2.0],
        [1.0, 0.0, 0.0],
    );
    push_quad(
        &mut m,
        [7.0, 3.0, 0.0],
        [7.0, 7.0, 0.0],
        [7.0, 7.0, 2.0],
        [7.0, 3.0, 2.0],
        [-1.0, 0.0, 0.0],
    );
    push_quad(
        &mut m,
        [3.0, 3.0, 0.0],
        [7.0, 3.0, 0.0],
        [7.0, 3.0, 2.0],
        [3.0, 3.0, 2.0],
        [0.0, 1.0, 0.0],
    );
    push_quad(
        &mut m,
        [3.0, 7.0, 0.0],
        [3.0, 7.0, 2.0],
        [7.0, 7.0, 2.0],
        [7.0, 7.0, 0.0],
        [0.0, -1.0, 0.0],
    );
    m
}

/// Flip any triangle whose normal disagrees with (centroid − centre) — sound for
/// star-shaped meshes like a sphere.
fn orient_outward(m: &mut TriMesh, centre: V3) {
    for t in &mut m.tris {
        let (a, b, c) = (
            m.verts[t[0] as usize],
            m.verts[t[1] as usize],
            m.verts[t[2] as usize],
        );
        let n = cross(sub(b, a), sub(c, a));
        let mid = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        if dot(n, sub(mid, centre)) < 0.0 {
            t.reverse();
        }
    }
}

/// UV sphere: `stacks` meridian rings × `slices` around. 64×32 keeps the chord sagitta on
/// r=5 near 0.006 mm, so the mesh's own deficit stays well inside the 1% tolerance.
fn uv_sphere(r: f64, c: V3, stacks: usize, slices: usize) -> TriMesh {
    let mut m = TriMesh {
        verts: vec![[c[0], c[1], c[2] + r]],
        tris: Vec::new(),
    };
    for j in 1..stacks {
        let phi = std::f64::consts::PI * j as f64 / stacks as f64;
        for k in 0..slices {
            let th = 2.0 * std::f64::consts::PI * k as f64 / slices as f64;
            m.verts.push([
                c[0] + r * phi.sin() * th.cos(),
                c[1] + r * phi.sin() * th.sin(),
                c[2] + r * phi.cos(),
            ]);
        }
    }
    let south = m.verts.len();
    m.verts.push([c[0], c[1], c[2] - r]);
    let rv = |j: usize, k: usize| (1 + (j - 1) * slices + (k % slices)) as u32; // ring j vertex
    for k in 0..slices {
        m.tris.push([0, rv(1, k), rv(1, k + 1)]);
    }
    for j in 1..stacks - 1 {
        for k in 0..slices {
            let (a, b, cc, d) = (rv(j, k), rv(j, k + 1), rv(j + 1, k + 1), rv(j + 1, k));
            m.tris.push([a, b, cc]);
            m.tris.push([a, cc, d]);
        }
    }
    for k in 0..slices {
        m.tris
            .push([south as u32, rv(stacks - 1, k + 1), rv(stacks - 1, k)]);
    }
    orient_outward(&mut m, c);
    m
}

/// A tetrahedron with one face removed: 3 open edges but still a sane distance field.
fn open_tet() -> TriMesh {
    let s = 2.0f64;
    let mut m = TriMesh {
        verts: vec![[0.0; 3], [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]],
        tris: vec![[0, 1, 2], [0, 1, 3], [0, 2, 3]],
    };
    let centre = [s / 4.0; 3];
    orient_outward(&mut m, centre);
    m
}

#[test]
fn analytic_box_volume_matches_closed_form() {
    // hand-built grid of the exact box SDF: reconstruction against the closed form probes
    // the volume helper itself, independently of the voxeliser. A surface lying exactly on
    // node planes is captured exactly; an offset one carries real trilinear rounding at the
    // convex edges (~h² per edge), so a 4 mm box at 0.25 spacing sits ~0.5 % low and 1 %
    // is the budget.
    for (offset, tol) in [(0.0, 0.002), (0.13, 0.01)] {
        let s = 0.25;
        let origin = [-2.75 + offset; 3];
        let dims = [23usize; 3];
        let mut data = Vec::with_capacity(23 * 23 * 23);
        for k in 0..23 {
            for j in 0..23 {
                for i in 0..23 {
                    let p = [
                        origin[0] + i as f64 * s,
                        origin[1] + j as f64 * s,
                        origin[2] + k as f64 * s,
                    ];
                    data.push(sd_box(p, [-2.0; 3], [2.0; 3]) as f32);
                }
            }
        }
        let g = Grid3 {
            origin,
            spacing: s,
            dims,
            data,
        };
        let vol = field_volume(&g);
        assert!(
            (vol - 64.0).abs() / 64.0 < tol,
            "offset {offset}: reconstructed {vol} vs 64.0"
        );
    }
}

#[test]
fn cube_volume_band_and_flood() {
    let m = cube([0.0; 3], 20.0);
    let v = voxelize(&m, None, None).unwrap();
    let g = &v.field.dist;
    assert_eq!(g.spacing, 0.25, "diag 34.64/180 clamps up to the minimum");
    let vol = field_volume(g);
    assert!(
        (vol - 8000.0).abs() / 8000.0 < 0.01,
        "cube volume {vol} vs 8000"
    );
    // deep interior: beyond the exact zone, flooded to −band
    let deep = v.field.sample([10.0, 10.0, 10.0]).v;
    assert!(
        (deep + 0.75).abs() < 1e-5,
        "deep centre {deep} should be −band"
    );
}

#[test]
fn sphere_volume_within_one_percent() {
    let m = uv_sphere(5.0, [5.0; 3], 64, 32);
    let v = voxelize(&m, None, None).unwrap();
    let vol = field_volume(&v.field.dist);
    let exact = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    assert!(
        (vol - exact).abs() / exact < 0.01,
        "sphere volume {vol} vs {exact:.3}"
    );
}

#[test]
fn tube_volume_and_signs() {
    let m = tube();
    let v = voxelize(&m, None, None).unwrap();
    let vol = field_volume(&v.field.dist);
    assert!(
        (vol - 168.0).abs() / 168.0 < 0.01,
        "tube volume {vol} vs 168"
    );
    let f = &v.field;
    // material just inside the hole wall: exact negative distance to the wall
    let wall = f.sample([2.5, 5.0, 1.0]).v;
    assert!((wall + 0.5).abs() < 0.01, "material at hole wall: {wall}");
    // inside the hole: above the surface (hole centre is 2 mm from every wall)
    let hole = f.sample([5.0, 5.0, 1.0]).v;
    assert!(
        (hole - 2.0).abs() < 0.01,
        "hole centre {hole} should read +2"
    );
    // deep material between hole and outer wall: still negative after any ties
    let deep = f.sample([5.0, 1.0, 1.0]).v;
    assert!((deep + 1.0).abs() < 0.01, "material mid-wall: {deep}");
    // just outside the outer wall
    let out = f.sample([-0.5, 5.0, 1.0]).v;
    assert!((out - 0.5).abs() < 0.01, "outside point: {out}");
}

#[test]
fn band_is_exact_and_never_overestimates() {
    let m = cube([0.0; 3], 20.0);
    let v = voxelize(&m, None, None).unwrap();
    let g = &v.field.dist;
    let band = 3.0 * g.spacing;
    let mut in_band = 0usize;
    for k in 0..g.dims[2] {
        for j in 0..g.dims[1] {
            for i in 0..g.dims[0] {
                let p = [
                    g.origin[0] + i as f64 * g.spacing,
                    g.origin[1] + j as f64 * g.spacing,
                    g.origin[2] + k as f64 * g.spacing,
                ];
                let exact = sd_box(p, [0.0; 3], [20.0; 3]);
                let val = g.data[(k * g.dims[1] + j) * g.dims[0] + i] as f64;
                assert!(
                    val.abs() <= exact.abs() + 1e-4,
                    "overestimate at {p:?}: {val} vs {exact}"
                );
                if val.abs() < band {
                    in_band += 1;
                    assert!(
                        (val - exact).abs() <= 1e-4,
                        "in-band error at {p:?}: {val} vs {exact}"
                    );
                }
            }
        }
    }
    assert!(
        in_band > g.data.len() / 20,
        "expected a thick band, got {in_band} nodes"
    );
}

#[test]
fn six_patches_stable_under_respacing() {
    let m = cube([0.0; 3], 10.0);
    let adj = edge_adjacency(&m);
    let id = segment(&m, &adj, 30.0_f64.to_radians());
    let v1 = voxelize(&m, Some(&id), Some(0.25)).unwrap();
    let v2 = voxelize(&m, Some(&id), Some(0.5)).unwrap();
    assert_eq!(v1.field.meta.patches, 6, "{:?}", v1.field.meta);
    assert_eq!(v2.field.meta.patches, 6);
    // one face centre per patch face; ids must agree across spacings
    let faces: [V3; 6] = [
        [5.0, 5.0, 0.05],
        [5.0, 5.0, 9.95],
        [5.0, 0.05, 5.0],
        [5.0, 9.95, 5.0],
        [0.05, 5.0, 5.0],
        [9.95, 5.0, 5.0],
    ];
    for f in &faces {
        assert_eq!(v1.field.patch_at(*f), v2.field.patch_at(*f), "face {f:?}");
    }
    let ids: HashSet<u16> = faces.iter().filter_map(|f| v1.field.patch_at(*f)).collect();
    assert_eq!(ids.len(), 6, "six distinct face ids: {ids:?}");
    // deep centre is beyond the band at spacing 0.5 → unclaimed
    assert_eq!(v2.field.patch_at([5.0, 5.0, 5.0]), None);
}

#[test]
fn open_edges_are_counted_and_the_field_stays_finite() {
    let m = open_tet();
    let v = voxelize(&m, None, None).unwrap();
    assert_eq!(
        v.field.meta.open_edges, 3,
        "one missing face leaves 3 open edges"
    );
    let g = &v.field.dist;
    let (mut neg, mut pos) = (false, false);
    for &val in &g.data {
        assert!(val.is_finite(), "every node must be classified");
        neg |= val < 0.0;
        pos |= val > 0.0;
    }
    assert!(neg && pos, "both signs must survive the open interior");
}

#[test]
fn default_spacing_clamps_and_budget_refuses() {
    let bb = |side: f64| Aabb {
        lo: [0.0; 3],
        hi: [side; 3],
    };
    // 1 mm cube: diag/180 → clamped up to the printable floor
    assert_eq!(default_spacing(&bb(1.0)), 0.25);
    // 2 m part: diag/180 → clamped down to the ceiling
    let big = bb(2000.0);
    assert_eq!(default_spacing(&big), 8.0);
    // the default still needs widening to fit the budget, and the result fits
    let s = fit_spacing(&big, None).unwrap();
    assert!(s >= 8.0, "2 m cube should start from the ceiling, got {s}");
    let (dims, _) = grid_dims(&big, s);
    let cells: f64 = dims.iter().map(|&d| d as f64).product();
    assert!(cells <= MAX_CELLS as f64, "{cells} cells at spacing {s}");
    // … but a forcingly-fine explicit spacing is refused with the fix in the message
    let err = fit_spacing(&bb(20.0), Some(0.02)).unwrap_err();
    assert!(err.contains("raise the voxel size"), "{err}");
}

/// `unwrap_err` without requiring `Voxeled: Debug`.
fn err_of(r: Result<Voxeled, String>) -> String {
    match r {
        Ok(_) => panic!("expected voxelize to fail"),
        Err(e) => e,
    }
}

#[test]
fn voxelize_input_errors_are_named() {
    let empty = TriMesh {
        verts: vec![],
        tris: vec![],
    };
    assert!(err_of(voxelize(&empty, None, None)).contains("no triangles"));

    let m = cube([0.0; 3], 10.0);
    let segs = vec![1u32; 11];
    assert!(err_of(voxelize(&m, Some(&segs), None)).contains("segmentation"));

    // a flat quad: zero z extent cannot bound a solid
    let flat = TriMesh {
        verts: vec![[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        tris: vec![[0, 1, 2], [0, 2, 3]],
    };
    assert!(err_of(voxelize(&flat, None, None)).contains("flat"));

    assert!(err_of(voxelize(&m, None, Some(0.0))).contains("positive"));

    // seg id beyond the patch-grid cap
    let segs = vec![65525u32; 12];
    assert!(err_of(voxelize(&m, Some(&segs), None)).contains("patch-grid cap"));
}
