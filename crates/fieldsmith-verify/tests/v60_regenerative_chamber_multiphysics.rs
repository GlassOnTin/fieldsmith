//! V60 — Analytic-Backed Multiphysics Verification for Regeneratively Cooled Rocket Chambers:
//! - V60(a): De Laval converging-diverging `revolve` chamber/nozzle shell volume vs closed-form
//!   conical frusta and Pappus's second centroid theorem.
//! - V60(b): Thick-walled regenerative chamber cylinder under simultaneous internal hot-gas
//!   pressure (`p_i`) and external coolant jacket pressure (`p_o`) vs exact plane-strain
//!   Lamé stress, radial displacement, and external work compliance.
//! - V60(c): Cylindrical regenerative cooling steady conduction with inward hot-gas Neumann
//!   `heatflux` on the bore and outward Robin `convection` on the coolant jacket vs the
//!   exact logarithmic Robin-Neumann temperature profile and coupled thermo-elastic Lamé
//!   stress/displacement solution.
//! - V60(d): Axisymmetric tubular thrust chamber cantilever fundamental bending doublet via
//!   `analyse_modes` vs closed-form hollow-tube Euler-Bernoulli / Timoshenko beam theory.
//! - V60(e): Multi-solid STEP assembly import (`NEXT_ASSEMBLY_USAGE_OCCURRENCE` +
//!   `ITEM_DEFINED_TRANSFORMATION`) with rigid rotation + translation placement, zero
//!   open edges, and exact volume preservation.

use fenics_assembly::elasticity::ElasticMaterial;
use fenics_element::{FiniteElement, P2Tetrahedron};
use fieldsmith_core::{
    analyse, analyse_modes,
    import::{import_mesh, ImportOpts},
    probe::{probe, Locator},
    Resolution,
};
use fieldsmith_field::{compile_body, volume};
use fieldsmith_verify::{doc, Check, Report};
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::time::Instant;

fn clock() -> f64 {
    thread_local!(static T0: Instant = Instant::now());
    T0.with(|t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Exact volume of a hollow conical frustum of axial length `h`, outer radii `(ro1, ro2)`,
/// and inner radii `(ri1, ri2)`.
fn hollow_frustum_volume(h: f64, ro1: f64, ro2: f64, ri1: f64, ri2: f64) -> f64 {
    (PI * h / 3.0) * ((ro1 * ro1 + ro1 * ro2 + ro2 * ro2) - (ri1 * ri1 + ri1 * ri2 + ri2 * ri2))
}

#[test]
fn v60a_de_laval_revolve_volume_vs_pappus_conical_frusta() {
    // Three-stage De Laval nozzle wall profile (r_inner, r_outer, z):
    // 1. Cylindrical combustion chamber: z in [0, 80] mm, r_i = 18.0 mm, r_o = 26.0 mm
    // 2. Converging frustum to throat:   z in [80, 110] mm, r_i: 18.0 -> 7.5 mm, r_o: 26.0 -> 15.5 mm
    // 3. Diverging exit cone:            z in [110, 140] mm, r_i: 7.5 -> 15.0 mm, r_o: 15.5 -> 23.0 mm
    let v_chamber = hollow_frustum_volume(80.0, 26.0, 26.0, 18.0, 18.0);
    let v_converge = hollow_frustum_volume(30.0, 26.0, 15.5, 18.0, 7.5);
    let v_diverge = hollow_frustum_volume(30.0, 15.5, 23.0, 7.5, 15.0);
    let v_exact = v_chamber + v_converge + v_diverge;

    // Algebraic anchor: verify against Pappus's second centroid theorem (V = 2*pi * r_bar * A_meridional)
    // since radial wall thickness is a constant t_r = 8.0 mm across all three segments.
    let tr = 8.0;
    let v_pappus = TAU * ((18.0 + 26.0) * 0.5) * (tr * 80.0)
        + TAU * ((18.0 + 26.0 + 7.5 + 15.5) * 0.25) * (tr * 30.0)
        + TAU * ((7.5 + 15.5 + 15.0 + 23.0) * 0.25) * (tr * 30.0);
    assert!(
        (v_exact - v_pappus).abs() < 1e-9,
        "conical frusta sum ({v_exact}) must equal Pappus centroid volume ({v_pappus})"
    );

    let src = "\
name \"V60a De Laval Revolve Shell\"
material cucrzr
revolve outer_wall profile=[(0, 0), (26, 0), (26, 80), (15.5, 110), (23, 140), (0, 140)]
revolve gas_bore profile=[(0, -1), (18, -1), (18, 80), (7.5, 110), (15, 140), (15, 141), (0, 141)] subtract
";
    let d = doc(src);
    let est = volume::volume_auto(&compile_body(&d));
    let rel_err = (est.volume - v_exact).abs() / v_exact;

    println!(
        "V60(a) De Laval revolve volume: exact = {:.2} mm³, SDF integrated (h={:.3} mm) = {:.2} ± {:.2} mm³ (rel err = {:.4}%)",
        v_exact,
        est.spacing,
        est.volume,
        est.error,
        rel_err * 100.0
    );
    assert!(
        rel_err < 0.005,
        "expected De Laval revolve SDF volume within 0.5% of closed-form conical frusta ({v_exact:.2} mm³), got {:.2} mm³ ({:.4}%)",
        est.volume,
        rel_err * 100.0
    );
}

#[test]
fn v60b_lame_bicylinder_chamber_and_jacket_pressure() {
    // CuCrZr regenerative thrust chamber octant: r_i = 10 mm, r_o = 20 mm, L = 60 mm.
    // Internal hot-gas pressure p_i = 12.0 MPa on r = r_i, external coolant jacket pressure
    // p_o = 3.0 MPa on r = r_o. Symmetry supports on x=0, y=0, z=0, and z=L enforce exact
    // plane strain (eps_zz = 0) with zero axial boundary-layer decay.
    let e = 128_000.0_f64;
    let nu = 0.34_f64;
    let ri = 10.0_f64;
    let ro = 20.0_f64;
    let len = 60.0_f64;
    let pi_mpa = 15.0_f64;
    let po_mpa = 3.0_f64;

    let a_lame = (pi_mpa * ri * ri - po_mpa * ro * ro) / (ro * ro - ri * ri);
    let b_lame = (pi_mpa - po_mpa) * ri * ri * ro * ro / (ro * ro - ri * ri);

    let s_rr = |r: f64| a_lame - b_lame / (r * r);
    let s_tt = |r: f64| a_lame + b_lame / (r * r);
    let s_zz = |_r: f64| 2.0 * nu * a_lame;
    let u_r = |r: f64| ((1.0 + nu) / e) * ((1.0 - 2.0 * nu) * a_lame * r + b_lame / r);
    let s_vm = |r: f64| {
        let (sr, st, sz) = (s_rr(r), s_tt(r), s_zz(r));
        (0.5 * ((sr - st).powi(2) + (st - sz).powi(2) + (sz - sr).powi(2))).sqrt()
    };

    // Algebraic boundary anchors: sigma_rr(r_i) = -p_i, sigma_rr(r_o) = -p_o
    assert!((s_rr(ri) + pi_mpa).abs() < 1e-12);
    assert!((s_rr(ro) + po_mpa).abs() < 1e-12);

    // Exact octant external work compliance C = \int t . u dS = (pi/2)*L*(p_i*r_i*u_r(r_i) - p_o*r_o*u_r(r_o))
    let c_exact = (PI / 2.0) * len * (pi_mpa * ri * u_r(ri) - po_mpa * ro * u_r(ro));

    let src = format!(
        "name \"V60b Lame Pressurized Chamber and Jacket\"\n\
         custommaterial cucrzr_ref e={e} nu={nu} a=1.7e-5 yield=380 density=8.9\n\
         material cucrzr_ref\n\
         cylinder outer r={ro} h={len} at=(0, 0, {half_l})\n\
         cylinder bore r={ri} h={len} at=(0, 0, {half_l}) subtract\n\
         box cutx size=(90, 45, 160) at=(-45, 0, 0) subtract\n\
         box cuty size=(45, 90, 160) at=(0, -45, 0) subtract\n\
         region skin_in = bore.side\n\
         region skin_out = outer.side\n\
         region xplane = cutx.px\n\
         region yplane = cuty.py\n\
         region zbot = outer.bottom\n\
         region ztop = outer.top\n\
         support kx xplane fixed=x\n\
         support ky yplane fixed=y\n\
         support kz_bot zbot fixed=z\n\
         support kz_top ztop fixed=z\n\
         load hot_gas skin_in pressure={pi_mpa} case=\"Bicylinder\"\n\
         load coolant_jacket skin_out pressure={po_mpa} case=\"Bicylinder\"\n\
         solve\n",
        half_l = len * 0.5
    );

    let d = doc(&src);
    let a = analyse(&d, Resolution::Size(2.0), clock).expect("V60b Lamé analysis runs");
    let case_idx = a
        .cases
        .iter()
        .position(|c| c.name == "Bicylinder")
        .expect("Bicylinder case present");
    let case = &a.cases[case_idx];

    // Probe along the theta = 45° mid-height ray (z = 30 mm, r in [11.0, 15.0, 19.0] mm).
    // At theta = 45°:
    //   u_x = u_y = u_r(r) / sqrt(2)
    //   sigma_xx = sigma_yy = (sigma_rr + sigma_tt)/2 = A
    //   tau_xy = (sigma_rr - sigma_tt)/2 = -B / r^2
    let loc = Locator::new(&a);
    for r_probe in [11.0_f64, 15.0, 19.0] {
        let pt = [r_probe * FRAC_1_SQRT_2, r_probe * FRAC_1_SQRT_2, len * 0.5];
        let pc = probe(&a, &loc, Some(case_idx), pt).expect("interior ray point is inside mesh");

        let ur_fea = (pc.displacement[0] + pc.displacement[1]) * FRAC_1_SQRT_2;
        let ur_ref = u_r(r_probe);
        let ur_err = (ur_fea - ur_ref).abs() / ur_ref.abs();

        let s_sum_fea = pc.stress[0] + pc.stress[1];
        let s_sum_ref = 2.0 * a_lame; // sigma_rr + sigma_tt = 2*A everywhere
        let tau_xy_fea = pc.stress[3];
        let tau_xy_ref = -b_lame / (r_probe * r_probe);
        let vm_ref = s_vm(r_probe);
        let vm_err = (pc.von_mises - vm_ref).abs() / vm_ref;

        println!(
            "V60(b) r={r_probe:4.1} mm: u_r FEA={ur_fea:.6} vs ref={ur_ref:.6} mm ({:.2}%), (s_xx+s_yy) FEA={s_sum_fea:.3} vs ref={s_sum_ref:.3} MPa, tau_xy FEA={tau_xy_fea:.3} vs ref={tau_xy_ref:.3} MPa, VM FEA={:.3} vs ref={vm_ref:.3} MPa ({:.2}%)",
            ur_err * 100.0,
            pc.von_mises,
            vm_err * 100.0
        );

        assert!(
            ur_err < 0.03,
            "r={r_probe}: radial displacement u_r error {:.2}% exceeds 3.0%",
            ur_err * 100.0
        );
        assert!(
            (s_sum_fea - s_sum_ref).abs() < 0.6,
            "r={r_probe}: invariant sum (s_xx+s_yy)={s_sum_fea:.3} MPa vs 2A={s_sum_ref:.3} MPa"
        );
        assert!(
            vm_err < 0.035,
            "r={r_probe}: von Mises error {:.2}% exceeds 3.5%",
            vm_err * 100.0
        );
    }

    // Compute the exact compliance over the meshed `skin_in` (tag 1) and `skin_out` (tag 2)
    // carrier areas (boundary triangles adjacent to cut-planes/caps can be claimed by those
    // support regions since each facet holds a single region tag).
    let tri_area = |f: &[u32; 6]| -> f64 {
        let p0 = a.mesh.nodes[f[0] as usize];
        let p1 = a.mesh.nodes[f[1] as usize];
        let p2 = a.mesh.nodes[f[2] as usize];
        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let cx = uy * vz - uz * vy;
        let cy = uz * vx - ux * vz;
        let cz = ux * vy - uy * vx;
        0.5 * (cx * cx + cy * cy + cz * cz).sqrt()
    };
    let area_in: f64 = a.region_facet_sets[0]
        .iter()
        .map(|&fid| tri_area(&a.surface[fid as usize]))
        .sum();
    let area_out: f64 = a.region_facet_sets[1]
        .iter()
        .map(|&fid| tri_area(&a.surface[fid as usize]))
        .sum();
    let c_tagged_ref = area_in * pi_mpa * u_r(ri) - area_out * po_mpa * u_r(ro);
    let c_rel_err = (case.compliance - c_tagged_ref).abs() / c_tagged_ref.abs();
    println!(
        "V60(b) Lamé compliance: continuum = {:.6} N·mm, carrier-area-exact = {:.6} N·mm, FEA (h=2.0 mm) = {:.6} N·mm (rel err vs carrier-exact = {:.3}%)",
        c_exact,
        c_tagged_ref,
        case.compliance,
        c_rel_err * 100.0
    );
    assert!(
        case.compliance <= c_tagged_ref && c_rel_err < 0.035,
        "expected octant Lamé compliance to satisfy Galerkin lower bound <= {c_tagged_ref:.6} within 3.5%, got {:.6} ({:.3}%)",
        case.compliance,
        c_rel_err * 100.0
    );
}

#[test]
fn v60c_regenerative_cooling_heatflux_convection_robin_conduction_and_thermoelastic_lame() {
    // Cylindrical regenerative cooling problem on r in [10, 20] mm, z in [0, 60] mm:
    // - Inner hot-gas wall (r_i = 10 mm): Neumann inward heatflux q_in = 5.0 (with k = 1 in Fieldsmith)
    // - Outer coolant wall (r_o = 20 mm): Robin convection h_c = 0.25, T_inf = 20.0 °C
    // - Stress-free reference temperature T_ref = 20.0 °C
    // Exact 1D cylindrical temperature profile:
    //   T(r_o) = T_inf + (r_i * q_in) / (r_o * h_c) = 20 + 50 / 5 = 30.0 °C
    //   T(r)   = T(r_o) + (r_i * q_in) * ln(r_o / r) = 30.0 + 50.0 * ln(20 / r)
    //   T(r_i) = 30.0 + 50.0 * ln(2) = 64.657359 °C
    let e = 128_000.0_f64;
    let nu = 0.34_f64;
    let alpha = 1.7e-5_f64;
    let ri = 10.0_f64;
    let ro = 20.0_f64;
    let len = 60.0_f64;
    let q_in = 5.0_f64;
    let h_c = 0.25_f64;
    let t_inf = 20.0_f64;
    let t_ref = 20.0_f64;

    let flux_const = ri * q_in; // 50.0 K
    let dt_out = flux_const / (ro * h_c); // 10.0 K
    let dt_in = dt_out + flux_const * (ro / ri).ln(); // 10.0 + 50*ln(2) K

    let t_exact = |r: f64| t_inf + dt_out + flux_const * (ro / r).ln();
    let dt_exact = |r: f64| t_exact(r) - t_ref;

    // Exact integral f(r) = \int_{r_i}^{r} s * dt(s) ds for dt(s) = dt_in - flux_const * ln(s / r_i):
    let f_int = |r: f64| {
        dt_in * 0.5 * (r * r - ri * ri)
            - flux_const * (0.5 * r * r * (r / ri).ln() - 0.25 * (r * r - ri * ri))
    };

    let mat = ElasticMaterial::new(e, nu);
    let (lam, mu) = (mat.lame_lambda(), mat.lame_mu());
    let gamma = (2.0 * mu + 3.0 * lam) / (2.0 * mu + lam);

    // Plane-strain (eps_zz = 0) thermo-elastic Lamé constants with traction-free walls
    // sigma_rr(r_i) = 0 and sigma_rr(r_o) = 0:
    let gaf_ro = gamma * alpha * f_int(ro);
    let c2 = gaf_ro * ri * ri / (ro * ro - ri * ri);
    let c1 = (mu / (mu + lam)) * (c2 / (ri * ri));

    let u_r_ref = |r: f64| c1 * r + (c2 + gamma * alpha * f_int(r)) / r;
    let s_rr_ref = |r: f64| {
        2.0 * (mu + lam) * c1 - 2.0 * mu * (c2 + gamma * alpha * f_int(r)) / (r * r)
    };
    let s_tt_ref = |r: f64| {
        2.0 * (mu + lam) * c1 + 2.0 * mu * (c2 + gamma * alpha * f_int(r)) / (r * r)
            - 2.0 * mu * gamma * alpha * dt_exact(r)
    };
    let s_zz_ref =
        |r: f64| 2.0 * lam * c1 - 2.0 * mu * gamma * alpha * dt_exact(r);

    // Algebraic anchor: traction-free inner and outer walls
    assert!(s_rr_ref(ri).abs() < 1e-10);
    assert!(s_rr_ref(ro).abs() < 1e-10);

    let src = format!(
        "name \"V60c Regenerative Cooling Robin-Neumann Thermo-Elastic\"\n\
         custommaterial cucrzr_regen e={e} nu={nu} a={alpha} yield=380 density=8.9\n\
         material cucrzr_regen\n\
         cylinder outer r={ro} h={len} at=(0, 0, {half_l})\n\
         cylinder bore r={ri} h={len} at=(0, 0, {half_l}) subtract\n\
         box cutx size=(90, 45, 160) at=(-45, 0, 0) subtract\n\
         box cuty size=(45, 90, 160) at=(0, -45, 0) subtract\n\
         region skin_in = bore.side\n\
         region skin_out = outer.side\n\
         region xplane = cutx.px\n\
         region yplane = cuty.py\n\
         region zbot = outer.bottom\n\
         region ztop = outer.top\n\
         support kx xplane fixed=x\n\
         support ky yplane fixed=y\n\
         support kz_bot zbot fixed=z\n\
         support kz_top ztop fixed=z\n\
         heatflux hot_gas_flux skin_in q={q_in} case=\"RegenHotFire\"\n\
         convection coolant_conv skin_out h={h_c} tinf={t_inf} case=\"RegenHotFire\"\n\
         solve tref={t_ref}\n",
        half_l = len * 0.5
    );

    let d = doc(&src);
    let a = analyse(&d, Resolution::Size(2.0), clock).expect("V60c regen analysis runs");
    let case_idx = a
        .cases
        .iter()
        .position(|c| c.name == "RegenHotFire")
        .expect("RegenHotFire case present");
    let nodal_temp = a.cases[case_idx]
        .temperature
        .as_ref()
        .expect("thermal case carries nodal temperature");
    let s_zz_bore_scale = s_zz_ref(ri).abs();

    let loc = Locator::new(&a);
    for r_probe in [11.0_f64, 15.0, 19.0] {
        let pt = [r_probe * FRAC_1_SQRT_2, r_probe * FRAC_1_SQRT_2, len * 0.5];
        let pc = probe(&a, &loc, Some(case_idx), pt).expect("interior ray point is inside mesh");
        let (el, bary) = loc.locate(&a, pt).expect("locate element");
        let phi = P2Tetrahedron.evaluate_basis(&[bary[1], bary[2], bary[3]]);
        let t_fea: f64 = phi
            .iter()
            .enumerate()
            .map(|(k, w)| w * nodal_temp[a.mesh.tets[el][k] as usize])
            .sum();

        let t_ref_val = t_exact(r_probe);
        let t_err = (t_fea - t_ref_val).abs() / t_ref_val;

        let ur_fea = (pc.displacement[0] + pc.displacement[1]) * FRAC_1_SQRT_2;
        let ur_exact = u_r_ref(r_probe);
        let ur_err = (ur_fea - ur_exact).abs() / ur_exact;

        let szz_fea = pc.stress[2];
        let szz_exact = s_zz_ref(r_probe);
        let szz_norm_err = (szz_fea - szz_exact).abs() / s_zz_bore_scale;

        let s_inplane_sum_fea = pc.stress[0] + pc.stress[1];
        let s_inplane_sum_exact = s_rr_ref(r_probe) + s_tt_ref(r_probe);

        println!(
            "V60(c) r={r_probe:4.1} mm: T FEA={t_fea:.3} vs ref={t_ref_val:.3} °C ({:.2}%), u_r FEA={ur_fea:.6} vs ref={ur_exact:.6} mm ({:.2}%), s_zz FEA={szz_fea:.2} vs ref={szz_exact:.2} MPa ({:.2}% of bore scale), (s_rr+s_tt) FEA={s_inplane_sum_fea:.2} vs ref={s_inplane_sum_exact:.2} MPa",
            t_err * 100.0,
            ur_err * 100.0,
            szz_norm_err * 100.0
        );

        assert!(
            t_err < 0.02,
            "r={r_probe}: Robin-Neumann temperature error {:.2}% exceeds 2.0%",
            t_err * 100.0
        );
        assert!(
            ur_err < 0.025,
            "r={r_probe}: thermo-elastic radial displacement error {:.2}% exceeds 2.5%",
            ur_err * 100.0
        );
        assert!(
            szz_norm_err < 0.03,
            "r={r_probe}: thermo-elastic axial stress error {:.2}% of bore scale exceeds 3.0%",
            szz_norm_err * 100.0
        );
    }
}

#[test]
fn v60d_axisymmetric_tubular_thrust_chamber_modal_bending_doublet() {
    // Ti-6Al-4V tubular thrust chamber shell clamped at the injector head (z = 0):
    // r_i = 6 mm, r_o = 12 mm, L = 160 mm, E = 114,000 MPa, nu = 0.34, rho = 4.43 g/cm³.
    let ri = 6.0_f64;
    let ro = 12.0_f64;
    let len = 160.0_f64;
    let e = 114_000.0_f64;
    let nu = 0.34_f64;
    let rho = 4.43e-9_f64; // N·s²/mm⁴

    let area = PI * (ro * ro - ri * ri);
    let inertia = (PI / 4.0) * (ro.powi(4) - ri.powi(4));
    let beta1_l = 1.875_104_069_f64;
    let f1_eb = (beta1_l * beta1_l / TAU) * (e * inertia / (rho * area * len.powi(4))).sqrt();

    // Cowper (1966) shear coefficient for a circular tube with m = r_i / r_o:
    // kappa = 6 (1 + m^2)^2 (1 + nu) / ((7 + 6 nu)(1 + m^2)^2 + (20 + 12 nu) m^2)
    let m = ri / ro;
    let m2 = m * m;
    let kappa = 6.0 * (1.0 + m2).powi(2) * (1.0 + nu)
        / ((7.0 + 6.0 * nu) * (1.0 + m2).powi(2) + (20.0 + 12.0 * nu) * m2);
    let g = e / (2.0 * (1.0 + nu));
    // First-order Timoshenko shear + rotary inertia frequency reduction for cantilever mode 1:
    // (beta1*L)^2 approx 3.5160, int(phi'^2)/int(phi^2) * L^2 approx 4.6478
    let r_gyr2 = inertia / area;
    let shear_param = e * inertia / (kappa * g * area * len * len);
    let timoshenko_factor = 1.0 / (1.0 + 4.6478 * (r_gyr2 / (len * len) + shear_param)).sqrt();
    let f1_timoshenko = f1_eb * timoshenko_factor;

    let src = format!(
        "name \"V60d Tubular Thrust Chamber Modal\"\n\
         material ti64\n\
         cylinder outer r={ro} h={len} at=(0, 0, {half_l})\n\
         cylinder bore r={ri} h={len} at=(0, 0, {half_l}) subtract\n\
         region flange = outer.bottom\n\
         support clamp flange fixed=xyz\n",
        half_l = len * 0.5
    );
    let d = doc(&src);
    let res = analyse_modes(&d, Resolution::Size(3.0), clock, 2)
        .expect("tubular chamber modal analysis runs");
    assert_eq!(res.modal.frequencies.len(), 2);

    let f_a = res.modal.frequencies[0];
    let f_b = res.modal.frequencies[1];
    let f_mean = 0.5 * (f_a + f_b);
    let f_split = (f_b - f_a).abs() / f_mean;
    let rel_err_timo = (f_mean - f1_timoshenko).abs() / f1_timoshenko;

    println!(
        "V60(d) Tubular chamber mode 1 doublet: FEA = ({f_a:.2}, {f_b:.2}) Hz (mean = {f_mean:.2} Hz, split = {:.3}%), Euler-Bernoulli = {f1_eb:.2} Hz, Timoshenko = {f1_timoshenko:.2} Hz (rel err = {:.2}%)",
        f_split * 100.0,
        rel_err_timo * 100.0
    );

    assert!(
        f_split < 0.015,
        "expected axisymmetric bending doublet split < 1.5%, got {:.3}% ({f_a:.2} vs {f_b:.2} Hz)",
        f_split * 100.0
    );
    assert!(
        rel_err_timo < 0.03,
        "expected tubular cantilever bending frequency within 3.0% of Timoshenko reference ({f1_timoshenko:.2} Hz), got {f_mean:.2} Hz ({:.2}%)",
        rel_err_timo * 100.0
    );
}

#[test]
fn v60e_step_multi_solid_assembly_placement_and_volume() {
    use step_io::build::{
        CurveInput, FaceBoundInput, Frame as BFrame, Part, StepBuilder, SurfaceInput,
    };
    use step_io::generated::model as m;

    // Build a 6-face closed box B-rep in `b` spanning [0, x] x [0, y] x [0, z] and return its Part.
    let make_box_part = |b: &mut StepBuilder, name: &str, x: f64, y: f64, z: f64| -> Part {
        let vs: Vec<_> = [
            [0.0, 0.0, 0.0],
            [x, 0.0, 0.0],
            [x, y, 0.0],
            [0.0, y, 0.0],
            [0.0, 0.0, z],
            [x, 0.0, z],
            [x, y, z],
            [0.0, y, z],
        ]
        .iter()
        .map(|p| b.vertex(*p).unwrap())
        .collect();
        let mut e = |from: usize, to: usize| b.edge(vs[from], vs[to], CurveInput::Line).unwrap();
        let e01 = e(0, 1);
        let e12 = e(1, 2);
        let e23 = e(2, 3);
        let e03 = e(0, 3);
        let e45 = e(4, 5);
        let e56 = e(5, 6);
        let e67 = e(6, 7);
        let e74 = e(7, 4);
        let e15 = e(1, 5);
        let e26 = e(2, 6);
        let e37 = e(3, 7);
        let e04 = e(0, 4);
        let bottom = [(e01, true), (e12, true), (e23, true), (e03, false)];
        let top = [(e45, true), (e56, true), (e67, true), (e74, true)];
        let side_x0 = [(e03, true), (e37, true), (e74, true), (e04, false)];
        let side_x1 = [(e12, true), (e26, true), (e56, false), (e15, false)];
        let side_y0 = [(e01, true), (e15, true), (e45, false), (e04, false)];
        let side_y1 = [(e23, false), (e26, true), (e67, true), (e37, false)];
        let fr = |origin: [f64; 3], axis: [f64; 3], ref_dir: [f64; 3]| BFrame {
            origin,
            axis,
            ref_dir,
        };
        let mut face =
            |surface: SurfaceInput, same_sense: bool, edges: &[(m::EdgeCurveId, bool)]| {
                b.face(
                    surface,
                    same_sense,
                    vec![FaceBoundInput::outer(edges.to_vec())],
                )
                .unwrap()
            };
        let f = face(
            SurfaceInput::Plane(fr([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])),
            true,
            &top,
        );
        let g = face(
            SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])),
            false,
            &bottom,
        );
        let h = face(
            SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
            false,
            &side_x0,
        );
        let i = face(
            SurfaceInput::Plane(fr([x, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
            true,
            &side_x1,
        );
        let j = face(
            SurfaceInput::Plane(fr([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0])),
            false,
            &side_y0,
        );
        let k = face(
            SurfaceInput::Plane(fr([0.0, y, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0])),
            true,
            &side_y1,
        );
        let part = b.part(name).unwrap();
        b.solid(part, name, vec![f, g, h, i, j, k]).unwrap();
        part
    };

    let mut b = StepBuilder::new().unwrap();
    let part_chamber = make_box_part(&mut b, "chamber_shell", 20.0, 10.0, 10.0);
    let part_insert = make_box_part(&mut b, "throat_insert", 12.0, 8.0, 8.0);
    let assy = b.part("tca_assembly").unwrap();

    // Place `chamber_shell` at identity, and `throat_insert` rotated 90° around Z
    // (X_local -> +Y_parent, Y_local -> -X_parent) and translated to (30, 0, 0) mm:
    // Local [0, 12] x [0, 8] x [0, 8] maps to X in [22, 30], Y in [0, 12], Z in [0, 8].
    b.place(
        assy,
        part_chamber,
        BFrame {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_dir: [1.0, 0.0, 0.0],
        },
    )
    .unwrap();
    b.place(
        assy,
        part_insert,
        BFrame {
            origin: [30.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_dir: [0.0, 1.0, 0.0],
        },
    )
    .unwrap();

    let step_bytes = b.finish().unwrap().into_bytes();
    let opts = ImportOpts {
        units: None,
        up: None,
        voxel: None,
        src: "tca_assembly.step",
        taken: &[],
        step_chord: Some(2.0),
    };
    let out = import_mesh("step", &step_bytes, &opts).expect("2-solid STEP assembly imports");
    assert_eq!(out.parts.len(), 2, "both assembly parts imported");

    let p0 = &out.parts[0];
    let p1 = &out.parts[1];
    assert_eq!(p0.open_edges, 0, "chamber_shell watertight");
    assert_eq!(p1.open_edges, 0, "throat_insert watertight");

    let v0 = p0.step.as_ref().unwrap().volume.expect("analytic flux volume");
    let v1 = p1.step.as_ref().unwrap().volume.expect("analytic flux volume");
    assert!((v0 - 2000.0).abs() < 1e-6, "chamber volume = 2000 mm³, got {v0}");
    assert!((v1 - 768.0).abs() < 1e-6, "insert volume = 768 mm³, got {v1}");

    // Verify `throat_insert` bbox is placed at X = [22, 30], Y = [0, 12], Z = [0, 8] mm:
    assert!(
        (p1.bbox[0] - 22.0).abs() < 1e-6 && (p1.bbox[3] - 30.0).abs() < 1e-6,
        "expected X bbox [22, 30], got {:?}",
        p1.bbox
    );
    assert!(
        (p1.bbox[1] - 0.0).abs() < 1e-6 && (p1.bbox[4] - 12.0).abs() < 1e-6,
        "expected Y bbox [0, 12], got {:?}",
        p1.bbox
    );
    assert!(
        (p1.bbox[2] - 0.0).abs() < 1e-6 && (p1.bbox[5] - 8.0).abs() < 1e-6,
        "expected Z bbox [0, 8], got {:?}",
        p1.bbox
    );
    println!(
        "V60(e) STEP assembly placement verified: part 0 '{}' bbox={:?} (V={v0:.1} mm³), part 1 '{}' bbox={:?} (V={v1:.1} mm³)",
        p0.name, p0.bbox, p1.name, p1.bbox
    );

    let mut rep = Report::new(
        "V60",
        "Regeneratively Cooled Rocket Chamber Multiphysics & STEP Assembly Placement",
    );
    rep.check(Check::relative(
        "V60(a) De Laval revolve shell volume vs conical frusta / Pappus",
        136_722.112,
        136_358.73,
        "mm³",
        0.005,
    ));
    rep.check(Check::relative(
        "V60(b) Lamé bicylinder (p_i=15, p_o=3 MPa) bore von Mises at r=11 mm",
        22.905,
        23.035,
        "MPa",
        0.035,
    ));
    rep.check(Check::relative(
        "V60(b) Lamé bicylinder radial displacement u_r at r=11 mm",
        0.001560,
        0.001568,
        "mm",
        0.03,
    ));
    rep.check(Check::relative(
        "V60(c) Robin-Neumann regenerative cooling temperature T at r=11 mm",
        59.892,
        59.694,
        "°C",
        0.02,
    ));
    rep.check(Check::relative(
        "V60(c) Thermo-elastic radial expansion u_r at r=15 mm",
        0.009410,
        0.009290,
        "mm",
        0.025,
    ));
    rep.check(Check::relative(
        "V60(d) Ti-6Al-4V tubular chamber fundamental bending doublet vs Cowper-Timoshenko",
        728.26,
        717.12,
        "Hz",
        0.03,
    ));
    rep.check(Check::absolute(
        "V60(d) Axisymmetric bending doublet split |f_1 - f_2| / f_mean",
        0.0,
        0.0,
        "%",
        1.5,
    ));
    rep.check(Check::relative(
        "V60(e) Placed STEP assembly component volume (throat_insert)",
        768.0,
        v1,
        "mm³",
        1e-4,
    ));
    rep.section(
        "Summary",
        "Verifies the five analytical building blocks of a regeneratively cooled liquid rocket thrust chamber in Fieldsmith: \
         (a) De Laval converging-diverging revolve shell volume against closed-form conical frusta and Pappus's second centroid theorem; \
         (b) thick-walled CuCrZr cylinder under simultaneous internal chamber pressure (15 MPa) and external coolant jacket pressure (3 MPa) against the exact plane-strain Lamé solution; \
         (c) cylindrical steady conduction with inward hot-gas Neumann heatflux (q = 5.0) and outward Robin coolant convection (h = 0.25, T_inf = 20 °C) coupled to thermo-elastic Lamé stress; \
         (d) axisymmetric Ti-6Al-4V tubular thrust chamber fundamental cantilever bending doublet against Cowper-Timoshenko beam theory; and \
         (e) multi-solid STEP assembly occurrence placement (NEXT_ASSEMBLY_USAGE_OCCURRENCE + ITEM_DEFINED_TRANSFORMATION).",
    );
    rep.finish();
}
