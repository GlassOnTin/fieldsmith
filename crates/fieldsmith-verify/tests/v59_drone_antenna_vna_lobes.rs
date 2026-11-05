//! V59 Verification Suite: 4-Inch DeadCat CFRP Drone + Gemini Dual-Band (`900 MHz` + `2.4 GHz`)
//! Antenna VNA Scattering Matrix (`S11`, `S21`, `VSWR`), 50 Ω Smith Chart Impedance Trajectory,
//! 3D Radiation Lobes (`G(theta, phi)`), Anisotropic CFRP Eddy-Current Loss, and Automated
//! Geometry & L-Network Matching Optimisation.

use fieldsmith_doc::fsm;
use fieldsmith_solve::antenna::{
    analyze_drone_antenna, build_deadcat_drone_fsm, complex_gamma_50, optimise_drone_antenna,
    DeadCatGeminiAntennaSystem, DroneAntennaDesignSpec, DroneAntennaTopology,
};
use fieldsmith_solve::coupled_system::{
    CoupledEngineeringSystem, EngineConfig, MonolithicSystemEngine,
};

#[test]
fn v59_smith_chart_gamma_and_vswr_identities() {
    // 1. Exact 50 Ω match -> Gamma = 0 + j0
    let (re, im, mag) = complex_gamma_50(50.0, 0.0);
    assert!(re.abs() < 1e-12);
    assert!(im.abs() < 1e-12);
    assert!(mag < 1e-5);

    // 2. Pure 100 Ω resistive load -> Gamma = +1/3, VSWR = 2.0
    let (re100, im100, mag100) = complex_gamma_50(100.0, 0.0);
    assert!((re100 - 1.0 / 3.0).abs() < 1e-9);
    assert!(im100.abs() < 1e-9);
    let vswr100 = (1.0 + mag100) / (1.0 - mag100);
    assert!((vswr100 - 2.0).abs() < 1e-6);

    // 3. Pure reactive j50 Ω -> |Gamma| -> 1
    let (_, _, mag_react) = complex_gamma_50(0.05, 50.0);
    assert!(mag_react > 0.998);
}

#[test]
fn v59_unoptimised_tucked_vs_optimised_gemini_antenna_vna_and_lobes() {
    // 1. Evaluate the unoptimised "compact tucked" factory preset:
    //    d_cfrp = 6.5 mm, free-space dipole lengths inside 3.2 mm TPU boot, 38° diversity skew.
    let tucked = DroneAntennaDesignSpec::unoptimised_tucked_preset();
    let rep_tucked = analyze_drone_antenna(&tucked);

    // Because the antenna sits 6.5 mm from the CFRP carbon plate inside a 3.2 mm TPU boot:
    // - CFRP eddy-current power absorption is high (> 25%)
    // - Return loss S11 at 915 MHz and 2.44 GHz is poorly matched (> -9 dB)
    // - Diversity isolation S21 at 38° skew is degraded (> -14 dB)
    // - Nose-on CFRP/LiPo shadow creates a deep p10 blind spot (< -6.5 dBi)
    assert!(
        rep_tucked.low_band_summary.cfrp_power_loss_pct > 25.0,
        "expected >25% CFRP eddy loss when tucked 6.5 mm from carbon plate, got {:.1}%",
        rep_tucked.low_band_summary.cfrp_power_loss_pct
    );
    assert!(
        rep_tucked.low_band_summary.s11_at_target_db > -9.5
            || rep_tucked.high_band_summary.s11_at_target_db > -9.5,
        "expected detuned S11 > -9.5 dB for uncompensated free-space lengths in TPU pod"
    );
    assert!(
        !rep_tucked.all_targets_met,
        "unoptimised tucked preset must not pass dual-band qualification"
    );

    // 2. Run the automated Gemini Dual-Band Antenna Geometry & Matching Optimiser
    let opt = optimise_drone_antenna(&tucked, 8);
    let rep_opt = &opt.optimised_report;

    println!(
        "V59 Unoptimised Tucked -> 915 MHz S11 = {:.2} dB (VSWR {:.2}, eff {:.1}%), 2.44 GHz S11 = {:.2} dB (VSWR {:.2}, eff {:.1}%), S21 = {:.2} dB, p10 floor = {:.2} dBi",
        rep_tucked.low_band_summary.s11_at_target_db,
        rep_tucked.low_band_summary.vswr_at_target,
        rep_tucked.low_band_summary.total_efficiency_pct,
        rep_tucked.high_band_summary.s11_at_target_db,
        rep_tucked.high_band_summary.vswr_at_target,
        rep_tucked.high_band_summary.total_efficiency_pct,
        rep_tucked.low_band_summary.s21_isolation_db,
        rep_tucked.high_band_summary.p10_blind_spot_floor_dbi,
    );
    println!(
        "V59 Optimised Gemini   -> 915 MHz S11 = {:.2} dB (VSWR {:.2}, eff {:.1}%), 2.44 GHz S11 = {:.2} dB (VSWR {:.2}, eff {:.1}%), S21 = {:.2} dB, p10 floor = {:.2} dBi",
        rep_opt.low_band_summary.s11_at_target_db,
        rep_opt.low_band_summary.vswr_at_target,
        rep_opt.low_band_summary.total_efficiency_pct,
        rep_opt.high_band_summary.s11_at_target_db,
        rep_opt.high_band_summary.vswr_at_target,
        rep_opt.high_band_summary.total_efficiency_pct,
        rep_opt.low_band_summary.s21_isolation_db,
        rep_opt.high_band_summary.p10_blind_spot_floor_dbi,
    );

    // Verify all dual-band VNA, Smith Chart, efficiency, and 3D lobe blind-spot targets pass:
    assert!(
        rep_opt.low_band_summary.s11_at_target_db <= -12.0,
        "optimised 915 MHz S11 should be <= -12 dB, got {:.2} dB",
        rep_opt.low_band_summary.s11_at_target_db
    );
    assert!(
        rep_opt.high_band_summary.s11_at_target_db <= -12.0,
        "optimised 2.44 GHz S11 should be <= -12 dB, got {:.2} dB",
        rep_opt.high_band_summary.s11_at_target_db
    );
    assert!(
        rep_opt.low_band_summary.s21_isolation_db <= -15.0
            && rep_opt.high_band_summary.s21_isolation_db <= -15.0,
        "optimised port isolation S21 should be <= -15 dB"
    );
    assert!(
        rep_opt.low_band_summary.total_efficiency_pct >= 70.0
            && rep_opt.high_band_summary.total_efficiency_pct >= 70.0,
        "optimised total efficiency should be >= 70%"
    );
    assert!(
        rep_opt.high_band_summary.p10_blind_spot_floor_dbi
            > rep_tucked.high_band_summary.p10_blind_spot_floor_dbi + 3.0,
        "optimised elevation & standoff must raise p10 blind-spot floor by > 3 dB"
    );
    assert!(rep_opt.all_targets_met);

    // 3. Verify the generated 3D `.fsm` DeadCat drone assembly parses cleanly with 0 validation problems
    let doc = fsm::parse(&opt.fsm_3d).expect("generated DeadCat drone .fsm must parse");
    assert_eq!(doc.validate(), Vec::<String>::new());
    assert!(doc.node_ids().len() >= 25);
}

#[test]
fn v59_coupled_system_and_topologies() {
    for topo in [
        DroneAntennaTopology::RearTpuVDipolePod,
        DroneAntennaTopology::ConformalArmFoldedFpc,
        DroneAntennaTopology::SplitFrontRearOrthogonal,
    ] {
        let spec = DroneAntennaDesignSpec {
            topology: topo,
            ..DroneAntennaDesignSpec::default()
        };
        let rep = analyze_drone_antenna(&spec);
        assert_eq!(rep.low_band_sweep.len(), 41);
        assert_eq!(rep.high_band_sweep.len(), 41);
        assert_eq!(rep.high_band_summary.yaw_horizon_cut.len(), 72);
        assert_eq!(rep.high_band_summary.lobe_3d_rings.len(), 13);
        let fsm_text = build_deadcat_drone_fsm(&spec);
        let doc = fsm::parse(&fsm_text).expect("DeadCat .fsm must parse");
        assert!(doc.validate().is_empty());
    }

    let sys = DeadCatGeminiAntennaSystem::new();
    let p = sys.nominal_design_vector();
    let theta = sys.nominal_epistemic_vector();
    let cfg = EngineConfig::default();
    let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p, &theta)
        .expect("DeadCatGeminiAntennaSystem coupled solve must converge");
    assert_eq!(res.system_id, "deadcat_4in_gemini_antenna_v1");
    assert!(res.conservation_audits.iter().all(|a| a.passed()));
}
