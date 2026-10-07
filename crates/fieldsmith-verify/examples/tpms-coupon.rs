//! Measurement harness CLI: apparent (homogenised) modulus of a resolved TPMS unit-cell coupon
//! under uniaxial compression with free-slip platens. This produces the reference data behind the
//! homogenised stiffness laws used by the fill engine (`fieldsmith-core/src/fill.rs`) and checked
//! by V6 — the constants are *measured*, not assumed. The measurement itself lives in
//! `fieldsmith_verify::coupon` (see `crates/fieldsmith-verify/src/coupon.rs`); this example
//! parses the sweep's flags and prints the run's diagnostic block, one run per invocation.
//!
//! Coupon: an N³ block of unit cells of size `cell`, meshed by isosurface stuffing at
//! h = hmul · d, where d is the mode's thin-feature size (mm): sheet d = t (wall thickness
//! from the density calibration); network d = 2R_thin, with R_thin the thinnest strut radius,
//! measured from the field itself by a chamfer distance transform over one unit cell (the
//! network parameter is a level offset of the gradient-floored distance field, so it is *not*
//! a thickness). Sheet default hmul = 1 (one cell per wall); network default hmul = 0.5 (one
//! cell per strut radius: at one per diameter the curved struts are partly eaten, measured
//! ρ_mesh 0.737 of nominal and a solver refusal on the degraded gyroid ρ = 0.2 mesh).
//! `--shift s` offsets the lattice pattern by s mm inside the coupon (the load face keeps the
//! box): defuses the tie-alignment lottery that can otherwise leave the coupon face without
//! any planar load patches (measured both on gyroid sheet ρ = 0.6 and diamond sheet ρ = 0.3).
//! Loading: traction −σ·nₓ
//! on the material patches of the x=L face (the void parts of the face are not part of the
//! mesh), ux = 0 on the x=0 patches; y and z free (free-slip platens). Two point constraints
//! (uy=uz at the origin corner, uy on the y=0 face near the far x edge, both ≥ 1 cell from the
//! strain band) remove the rigid modes the platens leave; their local reactions decay away.
//!
//! Measurement: E* = Σ / ε with Σ the macroscopic section stress (total applied force over the
//! L² cross-section) and ε the macro strain, measured as the volume-weighted least-squares
//! slope of ux against x over the interior band (x = band·cell … L − band·cell, --band default
//! 1). Point probes are unusable here: at low density most probe points land in void. The
//! volume weighting uses the mesh itself, so it measures the material that is actually present.
//! E*_full (slope over the whole coupon) is reported alongside to show the boundary-layer effect
//! the interior band removes. Saint-Venant predicts the loaded face's non-periodic layer decays
//! like e^(−2π·m) over m cells, so the interior band at 1 cell carries a ≤ 0.5 % residual; the
//! N = 3 → 4 drift quantifies that directly. --band exists for the primitive network, where the
//! platen contacts through the first stratum are sparse (planar load area ~ 0.6–1.4 mm² of a
//! 36 mm² face) and the 1-cell band measured a 44–50 % E_int/E_full gap; widening the band
//! keeps strain measurement clear of the decay zone.
//!
//! Also reported: achieved volume fraction ρ_mesh (the law must be fit against *measured*
//! density — |s| is not an exact distance, so the actual wall volume differs slightly from the
//! nominal), mesh quality, and the volume-weighted interior mean of σxx as an equilibrium check
//! (should sit close to Σ for a periodic interior).
//!
//! One run per invocation, so sweeps are a shell loop:
//! `for rho in 0.2 0.3 0.4; do for n in 2 3 4; do cargo run -p fieldsmith-verify --release \
//!  --example tpms-coupon -- --rho $rho --n $n; done; done`

use fieldsmith_doc::{Family, FillMode};
use fieldsmith_verify::coupon::{self, CouponSpec};

fn main() {
    let get = |name: &str, default: &str| -> String {
        let mut found = None;
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            if a == format!("--{name}") {
                found = it.next();
            }
        }
        found.unwrap_or_else(|| default.into())
    };
    let family = match get("family", "gyroid").as_str() {
        "primitive" => Family::Primitive,
        "diamond" => Family::Diamond,
        _ => Family::Gyroid,
    };
    let mode = match get("mode", "sheet").as_str() {
        "network" => FillMode::Network,
        _ => FillMode::Sheet,
    };
    let h: Option<f64> = get("h", "").parse().ok();
    let hmul_default = coupon::default_hmul(mode).to_string();
    let mut spec = CouponSpec::new(family, mode, get("rho", "0.3").parse().expect("rho"));
    spec.cell = get("cell", "2.0").parse().expect("cell");
    spec.n = get("n", "3").parse().expect("n");
    spec.hmul = get("hmul", &hmul_default).parse().expect("hmul");
    spec.shift = get("shift", "0").parse().expect("shift");
    spec.h = h;
    spec.band = get("band", "1.0").parse().expect("band");
    spec.young = get("young", "1000").parse().expect("young");
    spec.nu = get("nu", "0.3").parse().expect("nu");
    spec.sigma = get("sigma", "10").parse().expect("sigma");

    let r = coupon::run(&spec);
    for line in &r.log {
        println!("{line}");
    }
}
