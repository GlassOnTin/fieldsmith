//! V6 diagnostic probe (ignored): run the gyroid-sheet N = 3 coupons and dump each coupon's
//! full diagnostic log — the solve residual, the two sides of the equilibrium identity in
//! N·mm, the interior/whole σxx bookkeeping — which the report path only surfaces on refusal.

use fieldsmith_doc::{Family, FillMode};
use fieldsmith_verify::coupon::{self, CouponSpec};

#[test]
#[ignore = "diagnostic probe: cargo test -p fieldsmith-verify --release --test v6_identity_probe -- --ignored --nocapture"]
fn v6_identity_probe() {
    for rho in [0.2, 0.3, 0.4] {
        let s = CouponSpec::new(Family::Gyroid, FillMode::Sheet, rho);
        let row = coupon::run(&s);
        println!("===== coupon rho = {rho} =====");
        for line in &row.log {
            println!("{line}");
        }
        println!(
            "ident_energy_err {:.4e} %  ident_hill_err {:.4e} %  sxx_all {:.5} MPa",
            row.ident_energy_err, row.ident_hill_err, row.sxx_all
        );
    }
}
