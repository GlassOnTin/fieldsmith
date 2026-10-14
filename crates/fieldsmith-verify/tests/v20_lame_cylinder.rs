//! V20 — the Lame-class thick cylinder under a through-wall temperature gradient.
//!
//! The full pipeline (`analyse`) drives an octant of a bore-20/pipe-40 mm cylinder
//! (r ∈ [10, 20], z ∈ [0, 120] mm): bore wall 90 °C, outer wall 20 °C, `solve
//! tref=20`, so ΔT = 70 K at the bore falling logarithmically to 0 at the outside —
//! hoop tension at the bore, the classic pressurised-pipe-shaped thermal stress state.
//!
//! Geometry: the outer cylinder minus the bore, minus two half-space boxes keeping
//! x ≥ 0 and y ≥ 0 — an octant whose three cut planes are exact symmetry planes of
//! the solution. Single-component supports on them (x=0 face fixed=x, y=0 face
//! fixed=y, z=0 face fixed=z — the cylinder's own bottom) kill all six rigid modes
//! — each rotation violates at least one face constraint on a surface of positive
//! measure — and every one is satisfied exactly by the exact field, which has
//! u_x = 0 at x=0, u_y = 0 at y=0, u_z = c0·z = 0 at z=0. The cut planes are also
//! exactly insulated for the conduction problem, consistent with the full cylinder's
//! symmetry. The free-end force balance (∫σzz dA = 0) is carried by the reference's
//! generalized-plane-strain constant c0, not by any constraint, so the model is the
//! whole physics: L/ro = 6 with wall 10 mm means the free-end Saint-Venant decay at
//! mid-length is e^-9 or smaller.
//!
//! Reference: generalized plane strain, u = (u_r(r)·cosθ, u_r(r)·sinθ, c0·z), with
//! the thermal strain in the constitutive law (σ = C:B − (3λ+2μ)·α·ΔT·I) and
//! ΔT(r) = T(r) − tref. With f(r) = ∫ ΔT(s)·s ds from the bore (f' = r·ΔT), this
//! gives u_r = C1·r + C2/r + γα·f(r)/r, γ = (2μ+3λ)/(2μ+λ), and
//!   σrr  = 2(μ+λ)C1 − 2μ(C2 + γα·f)/r² + λ·c0
//!   σθθ  = 2(μ+λ)C1 + 2μ(C2 + γα·f)/r² + λ·c0 − 2μγα·ΔT(r)
//!   σzz  = (2μ+λ)·c0 + 2λ·C1 − 2μγα·ΔT(r)
//! The three constants come from σrr = 0 on both walls and the free-end balance
//! ∫σzz dA = 0 over the annulus:
//!   C2 = γαF·ri²/(ro² − ri²),  F = f(ro)
//!   C1 = γαF(2μ − λ)/((2μ+3λ)(ro² − ri²))
//!   c0 = [4μγαF/(ro² − ri²) − 2λC1]/(2μ + λ)
//! The test first asserts the uniform-ΔT anchor of this algebra — u_r = αΔT·r,
//! σ ≡ 0, c0 = αΔT — before using any of it, so a transcription slip cannot produce
//! a quietly wrong reference. Probes sit on the octant's θ = 45° mid-face ray at the
//! axial mid-length (z = 60): mid-face is bulk, while the mesh's worst boundary-data
//! zone is the cut-plane junction (chime corners standing off both named carriers by
//! up to ~0.9 mm, carrying ≈ |∇T|·δ ≈ 10 K/mm · δ of Dirichlet offset — the conduction
//! fidelity row states the band that this leaves). And a θ = 45° ray makes the
//! (rr, θθ, zz) → (xx, yy) rotation exact: σxx = σyy = (σrr + σθθ)/2, u_x = u_r/√2,
//! with ρ = r on the ray by construction.

use fieldsmith_core::{
    analyse,
    probe::{probe, Locator},
    Resolution,
};
use fieldsmith_solve::assemble::{element_stiffness, RefData};
use fieldsmith_solve::{element_thermal_integrals, ThermalRefData};
use fieldsmith_verify::{doc, Check, Report};
use std::time::Instant;

const E: f64 = 70_000.0; // MPa
const NU: f64 = 0.33;
const ALPHA: f64 = 2.3e-5; // 1/K
const RI: f64 = 10.0; // bore radius, mm
const RO: f64 = 20.0; // outer radius, mm
const LEN: f64 = 120.0; // cylinder height, mm
const T_IN: f64 = 90.0; // bore wall, °C
const T_OUT: f64 = 20.0; // outer wall, °C
const T_REF: f64 = 20.0; // stress-free reference, °C

/// Probe ray: (r/√2, r/√2, 60) — the θ = 45° mid-face line at the axial mid-length.
/// Mid-face keeps the probes out of the cut-plane junction zone, and the rotation from
/// the reference's (σrr, σθθ) is exact there.
const PROBE_C: f64 = std::f64::consts::FRAC_1_SQRT_2;
const PROBE_Z: f64 = 60.0;
const PROBE_RS: [f64; 3] = [10.5, 15.0, 19.5]; // near bore, mid-wall, near outside

/// Probe point at radius `rp` on the ray.
fn probe_p(rp: f64) -> [f64; 3] {
    [rp * PROBE_C, rp * PROBE_C, PROBE_Z]
}

/// Octant anchor bands, measured 2026-10-14 at h = 3 (referee). The exact solution
/// u = αΔT·(x, y, z) lies in the P2 space and satisfies the uniform boundary data, so
/// the in-space identity K·u_e = b holds row-for-row to assembly roundoff and the
/// pipeline's solution satisfies its own rows to the PCG tolerance; the bands sit many
/// orders above the measured residuals and far below what a real defect moves. The
/// u_x probes sit on the unwarped mid-face ray, where the P2 interpolation of the
/// exact linear field is exact (measured 1.3e-12 mm).
const ANCHOR_T_HI: f64 = 1e-2; // °C — a uniform Dirichlet field reproduced exactly
const ANCHOR_STRESS_HI: f64 = 0.1; // MPa — σ ≡ 0 in the exact field
const ANCHOR_UR_HI: f64 = 1e-6; // mm — probe roundoff; measured 1.3e-12 at h = 3
const ANCHOR_RUE_HI: f64 = 1e-8; // N — assembly roundoff of the K·u_e = b identity
const ANCHOR_RUH_HI: f64 = 2e-5; // N — PCG rel tolerance against ‖b‖ ≈ 2.1e4 N
const ANCHOR_DU_HI: f64 = 1e-8; // mm — nodal u_h vs the exact field, free components

/// Lame-class generalized-plane-strain reference for wall ΔTs on an annulus, in
/// ΔT = T − tref terms.
struct Lame {
    lam: f64,
    mu: f64,
    gamma: f64,
    alpha: f64,
    ri: f64,
    l: f64, // ln(ro/ri)
    dt_in: f64,
    dt_out: f64,
    c1: f64,
    c2: f64,
    c0: f64,
}

impl Lame {
    fn new(
        young: f64,
        poisson: f64,
        alpha: f64,
        dt_in: f64,
        dt_out: f64,
        ri: f64,
        ro: f64,
    ) -> Lame {
        let m = fenics_assembly::elasticity::ElasticMaterial::new(young, poisson);
        let (lam, mu) = (m.lame_lambda(), m.lame_mu());
        let l = (ro / ri).ln();
        let mut s = Lame {
            lam,
            mu,
            gamma: (2.0 * mu + 3.0 * lam) / (2.0 * mu + lam),
            alpha,
            ri,
            l,
            dt_in,
            dt_out,
            c1: 0.0,
            c2: 0.0,
            c0: 0.0,
        };
        let f_ro = s.f(ro);
        let gaf_ro = s.gamma * alpha * f_ro;
        s.c2 = gaf_ro * ri * ri / (ro * ro - ri * ri);
        s.c1 = gaf_ro * (2.0 * mu - lam) / ((2.0 * mu + 3.0 * lam) * (ro * ro - ri * ri));
        s.c0 = (4.0 * mu * gaf_ro / (ro * ro - ri * ri) - 2.0 * lam * s.c1) / (2.0 * mu + lam);
        s
    }

    /// ΔT(r), the wall data interpolated logarithmically (the harmonic profile).
    fn dt(&self, r: f64) -> f64 {
        self.dt_in + (self.dt_out - self.dt_in) * (r / self.ri).ln() / self.l
    }

    /// f(r) = ∫_{ri}^{r} ΔT(s)·s ds; f' = r·ΔT(r), f(ri) = 0.
    fn f(&self, r: f64) -> f64 {
        self.dt_in * (r * r - self.ri * self.ri) / 2.0
            + (self.dt_out - self.dt_in) / self.l
                * (r * r / 2.0 * (r / self.ri).ln() - r * r / 4.0 + self.ri * self.ri / 4.0)
    }

    /// γα·f(r), the repeated factor in u_r and the stresses.
    fn gaf(&self, r: f64) -> f64 {
        self.gamma * self.alpha * self.f(r)
    }

    fn u_r(&self, r: f64) -> f64 {
        self.c1 * r + self.c2 / r + self.gaf(r) / r
    }

    fn s_rr(&self, r: f64) -> f64 {
        2.0 * (self.mu + self.lam) * self.c1 - 2.0 * self.mu * (self.c2 + self.gaf(r)) / (r * r)
            + self.lam * self.c0
    }

    fn s_tt(&self, r: f64) -> f64 {
        2.0 * (self.mu + self.lam) * self.c1
            + 2.0 * self.mu * (self.c2 + self.gaf(r)) / (r * r)
            + self.lam * self.c0
            - 2.0 * self.mu * self.gamma * self.alpha * self.dt(r)
    }

    fn s_zz(&self, r: f64) -> f64 {
        (2.0 * self.mu + self.lam) * self.c0 + 2.0 * self.lam * self.c1
            - 2.0 * self.mu * self.gamma * self.alpha * self.dt(r)
    }
}

/// Absolute temperature profile between the walls (°C).
fn t_profile(r: f64) -> f64 {
    T_OUT + (T_IN - T_OUT) * (RO / r).ln() / (RO / RI).ln()
}

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Max |component| over nodal stresses of a case.
fn max_stress_component(a: &fieldsmith_core::Analysis, case: usize) -> f64 {
    a.cases[case]
        .stress
        .iter()
        .flat_map(|s| s.iter())
        .fold(0.0f64, |m, v| m.max(v.abs()))
}

fn fixture() -> String {
    format!(
        "name \"V20 Lame cylinder\"\n\
         custommaterial heat e={E} nu={NU} a={ALPHA} yield=250 density=2.7\n\
         material heat\n\
         cylinder outer r={RO} h={LEN} at=(0, 0, 60)\n\
         cylinder bore r={RI} h={LEN} at=(0, 0, 60) subtract\n\
         box cutx size=(90, 45, 240) at=(-45, 0, 0) subtract\n\
         box cuty size=(45, 90, 240) at=(0, -45, 0) subtract\n\
         region skin_in = bore.side\n\
         region skin_out = outer.side\n\
         region xplane = cutx.px\n\
         region yplane = cuty.py\n\
         region zbase = outer.bottom\n\
         support kx xplane fixed=x\n\
         support ky yplane fixed=y\n\
         support kz zbase fixed=z\n\
         temperature hot skin_in temp={T_IN} case=\"Lame\"\n\
         temperature cold skin_out temp={T_OUT} case=\"Lame\"\n\
         solve tref={T_REF}\n"
    )
}

/// Run the octant at element size `h`; return (analysis, case index, wall-clock ms).
fn run(h: f64) -> (fieldsmith_core::Analysis, usize, f64) {
    let t0 = Instant::now();
    let d = doc(&fixture());
    let a = analyse(&d, Resolution::Size(h), clock).expect("analysis runs");
    let case = a
        .cases
        .iter()
        .position(|c| c.name == "Lame")
        .unwrap_or_else(|| {
            panic!(
                "Lame case ran (cases: {:?})",
                a.cases.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
            )
        });
    (a, case, t0.elapsed().as_secs_f64() * 1000.0)
}

#[test]
fn v20_lame_cylinder() {
    // --- Reference algebra guard: uniform ΔT must reproduce free expansion exactly ----
    // u_r = α·ΔT·r, σ ≡ 0, c0 = α·ΔT. Asserting this before using the reference means
    // a transcription slip in C1/C2/c0 or the stress formulas fails here, loudly,
    // rather than quietly corrupting every check.
    let dt0 = 40.0;
    let anchor = Lame::new(E, NU, ALPHA, dt0, dt0, RI, RO);
    let alpha_dt0 = ALPHA * dt0;
    assert!(
        (anchor.c0 - alpha_dt0).abs() < 1e-9 * alpha_dt0,
        "anchor c0 = {} vs αΔT = {}",
        anchor.c0,
        alpha_dt0
    );
    for r in [11.0, 15.0, 19.0] {
        let u_err = (anchor.u_r(r) - alpha_dt0 * r).abs();
        assert!(
            u_err < 1e-9 * alpha_dt0 * r,
            "anchor u_r({r}) err {u_err:.3e}"
        );
        for (k, s) in [anchor.s_rr(r), anchor.s_tt(r), anchor.s_zz(r)]
            .iter()
            .enumerate()
        {
            assert!(
                s.abs() < 1e-6 * E * alpha_dt0,
                "anchor stress comp {k} at r={r}: {s:.3e}"
            );
        }
    }

    let mut r = Report::new("V20", "Lame-class thermal cylinder — octant, gradient ΔT");
    let ref_model = Lame::new(E, NU, ALPHA, T_IN - T_REF, T_OUT - T_REF, RI, RO);

    // --- Octant anchor: uniform ΔT on the whole surface must give u_r = αΔT·r, σ ≡ 0 ---
    // V19 proved the coupling chain on a box; this proves the octant's curved-wall mesh,
    // region selection, and six-mode constraint set before the gradient case is judged.
    {
        // The two temperature statements share case "Anchor" at the same value, so the
        // whole surface (bore + outer) is 60 °C; the cut planes and ends stay insulated
        // — constant data, T ≡ 60, the uniform-expansion anchor.
        let d = doc(&format!(
            "name \"V20 anchor\"\n\
             custommaterial heat e={E} nu={NU} a={ALPHA} yield=250 density=2.7\n\
             material heat\n\
             cylinder outer r={RO} h={LEN} at=(0, 0, 60)\n\
             cylinder bore r={RI} h={LEN} at=(0, 0, 60) subtract\n\
             box cutx size=(90, 45, 240) at=(-45, 0, 0) subtract\n\
             box cuty size=(45, 90, 240) at=(0, -45, 0) subtract\n\
             region skin_in = bore.side\n\
             region skin_out = outer.side\n\
             region xplane = cutx.px\n\
             region yplane = cuty.py\n\
             region zbase = outer.bottom\n\
             support kx xplane fixed=x\n\
             support ky yplane fixed=y\n\
             support kz zbase fixed=z\n\
             temperature warm skin_in temp=60 case=\"Anchor\"\n\
             temperature warm2 skin_out temp=60 case=\"Anchor\"\n\
             solve tref={T_REF}\n"
        ));
        let a = analyse(&d, Resolution::Size(3.0), clock).expect("anchor runs");
        let case = a
            .cases
            .iter()
            .position(|c| c.name == "Anchor")
            .expect("anchor case");
        let t = a.cases[case].temperature.as_ref().expect("anchor T");
        let t_dev = t.iter().fold(0.0f64, |m, ti| m.max((ti - 60.0).abs()));
        let stress_max = max_stress_component(&a, case);
        let loc = Locator::new(&a);
        let ur_ref = ALPHA * (60.0 - T_REF);
        let ur_errs: Vec<f64> = PROBE_RS
            .iter()
            .map(|&rp| {
                let pv = probe(&a, &loc, Some(case), probe_p(rp)).expect("anchor probe");
                // On the 45° ray the exact field's x-component is αΔT·x = αΔT·r·cos45.
                (pv.displacement[0] - ur_ref * rp * PROBE_C).abs()
            })
            .collect();
        let ur_err = ur_errs.iter().copied().fold(0.0f64, f64::max);
        for (rp, e) in PROBE_RS.iter().zip(&ur_errs) {
            println!("V20 anchor probe r={rp}: |u_x − αΔT·x| = {e:.3e} mm");
        }
        println!(
            "V20 anchor: T dev {t_dev:.3e} °C, max stress component {stress_max:.3e} MPa, \
             max |u_probe − αΔT·r| = {ur_err:.3e} mm, αΔT·r(10.5) = {:.3e} mm",
            ur_ref * 10.5
        );
        // The in-space identity: is the exact solution a solution of the ASSEMBLED
        // system? Per element the row identity K_e·u_exact = β·∫ΔT_h·∇φa must hold
        // row-for-row (the P2 interpolant of the linear field reproduces it on the affine
        // map; both integrands are quadrature-exact), so a nonzero free-row residual
        // names where the chain breaks. Rows whose node is on a support plane (the
        // coordinate of the fixed component is exactly 0) carry reaction forces by
        // design and are excluded.
        let rd = RefData::new();
        let trd = ThermalRefData::new();
        let em = fenics_assembly::elasticity::ElasticMaterial::new(E, NU);
        let beta = (3.0 * em.lame_lambda() + 2.0 * em.lame_mu()) * ALPHA;
        let u_h = a.cases[case].u.clone();
        let ue: Vec<f64> = a
            .mesh
            .nodes
            .iter()
            .flat_map(|p| [ur_ref * p[0], ur_ref * p[1], ur_ref * p[2]])
            .collect();
        let is_fixed_c = |n: usize, c: usize| a.mesh.nodes[n][c].abs() < 1e-9;
        let dt_unif = vec![40.0f64; a.mesh.nodes.len()];
        let (mut r_ue, mut r_uh, mut f_row) = (
            vec![0.0f64; a.mesh.ndofs()],
            vec![0.0f64; a.mesh.ndofs()],
            vec![0.0f64; a.mesh.ndofs()],
        );
        for e in 0..a.mesh.tets.len() {
            let ke = element_stiffness(&a.mesh, e, E, NU, &rd);
            let le = element_thermal_integrals(&a.mesh, e, &dt_unif, &trd);
            let t10 = a.mesh.tets[e];
            for a_ in 0..10 {
                let rn = t10[a_] as usize;
                for i in 0..3 {
                    let fl = beta * le[a_][i];
                    f_row[rn * 3 + i] += fl;
                    let mut acc_ue = -fl;
                    let mut acc_uh = -fl;
                    for b_ in 0..10 {
                        let blk = &ke[a_ * 10 + b_];
                        let cn = t10[b_] as usize * 3;
                        acc_ue += blk[i * 3] * ue[cn]
                            + blk[i * 3 + 1] * ue[cn + 1]
                            + blk[i * 3 + 2] * ue[cn + 2];
                        acc_uh += blk[i * 3] * u_h[cn]
                            + blk[i * 3 + 1] * u_h[cn + 1]
                            + blk[i * 3 + 2] * u_h[cn + 2];
                    }
                    r_ue[rn * 3 + i] += acc_ue;
                    r_uh[rn * 3 + i] += acc_uh;
                }
            }
        }
        let mut rows: Vec<(f64, f64, usize, usize)> = Vec::new();
        for n in 0..a.mesh.nodes.len() {
            for c in 0..3 {
                if is_fixed_c(n, c) {
                    continue;
                }
                rows.push((r_ue[3 * n + c].abs(), f_row[3 * n + c].abs(), n, c));
            }
        }
        rows.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
        let r_uh_max = rows
            .iter()
            .map(|(_, _, n, c)| r_uh[3 * n + c].abs())
            .fold(0.0f64, f64::max);
        let bnorm = f_row.iter().map(|v| v * v).sum::<f64>().sqrt();
        let r_ue_max = rows[0].0;
        let du_free = rows
            .iter()
            .map(|(_, _, n, c)| (u_h[3 * n + *c] - ue[3 * n + *c]).abs())
            .fold(0.0f64, f64::max);
        println!(
            "V20 anchor: T dev {t_dev:.3e} °C, stress {stress_max:.3e} MPa, u_err {ur_err:.3e} mm; \
             identity ‖b‖ {bnorm:.3e} N, max free |r_ue| {r_ue_max:.3e} N, |r_uh| \
             {r_uh_max:.3e} N, |u_h − u_e| {du_free:.3e} mm ({} iters, rel {:.2e})",
            a.cases[case].solve.iterations,
            a.cases[case].solve.rel_residual
        );
        assert!(
            t_dev <= ANCHOR_T_HI && stress_max <= ANCHOR_STRESS_HI && ur_err <= ANCHOR_UR_HI,
            "V20 anchor outside the ship bands (T ≤ {ANCHOR_T_HI} °C, stress ≤ \
             {ANCHOR_STRESS_HI} MPa, u ≤ {ANCHOR_UR_HI} mm): {t_dev:.3e} / {stress_max:.3e} / \
             {ur_err:.3e} — re-measure and update ANCHOR_* with the regression explained"
        );
        assert!(
            r_ue_max <= ANCHOR_RUE_HI && r_uh_max <= ANCHOR_RUH_HI && du_free <= ANCHOR_DU_HI,
            "V20 anchor identity outside the ship bands (r_ue ≤ {ANCHOR_RUE_HI} N, r_uh ≤ \
             {ANCHOR_RUH_HI} N, du ≤ {ANCHOR_DU_HI} mm): {r_ue_max:.3e} / {r_uh_max:.3e} / \
             {du_free:.3e} — re-measure and update ANCHOR_* with the regression explained"
        );
        r.section(
            "Anchor — uniform ΔT on the whole surface",
            format!(
                "Both temperature statements at 60 °C (case \"Anchor\"): conduction with \
                 constant boundary data gives T ≡ 60 °C and ΔT = 40 K uniform, so the exact \
                 solution is u = αΔT·(x, y, z) with σ ≡ 0 and u_r = αΔT·r. Same supports as \
                 the gradient case — this exercises the curved-wall mesh, the `zbase` region \
                 selection (per-corner carrier membership: rim facets belong to the cap \
                 plane), and the six-mode constraint set. Convergence: {} iterations, \
                 rel residual {:.1e}. The assembled rows are checked directly: K·u_e = b on \
                 every free row to assembly roundoff, and K·u_h = b to the PCG tolerance.",
                a.cases[case].solve.iterations, a.cases[case].solve.rel_residual
            ),
        );
        r.check(Check::absolute(
            "anchor: temperature uniformity, max |T − 60| over nodes",
            0.0,
            t_dev,
            "°C",
            ANCHOR_T_HI,
        ));
        r.check(Check::absolute(
            "anchor: stress-free, max |stress component| over nodes",
            0.0,
            stress_max,
            "MPa",
            ANCHOR_STRESS_HI,
        ));
        r.check(Check::absolute(
            "anchor: u_x vs αΔT·x at the probes (max over PROBE_RS)",
            ur_ref * 19.5 * PROBE_C,
            ur_ref * 19.5 * PROBE_C - ur_err,
            "mm",
            ANCHOR_UR_HI,
        ));
        r.check(Check::absolute(
            "anchor: in-space identity K·u_e = b, max free-row residual",
            0.0,
            r_ue_max,
            "N",
            ANCHOR_RUE_HI,
        ));
        r.check(Check::absolute(
            "anchor: pipeline solution K·u_h = b, max free-row residual (PCG tolerance)",
            0.0,
            r_uh_max,
            "N",
            ANCHOR_RUH_HI,
        ));
        r.check(Check::absolute(
            "anchor: nodal u_h vs u = αΔT·(x, y, z), max over free components",
            0.0,
            du_free,
            "mm",
            ANCHOR_DU_HI,
        ));
    }

    // --- Mesh study -------------------------------------------------------------------
    let mut study = String::from(
        "| h (mm) | dofs | σxx = σyy @ r=15 | u_x @ r=15 | max |T − log| (°C) | ms |\n\
         |---|---|---|---|---|---|\n",
    );
    let mut runs = Vec::new();
    for h in [6.0f64, 4.0, 3.0] {
        let (a, case, ms) = run(h);
        let loc = Locator::new(&a);
        let t_field = a.cases[case].temperature.as_ref().expect("thermal field");
        let t_err = a
            .mesh
            .nodes
            .iter()
            .zip(t_field)
            .map(|(p, &ti)| {
                let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
                (ti - t_profile(rr)).abs()
            })
            .fold(0.0f64, f64::max);
        let mut probes = Vec::new();
        for &rp in PROBE_RS.iter() {
            let pv = probe(&a, &loc, Some(case), probe_p(rp))
                .unwrap_or_else(|| panic!("probe at {:?}", probe_p(rp)));
            probes.push(pv);
        }
        study.push_str(&format!(
            "| {h:.0} | {} | {:+.2} | {:+.5} | {t_err:.2e} | {ms:.0} |\n",
            a.cases[case].solve.dofs, probes[1].stress[0], probes[1].displacement[0],
        ));
        runs.push((h, a, case, ms, t_err, probes));
    }

    let (h_fine, a, case, ms, t_err, probes) = runs.pop().expect("finest mesh ran");
    let solve = &a.cases[case].solve;

    // Reference values at each probe: on the θ = 45° ray the rotation is exact —
    // σxx = σrr cos²45 + σθθ sin²45 = σyy = (σrr + σθθ)/2, u_x = u_r·cos45, ρ = r.
    let mut want = Vec::new();
    for &rp in PROBE_RS.iter() {
        let (srr, stt, szz) = (ref_model.s_rr(rp), ref_model.s_tt(rp), ref_model.s_zz(rp));
        want.push((
            (srr + stt) / 2.0,           // σxx (probe σxx)
            (srr + stt) / 2.0,           // σyy — equal to σxx on the 45° ray
            szz,                         // σzz (θ-free)
            ref_model.u_r(rp) * PROBE_C, // u_x = u_r cos45
        ));
    }

    for ((rp, pv), w) in PROBE_RS.iter().zip(&probes).zip(&want) {
        println!(
            "V20 h={h_fine} r={rp}: σxx {:+.2} (ref {:+.2}), σyy {:+.2} (ref {:+.2}), \
             σzz {:+.2} (ref {:+.2}), u_x {:+.5} (ref {:+.5})",
            pv.stress[0], w.0, pv.stress[1], w.1, pv.stress[2], w.2, pv.displacement[0], w.3
        );
    }
    println!(
        "V20 mesh study:\n{study}conduction on finest: {} iters, rel {:.2e}; wall {ms:.0} ms",
        solve.iterations, solve.rel_residual
    );

    r.section(
        "Model",
        format!(
            "Octant of the r={RI}..{RO} mm cylinder, z=0..{LEN} mm (bore subtract + two half-space \
             boxes keeping x ≥ 0 and y ≥ 0). Bore wall {T_IN} °C, outer wall {T_OUT} °C, \
             `solve tref={T_REF}` → ΔT = 70 K at the bore, 0 outside, logarithmic through the \
             wall. Supports: single-component on the three exact symmetry planes (x=0 fixed=x, \
             y=0 fixed=y, z=0 fixed=z) — the exact field has u_x = 0, u_y = 0, u_z = c0·z = 0 \
             there, and all six rigid modes are killed. The cut planes are insulated, matching \
             the full-cylinder symmetry. The free-end force balance lives in the reference's \
             generalized-plane-strain c0, not a constraint; with L/ro = 6 and wall 10 mm the \
             free-end Saint-Venant decay at mid-length is below e^-9."
        ),
    );
    r.section(
        "Reference",
        "Generalized plane strain with the thermal strain in the law: u = (u_r cosθ, u_r sinθ, \
         c0·z), u_r = C1·r + C2/r + γα·f(r)/r with f' = r·ΔT and γ = (2μ+3λ)/(2μ+λ); \
         σrr/σθθ/σzz as derived in the test header. Constants from σrr = 0 on both walls plus \
         the free-end balance ∫σzz dA = 0. The uniform-ΔT anchor (u_r = αΔT·r, σ ≡ 0, \
         c0 = αΔT) is asserted by the test before the reference is used.",
    );
    r.section(
        "Mesh study, h = 6/4/3 mm (probe row is r = 15)",
        study.clone(),
    );
    r.section(
        "Measured (finest mesh, θ = 45° mid-face probes at the axial mid-length)",
        format!(
            "Conduction converged in {} iterations (rel {:.1e}); max nodal |T − log profile| \
             {t_err:.2e} °C. Element size {h_fine:.0} mm, wall clock {ms:.0} ms.",
            solve.iterations, solve.rel_residual
        ),
    );

    // Declared from the measured run above (repo pattern), with the mechanisms named. Both
    // are O(h) in the lattice corner offsets from a named surface, |∇T| ≈ 10.1 K/mm at the
    // bore wall:
    //
    // • Claimed chimes: a facet whose normal stays inside a carrier's cone but whose corner
    //   stands up to ~0.29·h+ off that carrier prescribes the carrier's data there — at
    //   h = 3 δ ≤ 0.88 mm → stress errors of that first-order kind (measured σxx max 7.43
    //   at r = 10.5; the r = 15 rows sit at ≈ 1.1 MPa across the ladder; r = 19.5 up to
    //   5.3).
    //
    // • Refused chimes: facets square to neither carrier's cone — the free-end plane's
    //   triangles hugging the bore rim — are no longer claimed, and the rim-corner node
    //   takes the solve value: measured 12.55 °C below the log profile at h = 3, node
    //   [1.54, 10.77, 120.0], r = 10.88 (0.88 off the bore), T = 68.94 — a free node,
    //   not a pinned one. The ladder falls first-order (1.94e1 at h = 6, 1.79e1 at h = 4,
    //   1.26e1 at h = 3). Claiming those facets would pin the corner at 90 °C, an error of
    //   |∇T|·δ ≈ 8.5 — but the facet faces the free end, not the bore, so the cone refuses
    //   it and the band holds the free corner instead.
    //
    // The bands hold these O(h) terms, not a finer claim.
    const SIG_HI: f64 = 10.0; // MPa — measured max 7.43 (near-bore chime data-extension)
    const U_HI: f64 = 0.005; // mm — measured max 9.8e-4 (near-bore)
    const T_HI: f64 = 20.0; // °C — measured max 12.55 (refused bore-rim chime; corner free)

    for ((rp, pv), w) in PROBE_RS.iter().zip(&probes).zip(&want) {
        r.check(Check::absolute(
            &format!("finest: (σrr+σθθ)/2 at r={rp} (probe σxx at θ=45°)"),
            w.0,
            pv.stress[0],
            "MPa",
            SIG_HI,
        ));
        r.check(Check::absolute(
            &format!("finest: (σrr+σθθ)/2 at r={rp} (probe σyy at θ=45°)"),
            w.1,
            pv.stress[1],
            "MPa",
            SIG_HI,
        ));
        r.check(Check::absolute(
            &format!("finest: u_r/√2 at r={rp} (probe u_x at θ=45°)"),
            w.3,
            pv.displacement[0],
            "mm",
            U_HI,
        ));
    }
    r.check(Check::absolute(
        "finest: σzz at r=10.5 (near bore)",
        want[0].2,
        probes[0].stress[2],
        "MPa",
        SIG_HI,
    ));
    r.check(Check::absolute(
        "finest: σzz at r=15 (mid-wall)",
        want[1].2,
        probes[1].stress[2],
        "MPa",
        SIG_HI,
    ));
    r.check(Check::absolute(
        "finest: σzz at r=19.5 (near outside)",
        want[2].2,
        probes[2].stress[2],
        "MPa",
        SIG_HI,
    ));
    r.check(Check::absolute(
        "finest: conduction fidelity, max nodal |T − log profile|",
        0.0,
        t_err,
        "°C",
        T_HI,
    ));

    r.finish();
}
