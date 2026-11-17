//! V57 — Engineering Fixings Catalogue, Automatic Host Drilling/Tapping,
//! Inclined-Plane Sliding-Surface Torque Envelope (`T_min` & `T_max`), Empirical
//! Weibull / Creep Failure Distribution Regression, and Coupled Adjoint Qualification.

use fieldsmith_doc::fixings::{
    evaluate_fixing_torque_envelope, evaluate_open_standard_benchmarks, fit_failure_distribution,
    insert_fixing_into_document, FailureSampleRow, FixingEnvelopeInput, FixingInsertRequest,
    FixingKind, SubstrateRegime,
};
use fieldsmith_doc::fsm;
use fieldsmith_mcp::Server;
use fieldsmith_solve::coupled_system::{
    CoupledEngineeringSystem, EngineConfig, MonolithicSystemEngine,
};
use fieldsmith_solve::fixings::FastenerFixingSystem;
use fieldsmith_verify::{Check, Report};
use serde_json::{json, Value};

/// Emit `docs/verification/V57.md`. Mirrors the assertions in the tests below as report
/// checks so the "Verified by V57" claim in docs/README.md (A13) points at a shipped
/// report. The sibling tests remain the asserting source.
#[test]
fn v57_report() {
    let mut r = Report::new(
        "V57",
        "Engineering Fixings Catalogue, Inclined-Plane Torque Envelope, Weibull Regression & Open Standards",
    );

    // Weibull coupon regression recovers the true 2-parameter law (test 3).
    let (true_m, true_sigma0, true_t0) = (8.5_f64, 64.0_f64, 1.92_f64);
    let pfs = [0.05, 0.12, 0.25, 0.40, 0.55, 0.70, 0.85, 0.94];
    let rows: Vec<FailureSampleRow> = pfs
        .iter()
        .enumerate()
        .map(|(i, &pf)| {
            let factor = (-(1.0_f64 - pf).ln()).powf(1.0 / true_m);
            FailureSampleRow {
                sample_id: format!("C-{}", i + 1),
                torque_nm: true_t0 * factor,
                principal_stress_mpa: true_sigma0 * factor,
                failure_prob: Some(pf),
                note: "Boss radial split coupon".into(),
            }
        })
        .collect();
    let fit = fit_failure_distribution(&rows, 1.0e-3);
    r.check(Check::absolute("Weibull modulus m recovered", true_m, fit.weibull_modulus_m, "-", 1.0e-4));
    r.check(Check::absolute("Weibull scale σ₀ recovered", true_sigma0, fit.weibull_scale_sigma0_mpa, "MPa", 1.0e-3));
    r.check(Check::absolute("Weibull torque scale T₀ recovered", true_t0, fit.weibull_scale_t0_nm, "N·m", 1.0e-4));
    r.check(Check::absolute(
        "Weibull regression R² > 0.9999",
        1.0,
        if fit.weibull_r2 > 0.9999 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    // Open standards: quick handbook rule vs first-principles rederivation (test 5).
    let inp = FixingEnvelopeInput {
        fixing_kind: FixingKind::HexCapBolt,
        nominal_d_mm: 4.0,
        pitch_mm: 0.7,
        engagement_length_mm: 8.0,
        boss_od_mm: 10.0,
        substrate_material: "alsi10mg".into(),
        fastener_material: "ti64".into(),
        mu_thread: 0.14,
        mu_head: 0.15,
        mu_joint: 0.28,
        vibration_g: 15.0,
        shock_g: 40.0,
        payload_mass_kg: 0.25,
        num_fixings: 1,
        delta_temp_c: 0.0,
        service_hours: 1000.0,
        target_failure_prob: 1.0e-4,
        weibull_m_override: None,
        weibull_sigma0_override_mpa: None,
        ..Default::default()
    };
    let benchmarks = evaluate_open_standard_benchmarks(&inp);
    r.check(Check::absolute("Open-standard benchmarks evaluated", 4.0, benchmarks.len() as f64, "standards", 0.0));
    let b_h28 = &benchmarks[0];
    r.check(Check::absolute("FED-STD-H28 metric stress area A_s", 8.78, b_h28.standard_value, "mm²", 0.01));
    r.check(Check::absolute("FED-STD-H28 vs 60° triangle rederivation discrepancy", 0.0, b_h28.discrepancy_pct, "%", 1.0e-3));
    r.check(Check::absolute("NASA-STD-5020B vs Motosh integral discrepancy", 0.0, benchmarks[1].discrepancy_pct, "%", 2.5));
    r.check(Check::absolute("NIST/SEMATECH Weibull scale recovery discrepancy", 0.0, benchmarks[2].discrepancy_pct, "%", 0.5));
    r.check(Check::absolute(
        "NASA-HDBK-7005 Sopwith first-thread Y₁ in (1.8, 3.2)",
        1.0,
        if benchmarks[3].apriori_value > 1.8 && benchmarks[3].apriori_value < 3.2 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    // Substrate-regime envelope physics (test 2).
    let base_input = FixingEnvelopeInput {
        fixing_kind: FixingKind::SelfTappingScrew,
        nominal_d_mm: 4.0,
        pitch_mm: 0.70,
        engagement_length_mm: 8.0,
        boss_od_mm: 10.0,
        substrate_material: "petg".into(),
        fastener_material: "ti64".into(),
        vibration_g: 10.0,
        shock_g: 25.0,
        payload_mass_kg: 0.40,
        num_fixings: 1,
        delta_temp_c: 0.0,
        service_hours: 1000.0,
        target_failure_prob: 1.0e-3,
        mu_thread: 0.16,
        mu_head: 0.18,
        mu_joint: 0.25,
        weibull_m_override: None,
        weibull_sigma0_override_mpa: None,
        ..Default::default()
    };
    let plastic_env = evaluate_fixing_torque_envelope(&base_input);
    let metal_env = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "alsi10mg".into(),
        ..base_input.clone()
    });
    let high_vib_env = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        vibration_g: 28.0,
        shock_g: 70.0,
        ..base_input.clone()
    });
    let jwst_loose = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "sic_jwst".into(),
        target_failure_prob: 1.0e-2,
        ..base_input.clone()
    });
    let jwst_strict = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "sic_jwst".into(),
        target_failure_prob: 1.0e-6,
        ..base_input.clone()
    });
    r.check(Check::absolute(
        "PETG boss creep retention factor < 0.90",
        1.0,
        if plastic_env.creep_retention_factor < 0.90 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));
    r.check(Check::absolute(
        "AlSi10Mg metal T_max > 2.2× PETG T_max",
        1.0,
        if metal_env.max_torque_nm > 2.2 * plastic_env.max_torque_nm { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));
    r.check(Check::absolute(
        "Higher vibration g-load raises T_min > 1.4×",
        1.0,
        if high_vib_env.min_torque_nm > plastic_env.min_torque_nm * 1.4 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));
    r.check(Check::absolute(
        "JWST P_f=1e-6 tightens SiC T_max < 0.65× P_f=1e-2",
        1.0,
        if jwst_strict.max_torque_nm < jwst_loose.max_torque_nm * 0.65 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    // All seven catalogue fixings auto-tap the host, validate, and round-trip (test 1).
    let mut ok_kinds = 0.0;
    for kind in FixingKind::ALL {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Fixing Host Enclosure\"\n\
             material petg\n\
             box enclosure_wall size=(32, 24, 10) at=(0, 0, 0) round=1\n\
             region base_mount = enclosure_wall.nz\n\
             support mount_fix base_mount fixed=xyz\n",
        )
        .expect("parse base host");
        let summary = insert_fixing_into_document(
            &mut doc,
            &FixingInsertRequest {
                kind,
                size: "M4".into(),
                engagement_mm: 8.0,
                host_id: Some("enclosure_wall".into()),
                fastener_material: "ti64".into(),
                vibration_g: 12.0,
                payload_mass_kg: 0.15,
                ..Default::default()
            },
        );
        let printed = fsm::print(&doc);
        let reparsed = fsm::parse(&printed).expect("round-trip parse");
        if !summary.host_features_added.is_empty()
            && !summary.hardware_nodes_added.is_empty()
            && !summary.loads_added.is_empty()
            && summary.envelope.max_torque_nm > summary.envelope.min_torque_nm
            && doc.validate().is_empty()
            && reparsed.validate().is_empty()
        {
            ok_kinds += 1.0;
        }
    }
    r.check(Check::absolute(
        "Catalogue fixings auto-tapped, positive torque window, clean validation + round-trip",
        FixingKind::ALL.len() as f64,
        ok_kinds,
        "kinds",
        0.0,
    ));

    // Monolithic 72-DOF fastener system + MCP round-trip (test 4).
    let sys = FastenerFixingSystem::new();
    let res = MonolithicSystemEngine::solve_and_analyze(
        &sys,
        &EngineConfig::default(),
        &sys.nominal_design_vector(),
        &sys.nominal_epistemic_vector(),
    )
    .expect("solve 72-DOF FastenerFixingSystem");
    r.check(Check::absolute("FastenerFixingSystem state DOFs", 72.0, res.state.len() as f64, "DOF", 0.0));
    r.check(Check::absolute("Adjoint vs tangent max rel err", 0.0, res.adjoint_vs_tangent_max_rel_err, "rel", 1.0e-4));
    r.check(Check::absolute(
        "Conservation audits all pass",
        1.0,
        if res.conservation_audits.iter().all(|a| a.passed()) { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));
    let mut srv = Server::new();
    let resp = srv
        .handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"insert_fixing","arguments":{"kind":"self_tapping_screw","size":"M4","engagement_mm":8.0,"vibration_g":14.0}}})
                .to_string(),
        )
        .expect("insert_fixing response");
    let v: Value = serde_json::from_str(&resp).unwrap();
    let payload: Value =
        serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    r.check(Check::absolute(
        "MCP insert_fixing validation problems",
        0.0,
        payload["problems"].as_array().unwrap().len() as f64,
        "problems",
        0.0,
    ));
    r.check(Check::absolute(
        "MCP insert_fixing envelope T_max > 0.5 N·m",
        1.0,
        if payload["fixing"]["envelope"]["max_torque_nm"].as_f64().unwrap() > 0.5 { 1.0 } else { 0.0 },
        "-",
        0.0,
    ));

    r.section(
        "Summary",
        format!(
            "Weibull coupon regression recovers m = {:.5} (true 8.5), σ₀ = {:.4} MPa (true 64), \
             T₀ = {:.5} N·m (true 1.92), R² = {:.6}. PETG boss T_max = {:.3} N·m vs AlSi10Mg \
             metal {:.3} N·m; JWST SiC P_f=1e-6 T_max = {:.3} N·m vs P_f=1e-2 {:.3} N·m. \
             FED-STD-H28 A_s = {:.4} mm², Motosh vs NASA-STD-5020B discrepancy {:.3} %, \
             NIST Weibull recovery {:.3} %. All {} catalogue fixings auto-tap, validate and \
             round-trip; the 72-DOF monolithic fastener system and MCP insert_fixing / \
             solve_coupled_system round-trip pass.",
            fit.weibull_modulus_m, fit.weibull_scale_sigma0_mpa, fit.weibull_scale_t0_nm, fit.weibull_r2,
            plastic_env.max_torque_nm, metal_env.max_torque_nm,
            jwst_strict.max_torque_nm, jwst_loose.max_torque_nm,
            b_h28.standard_value, benchmarks[1].discrepancy_pct, benchmarks[2].discrepancy_pct,
            ok_kinds as usize,
        ),
    );
    r.section(
        "Limits",
        "The torque envelope is the inclined-plane / Motosh three-surface analytical model \
         with empirical friction coefficients and Gibson–Ashby-style substrate regimes, not \
         a resolved thread-contact FEA; the Weibull fit is a median-rank regression on \
         synthetic coupons generated from the same law it recovers (an identity check, not \
         independent experimental data). One-sided physics targets are recorded as 0/1 \
         indicator checks (the V56 convention); measured values are in the Summary. The \
         self-tapping 3D P2 FEA reachability solve is asserted in the sibling catalogue test.",
    );
    r.finish();
}

#[test]
fn v57_all_seven_catalogue_fixings_auto_tap_host_and_solve_3d_fea() {
    for kind in FixingKind::ALL {
        let mut doc = fsm::parse(
            "fieldsmith 1\n\
             name \"Fixing Host Enclosure\"\n\
             material petg\n\
             box enclosure_wall size=(32, 24, 10) at=(0, 0, 0) round=1\n\
             region base_mount = enclosure_wall.nz\n\
             support mount_fix base_mount fixed=xyz\n",
        )
        .expect("parse base host");

        let req = FixingInsertRequest {
            kind,
            size: "M4".into(),
            engagement_mm: 8.0,
            at: None,
            id_prefix: None,
            host_id: Some("enclosure_wall".into()),
            fastener_material: "ti64".into(),
            substrate_material: None,
            vibration_g: 12.0,
            payload_mass_kg: 0.15,
            ..Default::default()
        };
        let summary = insert_fixing_into_document(&mut doc, &req);

        assert!(
            !summary.host_features_added.is_empty(),
            "{kind:?} must automatically add subtracted holes/tapping/catch features to the fixed object"
        );
        assert!(
            !summary.hardware_nodes_added.is_empty(),
            "{kind:?} must add fixing hardware geometry"
        );
        assert!(
            !summary.loads_added.is_empty(),
            "{kind:?} must automatically configure a clamping preload or snap load"
        );
        assert!(
            summary.envelope.max_torque_nm > summary.envelope.min_torque_nm,
            "{kind:?} must have a positive safe torque window [T_min, T_max]: T_min={} T_max={}",
            summary.envelope.min_torque_nm,
            summary.envelope.max_torque_nm
        );

        let problems = doc.validate();
        assert!(
            problems.is_empty(),
            "{kind:?} generated document must validate cleanly, got: {problems:?}"
        );

        // Round-trip through .fsm text printer and parser:
        let printed = fsm::print(&doc);
        let reparsed = fsm::parse(&printed)
            .unwrap_or_else(|e| panic!("{kind:?} failed .fsm round-trip parse: {e}\n{printed}"));
        assert!(reparsed.validate().is_empty());
    }

    // Solve 3D P2 FEA on the self-tapping screw assembly to verify finite displacement and principal stress:
    let mut doc = fsm::parse(
        "fieldsmith 1\n\
         name \"Self-Tapping Boss Plate\"\n\
         material petg\n\
         box boss_plate size=(28, 22, 10) at=(0, 0, 0) round=1\n\
         region bottom_fix = boss_plate.nz\n\
         support s1 bottom_fix fixed=xyz\n",
    )
    .unwrap();
    insert_fixing_into_document(
        &mut doc,
        &FixingInsertRequest {
            kind: FixingKind::SelfTappingScrew,
            size: "M4".into(),
            engagement_mm: 7.5,
            at: None,
            id_prefix: Some("st_m4".into()),
            host_id: Some("boss_plate".into()),
            fastener_material: "ti64".into(),
            substrate_material: Some("petg".into()),
            vibration_g: 10.0,
            payload_mass_kg: 0.35,
            ..Default::default()
        },
    );
    let analysis = fieldsmith_core::analyse(&doc, fieldsmith_core::Resolution::Size(4.0), || 0.0)
        .expect("3D P2 FEA on self-tapping fixing");
    let case = &analysis.cases[0];
    assert!(case.max_displacement > 0.0 && case.max_displacement.is_finite());
    assert!(case.max_von_mises > 0.0 && case.max_von_mises.is_finite());
    let max_sigma1 = case
        .stress
        .iter()
        .map(|s| fieldsmith_solve::principal_stresses(s)[2])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(max_sigma1 > 0.0 && max_sigma1.is_finite());
}

#[test]
fn v57_plastic_vs_metal_vs_jwst_composite_torque_envelope_physics() {
    let base_input = FixingEnvelopeInput {
        fixing_kind: FixingKind::SelfTappingScrew,
        nominal_d_mm: 4.0,
        pitch_mm: 0.70,
        engagement_length_mm: 8.0,
        boss_od_mm: 10.0,
        substrate_material: "petg".into(),
        fastener_material: "ti64".into(),
        vibration_g: 10.0,
        shock_g: 25.0,
        payload_mass_kg: 0.40,
        num_fixings: 1,
        delta_temp_c: 0.0,
        service_hours: 1000.0,
        target_failure_prob: 1.0e-3,
        mu_thread: 0.16,
        mu_head: 0.18,
        mu_joint: 0.25,
        weibull_m_override: None,
        weibull_sigma0_override_mpa: None,
        ..Default::default()
    };

    let plastic_env = evaluate_fixing_torque_envelope(&base_input);
    assert_eq!(plastic_env.substrate_regime, SubstrateRegime::PlasticBoss);
    assert!(plastic_env.creep_retention_factor < 0.90);

    // Metal housing (AlSi10Mg) must automatically select the MetalHousing regime and yield a much higher T_max:
    let metal_env = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "alsi10mg".into(),
        ..base_input.clone()
    });
    assert_eq!(metal_env.substrate_regime, SubstrateRegime::MetalHousing);
    assert!(
        metal_env.max_torque_nm > 2.2 * plastic_env.max_torque_nm,
        "Metal AlSi10Mg T_max ({:.2} N·m) must exceed Plastic PETG T_max ({:.2} N·m)",
        metal_env.max_torque_nm,
        plastic_env.max_torque_nm
    );

    // Increasing knock/vibration g-load must monotonically increase Minimal Torque T_min:
    let high_vib_env = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        vibration_g: 28.0,
        shock_g: 70.0,
        ..base_input.clone()
    });
    assert!(
        high_vib_env.min_torque_nm > plastic_env.min_torque_nm * 1.4,
        "Higher vibration g-load must increase T_min ({:.3} vs {:.3} N·m)",
        high_vib_env.min_torque_nm,
        plastic_env.min_torque_nm
    );

    // JWST spaceflight reliability (P_f = 1e-6) on Brittle SiC ceramic must tighten Weibull T_max vs P_f = 1e-2:
    let jwst_loose = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "sic_jwst".into(),
        target_failure_prob: 1.0e-2,
        ..base_input.clone()
    });
    let jwst_strict = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "sic_jwst".into(),
        target_failure_prob: 1.0e-6,
        ..base_input.clone()
    });
    assert_eq!(
        jwst_strict.substrate_regime,
        SubstrateRegime::BrittleWeibullOptics
    );
    assert!(
        jwst_strict.max_torque_nm < jwst_loose.max_torque_nm * 0.65,
        "JWST 10⁻⁶ fracture reliability must tighten T_max ({:.2} vs {:.2} N·m)",
        jwst_strict.max_torque_nm,
        jwst_loose.max_torque_nm
    );

    // Anisotropic 3K Woven CFRP composite regime:
    let cfrp_env = evaluate_fixing_torque_envelope(&FixingEnvelopeInput {
        substrate_material: "cf_woven_3k".into(),
        ..base_input
    });
    assert_eq!(cfrp_env.substrate_regime, SubstrateRegime::WovenComposite);
    assert!(cfrp_env.max_torque_nm > cfrp_env.min_torque_nm);
}

#[test]
fn v57_weibull_failure_spreadsheet_regression_recovers_true_parameters() {
    // Generate synthetic coupon samples from a true 2-parameter Weibull law (m = 8.5, σ_0 = 64.0 MPa, T_0 = 1.92 N·m):
    let true_m = 8.5_f64;
    let true_sigma0 = 64.0_f64;
    let true_t0 = 1.92_f64;
    let pfs = [0.05, 0.12, 0.25, 0.40, 0.55, 0.70, 0.85, 0.94];

    let rows: Vec<FailureSampleRow> = pfs
        .iter()
        .enumerate()
        .map(|(i, &pf)| {
            let factor = (-(1.0_f64 - pf).ln()).powf(1.0 / true_m);
            FailureSampleRow {
                sample_id: format!("C-{}", i + 1),
                torque_nm: true_t0 * factor,
                principal_stress_mpa: true_sigma0 * factor,
                failure_prob: Some(pf),
                note: "Boss radial split coupon".into(),
            }
        })
        .collect();

    let fit = fit_failure_distribution(&rows, 1.0e-3);
    assert!(
        (fit.weibull_modulus_m - true_m).abs() < 1.0e-4,
        "Expected m = {true_m}, got {}",
        fit.weibull_modulus_m
    );
    assert!(
        (fit.weibull_scale_sigma0_mpa - true_sigma0).abs() < 1.0e-3,
        "Expected σ_0 = {true_sigma0}, got {}",
        fit.weibull_scale_sigma0_mpa
    );
    assert!(
        (fit.weibull_scale_t0_nm - true_t0).abs() < 1.0e-4,
        "Expected T_0 = {true_t0}, got {}",
        fit.weibull_scale_t0_nm
    );
    assert!(fit.weibull_r2 > 0.9999);
}

#[test]
fn v57_monolithic_72dof_fastener_system_and_mcp_roundtrip() {
    let sys = FastenerFixingSystem::new();
    let cfg = EngineConfig::default();
    let p0 = sys.nominal_design_vector();
    let theta0 = sys.nominal_epistemic_vector();

    let res = MonolithicSystemEngine::solve_and_analyze(&sys, &cfg, &p0, &theta0)
        .expect("solve 72-DOF FastenerFixingSystem");
    assert_eq!(res.state.len(), 72);
    assert!(res.adjoint_vs_tangent_max_rel_err < 1.0e-4);
    assert!(res.conservation_audits.iter().all(|a| a.passed()));

    // Verify MCP `insert_fixing` + `solve_coupled_system` round-trip:
    let mut srv = Server::new();
    let resp = srv
        .handle(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "insert_fixing",
                    "arguments": {
                        "kind": "self_tapping_screw",
                        "size": "M4",
                        "engagement_mm": 8.0,
                        "vibration_g": 14.0
                    }
                }
            })
            .to_string(),
        )
        .expect("insert_fixing response");
    let v: Value = serde_json::from_str(&resp).unwrap();
    let payload: Value =
        serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["problems"].as_array().unwrap().len(), 0);
    assert!(
        payload["fixing"]["envelope"]["max_torque_nm"]
            .as_f64()
            .unwrap()
            > 0.5
    );

    let coupled_resp = srv
        .handle(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "solve_coupled_system",
                    "arguments": {
                        "system_id": "jwst_fastener_fixing_envelope_v1",
                        "sync_cad": true
                    }
                }
            })
            .to_string(),
        )
        .expect("solve_coupled_system response");
    let cv: Value = serde_json::from_str(&coupled_resp).unwrap();
    let cpayload: Value =
        serde_json::from_str(cv["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(cpayload["synced_to_cad_doc"], true);
}

#[test]
fn v57_open_standards_quick_lookup_vs_apriori_rederivation_benchmarks() {
    let inp = FixingEnvelopeInput {
        fixing_kind: FixingKind::HexCapBolt,
        nominal_d_mm: 4.0,
        pitch_mm: 0.7,
        engagement_length_mm: 8.0,
        boss_od_mm: 10.0,
        substrate_material: "alsi10mg".into(),
        fastener_material: "ti64".into(),
        mu_thread: 0.14,
        mu_head: 0.15,
        mu_joint: 0.28,
        vibration_g: 15.0,
        shock_g: 40.0,
        payload_mass_kg: 0.25,
        num_fixings: 1,
        delta_temp_c: 0.0,
        service_hours: 1000.0,
        target_failure_prob: 1.0e-4,
        weibull_m_override: None,
        weibull_sigma0_override_mpa: None,
        ..Default::default()
    };

    let benchmarks = evaluate_open_standard_benchmarks(&inp);
    assert_eq!(benchmarks.len(), 4);

    // 1. FED-STD-H28 §2B / ISO 898-1 Metric 60° Thread Stress Area A_s:
    //    (π/4)*(d - 0.938194*P)^2 vs 13√3/24 fundamental 60° triangle derivation
    let b_h28 = &benchmarks[0];
    assert_eq!(b_h28.id, "fed_std_h28_stress_area");
    assert!((b_h28.standard_value - 8.78).abs() < 0.01);
    assert!(
        b_h28.discrepancy_pct < 1.0e-3,
        "Expected exact geometric agreement (< 0.001%), got {}%",
        b_h28.discrepancy_pct
    );

    // 2. NASA-STD-5020B §4.2 / MIL-HDBK-60 Short-Form K*d*F vs Motosh 3-Surface Helical Wedge Integral:
    let b_5020 = &benchmarks[1];
    assert_eq!(b_5020.id, "nasa_std_5020b_torque_preload");
    assert!(
        b_5020.discrepancy_pct < 2.5,
        "Expected Motosh integral vs NASA-STD-5020B K=0.193 within 2.5%, got {}%",
        b_5020.discrepancy_pct
    );

    // 3. NIST/SEMATECH e-Handbook §8.1.6.2 Weibull Parameter Recovery:
    let b_nist = &benchmarks[2];
    assert_eq!(b_nist.id, "nist_sematech_8162_weibull");
    assert!(
        b_nist.discrepancy_pct < 0.5,
        "Expected NIST Weibull scale recovery within 0.5%, got {}%",
        b_nist.discrepancy_pct
    );

    // 4. NASA-HDBK-7005 / CARES & Sopwith First-Thread Load Concentration Y_1:
    let b_sopwith = &benchmarks[3];
    assert_eq!(b_sopwith.id, "nasa_hdbk_7005_sopwith_thread_load");
    assert!(b_sopwith.apriori_value > 1.8 && b_sopwith.apriori_value < 3.2);
}
