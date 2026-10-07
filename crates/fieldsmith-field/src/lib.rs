//! Field IR for Fieldsmith.
//!
//! A document compiles to an [`Expr`] tree whose evaluation yields a dual sample
//! `D = (signed distance, gradient)`. The same tree is emitted as GLSL ([`glsl`]) for the live
//! preview; a shared verification case checks the two back-ends agree.

pub mod dual;
pub mod expr;
pub mod glsl;
pub mod lattice;
pub mod meshfield;
pub mod prim;
pub mod volume;

pub use dual::{D, V3};
pub use expr::{bounds, compile_body, eval, Aabb, Expr};

#[cfg(test)]
mod tests {
    use super::*;
    use fieldsmith_doc::fsm;

    fn doc(src: &str) -> fieldsmith_doc::Document {
        fsm::parse(&format!("fieldsmith 1\n{src}")).unwrap()
    }

    #[test]
    fn subtract_makes_a_hole_and_add_after_subtract_restores() {
        let d = doc("box b size=(10, 10, 2)\ncylinder h r=2 h=4 subtract\n");
        let e = compile_body(&d);
        assert!(
            eval(&e, [0.0, 0.0, 0.0]).v > 0.0,
            "inside the hole is empty"
        );
        assert!(eval(&e, [4.0, 4.0, 0.0]).v < 0.0);
        let d = doc("box b size=(10, 10, 2)\ncylinder h r=2 h=4 subtract\nsphere s r=1\n");
        assert!(eval(&compile_body(&d), [0.0, 0.0, 0.0]).v < 0.0);
    }

    #[test]
    fn shell_sides_place_the_wall_inside_outside_or_centred() {
        // A 20 mm cube shelled 2 mm: wall spans [8, 10], [10, 12] or [9, 11] from the centre.
        for (side, inner, outer) in [
            ("inside", 8.0, 10.0),
            ("outside", 10.0, 12.0),
            ("centre", 9.0, 11.0),
        ] {
            let d = doc(&format!(
                "group g {{\n  box b size=(20, 20, 20)\n  shell 2 {side}\n}}\n"
            ));
            let e = compile_body(&d);
            let mid = 0.5 * (inner + outer);
            assert!(
                eval(&e, [mid, 0.0, 0.0]).v < 0.0,
                "{side}: wall at x = {mid}"
            );
            assert!(
                (eval(&e, [inner - 0.5, 0.0, 0.0]).v - 0.5).abs() < 1e-9,
                "{side}: hollow inside"
            );
            assert!(
                (eval(&e, [outer + 0.5, 0.0, 0.0]).v - 0.5).abs() < 1e-9,
                "{side}: outer face at {outer}"
            );
        }
    }

    #[test]
    fn coplanar_seam_stays_sharp_but_corner_is_filleted() {
        // Two boxes side by side share the top plane z = 1: blending must not bulge the seam.
        let d = doc(
            "box a size=(10, 10, 2) at=(-5, 0, 0)\nbox b size=(10, 10, 2) at=(5, 0, 0) blend=4\n",
        );
        let e = compile_body(&d);
        assert!((eval(&e, [0.0, 0.0, 1.5]).v - 0.5).abs() < 1e-9);
        // An L: concave corner gets material added by the fillet.
        let d = doc(
            "box a size=(10, 10, 2) at=(0, 0, 1)\nbox b size=(10, 2, 10) at=(0, 4, 5) blend=4\n",
        );
        let e = compile_body(&d);
        let corner = [0.0, 2.6, 2.6];
        let sharp =
            doc("box a size=(10, 10, 2) at=(0, 0, 1)\nbox b size=(10, 2, 10) at=(0, 4, 5)\n");
        assert!(eval(&e, corner).v < 0.0 && eval(&compile_body(&sharp), corner).v > 0.0);
    }

    #[test]
    fn transforms_and_patterns() {
        let d = doc("group g at=(10, 0, 0) {\n  sphere s r=1 at=(2, 0, 0)\n  circular 4 axis=z angle=360\n}\n");
        let e = compile_body(&d);
        for p in [
            [12.0, 0.0, 0.0],
            [10.0, 2.0, 0.0],
            [8.0, 0.0, 0.0],
            [10.0, -2.0, 0.0],
        ] {
            assert!(
                eval(&e, p).v.abs() < 1e-9 + 1.0 && eval(&e, p).v < 0.0,
                "{p:?}"
            );
        }
        let d = doc("group g {\n  box b size=(1, 1, 1) at=(3, 0, 0)\n  mirror x\n  linear 3 step=(0, 5, 0)\n}\n");
        let e = compile_body(&d);
        assert!(eval(&e, [-3.0, 10.0, 0.0]).v < 0.0);
        let b = bounds(&e);
        assert!((b.lo[0] + 3.5).abs() < 1e-9 && (b.hi[1] - 10.5).abs() < 1e-9);
    }

    #[test]
    fn identify_surface_and_faces() {
        let d = doc("box base size=(20, 20, 4)\ncylinder hole r=3 h=10 subtract\n");
        let (id, face, _) = expr::identify_surface(&d, [3.0, 0.0, 0.5]).unwrap();
        assert_eq!((id.as_str(), face), ("hole", fieldsmith_doc::Face::Side));
        let (id, face, _) = expr::identify_surface(&d, [7.0, 7.0, 2.0]).unwrap();
        assert_eq!((id.as_str(), face), ("base", fieldsmith_doc::Face::Pz));
    }

    #[test]
    fn blend_is_a_true_fillet_without_end_bulge() {
        let d = doc("box base size=(60, 40, 8) at=(30, 20, 4)\nbox upright size=(60, 8, 40) at=(30, 4, 20) blend=8\n");
        let e = compile_body(&d);
        // On the ideal R8 fillet surface along the diagonal, mid-span.
        let s = 8.0 + 8.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
        assert!(expr::eval(&e, [30.0, s, s]).v.abs() < 1e-9);
        // Just past the end faces the fillet must not continue.
        for x in [-0.3, 60.3] {
            assert!(
                expr::eval(&e, [x, s - 0.5, s - 0.5]).v > 0.0,
                "fillet bulges past end at x={x}"
            );
        }
        let ideal = 2.0 * 60.0 * 40.0 * 8.0 - 60.0 * 8.0 * 8.0
            + 60.0 * 64.0 * (1.0 - std::f64::consts::FRAC_PI_4);
        let v = volume::volume(&e, 0.2).volume;
        assert!(
            (v - ideal).abs() / ideal < 5e-4,
            "volume {v} vs ideal {ideal}"
        );
    }

    #[test]
    fn raymarch_hits_top_face() {
        let d = doc("box b size=(10, 10, 2)\n");
        let e = compile_body(&d);
        let t = expr::raymarch(&e, [0.0, 0.0, 10.0], [0.0, 0.0, -1.0], 100.0).unwrap();
        assert!((t - 9.0).abs() < 1e-3);
    }

    #[test]
    fn glsl_generation_covers_every_node_type() {
        let d = doc("box a size=(1, 2, 3) round=0.1\ncylinder c r=1 h=2 subtract blend=0.2\ngroup g rotate=(10, 20, 30) intersect {\n  sphere s r=4\n  capsule k r=1 length=3\n  torus t major=3 minor=1\n  extrude e h=2 profile=[(0, 0), (2, 0, 0.5), (2, 2), (0, 2)]\n  shell 0.5 inside\n  offset 0.1\n  mirror y\n  linear 2 step=(1, 0, 0)\n  circular 3 axis=x angle=90\n}\n");
        let g = glsl::generate(&compile_body(&d));
        for needle in [
            "fs_box",
            "fs_cylinder",
            "fs_rmin(",
            "fs_cgate(",
            "fs_sphere",
            "fs_capsule",
            "fs_torus",
            "fs_seg_arc",
            "fs_rot(0",
            "vec4 fs_field",
        ] {
            assert!(g.source.contains(needle), "missing {needle}");
        }
    }
}
