//! V4 — field volume against closed form.
//!
//! The volume a user sees (`volume_auto`, also used by `fieldsmith volume` and the app's mass
//! readout) is integrated over the signed-distance field with sub-voxel occupancy. Two shapes
//! with exact answers are checked: a sphere, and a box with a through-bore.

use fieldsmith_field::{compile_body, volume};
use fieldsmith_verify::{doc, example, Check, Report};
use std::f64::consts::PI;

#[test]
fn v4_field_volume() {
    let sphere = doc("sphere s r=10\n");
    let v_sphere = volume::volume_auto(&compile_body(&sphere));
    let ref_sphere = 4.0 / 3.0 * PI * 1000.0;

    let bored = example("box-minus-cylinder.fsm");
    let v_bored = volume::volume_auto(&compile_body(&bored));
    let ref_bored = 40.0 * 30.0 * 20.0 - PI * 8.0 * 8.0 * 20.0;

    let mut r = Report::new("V4", "Field volume against closed form");
    r.check(Check::relative(
        "Sphere r = 10 mm",
        ref_sphere,
        v_sphere.volume,
        "mm³",
        0.005,
    ));
    r.check(Check::relative(
        "Box 40 × 30 × 20 minus bore r = 8",
        ref_bored,
        v_bored.volume,
        "mm³",
        0.005,
    ));
    r.section(
        "Method",
        format!(
            "The document is compiled to its signed-distance field and integrated on a voxel grid \
             (`fieldsmith_field::volume::volume_auto`, the same call the app and CLI use). Each voxel \
             contributes a linear occupancy estimate from the field value and gradient at its centre, \
             `clamp(0.5 − d/(|∇d|·h·‖n‖₁), 0, 1)`; blocks of 8³ voxels far from the surface are \
             classified wholesale.\n\n\
             | Shape | Grid spacing | Self-estimated error (h vs 2h) |\n| :-- | --: | --: |\n\
             | Sphere | {:.4} mm | {:.3} mm³ |\n| Bored box | {:.4} mm | {:.3} mm³ |",
            v_sphere.spacing, v_sphere.error, v_bored.spacing, v_bored.error
        ),
    );
    r.section(
        "References",
        "Sphere: V = 4/3 π r³ = 4188.790 mm³. Box minus a coaxial through-bore of the box height: \
         V = 40·30·20 − π·8²·20 = 19 978.76 mm³. Document: `examples/box-minus-cylinder.fsm`.",
    );
    r.finish();
}
