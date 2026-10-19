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

/// The engine's own isotropic law at a density (relative units — the ratios cancel
/// the material modulus).
fn coupon_law_at(family: Family, mode: FillMode, rho: f64) -> f64 {
    fieldsmith_core::fill::law_stiffness(family, mode, rho)
}

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
    let mut ratios: Vec<(String, f64, f64)> = Vec::new();
    let mut cubic: Vec<(String, f64, f64, f64, f64, f64)> = Vec::new();
    for &(fam, mode, label) in &cases {
        let a: CouponResult = coupon::run(&coupon_at(fam, mode, 0.0));
        let b: CouponResult = coupon::run(&coupon_at(fam, mode, 45.0));
        let (e0, e45) = (
            a.e_int.expect("band modulus"),
            b.e_int.expect("band modulus"),
        );
        // The raw ratio mixes whichever density each run achieved; the density-normalized
        // ratio divides each coupon's modulus by the isotropic law at ITS OWN measured
        // density, so a small density difference cannot masquerade as a direction effect
        // (E ~ ρ^n with n 1.2-2.1: a few percent of density is tens of percent of modulus).
        let ratio = e45 / e0;
        let norm45 = e45 / coupon_law_at(fam, mode, b.rho_mesh);
        let norm0 = e0 / coupon_law_at(fam, mode, a.rho_mesh);
        let norm_ratio = norm45 / norm0;
        ratios.push((label.to_string(), ratio, norm_ratio));
        let l45 = 100.0 * (ratio - 1.0).abs();
        // The cubic-tensor fit from the measured triple (E[100] = e0, E[110] = the
        // normalized 45° modulus, ν = the interior Poisson of the 0° run):
        //   S11 = 1/E0, S12 = −ν·S11, S44 = 2(1/E45 − S12)
        // and the Zener anisotropy ratio Z = 2·C44/(C11 − C12) with C = S⁻¹ (Z = 1
        // exactly isotropic). The density-normalized E45 divides by the law at the
        // rotated coupon's own density first.
        let law45 = coupon_law_at(fam, mode, b.rho_mesh);
        let law0 = coupon_law_at(fam, mode, a.rho_mesh);
        let e45_norm = e45 * law0 / law45;
        let e_ref = e0.max(1e-300);
        let s11 = 1.0 / e_ref;
        let nu = a.nu_int.expect("band Poisson");
        let s12 = -nu * s11;
        let s44 = 2.0 * (1.0 / e45_norm - s12);
        // The stiffness constants invert the cubic compliance's (a, b, b) diagonal
        // block in closed form: D = (S11 − S12)(S11 + 2·S12), C11 = (S11 + S12)/D,
        // C12 = −S12/D, C44 = 1/S44. Zener: Z = 2·C44/(C11 − C12) — 1 is isotropic.
        let det = (s11 - s12) * (s11 + 2.0 * s12);
        let c11 = (s11 + s12) / det;
        let c12 = -s12 / det;
        let c44 = 1.0 / s44;
        let zener = 2.0 * c44 / (c11 - c12);
        cubic.push((label.to_string(), s11, s12, s44, c44, zener));
        body.push_str(&format!(
            "- {label}: E*(0°) = {e0:.2} MPa (ρ_mesh {rho0:.4}), E*(45°) = {e45:.2} MPa \
             (ρ_mesh {rho45:.4}); raw ratio {ratio:+.5} (error {l45:.2} %), \
             density-normalized ratio {norm_ratio:+.5}.\n",
            rho0 = a.rho_mesh,
            rho45 = b.rho_mesh,
        ));
    }

    // The pinned bound: the reference values from the release-measured gate; the gate's
    // own body records them. Re-running in release updates the report; the assert holds
    // the pinned claim.
    // The pinned reference = the release-measured ratios: raw, then density-normalized
    // (each coupon's modulus over the engine's isotropic law at its own measured density
    // — the 45° runs' ρ_mesh differ from the 0° runs' by a few percent, and at n =
    // 1.2-2.1 that is tens of percent of modulus). Measured: raw 1.1356 / 1.9237 /
    // 0.8000 / 1.4398 / 2.3037; normalized 1.1358 / 2.0694 / 0.8128 / 1.5673 / 2.2172.
    // The DIRECTION ERROR is the finding: even density-normalized, the isotropic law
    // understates the 45° direction by up to 2.07× (primitive sheet) and overstates by
    // 19 % (diamond sheet) — the normalized ratio is the honest one and the anisotropic
    // extension is owed.
    let reference: [(&str, f64, f64); 5] = [
        ("gyroid sheet", 1.135618, 1.135842),
        ("primitive sheet", 1.923733, 2.069422),
        ("diamond sheet", 0.8000425, 0.8127922),
        ("gyroid network", 1.439837, 1.567271),
        ("diamond network", 2.303728, 2.217194),
    ];
    for (label, ratio, norm) in &ratios {
        let pinned = reference
            .iter()
            .find(|&&(l, _, _)| l == label.as_str())
            .expect("pinned label");
        r.check(fieldsmith_verify::Check::relative(
            &format!("{label}: E*(45°)/E*(0°) raw"),
            pinned.1,
            *ratio,
            "ratio",
            0.05,
        ));
        r.check(fieldsmith_verify::Check::relative(
            &format!("{label}: E*(45°)/E*(0°) density-normalized"),
            pinned.2,
            *norm,
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
                let parts: Vec<String> = ratios
                    .iter()
                    .map(|(l, x, n)| format!("{l} raw {x:+.5} normalized {n:+.5}"))
                    .collect();
                parts.join("; ")
            }
        ),
    )
    .section(
        "Cubic-tensor fits",
        format!(
            "Per family, from the measured (E[100], E[110]-normalized, ν) triple: {cub}",
            cub = {
                let parts: Vec<String> = cubic
                    .iter()
                    .map(|(l, s11, s12, s44, c44, z)| {
                        format!(
                            "{l}: S11 {s11:+.4e}, S12 {s12:+.4e}, S44 {s44:+.4e} MPa⁻¹; \
                             C44 {c44:+.1} MPa; Zener {z:+.4} (1 = isotropic)"
                        )
                    })
                    .collect();
                parts.join(";  ")
            },
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
         - The indicator half of the bullet landed with the graded solve's per-case \
         yield- and buckling-utilization factors (the Gibson-Ashby open-cell strength \
         0.3·σ_ys·φ^1.5 and elastic-collapse 0.05·E_s·φ² laws, unit-gated in the \
         core's `fill_mesh.rs`; constants literature-cited, not fitted — a \
         linear-elastic solve cannot measure plasticity or buckling, so the rigorous \
         buckling route, the geometric-stiffness eigenproblem, remains tracked).",
    );
    r.finish();
}
