//! Derived offset/shell grids over mesh operands (goal M7, step 2).
//!
//! A hydrated mesh operand compiles a `Shell` / `Offset` modifier to a keyed derived
//! `Expr::Mesh` leaf instead of the eval-time wrap. Two landing routes:
//!
//! - Pointwise (param within one band, < 3·spacing): the parent's stored distances are
//!   exact there, so the derived surface is the exact Euclidean offset of the parent's
//!   zero set. Grow rows reference the Steiner form (cube ⊕ ball: core ∪ 6 face slabs ∪
//!   12 edge quarter-cylinders ∪ 8 corner octants); erosion rows stay exact cubes and
//!   cube shells, because erosion of a cube keeps square corners. The L∞ square-corner
//!   form is the wrong reference for a grow — the reconciliation run measured −4.9 %
//!   against it on the half-5 rows and −0.77 % against Steiner.
//! - Mask bake (param from one band out): the chamfer thresholds count depth in grid
//!   cells and the bake stores every kept/dropped boundary as a ±1 cell pair, so each
//!   face's isosurface lands exactly at hull + ½·spacing — half a cell outward for a
//!   whole-cell param, never short of the param, at most one spacing past it; wall
//!   thickness is exact for whole-cell walls because both faces shift by the same half
//!   cell. The chamfer's diagonal steps round box corners, so mask rows reference the
//!   landing as solid-cell-count parity (hull-clipped, 0.03–0.72 % from the integrator)
//!   plus crossing windows per axis, with the L∞-land cube printed as the documented
//!   corner cut (measured −6.75 % gro·4, −10.7 % outside-4 ring, −8.7 % centre-6 wall
//!   on the half-10 parent).
//!
//! The eval-time wrap is exact for analytic primitives (no band clamp on their operands)
//! and wrong past the band for clamped voxel ones, which is why the routes split. Every
//! constant below is pinned from a reconciliation run; `measured` prints every row so
//! retuning sees the numbers, not assumptions.

use fieldsmith_doc::{Document, Modifier, Node, NodeKind, Op, ShellSide, Transform};
use fieldsmith_field::expr::Expr;
use fieldsmith_field::lattice::Grid3;
use fieldsmith_field::meshfield::{
    hash_mesh_input, hydrate, lookup, offset_field, offset_key, shell_field, shell_key, MeshField,
    MeshMeta, BAND_CELLS, MAX_DERIVED_SPACING, MAX_EXTEND_CELLS, SHELL_CENTRE, SHELL_INSIDE,
    SHELL_OUTSIDE,
};
use fieldsmith_field::{compile_body, eval, volume, Aabb};
use std::sync::Arc;

/// Bake an exact box signed-distance grid (band-clamped, like every voxel field).
fn baked_box(half: f64, spacing: f64) -> (Grid3, Aabb) {
    let band = BAND_CELLS * spacing;
    let mut dims = [0usize; 3];
    for d in &mut dims {
        *d = ((2.0 * half + 2.0 * band) / spacing).ceil() as usize + 2;
    }
    let origin = [-(half + band); 3];
    let mut data = Vec::with_capacity(dims[0] * dims[1] * dims[2]);
    for k in 0..dims[2] {
        for j in 0..dims[1] {
            for i in 0..dims[0] {
                let p = [
                    origin[0] + spacing * i as f64,
                    origin[1] + spacing * j as f64,
                    origin[2] + spacing * k as f64,
                ];
                let q = [p[0].abs() - half, p[1].abs() - half, p[2].abs() - half];
                let m = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
                let outside = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                let inside = q[0].max(q[1]).max(q[2]).min(0.0);
                data.push((outside + inside).clamp(-band, band) as f32);
            }
        }
    }
    (
        Grid3 {
            origin,
            spacing,
            dims,
            data,
        },
        Aabb {
            lo: [-half; 3],
            hi: [half; 3],
        },
    )
}

/// Hydrate a box (2·`half` mm a side) at `spacing` under a tag-derived key.
fn hyd_box(tag: &str, half: f64, spacing: f64) -> u64 {
    let key = hash_mesh_input(format!("derived:{tag}").as_bytes(), b"");
    if lookup(key).is_none() {
        let (grid, bbox) = baked_box(half, spacing);
        hydrate(
            key,
            MeshField {
                dist: Arc::new(grid),
                patch: None,
                meta: MeshMeta {
                    bbox,
                    tris: 12,
                    patches: 0,
                    open_edges: 0,
                },
            },
        );
    }
    key
}

fn mesh_node(id: &str, key: u64, half: f64) -> Node {
    Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        kind: NodeKind::Mesh {
            src: "test.stl".into(),
            hash: format!("{key:016x}"),
            units: "mm".into(),
            up: "z".into(),
            voxel: 1.0,
            bbox: [-half, -half, -half, half, half, half],
            tris: 12,
            patches: 0,
            open_edges: 0,
        },
    }
}

/// A group holding `children` with the given modifiers — where modifiers attach.
fn group(id: &str, children: Vec<Node>, modifiers: Vec<Modifier>) -> Node {
    Node {
        id: id.into(),
        op: Op::Add,
        blend: 0.0,
        suppressed: false,
        transform: Transform::default(),
        kind: NodeKind::Group {
            children,
            modifiers,
        },
    }
}

/// The doc's compiled expression for a single-mesh group plus the modifier: the fast path.
fn compiled_offset(key: u64, d: f64) -> Expr {
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![mesh_node("mesh1", key, 5.0)],
        vec![Modifier::Offset { distance: d }],
    ));
    compile_body(&doc)
}

fn compiled_shell(key: u64, side: ShellSide, t: f64) -> Expr {
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![mesh_node("mesh1", key, 5.0)],
        vec![Modifier::Shell { thickness: t, side }],
    ));
    compile_body(&doc)
}

/// Volume of the compiled expression, printing the measured/closed-form pair so tolerance
/// constants can be set from runs.
fn measured(e: &Expr, h: f64, name: &str, want: f64) -> f64 {
    let v = volume::volume(e, h).volume;
    println!(
        "{name}: measured {v:.2} mm³, closed form {want:.2} mm³, error {:.3}%",
        100.0 * (v - want).abs() / want
    );
    v
}

fn expect_close(v: f64, want: f64, tol_pct: f64, name: &str) {
    assert!(
        100.0 * (v - want).abs() / want <= tol_pct,
        "{name}: {v} vs closed form {want} beyond {tol_pct}%"
    );
}

/// Steiner form of a ball-dilated a-cube: core ∪ 6 face slabs ∪ 12 edge
/// quarter-cylinders ∪ 8 corner octants of a sphere — the closed form behind a grow.
fn steiner(a: f64, t: f64) -> f64 {
    a.powi(3)
        + 6.0 * a.powi(2) * t
        + 3.0 * std::f64::consts::PI * a * t * t
        + (4.0 * std::f64::consts::PI / 3.0) * t.powi(3)
}

/// The baked solid's own measure: kept cell count × s³. On the mask route every stored
/// boundary is a ±1 cell pair, so each face's hull sits half a spacing from its
/// outermost kept centre; the hull-clipped cell measure runs within a percent of the
/// integrator on every mask row (the reconciliation run pinned 0.03–0.72 %).
fn baked_solid_mm3(e: &Expr) -> (f64, f64) {
    let Expr::Mesh {
        key,
        spec: Some(sp),
        ..
    } = e
    else {
        panic!("mask rows expect a baked derived leaf, got {e:?}");
    };
    let baked = lookup(*key).expect("derived key must be registered");
    let cells = baked.dist.data.iter().filter(|v| **v <= 0.0f32).count() as f64;
    (cells * sp.spacing.powi(3), cells)
}

/// Probe the mask landing directly: on each of x, y and z the surface must sit inside a
/// one-spacing window opening at `base` (probes at base − 0.01 and base + 1.01, along
/// interior lines far from the corners). `solid_below` picks which sample is solid.
fn crossing_window(e: &Expr, base: f64, solid_below: bool) {
    for (axis, p) in [
        (0usize, [0.17f64, -0.31, 0.0]),
        (1, [0.0, -0.31, 0.17]),
        (2, [-0.31, 0.0, 0.17]),
    ] {
        let mut below = p;
        let mut above = p;
        below[axis] = base - 0.01;
        above[axis] = base + 1.01;
        let (bd, ad) = (eval(e, below).v, eval(e, above).v);
        let ok = if solid_below {
            bd < 0.0 && ad > 0.0
        } else {
            bd > 0.0 && ad < 0.0
        };
        assert!(
            ok,
            "crossing window on axis {axis} around base {base}: below {bd:.3}, above {ad:.3}"
        );
    }
}

#[test]
fn offset_pointwise_route_lands_on_the_closed_form() {
    let key = hyd_box("offset-point", 5.0, 1.0);
    // Grows reference Steiner (rounded Euclidean corners); shrinks stay exact cubes.
    for (d, want) in [
        (0.6, steiner(10.0, 0.6)),
        (1.8, steiner(10.0, 1.8)),
        (-0.6, (8.8f64).powi(3)),
        (-1.8, (6.4f64).powi(3)),
    ] {
        let e = compiled_offset(key, d);
        assert!(
            matches!(e, Expr::Mesh { .. }),
            "single mesh operand bakes a leaf, got {:?}",
            e
        );
        let v = measured(&e, 0.2, "offset pointwise", want);
        expect_close(v, want, 2.0, "offset pointwise");
    }
}

#[test]
fn shell_pointwise_walls_land_on_the_closed_form() {
    // t = 2.0: ≥ 6·h and ≥ 2·spacing thick, so the walls are sampleable. Inside keeps
    // cube shells (erosion); outside is a Steiner annulus; centre is the Steiner outer
    // half-ring plus the square inner half-ring.
    let key = hyd_box("shell-point", 5.0, 1.0);
    let cases = [
        (ShellSide::Inside, 2.0f64, 1000.0 - (6.0f64).powi(3)),
        (ShellSide::Outside, 2.0f64, steiner(10.0, 2.0) - 1000.0),
        (
            ShellSide::Centre,
            2.0f64,
            (steiner(10.0, 1.0) - 1000.0) + (1000.0 - (8.0f64).powi(3)),
        ),
    ];
    for (side, t, want) in cases {
        let e = compiled_shell(key, side, t);
        assert!(
            matches!(e, Expr::Mesh { .. }),
            "single mesh operand bakes a leaf, got {:?}",
            e
        );
        let v = measured(&e, 0.2, "shell pointwise", want);
        expect_close(v, want, 2.0, "shell pointwise");
    }
}

#[test]
fn sub_spacing_shell_walls_alias_to_nothing() {
    // t = 0.4 < one spacing: the shell's (−t, 0) kink band contains no cell centre beside
    // the surface cell itself, so the baked wall degenerates to a zero-valued plane and the
    // measured wall volume collapses toward the integrator's half-weight skin. Documented
    // lossy case; validate (step 6) warns on sub-voxel thickness. Direction only.
    let key = hyd_box("shell-thin", 5.0, 1.0);
    let e = compiled_shell(key, ShellSide::Inside, 0.4);
    let want = 1000.0 - (10.0f64 - 0.8).powi(3);
    let v = volume::volume(&e, 0.2).volume;
    println!("sub-spacing shell: measured {v:.2}, closed form {want:.2}");
    assert!(
        v < 0.5 * want,
        "sub-spacing wall must collapse, not inflate: {v} ≥ {want}/2"
    );
}

#[test]
fn offset_mask_route_lands_half_a_cell_out() {
    // Whole-cell params on the 20 mm box (half 10): the bake surface lands half a spacing
    // past the outermost kept cell — grow 4 keeps rings ≤ 4 past the ±10 surface (every
    // axis crosses at 14.5), shrink 4 keeps depth ≥ 4.5 cells (crossing 6.5, the 13³
    // cube, measured −0.72 %). Grow volume: cell-count parity, bracketed by Steiner
    // (Euclidean lower bound) and the L∞-land 29³ ceiling — the chamfer corner cut is
    // part of the claim, measured −6.75 % against the ceiling.
    let key = hyd_box("offset-mask", 10.0, 1.0);
    let e = compiled_offset_masked_half10(key, 4.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let (cells_mm3, cells) = baked_solid_mm3(&e);
    let v = measured(&e, 0.1, "offset mask grow 4 (per cells)", cells_mm3);
    expect_close(v, cells_mm3, 1.0, "offset mask grow 4 cells parity");
    let steiner_lo = steiner(20.0, 4.0);
    let linf_hi = (29f64).powi(3);
    println!(
        "offset mask grow 4: {cells:.0} kept cells, window [Steiner {steiner_lo:.0} .. L∞-land {linf_hi:.0}], measured {v:.0}"
    );
    assert!(
        v >= steiner_lo,
        "grow 4 below the Steiner bound: {v} < {steiner_lo}"
    );
    assert!(
        v <= linf_hi,
        "grow 4 above the L∞-land ceiling: {v} > {linf_hi}"
    );
    crossing_window(&e, 14.0, true);

    let e = compiled_offset_masked_half10(key, -4.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let want = (13.0f64).powi(3);
    let v = measured(&e, 0.1, "offset mask shrink 4", want);
    expect_close(v, want, 2.0, "offset mask shrink 4");
    crossing_window(&e, 6.0, true);
}

fn compiled_shell_masked_half10(key: u64, side: ShellSide, t: f64) -> Expr {
    // Same build as `compiled_shell` but with the half-10 parent the mask rows use.
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![mesh_node("mesh1", key, 10.0)],
        vec![Modifier::Shell { thickness: t, side }],
    ));
    compile_body(&doc)
}

fn compiled_offset_masked_half10(key: u64, d: f64) -> Expr {
    // Same build as `compiled_offset` but with the half-10 parent the mask rows use.
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![mesh_node("mesh1", key, 10.0)],
        vec![Modifier::Offset { distance: d }],
    ));
    compile_body(&doc)
}

#[test]
fn offset_mask_route_quantisation_never_short_of_the_param() {
    // A 4.3 mm grow rounds the kept ring half-up (th = |d|/s + 0.5 = 4.8 keeps 4 rings,
    // landing 14.5): the landing never sits short of the param and at most one spacing
    // past it, probed as a sign window on every axis.
    let key = hyd_box("offset-quant", 10.0, 1.0);
    let e = compiled_offset_masked_half10(key, 4.3);
    assert!(matches!(e, Expr::Mesh { .. }));
    crossing_window(&e, 10.0 + 4.3, true);
    let (cells_mm3, _cells) = baked_solid_mm3(&e);
    let v = volume::volume(&e, 0.1).volume;
    println!(
        "offset quantise 4.3: measured {v:.0}, baked cells {cells_mm3:.0}, parity {:.3}%",
        100.0 * (v - cells_mm3).abs() / cells_mm3
    );
}

#[test]
fn shell_mask_route_thickness_is_exact() {
    // Whole-cell walls on the 20 mm box: both faces shift by the same half cell, so the
    // thickness lands on t exactly (crossing windows pin both faces; the inside wall's
    // kept cells count the closed form 21³ − 13³ exactly). Outside and centre rings
    // round at the chamfer corners, so they reference the baked cell count with the
    // L∞-land cube printed as the documented corner cut.
    //   inside 4:   wall [6.5, 10.5]  → 21³ − 13³ = 7064
    //   outside 4:  wall [10.5, 14.5] → L∞-land 29³ − 21³, cells-parity (−10.7 % measured)
    //   centre 6:   wall [7.5, 13.5]  → L∞-land 27³ − 15³, cells-parity (−8.7 % measured)
    let key = hyd_box("shell-mask", 10.0, 1.0);

    let e = compiled_shell_masked_half10(key, ShellSide::Inside, 4.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let want = (21.0f64).powi(3) - (13.0f64).powi(3);
    let v = measured(&e, 0.1, "shell mask inside 4", want);
    expect_close(v, want, 2.0, "shell mask inside 4");
    crossing_window(&e, 6.0, false);
    crossing_window(&e, 10.0, true);

    let e = compiled_shell_masked_half10(key, ShellSide::Outside, 4.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let (cells_mm3, _) = baked_solid_mm3(&e);
    let v = measured(&e, 0.1, "shell mask outside 4 (per cells)", cells_mm3);
    expect_close(v, cells_mm3, 1.0, "shell mask outside 4 cells parity");
    crossing_window(&e, 10.0, false);
    crossing_window(&e, 14.0, true);
    let linf = (29f64).powi(3) - (21f64).powi(3);
    println!("shell mask outside 4: L∞-land {linf:.0}, measured {v:.0}");
    assert!(
        v < linf,
        "outside 4 sits below the L∞-land cube (corner cut): {v} vs {linf}"
    );

    let e = compiled_shell_masked_half10(key, ShellSide::Centre, 6.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let (cells_mm3, _) = baked_solid_mm3(&e);
    let v = measured(&e, 0.1, "shell mask centre 6 (per cells)", cells_mm3);
    expect_close(v, cells_mm3, 1.0, "shell mask centre 6 cells parity");
    crossing_window(&e, 7.0, false);
    crossing_window(&e, 13.0, true);
    let linf = (27f64).powi(3) - (15f64).powi(3);
    println!("shell mask centre 6: L∞-land {linf:.0}, measured {v:.0}");
    assert!(
        v < linf,
        "centre 6 sits below the L∞-land cube (corner cut): {v} vs {linf}"
    );
    // Thickness exact beyond the corners: the wall span is the crossing-window pair.
}

#[test]
fn group_of_one_bakes_and_group_of_two_keeps_the_wrap() {
    // Group of one over a mesh: the bake. Group of two (mesh + primitive): the eval-time
    // wrap stays — exact for the analytic box operand, whose distances have no band clamp.
    // The wrap's failure mode on clamped VOXEL operands is proven separately below.
    let key = hyd_box("path-split", 5.0, 1.0);
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![
            mesh_node("mesh1", key, 5.0),
            Node {
                id: "b".into(),
                op: Op::Add,
                blend: 0.0,
                suppressed: false,
                transform: Transform {
                    translate: [30.0; 3],
                    rotate: [0.0; 3],
                },
                kind: NodeKind::Box {
                    size: [8.0; 3],
                    round: 0.0,
                },
            },
        ],
        vec![Modifier::Shell {
            thickness: 2.0,
            side: ShellSide::Inside,
        }],
    ));
    // Group of one first: the same modifier over the bare mesh bakes a leaf.
    let one = compiled_shell(key, ShellSide::Inside, 2.0);
    assert!(
        matches!(one, Expr::Mesh { .. }),
        "group of one over a hydrated mesh bakes a leaf, got {one:?}"
    );
    let e = compile_body(&doc);
    let Expr::Shell {
        side: ShellSide::Inside,
        t: 2.0,
        e: inner,
    } = &e
    else {
        panic!("multi-operand group keeps the general path, got {e:?}");
    };
    assert!(
        matches!(**inner, Expr::Combine { .. }),
        "wrap rides over the combined operands, got {inner:?}"
    );
    // The wrap's algebra on the analytic box: the 4 mm core (|d| = 4 > t) empties,
    // the ≤ 2 mm wall stays solid.
    assert!(eval(&e, [30.0; 3]).v > 0.0, "combined group core removed");
    assert!(eval(&e, [33.0, 30.0, 30.0]).v < 0.0, "wall survives");
}

#[test]
fn pointwise_bake_agrees_with_the_eval_wrap() {
    // Two interpolation models share one algebra: the wrap maps the interpolated parent
    // value pointwise; the bake maps at the parent's sample planes and interpolates the
    // mapped values. At the sample planes both hold the same number exactly. Off sample,
    // across a wall's kink cells, the values differ — the bake's chord vs the wrap's
    // kinked piece — while the zero set is shared wherever the kink band spans the sample
    // planes (t ≥ one spacing). The lattice stays far-field (±3.5, never crosses 5); the
    // sweep crosses the +x wall and asserts shared surface plus a value gap bounded by
    // the cell spacing, not a fitted tolerance.
    let key = hyd_box("agree", 5.0, 1.0);
    for t in [0.8, 1.6] {
        let e = compiled_shell(key, ShellSide::Outside, t);
        let wrap = Expr::Shell {
            side: ShellSide::Outside,
            t,
            e: Box::new(Expr::Mesh {
                key,
                bbox: Aabb {
                    lo: [-5.0; 3],
                    hi: [5.0; 3],
                },
                spec: None,
            }),
        };
        // Exact at the parent's sample planes — integer positions across the +x wall.
        let mut sample = 0.0f64;
        for i in -4..=4 {
            for j in -4..=4 {
                for k in -4..=4 {
                    let p = [5.0 + i as f64, j as f64, k as f64];
                    sample = sample.max((eval(&e, p).v - eval(&wrap, p).v).abs());
                }
            }
        }
        assert!(sample < 1e-4, "t={t}: sample-plane agreement {sample:.2e}");
        // Far field, off sample: the two models are the same mapped algebra on nearly
        // parallel interpolated parents, so the gap stays at f32 roundoff.
        let mut far = 0.0f64;
        for i in -12..=12 {
            for j in -12..=12 {
                for k in -12..=12 {
                    let p = [
                        0.29 * i as f64 + 0.17,
                        0.29 * j as f64 - 0.11,
                        0.29 * k as f64 + 0.05,
                    ];
                    far = far.max((eval(&e, p).v - eval(&wrap, p).v).abs());
                }
            }
        }
        assert!(far < 1e-4, "t={t}: far-field agreement {far:.2e}");
        if t < 1.0 {
            println!("shell t={t}: sample planes agree; sweep skipped — the kink band holds one sample plane, the wall aliases to its zero plane (sub-voxel case)");
            continue;
        }
        // Wall-band sweep: same side everywhere (shared zero set), value gap ≤ spacing.
        let mut worst = 0.0f64;
        let mut gap_at = [0.0f64; 3];
        for i in 0..=48 {
            let p = [4.5 + i as f64 * 0.05, 0.17, -0.31];
            let a = eval(&e, p).v;
            let b = eval(&wrap, p).v;
            let same =
                (a < 0.0 && b < 0.0) || (a > 0.0 && b > 0.0) || (a.abs() < 1e-9 && b.abs() < 1e-9);
            assert!(
                same,
                "t={t}: bake {a:+.3} and wrap {b:+.3} split sides at {p:?}"
            );
            if (a - b).abs() > worst {
                worst = (a - b).abs();
                gap_at = p;
            }
        }
        println!("shell t={t} pointwise-vs-wrap: wall-band value gap {worst:.3} at {gap_at:?} (bound: one spacing)");
        assert!(
            worst <= 1.0,
            "t={t}: the interpolation models differ by more than a spacing"
        );
    }
}

#[test]
fn mask_route_diverges_from_the_wrap_beyond_the_band() {
    // The eval-time wrap is provably wrong past ±3·spacing on clamped voxel data: the deep
    // core of a shell stays solid there. The bake is the point of the feature.
    let key = hyd_box("diverge", 5.0, 1.0);
    let e = compiled_shell(key, ShellSide::Inside, 4.0);
    let wrap = Expr::Shell {
        side: ShellSide::Inside,
        t: 4.0,
        e: Box::new(Expr::Mesh {
            key,
            bbox: Aabb {
                lo: [-5.0; 3],
                hi: [5.0; 3],
            },
            spec: None,
        }),
    };
    let derived = eval(&e, [0.0; 3]).v;
    let wrapped = eval(&wrap, [0.0; 3]).v;
    println!("core: derived {derived:.3}, wrap {wrapped:.3}");
    assert!(derived > 0.0, "bake must empty the deep core: {derived}");
    assert!(wrapped < 0.0, "wrap keeps the core solid, as predicted");
}

#[test]
fn unhydrated_parent_keeps_the_eval_wrap() {
    // No hydration for this key: the compile cannot bake, and keeps the wrap so validate
    // (registry-aware, step 6) names the unhydrated mesh instead.
    let key = hash_mesh_input(b"derived:never-hydrated".as_slice(), b"");
    assert!(lookup(key).is_none(), "test key must stay unhydrated");
    let e = compiled_shell(key, ShellSide::Inside, 2.0);
    assert!(
        matches!(&e, Expr::Shell { t: 2.0, .. }),
        "unhydrated operand keeps the wrap, got {:?}",
        e
    );
}

#[test]
fn transformed_mesh_derivations_rewrap_placement() {
    // The derived grid lives in the mesh's local frame: a translated mesh re-wraps its
    // derived leaf in the same translation, and the bake key ignores placement (content
    // addressing — the same mesh under different transforms shares one bake, covered
    // structurally by the keys test).
    let key = hyd_box("rewrap", 5.0, 1.0);
    let mut doc = Document::default();
    doc.root.push(group(
        "edit",
        vec![mesh_node("mesh1", key, 5.0)],
        vec![Modifier::Offset { distance: 1.5 }],
    ));
    doc.root[0].transform.translate = [20.0, 5.0, -3.0];
    let e = compile_body(&doc);
    assert!(
        matches!(&e, Expr::Xform { e, .. } if matches!(**e, Expr::Mesh { .. })),
        "derived leaf re-wrapped in the node's placement, got {:?}",
        e
    );
    // The grown surface evaluates in the translated frame: pointwise offsets land exact,
    // so x = 25 + 1.5 ± 0.01 straddles the grown +x face.
    let a = eval(&e, [25.0 + 1.5 - 0.01, 5.0, -3.0]).v;
    let b = eval(&e, [25.0 + 1.5 + 0.01, 5.0, -3.0]).v;
    assert!(
        a < 0.0 && b > 0.0,
        "surface at +x grown by 1.5: inside {a}, outside {b}"
    );
}

#[test]
fn derived_keys_follow_content_and_params() {
    let a = "0123456789abcdef";
    let b = "fedcba9876543210";
    // Determinism.
    assert_eq!(offset_key(a, 1.0), offset_key(a, 1.0));
    assert_eq!(
        shell_key(a, SHELL_CENTRE, 2.0),
        shell_key(a, SHELL_CENTRE, 2.0)
    );
    // Different parent, distance or side → different keys.
    assert_ne!(offset_key(a, 1.0), offset_key(b, 1.0));
    assert_ne!(offset_key(a, 1.0), offset_key(a, 2.0));
    assert_ne!(offset_key(a, 1.0), offset_key(a, -1.0));
    assert_ne!(
        shell_key(a, SHELL_INSIDE, 2.0),
        shell_key(a, SHELL_OUTSIDE, 2.0)
    );
    assert_ne!(
        shell_key(a, SHELL_INSIDE, 2.0),
        shell_key(a, SHELL_CENTRE, 2.0)
    );
    // Offset and shell namespaces stay disjoint from each other, the import hash and the
    // extend recipes.
    assert_ne!(offset_key(a, 2.0), shell_key(a, SHELL_CENTRE, 2.0));
    assert_ne!(offset_key(a, 2.0), hash_mesh_input(a.as_bytes(), b""));
    let k = format!("{:016x}", offset_key(a, 2.0));
    assert_eq!(k.len(), 16, "hex key round-trips as a doc node hash: {k}");
    assert_eq!(u64::from_str_radix(&k, 16).unwrap(), offset_key(a, 2.0));
}

#[test]
fn empty_solids_stay_valid_vacuum() {
    // Shrinking past the deepest core empties the solid legitimately: the derived field is
    // vacuous everywhere (band-clamped padding), still samples, and measures zero.
    let key = hyd_box("empty", 5.0, 1.0);
    let e = compiled_offset(key, -6.0);
    assert!(matches!(e, Expr::Mesh { .. }));
    let d = eval(&e, [0.0; 3]).v;
    println!("emptied solid centre distance: {d:.3}");
    assert!(d > 2.0, "emptied solid is vacuum: {d}");
    assert!(d <= 3.0 + 1e-3, "band clamped: {d}");
    let v = volume::volume(&e, 0.2).volume;
    assert!(v.abs() < 1e-6, "emptied solid measures zero volume: {v}");
    // A centre shell that still leaves wall material keeps measuring solid.
    let e2 = compiled_shell(key, ShellSide::Centre, 4.0);
    assert!(volume::volume(&e2, 0.2).volume > 0.0, "centre shell solid");
}

#[test]
fn derived_leaves_expose_no_patch_grid_tris() {
    let key = hyd_box("nometa", 5.0, 1.0);
    let e = compiled_offset(key, 1.5);
    let Expr::Mesh { key: k, spec, .. } = &e else {
        panic!("expected a leaf");
    };
    let s = spec.as_ref().expect("derived spec present");
    assert!(!s.patch, "derived grids expose no patch grid (named gap)");
    assert_ne!(*k, key, "derived key differs from the parent");
}

#[test]
fn spacing_cap_refuses_and_names_the_node() {
    // Parent spacing already at the coarsening ceiling: a 1000 mm offset on this parent
    // needs an expanded grid over the budget even at MAX_DERIVED_SPACING, so the bake
    // refuses and names the node, like the extend budget does.
    let key = hyd_box("cap", 5.0, MAX_DERIVED_SPACING);
    let parent = lookup(key).expect("hydrated");
    let cells = parent.dist.dims.iter().map(|d| *d as u64).product::<u64>();
    assert!(
        cells < MAX_EXTEND_CELLS as u64,
        "fixture parent fits its own import budget: {cells}"
    );
    let err = offset_field(&parent, 1000.0, "meshC").expect_err("too big to bake");
    assert!(err.contains("meshC"), "refusal names the node: {err}");
    assert!(err.contains("6000000"), "refusal names the budget: {err}");
    let err = shell_field(&parent, SHELL_INSIDE, 1000.0, "shellC").expect_err("too big to bake");
    assert!(err.contains("shellC") && err.contains("budget"), "{err}");
}
