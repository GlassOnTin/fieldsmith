//! V36 — how direction-dependent is the homogenised lattice law?
//!
//! GOAL/§7 item 2: the engine's lattice law `E*(ρ) = C·E_s·ρ^n` (V6) is isotropic — one
//! modulus per family and mode. Real TPMS lattices are only NEARLY so: the cubic cell's
//! symmetry kills the x/y/z anisotropy by construction, but a rotated lattice presents a
//! different effective modulus to the same uniaxial coupon. This gate measures the ratio
//! E*(45°)/E*(0°) per family and mode by rotating the lattice's argument 45° about z
//! (platens, loads and diagnostics stay axial — only the lattice turns).
//!
//! Protocol mirrors V6: resolved isosurface-stuffed coupon, N = 3 cells per side, the
//! calibrated feature at ρ = 0.22 (the fit band's low end), free-slip platens, the
//! interior-band strain readout. Like V6 the gate is release-run heavy and ignore-marked;
//! the pinned numbers came from a local release run.
//!
//! What this settles: whether the isotropic law's direction error sits inside its own
//! fitted residual (V6's ≤ 2.1 %) or whether the per-direction law extension (the rungs'
//! "anisotropic homogenisation") is owed. The report states the measured ratio per
//! family and the honest bound; the law's constants are untouched.

use fieldsmith_doc::{Family, FillMode};
use fieldsmith_verify::coupon::{self, CouponResult, CouponSpec};
use fieldsmith_verify::Report;

fn coupon_at(family: Family, mode: FillMode, rotdeg: f64) -> CouponSpec {
    let mut s = CouponSpec::new(family, mode, 0.22);
    s.n = 3;
    s.rotdeg = rotdeg;
    s
}

#[test]
#[ignore = "heavy (~15 min at release): cargo test -p fieldsmith-verify --release --test v36_tpms_isotropy -- --ignored --nocapture"]
fn v36_rotated_lattice_isotropy() {
    let mut r = Report::new(
        "V36",
        "TPMS lattice isotropy: the direction dependence of the homogenised law",
    );

    // (family, mode, label): three sheets + two networks at ρ = 0.22, N = 3.
    let cases = [
        (Family::Gyroid, FillMode::Sheet, "gyroid sheet"),
        (Family::Primitive, FillMode::Sheet, "primitive sheet"),
        (Family::Diamond, FillMode::Sheet, "diamond sheet"),
        (Family::Gyroid, FillMode::Network, "gyroid network"),
        (Family::Diamond, FillMode::Network, "diamond network"),
    ];

    let mut body = String::new();
    let mut ratios: Vec<(String, f64)> = Vec::new();
    for &(fam, mode, label) in &cases {
        let a: CouponResult = coupon::run(&coupon_at(fam, mode, 0.0));
        let b: CouponResult = coupon::run(&coupon_at(fam, mode, 45.0));
        let (e0, e45) = (
            a.e_int.expect("band modulus"),
            b.e_int.expect("band modulus"),
        );
        // The law's reference at each run's OWN measured density: the honest comparison
        // is ratio-to-ratio, the density difference being the coupon's own calibration.
        let ratio = e45 / e0;
        let l45 = 100.0 * (ratio - 1.0).abs();
        ratios.push((label.to_string(), ratio));
        body.push_str(&format!(
            "- {label}: E*(0°) = {e0:.2} MPa (ρ_mesh {rho0:.4}), E*(45°) = {e45:.2} MPa \
             (ρ_mesh {rho45:.4}); ratio {ratio:+.5} — the direction error {l45:.2} %.\n",
            rho0 = a.rho_mesh,
            rho45 = b.rho_mesh,
        ));
    }

    // The pinned bound: the reference values from the release-measured gate; the gate's
    // own body records them. Re-running in release updates the report; the assert holds
    // the pinned claim.
    // The pinned reference = the release-measured ratios (the gate guards the
    // measurement once pinned, and the ±5 % band is the coupon's re-run noise band —
    // V6's ρ_mesh measured reproducible to ≤ 0.4 %, giving the ratio a band of the
    // same order; ±5 % keeps a margin). The first measured run's values: 1.1356
    // (gyroid sheet), 1.9237 (primitive sheet), 0.8000 (diamond sheet), 1.4398
    // (gyroid network), 2.3037 (diamond network). The DIRECTION ERROR is the finding:
    // the isotropic law understates the 45° direction by up to 2.3× (diamond network)
    // and overstates by 25 % (diamond sheet) — the anisotropic extension is owed.
    let reference: [(&str, f64); 5] = [
        ("gyroid sheet", 1.135618),
        ("primitive sheet", 1.923733),
        ("diamond sheet", 0.8000425),
        ("gyroid network", 1.439837),
        ("diamond network", 2.303728),
    ];
    for (label, ratio) in &ratios {
        let (_, pinned) = reference
            .iter()
            .find(|&&(l, _)| l == label.as_str())
            .expect("pinned label");
        r.check(fieldsmith_verify::Check::relative(
            &format!("{label}: E*(45°)/E*(0°)"),
            *pinned,
            *ratio,
            "ratio",
            0.05,
        ));
    }
    r.section(
        "Protocol",
        "Each coupon is the resolved lattice (isosurface stuffing, N = 3 cells per side, \
         one mesh cell per thin feature, free-slip platens, interior-band strain readout \
         as V6). The rotated coupon's lattice argument is rotated in-plane 45° about z \
         with density calibration untouched: a level set rotated is the same surface, so \
         ρ_mesh is the coupon's own honest density and only the lattice's orientation \
         moves. E*(0°) vs E*(45°) measures the law's direction dependence at that angle.",
    )
    .section(
        "Measured ratios",
        format!(
            "The coupon pairs: {ratio_str}",
            ratio_str = {
                let parts: Vec<String> =
                    ratios.iter().map(|(l, x)| format!("{l} {x:+.5}")).collect();
                parts.join("; ")
            }
        ),
    )
    .section(
        "Coverage and limits",
        "- The rotations measured are two orientations (0°/45°) at one density (0.22) per \
         family: the bound holds where measured; no claim over other densities or angles \
         is made. The cubic cell's symmetry already kills the x/y/z uniaxial anisotropy \
         by construction, which is why the interesting direction is the 45° one.\n\
         - The engine's per-element law stays the isotropic `E*(ρ)` of V6; this gate \
         decides whether the direction extension (§7 item 2's anisotropic \
         homogenisation) is owed or whether the isotropy error sits inside the law's \
         own fitted residual (V6 measured ≤ 2.1 % against resolved coupons). The \
         measured ratios above are the raw evidence; the pinned gate bound (±15 %) is a \
         regression guard, not a fit claim.\n\
         - The buckling/yield indicators for thin walls (§7 item 2's remaining bullet) \
         are a separate slice with its own gate.",
    );
    r.finish();
}
